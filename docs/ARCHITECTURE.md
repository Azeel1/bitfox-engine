# Bitfox Architecture

Bitfox is a single Rust crate (`engine/`) that builds two artifacts from the
same code: the `bitfox` UCI binary, and a `cdylib` shared library exposing a C
ABI. There is one chess implementation; nothing reimplements the engine
elsewhere.

## Crate layout

The library is organised into focused modules under `engine/src/`:

- `types/` - squares, pieces, colors, moves, bitboards, castling rights, and
  score constants;
- `board/` - board state and the mailbox, plus FEN parsing (`fen.rs`),
  make/unmake (`make.rs`), Zobrist hashing (`zobrist.rs`), static exchange
  evaluation (`see.rs`), and threat-square generation (`threats.rs`);
- `movegen/` - staged legal move generation and the magic-bitboard slider
  tables (`magic.rs`);
- `search/` - iterative deepening (`iterative.rs`), negamax PVS (`negamax.rs`),
  quiescence (`quiescence.rs`), move ordering (`ordering.rs`), the history
  tables (`history.rs`), correction history (`corrhist.rs`), the principal
  variation (`pv.rs`), and time management (`time.rs`);
- `eval/` - NNUE inference (`nnue.rs`) with the embedded network under
  `networks/`, and the classical PSQT reference evaluator (`psqt.rs`);
- `tt.rs` - the shared, lock-free transposition table;
- `tools/` - the `perft`, `bench`, and `datagen` subcommands;
- `uci.rs` - the UCI protocol loop;
- `ffi.rs` - the exported `cc_*` C boundary used by the GUI.

`main.rs` dispatches the command line (`uci`, `perft`, `divide`, `bench`,
`datagen`); with no argument it starts the UCI loop.

## The two artifacts

`Cargo.toml` declares both an `rlib`/binary and a `cdylib`. The binary is the
UCI engine. The `cdylib` exports the `cc_*` functions in `ffi.rs` - board
creation, FEN, legal-move generation, make-move, search, perft, and position
queries - over a flat C ABI.

The Qt board links against the same `cdylib` through the C header in
`gui-qt/include/bitfox_core.h`. Board state, legal move highlights, search, and
position queries flow through that C ABI; UCI engines are launched separately by
the Qt process driver when a side is assigned to an engine binary.

## Training pipeline

NNUE networks are trained in the separate `trainer/` workspace, built on Bullet.
The engine's `datagen` subcommand plays fixed-node self-play games and writes
one `FEN | score | wdl` record per quiet position; `trainer/convert` turns that
text into bulletformat, and the trainer produces a quantised network that the
engine embeds at build time
(`include_bytes!("../../networks/bitfox.nnue")`). `trainer/` neither depends on
nor modifies the engine crate.
