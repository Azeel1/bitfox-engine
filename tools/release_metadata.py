import argparse
import hashlib
from pathlib import Path
import re
import tarfile
import tempfile
import zipfile


PACKAGE_SUFFIXES = (
    "linux-x86_64.tar.gz",
    "linux-aarch64.tar.gz",
    "macos-arm64.tar.gz",
    "windows-x86_64.zip",
    "linux-x86_64-gui.tar.gz",
    "macos-arm64-gui.zip",
    "windows-x86_64-gui.zip",
)


def validate_version(version):
    if not isinstance(version, str) or re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", version) is None:
        raise ValueError("release version must be a stable major.minor.patch version")
    if version == "0.0.0":
        raise ValueError("release version must be above 0.0.0")
    return version


def package_names(version):
    validate_version(version)
    return sorted(f"bitfox-{version}-{suffix}" for suffix in PACKAGE_SUFFIXES)


def create_checksums(directory, version, *, require_source=False, source_commit=None):
    directory = Path(directory)
    names = package_names(version)
    from source_bundle import source_name, verify_archive

    if source_commit is not None and not re.fullmatch(r"[0-9a-f]{40}", source_commit):
        raise ValueError("the exact source commit is required")
    if require_source and source_commit is None:
        raise ValueError("source verification requires the release source commit")
    source = source_name(version)
    allowed = set(names) | {name + ".sha256" for name in names}
    optional = {source, source + ".sha256"}
    files = {}
    for path in directory.rglob("*"):
        if path.is_symlink():
            raise ValueError(f"release assets cannot contain symlinks: {path.name}")
        if path.is_dir():
            continue
        if path == directory / "SHA256SUMS":
            continue
        if not path.is_file() or path.name not in allowed | optional or path.name in files:
            raise ValueError(f"unexpected or duplicate release asset: {path.name}")
        files[path.name] = path
    source_members = optional & set(files)
    if source_members:
        allowed |= optional
    elif require_source or source_commit is not None:
        raise ValueError("missing verified corresponding-source archive")
    if set(files) != allowed:
        raise ValueError("missing release assets: " + ", ".join(sorted(allowed - set(files))))
    if source_members:
        verify_archive(files[source], {name: files[name] for name in names}, version,
                       expected_commit=source_commit)
        names = sorted([*names, source])
    records = []
    for name in names:
        path = files[name]
        if path.stat().st_size <= 0:
            raise ValueError(f"empty release package: {name}")
        digest = hashlib.sha256()
        with path.open("rb") as source:
            for block in iter(lambda: source.read(1024 * 1024), b""):
                digest.update(block)
        expected = f"{digest.hexdigest()}  {name}"
        recorded = files[name + ".sha256"].read_text(encoding="utf-8").strip()
        if recorded != expected:
            raise ValueError(f"checksum mismatch or invalid checksum record: {name}")
        records.append(expected)
    output = "\n".join(records) + "\n"
    destination = directory / "SHA256SUMS"
    with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=directory,
                                     prefix=".checksums-", delete=False) as partial:
        temporary = Path(partial.name)
        try:
            partial.write(output)
            partial.close()
            temporary.replace(destination)
        finally:
            temporary.unlink(missing_ok=True)
    return destination


def main():
    parser = argparse.ArgumentParser(description="Verify release assets and prepare updater checksums")
    parser.add_argument("version")
    parser.add_argument("--directory")
    parser.add_argument("--require-source", action="store_true")
    parser.add_argument("--source-commit")
    args = parser.parse_args()
    try:
        if args.directory:
            print(create_checksums(args.directory, args.version, require_source=args.require_source,
                                   source_commit=args.source_commit))
        else:
            if args.require_source or args.source_commit is not None:
                raise ValueError("source verification requires --directory")
            print(validate_version(args.version))
    except (OSError, UnicodeError, ValueError, tarfile.TarError, zipfile.BadZipFile) as exc:
        parser.error(str(exc))


if __name__ == "__main__":
    main()
