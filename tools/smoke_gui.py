import argparse
import math
import os
from pathlib import Path
import signal
import subprocess
import tempfile


def smoke(binary, seconds=5, platform="offscreen", plugin_dir=None):
    if not math.isfinite(seconds) or seconds <= 0:
        raise ValueError("Smoke duration must be finite and positive")
    environment = os.environ.copy()
    environment["QT_QPA_PLATFORM"] = platform
    if plugin_dir is not None:
        plugins = Path(plugin_dir).resolve()
        if not (plugins / "platforms").is_dir():
            raise ValueError("Packaged platform plugin directory is missing")
        environment["QT_PLUGIN_PATH"] = str(plugins)
        environment["QT_QPA_PLATFORM_PLUGIN_PATH"] = str(plugins / "platforms")
    with tempfile.TemporaryFile(mode="w+") as output:
        process = subprocess.Popen(
            [str(Path(binary).resolve())],
            cwd=Path(binary).resolve().parent,
            env=environment,
            stdout=output,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        try:
            try:
                code = process.wait(timeout=seconds)
            except subprocess.TimeoutExpired:
                return
            output.seek(0)
            raise RuntimeError(
                f"Packaged GUI exited before {seconds}s, code {code}:\n"
                + output.read()[-8000:]
            )
        finally:
            if process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    if process.poll() is None:
                        try:
                            os.killpg(process.pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                    process.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description="Check packaged GUI startup on a POSIX CI host.")
    parser.add_argument("binary", type=Path)
    parser.add_argument("--platform", default="offscreen")
    parser.add_argument("--plugin-dir", type=Path)
    args = parser.parse_args()
    smoke(args.binary, platform=args.platform, plugin_dir=args.plugin_dir)
    print("Packaged GUI stayed alive for the smoke window")


if __name__ == "__main__":
    main()
