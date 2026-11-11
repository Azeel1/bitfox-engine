# Bitfox Architecture

Bitfox is a single Rust crate (`engine/`) that builds two artifacts from the
same code: the `bitfox` UCI binary, and a `cdylib` shared library exposing a C
ABI.

## Crate layout

The library is organised into focused modules under `engine/src/`:

- `types/` - squares, pieces, colors, moves, bitboards, castling rights, and
  score constants;
- `board/` - board state and the mailbox, plus FEN parsing (`fen.rs`),
  make/unmake (`make.rs`), Zobrist hashing (`zobrist.rs`), static exchange
  evaluation (`see.rs`), threat-square generation (`threats.rs`), and draw and
  repetition handling;
- `movegen/` - pseudo-legal move generation and the magic-bitboard slider
  tables (`magic.rs`);
- `search/` - iterative deepening (`iterative.rs`), negamax PVS (`negamax.rs`),
  quiescence (`quiescence.rs`), move ordering (`ordering.rs`), the history
  tables (`history.rs`), correction history (`corrhist.rs`), LazySMP helpers
  (`smp.rs`), search parameters (`params.rs`), Syzygy probing (`syzygy.rs`),
  the principal variation (`pv.rs`), and time management (`time.rs`);
- `eval/` - the shared NNUE layout (`contract.rs`), inference (`nnue.rs`), the
  SIMD kernels (`simd.rs`), the embedded network in `engine/networks/`, and the
  classical PSQT reference evaluator (`psqt.rs`);
- `tt.rs` - the shared atomic transposition table, arranged as four entries in
  each 64-byte cache-line-aligned cluster;
- `tools/` - the `perft`, `bench`, and `datagen` subcommands;
- `uci.rs` - the UCI protocol loop;
- `ffi.rs` - the exported `cc_*` C boundary used by the GUI.

`main.rs` dispatches the command line (`uci`, `perft`, `divide`, `bench`,
`datagen`); with no argument it starts the UCI loop.

## The two artifacts

`Cargo.toml` declares both an `rlib`/binary and a `cdylib`. The binary is the
UCI engine. The `cdylib` exports the `cc_*` functions in `ffi.rs` - board
creation, FEN, legal-move generation, make-move, search, perft, and position
queries - over a flat C ABI. Capacity-aware entry points and an owned searcher
handle are available alongside the legacy calls.

For capacity-aware `_n` calls, text sizes include the terminating NUL and array
sizes count entries. A null output is accepted only with zero capacity. If the
capacity is too small, the call writes nothing. `cc_search` and `cc_search_ex`
use the calling thread's legacy search state; owned searcher handles have
separate state.

The Qt board links against the same `cdylib` through the C header in
`engine/include/bitfox_core.h` (`gui-qt/include/bitfox_core.h` forwards to it).
Board state, legal move highlights, search, and position queries flow through
that C ABI; UCI engines are launched separately by the Qt process driver when a
side is assigned to an engine binary.

## Search ownership

One `Search` owns the main search state, the shared transposition table, and a
pool of `Threads - 1` helper threads. The helpers persist between searches and
run independent LazySMP searches over cloned boards while sharing the table,
stop flag, and node counter. A node-limited search bypasses the helper pool and
uses the main worker only so its budget and result do not depend on `Threads`.

The board's ordinary Zobrist key describes the position. TT access derives a
separate key that also mixes in the halfmove clock, capped at the fifty-move
boundary. Repetition draws in game state require three occurrences. Inside
search, a repetition created within the current path can be scored as a cycle
after one prior occurrence, while repetitions that predate the root still
require the full threefold history.

## Training pipeline

NNUE networks are trained in the separate `trainer/` workspace, built on Bullet.
The engine's `datagen` subcommand plays fixed-node self-play games and writes
one `FEN | score | wdl` record per quiet position. `trainer/convert` turns that
text into bulletformat, and the trainer produces a quantised network that the
engine embeds at build time
(`include_bytes!("../../networks/bitfox.nnue")`). The trainer imports
`engine/src/eval/contract.rs` directly, so both sides compile against the same
768-wide layout, bucket counts, quantisation scales, and network size. Training
does not modify the engine crate; replacing the embedded network is a separate
build step. `engine/build.rs` reads the network before it is embedded and fails
the build if its weights could overflow the int16 accumulator or the int16
products in the output layer.
