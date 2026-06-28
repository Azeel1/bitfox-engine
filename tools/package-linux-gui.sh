#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
version="$(awk -F'"' '/^version = / {print $2; exit}' "${repo_root}/engine/Cargo.toml")"
if [[ -z "$version" ]]; then
    echo "could not read package version from engine/Cargo.toml" >&2
    exit 1
fi

build_dir="${BUILD_DIR:-${repo_root}/gui-qt/build-linux}"
if [[ ! -x "${build_dir}/bitfox-board" && -x "${repo_root}/gui-qt/build/bitfox-board" ]]; then
    build_dir="${repo_root}/gui-qt/build"
fi

board_src="${build_dir}/bitfox-board"
core_src="${build_dir}/libbitfox.so"
engine_src="${repo_root}/engine/target/release/bitfox"

for path in "$board_src" "$core_src" "$engine_src"; do
    if [[ ! -f "$path" ]]; then
        echo "missing build output: $path" >&2
        exit 1
    fi
done

for tool in ldd patchelf sha256sum tar timeout; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "missing required tool: $tool" >&2
        exit 1
    fi
done

qt_plugin_dir="${QT_PLUGIN_DIR:-}"
if [[ -z "$qt_plugin_dir" ]]; then
    if command -v qtpaths6 >/dev/null 2>&1; then
        qt_plugin_dir="$(qtpaths6 --plugin-dir)"
    elif command -v qmake6 >/dev/null 2>&1; then
        qt_plugin_dir="$(qmake6 -query QT_INSTALL_PLUGINS)"
    else
        qt_plugin_dir="$(find /usr -path "*/qt6/plugins/platforms/libqxcb.so" -print -quit 2>/dev/null | sed 's#/platforms/libqxcb.so$##')"
    fi
fi
if [[ -z "$qt_plugin_dir" || ! -d "$qt_plugin_dir/platforms" ]]; then
    echo "could not locate Qt plugin directory" >&2
    exit 1
fi

dist_root="${DIST_DIR:-${repo_root}/dist}"
dist="${dist_root}/Bitfox"
lib_dir="${dist}/lib"
plugin_dir="${dist}/plugins"
engine_dir="${dist}/engines"
archive="${dist_root}/bitfox-${version}-linux-x86_64-gui.tar.gz"

rm -rf "$dist" "$archive" "${archive}.sha256"
mkdir -p "$lib_dir" "$plugin_dir" "$engine_dir"

board_bin="${dist}/bitfox-board-bin"
wrapper="${dist}/bitfox-board"
core_dst="${lib_dir}/libbitfox.so"
engine_dst="${engine_dir}/Bitfox ${version}"

cp "$board_src" "$board_bin"
cp "$core_src" "$core_dst"
cp "$engine_src" "$engine_dst"
chmod 755 "$board_bin" "$engine_dst"

cat > "$wrapper" <<'SH'
#!/usr/bin/env sh
set -eu
app_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
export LD_LIBRARY_PATH="$app_dir/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export QT_PLUGIN_PATH="$app_dir/plugins${QT_PLUGIN_PATH:+:$QT_PLUGIN_PATH}"
exec "$app_dir/bitfox-board-bin" "$@"
SH
chmod 755 "$wrapper"

patchelf --set-soname libbitfox.so "$core_dst"
while IFS= read -r needed; do
    if [[ "$needed" == */libbitfox.so ]]; then
        patchelf --replace-needed "$needed" libbitfox.so "$board_bin"
    fi
done < <(patchelf --print-needed "$board_bin")
patchelf --set-rpath '$ORIGIN/lib:$ORIGIN' "$board_bin"

pending=("$board_bin" "$core_dst" "$engine_dst")

is_system_lib() {
    case "$(basename "$1")" in
        linux-vdso*|ld-linux*|libc.so.*|libm.so.*|libpthread.so.*|libdl.so.*|librt.so.*|libresolv.so.*)
            return 0
            ;;
    esac
    return 1
}

copy_runtime_lib() {
    local src="$1"
    [[ -f "$src" ]] || return 0
    is_system_lib "$src" && return 0

    local dst="${lib_dir}/$(basename "$src")"
    if [[ ! -e "$dst" ]]; then
        cp -L "$src" "$dst"
        chmod 644 "$dst" || true
        pending+=("$dst")
    fi
}

collect_deps() {
    local file="$1"
    while IFS= read -r dep; do
        copy_runtime_lib "$dep"
    done < <(ldd "$file" | awk '
        /=> \// {print $3}
        /^\// {print $1}
    ')
}

copy_plugin() {
    local src="$1"
    [[ -f "$src" ]] || return 0
    local rel="${src#${qt_plugin_dir}/}"
    local dst="${plugin_dir}/${rel}"
    mkdir -p "$(dirname "$dst")"
    cp -L "$src" "$dst"
    chmod 755 "$dst" || true
    pending+=("$dst")
}

for plugin in platforms/libqxcb.so platforms/libqoffscreen.so; do
    copy_plugin "${qt_plugin_dir}/${plugin}"
done

for dir in imageformats iconengines multimedia styles tls; do
    if [[ -d "${qt_plugin_dir}/${dir}" ]]; then
        while IFS= read -r plugin; do
            copy_plugin "$plugin"
        done < <(find "${qt_plugin_dir}/${dir}" -type f -name "*.so*")
    fi
done

while ((${#pending[@]})); do
    item="${pending[0]}"
    pending=("${pending[@]:1}")
    collect_deps "$item"
done

while IFS= read -r plugin; do
    patchelf --set-rpath '$ORIGIN/../../lib:$ORIGIN/..:$ORIGIN' "$plugin"
done < <(find "$plugin_dir" -type f -name "*.so*")

if patchelf --print-needed "$board_bin" | grep -q "/libbitfox.so"; then
    patchelf --print-needed "$board_bin" >&2
    echo "bitfox-board still references an absolute libbitfox path" >&2
    exit 1
fi

if LD_LIBRARY_PATH="$lib_dir" ldd "$board_bin" | grep -q "not found"; then
    LD_LIBRARY_PATH="$lib_dir" ldd "$board_bin" >&2
    exit 1
fi

uci_out="$(printf "uci\nisready\nquit\n" | "$engine_dst")"
echo "$uci_out"
grep -q "^uciok$" <<<"$uci_out"
grep -q "^readyok$" <<<"$uci_out"

status=0
QT_QPA_PLATFORM=offscreen timeout 5s "$wrapper" >/tmp/bitfox-gui-smoke.log 2>&1 || status=$?
if [[ "$status" != 0 && "$status" != 124 ]]; then
    cat /tmp/bitfox-gui-smoke.log >&2
    exit "$status"
fi

mkdir -p "$dist_root"
(
    cd "$dist_root"
    tar --sort=name --owner=0 --group=0 --numeric-owner -czf "$(basename "$archive")" "Bitfox"
)
sha256sum "$archive" | awk -v name="$(basename "$archive")" '{print $1 "  " name}' > "${archive}.sha256"
echo "created ${archive}"
