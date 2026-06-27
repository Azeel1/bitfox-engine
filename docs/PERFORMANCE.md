# Performance Notes

Bitfox is built around cheap make/unmake and fast evaluation: magic-bitboard
sliders, incremental Zobrist keys, an incremental NNUE accumulator, and a shared
lock-free transposition table.

## Hot paths

- **Move generation** is the cheapest path. Perft is pure generation and
  make/unmake with no evaluation, so it runs far faster than search.
- **Search** pays for evaluation and the selectivity machinery. The NNUE
  accumulator is updated incrementally on make/unmake and only fully refreshed
  when the moving king crosses a bucket boundary, which keeps per-node
  evaluation cheap; on Apple Silicon the accumulator and forward pass use NEON
  SIMD.
- **Threads** scale through LazySMP: helper threads share the transposition
  table, so raising the `Threads` option increases effective depth rather than
  raw single-thread nps.

## Build flags

The release profile is tuned for the engine's hot loops:

- fat LTO and a single codegen unit;
- `opt-level = 3`;
- `panic = "abort"`.

Always measure with `make release` (or `cargo build --release`); a debug build
does not reflect engine speed.

## Measuring

The `bench` subcommand runs a fixed-depth search over a fixed set of reference
positions and prints total nodes and nodes per second. Use it to compare two
builds on the same machine:

```sh
bitfox bench         # default depth 12
bitfox bench 14
```

For move-generation throughput, time perft:

```sh
bitfox perft 6
```

Single-machine `bench` nps is the right signal for micro-optimisations; for
anything that changes what the search explores, use the self-play gate
(`tools/selfplay.py`), which measures playing strength under a fixed node limit
rather than raw speed.
