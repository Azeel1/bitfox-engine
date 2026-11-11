import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import bitfox
import bitfox_project


class ProjectCommandTests(unittest.TestCase):
    def fixture(self, root):
        (root / "engine/.cargo").mkdir(parents=True)
        (root / "engine/Cargo.toml").write_text('[package]\nname="bitfox"\nversion="2.6.0"\n')
        (root / "rust-toolchain.toml").write_text('[toolchain]\nchannel="1.96.0"\n')
        (root / "engine/.cargo/config.toml").write_text('[target.x86_64-pc-windows-msvc]\nrustflags=["-Ctarget-cpu=x86-64-v2"]\n')

    def test_engine_build_uses_engine_cwd_and_pinned_toolchain(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            self.fixture(root)
            command = bitfox_project.cargo_command("build", root=root, profile="release")
            self.assertEqual(command["cwd"], str(root / "engine"))
            self.assertEqual(command["argv"], ["cargo", "+1.96.0", "build", "--locked", "--release"])
            self.assertEqual(bitfox_project.project_version(root), "2.6.0")

    def test_datagen_and_separate_training_packages_use_correct_profiles(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            self.fixture(root)
            run = bitfox_project.cargo_command("run", root=root, profile="datagen", extra=["--", "datagen", "100"])
            self.assertIn("datagen", run["argv"])
            self.assertNotIn("--release", run["argv"])
            convert = bitfox_project.cargo_command("run", root=root, package="convert", profile="release")
            self.assertEqual(convert["cwd"], str(root / "trainer/convert"))
            train = bitfox_project.cargo_command("run", root=root, package="trainer", profile="release")
            self.assertEqual(train["cwd"], str(root / "trainer"))

    def test_cli_dispatch_preserves_training_arguments(self):
        values = ["/tmp/path with spaces/net", "--positions", "1000", "--skip-match"]
        for command, script in [("train", "trainer/experiment.py"), ("check", "tools/check_project.py")]:
            result = bitfox.specification(command, values)
            self.assertEqual(result[0]["argv"][2], str(bitfox.ROOT / script))
            self.assertEqual(result[0]["argv"][3:], values)

    def test_cli_dry_run_never_executes_a_process_or_changes_cwd(self):
        with mock.patch.object(bitfox.os, "execvp") as execute, \
             mock.patch.object(bitfox.os, "chdir") as chdir, \
             mock.patch("sys.stdout", new_callable=io.StringIO) as output:
            self.assertEqual(bitfox.main(["--dry-run", "build", "--target", "x86_64-pc-windows-msvc"]), 0)
            result = json.loads(output.getvalue())
        execute.assert_not_called()
        chdir.assert_not_called()
        self.assertIn("x86_64-pc-windows-msvc", result["commands"][0]["argv"])
        self.assertEqual(result["commands"][0]["cwd"], str(bitfox.ROOT / "engine"))

    def test_execution_replaces_wrapper_and_preserves_child_exit_semantics(self):
        with mock.patch.object(bitfox.os, "execvp", side_effect=SystemExit(7)) as execute, \
             mock.patch.object(bitfox.os, "chdir") as chdir:
            with self.assertRaises(SystemExit) as error:
                bitfox.main(["test", "--all-targets", "--all-features"])
        self.assertEqual(error.exception.code, 7)
        chdir.assert_called_once_with(str(bitfox.ROOT / "engine"))
        command = execute.call_args.args[1]
        self.assertIn("release-test", command)
        self.assertIn("--all-targets", command)
        self.assertIn("--all-features", command)

    def test_gui_configuration_does_not_start_a_game_or_install_dependencies(self):
        command = bitfox.specification("gui", ["configure", "--", "-DCMAKE_PREFIX_PATH=/tmp/Qt SDK"])[0]
        self.assertEqual(command["argv"][0], "cmake")
        self.assertIn("-DBITFOX_STRICT_WARNINGS=ON", command["argv"])
        self.assertIn("-DBUILD_TESTING=ON", command["argv"])
        self.assertEqual(command["argv"][-1], "-DCMAKE_PREFIX_PATH=/tmp/Qt SDK")
        self.assertIn(str(bitfox.ROOT / "build/gui"), command["argv"])

    def test_unpinned_toolchain_and_unknown_package_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.fixture(root)
            (root / "rust-toolchain.toml").write_text('[toolchain]\nchannel="stable"\n')
            with self.assertRaisesRegex(ValueError, "pin"):
                bitfox_project.cargo_command("build", root=root)
            with self.assertRaisesRegex(ValueError, "unsupported"):
                bitfox_project.cargo_command("build", root=root, package="../other")


if __name__ == "__main__":
    unittest.main()
