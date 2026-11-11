import json
from contextlib import nullcontext
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import gui_dependency_notices as notices


class GuiDependencyNoticesTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.bundle = self.root / "Bitfox"
        self.bundle.mkdir()

    def file(self, name, contents=b"runtime"):
        path = self.bundle / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(contents)
        return path

    def windows(self, extra=()):
        for name in ["Qt6Core.dll", "Qt6Multimedia.dll", *extra]:
            self.file(name)
        return notices.collect_official(self.bundle, "windows", "6.8.3")

    def manifest(self):
        return self.bundle / "GUI_DEPENDENCIES.json"

    def test_windows_inventory_covers_svg_and_tuio_plugins(self):
        self.windows(["Qt6Svg.dll", "imageformats/qsvg.dll", "iconengines/qsvgicon.dll",
                      "generic/qtuiotouchplugin.dll"])
        data = notices.verify(self.bundle)
        self.assertEqual({v["provider"] for v in data["dependencies"]},
                         {"qtbase", "qtmultimedia", "qtsvg"})
        self.assertEqual(len(data["dependencies"]), 6)
        self.assertNotIn(str(self.root), self.manifest().read_text(encoding="utf-8"))

    def test_unapproved_libraries_and_optional_graphics_binaries_stop_packaging(self):
        for name in ["private.dll", "opengl32sw.dll", "D3Dcompiler_47.dll", "dxcompiler.dll",
                     "postproc-58.dll", "debug-helper.exe"]:
            with self.subTest(name=name):
                path = self.file(name)
                with self.assertRaisesRegex(ValueError, "no verified notice coverage"):
                    notices.collect_official(self.bundle, "windows", "6.8.3")
                path.unlink()

    def test_new_qt_versions_require_new_reviewed_notices(self):
        self.file("Qt6Core.dll")
        with self.assertRaisesRegex(ValueError, "no approved"):
            notices.collect_official(self.bundle, "windows", "6.8.4")
        self.assertFalse(self.manifest().exists())

    def test_notice_corruption_and_missing_files_are_rejected(self):
        self.windows()
        notice = self.bundle / notices.verify(self.bundle)["notices"][0]["path"]
        original = notice.read_bytes()
        notice.write_bytes(b"incomplete copyright")
        with self.assertRaisesRegex(ValueError, "checksum differs"):
            notices.verify(self.bundle)
        notice.write_bytes(original)
        notice.unlink()
        with self.assertRaisesRegex(ValueError, "missing required"):
            notices.verify(self.bundle)

    def test_manifest_cannot_omit_added_runtime_or_use_another_notice_provider(self):
        self.windows()
        path = self.file("Qt6Network.dll")
        with self.assertRaisesRegex(ValueError, "inventory is incomplete"):
            notices.verify(self.bundle)
        path.unlink()
        data = notices.verify(self.bundle)
        data["dependencies"][0]["provider"] = "qtmultimedia"
        self.manifest().write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, "incorrect notice provider"):
            notices.verify(self.bundle)

    def test_dependency_without_a_notice_and_unsafe_notice_paths_are_rejected(self):
        self.windows()
        data = notices.verify(self.bundle)
        data["notices"] = [v for v in data["notices"] if v["provider"] != "qtmultimedia"]
        self.manifest().write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, "missing its license notice"):
            notices.verify(self.bundle)
        data["notices"][0]["path"] = "../external-license.txt"
        self.manifest().write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, "invalid dependency path"):
            notices.verify(self.bundle)

    def test_ffmpeg_plugin_requires_runtime_and_its_separate_notice(self):
        self.file("Qt6Core.dll")
        self.file("Qt6Multimedia.dll")
        self.file("multimedia/ffmpegmediaplugin.dll")
        with self.assertRaisesRegex(ValueError, "missing its runtime"):
            notices.collect_official(self.bundle, "windows", "6.8.3")
        self.file("avutil-59.dll")
        with patch.object(notices, "ffmpeg_version", return_value="7.1"):
            notices.collect_official(self.bundle, "windows", "6.8.3")
        data = notices.verify(self.bundle)
        ffmpeg_notice = next(v for v in data["notices"] if v["provider"] == "ffmpeg")
        text = (self.bundle / ffmpeg_notice["path"]).read_text(encoding="utf-8")
        self.assertIn("Independent JPEG Group", text)
        self.assertIn("GNU LESSER GENERAL PUBLIC LICENSE", text)
        self.assertIn("ISC", text)
        self.assertIn("Mozilla Public License", text)

    def test_microsoft_installer_keeps_runtime_version_and_copyright(self):
        with patch.object(notices, "microsoft_metadata", return_value={
                "version": "14.44.35211.0", "copyright": "Copyright Microsoft Corporation"}):
            self.windows(["vc_redist.x64.exe"])
        data = notices.verify(self.bundle)
        runtime = next(v for v in data["dependencies"] if v["provider"] == "msvc")
        self.assertEqual(runtime["version"], "14.44.35211.0")
        self.assertIn("Microsoft", runtime["copyright"])
        self.assertTrue(any(v["provider"] == "msvc" for v in data["notices"]))

    def test_ffmpeg_version_and_license_are_checked_from_the_runtime(self):
        self.file("avutil-59.dll")
        for version, license_text, configuration in [
            (b"7.1", b"LGPL version 2.1 or later", b"--disable-programs"),
            (b"7.2", b"LGPL version 2.1 or later", b""),
            (b"7.1", b"GPL version 2 or later", b""),
            (b"7.1", b"LGPL version 2.1 or later", b"--enable-nonfree"),
        ]:
            with self.subTest(version=version, license=license_text, configuration=configuration):
                library = type("Library", (), {})()
                library.av_version_info = lambda: version
                library.avutil_license = lambda: license_text
                library.avutil_configuration = lambda: configuration
                with patch.object(notices.os, "add_dll_directory", return_value=nullcontext(), create=True), \
                        patch.object(notices.ctypes, "CDLL", return_value=library):
                    if version == b"7.1" and license_text.startswith(b"LGPL") and b"nonfree" not in configuration:
                        self.assertEqual(notices.ffmpeg_version(self.bundle), "7.1")
                    else:
                        with self.assertRaises(ValueError):
                            notices.ffmpeg_version(self.bundle)

    def test_notice_symlink_cannot_read_outside_the_bundle(self):
        self.windows()
        data = notices.verify(self.bundle)
        notice = self.bundle / data["notices"][0]["path"]
        external = self.root / "external.txt"
        external.write_bytes(notice.read_bytes())
        notice.unlink()
        notice.symlink_to(external)
        with self.assertRaisesRegex(ValueError, "missing required"):
            notices.verify(self.bundle)

    def test_macos_framework_symlinks_are_not_duplicate_dependencies(self):
        self.bundle = self.root / "Bitfox.app"
        self.bundle.mkdir()
        for module in ["Core", "Multimedia"]:
            base = f"Contents/Frameworks/Qt{module}.framework"
            self.file(f"{base}/Versions/A/Qt{module}")
            (self.bundle / f"{base}/Versions/Current").symlink_to("A")
            (self.bundle / f"{base}/Qt{module}").symlink_to(f"Versions/Current/Qt{module}")
        notices.collect_official(self.bundle, "macos", "6.11.0")
        data = notices.verify(self.bundle)
        self.assertEqual(len(data["dependencies"]), 2)
        self.assertTrue(all(v["integrity"] == "bundle-codesign" for v in data["dependencies"]))

    def test_non_qt_macos_framework_cannot_escape_notice_coverage(self):
        self.bundle = self.root / "Bitfox.app"
        self.bundle.mkdir()
        self.file("Contents/Frameworks/Vendor.framework/Versions/A/Vendor")
        with self.assertRaisesRegex(ValueError, "no verified notice coverage"):
            notices.collect_official(self.bundle, "macos", "6.11.0")

    def test_linux_copies_the_actual_package_copyright_and_referenced_license(self):
        runtime = self.file("lib/libQt6Core.so.6")
        docs = self.root / "doc"
        common = self.root / "common"
        (docs / "libqt6core6").mkdir(parents=True)
        common.mkdir()
        copyright = "Copyright Example author\nSee /usr/share/common-licenses/GPL-3\n"
        (docs / "libqt6core6/copyright").write_text(copyright)
        (common / "GPL-3").write_text("Complete GPL fixture")
        with patch.object(notices, "debian_owner", return_value=(
                "libqt6core6:amd64", "6.2.4-0ubuntu1+b1", "qt6-base", "6.2.4-0ubuntu1")):
            notices.collect_debian(self.bundle, [(str(runtime), "lib/libQt6Core.so.6")], docs, common)
        data = notices.verify(self.bundle)
        dependency = data["dependencies"][0]
        self.assertEqual(dependency["binary_package"], "libqt6core6:amd64")
        self.assertEqual(dependency["version"], "6.2.4-0ubuntu1+b1")
        self.assertEqual(dependency["source_package"], "qt6-base")
        self.assertEqual(dependency["source_version"], "6.2.4-0ubuntu1")
        self.assertNotIn(str(self.root), self.manifest().read_text(encoding="utf-8"))
        self.assertEqual((self.bundle / "licenses/libqt6core6-copyright.txt").read_text(encoding="utf-8"), copyright)
        self.assertTrue((self.bundle / "licenses/GPL-3.txt").is_file())
        for key in ("source_package", "source_version"):
            with self.subTest(missing=key):
                changed = json.loads(json.dumps(data))
                changed["dependencies"][0].pop(key)
                self.manifest().write_text(json.dumps(changed))
                with self.assertRaisesRegex(ValueError, "invalid Debian source metadata"):
                    notices.verify(self.bundle)

    def test_debian_owner_reads_the_exact_source_version_instead_of_binary_version(self):
        runtime = self.file("lib/libQt6Core.so.6")
        found = notices.subprocess.CompletedProcess(
            [], 0, stdout=f"libqt6core6:amd64: {runtime}\n", stderr="")
        with patch.object(notices.subprocess, "run", return_value=found), \
                patch.object(notices.subprocess, "check_output", return_value=(
                    "2:6.2.4+dfsg-2+b1\tqt6-base\t2:6.2.4+dfsg-2")) as query:
            self.assertEqual(notices.debian_owner(runtime), (
                "libqt6core6:amd64", "2:6.2.4+dfsg-2+b1", "qt6-base", "2:6.2.4+dfsg-2"))
        self.assertIn("${source:Package}", query.call_args.args[0][2])
        self.assertIn("${source:Version}", query.call_args.args[0][2])

    def test_debian_owner_rejects_missing_or_invalid_source_metadata(self):
        runtime = self.file("lib/libQt6Core.so.6")
        found = notices.subprocess.CompletedProcess(
            [], 0, stdout=f"libqt6core6:amd64: {runtime}\n", stderr="")
        for output in ["6.2.4", "6.2.4\t\t6.2.4", "6.2.4\tqt6-base\t",
                       "6.2.4\t../qt6-base\t6.2.4", "6.2.4\tqt6-base\t6.2.4 unsafe"]:
            with self.subTest(output=output), \
                    patch.object(notices.subprocess, "run", return_value=found), \
                    patch.object(notices.subprocess, "check_output", return_value=output):
                with self.assertRaisesRegex(ValueError, "Debian source metadata"):
                    notices.debian_owner(runtime)

    def test_linux_unknown_owner_missing_copyright_and_missing_origins_fail(self):
        runtime = self.file("lib/libQt6Core.so.6")
        with patch.object(notices, "debian_owner", side_effect=ValueError("expected one Debian package owner")):
            with self.assertRaisesRegex(ValueError, "package owner"):
                notices.collect_debian(self.bundle, [(str(runtime), "lib/libQt6Core.so.6")])
        with patch.object(notices, "debian_owner", return_value=(
                "libqt6core6:amd64", "6.2.4", "qt6-base", "6.2.4")):
            with self.assertRaisesRegex(ValueError, "missing Debian copyright"):
                notices.collect_debian(self.bundle, [(str(runtime), "lib/libQt6Core.so.6")], self.root / "missing")
        with self.assertRaisesRegex(ValueError, "differ from collected origins"):
            notices.collect_debian(self.bundle, [])

    def test_source_notice_catalog_is_pinned(self):
        catalog = json.loads((notices.ROOT / "licenses/qt/catalog.json").read_text(encoding="utf-8"))
        for key, item in catalog.items():
            with self.subTest(component=key):
                path = notices.ROOT / "licenses/qt" / item["path"]
                self.assertEqual(notices.digest(path), item["sha256"])
                self.assertIn(item["source"], path.read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
