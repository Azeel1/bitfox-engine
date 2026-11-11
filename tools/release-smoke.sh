#!/usr/bin/env bash
set -euo pipefail

repo="${REPO:-Azeel1/bitfox-engine}"
if [[ $# -gt 0 ]]; then
    tag="$1"
else
    version_from_cargo="$(awk -F'"' '/^version = / {print $2; exit}' engine/Cargo.toml)"
    tag="v${version_from_cargo}"
fi
version="${tag#v}"
if [[ ! "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z][0-9A-Za-z.-]*)?([+][0-9A-Za-z][0-9A-Za-z.-]*)?$ ]]; then
    echo "invalid release tag: ${tag}" >&2
    exit 1
fi
base_url="https://github.com/${repo}/releases/download/${tag}"
asset_dir="${ASSET_DIR:-}"
repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
mkdir -p "${repo_root}/tmp"
work="$(mktemp -d "${repo_root}/tmp/release-smoke.XXXXXX")"

cleanup() {
    rm -rf "$work"
}
trap cleanup EXIT

assets=(
    "bitfox-${version}-windows-x86_64.zip"
    "bitfox-${version}-windows-x86_64.zip.sha256"
    "bitfox-${version}-linux-x86_64.tar.gz"
    "bitfox-${version}-linux-x86_64.tar.gz.sha256"
    "bitfox-${version}-linux-aarch64.tar.gz"
    "bitfox-${version}-linux-aarch64.tar.gz.sha256"
    "bitfox-${version}-macos-arm64.tar.gz"
    "bitfox-${version}-macos-arm64.tar.gz.sha256"
    "bitfox-${version}-windows-x86_64-gui.zip"
    "bitfox-${version}-windows-x86_64-gui.zip.sha256"
    "bitfox-${version}-linux-x86_64-gui.tar.gz"
    "bitfox-${version}-linux-x86_64-gui.tar.gz.sha256"
    "bitfox-${version}-macos-arm64-gui.zip"
    "bitfox-${version}-macos-arm64-gui.zip.sha256"
)

download() {
    local name="$1"
    if [[ -n "$asset_dir" ]]; then
        local matches=()
        while IFS= read -r path; do
            matches+=("$path")
        done < <(find "$asset_dir" -type f -name "$name")
        if [[ "${#matches[@]}" -ne 1 ]]; then
            echo "expected one local ${name}, found ${#matches[@]}" >&2
            exit 1
        fi
        echo "copy ${name}"
        cp "${matches[0]}" "${work}/${name}"
    else
        echo "download ${name}"
        curl -fsSL "${base_url}/${name}" -o "${work}/${name}"
    fi
}

verify_sha() {
    local archive="$1"
    local sha_file="${archive}.sha256"
    local expected actual listed record
    record="$(awk '
        NF {count++; fields = NF; hash = $1; name = $2}
        END {
            if (count == 1 && fields == 2) {
                printf "%s\t%s", hash, name
            }
        }
    ' "${work}/${sha_file}")" || record=""
    IFS=$'\t' read -r expected listed <<<"$record"
    if [[ ! "$expected" =~ ^[0-9A-Fa-f]{64}$ || "$listed" != "$archive" ]]; then
        echo "invalid checksum file for ${archive}" >&2
        exit 1
    fi
    expected="$(printf '%s' "$expected" | tr 'A-F' 'a-f')"
    if command -v shasum >/dev/null 2>&1; then
        actual="$(shasum -a 256 "${work}/${archive}" | awk '{print $1}')"
    elif command -v sha256sum >/dev/null 2>&1; then
        actual="$(sha256sum "${work}/${archive}" | awk '{print $1}')"
    else
        echo "missing checksum tool: install shasum or sha256sum" >&2
        exit 1
    fi
    if [[ "$actual" != "$expected" ]]; then
        echo "checksum mismatch for ${archive}" >&2
        echo "expected ${expected}" >&2
        echo "actual   ${actual}" >&2
        exit 1
    fi
    echo "checksum ok ${archive}"
}

inspect_archive() {
    local kind="$1"
    local archive="$2"
    local root="$3"
    shift 3
    BITFOX_RELEASE_TOOLS="${repo_root}/tools" python3 - "$kind" "${work}/${archive}" "$root" "$@" <<'PY'
import os
import posixpath
from pathlib import PurePosixPath
import re
import stat
import sys
import tarfile
import unicodedata
import zipfile

kind, archive, root, *required = sys.argv[1:]
max_members = 20_000
max_file_size = 512 * 1024 * 1024
max_total_size = 2 * 1024 * 1024 * 1024
names = set()
folded_names = set()
regular_files = {}
links = []
total_size = 0


def fail(message):
    raise SystemExit(f"{os.path.basename(archive)}: {message}")


def checked_name(raw):
    if not raw or len(raw) > 4096 or "\0" in raw or "\\" in raw:
        fail(f"unsafe member name {raw!r}")
    name = raw[:-1] if raw.endswith("/") else raw
    parts = name.split("/")
    if (
        not name
        or name.startswith("/")
        or re.match(r"^[A-Za-z]:", name)
        or any(not part or part in (".", "..") for part in parts)
        or any(any(ord(char) < 32 for char in part) for part in parts)
        or parts[0] != root
    ):
        fail(f"unsafe member name {raw!r}")
    normalized = PurePosixPath(*parts).as_posix()
    private_names = {"node_modules", "__pycache__"}
    private_suffixes = (
        ".key", ".pem", ".p12", ".pfx", ".jks", ".keystore", ".log", ".tmp",
        ".prof", ".profraw", ".profdata", ".nnue", ".data", ".ckpt", ".dsym",
        ".md", ".markdown",
    )
    if any(
        part.startswith(".")
        or part.casefold() in private_names
        or part.casefold().endswith(private_suffixes)
        for part in parts
    ):
        fail(f"private development member {normalized!r}")
    folded = unicodedata.normalize("NFC", normalized).casefold()
    if normalized in names or folded in folded_names:
        fail(f"duplicate member {normalized!r}")
    names.add(normalized)
    folded_names.add(folded)
    return normalized


def add_size(name, size):
    global total_size
    if size < 0 or size > max_file_size:
        fail(f"member {name!r} has invalid size {size}")
    total_size += size
    if total_size > max_total_size:
        fail("unpacked archive is too large")


if kind == "tar":
    with tarfile.open(archive, "r:gz") as source:
        for count, member in enumerate(source, 1):
            if count > max_members:
                fail("archive has too many members")
            name = checked_name(member.name)
            if member.isfile():
                add_size(name, member.size)
                regular_files[name] = member.size
            elif not member.isdir():
                fail(f"member {name!r} is not a regular file or directory")
elif kind == "zip":
    with zipfile.ZipFile(archive) as source:
        for count, member in enumerate(source.infolist(), 1):
            if count > max_members:
                fail("archive has too many members")
            if member.flag_bits & 1:
                fail(f"member {member.filename!r} is encrypted")
            name = checked_name(member.filename)
            mode = (member.external_attr >> 16) & 0xFFFF
            file_type = stat.S_IFMT(mode)
            if member.is_dir():
                continue
            if stat.S_ISLNK(mode):
                if member.file_size > 4096:
                    fail(f"symlink {name!r} has an invalid target")
                try:
                    target = source.read(member).decode("utf-8")
                except UnicodeDecodeError:
                    fail(f"symlink {name!r} has a non-UTF-8 target")
                target_parts = target.split("/")
                if (
                    not target
                    or target.startswith("/")
                    or "\\" in target
                    or "\0" in target
                    or re.match(r"^[A-Za-z]:", target)
                    or any(not part or part in (".", "..") for part in target_parts)
                ):
                    fail(f"symlink {name!r} escapes the package")
                destination = posixpath.normpath(
                    posixpath.join(posixpath.dirname(name), target)
                )
                if destination != root and not destination.startswith(root + "/"):
                    fail(f"symlink {name!r} escapes the package")
                links.append((name, destination))
                add_size(name, member.file_size)
            elif file_type in (0, stat.S_IFREG):
                add_size(name, member.file_size)
                regular_files[name] = member.file_size
            else:
                fail(f"member {name!r} has an unsupported file type")
        bad_member = source.testzip()
        if bad_member is not None:
            fail(f"CRC check failed for {bad_member!r}")
else:
    fail(f"unknown archive type {kind!r}")

link_targets = dict(links)


def resolved_target(name):
    seen = set()
    for _ in range(40):
        parts = name.split("/")
        for index in range(1, len(parts) + 1):
            prefix = "/".join(parts[:index])
            if prefix not in link_targets:
                continue
            if prefix in seen:
                fail(f"cyclic symlink {prefix!r}")
            seen.add(prefix)
            name = "/".join([link_targets[prefix], *parts[index:]])
            break
        else:
            return name
    fail("symlink chain is too long")


for link, destination in links:
    resolved = resolved_target(destination)
    if resolved not in names and not any(
        name.startswith(resolved + "/") for name in names
    ):
        fail(f"symlink {link!r} has a missing target")

for item in required:
    if regular_files.get(item, 0) <= 0:
        fail(f"missing or empty required regular file {item!r}")

if root.startswith("bitfox-"):
    allowed = set(required) | {root}
    if root.endswith("-windows-x86_64"):
        sys.path.insert(0, os.environ["BITFOX_RELEASE_TOOLS"])
        from windows_runtime import RUNTIME_FILES
        allowed.update(root + "/" + name for name in RUNTIME_FILES)
        allowed.add(root + "/licenses")
    unexpected = names - allowed
    if unexpected:
        fail(f"unexpected engine member {sorted(unexpected)[0]!r}")

if not names:
    fail("archive is empty")
print(f"archive ok {os.path.basename(archive)} ({len(names)} members)")
PY
}

run_local_uci() {
    local label="$1"
    shift
    echo "uci smoke ${label}"
    python3 - "$label" "$version" "$@" <<'PY'
import subprocess
import os
import signal
import sys

label = sys.argv[1]
expected_version = sys.argv[2]
cmd = sys.argv[3:]
proc = subprocess.Popen(
    cmd,
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
    start_new_session=os.name == "posix",
)

def stop(force=False):
    if proc.poll() is not None:
        return
    try:
        if os.name == "posix":
            os.killpg(proc.pid, signal.SIGKILL if force else signal.SIGTERM)
        elif force:
            proc.kill()
        else:
            proc.terminate()
    except ProcessLookupError:
        pass

try:
    commands = "uci\nisready\nposition startpos\ngo depth 1\nquit\n"
    try:
        stdout, stderr = proc.communicate(commands, timeout=30)
    except subprocess.TimeoutExpired:
        stop()
        try:
            stdout, stderr = proc.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            stop(force=True)
            stdout, stderr = proc.communicate(timeout=5)
        raise RuntimeError(f"{label}: UCI smoke timed out")
    if proc.returncode != 0:
        raise RuntimeError(f"{label}: engine exited with {proc.returncode}: {stderr}")
    lines = stdout.splitlines()
    identity = f"id name Bitfox {expected_version}"
    if identity not in lines:
        raise RuntimeError(f"{label}: missing {identity!r}; saw {lines!r}")
    for token in ("uciok", "readyok"):
        if token not in lines:
            raise RuntimeError(f"{label}: missing {token}; saw {lines!r}")
    best = next((line for line in lines if line.startswith("bestmove ")), "")
    if len(best.split()) < 2 or best.split()[1] == "0000":
        raise RuntimeError(f"{label}: invalid {best!r}")
except Exception:
    stop(force=True)
    try:
        _, trailing = proc.communicate(timeout=5)
        stderr = (locals().get("stderr") or "") + trailing
    except subprocess.TimeoutExpired:
        stderr = locals().get("stderr") or ""
    if stderr:
        print(stderr, file=sys.stderr)
    raise
PY
}

run_docker_uci() {
    local label="$1"
    local platform="$2"
    local dir="$3"
    if ! command -v docker >/dev/null 2>&1; then
        echo "docker is required for ${label} UCI smoke" >&2
        exit 1
    fi
    echo "uci smoke ${label}"
    docker run --rm --platform "$platform" \
        -v "${dir}:/release:ro" \
        -w /release \
        debian:bookworm-slim \
        sh -lc '
            set -eu
            expected="$1"
            out="$(printf "uci\nisready\nposition startpos\ngo depth 1\nquit\n" | timeout --kill-after=5s 30s ./bitfox)"
            echo "$out"
            echo "$out" | grep -Fxq "id name Bitfox $expected"
            echo "$out" | grep -q "^uciok$"
            echo "$out" | grep -q "^readyok$"
            echo "$out" | grep -Eq "^bestmove [a-h][1-8][a-h][1-8][nbrq]?"
        ' sh "$version"
}

run_docker_wine_uci() {
    local label="$1"
    local dir="$2"
    local exe="${3:-./bitfox.exe}"
    if ! command -v docker >/dev/null 2>&1; then
        echo "docker is required for ${label} UCI smoke" >&2
        exit 1
    fi
    echo "uci smoke ${label}"
    docker run --rm --platform linux/amd64 \
        -v "${dir}:/release:ro" \
        -w /release \
        debian:bookworm-slim \
        sh -lc '
            set -eu
            exe="$1"
            expected="$2"
            wine_log="$(mktemp /tmp/bitfox-wine.XXXXXX)"
            trap '\''rm -f "$wine_log"'\'' EXIT HUP INT TERM
            apt-get update >/dev/null
            apt-get install -y --no-install-recommends wine ca-certificates >/dev/null
            out="$(printf "uci\nisready\nposition startpos\ngo depth 1\nquit\n" |
                WINEDEBUG=-all timeout --kill-after=5s 60s wine "$exe" 2>"$wine_log")" || {
                cat "$wine_log" >&2
                exit 1
            }
            echo "$out"
            echo "$out" | grep -Fxq "id name Bitfox $expected"
            echo "$out" | grep -q "^uciok$"
            echo "$out" | grep -q "^readyok$"
            echo "$out" | grep -Eq "^bestmove [a-h][1-8][a-h][1-8][nbrq]?"
        ' sh "$exe" "$version"
}

run_docker_linux_gui() {
    local label="$1"
    local dir="$2"
    if ! command -v docker >/dev/null 2>&1; then
        echo "docker is required for ${label} GUI smoke" >&2
        exit 1
    fi
    echo "gui smoke ${label}"
    docker run --rm --platform linux/amd64 \
        -v "${dir}:/release:ro" \
        -w /release \
        debian:bookworm-slim \
        sh -lc '
            set -eu
            version="$1"
            engine="./engines/Bitfox ${version}"
            test -x ./bitfox-board
            test -x "$engine"
            if LD_LIBRARY_PATH="$PWD/lib" ldd ./bitfox-board-bin | grep -q "not found"; then
                LD_LIBRARY_PATH="$PWD/lib" ldd ./bitfox-board-bin >&2
                exit 1
            fi
            out="$(printf "uci\nisready\nquit\n" | timeout 30s "$engine")"
            echo "$out"
            echo "$out" | grep -Fxq "id name Bitfox $version"
            echo "$out" | grep -q "^uciok$"
            echo "$out" | grep -q "^readyok$"
            gui_log="$(mktemp /tmp/bitfox-gui.XXXXXX)"
            trap '\''rm -f "$gui_log"'\'' EXIT HUP INT TERM
            status=0
            QT_QPA_PLATFORM=offscreen \
                timeout --signal=TERM --kill-after=5s 5s ./bitfox-board \
                >"$gui_log" 2>&1 || status=$?
            if [ "$status" -ne 124 ]; then
                cat "$gui_log" >&2
                echo "GUI exited before the smoke window (status $status)" >&2
                exit 1
            fi
        ' sh "$version"
}

for asset in "${assets[@]}"; do
    download "$asset"
done

verify_sha "bitfox-${version}-windows-x86_64.zip"
verify_sha "bitfox-${version}-linux-x86_64.tar.gz"
verify_sha "bitfox-${version}-linux-aarch64.tar.gz"
verify_sha "bitfox-${version}-macos-arm64.tar.gz"
verify_sha "bitfox-${version}-windows-x86_64-gui.zip"
verify_sha "bitfox-${version}-linux-x86_64-gui.tar.gz"
verify_sha "bitfox-${version}-macos-arm64-gui.zip"

inspect_archive tar "bitfox-${version}-linux-x86_64.tar.gz" \
    "bitfox-${version}-linux-x86_64" \
    "bitfox-${version}-linux-x86_64/LICENSE" \
    "bitfox-${version}-linux-x86_64/THIRD_PARTY_NOTICES.txt" \
    "bitfox-${version}-linux-x86_64/bitfox" \
    "bitfox-${version}-linux-x86_64/bitfox-avx2"
inspect_archive tar "bitfox-${version}-linux-aarch64.tar.gz" \
    "bitfox-${version}-linux-aarch64" \
    "bitfox-${version}-linux-aarch64/LICENSE" \
    "bitfox-${version}-linux-aarch64/THIRD_PARTY_NOTICES.txt" \
    "bitfox-${version}-linux-aarch64/bitfox"
inspect_archive tar "bitfox-${version}-macos-arm64.tar.gz" \
    "bitfox-${version}-macos-arm64" \
    "bitfox-${version}-macos-arm64/LICENSE" \
    "bitfox-${version}-macos-arm64/THIRD_PARTY_NOTICES.txt" \
    "bitfox-${version}-macos-arm64/bitfox"
inspect_archive zip "bitfox-${version}-windows-x86_64.zip" \
    "bitfox-${version}-windows-x86_64" \
    "bitfox-${version}-windows-x86_64/LICENSE" \
    "bitfox-${version}-windows-x86_64/THIRD_PARTY_NOTICES.txt" \
    "bitfox-${version}-windows-x86_64/RUNTIME_DEPENDENCIES.json" \
    "bitfox-${version}-windows-x86_64/licenses/msvc-2022.txt" \
    "bitfox-${version}-windows-x86_64/bitfox.exe" \
    "bitfox-${version}-windows-x86_64/bitfox-avx2.exe"
inspect_archive tar "bitfox-${version}-linux-x86_64-gui.tar.gz" \
    "Bitfox" "Bitfox/LICENSE" "Bitfox/bitfox-board" "Bitfox/bitfox-board-bin" \
    "Bitfox/THIRD_PARTY_NOTICES.txt" \
    "Bitfox/GUI_DEPENDENCIES.json" \
    "Bitfox/lib/libbitfox.so" "Bitfox/engines/Bitfox ${version}"
inspect_archive zip "bitfox-${version}-macos-arm64-gui.zip" \
    "Bitfox.app" "Bitfox.app/Contents/Resources/LICENSE" "Bitfox.app/Contents/MacOS/bitfox-board" \
    "Bitfox.app/Contents/Resources/THIRD_PARTY_NOTICES.txt" \
    "Bitfox.app/Contents/Resources/GUI_DEPENDENCIES.json" \
    "Bitfox.app/Contents/MacOS/libbitfox.dylib" \
    "Bitfox.app/Contents/PlugIns/tls/libqsecuretransportbackend.dylib" \
    "Bitfox.app/Contents/MacOS/engines/Bitfox ${version}"
inspect_archive zip "bitfox-${version}-windows-x86_64-gui.zip" \
    "Bitfox" "Bitfox/LICENSE" "Bitfox/bitfox-board.exe" "Bitfox/bitfox.dll" \
    "Bitfox/THIRD_PARTY_NOTICES.txt" \
    "Bitfox/GUI_DEPENDENCIES.json" \
    "Bitfox/RUNTIME_DEPENDENCIES.json" \
    "Bitfox/engines/Bitfox ${version}.exe"

tar -xzf "${work}/bitfox-${version}-linux-x86_64.tar.gz" -C "$work"
tar -xzf "${work}/bitfox-${version}-linux-aarch64.tar.gz" -C "$work"
tar -xzf "${work}/bitfox-${version}-macos-arm64.tar.gz" -C "$work"
unzip -q "${work}/bitfox-${version}-windows-x86_64.zip" -d "$work"

mkdir -p "${work}/gui-linux" "${work}/gui-macos" "${work}/gui-windows"
tar -xzf "${work}/bitfox-${version}-linux-x86_64-gui.tar.gz" -C "${work}/gui-linux"
unzip -q "${work}/bitfox-${version}-macos-arm64-gui.zip" -d "${work}/gui-macos"
unzip -q "${work}/bitfox-${version}-windows-x86_64-gui.zip" -d "${work}/gui-windows"

for bundle in "${work}/gui-linux/Bitfox" "${work}/gui-macos/Bitfox.app" "${work}/gui-windows/Bitfox"; do
    python3 "${repo_root}/tools/gui_dependency_notices.py" verify --bundle "$bundle"
done

for bundle in "${work}/bitfox-${version}-windows-x86_64" "${work}/gui-windows/Bitfox"; do
    python3 "${repo_root}/tools/windows_runtime.py" verify --bundle "$bundle"
done

test -f "${work}/gui-windows/Bitfox/bitfox-board.exe"
test -f "${work}/gui-windows/Bitfox/Qt6Core.dll"
test -f "${work}/gui-windows/Bitfox/platforms/qwindows.dll"
test -f "${work}/gui-windows/Bitfox/engines/Bitfox ${version}.exe"

run_docker_uci "Linux x86_64" "linux/amd64" "${work}/bitfox-${version}-linux-x86_64"
run_docker_uci "Linux ARM64" "linux/arm64" "${work}/bitfox-${version}-linux-aarch64"
run_docker_linux_gui "Linux x86_64" "${work}/gui-linux/Bitfox"

if [[ "$(uname -s)" == "Darwin" && "$(uname -m)" == "arm64" ]]; then
    run_local_uci "macOS arm64" "${work}/bitfox-${version}-macos-arm64/bitfox"
    codesign --verify --deep --strict "${work}/gui-macos/Bitfox.app"
    run_local_uci "macOS GUI bundled engine" \
        "${work}/gui-macos/Bitfox.app/Contents/MacOS/engines/Bitfox ${version}"
else
    echo "skip macOS arm64 UCI on this host"
fi

if command -v wine >/dev/null 2>&1; then
    run_local_uci "Windows x86_64" wine "${work}/bitfox-${version}-windows-x86_64/bitfox.exe"
    run_local_uci "Windows GUI bundled engine" wine \
        "${work}/gui-windows/Bitfox/engines/Bitfox ${version}.exe"
else
    run_docker_wine_uci "Windows x86_64" "${work}/bitfox-${version}-windows-x86_64"
    run_docker_wine_uci "Windows GUI bundled engine" \
        "${work}/gui-windows/Bitfox" "./engines/Bitfox ${version}.exe"
fi
echo "windows GUI package structure ok"

echo "release smoke ok ${tag}"
