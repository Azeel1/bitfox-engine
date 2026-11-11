import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from smoke_gui import smoke


@unittest.skipUnless(os.name == "posix", "POSIX CI smoke test")
class GuiSmokeTests(unittest.TestCase):
    def script(self, root, body):
        binary = Path(root) / "gui"
        binary.write_text(f"#!{sys.executable}\n" + body)
        binary.chmod(0o755)
        return binary

    def test_running_gui_passes_without_stopping_an_unrelated_process(self):
        unrelated = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
        try:
            with tempfile.TemporaryDirectory() as root:
                binary = self.script(root, "import time\ntime.sleep(60)\n")
                smoke(binary, seconds=0.15)
                self.assertIsNone(unrelated.poll())
        finally:
            unrelated.terminate()
            unrelated.wait(timeout=5)

    def test_clean_early_exit_is_not_successful_startup(self):
        with tempfile.TemporaryDirectory() as root:
            binary = self.script(root, "raise SystemExit(0)\n")
            with self.assertRaisesRegex(RuntimeError, "code 0"):
                smoke(binary, seconds=5)

    def test_exited_gui_process_group_is_not_signalled(self):
        with tempfile.TemporaryDirectory() as root:
            binary = self.script(root, "raise SystemExit(0)\n")
            with patch("smoke_gui.os.killpg") as send_signal:
                with self.assertRaisesRegex(RuntimeError, "code 0"):
                    smoke(binary, seconds=5)
                send_signal.assert_not_called()

    def test_crash_reports_output(self):
        with tempfile.TemporaryDirectory() as root:
            binary = self.script(root, "print('missing platform plugin', flush=True)\nraise SystemExit(3)\n")
            with self.assertRaisesRegex(RuntimeError, "missing platform plugin"):
                smoke(binary, seconds=5)

    def test_invalid_duration_is_rejected(self):
        for value in (0, -1, float("nan"), float("inf")):
            with self.assertRaises(ValueError):
                smoke(Path("does-not-exist"), seconds=value)

    def test_packaged_platform_and_plugin_paths_reach_process(self):
        with tempfile.TemporaryDirectory() as root:
            plugins = Path(root) / "PlugIns"
            (plugins / "platforms").mkdir(parents=True)
            record = Path(root) / "environment"
            binary = self.script(root, "import json, os, pathlib, time\n"
                f"pathlib.Path({str(record)!r}).write_text(json.dumps(dict(os.environ)))\n"
                "time.sleep(60)\n")
            smoke(binary, seconds=5, platform="cocoa", plugin_dir=plugins)
            environment = json.loads(record.read_text())
            self.assertEqual(environment["QT_QPA_PLATFORM"], "cocoa")
            self.assertEqual(environment["QT_PLUGIN_PATH"], str(plugins.resolve()))
            self.assertEqual(environment["QT_QPA_PLATFORM_PLUGIN_PATH"],
                             str((plugins / "platforms").resolve()))

    def test_missing_packaged_plugins_fail_before_launch(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaisesRegex(ValueError, "plugin directory is missing"):
                smoke(Path("does-not-exist"), plugin_dir=Path(root))


if __name__ == "__main__":
    unittest.main()
