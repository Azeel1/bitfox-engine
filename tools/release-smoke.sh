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
base_url="https://github.com/${repo}/releases/download/${tag}"
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
    echo "download ${name}"
    curl -fsSL "${base_url}/${name}" -o "${work}/${name}"
}

verify_sha() {
    local archive="$1"
    local sha_file="${archive}.sha256"
    local expected actual
    expected="$(awk '{print $1}' "${work}/${sha_file}")"
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

run_local_uci() {
    local label="$1"
    shift
    echo "uci smoke ${label}"
    python3 - "$label" "$@" <<'PY'
import subprocess
import sys
import time

label = sys.argv[1]
cmd = sys.argv[2:]
proc = subprocess.Popen(
    cmd,
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
    bufsize=1,
)

def send(line):
    proc.stdin.write(line + "\n")
    proc.stdin.flush()

def wait_for(prefix, timeout=15.0):
    deadline = time.monotonic() + timeout
    lines = []
    while time.monotonic() < deadline:
        line = proc.stdout.readline()
        if not line:
            break
        line = line.strip()
        lines.append(line)
        if line.startswith(prefix):
            return line, lines
    raise RuntimeError(f"{label}: missing {prefix}; saw {lines!r}")

try:
    send("uci")
    wait_for("uciok")
    send("isready")
    wait_for("readyok")
    send("position startpos")
    send("go depth 1")
    best, _ = wait_for("bestmove", timeout=30.0)
    if len(best.split()) < 2 or best.split()[1] == "0000":
        raise RuntimeError(f"{label}: invalid {best!r}")
    send("quit")
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        raise
except Exception:
    proc.kill()
    stderr = proc.stderr.read()
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
            out="$(printf "uci\nisready\nposition startpos\ngo depth 1\nquit\n" | timeout 30s ./bitfox)"
            echo "$out"
            echo "$out" | grep -q "^uciok$"
            echo "$out" | grep -q "^readyok$"
            echo "$out" | grep -Eq "^bestmove [a-h][1-8][a-h][1-8][nbrq]?"
        '
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
            apt-get update >/dev/null
            apt-get install -y --no-install-recommends wine ca-certificates >/dev/null
            out="$(printf "uci\nisready\nposition startpos\ngo depth 1\nquit\n" |
                WINEDEBUG=-all timeout 60s wine "$exe" 2>/tmp/wine.err)" || {
                cat /tmp/wine.err >&2
                exit 1
            }
            echo "$out"
            echo "$out" | grep -q "^uciok$"
            echo "$out" | grep -q "^readyok$"
            echo "$out" | grep -Eq "^bestmove [a-h][1-8][a-h][1-8][nbrq]?"
        ' sh "$exe"
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
            test -x ./bitfox-board
            test -x "./engines/Bitfox '"${version}"'"
            if LD_LIBRARY_PATH="$PWD/lib" ldd ./bitfox-board-bin | grep -q "not found"; then
                LD_LIBRARY_PATH="$PWD/lib" ldd ./bitfox-board-bin >&2
                exit 1
            fi
            out="$(printf "uci\nisready\nquit\n" | timeout 30s "./engines/Bitfox '"${version}"'")"
            echo "$out"
            echo "$out" | grep -q "^uciok$"
            echo "$out" | grep -q "^readyok$"
            status=0
            QT_QPA_PLATFORM=offscreen timeout 5s ./bitfox-board >/tmp/bitfox-gui.log 2>&1 || status=$?
            if [ "$status" != 0 ] && [ "$status" != 124 ]; then
                cat /tmp/bitfox-gui.log >&2
                exit "$status"
            fi
        '
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

tar -xzf "${work}/bitfox-${version}-linux-x86_64.tar.gz" -C "$work"
tar -xzf "${work}/bitfox-${version}-linux-aarch64.tar.gz" -C "$work"
tar -xzf "${work}/bitfox-${version}-macos-arm64.tar.gz" -C "$work"
unzip -q "${work}/bitfox-${version}-windows-x86_64.zip" -d "$work"

mkdir -p "${work}/gui-linux" "${work}/gui-macos" "${work}/gui-windows"
tar -xzf "${work}/bitfox-${version}-linux-x86_64-gui.tar.gz" -C "${work}/gui-linux"
unzip -q "${work}/bitfox-${version}-macos-arm64-gui.zip" -d "${work}/gui-macos"
unzip -q "${work}/bitfox-${version}-windows-x86_64-gui.zip" -d "${work}/gui-windows"

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
else
    run_docker_wine_uci "Windows x86_64" "${work}/bitfox-${version}-windows-x86_64"
fi
echo "windows GUI package structure ok"

echo "release smoke ok ${tag}"
