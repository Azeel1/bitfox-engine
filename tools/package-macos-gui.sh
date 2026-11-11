#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version="$(awk -F'"' '/^version = / {print $2; exit}' "${repo_root}/engine/Cargo.toml")"
if [[ -z "$version" ]]; then
    echo "could not read package version from engine/Cargo.toml" >&2
    exit 1
fi
deployment_target="${MACOSX_DEPLOYMENT_TARGET:-14.0}"
if [[ ! "$deployment_target" =~ ^[0-9]+\.[0-9]+(\.[0-9]+)?$ ]]; then
    echo "invalid MACOSX_DEPLOYMENT_TARGET: ${deployment_target}" >&2
    exit 1
fi
export MACOSX_DEPLOYMENT_TARGET="$deployment_target"

build_dir="${BUILD_DIR:-${repo_root}/gui-qt/build-macos}"
if [[ ! -x "${build_dir}/bitfox-board" && -x "${repo_root}/gui-qt/build/bitfox-board" ]]; then
    build_dir="${repo_root}/gui-qt/build"
fi

board_src="${build_dir}/bitfox-board"
core_src="${build_dir}/libbitfox.dylib"
engine_src="${ENGINE_BIN:-${repo_root}/engine/target/release/bitfox}"

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
if ! command -v zip >/dev/null 2>&1; then
    echo "zip was not found in PATH" >&2
    exit 1
fi

qt_plugin_dir="${QT_PLUGIN_DIR:-}"
if [[ -z "$qt_plugin_dir" ]]; then
    qt_bin_dir="$(dirname "$macdeployqt_bin")"
    if [[ -x "${qt_bin_dir}/qtpaths" ]]; then
        qt_plugin_dir="$("${qt_bin_dir}/qtpaths" --plugin-dir)"
    elif [[ -x "${qt_bin_dir}/qtpaths6" ]]; then
        qt_plugin_dir="$("${qt_bin_dir}/qtpaths6" --plugin-dir)"
    elif command -v qtpaths6 >/dev/null 2>&1; then
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

cp "${repo_root}/LICENSE" "${resources_dir}/LICENSE"
cp "${repo_root}/licenses/rust-dependencies.txt" "${resources_dir}/THIRD_PARTY_NOTICES.txt"
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
  <key>LSMinimumSystemVersion</key>
  <string>${deployment_target}</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

linked_libraries() {
    otool -L "$1" | awk '/^[[:space:]]/ {
        sub(/^[[:space:]]+/, "")
        sub(/[[:space:]]+\(compatibility version .*/, "")
        print
    }' | LC_ALL=C sort -u
}

linked_rpaths() {
    otool -l "$1" | awk '
        $1 == "cmd" {rpath = ($2 == "LC_RPATH")}
        rpath && $1 == "path" {
            sub(/^[[:space:]]*path[[:space:]]+/, "")
            sub(/[[:space:]]+\(offset [0-9]+\)$/, "")
            print
        }
    ' | LC_ALL=C sort -u
}

fix_bitfox_names() {
    install_name_tool -id "@loader_path/libbitfox.dylib" "${macos_dir}/libbitfox.dylib"
    while IFS= read -r needed; do
        if [[ "$(basename "$needed")" == "libbitfox.dylib" ]]; then
            install_name_tool -change "$needed" "@loader_path/libbitfox.dylib" "${macos_dir}/bitfox-board"
        fi
    done < <(linked_libraries "${macos_dir}/bitfox-board")
}

fix_bundle_library_paths() {
    local frameworks_dir="${contents}/Frameworks"
    [[ -d "$frameworks_dir" ]] || return 0

    while IFS= read -r -d '' file; do
        if ! file "$file" | grep -q "Mach-O"; then
            continue
        fi

        if [[ "$file" == "${frameworks_dir}/"*.dylib ]]; then
            install_name_tool -id "@executable_path/../Frameworks/$(basename "$file")" "$file"
        fi

        while IFS= read -r needed; do
            local base replacement
            base="$(basename "$needed")"
            replacement="@executable_path/../Frameworks/${base}"
            if [[ -f "${frameworks_dir}/${base}" ]]; then
                case "$needed" in
                    /opt/homebrew/*|/usr/local/Cellar/*|/usr/local/opt/*|/Users/*|@rpath/*.dylib)
                        install_name_tool -change "$needed" "$replacement" "$file"
                        ;;
                esac
            fi
            if [[ "$needed" =~ ([^/]+\.framework/Versions/A/[^/]+)$ ]]; then
                local framework_rel="${BASH_REMATCH[1]}"
                if [[ -e "${frameworks_dir}/${framework_rel}" ]]; then
                    replacement="@executable_path/../Frameworks/${framework_rel}"
                    case "$needed" in
                        /opt/homebrew/*|/usr/local/Cellar/*|/usr/local/opt/*|/Users/*|@rpath/*)
                            install_name_tool -change "$needed" "$replacement" "$file"
                            ;;
                    esac
                fi
            fi
        done < <(linked_libraries "$file")
        while IFS= read -r rpath; do
            case "$rpath" in
                /Users/*|/home/*|/opt/homebrew/*|/usr/local/Cellar/*|/usr/local/opt/*)
                    install_name_tool -delete_rpath "$rpath" "$file"
                    ;;
            esac
        done < <(linked_rpaths "$file")
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
copy_qt_plugin "tls/libqsecuretransportbackend.dylib" required
copy_qt_plugin "styles/libqmacstyle.dylib"
copy_qt_plugin "multimedia/libdarwinmediaplugin.dylib"
fix_bitfox_names
fix_bundle_library_paths

bad_refs="$(
    find "$app" -type f \( -perm -111 -o -name "*.dylib" \) -print0 |
    while IFS= read -r -d '' file; do
        if file "$file" | grep -q "Mach-O"; then
            { linked_libraries "$file"; linked_rpaths "$file"; } |
                grep -E "/Users/|/home/|/opt/homebrew|/usr/local/(Cellar|opt)" || true
        fi
    done
)"
if [[ -n "$bad_refs" ]]; then
    echo "found local library references in app bundle:" >&2
    echo "$bad_refs" >&2
    exit 1
fi

version_at_most() {
    awk -v actual="$1" -v limit="$2" 'BEGIN {
        split(actual, a, ".")
        split(limit, b, ".")
        for (i = 1; i <= 3; i++) {
            av = a[i] + 0
            bv = b[i] + 0
            if (av < bv) exit 0
            if (av > bv) exit 1
        }
        exit 0
    }'
}

while IFS= read -r -d '' binary; do
    if ! file "$binary" | grep -q "Mach-O"; then
        continue
    fi
    build_versions="$(otool -l "$binary" | awk '
        $1 == "cmd" {load_command = $2}
        load_command == "LC_BUILD_VERSION" && $1 == "minos" {print $2}
        load_command == "LC_VERSION_MIN_MACOSX" && $1 == "version" {print $2}
    ')"
    if [[ -z "$build_versions" ]]; then
        echo "could not read deployment target from ${binary}" >&2
        exit 1
    fi
    while IFS= read -r actual_target; do
        if ! version_at_most "$actual_target" "$deployment_target"; then
            echo "${binary} requires macOS ${actual_target}; package limit is ${deployment_target}" >&2
            exit 1
        fi
    done <<<"$build_versions"
done < <(find "$app" -type f \( -perm -111 -o -name "*.dylib" \) -print0)

uci_out="$(printf "uci\nisready\nquit\n" | "${engine_dir}/Bitfox ${version}")"
echo "$uci_out"
grep -Fxq "id name Bitfox ${version}" <<<"$uci_out"
grep -q "^uciok$" <<<"$uci_out"
grep -q "^readyok$" <<<"$uci_out"

qt_version="$("$(dirname "$macdeployqt_bin")/qmake" -query QT_VERSION)"
python3 "${repo_root}/tools/gui_dependency_notices.py" official \
    --bundle "$app" --platform macos --qt-version "$qt_version"
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"
python3 "${repo_root}/tools/gui_dependency_notices.py" verify --bundle "$app"

mkdir -p "$dist_root"
source_date_epoch="${SOURCE_DATE_EPOCH:-$(git -C "$repo_root" show -s --format=%ct HEAD)}"
archive_time="$(date -u -r "$source_date_epoch" "+%Y%m%d%H%M.%S")"
find "$app" -exec touch -h -t "$archive_time" {} +
(
    cd "$dist_root"
    find "Bitfox.app" -print | LC_ALL=C sort | \
        zip -X -q -y "$(basename "$archive")" -@
)
shasum -a 256 "$archive" | awk -v name="$(basename "$archive")" '{print $1 "  " name}' > "${archive}.sha256"
echo "created ${archive}"
