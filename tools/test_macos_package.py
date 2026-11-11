import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


class MacosPackageTests(unittest.TestCase):
    def source_fixture(self, root):
        (root / "tools").mkdir(parents=True)
        (root / "engine").mkdir()
        (root / "engine/Cargo.toml").write_text(
            '[package]\nname = "bitfox"\nversion = "2.6.0"\n', encoding="utf-8")
        script = root / "tools/package-macos-gui.sh"
        shutil.copyfile(ROOT / "tools/package-macos-gui.sh", script)
        return script

    def check_source_root(self, script, cwd):
        environment = os.environ.copy()
        for name in ["BUILD_DIR", "ENGINE_BIN", "DIST_DIR", "GIT_DIR", "GIT_WORK_TREE",
                     "MACOSX_DEPLOYMENT_TARGET"]:
            environment.pop(name, None)
        result = subprocess.run(["bash", str(script)], cwd=cwd, env=environment,
                                capture_output=True, text=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        expected = script.parent.parent / "gui-qt/build-macos/bitfox-board"
        self.assertIn("missing build output: " + str(expected), result.stderr)
        self.assertNotIn("could not read package version", result.stderr)

    def test_archive_build_uses_its_source_from_an_unrelated_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory)
            script = self.source_fixture(parent / "source with spaces")
            self.check_source_root(script, parent)

    def test_nested_snapshot_does_not_use_the_outer_git_repository(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory)
            subprocess.run(["git", "init", "-q", str(parent)], check=True, capture_output=True)
            (parent / "engine").mkdir()
            (parent / "engine/Cargo.toml").write_text(
                '[package]\nname = "outer"\nversion = "9.9.9"\n', encoding="utf-8")
            script = self.source_fixture(parent / "private/snapshot")
            self.check_source_root(script, parent)

    @unittest.skipUnless(sys.platform == "darwin" and shutil.which("cmake"),
                         "requires macOS and CMake")
    def test_cmake_deployment_target_default_and_overrides(self):
        for environment_target, argument_target, expected in [
            (None, None, "14.0"),
            ("", "", "14.0"),
            ("13.0", None, "13.0"),
            ("13.0", "", "13.0"),
            ("13.0", "15.0", "15.0"),
        ]:
            with self.subTest(environment=environment_target, argument=argument_target), \
                    tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.source_fixture(root)
                source = root / "gui-qt"
                source.mkdir()
                shutil.copyfile(ROOT / "gui-qt/CMakeLists.txt", source / "CMakeLists.txt")
                hook = root / "capture-deployment.cmake"
                hook.write_text(
                    'file(WRITE "${CMAKE_BINARY_DIR}/deployment.txt" '
                    '"${CMAKE_OSX_DEPLOYMENT_TARGET}")\n'
                    'message(FATAL_ERROR "deployment target captured")\n',
                    encoding="utf-8")
                build = root / "build"
                environment = os.environ.copy()
                environment.pop("MACOSX_DEPLOYMENT_TARGET", None)
                if environment_target is not None:
                    environment["MACOSX_DEPLOYMENT_TARGET"] = environment_target
                command = ["cmake", "-S", str(source), "-B", str(build),
                           "-DCMAKE_PROJECT_INCLUDE_BEFORE=" + str(hook)]
                if argument_target is not None:
                    command.append("-DCMAKE_OSX_DEPLOYMENT_TARGET=" + argument_target)
                result = subprocess.run(command, capture_output=True, text=True,
                                        env=environment, timeout=15)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("deployment target captured", result.stderr)
                self.assertEqual((build / "deployment.txt").read_text(), expected)

    def query(self, helper_name, fixture):
        script = (ROOT / "tools" / "package-macos-gui.sh").read_text()
        start = helper_name + "() {"
        helper = start + script.split(start, 1)[1].split("\n}", 1)[0] + "\n}\n"
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory)
            output = temporary / "otool-output.txt"
            output.write_text(fixture)
            command = temporary / "otool"
            command.write_text('#!/bin/sh\ncat "$OTOOL_OUTPUT"\n')
            command.chmod(0o755)
            environment = os.environ.copy()
            environment["PATH"] = directory + os.pathsep + environment["PATH"]
            environment["OTOOL_OUTPUT"] = str(output)
            result = subprocess.run(
                ["bash", "-c", helper + helper_name + ' "fixture"'],
                capture_output=True,
                text=True,
                env=environment,
                timeout=10,
            )
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.splitlines()

    def test_universal_library_headers_are_not_dependencies(self):
        fixture = (
            "/Users/builder/QtCore (architecture x86_64):\n"
            "\t@rpath/QtCore.framework/Versions/A/QtCore "
            "(compatibility version 6.0.0, current version 6.11.0)\n"
            "\t/Users/builder/with space/libbitfox.dylib "
            "(compatibility version 0.0.0, current version 0.0.0)\n"
            "/Users/builder/QtCore (architecture arm64):\n"
            "\t@rpath/QtCore.framework/Versions/A/QtCore "
            "(compatibility version 6.0.0, current version 6.11.0)\n"
            "\t/usr/lib/libSystem.B.dylib "
            "(compatibility version 1.0.0, current version 1351.0.0)\n"
        )
        self.assertEqual(self.query("linked_libraries", fixture), [
            "/Users/builder/with space/libbitfox.dylib",
            "/usr/lib/libSystem.B.dylib",
            "@rpath/QtCore.framework/Versions/A/QtCore",
        ])

    def test_rpaths_keep_spaces_and_exclude_other_load_commands(self):
        fixture = (
            "Load command 1\n          cmd LC_RPATH\n      cmdsize 88\n"
            "         path /Users/builder/with space/release (offset 12)\n"
            "Load command 2\n          cmd LC_LOAD_DYLIB\n      cmdsize 64\n"
            "         name @rpath/QtCore (offset 24)\n"
            "Load command 3\n          cmd LC_RPATH\n      cmdsize 40\n"
            "         path @loader_path (offset 12)\n"
        )
        self.assertEqual(self.query("linked_rpaths", fixture), [
            "/Users/builder/with space/release",
            "@loader_path",
        ])


if __name__ == "__main__":
    unittest.main()
