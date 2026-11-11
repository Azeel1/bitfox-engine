import json
import os
from pathlib import Path
import stat
import tempfile
import unittest
from unittest.mock import patch

import windows_runtime as runtime


def pe_bytes(dll=False, machine=0x8664):
    data = bytearray(90)
    data[:2] = b'MZ'
    data[60:64] = (64).to_bytes(4, 'little')
    data[64:68] = b'PE\0\0'
    data[68:70] = machine.to_bytes(2, 'little')
    data[84:86] = (2).to_bytes(2, 'little')
    data[86:88] = (0x2000 if dll else 0).to_bytes(2, 'little')
    data[88:90] = (0x20b).to_bytes(2, 'little')
    return bytes(data)


class WindowsRuntimeTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.bundle = self.root / 'bundle'
        self.bundle.mkdir()
        self.installation = self.root / 'Visual Studio'
        version_file = self.installation / 'VC/Auxiliary/Build/Microsoft.VCRedistVersion.default.txt'
        version_file.parent.mkdir(parents=True)
        version_file.write_text('14.44.35211\n')
        self.redist = self.installation / 'VC/Redist/MSVC/14.44.35211/x64/Microsoft.VC143.CRT'
        self.redist.mkdir(parents=True)
        for name in runtime.RUNTIME_FILES:
            (self.redist / name).write_bytes(pe_bytes(dll=True) + name.encode())
        locate = patch.object(runtime, 'locate_redist', side_effect=lambda: runtime.redist_directory(self.installation))
        locate.start()
        self.addCleanup(locate.stop)
        metadata = patch.object(runtime.notices, 'microsoft_metadata', return_value={
            'version': '14.44.35211.0', 'copyright': 'Copyright Microsoft Corporation'})
        self.metadata = metadata.start()
        self.addCleanup(metadata.stop)

    def executables(self, names):
        for name in names:
            path = self.bundle / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(pe_bytes())
        return names

    def stage(self, names=None):
        names = self.executables(names or ['bitfox.exe', 'bitfox-avx2.exe'])
        return runtime.stage(self.bundle, names)

    def write_manifest(self, value):
        (self.bundle / runtime.MANIFEST).write_text(json.dumps(value))

    def test_standalone_and_gui_paths_receive_identical_complete_crt_sets(self):
        names = self.executables(['bitfox-board.exe', 'engines/Bitfox 2.6.0.exe'])
        first = runtime.stage(self.bundle, names)
        self.assertEqual(len(first['dependencies']), len(runtime.RUNTIME_FILES) * 2)
        for name in runtime.RUNTIME_FILES:
            self.assertEqual((self.bundle / name).read_bytes(), (self.bundle / 'engines' / name).read_bytes())
        self.assertEqual(runtime.stage(self.bundle, names), first)
        self.assertEqual(runtime.verify(self.bundle), first)
        self.assertNotIn(str(self.root), json.dumps(first))
        self.assertEqual(first['origin']['directory'], 'x64/Microsoft.VC143.CRT')

    def test_standalone_multiple_executables_share_one_runtime_copy(self):
        result = self.stage()
        self.assertEqual(len(result['dependencies']), len(runtime.RUNTIME_FILES))
        self.assertEqual(result['executables'], ['bitfox-avx2.exe', 'bitfox.exe'])

    def test_conflicting_files_are_preserved_before_any_staging(self):
        names = self.executables(['bitfox.exe'])
        existing = self.bundle / 'msvcp140.dll'
        existing.write_bytes(b'existing user file')
        with self.assertRaisesRegex(ValueError, 'Conflicting runtime package member'):
            runtime.stage(self.bundle, names)
        self.assertEqual(existing.read_bytes(), b'existing user file')
        self.assertFalse((self.bundle / runtime.MANIFEST).exists())
        self.assertFalse((self.bundle / 'vcruntime140.dll').exists())

    def test_conflicting_manifest_is_preserved(self):
        names = self.executables(['bitfox.exe'])
        self.write_manifest({'existing': True})
        with self.assertRaisesRegex(ValueError, 'Conflicting runtime inventory'):
            runtime.stage(self.bundle, names)
        self.assertEqual(json.loads((self.bundle / runtime.MANIFEST).read_text()), {'existing': True})
        self.assertFalse((self.bundle / 'msvcp140.dll').exists())

    def test_missing_or_changed_dll_is_rejected(self):
        self.stage()
        target = self.bundle / 'msvcp140.dll'
        target.write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError, 'Missing or changed runtime DLL'):
            runtime.verify(self.bundle)
        target.unlink()
        with self.assertRaisesRegex(ValueError, 'Missing or changed runtime DLL'):
            runtime.verify(self.bundle)

    def test_undeclared_runtime_and_executable_are_rejected(self):
        self.stage()
        extra = self.bundle / 'msvcp999.dll'
        extra.write_bytes(pe_bytes(dll=True))
        with self.assertRaisesRegex(ValueError, 'inventory is incomplete'):
            runtime.verify(self.bundle)
        extra.unlink()
        (self.bundle / 'extra.exe').write_bytes(pe_bytes())
        with self.assertRaisesRegex(ValueError, 'every packaged executable'):
            runtime.verify(self.bundle)

    def test_required_runtime_cannot_be_removed_from_inventory_and_disk(self):
        data = self.stage()
        data['dependencies'] = [entry for entry in data['dependencies'] if entry['path'] != 'vcruntime140_1.dll']
        (self.bundle / 'vcruntime140_1.dll').unlink()
        self.write_manifest(data)
        with self.assertRaisesRegex(ValueError, 'same complete CRT set'):
            runtime.verify(self.bundle)

    def test_duplicate_runtime_record_is_rejected(self):
        data = self.stage()
        data['dependencies'].append(data['dependencies'][0])
        self.write_manifest(data)
        with self.assertRaisesRegex(ValueError, 'duplicate runtime'):
            runtime.verify(self.bundle)

    def test_notice_bytes_and_catalog_identity_are_enforced(self):
        data = self.stage()
        notice = self.bundle / data['notices'][0]['path']
        original = notice.read_bytes()
        notice.write_bytes(b'changed notice')
        with self.assertRaisesRegex(ValueError, 'notice differs'):
            runtime.verify(self.bundle)
        notice.write_bytes(original)
        data['notices'][0]['source'] = 'https://example.invalid/notice'
        self.write_manifest(data)
        with self.assertRaisesRegex(ValueError, 'notice differs'):
            runtime.verify(self.bundle)

    def test_distribution_origin_cannot_be_system_or_absolute_path(self):
        data = self.stage()
        for key, value in [('directory', 'C:/Windows/System32'), ('redist_version', '../../System32')]:
            changed = json.loads(json.dumps(data))
            changed['origin'][key] = value
            self.write_manifest(changed)
            with self.assertRaisesRegex(ValueError, 'distribution provenance'):
                runtime.verify(self.bundle)

    def test_default_redist_selection_rejects_path_injection_and_missing_source(self):
        directory, version = runtime.redist_directory(self.installation)
        self.assertEqual(directory, self.redist)
        self.assertEqual(version, '14.44.35211')
        default = self.installation / 'VC/Auxiliary/Build/Microsoft.VCRedistVersion.default.txt'
        default.write_text('../../System32')
        with self.assertRaisesRegex(ValueError, 'redistributable version'):
            runtime.redist_directory(self.installation)
        default.write_text('14.44.99999')
        with self.assertRaisesRegex(ValueError, 'CRT directory is missing'):
            runtime.redist_directory(self.installation)

    def test_staging_rejects_incomplete_or_foreign_runtime(self):
        names = self.executables(['bitfox.exe'])
        (self.redist / 'vcruntime140_1.dll').unlink()
        with self.assertRaisesRegex(ValueError, 'redistributable is incomplete'):
            runtime.stage(self.bundle, names)
        (self.redist / 'vcruntime140_1.dll').write_bytes(pe_bytes(dll=True, machine=0x14c))
        with self.assertRaisesRegex(ValueError, 'Expected an x64 DLL'):
            runtime.stage(self.bundle, names)
        self.assertFalse((self.bundle / runtime.MANIFEST).exists())

    def test_pe_format_and_image_type_are_checked(self):
        path = self.root / 'image'
        for contents in (b'invalid', pe_bytes(machine=0x14c), pe_bytes(dll=True)):
            path.write_bytes(contents)
            with self.assertRaises(ValueError):
                runtime.pe_x64(path, dll=False)
        path.write_bytes(pe_bytes())
        runtime.pe_x64(path, dll=False)
        with self.assertRaises(ValueError):
            runtime.pe_x64(path, dll=True)

    def test_invalid_metadata_and_duplicate_json_keys_are_rejected(self):
        names = self.executables(['bitfox.exe'])
        self.metadata.return_value = {'version': 'invalid', 'copyright': 'Microsoft'}
        with self.assertRaisesRegex(ValueError, 'file metadata'):
            runtime.stage(self.bundle, names)
        (self.bundle / runtime.MANIFEST).write_text('{"schema":1,"schema":1}')
        with self.assertRaisesRegex(ValueError, 'Duplicate runtime inventory key'):
            runtime.verify(self.bundle)

    def test_reparse_directory_is_rejected_before_children_are_read(self):
        junction = self.bundle / 'junction'
        junction.mkdir()
        original = Path.lstat

        def metadata(path):
            value = original(path)
            if path == junction:
                return type('Metadata', (), {'st_mode': stat.S_IFDIR, 'st_file_attributes': 0x400})()
            return value

        with patch.object(Path, 'lstat', metadata), self.assertRaisesRegex(ValueError, 'link or reparse point'):
            runtime.files_in(self.bundle)

    @unittest.skipIf(os.name == 'nt', 'Native Windows package inspection checks reparse attributes')
    def test_source_and_package_symlinks_are_rejected(self):
        names = self.executables(['bitfox.exe'])
        link = self.bundle / 'alias.exe'
        link.symlink_to(self.bundle / 'bitfox.exe')
        with self.assertRaisesRegex(ValueError, 'link or reparse point'):
            runtime.stage(self.bundle, names)
        link.unlink()
        target = self.redist / 'msvcp140.dll'
        target.unlink()
        target.symlink_to(self.redist / 'vcruntime140.dll')
        with self.assertRaisesRegex(ValueError, 'links or reparse points'):
            runtime.stage(self.bundle, names)


if __name__ == '__main__':
    unittest.main()
