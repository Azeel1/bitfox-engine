import json
from pathlib import Path
import shutil
import tempfile
import unittest

import check_project


class ProjectContractTests(unittest.TestCase):
    def fixture(self, root):
        for name in ['engine/Cargo.toml', 'engine/Cargo.lock', 'engine/.cargo/config.toml', 'engine/src/eval/contract.rs',
                     'engine/networks/manifest.json', 'engine/include/bitfox_core.h',
                     'gui-qt/include/bitfox_core.h', 'trainer/Cargo.toml', 'trainer/Cargo.lock',
                     'trainer/convert/Cargo.toml', 'rust-toolchain.toml']:
            target = root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(check_project.ROOT / name, target)
        shutil.copyfile(check_project.ROOT / 'engine/networks/bitfox.nnue', root / 'engine/networks/bitfox.nnue')

    def test_real_project_contract(self):
        result = check_project.validate(check_project.ROOT)
        self.assertEqual(result['c_abi_version'], 1)
        self.assertEqual(result['network_bytes'], 11822656)

    def test_missing_windows_cpu_target_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.fixture(root)
            path = root / 'engine/.cargo/config.toml'
            text = path.read_text().replace('target.x86_64-pc-windows-msvc', 'target.example-unused-platform')
            path.write_text(text)
            with self.assertRaisesRegex(ValueError, 'windows-msvc'):
                check_project.validate(root)

    def test_changed_network_byte_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.fixture(root)
            path = root / 'engine/networks/bitfox.nnue'
            with path.open('r+b') as output:
                value = output.read(1)
                output.seek(0)
                output.write(bytes([value[0] ^ 1]))
            with self.assertRaisesRegex(ValueError, 'network does not match'):
                check_project.validate(root)

    def test_contract_architecture_mismatch_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.fixture(root)
            path = root / 'engine/networks/manifest.json'
            metadata = json.loads(path.read_text())
            metadata['architecture']['hidden_size'] = 1024
            path.write_text(json.dumps(metadata))
            with self.assertRaisesRegex(ValueError, 'wrong hidden_size'):
                check_project.validate(root)

    def test_version_and_training_revision_mismatches_are_rejected(self):
        version = check_project.read_toml(check_project.ROOT / 'engine/Cargo.toml')['package']['version']
        for relative, before, after, message in [
            ('engine/Cargo.lock', f'name = "bitfox"\nversion = "{version}"', 'name = "bitfox"\nversion = "9.9.9"', 'lock versions'),
            ('trainer/Cargo.toml', 'rev = "ae9dd186af26d4a54518df5021ecde24e3ce8dc8"', 'branch = "main"', 'exact commit'),
        ]:
            with self.subTest(file=relative), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                self.fixture(root)
                path = root / relative
                text = path.read_text()
                self.assertIn(before, text)
                path.write_text(text.replace(before, after, 1))
                with self.assertRaisesRegex(ValueError, message):
                    check_project.validate(root)


if __name__ == '__main__':
    unittest.main()
