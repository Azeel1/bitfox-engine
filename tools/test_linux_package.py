import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


class LinuxPackageRootTests(unittest.TestCase):
    def fixture(self, root):
        (root / 'tools').mkdir(parents=True)
        (root / 'engine').mkdir()
        (root / 'engine/Cargo.toml').write_text('[package]\nname = "bitfox"\nversion = "2.6.0"\n')
        path = root / 'tools/package-linux-gui.sh'
        shutil.copyfile(ROOT / 'tools/package-linux-gui.sh', path)
        return path

    def invoke(self, script, cwd):
        environment = os.environ.copy()
        for name in ['BUILD_DIR', 'ENGINE_BIN', 'DIST_DIR', 'GIT_DIR', 'GIT_WORK_TREE']:
            environment.pop(name, None)
        result = subprocess.run(['bash', str(script)], cwd=cwd, env=environment,
                                capture_output=True, text=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('missing build output: ' + str(script.parent.parent / 'gui-qt/build-linux/bitfox-board'), result.stderr)
        self.assertNotIn('could not read package version', result.stderr)

    def test_archive_build_uses_its_own_source_from_an_unrelated_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory)
            source = parent / 'source with spaces'
            script = self.fixture(source)
            self.invoke(script, parent)

    def test_nested_source_snapshot_does_not_use_the_outer_git_repository(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory)
            subprocess.run(['git', 'init', '-q', str(parent)], check=True, capture_output=True)
            (parent / 'engine').mkdir()
            (parent / 'engine/Cargo.toml').write_text('[package]\nname="wrong"\nversion="9.9.9"\n')
            script = self.fixture(parent / 'private/snapshot')
            self.invoke(script, parent)


class LinuxPlatformPluginTests(unittest.TestCase):
    def invoke(self, root, body):
        source = (ROOT / 'tools/package-linux-gui.sh').read_text()
        helper = 'copy_plugin() {' + source.split('copy_plugin() {', 1)[1].split('\n}', 1)[0] + '\n}\n'
        plugins = root / 'Qt plugins'
        bundle = root / 'Bitfox package'
        origins = root / 'origins.tsv'
        script = ('set -euo pipefail\nqt_plugin_dir="$1"\ndist="$2"\n'
                  'plugin_dir="${dist}/plugins"\ndependency_origins="$3"\npending=()\n' +
                  helper + body)
        result = subprocess.run(['bash', '-c', script, '--', str(plugins), str(bundle), str(origins)],
                                capture_output=True, text=True, timeout=10)
        return result, plugins, bundle, origins

    def platform_loop(self):
        source = (ROOT / 'tools/package-linux-gui.sh').read_text()
        start = 'for plugin in platforms/libqxcb.so platforms/libqoffscreen.so; do'
        return start + source.split(start, 1)[1].split('\ndone', 1)[0] + '\ndone\n'

    def test_each_required_platform_plugin_is_checked_before_packaging(self):
        for missing in ('libqxcb.so', 'libqoffscreen.so'):
            with self.subTest(missing=missing), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                platforms = root / 'Qt plugins/platforms'
                platforms.mkdir(parents=True)
                other = 'libqoffscreen.so' if missing == 'libqxcb.so' else 'libqxcb.so'
                (platforms / other).write_bytes(b'platform fixture')
                result, _, _, _ = self.invoke(root, self.platform_loop())
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('missing required Qt plugin:', result.stderr)
                self.assertIn(missing, result.stderr)

    def test_available_platform_plugins_are_copied_and_recorded(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            platforms = root / 'Qt plugins/platforms'
            platforms.mkdir(parents=True)
            for name in ('libqxcb.so', 'libqoffscreen.so'):
                (platforms / name).write_bytes(name.encode())
            result, plugins, bundle, origins = self.invoke(root, self.platform_loop())
            self.assertEqual(result.returncode, 0, result.stderr)
            for name in ('libqxcb.so', 'libqoffscreen.so'):
                self.assertEqual((bundle / 'plugins/platforms' / name).read_bytes(), name.encode())
                self.assertIn(str(plugins / 'platforms' / name) + '\tplugins/platforms/' + name,
                              origins.read_text())

    def test_missing_optional_plugin_does_not_fail_packaging(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            result, _, _, origins = self.invoke(root, 'copy_plugin "${qt_plugin_dir}/styles/optional.so"\n')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(origins.exists())


if __name__ == '__main__':
    unittest.main()
