# Bitfox Board

A Qt 6 desktop board for playing against Bitfox engine releases. Pick an engine
for either side, set the think time, and play with drag-and-drop, sounds, move
list, captured-piece tray, and undo/redo.

## Requirements

- Rust 1.96 (`../rust-toolchain.toml` and the crate requirement)
- Qt 6 (Widgets, Multimedia, Network)
- CMake 3.21+ and a C++17 compiler

The CMake build runs `cargo build --release --locked` in `engine/` and links the
board against the produced `libbitfox` engine core. The shared C ABI is
defined in `../engine/include/bitfox_core.h`; the `bitfox_core` CMake target
provides that include directory to its consumers.

## Build

```sh
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build --parallel
```

The binary is `build/bitfox-board`.

To use another Cargo output directory, pass `-DCARGO_TARGET_DIR=/path/to/target`
or set the environment variable before configuring. The older
`ENGINE_LIB_DIR` setting is still accepted, but it must point to the matching
Cargo release directory. CMake rejects conflicting paths instead of linking a
stale library.

On macOS, an explicit architecture selects the matching Rust target:

```sh
cmake -S . -B build-arm64 -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_OSX_ARCHITECTURES=arm64
```

A universal `arm64;x86_64` build needs two separately built Rust libraries and
is not assembled by this project.

Enable the warning-clean build and run the UCI process, main window, release
extraction, application updater, and runtime-layout tests with:

```sh
cmake -S . -B build-strict -DCMAKE_BUILD_TYPE=Release \
  -DBUILD_TESTING=ON \
  -DBITFOX_STRICT_WARNINGS=ON
cmake --build build-strict --parallel
ctest --test-dir build-strict --output-on-failure
```

`cmake --install build --prefix /path/to/stage` installs the board and engine
core together. Qt runtime deployment and application signing are separate
packaging steps.

## Engines

Drop UCI engine binaries into an `engines/` folder next to the executable. Each
binary appears in the White and Black selectors, so
you can match any two releases against each other. With no engines present, both
sides default to human play.

## Controls

- Click or drag a piece to move; release on a highlighted square.
- Undo / Redo with the buttons or the left / right arrow keys.
- Flip rotates the board.

## Layout

```
gui-qt/
├── CMakeLists.txt
├── cmake/                    local CTest helpers
├── include/bitfox_core.h     compatibility include for the engine C ABI
├── resources/                pieces, sounds, stylesheet
└── src/
    ├── core/                 board state over the engine core
    ├── board/                board rendering and captured-piece tray
    ├── engine/               UCI process driver and release discovery
    ├── audio/                sound effects
    ├── game/                 game flow, history, undo/redo
    └── ui/                   main window and side panel
```

## Application updates

3.0.0 is the first version that can update itself, so anyone on 2.5.0 has to
download this one by hand.

The app checks for a new release once a day (you can turn that off in
Settings), and **Check for application updates** checks right away. It looks at
the latest stable release of `Azeel1/bitfox-engine` and picks the GUI package
for your system:

- `bitfox-VERSION-linux-x86_64-gui.tar.gz`
- `bitfox-VERSION-macos-arm64-gui.zip`
- `bitfox-VERSION-windows-x86_64-gui.zip`

**Update now** downloads it and checks its size and SHA-256 against the
release, and **Later** skips that release for a week. Once the package is
verified, **Open verified package** shows it in your file manager, and you
replace the installed app yourself after closing it. The checksum only proves
the file is the one on the release page; it is not a signed or notarized
installer.

Engine versions are downloaded separately, under **Get engine versions**.
