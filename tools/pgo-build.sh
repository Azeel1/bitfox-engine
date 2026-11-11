#!/usr/bin/env bash
set -euo pipefail

case "$#" in
    1) cpu=""; out="$1" ;;
    2) cpu="$1"; out="$2" ;;
    *) echo "usage: pgo-build.sh [target-cpu] <output>" >&2; exit 2 ;;
esac
if [[ -z "$out" ]]; then
    echo "output path must not be empty" >&2
    exit 2
fi
depth="${PGO_BENCH_DEPTH:-14}"
[[ "$out" == /* ]] || out="${PWD}/${out}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
engine="${repo_root}/engine"

native_path() {
    if command -v cygpath >/dev/null 2>&1; then
        cygpath -m "$1"
    else
        printf '%s' "$1"
    fi
}

cd "$engine"
rustup component add llvm-tools >/dev/null
host="$(rustc -vV | sed -n 's/^host: //p')"
if [[ -z "$cpu" ]]; then
    case "$host" in
        x86_64-unknown-linux-gnu|x86_64-pc-windows-msvc|x86_64-pc-windows-gnu)
            cpu="x86-64-v2" ;;
    esac
fi
work="${engine}/target/pgo${cpu:+-$cpu}"
exe=""
[[ "$host" == *windows* ]] && exe=".exe"
profdata="$(rustc --print sysroot)/lib/rustlib/${host}/bin/llvm-profdata${exe}"
test -x "$profdata"

base_flags=(
    "--remap-path-prefix=$(native_path "${CARGO_HOME:-${HOME}/.cargo}")=cargo"
    "--remap-path-prefix=$(native_path "$repo_root")=bitfox"
)
[[ -n "$cpu" ]] && base_flags+=("-Ctarget-cpu=${cpu}")

rm -rf "$work"
mkdir -p "$work/raw"

printf -v build_flags '%s\x1f' "${base_flags[@]}" "-Cprofile-generate=$(native_path "$work/raw")"
CARGO_ENCODED_RUSTFLAGS="${build_flags%$'\x1f'}" \
    cargo build --release --locked --target-dir "$work/instrumented"
"$work/instrumented/release/bitfox${exe}" bench "$depth" >/dev/null
"$profdata" merge -o "$work/merged.profdata" "$work"/raw/*.profraw

printf -v build_flags '%s\x1f' "${base_flags[@]}" "-Cprofile-use=$(native_path "$work/merged.profdata")"
CARGO_ENCODED_RUSTFLAGS="${build_flags%$'\x1f'}" \
    cargo build --release --locked --target-dir "$work/final"

mkdir -p "$(dirname "$out")"
cp "$work/final/release/bitfox${exe}" "$out"
