# Performance Notes

Bitfox is built around cheap make/unmake and fast evaluation: magic-bitboard
sliders, incremental Zobrist keys, an incremental NNUE accumulator, and a shared
lock-free transposition table.

## Hot paths

- **Move generation** is the cheapest path. Perft is pure generation and
  make/unmake with no evaluation, so it runs far faster than search.
- **Search** pays for evaluation and the selectivity machinery. Accumulator
  updates are deferred until an evaluation actually needs them, so nodes that
  cut off before evaluating pay nothing; crossing a king bucket rebuilds from a
  per-bucket cache by the piece difference instead of from scratch. On Apple
  Silicon the accumulator adds and subtracts use NEON intrinsics. The output
  layer deliberately does not: a hand-written NEON version of it was measurably
  slower than what the autovectoriser produces from flat code.
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

Single-machine `bench` nps is the right signal for micro-optimisations, and a
change that is meant to be pure speed should leave the node count untouched -
if the totals move, the search changed too and nps alone no longer tells you
anything. For anything that changes what the search explores, use the self-play
gate at a real time control rather than raw speed or fixed-node results.
