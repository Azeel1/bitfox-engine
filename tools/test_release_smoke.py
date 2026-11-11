import io
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile


ROOT = Path(__file__).resolve().parents[1]


class ReleaseSmokeTests(unittest.TestCase):
    def inspect(self, links, files, root="Bitfox.app", required=(), kind="zip"):
        script = (ROOT / "tools" / "release-smoke.sh").read_text()
        marker = 'python3 - "$kind" "${work}/${archive}" "$root" "$@" <<\'PY\'\n'
        inspector = script.split(marker, 1)[1].split("\nPY\n}", 1)[0]
        with tempfile.TemporaryDirectory() as tmp:
            archive = Path(tmp) / ("fixture.zip" if kind == "zip" else "fixture.tar.gz")
            if kind == "zip":
                with zipfile.ZipFile(archive, "w") as output:
                    for name in files:
                        output.writestr(root + "/" + name, b"fixture")
                    for name, target in links.items():
                        entry = zipfile.ZipInfo(root + "/" + name)
                        entry.create_system = 3
                        entry.external_attr = (stat.S_IFLNK | 0o777) << 16
                        output.writestr(entry, target)
            else:
                self.assertEqual(kind, "tar")
                self.assertFalse(links)
                with tarfile.open(archive, "w:gz") as output:
                    for name in files:
                        entry = tarfile.TarInfo(root + "/" + name)
                        entry.size = len(b"fixture")
                        output.addfile(entry, io.BytesIO(b"fixture"))
            return subprocess.run(
                [sys.executable, "-c", inspector, kind, str(archive), root, *required],
                capture_output=True,
                text=True,
                timeout=10,
                env=dict(os.environ, BITFOX_RELEASE_TOOLS=str(ROOT / "tools")),
            )

    def test_qt_framework_links_resolve_through_versions_current(self):
        result = self.inspect(
            {
                "Contents/Frameworks/QtCore.framework/Versions/Current": "A",
                "Contents/Frameworks/QtCore.framework/QtCore": "Versions/Current/QtCore",
                "Contents/Frameworks/QtCore.framework/Resources": "Versions/Current/Resources",
            },
            [
                "Contents/Frameworks/QtCore.framework/Versions/A/QtCore",
                "Contents/Frameworks/QtCore.framework/Versions/A/Resources/Info.plist",
            ],
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_missing_cyclic_and_escaping_link_targets_are_rejected(self):
        cases = [
            ({"link": "missing"}, "missing target"),
            ({"link": "other", "other": "link"}, "cyclic symlink"),
            ({"link": "../outside"}, "escapes the package"),
        ]
        for links, expected in cases:
            with self.subTest(links=links):
                result = self.inspect(links, ["regular"])
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(expected, result.stderr)

    def test_link_resolution_has_a_finite_depth(self):
        links = {f"link{index}": f"link{index + 1}" for index in range(50)}
        result = self.inspect(links, ["link50"])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("symlink chain is too long", result.stderr)

    def test_engine_archive_rejects_unexpected_files(self):
        root = "bitfox-2.6.0-windows-x86_64"
        files = ["LICENSE", "bitfox.exe", "bitfox-avx2.exe"]
        required = [root + "/" + name for name in files]
        result = self.inspect({}, files, root, required)
        self.assertEqual(result.returncode, 0, result.stderr)
        result = self.inspect({}, files + ["private-notes.txt"], root, required)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unexpected engine member", result.stderr)

    def test_windows_archive_accepts_approved_runtime_files_and_requires_inventory(self):
        root = "bitfox-2.6.0-windows-x86_64"
        files = ["LICENSE", "THIRD_PARTY_NOTICES.txt", "bitfox.exe", "bitfox-avx2.exe",
                 "RUNTIME_DEPENDENCIES.json", "licenses/msvc-2022.txt"]
        required = [root + "/" + name for name in files]
        result = self.inspect({}, files + ["msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll"],
                              root, required)
        self.assertEqual(result.returncode, 0, result.stderr)
        result = self.inspect({}, files + ["unknown.dll"], root, required)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unexpected engine member", result.stderr)
        result = self.inspect({}, [name for name in files if name != "RUNTIME_DEPENDENCIES.json"],
                              root, required)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("required regular file", result.stderr)

    @unittest.skipUnless(os.name == "posix" and shutil.which("bash"),
                         "Release smoke shell fixture requires POSIX Bash")
    def test_both_windows_runtime_checks_fail_before_engine_execution(self):
        script = (ROOT / "tools/release-smoke.sh").read_text()
        start = script.index('for bundle in "${work}/bitfox-${version}-windows-x86_64"')
        end = script.index("\ndone", start) + len("\ndone")
        fragment = script[start:end]
        for failure, expected in [("none", ["engine", "gui", "launch"]),
                                  ("engine", ["engine"]), ("gui", ["engine", "gui"])]:
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as tmp:
                trace = Path(tmp) / "calls"
                prelude = '''set -e
work=/fixture
repo_root=/repo
version=2.6.0
python3() {
    case "$*" in
        *bitfox-2.6.0-windows-x86_64*) label=engine;;
        *gui-windows/Bitfox*) label=gui;;
        *) return 92;;
    esac
    printf '%s\\n' "$label" >> "$BITFOX_TRACE"
    if [ "$label" = "$BITFOX_FAIL" ]; then return 23; fi
}
'''
                result = subprocess.run(
                    ["bash", "-c", prelude + fragment + '\nprintf "launch\\n" >> "$BITFOX_TRACE"\n'],
                    env=dict(os.environ, BITFOX_TRACE=str(trace), BITFOX_FAIL=failure),
                    capture_output=True, text=True, timeout=10,
                )
                self.assertEqual(trace.read_text().splitlines(), expected)
                self.assertEqual(result.returncode == 0, failure == "none", result.stderr)

    def test_private_artifacts_are_rejected_inside_gui_dependencies(self):
        for name in (".env", ".env.production", ".git/config",
                     "signing.p12", "upload.keystore", "debug.log",
                     "profile.profraw", "network.nnue"):
            with self.subTest(name=name):
                result = self.inspect({}, ["Contents/Frameworks/" + name])
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("private development member", result.stderr)

    def test_markdown_notes_are_rejected_in_binary_archives(self):
        for kind in ("zip", "tar"):
            for name in ("private-notes.md", "review.MARKDOWN", "notes.Md"):
                with self.subTest(kind=kind, name=name):
                    result = self.inspect({}, ["Contents/Frameworks/" + name], kind=kind)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("private development member", result.stderr)

    def test_gui_license_notices_and_qt_resources_are_accepted(self):
        for kind, root, prefix in (
            ("zip", "Bitfox.app", "Contents/Resources/"),
            ("zip", "Bitfox", ""),
            ("tar", "Bitfox", ""),
        ):
            with self.subTest(kind=kind, root=root):
                files = [prefix + name for name in (
                    "LICENSE", "THIRD_PARTY_NOTICES.txt", "GUI_DEPENDENCIES.json",
                    "licenses/qtbase-6.11.0.txt", "licenses/libc6-copyright.txt",
                )]
                if prefix:
                    files.append("Contents/Frameworks/QtCore.framework/Versions/A/Resources/Info.plist")
                required = [root + "/" + name for name in files]
                result = self.inspect({}, files, root, required, kind)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_required_members_cannot_be_directories_or_symlinks(self):
        required = ["Bitfox.app/Contents/MacOS/bitfox-board"]
        for links, files in [
            ({}, ["Contents/MacOS/bitfox-board/"]),
            ({"Contents/MacOS/bitfox-board": "other"}, ["Contents/MacOS/other"]),
        ]:
            with self.subTest(links=links, files=files):
                result = self.inspect(links, files, required=required)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("required regular file", result.stderr)

    def test_gui_archive_requires_a_regular_dependency_manifest(self):
        for root, manifest in [
            ("Bitfox", "GUI_DEPENDENCIES.json"),
            ("Bitfox.app", "Contents/Resources/GUI_DEPENDENCIES.json"),
        ]:
            with self.subTest(root=root):
                required = [root + "/" + manifest]
                result = self.inspect({}, ["LICENSE"], root, required)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("required regular file", result.stderr)
                result = self.inspect({}, [manifest], root, required)
                self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
