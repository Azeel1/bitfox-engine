import copy
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import stat
import sys
import tarfile
import tempfile
import textwrap
import unittest
import zipfile

import release_metadata
import source_bundle


class SourceBundleTests(unittest.TestCase):
    version = "2.6.0"
    commit = "1234567890abcdef1234567890abcdef12345678"

    def windows_engine_files(self):
        def pe(dll):
            raw = bytearray(90)
            raw[:2] = b"MZ"
            raw[60:64] = (64).to_bytes(4, "little")
            raw[64:68] = b"PE\0\0"
            raw[68:70] = b"\x64\x86"
            raw[84:86] = (2).to_bytes(2, "little")
            raw[86:88] = (0x2000 if dll else 0).to_bytes(2, "little")
            raw[88:90] = b"\x0b\x02"
            return bytes(raw)
        files = {"LICENSE": b"license fixture", "THIRD_PARTY_NOTICES.txt": b"notice fixture"}
        files.update({name: pe(False) + name.encode() for name in ("bitfox.exe", "bitfox-avx2.exe")})
        dependencies = []
        for name in sorted(source_bundle.windows_runtime.REQUIRED_FILES):
            raw = pe(True) + name.encode()
            files[name] = raw
            dependencies.append({"path": name, "sha256": hashlib.sha256(raw).hexdigest(), "provider": "msvc",
                                 "version": "14.42.34433.0", "copyright": "Microsoft Corporation"})
        path, notice = source_bundle.windows_runtime.notice_record()
        files[notice["path"]] = path.read_bytes()
        inventory = {"schema": 1, "platform": "windows", "architecture": "x86_64",
                     "executables": ["bitfox-avx2.exe", "bitfox.exe"],
                     "origin": {"provider": "Visual Studio 2022", "redist_version": "14.42.34433",
                                "directory": "x64/Microsoft.VC143.CRT"},
                     "dependencies": dependencies, "notices": [notice]}
        files[source_bundle.windows_runtime.MANIFEST] = json.dumps(inventory).encode()
        return files

    def write_windows_engine(self, path, files):
        with zipfile.ZipFile(path, "w") as archive:
            for name, raw in files.items():
                archive.writestr(f"bitfox-{self.version}-windows-x86_64/" + name, raw)

    def fixture(self, root):
        packages = {}
        for name in release_metadata.package_names(self.version):
            path = root / name
            platform = next((p for p, (target, member) in source_bundle.GUI_TARGETS.items()
                             if name == f"bitfox-{self.version}-{target}-gui" + (".tar.gz" if p == "linux" else ".zip")), None)
            if name == f"bitfox-{self.version}-windows-x86_64.zip":
                self.write_windows_engine(path, self.windows_engine_files())
            elif platform is None:
                path.write_bytes(b"synthetic engine fixture\n" + name.encode())
            else:
                member = source_bundle.GUI_TARGETS[platform][1]
                if platform == "linux":
                    inventory = {"schema": 1, "platform": platform, "dependencies": [
                        {"path": "lib/libQt6Core.so.6", "provider": "libqt6core6", "source_package": "qt6-base",
                         "source_version": "6.2.4+dfsg-2ubuntu1"},
                    ]}
                elif platform == "macos":
                    inventory = {"schema": 1, "platform": platform, "qt_version": "6.11.0", "dependencies": [
                        {"path": "Contents/Frameworks/QtCore.framework/Versions/A/QtCore", "provider": "qtbase"},
                        {"path": "Contents/Frameworks/QtMultimedia.framework/Versions/A/QtMultimedia", "provider": "qtmultimedia"},
                    ]}
                else:
                    inventory = {"schema": 1, "platform": platform, "qt_version": "6.8.3", "dependencies": [
                        {"path": "Qt6Core.dll", "provider": "qtbase"},
                        {"path": "Qt6Multimedia.dll", "provider": "qtmultimedia"},
                        {"path": "avutil-59.dll", "provider": "ffmpeg"},
                        {"path": "msvcp140.dll", "provider": "msvc", "version": "14.42.34433.0"},
                        {"path": "engines/msvcp140.dll", "provider": "msvc", "version": "14.42.34433.0"},
                    ]}
                raw = json.dumps(inventory).encode()
                if platform == "linux":
                    with tarfile.open(path, "w:gz") as archive:
                        info = tarfile.TarInfo(member)
                        info.size = len(raw)
                        archive.addfile(info, io.BytesIO(raw))
                else:
                    with zipfile.ZipFile(path, "w") as archive:
                        archive.writestr(member, raw)
            packages[name] = path
            self.checksum(path)
        expected = source_bundle.requirements(packages, self.version, self.commit)
        plan = {"components": {}}
        for index, item in enumerate(expected["components"]):
            plan["components"][item["id"]] = {}
            for role in item["roles"]:
                relative = f"payload/{index}/{role}.txt"
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(f"Synthetic test payload for {item['id']} {role}\n")
                plan["components"][item["id"]][role] = [relative]
        return packages, plan, expected

    def checksum(self, path):
        path.with_name(path.name + ".sha256").write_text(
            f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n")

    def create(self, root):
        packages, plan, expected = self.fixture(root)
        archive = source_bundle.create_archive(packages, self.version, self.commit, plan, root,
                                               root / source_bundle.source_name(self.version))
        return archive, packages, plan, expected

    def rewrite(self, archive, transform):
        with tarfile.open(archive, "r:gz") as source:
            members = [(member.name, source.extractfile(member).read(), member.type) for member in source]
        members = transform(members)
        with tarfile.open(archive, "w:gz") as destination:
            for name, raw, kind in members:
                member = tarfile.TarInfo(name)
                member.type, member.size = kind, len(raw)
                destination.addfile(member, io.BytesIO(raw))
        self.checksum(archive)

    def test_bundle_maps_exact_sources_and_binds_all_packages(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            archive, packages, plan, expected = self.create(root)
            result = source_bundle.verify_archive(archive, packages, self.version, expected_commit=self.commit)
            self.assertEqual(result["requirements"], expected)
            identifiers = {item["id"]: item for item in expected["components"]}
            self.assertIn("debian/qt6-base/6.2.4+dfsg-2ubuntu1", identifiers)
            self.assertEqual(identifiers["msvc/14.42.34433.0"]["roles"], ["redistribution"])
            runtime_uses = identifiers["msvc/14.42.34433.0"]["uses"]
            for name in source_bundle.windows_runtime.REQUIRED_FILES:
                self.assertIn({"package": f"bitfox-{self.version}-windows-x86_64.zip", "path": name}, runtime_uses)
            for name in ("msvcp140.dll", "engines/msvcp140.dll"):
                self.assertIn({"package": f"bitfox-{self.version}-windows-x86_64-gui.zip", "path": name}, runtime_uses)
            self.assertEqual(identifiers["ffmpeg/6.8.3"]["version"], "7.1")
            self.assertTrue(identifiers["qtbase/6.11.0"]["source"].endswith("8ba7ea4b77a4b8f1948760221e264917ddc9e1c8"))
            self.assertEqual(set(result["components"]), set(identifiers))
            self.assertEqual(len(result["requirements"]["packages"]), 7)

    def test_windows_runtime_metadata_and_payload_are_validated_before_binding(self):
        cases = ("missing-inventory", "missing-dll", "changed-dll", "unknown-dll", "changed-notice",
                 "notice-catalog", "provider", "version", "path", "digest", "architecture", "pe-machine",
                 "missing-executable", "duplicate-dependency", "duplicate-json-key")
        for mode in cases:
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                packages, _, _ = self.fixture(root)
                path = packages[f"bitfox-{self.version}-windows-x86_64.zip"]
                files = self.windows_engine_files()
                key = source_bundle.windows_runtime.MANIFEST
                data = json.loads(files[key])
                if mode == "missing-inventory":
                    files.pop(key)
                elif mode == "missing-dll":
                    files.pop("msvcp140.dll")
                elif mode == "changed-dll":
                    files["msvcp140.dll"] += b"changed"
                elif mode == "unknown-dll":
                    files["msvcp999.dll"] = files["msvcp140.dll"]
                elif mode == "changed-notice":
                    files["licenses/msvc-2022.txt"] += b"changed"
                elif mode == "notice-catalog":
                    data["notices"][0]["source"] = "https://invalid.example/notice"
                elif mode == "provider":
                    data["dependencies"][0]["provider"] = "qtbase"
                elif mode == "version":
                    data["dependencies"][0]["version"] = "unverified"
                elif mode == "path":
                    data["dependencies"][0]["path"] = "../msvcp140.dll"
                elif mode == "digest":
                    data["dependencies"][0]["sha256"] = "not-a-digest"
                elif mode == "architecture":
                    data["architecture"] = "arm64"
                elif mode == "pe-machine":
                    raw = bytearray(files["msvcp140.dll"])
                    raw[68:70] = b"\x64\xaa"
                    files["msvcp140.dll"] = bytes(raw)
                    for entry in data["dependencies"]:
                        if entry["path"] == "msvcp140.dll":
                            entry["sha256"] = hashlib.sha256(raw).hexdigest()
                elif mode == "missing-executable":
                    files.pop("bitfox-avx2.exe")
                elif mode == "duplicate-dependency":
                    data["dependencies"].append(copy.deepcopy(data["dependencies"][0]))
                if mode not in {"missing-inventory", "duplicate-json-key"}:
                    files[key] = json.dumps(data).encode()
                elif mode == "duplicate-json-key":
                    files[key] = b'{"schema":1,' + json.dumps(data).encode()[1:]
                self.write_windows_engine(path, files)
                with self.assertRaises(ValueError):
                    source_bundle.requirements(packages, self.version, self.commit)

    def test_windows_engine_extraction_rejects_duplicates_links_and_path_escape(self):
        for mode in ("duplicate", "case-conflict", "symlink", "escape", "other-root", "private-file"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                packages, _, _ = self.fixture(root)
                path = packages[f"bitfox-{self.version}-windows-x86_64.zip"]
                prefix = f"bitfox-{self.version}-windows-x86_64/"
                with zipfile.ZipFile(path, "a") as archive:
                    if mode == "duplicate":
                        import warnings
                        with warnings.catch_warnings():
                            warnings.simplefilter("ignore", UserWarning)
                            archive.writestr(prefix + "LICENSE", b"duplicate")
                    elif mode == "case-conflict":
                        archive.writestr(prefix + "MSVCP140.dll", b"conflict")
                    elif mode == "symlink":
                        entry = zipfile.ZipInfo(prefix + "concrt140.dll")
                        entry.create_system = 3
                        entry.external_attr = (stat.S_IFLNK | 0o777) << 16
                        archive.writestr(entry, "msvcp140.dll")
                    else:
                        name = {"escape": prefix + "../outside", "other-root": "elsewhere/member",
                                "private-file": prefix + "private-notes.txt"}[mode]
                        archive.writestr(name, b"unexpected")
                with self.assertRaises(ValueError):
                    source_bundle.requirements(packages, self.version, self.commit)
                self.assertFalse((root / "outside").exists())

    def test_source_checksums_are_verified_and_exported(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            packages_root = root / "assets"
            packages_root.mkdir()
            archive, packages, plan, expected = self.create(packages_root)
            (packages_root / "payload").rename(root / "payload")
            result = release_metadata.create_checksums(packages_root, self.version, require_source=True,
                                                       source_commit=self.commit)
            names = [line.split("  ")[1] for line in result.read_text().splitlines()]
            self.assertEqual(names, sorted([*release_metadata.package_names(self.version), archive.name]))
            self.assertEqual(len(names), 8)

    def test_cli_requires_source_for_publish_style_call(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            packages, plan, expected = self.fixture(root)
            (root / "payload").rename(root / "test-payload")
            import shutil
            shutil.rmtree(root / "test-payload")
            result = subprocess.run([sys.executable, str(Path(release_metadata.__file__).resolve()), self.version,
                                     "--directory", str(root), "--require-source", "--source-commit", self.commit],
                                    capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing verified corresponding-source archive", result.stderr)
            self.assertFalse((root / "SHA256SUMS").exists())

    def test_release_workflow_keeps_review_binary_only_and_blocks_incomplete_publish(self):
        repository = Path(__file__).resolve().parents[1]
        workflow = (repository / ".github/workflows/release.yml").read_text()
        version = workflow.split("  version:\n", 1)[1].split("\n  verify:\n", 1)[0]
        self.assertIn("source_commit: ${{ steps.meta.outputs.source_commit }}", version)
        self.assertIn('echo "source_commit=${source_commit}" >> "$GITHUB_OUTPUT"', version)
        review = workflow.split("  smoke-artifacts:\n", 1)[1].split("\n  publish:\n", 1)[0]
        review_step = review.split("      - name: Verify assets and prepare updater checksums\n", 1)[1].split("\n      - name:", 1)[0]
        review_script = review_step.split("        run: ", 1)[1].strip()
        publish = workflow.split("\n  publish:\n", 1)[1].split("\n  smoke-release:\n", 1)[0]
        publish_step = publish.split("      - name: Verify assets and prepare updater checksums\n", 1)[1].split("\n      - name:", 1)[0]
        self.assertIn("SOURCE_COMMIT: ${{ needs.version.outputs.source_commit }}", publish_step)
        publish_script = textwrap.dedent(publish_step.split("        run: |\n", 1)[1])
        for mode in ["review", "missing", "wrong-commit", "verified"]:
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                assets = root / "release-assets"
                assets.mkdir()
                (root / "tools").symlink_to(repository / "tools", target_is_directory=True)
                if mode in {"wrong-commit", "verified"}:
                    self.create(assets)
                else:
                    self.fixture(assets)
                (assets / "payload").rename(root / "payload")
                environment = dict(os.environ, VERSION=self.version, SOURCE_COMMIT=self.commit,
                                   PYTHONDONTWRITEBYTECODE="1")
                if mode == "wrong-commit":
                    environment["SOURCE_COMMIT"] = "0" * 40
                result = subprocess.run(["bash", "-c", review_script if mode == "review" else publish_script],
                                        cwd=root, env=environment, capture_output=True, text=True, timeout=15)
                if mode in {"review", "verified"}:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertTrue((assets / "SHA256SUMS").is_file())
                else:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertFalse((assets / "SHA256SUMS").exists())
                    expected = "missing verified corresponding-source archive" if mode == "missing" else "differs from the release commit"
                    self.assertIn(expected, result.stderr)

    def test_requires_exact_commit_and_preserves_previous_checksums_on_failure(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            assets = root / "assets"
            assets.mkdir()
            archive, packages, plan, expected = self.create(assets)
            (assets / "payload").rename(root / "payload")
            (assets / "SHA256SUMS").write_text("previous checksum inventory\n")
            with self.assertRaisesRegex(ValueError, "differs from the release commit"):
                release_metadata.create_checksums(assets, self.version, require_source=True, source_commit="0" * 40)
            self.assertEqual((assets / "SHA256SUMS").read_text(), "previous checksum inventory\n")
            with self.assertRaisesRegex(ValueError, "release source commit"):
                release_metadata.create_checksums(assets, self.version, require_source=True)

    def test_cli_reports_a_corrupt_source_archive_without_a_traceback(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            assets = root / "assets"
            assets.mkdir()
            self.fixture(assets)
            (assets / "payload").rename(root / "payload")
            archive = assets / source_bundle.source_name(self.version)
            archive.write_bytes(b"not a compressed source archive")
            self.checksum(archive)
            result = subprocess.run([sys.executable, str(Path(release_metadata.__file__).resolve()), self.version,
                                     "--directory", str(assets), "--require-source", "--source-commit", self.commit],
                                    capture_output=True, text=True, timeout=15)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("not a gzip file", result.stderr)
            self.assertNotIn("Traceback", result.stderr)
            self.assertFalse((assets / "SHA256SUMS").exists())

    def test_rejects_changed_packages_and_incomplete_binary_set(self):
        with tempfile.TemporaryDirectory() as tmp:
            archive, packages, plan, expected = self.create(Path(tmp))
            modified = next(path for name, path in packages.items() if "-gui" not in name)
            modified.write_bytes(b"replacement engine")
            self.checksum(modified)
            with self.assertRaisesRegex(ValueError, "does not match"):
                source_bundle.verify_archive(archive, packages, self.version)
            packages.pop(next(iter(packages)))
            with self.assertRaisesRegex(ValueError, "all seven binary packages"):
                source_bundle.verify_archive(archive, packages, self.version)

    def test_rejects_tampered_duplicate_nonregular_and_missing_payload(self):
        for mode in ["changed", "duplicate", "symlink", "missing", "traversal", "second-manifest"]:
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as tmp:
                archive, packages, plan, expected = self.create(Path(tmp))
                def mutate(members):
                    if mode == "changed":
                        name, raw, kind = members[1]
                        members[1] = (name, b"x" * len(raw), kind)
                    elif mode == "duplicate":
                        members.append(members[1])
                    elif mode == "symlink":
                        name, raw, kind = members[1]
                        members[1] = (name, b"", tarfile.SYMTYPE)
                    elif mode == "missing":
                        members.pop()
                    elif mode == "traversal":
                        members.append(("../outside", b"outside", tarfile.REGTYPE))
                    else:
                        members.append(members[0])
                    return members
                self.rewrite(archive, mutate)
                with self.assertRaises(ValueError):
                    source_bundle.verify_archive(archive, packages, self.version)

    def test_rejects_incomplete_roles_unsafe_inputs_and_malformed_paths(self):
        for mode in ["missing-role", "missing-component", "nonstring", "outside", "symlink", "empty"]:
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                packages, plan, expected = self.fixture(root)
                first = plan["components"]["bitfox"]
                source = first["source"][0]
                if mode == "missing-role":
                    first.pop("vendor")
                elif mode == "missing-component":
                    plan["components"].pop("bitfox")
                elif mode == "nonstring":
                    first["source"] = [{"path": source}]
                elif mode == "outside":
                    first["source"] = ["../outside"]
                elif mode == "symlink":
                    (root / source).unlink()
                    (root / source).symlink_to(root / first["build"][0])
                else:
                    (root / source).write_bytes(b"")
                with self.assertRaises(ValueError):
                    source_bundle.create_archive(packages, self.version, self.commit, plan, root,
                                                  root / source_bundle.source_name(self.version))
                self.assertFalse((root / source_bundle.source_name(self.version)).exists())

    def test_duplicate_json_keys_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "duplicate JSON key"):
            source_bundle.json_data('{"components":{},"components":{}}')

    def test_existing_archive_is_preserved(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            archive, packages, plan, expected = self.create(root)
            original = archive.read_bytes()
            with self.assertRaisesRegex(ValueError, "must not already exist"):
                source_bundle.create_archive(packages, self.version, self.commit, plan, root, archive)
            self.assertEqual(archive.read_bytes(), original)

    def test_dangling_output_and_checksum_symlinks_are_preserved(self):
        for member in ("archive", "checksum"):
            with self.subTest(member=member), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                packages, plan, expected = self.fixture(root)
                archive = root / source_bundle.source_name(self.version)
                checksum = archive.with_name(archive.name + ".sha256")
                link = archive if member == "archive" else checksum
                target = root / "unexpected-target"
                link.symlink_to(target)
                original = os.readlink(link)
                self.assertTrue(link.is_symlink())
                self.assertFalse(link.exists())
                with self.assertRaisesRegex(ValueError, "must not already exist"):
                    source_bundle.create_archive(packages, self.version, self.commit, plan, root, archive)
                self.assertTrue(link.is_symlink())
                self.assertEqual(os.readlink(link), original)
                self.assertFalse(target.exists())
                self.assertFalse(archive.exists())
                self.assertFalse(checksum.exists())


if __name__ == "__main__":
    unittest.main()
