# Performance Notes

Bitfox is built around cheap make/unmake and fast evaluation: magic-bitboard
sliders, incremental Zobrist keys, an incremental NNUE accumulator, and a shared
transposition table whose four-entry clusters occupy one aligned 64-byte cache
line.

## Hot paths

- **Move generation** is the cheapest path. Perft is pure generation and
  make/unmake with no evaluation, so it runs far faster than search.
  Slider multipliers are stored in `movegen/magic_numbers.rs`, so startup only
  builds the attack tables.
- **Search** pays for evaluation and the selectivity machinery. Ordinary
  accumulator updates are deferred until an evaluation needs them. Crossing a
  king bucket refreshes the accumulator immediately from a per-bucket cache by
  the piece difference instead of from scratch. The network is embedded as a
  64-byte aligned static, so nothing is copied or converted at startup.
- **SIMD.** On x86_64 the accumulator updates and the output layer have
  AVX-512, AVX2 and SSE2 versions, and the engine picks one the first time it
  evaluates. This matters most for the plain `x86-64-v2` build, which used to
  run the network on whatever the compiler made of SSE4.2. On aarch64 the
  accumulator uses NEON. The output layer there deliberately does not: a
  hand-written NEON version of it was measurably slower than what the
  autovectoriser produces from flat code. All kernels match the scalar code bit
  for bit, including int16 wrap-around, and the tests check each one the CPU
  supports against it.
- **Threads** use LazySMP with real worker threads and a shared transposition
  table. The helper pool is created from the `Threads` setting and reused across
  searches, avoiding thread creation on every `go`. Helpers repeat part of the
  tree, so scaling is not linear and depends on the position, time control, and
  hardware.
- **Node limits** deliberately run on the main worker only. This keeps a
  fixed-node result independent of `Threads`, but it also means that such a run
  says nothing about LazySMP scaling.

## Build flags

The release profile is tuned for the engine's hot loops:

- fat LTO and a single codegen unit;
- `opt-level = 3`;
- `panic = "abort"`.

Always measure with `make release` (or `cargo build --release --locked`); a
debug build does not reflect engine speed.

## Measuring

The `bench` subcommand runs a fixed-depth search over a fixed set of reference
positions and prints the SIMD kernels it used, total nodes and nodes per
second. It accepts the thread count and hash size so two builds can be
compared with identical resources:

```sh
bitfox bench         # default depth 12
bitfox bench 14
bitfox bench 14 4 16 # depth, threads, Hash MB
```

For move-generation throughput, time perft:

```sh
bitfox perft 6
```

Single-machine `bench` nps is useful for micro-optimisations, but one run is too
noisy to make a decision. Alternate the two builds in ABBA/BAAB order over
several cycles and compare the averages. A pure speed change leaves the
single-thread node count untouched; changes that alter the search tree should be
tested with timed self-play.
