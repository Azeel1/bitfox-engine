import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
from urllib.parse import quote, unquote, urlencode, urlsplit
from urllib.request import urlopen


API = 'https://api.launchpad.net/1.0'
ARCHIVE = API + '/ubuntu/+archive/primary'
JAMMY = API + '/ubuntu/jammy'
ARCHIVE_HOSTS = {
    'launchpad.net', 'launchpadlibrarian.net', 'archive.ubuntu.com',
    'security.ubuntu.com', 'ports.ubuntu.com', 'old-releases.ubuntu.com',
}
FILE = re.compile(r'[A-Za-z0-9._+~-]+')
CHECKSUM = re.compile(r'[0-9a-f]{64}')


def get_json(url, params):
    with urlopen(url + '?' + urlencode(params), timeout=30) as response:
        return json.loads(response.read(4 * 1024 * 1024))


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def filename(url):
    parsed = urlsplit(url)
    name = unquote(parsed.path.rsplit('/', 1)[-1])
    if parsed.scheme != 'https' or parsed.hostname not in ARCHIVE_HOSTS:
        raise ValueError('Source file URL is outside the Ubuntu archive hosts')
    if FILE.fullmatch(name) is None or name in {'.', '..'}:
        raise ValueError('Invalid source file name')
    return name


def download(url, directory, expected=None):
    target = directory / filename(url)
    if target.exists():
        if target.is_symlink() or not target.is_file():
            raise ValueError('Invalid existing source archive')
    else:
        if shutil.disk_usage(directory).free < 1024 * 1024 * 1024:
            raise RuntimeError('Insufficient space for further source archives')
        temporary = directory / ('.' + target.name + '.download')
        if temporary.exists():
            raise ValueError('An incomplete source download requires review')
        subprocess.run([
            'curl', '--fail', '--location', '--silent', '--show-error',
            '--proto', '=https', '--proto-redir', '=https', '--connect-timeout', '20',
            '--max-time', '1800', '--limit-rate', '1m', '--output', str(temporary), url,
        ], check=True)
        if expected is not None:
            verify(temporary, expected)
        temporary.rename(target)
    if expected is not None:
        verify(target, expected)
    return target


def verify(path, expected):
    sha256, size = expected
    if path.stat().st_size != size or digest(path) != sha256:
        raise ValueError('Source checksum or size differs: ' + path.name)


def parse_dsc(path, name, version):
    if path.stat().st_size > 256 * 1024:
        raise ValueError('Source control file is too large')
    lines = path.read_text(encoding='utf-8').splitlines()
    if 'Source: ' + name not in lines or 'Version: ' + version not in lines:
        raise ValueError('Source package name or version differs from the binary inventory')
    index = lines.index('Checksums-Sha256:')
    records = {}
    for line in lines[index + 1:]:
        if not line.startswith(' '):
            break
        sha256, size, member = line.split()
        if CHECKSUM.fullmatch(sha256) is None or FILE.fullmatch(member) is None:
            raise ValueError('Invalid source checksum record')
        if member in records or member in {'.', '..'} or not 0 < int(size) <= 512 * 1024 * 1024:
            raise ValueError('Invalid or duplicate source archive')
        records[member] = sha256, int(size)
    if not records:
        raise ValueError('Source control file has no SHA-256 inventory')
    return records


def collect(component, output):
    name, version = component['package'], component['version']
    data = get_json(ARCHIVE, {'ws.op': 'getPublishedSources', 'source_name': name,
                            'version': version, 'exact_match': 'true', 'distro_series': JAMMY})
    publications = [entry for entry in data['entries']
                    if entry['source_package_name'] == name and entry['source_package_version'] == version
                    and entry['status'] in {'Published', 'Superseded'}]
    if not publications:
        raise ValueError('No exact Jammy source publication found')
    publications.sort(key=lambda entry: entry['status'] != 'Published')
    publication = publications[0]['self_link']
    if not publication.startswith(ARCHIVE + '/+sourcepub/'):
        raise ValueError('Unexpected Ubuntu source publication')
    urls = get_json(publication, {'ws.op': 'sourceFileUrls'})
    by_name = {filename(url): url for url in urls}
    descriptors = [file for file in by_name if file.endswith('.dsc')]
    if len(descriptors) != 1:
        raise ValueError('Expected one source control file')
    directory = output / name / quote(version, safe='._+-~')
    directory.mkdir(parents=True, exist_ok=True)
    control = download(by_name[descriptors[0]], directory)
    expected = parse_dsc(control, name, version)
    files = [control]
    for member, checksum in sorted(expected.items()):
        files.append(download(by_name[member], directory, checksum))
    return {
        'component': component['id'], 'publication': publication,
        'files': [{'path': str(path.relative_to(output)), 'sha256': digest(path),
                   'bytes': path.stat().st_size} for path in files],
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--requirements', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    requirements = json.loads(args.requirements.read_text())
    components = [item for item in requirements['components'] if item['id'].startswith('debian/')]
    args.output.mkdir(parents=True, exist_ok=True)
    receipt = args.output / 'receipt.json'
    records = []
    with ThreadPoolExecutor(max_workers=3) as executor:
        futures = [(item, executor.submit(collect, item, args.output)) for item in components]
        for item, future in futures:
            try:
                record = future.result()
            except Exception as error:
                record = {'component': item['id'], 'error': str(error)}
            records.append(record)
            receipt.write_text(json.dumps(records, indent=2) + '\n')
            print(item['id'], record.get('error', 'verified'), flush=True)
    if any('error' in record for record in records):
        raise SystemExit(1)


if __name__ == '__main__':
    main()
