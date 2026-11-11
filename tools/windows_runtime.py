import argparse
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess

import gui_dependency_notices as notices


MANIFEST = 'RUNTIME_DEPENDENCIES.json'
RUNTIME_FILES = frozenset(name for name in notices.MSVC_FILES if name.endswith('.dll'))
REQUIRED_FILES = frozenset({'msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll'})
VERSION = re.compile(r'[0-9]+(?:[., ][0-9]+)*')
REDIST_VERSION = re.compile(r'[0-9]+(?:\.[0-9]+){2,3}')
RUNTIME_NAME = re.compile(r'(?:msvcp|vcruntime)[0-9][a-z0-9_]*\.dll|concrt[0-9]+\.dll')


def checked_path(path):
    path = Path(path).absolute()
    for ancestor in [*reversed(path.parents), path]:
        try:
            info = ancestor.lstat()
        except FileNotFoundError:
            continue
        if stat.S_ISLNK(info.st_mode) or getattr(info, 'st_file_attributes', 0) & 0x400:
            raise ValueError('Runtime paths must not contain links or reparse points')
    return path


def files_in(bundle):
    bundle = checked_path(bundle)
    if not bundle.is_dir():
        raise ValueError('Package directory does not exist')
    pending, result = [bundle], {}
    while pending:
        for path in sorted(pending.pop().iterdir()):
            info = path.lstat()
            if stat.S_ISLNK(info.st_mode) or getattr(info, 'st_file_attributes', 0) & 0x400:
                raise ValueError('Package contains a link or reparse point')
            if stat.S_ISDIR(info.st_mode):
                pending.append(path)
            elif stat.S_ISREG(info.st_mode):
                relative = path.relative_to(bundle).as_posix()
                if relative.casefold() in {name.casefold() for name in result}:
                    raise ValueError('Package has case-conflicting paths')
                result[relative] = path
            else:
                raise ValueError('Package contains an unsupported file type')
    return result


def pe_x64(path, dll):
    with Path(path).open('rb') as source:
        header = source.read(64)
        if len(header) != 64 or header[:2] != b'MZ':
            raise ValueError('Invalid PE executable: ' + Path(path).name)
        offset = int.from_bytes(header[60:64], 'little')
        size = os.fstat(source.fileno()).st_size
        if offset < 64 or offset > size - 26:
            raise ValueError('Invalid PE header offset')
        source.seek(offset)
        header = source.read(26)
        optional = int.from_bytes(header[20:22], 'little')
        characteristics = int.from_bytes(header[22:24], 'little')
        if header[:4] != b'PE\0\0' or header[4:6] != b'\x64\x86' \
                or optional < 2 or offset + 24 + optional > size or header[24:26] != b'\x0b\x02' \
                or bool(characteristics & 0x2000) != dll:
            raise ValueError('Expected an x64 ' + ('DLL' if dll else 'executable') + ': ' + Path(path).name)


def executable_members(bundle, executables, files):
    if not isinstance(executables, list) or not executables:
        raise ValueError('At least one executable is required')
    names = [notices.relative_path(value) for value in executables]
    if len({name.casefold() for name in names}) != len(names):
        raise ValueError('Duplicate executable paths')
    actual = {name for name in files if name.lower().endswith('.exe')}
    if set(names) != actual:
        raise ValueError('Runtime inventory must cover every packaged executable')
    for name in names:
        pe_x64(files[name], dll=False)
    return sorted(names)


def redist_directory(installation):
    installation = checked_path(installation)
    version_file = checked_path(installation / 'VC/Auxiliary/Build/Microsoft.VCRedistVersion.default.txt')
    version = version_file.read_text(encoding='utf-8-sig').strip()
    if REDIST_VERSION.fullmatch(version) is None:
        raise ValueError('Invalid Visual Studio redistributable version')
    directory = checked_path(installation / 'VC/Redist/MSVC' / version / 'x64/Microsoft.VC143.CRT')
    if not directory.is_dir():
        raise ValueError('Approved Visual Studio x64 CRT directory is missing')
    return directory, version


def locate_redist():
    if os.name != 'nt':
        raise ValueError('Runtime staging requires Windows with Visual Studio 2022')
    locator = checked_path(Path(os.environ['ProgramFiles(x86)']) / 'Microsoft Visual Studio/Installer/vswhere.exe')
    result = subprocess.run([str(locator), '-latest', '-products', '*', '-version', '[17.0,18.0)',
                             '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64',
                             '-property', 'installationPath'], check=True, capture_output=True,
                            text=True, encoding='utf-8', errors='strict')
    paths = result.stdout.strip().splitlines()
    if len(paths) != 1 or not Path(paths[0]).is_absolute():
        raise ValueError('Expected one Visual Studio 2022 installation')
    return redist_directory(paths[0])


def notice_record():
    catalog = json.loads((notices.ROOT / 'licenses/qt/catalog.json').read_text(encoding='utf-8'))['msvc/2022']
    source = checked_path(notices.ROOT / 'licenses/qt' / notices.relative_path(catalog['path']))
    if not source.is_file() or notices.digest(source) != catalog['sha256']:
        raise ValueError('Microsoft runtime notice differs from its catalog')
    return source, {'path': 'licenses/' + source.name, 'sha256': catalog['sha256'],
                    'provider': 'msvc', 'version': catalog['version'], 'source': catalog['source']}


def valid_version(value):
    return isinstance(value, str) and VERSION.fullmatch(value) is not None


def read_inventory(path):
    def unique(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError('Duplicate runtime inventory key')
            result[key] = value
        return result

    try:
        return json.loads(path.read_text(encoding='utf-8'), object_pairs_hook=unique)
    except (UnicodeError, json.JSONDecodeError) as error:
        raise ValueError('Invalid runtime inventory JSON') from error


def stage(bundle, executables):
    bundle = checked_path(bundle)
    files = files_in(bundle)
    executables = executable_members(bundle, executables, files)
    redist, version = locate_redist()
    available = {path.name.lower(): checked_path(path) for path in redist.iterdir()
                 if path.name.lower() in RUNTIME_FILES}
    if not REQUIRED_FILES.issubset(available):
        raise ValueError('Visual Studio redistributable is incomplete')
    parents = sorted({Path(name).parent.as_posix() for name in executables})
    entries, copies = [], {}
    for name, source in sorted(available.items()):
        if not source.is_file():
            raise ValueError('Runtime member must be a regular file')
        pe_x64(source, dll=True)
        metadata = notices.microsoft_metadata(source)
        if not valid_version(metadata.get('version')) or not isinstance(metadata.get('copyright'), str) \
                or 'Microsoft' not in metadata['copyright']:
            raise ValueError('Invalid Microsoft runtime file metadata')
        digest = notices.digest(source)
        for parent in parents:
            relative = (Path(parent) / name).as_posix()
            entries.append({'path': relative, 'sha256': digest, 'provider': 'msvc',
                            'version': metadata['version'], 'copyright': metadata['copyright']})
            copies[relative] = (source, digest)
    notice_source, notice = notice_record()
    copies[notice['path']] = (notice_source, notice['sha256'])
    expected_runtime = {entry['path'] for entry in entries}
    for name in files:
        if RUNTIME_NAME.fullmatch(Path(name).name.casefold()) and name not in expected_runtime:
            raise ValueError('Unexpected packaged Microsoft runtime member: ' + name)
    manifest = {'schema': 1, 'platform': 'windows', 'architecture': 'x86_64',
                'executables': executables,
                'origin': {'provider': 'Visual Studio 2022', 'redist_version': version,
                           'directory': 'x64/Microsoft.VC143.CRT'},
                'dependencies': sorted(entries, key=lambda entry: entry['path']), 'notices': [notice]}
    encoded = (json.dumps(manifest, indent=2, sort_keys=True) + '\n').encode()
    for name, (_, digest) in copies.items():
        destination = checked_path(bundle / name)
        if destination.exists() and (not destination.is_file() or notices.digest(destination) != digest):
            raise ValueError('Conflicting runtime package member: ' + name)
    destination = checked_path(bundle / MANIFEST)
    if destination.exists() and (not destination.is_file() or destination.read_bytes() != encoded):
        raise ValueError('Conflicting runtime inventory')
    created = []
    try:
        for name, (source, digest) in copies.items():
            destination = bundle / name
            if destination.exists():
                continue
            destination.parent.mkdir(parents=True, exist_ok=True)
            with source.open('rb') as input_file, destination.open('xb') as output_file:
                created.append(destination)
                shutil.copyfileobj(input_file, output_file)
            if notices.digest(destination) != digest:
                raise ValueError('Runtime changed while copying: ' + name)
        destination = bundle / MANIFEST
        if not destination.exists():
            with destination.open('xb') as output:
                created.append(destination)
                output.write(encoded)
        return verify(bundle)
    except BaseException:
        for path in reversed(created):
            path.unlink(missing_ok=True)
        raise


def verify(bundle):
    bundle = checked_path(bundle)
    files = files_in(bundle)
    if MANIFEST not in files:
        raise ValueError('Missing runtime dependency inventory')
    data = read_inventory(files[MANIFEST])
    if not isinstance(data, dict) or set(data) != {'schema', 'platform', 'architecture', 'executables', 'origin', 'dependencies', 'notices'} \
            or data['schema'] != 1 or data['platform'] != 'windows' or data['architecture'] != 'x86_64':
        raise ValueError('Invalid Windows runtime inventory')
    origin = data['origin']
    if not isinstance(origin, dict) or set(origin) != {'provider', 'redist_version', 'directory'} \
            or origin['provider'] != 'Visual Studio 2022' or origin['directory'] != 'x64/Microsoft.VC143.CRT' \
            or not isinstance(origin['redist_version'], str) or REDIST_VERSION.fullmatch(origin['redist_version']) is None:
        raise ValueError('Invalid runtime distribution provenance')
    executables = executable_members(bundle, data['executables'], files)
    parents = {Path(name).parent.as_posix() for name in executables}
    if not isinstance(data['dependencies'], list) or not data['dependencies']:
        raise ValueError('Missing runtime dependencies')
    found = {}
    for entry in data['dependencies']:
        if not isinstance(entry, dict) or set(entry) != {'path', 'sha256', 'provider', 'version', 'copyright'}:
            raise ValueError('Invalid runtime dependency record')
        name = notices.relative_path(entry['path'])
        if name in found or Path(name).name not in RUNTIME_FILES or Path(name).parent.as_posix() not in parents \
                or entry['provider'] != 'msvc' or not valid_version(entry['version']) \
                or not isinstance(entry['copyright'], str) or 'Microsoft' not in entry['copyright']:
            raise ValueError('Invalid or duplicate runtime dependency')
        if name not in files or notices.digest(files[name]) != entry['sha256']:
            raise ValueError('Missing or changed runtime DLL: ' + name)
        pe_x64(files[name], dll=True)
        found[name] = entry
    actual = {name for name in files if RUNTIME_NAME.fullmatch(Path(name).name.casefold())}
    if actual != set(found):
        raise ValueError('Runtime dependency inventory is incomplete')
    sets = [{Path(name).name for name in found if Path(name).parent.as_posix() == parent} for parent in parents]
    if any(not REQUIRED_FILES.issubset(names) or names != sets[0] for names in sets):
        raise ValueError('Each executable directory requires the same complete CRT set')
    for filename in sets[0]:
        identities = {(entry['sha256'], entry['version'], entry['copyright']) for name, entry in found.items()
                      if Path(name).name == filename}
        if len(identities) != 1:
            raise ValueError('Runtime copies differ between executable directories')
    _, expected_notice = notice_record()
    if data['notices'] != [expected_notice] or expected_notice['path'] not in files \
            or notices.digest(files[expected_notice['path']]) != expected_notice['sha256']:
        raise ValueError('Runtime notice differs from the approved catalog')
    return data


def main():
    parser = argparse.ArgumentParser(description='Stage and verify app-local Windows runtime dependencies')
    commands = parser.add_subparsers(dest='action', required=True)
    deploy = commands.add_parser('stage')
    deploy.add_argument('--bundle', type=Path, required=True)
    deploy.add_argument('--executable', action='append', required=True)
    check = commands.add_parser('verify')
    check.add_argument('--bundle', type=Path, required=True)
    args = parser.parse_args()
    result = stage(args.bundle, args.executable) if args.action == 'stage' else verify(args.bundle)
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
