# Bitfox Board

A Qt 6 desktop board for playing against Bitfox engine releases. Pick an engine
for either side, set the think time, and play with drag-and-drop, sounds, move
list, captured-piece tray, and undo/redo.

## Requirements

- Rust stable >= 1.85
- Qt 6 (Widgets, Multimedia)
- CMake 3.21+ and a C++17 compiler

The CMake build runs `cargo build --release` in `engine/` and links the board
against the produced `libbitfox` engine core.

## Build

```sh
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build --parallel
```

The binary is `build/bitfox-board`.

## Engines

Drop UCI engine binaries into an `engines/` folder next to the executable (or in
the working directory). Each binary appears in the White and Black selectors, so
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
├── include/bitfox_core.h     C ABI exposed by the engine core
├── resources/                pieces, sounds, stylesheet
└── src/
    ├── core/                 board state over the engine core
    ├── board/                board rendering and captured-piece tray
    ├── engine/               UCI process driver and release discovery
    ├── audio/                sound effects
    ├── game/                 game flow, history, undo/redo
    └── ui/                   main window and side panel
```
