import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess


ROOT = Path(__file__).resolve().parents[1]
BASE_MODULES = {
    "Core", "Gui", "Widgets", "Network", "Concurrent", "DBus", "Sql",
    "Test", "OpenGL", "OpenGLWidgets", "PrintSupport", "Xml",
}
MULTIMEDIA_MODULES = {"Multimedia", "MultimediaWidgets", "SpatialAudio"}
BASE_PLUGINS = {
    "qwindows", "qminimal", "qoffscreen", "qdirect2d", "qcocoa",
    "qgif", "qico", "qjpeg", "qnetworklistmanager", "qglib",
    "qmodernwindowsstyle", "qwindowsvistastyle", "qmacstyle",
    "qschannelbackend", "qopensslbackend", "qcertonlybackend",
    "qsecuretransportbackend",
    "qtuiotouchplugin",
}
SVG_MODULES = {"Svg", "SvgWidgets"}
SVG_PLUGINS = {"qsvg", "qsvgicon"}
MULTIMEDIA_PLUGINS = {"ffmpegmediaplugin", "windowsmediaplugin", "darwinmediaplugin"}
MSVC_FILES = {
    "vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll", "msvcp140_1.dll",
    "msvcp140_2.dll", "msvcp140_atomic_wait.dll", "msvcp140_codecvt_ids.dll",
    "concrt140.dll",
    "vc_redist.x64.exe", "vc_redist.x86.exe", "vc_redist.arm64.exe",
}
FFMPEG_7_MAJORS = {
    "avcodec": 61, "avdevice": 61, "avfilter": 10, "avformat": 61,
    "avutil": 59, "swresample": 5, "swscale": 8,
}


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def relative_path(value):
    if not isinstance(value, str) or not value or "\\" in value:
        raise ValueError("invalid dependency path")
    parts = value.split("/")
    if any(not part or part in (".", "..") for part in parts) or value.startswith("/"):
        raise ValueError("invalid dependency path")
    if any(ord(c) < 32 for c in value) or re.match(r"^[A-Za-z]:", value):
        raise ValueError("invalid dependency path")
    return PurePosixPath(value).as_posix()


def dependency_paths(bundle, platform):
    result = []
    for path in sorted(bundle.rglob("*")):
        if path.is_symlink() or not path.is_file():
            continue
        name = path.name
        rel = path.relative_to(bundle).as_posix()
        if platform == "windows":
            included = (name.lower().endswith(".dll") and rel.lower() != "bitfox.dll") or (
                name.lower().endswith(".exe") and rel.lower() != "bitfox-board.exe"
                and not rel.startswith("engines/")
            )
        elif platform == "linux":
            included = ".so" in name and rel != "lib/libbitfox.so"
        else:
            included = (name.endswith(".dylib") and rel != "Contents/MacOS/libbitfox.dylib") or bool(
                re.fullmatch(r"Contents/Frameworks/([^/.]+)\.framework/Versions/[^/]+/\1", rel)
            )
        if included:
            result.append(rel)
    return result


def official_provider(path, platform):
    name = PurePosixPath(path).name
    module = re.fullmatch(r"Qt6([A-Za-z0-9]+)\.dll", name) if platform == "windows" else None
    if platform == "macos":
        module = re.fullmatch(r"Qt([A-Za-z0-9]+)", name)
    if module:
        value = module.group(1)
        if value in BASE_MODULES:
            return "qtbase"
        if value in MULTIMEDIA_MODULES:
            return "qtmultimedia"
        if value in SVG_MODULES:
            return "qtsvg"
    plugin = name.removeprefix("lib").removesuffix(".dylib").removesuffix(".dll")
    if plugin in BASE_PLUGINS:
        return "qtbase"
    if plugin in MULTIMEDIA_PLUGINS:
        return "qtmultimedia"
    if plugin in SVG_PLUGINS:
        return "qtsvg"
    match = re.fullmatch(r"([a-z]+)-(\d+)\.dll", name.lower())
    if match and FFMPEG_7_MAJORS.get(match.group(1)) == int(match.group(2)):
        return "ffmpeg"
    if name.lower() in MSVC_FILES:
        return "msvc"
    raise ValueError(f"no verified notice coverage for dependency {path}")


def ffmpeg_version(bundle):
    path = bundle / "avutil-59.dll"
    if not path.is_file():
        raise ValueError("FFmpeg backend is missing avutil-59.dll")
    with os.add_dll_directory(str(bundle.resolve())):
        library = ctypes.CDLL(str(path.resolve()))
        for name in ("av_version_info", "avutil_license", "avutil_configuration"):
            getattr(library, name).restype = ctypes.c_char_p
        version = library.av_version_info().decode("utf-8")
        license_text = library.avutil_license().decode("utf-8")
        configuration = library.avutil_configuration().decode("utf-8")
    if version not in {"7.1", "n7.1"} or license_text != "LGPL version 2.1 or later":
        raise ValueError("FFmpeg build differs from the approved Qt 6.8.3 LGPL 2.1 build")
    if any(flag in configuration for flag in ("--enable-gpl", "--enable-nonfree", "--enable-version3")):
        raise ValueError("FFmpeg configuration has unapproved licensing options")
    return version.removeprefix("n")


def microsoft_metadata(path):
    environment = dict(os.environ, BITFOX_RUNTIME_NOTICE_INPUT=str(path.resolve()))
    command = "[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); " \
        "$v = (Get-Item -LiteralPath $env:BITFOX_RUNTIME_NOTICE_INPUT).VersionInfo; " \
        "@{version=$v.FileVersion;copyright=$v.LegalCopyright} | ConvertTo-Json -Compress"
    result = subprocess.check_output(["powershell", "-NoProfile", "-NonInteractive", "-Command", command],
                                     env=environment, text=True, encoding="utf-8-sig")
    value = json.loads(result)
    if not isinstance(value.get("version"), str) or not value["version"].strip() \
            or not isinstance(value.get("copyright"), str) or "Microsoft" not in value["copyright"]:
        raise ValueError(f"missing Microsoft runtime version or copyright: {path.name}")
    return value


def write_manifest(bundle, platform, dependencies, notices, qt_version=None):
    prefix = "Contents/Resources/" if platform == "macos" else ""
    manifest_path = bundle / (prefix + "GUI_DEPENDENCIES.json")
    data = {
        "schema": 1, "platform": platform, "qt_version": qt_version,
        "dependencies": dependencies, "notices": notices,
    }
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    manifest_path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    verify(bundle)
    return manifest_path


def collect_official(bundle, platform, qt_version, notice_root=ROOT / "licenses/qt"):
    expected = {"macos": "6.11.0", "windows": "6.8.3"}[platform]
    if qt_version != expected:
        raise ValueError(f"Qt version {qt_version!r} has no approved {platform} notices")
    catalog = json.loads((notice_root / "catalog.json").read_text(encoding="utf-8"))
    entries = []
    providers = set()
    for rel in dependency_paths(bundle, platform):
        provider = official_provider(rel, platform)
        if platform == "macos" and provider in {"ffmpeg", "msvc"}:
            raise ValueError(f"unexpected macOS dependency {rel}")
        providers.add(provider)
        entry = {"path": rel, "provider": provider}
        if platform == "macos":
            entry["integrity"] = "bundle-codesign"
        else:
            entry["sha256"] = digest(bundle / rel)
        if provider == "msvc":
            entry.update(microsoft_metadata(bundle / rel))
        entries.append(entry)
    if not {"qtbase", "qtmultimedia"}.issubset(providers):
        raise ValueError("packaged Qt Base and Multimedia dependencies are required")
    ffmpeg_plugin = any(PurePosixPath(v["path"]).name.lower() == "ffmpegmediaplugin.dll" for v in entries)
    if ffmpeg_plugin and "ffmpeg" not in providers:
        raise ValueError("FFmpeg media plugin is missing its runtime libraries")
    if "ffmpeg" in providers:
        version = ffmpeg_version(bundle)
        for entry in entries:
            if entry["provider"] == "ffmpeg":
                entry["version"] = version
    prefix = "Contents/Resources/" if platform == "macos" else ""
    notices = []
    for provider in sorted(providers):
        key = f"{provider}/{qt_version}" if provider != "msvc" else "msvc/2022"
        item = catalog[key]
        source = notice_root / relative_path(item["path"])
        if source.is_symlink() or digest(source) != item["sha256"]:
            raise ValueError(f"source notice differs from its catalog: {key}")
        rel = prefix + "licenses/" + source.name
        dst = bundle / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, dst)
        notices.append({"path": rel, "sha256": digest(dst), "provider": provider,
                        "version": item["version"], "source": item["source"]})
    return write_manifest(bundle, platform, entries, notices, qt_version)


def debian_owner(path):
    owners = set()
    for candidate in dict.fromkeys((str(path), str(path.resolve()))):
        result = subprocess.run(["dpkg-query", "-S", candidate], capture_output=True,
                                text=True, check=False)
        if result.returncode:
            continue
        for line in result.stdout.splitlines():
            owner, separator, found = line.rpartition(": ")
            if separator and found == candidate:
                owners.update(owner.split(", "))
    if len(owners) != 1:
        raise ValueError(f"expected one Debian package owner for {path.name}")
    owner = owners.pop()
    if not re.fullmatch(r"[a-z0-9][a-z0-9+.-]*(?::[a-z0-9-]+)?", owner):
        raise ValueError("invalid Debian package name")
    fields = subprocess.check_output([
        "dpkg-query", "-W", "-f=${Version}\t${source:Package}\t${source:Version}", owner,
    ], text=True).strip().split("\t")
    if len(fields) != 3:
        raise ValueError(f"missing Debian source metadata for {owner}")
    version, source_package, source_version = fields
    if not re.fullmatch(r"[a-z0-9][a-z0-9+.-]*", source_package) or any(
            not re.fullmatch(r"[0-9][A-Za-z0-9.+:~-]*", value)
            for value in (version, source_version)):
        raise ValueError(f"invalid Debian source metadata for {owner}")
    return owner, version, source_package, source_version


def collect_debian(bundle, origins, doc_root=Path("/usr/share/doc"),
                   common_root=Path("/usr/share/common-licenses")):
    dependencies = []
    notices = {}
    seen = set()
    for source, rel in origins:
        rel = relative_path(rel)
        if rel in seen:
            raise ValueError(f"duplicate dependency origin {rel}")
        seen.add(rel)
        owner, version, source_package, source_version = debian_owner(Path(source))
        provider = owner.split(":", 1)[0]
        notice_path = f"licenses/{provider}-copyright.txt"
        src = doc_root / provider / "copyright"
        if not src.is_file() or not src.stat().st_size:
            raise ValueError(f"missing Debian copyright for {owner}")
        text = src.read_text(encoding="utf-8")
        dst = bundle / notice_path
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src, dst)
        notices[notice_path] = {"path": notice_path, "sha256": digest(dst),
                                "provider": provider, "version": version,
                                "source": f"https://packages.ubuntu.com/jammy/{provider}"}
        for license_name in set(re.findall(r"/usr/share/common-licenses/([A-Za-z0-9_.+-]+)", text)):
            license_name = license_name.rstrip(".")
            license_source = common_root / license_name
            if not license_source.is_file() or not license_source.stat().st_size:
                raise ValueError(f"missing common license {license_name}")
            license_rel = f"licenses/{license_name}.txt"
            shutil.copyfile(license_source, bundle / license_rel)
            notices[license_rel] = {"path": license_rel, "sha256": digest(bundle / license_rel),
                                    "provider": provider, "version": version,
                                    "source": "https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/"}
        dependencies.append({"path": rel, "sha256": digest(bundle / rel),
                             "provider": provider, "binary_package": owner,
                             "version": version, "source_package": source_package,
                             "source_version": source_version})
    if seen != set(dependency_paths(bundle, "linux")):
        raise ValueError("packaged Linux dependencies differ from collected origins")
    if not any(v["provider"].startswith("libqt6") for v in dependencies):
        raise ValueError("packaged Qt dependencies are required")
    return write_manifest(bundle, "linux", dependencies, sorted(notices.values(), key=lambda v:v["path"]))


def verify(bundle):
    prefix = "Contents/Resources/" if (bundle / "Contents").is_dir() else ""
    manifest = bundle / (prefix + "GUI_DEPENDENCIES.json")
    if manifest.is_symlink():
        raise ValueError("dependency manifest must be a regular file")
    data = json.loads(manifest.read_text(encoding="utf-8"))
    if data.get("schema") != 1 or data.get("platform") not in {"macos", "windows", "linux"}:
        raise ValueError("invalid GUI dependency manifest")
    paths = set()
    providers = set()
    for group in ["dependencies", "notices"]:
        if not isinstance(data.get(group), list) or not data[group]:
            raise ValueError(f"missing GUI {group}")
        for entry in data[group]:
            rel = relative_path(entry["path"])
            if rel in paths:
                raise ValueError(f"duplicate GUI dependency member {rel}")
            paths.add(rel)
            path = bundle / rel
            if path.is_symlink() or not path.resolve().is_relative_to(bundle.resolve()) \
                    or not path.is_file() or not path.stat().st_size:
                raise ValueError(f"missing required GUI dependency member {rel}")
            if group == "dependencies" and data["platform"] == "macos":
                if entry.get("integrity") != "bundle-codesign":
                    raise ValueError("invalid signed-bundle integrity policy")
            elif digest(path) != entry["sha256"]:
                raise ValueError(f"GUI dependency checksum differs: {rel}")
            if group == "notices":
                if not rel.startswith(prefix + "licenses/") or not entry["source"].startswith("https://"):
                    raise ValueError("invalid GUI notice source or path")
                providers.add(entry["provider"])
            elif data["platform"] in {"macos", "windows"}:
                if official_provider(rel, data["platform"]) != entry["provider"]:
                    raise ValueError("GUI dependency has an incorrect notice provider")
            elif not isinstance(entry.get("binary_package"), str) or not re.fullmatch(
                    r"[a-z0-9][a-z0-9+.-]*(?::[a-z0-9-]+)?", entry["binary_package"]) \
                    or entry["binary_package"].split(":", 1)[0] != entry["provider"] \
                    or not isinstance(entry.get("source_package"), str) or not re.fullmatch(
                        r"[a-z0-9][a-z0-9+.-]*", entry["source_package"]) \
                    or any(not isinstance(entry.get(key), str) or not re.fullmatch(
                        r"[0-9][A-Za-z0-9.+:~-]*", entry[key])
                        for key in ("version", "source_version")):
                raise ValueError("GUI dependency has invalid Debian source metadata")
    expected = set(dependency_paths(bundle, data["platform"]))
    if expected != {v["path"] for v in data["dependencies"]}:
        raise ValueError("GUI dependency inventory is incomplete")
    if not {v["provider"] for v in data["dependencies"]}.issubset(providers):
        raise ValueError("GUI dependency is missing its license notice")
    if data["platform"] in {"macos", "windows"}:
        expected_qt = {"macos": "6.11.0", "windows": "6.8.3"}[data["platform"]]
        if data.get("qt_version") != expected_qt:
            raise ValueError("GUI manifest has an unsupported Qt version")
        catalog = json.loads((ROOT / "licenses/qt/catalog.json").read_text(encoding="utf-8"))
        for entry in data["notices"]:
            key = f'{entry["provider"]}/{expected_qt}' if entry["provider"] != "msvc" else "msvc/2022"
            item = catalog[key]
            if entry["sha256"] != item["sha256"] or entry["version"] != item["version"] \
                    or entry["source"] != item["source"]:
                raise ValueError("GUI notice differs from the approved catalog")
    return data


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="action", required=True)
    official = sub.add_parser("official")
    official.add_argument("--bundle", type=Path, required=True)
    official.add_argument("--platform", choices=["macos", "windows"], required=True)
    official.add_argument("--qt-version", required=True)
    debian = sub.add_parser("debian")
    debian.add_argument("--bundle", type=Path, required=True)
    debian.add_argument("--origins", type=Path, required=True)
    check = sub.add_parser("verify")
    check.add_argument("--bundle", type=Path, required=True)
    args = parser.parse_args()
    if args.action == "official":
        collect_official(args.bundle, args.platform, args.qt_version)
    elif args.action == "debian":
        origins = [line.split("\t") for line in args.origins.read_text().splitlines()]
        if any(len(row) != 2 for row in origins):
            raise ValueError("invalid dependency origins")
        collect_debian(args.bundle, origins)
    else:
        verify(args.bundle)


if __name__ == "__main__":
    main()
