#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
version="$(awk -F'"' '/^version = / {print $2; exit}' "${repo_root}/engine/Cargo.toml")"
if [[ -z "$version" ]]; then
    echo "could not read package version from engine/Cargo.toml" >&2
    exit 1
fi

build_dir="${BUILD_DIR:-${repo_root}/gui-qt/build-macos}"
if [[ ! -x "${build_dir}/bitfox-board" && -x "${repo_root}/gui-qt/build/bitfox-board" ]]; then
    build_dir="${repo_root}/gui-qt/build"
fi

board_src="${build_dir}/bitfox-board"
core_src="${build_dir}/libbitfox.dylib"
engine_src="${repo_root}/engine/target/release/bitfox"

for path in "$board_src" "$core_src" "$engine_src"; do
    if [[ ! -f "$path" ]]; then
        echo "missing build output: $path" >&2
        exit 1
    fi
done

macdeployqt_bin="$(command -v macdeployqt || true)"
if [[ -z "$macdeployqt_bin" ]]; then
    echo "macdeployqt was not found in PATH" >&2
    exit 1
fi

qt_plugin_dir="${QT_PLUGIN_DIR:-}"
if [[ -z "$qt_plugin_dir" ]]; then
    if command -v qtpaths6 >/dev/null 2>&1; then
        qt_plugin_dir="$(qtpaths6 --plugin-dir)"
    elif command -v qmake6 >/dev/null 2>&1; then
        qt_plugin_dir="$(qmake6 -query QT_INSTALL_PLUGINS)"
    else
        qt_plugin_dir="$(find /opt/homebrew /usr/local -path "*/share/qt/plugins/platforms/libqcocoa.dylib" -print -quit 2>/dev/null | sed 's#/platforms/libqcocoa.dylib$##')"
    fi
fi
if [[ -z "$qt_plugin_dir" || ! -d "$qt_plugin_dir" ]]; then
    echo "could not locate Qt plugin directory" >&2
    exit 1
fi

dist_root="${DIST_DIR:-${repo_root}/dist}"
app="${dist_root}/Bitfox.app"
contents="${app}/Contents"
macos_dir="${contents}/MacOS"
resources_dir="${contents}/Resources"
engine_dir="${macos_dir}/engines"
archive="${dist_root}/bitfox-${version}-macos-arm64-gui.zip"

rm -rf "$app" "$archive" "${archive}.sha256"
mkdir -p "$macos_dir" "$resources_dir" "$engine_dir"

cp "$board_src" "${macos_dir}/bitfox-board"
cp "$core_src" "${macos_dir}/libbitfox.dylib"
cp "$engine_src" "${engine_dir}/Bitfox ${version}"
chmod 755 "${macos_dir}/bitfox-board" "${engine_dir}/Bitfox ${version}"

cat > "${contents}/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>bitfox-board</string>
  <key>CFBundleIdentifier</key>
  <string>com.bitfox.board</string>
  <key>CFBundleName</key>
  <string>Bitfox</string>
  <key>CFBundleDisplayName</key>
  <string>Bitfox</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${version}</string>
  <key>CFBundleVersion</key>
  <string>${version}</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

fix_bitfox_names() {
    install_name_tool -id "@loader_path/libbitfox.dylib" "${macos_dir}/libbitfox.dylib"
    while IFS= read -r needed; do
        if [[ "$(basename "$needed")" == "libbitfox.dylib" ]]; then
            install_name_tool -change "$needed" "@loader_path/libbitfox.dylib" "${macos_dir}/bitfox-board"
        fi
    done < <(otool -L "${macos_dir}/bitfox-board" | awk 'NR > 1 {print $1}')
}

fix_bundle_library_paths() {
    local frameworks_dir="${contents}/Frameworks"
    [[ -d "$frameworks_dir" ]] || return 0

    while IFS= read -r -d '' file; do
        if ! file "$file" | grep -q "Mach-O"; then
            continue
        fi

        if [[ "$file" == "${frameworks_dir}/"*.dylib ]]; then
            install_name_tool -id "@executable_path/../Frameworks/$(basename "$file")" "$file" 2>/dev/null || true
        fi

        while IFS= read -r needed; do
            local base replacement
            base="$(basename "$needed")"
            replacement="@executable_path/../Frameworks/${base}"
            if [[ -f "${frameworks_dir}/${base}" ]]; then
                case "$needed" in
                    /opt/homebrew/*|/usr/local/Cellar/*|/usr/local/opt/*|/Users/*|@rpath/*.dylib)
                        install_name_tool -change "$needed" "$replacement" "$file" 2>/dev/null || true
                        ;;
                esac
            fi
            if [[ "$needed" =~ ([^/]+\.framework/Versions/A/[^/]+)$ ]]; then
                local framework_rel="${BASH_REMATCH[1]}"
                if [[ -e "${frameworks_dir}/${framework_rel}" ]]; then
                    replacement="@executable_path/../Frameworks/${framework_rel}"
                    case "$needed" in
                        /opt/homebrew/*|/usr/local/Cellar/*|/usr/local/opt/*|/Users/*|@rpath/*)
                            install_name_tool -change "$needed" "$replacement" "$file" 2>/dev/null || true
                            ;;
                    esac
                fi
            fi
        done < <(otool -L "$file" | awk 'NR > 1 {print $1}')
    done < <(find "$app" -type f \( -perm -111 -o -name "*.dylib" \) -print0)
}

copy_qt_plugin() {
    local rel="$1"
    local required="${2:-optional}"
    local src="${qt_plugin_dir}/${rel}"
    local dst="${contents}/PlugIns/${rel}"
    if [[ ! -f "$src" ]]; then
        if [[ "$required" == "required" ]]; then
            echo "missing Qt plugin: $src" >&2
            exit 1
        fi
        return 0
    fi
    mkdir -p "$(dirname "$dst")"
    cp -L "$src" "$dst"
    chmod 755 "$dst" || true
}

fix_bitfox_names
"$macdeployqt_bin" "$app" -verbose=1 -no-plugins -no-codesign
rm -f "${contents}/Frameworks/libbitfox.dylib"
copy_qt_plugin "platforms/libqcocoa.dylib" required
copy_qt_plugin "styles/libqmacstyle.dylib"
copy_qt_plugin "multimedia/libdarwinmediaplugin.dylib"
fix_bitfox_names
fix_bundle_library_paths

bad_refs="$(
    find "$app" -type f \( -perm -111 -o -name "*.dylib" \) -print0 |
    while IFS= read -r -d '' file; do
        if file "$file" | grep -q "Mach-O"; then
            otool -L "$file" | awk 'NR > 1 {print $1}' |
                grep -E "/Users/|/opt/homebrew|/usr/local/(Cellar|opt)" || true
        fi
    done
)"
if [[ -n "$bad_refs" ]]; then
    echo "found local library references in app bundle:" >&2
    echo "$bad_refs" >&2
    exit 1
fi

uci_out="$(printf "uci\nisready\nquit\n" | "${engine_dir}/Bitfox ${version}")"
echo "$uci_out"
grep -q "^uciok$" <<<"$uci_out"
grep -q "^readyok$" <<<"$uci_out"

codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"

mkdir -p "$dist_root"
(
    cd "$dist_root"
    ditto -c -k --sequesterRsrc --keepParent "Bitfox.app" "$(basename "$archive")"
)
shasum -a 256 "$archive" | awk -v name="$(basename "$archive")" '{print $1 "  " name}' > "${archive}.sha256"
echo "created ${archive}"
