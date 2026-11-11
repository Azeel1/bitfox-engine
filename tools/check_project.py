import argparse
import hashlib
import json
from pathlib import Path
import re
import tomllib


ROOT = Path(__file__).resolve().parents[1]


def read_toml(path):
    return tomllib.loads(path.read_text(encoding='utf-8'))


def validate(root):
    root = Path(root).resolve()

    engine = read_toml(root / 'engine/Cargo.toml')
    package = engine['package']
    version = package['version']
    if re.fullmatch(r'(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)', version) is None:
        raise ValueError('Engine version must be a stable semantic version')

    lock = read_toml(root / 'engine/Cargo.lock')
    locked = [p for p in lock['package'] if p['name'] == 'bitfox']
    if len(locked) != 1 or locked[0]['version'] != version:
        raise ValueError('Engine manifest and lock versions differ')

    toolchain = read_toml(root / 'rust-toolchain.toml')['toolchain']['channel']
    if package['rust-version'] != '.'.join(toolchain.split('.')[:2]):
        raise ValueError('Engine Rust requirement differs from the pinned toolchain')

    metadata = json.loads((root / 'engine/networks/manifest.json').read_text())
    if metadata['schema_version'] != 1 or metadata['file'] != 'bitfox.nnue':
        raise ValueError('Unsupported network manifest')

    net = root / 'engine/networks' / metadata['file']
    digest = hashlib.sha256(net.read_bytes()).hexdigest()
    if net.stat().st_size != metadata['bytes'] or digest != metadata['sha256']:
        raise ValueError('Active network does not match its manifest')

    contract = root / 'engine/src/eval/contract.rs'
    text = contract.read_text()
    if hashlib.sha256(contract.read_bytes()).hexdigest() != metadata['contract_sha256']:
        raise ValueError('NNUE contract changed without a matching network manifest')

    mapping = {
        'HL': 'hidden_size',
        'INPUT_BUCKETS': 'input_buckets',
        'OUTPUT_BUCKETS': 'output_buckets',
        'QA': 'qa',
        'QB': 'qb',
        'SCALE': 'scale',
    }
    architecture = metadata['architecture']
    for constant, name in mapping.items():
        match = re.search(r'pub const ' + constant + r':\s*\w+\s*=\s*(\d+)\s*;', text)
        if match is None or int(match[1]) != architecture[name]:
            raise ValueError('NNUE manifest has the wrong ' + name)

    if architecture['piece_features'] != 768 or architecture['features'] != 768 * architecture['input_buckets']:
        raise ValueError('NNUE input feature count mismatch')

    h = architecture['hidden_size']
    o = architecture['output_buckets']
    f = architecture['features']
    expected_bytes = ((f * h + h + o * 2 * h + o) * 2 + 63) & ~63
    if expected_bytes != metadata['bytes']:
        raise ValueError('NNUE manifest size does not match its architecture')

    targets = read_toml(root / 'engine/.cargo/config.toml').get('target', {})
    for target in ['x86_64-unknown-linux-gnu', 'x86_64-pc-windows-gnu', 'x86_64-pc-windows-msvc']:
        flags = targets.get(target, {}).get('rustflags', [])
        has_v2 = (
            '-Ctarget-cpu=x86-64-v2' in flags
            or any(flags[idx:idx + 2] == ['-C', 'target-cpu=x86-64-v2'] for idx in range(len(flags)))
        )
        if not has_v2:
            raise ValueError('Missing baseline CPU configuration for ' + target)

    trainer = read_toml(root / 'trainer/Cargo.toml')
    bullet = trainer['dependencies']['bullet_lib']
    revision = bullet.get('rev', '')
    if re.fullmatch(r'[0-9a-f]{40}', revision) is None or 'branch' in bullet:
        raise ValueError('Trainer Bullet dependency must use an exact commit')

    trainer_lock = read_toml(root / 'trainer/Cargo.lock')
    entries = [p for p in trainer_lock['package'] if p['name'] == 'bullet_lib']
    expected_source = 'git+' + bullet['git'] + '?rev=' + revision + '#' + revision
    if len(entries) != 1 or entries[0]['source'] != expected_source:
        raise ValueError('Trainer manifest and lock disagree on Bullet')

    for path in ['trainer/Cargo.toml', 'trainer/convert/Cargo.toml']:
        info = read_toml(root / path)['package']
        if info.get('rust-version') != package['rust-version'] or info.get('license') != package['license']:
            raise ValueError('Rust requirement or project license differs in ' + path)

    header = root / 'engine/include/bitfox_core.h'
    if not header.is_file() or 'CC_ABI_VERSION = 1' not in header.read_text():
        raise ValueError('Canonical C ABI header missing or version changed')

    if (root / 'gui-qt/include/bitfox_core.h').read_text().strip() != '#include "../../engine/include/bitfox_core.h"':
        raise ValueError('Qt compatibility header must forward to the canonical header')

    return {
        'version': version,
        'toolchain': toolchain,
        'network_sha256': digest,
        'network_bytes': metadata['bytes'],
        'bullet_revision': revision,
        'c_abi_version': 1,
    }


def main():
    parser = argparse.ArgumentParser(description='Check shared project versions, network contract and pinned training dependencies.')
    parser.add_argument('--root', type=Path, default=ROOT)
    args = parser.parse_args()
    try:
        print(json.dumps(validate(args.root), indent=2))
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.error(str(error))


if __name__ == '__main__':
    main()
