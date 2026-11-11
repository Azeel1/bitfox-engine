import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

import shuffle_data


class ShuffleDataTests(unittest.TestCase):
    def test_failed_or_interrupted_publication_removes_the_final_output(self):
        for error in (OSError("directory sync failed"), shuffle_data.TerminationRequested(signal.SIGTERM)):
            with self.subTest(error=type(error).__name__), tempfile.TemporaryDirectory() as tmp:
                source = os.path.join(tmp, "source.data")
                output = os.path.join(tmp, "shuffled.data")
                with open(source, "wb") as fh:
                    fh.write(b"a" * shuffle_data.REC)
                with mock.patch.object(shuffle_data, "sync_parent_directory", side_effect=[error, None]):
                    with self.assertRaises(type(error)):
                        shuffle_data.shuffle(output, [source], 2, 3)
                self.assertFalse(os.path.lexists(output))
                self.assertEqual(os.listdir(tmp), ["source.data"])

    def test_preserves_records_and_refuses_to_replace_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            source = os.path.join(tmp, "source.data")
            output = os.path.join(tmp, "shuffled.data")
            records = [bytes([index]) * shuffle_data.REC for index in range(8)]
            with open(source, "wb") as fh:
                fh.write(b"".join(records))

            total = shuffle_data.shuffle(output, [source], 3, 17)
            self.assertEqual(total, len(records))
            with open(output, "rb") as fh:
                shuffled = fh.read()
            result = [
                shuffled[index : index + shuffle_data.REC]
                for index in range(0, len(shuffled), shuffle_data.REC)
            ]
            self.assertCountEqual(result, records)
            with self.assertRaises(FileExistsError):
                shuffle_data.shuffle(output, [source], 3, 17)

    def test_ignores_a_stale_pid_temporary_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            source = os.path.join(tmp, "source.data")
            output = os.path.join(tmp, "shuffled.data")
            stale = f"{output}.tmp.{os.getpid()}"
            with open(source, "wb") as fh:
                fh.write(b"a" * shuffle_data.REC)
            with open(stale, "wb") as fh:
                fh.write(b"stale")

            self.assertEqual(shuffle_data.shuffle(output, [source], 2, 3), 1)
            with open(stale, "rb") as fh:
                self.assertEqual(fh.read(), b"stale")

    def test_rejects_an_input_changed_while_reading_and_cleans_outputs(self):
        with tempfile.TemporaryDirectory() as tmp:
            source = os.path.join(tmp, "source.data")
            output = os.path.join(tmp, "shuffled.data")
            with open(source, "wb") as fh:
                fh.write(b"a" * (shuffle_data.REC * 2))
            initial_state = os.stat(source)

            original = shuffle_data.np.frombuffer
            changed = False

            def mutate(data, *args, **kwargs):
                nonlocal changed
                rows = original(data, *args, **kwargs)
                if not changed:
                    changed = True
                    with open(source, "r+b") as fh:
                        fh.write(b"b" * shuffle_data.REC)
                        fh.flush()
                        os.fsync(fh.fileno())
                    os.utime(source, ns=(initial_state.st_atime_ns,
                                         initial_state.st_mtime_ns + 2_000_000_000))
                    self.assertNotEqual(os.stat(source).st_mtime_ns,
                                        initial_state.st_mtime_ns)
                return rows

            with mock.patch.object(shuffle_data.np, "frombuffer", side_effect=mutate):
                with self.assertRaisesRegex(ValueError, "input changed while"):
                    shuffle_data.shuffle(output, [source], 2, 3)

            self.assertFalse(os.path.exists(output))
            leftovers = [
                name
                for name in os.listdir(tmp)
                if name.startswith("bitfox-shuffle-")
                or name.startswith(".shuffled.data.")
            ]
            self.assertEqual(leftovers, [])

    def test_bucket_close_failure_still_removes_the_work_directory(self):
        with tempfile.TemporaryDirectory() as tmp:
            source = os.path.join(tmp, "source.data")
            output = os.path.join(tmp, "shuffled.data")
            with open(source, "wb") as fh:
                fh.write(b"a" * shuffle_data.REC)

            real_open = open
            wrapped = False

            class FailingClose:
                def __init__(self, fh):
                    self.fh = fh

                def write(self, data):
                    return self.fh.write(data)

                def close(self):
                    self.fh.close()
                    raise OSError("bucket close failed")

            def open_file(path, mode="r", *args, **kwargs):
                nonlocal wrapped
                fh = real_open(path, mode, *args, **kwargs)
                if mode == "wb" and "bucket-" in os.path.basename(path) and not wrapped:
                    wrapped = True
                    return FailingClose(fh)
                return fh

            with mock.patch("builtins.open", side_effect=open_file):
                with self.assertRaisesRegex(OSError, "bucket close failed"):
                    shuffle_data.shuffle(output, [source], 2, 3)

            self.assertFalse(os.path.exists(output))
            self.assertFalse(
                any(name.startswith("bitfox-shuffle-") for name in os.listdir(tmp))
            )

    def test_syncs_the_directory_before_and_after_removing_the_temporary_link(self):
        with tempfile.TemporaryDirectory() as tmp:
            source = os.path.join(tmp, "source.data")
            output = os.path.join(tmp, "shuffled.data")
            with open(source, "wb") as fh:
                fh.write(b"a" * shuffle_data.REC)

            with mock.patch.object(
                shuffle_data, "sync_parent_directory"
            ) as sync_directory:
                shuffle_data.shuffle(output, [source], 2, 3)

            self.assertEqual(sync_directory.call_args_list, [mock.call(output)] * 2)

    @unittest.skipUnless(os.name == "posix", "POSIX signals are required")
    def test_sigterm_removes_working_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            source = os.path.join(tmp, "source.data")
            output = os.path.join(tmp, "shuffled.data")
            with open(source, "wb") as fh:
                fh.truncate(256 * 1024 * 1024)
            process = subprocess.Popen(
                [
                    sys.executable,
                    os.path.abspath(shuffle_data.__file__),
                    output,
                    source,
                    "--buckets",
                    "8",
                    "--tmp-dir",
                    tmp,
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            deadline = time.monotonic() + 5.0
            while time.monotonic() < deadline:
                if any(
                    name.startswith("bitfox-shuffle-") for name in os.listdir(tmp)
                ):
                    break
                if process.poll() is not None:
                    self.fail("shuffle process exited before the signal test")
                time.sleep(0.01)
            else:
                process.kill()
                process.wait(timeout=5)
                self.fail("shuffle process did not create its working directory")

            process.send_signal(signal.SIGTERM)
            stdout, stderr = process.communicate(timeout=10)
            self.assertEqual(process.returncode, 128 + signal.SIGTERM, stdout + stderr)
            self.assertFalse(os.path.exists(output))
            self.assertFalse(
                any(name.startswith("bitfox-shuffle-") for name in os.listdir(tmp))
            )


if __name__ == "__main__":
    unittest.main()
