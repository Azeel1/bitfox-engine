import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
from urllib.parse import quote, urlsplit
import zipfile

import collect_ubuntu_sources as ubuntu
import source_bundle


ROOT = Path.cwd()
CI = Path(__file__).resolve().parent
APT = [
    '-o', 'Dir::Etc::sourcelist=/etc/apt/sources.list.d/bitfox-reviewed-sources.list',
    '-o', 'Dir::Etc::sourceparts=-', '-o', 'Acquire::Retries=3',
]


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')
    return path


def archive_product(path, commit, version):
    current = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
    if current != commit:
        raise ValueError('Product source must be checked out at the binary review commit')
    with path.open('xb') as output:
        subprocess.run(['git', 'archive', '--format=tar.gz', '--prefix=bitfox-' + version + '/', commit],
                       stdout=output, check=True)


def collect_debian(component, target):
    name, version = component['package'], component['version']
    if re.fullmatch(r'[a-z0-9][a-z0-9+.-]*', name) is None or \
            re.fullmatch(r'[0-9][A-Za-z0-9.+:~-]*', version) is None:
        raise ValueError('Invalid Ubuntu source identity')
    with tempfile.TemporaryDirectory(prefix='bitfox-ubuntu-source-') as directory:
        temporary = Path(directory)
        result = subprocess.run(['apt-get', *APT, 'source', '--download-only', name + '=' + version],
                                cwd=temporary, capture_output=True, text=True)
        if result.returncode == 0:
            source = temporary
            origin = 'Ubuntu APT source indexes verified by the Ubuntu archive keyring'
        else:
            fallback = ubuntu.collect(component, temporary / 'launchpad')
            source = (temporary / 'launchpad' / fallback['files'][0]['path']).parent
            origin = fallback['publication']
        descriptors = list(source.glob('*.dsc'))
        if len(descriptors) != 1:
            raise ValueError('Expected one source descriptor for ' + name)
        descriptor = descriptors[0]
        checksums = ubuntu.parse_dsc(descriptor, name, version)
        target.mkdir(parents=True, exist_ok=False)
        files = [target / descriptor.name]
        shutil.copyfile(descriptor, files[0])
        for member, checksum in sorted(checksums.items()):
            ubuntu.verify(source / member, checksum)
            target_file = target / member
            shutil.copyfile(source / member, target_file)
            files.append(target_file)
        evidence = write_json(target / 'source-origin.json', {
            'component': component['id'], 'origin': origin,
            'descriptor': descriptor.name, 'files': [path.name for path in files],
        })
    build = [path for path in files if path.suffix == '.dsc' or
             '.debian.tar.' in path.name or path.name.endswith('.diff.gz')]
    if len(build) == 1:
        build = files
    return {'source': files, 'build': [*build, evidence]}


def collect_upstream(component, target, locked, package_inventory, ffmpeg_flags):
    item = locked[component['id']]
    url = item['source_url']
    if urlsplit(url).hostname != 'codeload.github.com' or \
            not url.endswith('/' + item['source_commit']) or \
            not component['source'].endswith('/' + item['source_commit']):
        raise ValueError('Upstream source does not match the reviewed commit')
    target.mkdir(parents=True, exist_ok=False)
    source = target / (component['id'].replace('/', '-') + '.tar.gz')
    subprocess.run(['curl', '--fail', '--location', '--silent', '--show-error',
                    '--max-time', '900', '--output', str(source), url], check=True)
    ubuntu.verify(source, (item['sha256'], item['bytes']))
    evidence = {'source_commit': item['source_commit'], 'source_tree': item['source_tree'],
                'source_url': url, 'uses': component['uses'],
                'binary_package_inventories': package_inventory}
    if component['id'].startswith('ffmpeg/'):
        evidence['compiled_configuration'] = ffmpeg_flags
    build = write_json(target / 'build-evidence.json', evidence)
    return {'source': [source], 'build': [source, build]}


def windows_configuration(packages):
    package = next(path for name, path in packages.items() if name.endswith('-windows-x86_64-gui.zip'))
    with zipfile.ZipFile(package) as archive:
        binary = archive.read('Bitfox/avutil-59.dll')
    flags = [value.decode('ascii') for value in re.findall(rb'[ -~]{20,}', binary)
             if b'--enable-' in value and b'--disable-' in value]
    if not flags or any(flag in value for value in flags for flag in
                        ('--enable-gpl', '--enable-nonfree', '--enable-version3')):
        raise ValueError('FFmpeg configuration differs from the reviewed license profile')
    return flags


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--packages', type=Path, required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--artifact-commit', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    output = args.output
    output.mkdir(exist_ok=False)
    packages = source_bundle.binary_packages(args.packages, args.version)
    requirements = source_bundle.requirements(packages, args.version, args.artifact_commit)
    write_json(output / 'requirements.json', requirements)
    locked = {item['component']: item
              for item in json.loads((CI / 'upstream-source-lock.json').read_text())}
    gui = {}
    for platform, (target, member) in source_bundle.GUI_TARGETS.items():
        name = f'bitfox-{args.version}-{target}-gui' + ('.tar.gz' if platform == 'linux' else '.zip')
        gui[name] = source_bundle.package_inventory(packages[name], member)
    win_engine = f'bitfox-{args.version}-windows-x86_64.zip'
    runtime = source_bundle.windows_engine_inventory(packages[win_engine], args.version)
    ffmpeg_flags = windows_configuration(packages)
    components = {}
    for item in requirements['components']:
        identity = item['id']
        target = output / 'payload' / quote(identity, safe='._+-~')
        inventories = {use['package']: gui[use['package']] for use in item['uses']
                       if use['package'] in gui}
        if identity == 'bitfox':
            target.mkdir(parents=True)
            source = target / 'reviewed-product-source.tar.gz'
            archive_product(source, args.artifact_commit, args.version)
            with tempfile.TemporaryDirectory(prefix='bitfox-rust-vendor-') as directory:
                vendor = Path(directory) / 'vendor'
                command = ['cargo', 'vendor', '--locked', '--versioned-dirs',
                           '--manifest-path', 'engine/Cargo.toml', str(vendor)]
                subprocess.run(command, check=True, stdout=subprocess.DEVNULL)
                vendor_archive = target / 'locked-engine-vendor.tar.gz'
                with tarfile.open(vendor_archive, 'w:gz') as archive:
                    archive.add(vendor, arcname='vendor')
            evidence = write_json(target / 'build-evidence.json', {
                'source_commit': args.artifact_commit, 'packages': requirements['packages'],
                'build_recipes': ['.github/workflows/release.yml', 'tools/pgo-build.sh',
                                  'engine/Cargo.toml', 'engine/Cargo.lock', 'gui-qt/CMakeLists.txt'],
                'vendor_scope': 'All registry sources in the locked engine dependency graph',
            })
            roles = {'source': [source], 'build': [source, evidence], 'vendor': [vendor_archive]}
        elif identity.startswith('debian/'):
            roles = collect_debian(item, target)
        elif identity.startswith('msvc/'):
            target.mkdir(parents=True)
            notice = target / 'msvc-2022.txt'
            shutil.copyfile(ROOT / 'licenses/qt/msvc-2022.txt', notice)
            evidence = write_json(target / 'redistribution-evidence.json', {
                'redistributable_version': item['version'], 'uses': item['uses'],
                'runtime_inventory': runtime,
                'official_redist_list': 'https://learn.microsoft.com/en-us/visualstudio/releases/2022/redistribution',
            })
            roles = {'redistribution': [notice, evidence]}
        else:
            roles = collect_upstream(item, target, locked, inventories, ffmpeg_flags)
        components[identity] = {role: [path.relative_to(output).as_posix() for path in paths]
                                for role, paths in roles.items()}
        print(identity, 'prepared', flush=True)
    plan = {'components': components}
    write_json(output / 'plan.json', plan)
    archive = output / source_bundle.source_name(args.version)
    source_bundle.create_archive(packages, args.version, args.artifact_commit, plan, output, archive)
    verified = source_bundle.verify_archive(archive, packages, args.version,
                                            expected_commit=args.artifact_commit)
    receipt = {
        'scope': 'Matching source materials for the seven reviewed packages at the release commit',
        'artifact_commit': args.artifact_commit,
        'packages': len(requirements['packages']),
        'components': len(components),
        'payload_files': len(verified['files']),
        'archive_bytes': archive.stat().st_size,
        'archive_sha256': ubuntu.digest(archive),
        'validator_passed': True,
    }
    write_json(output / 'receipt.json', receipt)
    print(json.dumps(receipt), flush=True)


if __name__ == '__main__':
    main()
