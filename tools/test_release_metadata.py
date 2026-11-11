import hashlib
from pathlib import Path
import tempfile
import unittest

import release_metadata


class ReleaseMetadataTests(unittest.TestCase):
    def populate(self, root):
        for name in release_metadata.package_names("2.6.0"):
            folder = root / name.split(".")[-1]
            folder.mkdir(exist_ok=True)
            data = name.encode()
            (folder / name).write_bytes(data)
            digest = hashlib.sha256(data).hexdigest()
            (folder / (name + ".sha256")).write_text(f"{digest}  {name}\n")

    def test_stable_versions_pass_and_unsafe_versions_are_rejected(self):
        for version in ("0.0.1", "0.999.999", "1.0.0", "2.5.0", "2.6.0", "3.0.0", "2.1000.0"):
            with self.subTest(version=version):
                self.assertEqual(release_metadata.validate_version(version), version)
        for version in ("0.0.0", "02.5.0", "2.5", "2.6.0-beta", "2.5.0/../../evil"):
            with self.subTest(version=version), self.assertRaises(ValueError):
                release_metadata.validate_version(version)

    def test_checksums_cover_exact_primary_packages(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.populate(root)
            path = release_metadata.create_checksums(root, "2.6.0")
            lines = path.read_text().splitlines()
            self.assertEqual([line.split("  ")[1] for line in lines], release_metadata.package_names("2.6.0"))
            self.assertEqual(len(lines), 7)
            self.assertEqual(release_metadata.create_checksums(root, "2.6.0"), path)

    def test_rejects_missing_unexpected_corrupted_duplicate_and_symlink_assets(self):
        for mode in ("missing", "unexpected", "corrupted", "duplicate", "symlink"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                self.populate(root)
                original = next(root.rglob("*.zip"))
                if mode == "missing":
                    original.unlink()
                elif mode == "unexpected":
                    (root / "private-notes.txt").write_text("not for publication")
                elif mode == "corrupted":
                    original.write_bytes(b"changed after checksum")
                elif mode == "duplicate":
                    (root / original.name).write_bytes(original.read_bytes())
                else:
                    (root / "link").symlink_to(original)
                with self.assertRaises(ValueError):
                    release_metadata.create_checksums(root, "2.6.0")
                self.assertFalse((root / "SHA256SUMS").exists())


if __name__ == "__main__":
    unittest.main()
