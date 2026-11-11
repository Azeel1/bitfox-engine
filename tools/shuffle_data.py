import argparse
import os
import signal
import shutil
import sys
import tempfile

import numpy as np

REC = 32


class TerminationRequested(Exception):
    def __init__(self, signum):
        super().__init__(f"terminated by signal {signum}")
        self.signum = signum


def request_termination(signum, _frame):
    raise TerminationRequested(signum)


def file_state(value):
    return (
        value.st_dev,
        value.st_ino,
        value.st_size,
        value.st_mtime_ns,
        value.st_ctime_ns,
    )


def close_files(files):
    error = None
    for fh in files:
        try:
            fh.close()
        except OSError as exc:
            if error is None:
                error = exc
    return error


def sync_parent_directory(path):
    if os.name != "posix":
        return
    parent = os.path.dirname(path) or "."
    flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0)
    descriptor = os.open(parent, flags)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def shuffle(output, inputs, bucket_count, seed, temporary_root=None):
    output = os.path.abspath(output)
    if bucket_count <= 0:
        raise ValueError("bucket count must be positive")
    if os.path.exists(output):
        raise FileExistsError(f"output already exists: {output}")
    input_paths = [os.path.abspath(path) for path in inputs]
    if output in input_paths:
        raise ValueError("output must differ from every input")
    input_states = {}
    for path in input_paths:
        if not os.path.isfile(path):
            raise FileNotFoundError(f"input does not exist: {path}")
        state = os.stat(path)
        if state.st_size % REC:
            raise ValueError(f"{path}: size not a multiple of {REC}")
        input_states[path] = file_state(state)

    output_parent = os.path.dirname(output)
    os.makedirs(output_parent, exist_ok=True)
    temporary_root = (
        os.path.abspath(temporary_root) if temporary_root else output_parent
    )
    if not os.path.isdir(temporary_root):
        raise FileNotFoundError(f"temporary directory does not exist: {temporary_root}")
    work_dir = tempfile.mkdtemp(prefix="bitfox-shuffle-", dir=temporary_root)
    temporary_output = None
    published_identity = None
    committed = False

    rng = np.random.default_rng(seed)
    paths = [os.path.join(work_dir, f"bucket-{i:04d}") for i in range(bucket_count)]
    buckets = []
    total = 0
    try:
        buckets = [open(path, "wb") for path in paths]
        for path in input_paths:
            with open(path, "rb") as fh:
                expected_state = input_states[path]
                if file_state(os.fstat(fh.fileno())) != expected_state:
                    raise ValueError(f"input changed before it was read: {path}")
                while True:
                    chunk = fh.read(REC * (1 << 20))
                    if not chunk:
                        break
                    rows = np.frombuffer(chunk, dtype=np.uint8).reshape(-1, REC)
                    assignment = rng.integers(0, bucket_count, len(rows))
                    for index in range(bucket_count):
                        part = rows[assignment == index]
                        if len(part):
                            buckets[index].write(part.tobytes())
                    total += len(rows)
                if file_state(os.fstat(fh.fileno())) != expected_state:
                    raise ValueError(f"input changed while it was read: {path}")
            if file_state(os.stat(path)) != expected_state:
                raise ValueError(f"input changed while it was read: {path}")
        close_error = close_files(buckets)
        buckets = []
        if close_error is not None:
            raise close_error

        with tempfile.NamedTemporaryFile(
            "wb",
            prefix=f".{os.path.basename(output)}.",
            suffix=".tmp",
            dir=output_parent,
            delete=False,
        ) as out:
            temporary_output = out.name
            for path in paths:
                rows = np.fromfile(path, dtype=np.uint8).reshape(-1, REC)
                out.write(rows[rng.permutation(len(rows))].tobytes())
            out.flush()
            os.fsync(out.fileno())
            info = os.fstat(out.fileno())
            published_identity = (info.st_dev, info.st_ino)
        os.link(temporary_output, output)
        sync_parent_directory(output)
        os.unlink(temporary_output)
        sync_parent_directory(output)
        committed = True
    finally:
        active_error = sys.exc_info()[0] is not None
        cleanup_error = close_files(buckets)
        if not committed and published_identity is not None:
            try:
                info = os.lstat(output)
                if (info.st_dev, info.st_ino) == published_identity:
                    os.unlink(output)
                    sync_parent_directory(output)
            except FileNotFoundError:
                pass
            except OSError as exc:
                if cleanup_error is None:
                    cleanup_error = exc
        try:
            if temporary_output is not None and os.path.exists(temporary_output):
                os.unlink(temporary_output)
        except OSError as exc:
            if cleanup_error is None:
                cleanup_error = exc
        try:
            shutil.rmtree(work_dir)
        except OSError as exc:
            if cleanup_error is None:
                cleanup_error = exc
        if cleanup_error is not None and not active_error:
            raise cleanup_error
    return total


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("output")
    ap.add_argument("inputs", nargs="+")
    ap.add_argument("--buckets", type=int, default=32)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--tmp-dir")
    args = ap.parse_args()

    previous_sigterm = signal.getsignal(signal.SIGTERM)
    previous_sigint = signal.getsignal(signal.SIGINT)
    signal.signal(signal.SIGTERM, request_termination)
    signal.signal(signal.SIGINT, request_termination)
    try:
        total = shuffle(
            args.output,
            args.inputs,
            args.buckets,
            args.seed,
            args.tmp_dir,
        )
    except TerminationRequested as exc:
        print(f"terminated by signal {exc.signum}", file=sys.stderr)
        return 128 + exc.signum
    except (OSError, ValueError) as exc:
        sys.exit(f"error: {exc}")
    finally:
        signal.signal(signal.SIGTERM, previous_sigterm)
        signal.signal(signal.SIGINT, previous_sigint)
    print(f"shuffled {total} records into {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
