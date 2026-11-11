import errno
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import Mock, patch

import smoke_engine


FAKE_ENGINE = '''
import json
from pathlib import Path
import sys
import time

mode, log = sys.argv[1:]
opening = False
depth = 0
for raw in sys.stdin:
    line = raw.strip()
    with Path(log).open("a") as out:
        out.write(json.dumps(line) + "\\n")
    if line == "uci":
        if mode == "eof":
            sys.exit(0)
        print("id name Bitfox " + ("2.5.0" if mode == "version" else "2.6.0"))
        print("option name Hash type spin default 64 min 1 max 4096")
        if mode != "options":
            print("option name Threads type spin default 1 min 1 max 256")
        print("uciok", flush=True)
    elif line == "isready":
        print("readyok", flush=True)
    elif line.startswith("position"):
        opening = " moves " in line
    elif line.startswith("go "):
        if mode == "noise":
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline:
                print("info string waiting", flush=True)
                time.sleep(0.01)
            continue
        depth = int(line.split()[2]) if line.startswith("go depth") else 2
        pv = "g8f6 d2d4" if opening else "e2e4 e7e5"
        best = pv.split()[0]
        if mode == "pv":
            pv = "e2e4 e2e3"
        if mode == "best":
            best = "a1a8"
        if mode == "empty":
            pv = ""
        if mode == "null":
            best = "0000"
        if mode == "shallow":
            depth = 1
        if mode != "no-search":
            print(f"info depth {depth} nodes 123 time 1 pv {pv}", flush=True)
        print("bestmove " + best, flush=True)
    elif line == "quit":
        if mode == "quit":
            time.sleep(3)
        sys.exit(3 if mode == "status" else 0)
'''


class EngineSmokeTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.fake = self.root / "fake engine.py"
        self.fake.write_text(FAKE_ENGINE)
        self.log = self.root / "commands.jsonl"

    def command(self, mode="ok"):
        return [sys.executable, "-u", str(self.fake), mode, str(self.log)]

    def commands(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_depth_and_timed_searches_use_both_thread_counts_and_wait_for_readiness(self):
        results = smoke_engine.smoke(self.command(), "2.6.0", timeout=2)
        self.assertEqual([(v["threads"], v["limit"]) for v in results],
                         [(1, "depth 6"), (1, "movetime 200"),
                          (4, "depth 6"), (4, "movetime 200")])
        commands = self.commands()
        self.assertEqual(commands[0], "uci")
        self.assertIn("setoption name Hash value 16", commands)
        self.assertEqual(commands[-1], "quit")
        self.assertEqual(commands.count("quit"), 1)
        for index, command in enumerate(commands):
            if command.startswith("go "):
                self.assertEqual(commands[index - 1], "isready")
                self.assertTrue(commands[index - 2].startswith("position startpos"))
        self.assertEqual([v["bestmove"] for v in results], ["e2e4", "g8f6", "e2e4", "g8f6"])

    def test_invalid_pv_bestmove_and_null_move_fail(self):
        for mode in ("pv", "best", "null", "empty"):
            with self.subTest(mode=mode):
                with self.assertRaisesRegex(RuntimeError, "illegal move|empty PV"):
                    smoke_engine.smoke(self.command(mode), "2.6.0", timeout=2)

    def test_bestmove_requires_search_evidence_and_requested_depth(self):
        for mode, message in [("no-search", "without searched nodes"),
                              ("shallow", "requested depth")]:
            with self.subTest(mode=mode):
                with self.assertRaisesRegex(RuntimeError, message):
                    smoke_engine.smoke(self.command(mode), "2.6.0", timeout=2)

    def test_handshake_rejects_wrong_version_missing_options_and_early_eof(self):
        for mode, message in [("version", "identity"), ("options", "Threads"),
                              ("eof", "closed its output")]:
            with self.subTest(mode=mode):
                with self.assertRaisesRegex(RuntimeError, message):
                    smoke_engine.smoke(self.command(mode), "2.6.0", timeout=2)

    def test_dead_pipe_cleanup_preserves_early_eof_failure(self):
        original = smoke_engine.UciProcess
        processes = []

        for code in (errno.EPIPE, errno.EINVAL):
            with self.subTest(errno=code):
                def start(*args, **kwargs):
                    engine = original(*args, **kwargs)
                    stream = engine.process.stdin

                    def failed_close():
                        try:
                            stream.close()
                        finally:
                            raise OSError(code, "Closed pipe")

                    engine.process.stdin = Mock(wraps=stream)
                    engine.process.stdin.close.side_effect = failed_close
                    processes.append(engine)
                    return engine

                with patch.object(smoke_engine, "UciProcess", side_effect=start):
                    with self.assertRaisesRegex(RuntimeError, "closed its output"):
                        smoke_engine.smoke(self.command("eof"), "2.6.0", timeout=2)
                self.assertIsNotNone(processes[-1].process.poll())
                self.assertTrue(processes[-1].process.stdout.closed)

    def test_cleanup_does_not_hide_other_errors_or_a_live_child(self):
        for code, exit_status in ((errno.EIO, 0), (errno.EINVAL, None), (errno.EPIPE, None)):
            with self.subTest(errno=code, exit_status=exit_status):
                engine = smoke_engine.UciProcess.__new__(smoke_engine.UciProcess)
                engine.timeout = 1
                engine.process = Mock()
                engine.process.poll.return_value = exit_status
                engine.process.stdin.close.side_effect = OSError(code, "Close failed")
                engine.reader = Mock()
                engine.reader.is_alive.return_value = False
                with self.assertRaises(OSError) as caught:
                    engine.close()
                self.assertEqual(caught.exception.errno, code)
                engine.process.stdout.close.assert_called_once()

    def test_output_does_not_extend_deadline_and_only_owned_process_is_stopped(self):
        original = smoke_engine.subprocess.Popen
        processes = []

        def start(*args, **kwargs):
            process = original(*args, **kwargs)
            processes.append(process)
            return process

        started = time.monotonic()
        with patch.object(smoke_engine.subprocess, "Popen", side_effect=start):
            with self.assertRaisesRegex(RuntimeError, "deadline exceeded"):
                smoke_engine.smoke(self.command("noise"), "2.6.0", timeout=0.3)
        self.assertLess(time.monotonic() - started, 2)
        self.assertEqual(len(processes), 1)
        self.assertIsNotNone(processes[0].poll())

    def test_quit_requires_timely_successful_exit(self):
        for mode, message in [("quit", "did not exit after quit"), ("status", "status 3")]:
            with self.subTest(mode=mode):
                with self.assertRaisesRegex(RuntimeError, message):
                    smoke_engine.smoke(self.command(mode), "2.6.0", timeout=0.3)

    def test_invalid_timeout_is_rejected_before_starting_a_process(self):
        for timeout in (0, -1, float("nan"), float("inf")):
            with self.subTest(timeout=timeout), patch.object(smoke_engine, "UciProcess") as start:
                with self.assertRaisesRegex(ValueError, "parameters"):
                    smoke_engine.smoke(self.command(), "2.6.0", timeout=timeout)
                start.assert_not_called()

    def test_cli_preserves_launcher_arguments_and_paths_with_spaces(self):
        result = subprocess.run([sys.executable, str(Path(smoke_engine.__file__).resolve()),
                                 "--version", "2.6.0", "--", *self.command()],
                                text=True, capture_output=True, timeout=5, check=True)
        self.assertEqual(len([json.loads(line) for line in result.stdout.splitlines()]), 4)


if __name__ == "__main__":
    unittest.main()
