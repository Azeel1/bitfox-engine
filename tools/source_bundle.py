import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import shutil
import stat
import tarfile
import tempfile
import zipfile

import gui_dependency_notices as notices
import windows_runtime


MANIFEST = "SOURCE_MANIFEST.json"
MAX_MANIFEST = 8 * 1024 * 1024
MAX_ARCHIVE = 2 * 1024 * 1024 * 1024
MAX_CONTENT = 8 * 1024 * 1024 * 1024
MAX_MEMBERS = 100_000
GUI_TARGETS = {
    "linux": ("linux-x86_64", "Bitfox/GUI_DEPENDENCIES.json"),
    "windows": ("windows-x86_64", "Bitfox/GUI_DEPENDENCIES.json"),
    "macos": ("macos-arm64", "Bitfox.app/Contents/Resources/GUI_DEPENDENCIES.json"),
}


def source_name(version):
    from release_metadata import validate_version
    return f"bitfox-{validate_version(version)}-corresponding-source.tar.gz"


def json_data(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError(f"duplicate JSON key: {key}")
            result[key] = value
        return result
    try:
        return json.loads(raw, object_pairs_hook=pairs)
    except (UnicodeError, json.JSONDecodeError) as exc:
        raise ValueError("invalid source metadata JSON") from exc


def package_inventory(path, member_name):
    if path.suffix == ".zip":
        with zipfile.ZipFile(path) as archive:
            members = [m for m in archive.infolist() if m.filename == member_name]
            if len(members) != 1 or not 0 < members[0].file_size <= MAX_MANIFEST:
                raise ValueError(f"missing or invalid packaged dependency inventory: {path.name}")
            raw = archive.read(members[0])
    else:
        raw = None
        with tarfile.open(path, "r:gz") as archive:
            for count, member in enumerate(archive):
                if count >= MAX_MEMBERS:
                    raise ValueError("too many binary package members")
                if member.name == member_name:
                    if raw is not None or not member.isfile() or not 0 < member.size <= MAX_MANIFEST:
                        raise ValueError(f"invalid packaged dependency inventory: {path.name}")
                    raw = archive.extractfile(member).read(MAX_MANIFEST + 1)
        if raw is None:
            raise ValueError(f"missing packaged dependency inventory: {path.name}")
    return json_data(raw)


def windows_engine_inventory(path, version):
    root = f"bitfox-{version}-windows-x86_64"
    required = {"LICENSE", "THIRD_PARTY_NOTICES.txt", "bitfox.exe", "bitfox-avx2.exe",
                windows_runtime.MANIFEST, "licenses/msvc-2022.txt"}
    if path.is_symlink() or not path.is_file() or not 0 < path.stat().st_size < MAX_ARCHIVE:
        raise ValueError("Windows engine package must be a regular ZIP below 2 GiB")
    with tempfile.TemporaryDirectory(prefix="source-runtime-") as temporary:
        destination = Path(temporary).resolve()
        names, files, total = set(), set(), 0
        with zipfile.ZipFile(path) as archive:
            for count, member in enumerate(archive.infolist(), 1):
                if count > MAX_MEMBERS or member.flag_bits & 1:
                    raise ValueError("invalid Windows engine ZIP members")
                name = notices.relative_path(member.filename.removesuffix("/"))
                if name.casefold() in names or (name != root and not name.startswith(root + "/")):
                    raise ValueError("duplicate or out-of-root Windows engine member")
                names.add(name.casefold())
                relative = name[len(root):].lstrip("/")
                mode = stat.S_IFMT(member.external_attr >> 16)
                if member.is_dir():
                    if mode not in (0, stat.S_IFDIR) or relative not in ("", "licenses"):
                        raise ValueError("unexpected Windows engine directory")
                    (destination / name).mkdir(parents=True, exist_ok=True)
                    continue
                if mode not in (0, stat.S_IFREG) or relative not in required | windows_runtime.RUNTIME_FILES:
                    raise ValueError("unexpected or nonregular Windows engine member")
                if not 0 < member.file_size <= 512 * 1024 * 1024:
                    raise ValueError("invalid Windows engine member size")
                if relative == windows_runtime.MANIFEST and member.file_size > MAX_MANIFEST:
                    raise ValueError("Windows runtime inventory exceeds size limit")
                total += member.file_size
                if total > 2 * 1024 * 1024 * 1024:
                    raise ValueError("Windows engine package expands above 2 GiB")
                target = destination / name
                target.parent.mkdir(parents=True, exist_ok=True)
                with archive.open(member) as source, target.open("xb") as output:
                    shutil.copyfileobj(source, output, 1024 * 1024)
                files.add(relative)
        if not required.issubset(files):
            raise ValueError("Windows engine package lacks runtime inventory, notices or executables")
        bundle = destination / root
        json_data((bundle / windows_runtime.MANIFEST).read_bytes())
        inventory = windows_runtime.verify(bundle)
        if set(inventory["executables"]) != {"bitfox.exe", "bitfox-avx2.exe"}:
            raise ValueError("unexpected Windows engine executable inventory")
        return inventory


def requirements(packages, version, source_commit):
    from release_metadata import package_names
    if not isinstance(source_commit, str) or not re.fullmatch(r"[0-9a-f]{40}", source_commit):
        raise ValueError("the exact source commit is required")
    names = package_names(version)
    if set(packages) != set(names):
        raise ValueError("all seven binary packages are required for source binding")
    catalog = json_data((notices.ROOT / "licenses/qt/catalog.json").read_bytes())
    components = {
        "bitfox": {
            "id": "bitfox", "version": version,
            "source": f"https://github.com/Azeel1/bitfox-engine/tree/{source_commit}",
            "roles": ["source", "build", "vendor"],
            "uses": [{"package": name} for name in names],
        }
    }
    for platform, (target, member) in GUI_TARGETS.items():
        name = f"bitfox-{version}-{target}-gui" + (".tar.gz" if platform == "linux" else ".zip")
        data = package_inventory(Path(packages[name]), member)
        if not isinstance(data, dict) or data.get("schema") != 1 or data.get("platform") != platform \
                or not isinstance(data.get("dependencies"), list) or not data["dependencies"]:
            raise ValueError(f"invalid packaged dependency inventory: {name}")
        seen = set()
        for item in data["dependencies"]:
            try:
                path = notices.relative_path(item["path"])
                provider = item["provider"]
                if path in seen:
                    raise ValueError("duplicate packaged dependency")
                seen.add(path)
                if platform == "linux":
                    package, revision = item["source_package"], item["source_version"]
                    if not isinstance(package, str) or not re.fullmatch(r"[a-z0-9][a-z0-9+.-]*", package) \
                            or not isinstance(revision, str) or not re.fullmatch(r"[0-9][A-Za-z0-9.+:~-]*", revision):
                        raise ValueError("invalid Debian source identity")
                    identity = {
                        "id": f"debian/{package}/{revision}", "package": package,
                        "version": revision, "roles": ["source", "build"],
                    }
                else:
                    if notices.official_provider(path, platform) != provider:
                        raise ValueError("incorrect packaged dependency provider")
                    qt_version = {"macos": "6.11.0", "windows": "6.8.3"}[platform]
                    if data.get("qt_version") != qt_version:
                        raise ValueError("unsupported packaged Qt version")
                    key = f"{provider}/{qt_version}" if provider != "msvc" else "msvc/2022"
                    record = catalog[key]
                    identity = {
                        "id": key, "version": record["version"], "source": record["source"],
                        "roles": ["source", "build"],
                    }
                    if provider == "msvc":
                        version_text = item.get("version")
                        if not isinstance(version_text, str) or not re.fullmatch(r"[0-9]+(?:[., ][0-9]+)*", version_text):
                            raise ValueError("missing Microsoft runtime version")
                        identity.update(id=f"msvc/{version_text}", version=version_text, roles=["redistribution"])
                identifier = identity["id"]
                component = components.setdefault(identifier, dict(identity, uses=[]))
                if {k: v for k, v in component.items() if k != "uses"} != identity:
                    raise ValueError("inconsistent source identity")
                component["uses"].append({"package": name, "path": path})
            except (KeyError, TypeError) as exc:
                raise ValueError(f"incomplete packaged dependency: {name}") from exc
    name = f"bitfox-{version}-windows-x86_64.zip"
    runtime = windows_engine_inventory(Path(packages[name]), version)
    for item in runtime["dependencies"]:
        version_text = item["version"]
        identity = {"id": f"msvc/{version_text}", "version": version_text,
                    "source": catalog["msvc/2022"]["source"], "roles": ["redistribution"]}
        component = components.setdefault(identity["id"], dict(identity, uses=[]))
        if {k: v for k, v in component.items() if k != "uses"} != identity:
            raise ValueError("inconsistent source identity")
        component["uses"].append({"package": name, "path": item["path"]})
    return {
        "schema": 1, "version": version, "source_commit": source_commit,
        "packages": {name: notices.digest(Path(packages[name])) for name in names},
        "components": [dict(components[key], uses=sorted(components[key]["uses"], key=lambda v: (v["package"], v.get("path", ""))))
                       for key in sorted(components)],
    }


def component_paths(records, expected):
    components = {item["id"]: item for item in expected["components"]}
    if not isinstance(records, dict) or set(records) != set(components):
        raise ValueError("source component coverage is incomplete")
    referenced = set()
    for identifier, record in records.items():
        if not isinstance(record, dict) or set(record) != set(components[identifier]["roles"]):
            raise ValueError(f"source and build evidence is incomplete: {identifier}")
        for role, paths in record.items():
            if not isinstance(paths, list) or not paths or not all(isinstance(p, str) for p in paths) \
                    or len(set(paths)) != len(paths):
                raise ValueError(f"missing {role} files for {identifier}")
            for path in paths:
                if notices.relative_path(path) != path or not path.startswith("payload/"):
                    raise ValueError("source payload paths must be below payload/")
                referenced.add(path)
    return referenced


def validate_manifest(data, expected):
    if not isinstance(data, dict) or set(data) != {"requirements", "components", "files"} \
            or data["requirements"] != expected or not isinstance(data["files"], dict):
        raise ValueError("source manifest does not match the binary packages or source identities")
    referenced = component_paths(data["components"], expected)
    if referenced != set(data["files"]):
        raise ValueError("source payload inventory differs from component coverage")
    total = 0
    for path, record in data["files"].items():
        if not isinstance(record, dict) or set(record) != {"sha256", "bytes"} \
                or not isinstance(record["sha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", record["sha256"]) \
                or type(record["bytes"]) is not int or not 0 < record["bytes"] <= MAX_CONTENT:
            raise ValueError(f"invalid source payload digest or size: {path}")
        total += record["bytes"]
    if total > MAX_CONTENT or len(data["files"]) >= MAX_MEMBERS:
        raise ValueError("source payload exceeds the bundle limits")


def verify_archive(path, packages, version, *, expected_commit=None):
    if path.is_symlink() or not path.is_file() or not 0 < path.stat().st_size < MAX_ARCHIVE:
        raise ValueError("source archive must be a regular file below 2 GiB")
    with tarfile.open(path, "r:gz") as archive:
        member = archive.next()
        if member is None or member.name != MANIFEST or not member.isfile() or not 0 < member.size <= MAX_MANIFEST:
            raise ValueError("source archive must start with SOURCE_MANIFEST.json")
        data = json_data(archive.extractfile(member).read(MAX_MANIFEST + 1))
        try:
            commit = data["requirements"]["source_commit"]
        except (KeyError, TypeError) as exc:
            raise ValueError("source manifest has no commit") from exc
        if expected_commit is not None and commit != expected_commit:
            raise ValueError("source manifest differs from the release commit")
        expected = requirements(packages, version, commit)
        validate_manifest(data, expected)
        found = set()
        for member in archive:
            if member.name == MANIFEST and not found and member.offset == 0:
                continue
            if not member.isfile() or member.name in found or member.name not in data["files"]:
                raise ValueError("unexpected, duplicate or nonregular source archive member")
            record = data["files"][member.name]
            if member.size != record["bytes"]:
                raise ValueError("source archive member size differs")
            digest = hashlib.sha256()
            with archive.extractfile(member) as source:
                for block in iter(lambda: source.read(1024 * 1024), b""):
                    digest.update(block)
            if digest.hexdigest() != record["sha256"]:
                raise ValueError("source archive member checksum differs")
            found.add(member.name)
        if found != set(data["files"]):
            raise ValueError("source archive is missing payload files")
    return data


def create_archive(packages, version, commit, plan, root, output):
    expected = requirements(packages, version, commit)
    if not isinstance(plan, dict) or set(plan) != {"components"}:
        raise ValueError("source plan must contain explicit component file lists")
    data = {"requirements": expected, "components": plan["components"], "files": {}}
    for relative in component_paths(data["components"], expected):
        path = root / relative
        if path.is_symlink() or not path.is_file() or not path.resolve().is_relative_to(root.resolve()):
            raise ValueError(f"source input is missing or unsafe: {relative}")
        data["files"][relative] = {"sha256": notices.digest(path), "bytes": path.stat().st_size}
    validate_manifest(data, expected)
    raw = (json.dumps(data, indent=2, sort_keys=True) + "\n").encode()
    if len(raw) > MAX_MANIFEST:
        raise ValueError("source manifest exceeds the size limit")
    checksum = output.with_name(output.name + ".sha256")
    if output.name != source_name(version) or output.exists() or checksum.exists() \
            or output.is_symlink() or checksum.is_symlink():
        raise ValueError("source output must use the release name and must not already exist")
    with tempfile.TemporaryDirectory(prefix="source-bundle-", dir=output.parent) as temporary:
        partial = Path(temporary) / output.name
        with partial.open("wb") as destination, gzip.GzipFile(filename="", mode="wb", fileobj=destination, mtime=0) as compressed, \
                tarfile.open(fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT) as archive:
            header = tarfile.TarInfo(MANIFEST)
            header.mode, header.size = 0o644, len(raw)
            archive.addfile(header, io.BytesIO(raw))
            for relative, record in sorted(data["files"].items()):
                header = tarfile.TarInfo(relative)
                header.mode, header.size = 0o644, record["bytes"]
                with (root / relative).open("rb") as source:
                    archive.addfile(header, source)
        verify_archive(partial, packages, version)
        digest = notices.digest(partial)
        partial.rename(output)
        checksum.write_text(f"{digest}  {output.name}\n", encoding="utf-8")
    return output


def binary_packages(directory, version):
    from release_metadata import package_names
    result = {}
    for name in package_names(version):
        matches = list(directory.rglob(name))
        if len(matches) != 1 or matches[0].is_symlink() or not matches[0].is_file():
            raise ValueError(f"expected exactly one binary package: {name}")
        result[name] = matches[0]
    return result


def main():
    parser = argparse.ArgumentParser(description="Bind local source material and build recipes to frozen release packages")
    parser.add_argument("action", choices=["requirements", "create", "verify"])
    parser.add_argument("version")
    parser.add_argument("--packages", required=True, type=Path)
    parser.add_argument("--commit")
    parser.add_argument("--plan", type=Path)
    parser.add_argument("--root", type=Path)
    parser.add_argument("--archive", type=Path)
    args = parser.parse_args()
    try:
        packages = binary_packages(args.packages, args.version)
        if args.action == "requirements":
            print(json.dumps(requirements(packages, args.version, args.commit), indent=2, sort_keys=True))
        elif args.action == "create":
            if not all([args.plan, args.root, args.archive]):
                raise ValueError("create requires --plan, --root and --archive")
            print(create_archive(packages, args.version, args.commit, json_data(args.plan.read_bytes()), args.root, args.archive))
        else:
            if not args.archive:
                raise ValueError("verify requires --archive")
            verify_archive(args.archive, packages, args.version, expected_commit=args.commit)
            print("Source inventory, declared provenance, payload hashes and package bindings verified")
    except (OSError, ValueError, TypeError, KeyError, tarfile.TarError, zipfile.BadZipFile) as exc:
        parser.error(str(exc))


if __name__ == "__main__":
    main()
