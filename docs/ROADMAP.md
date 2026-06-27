# Bitfox Roadmap

Board representation, move generation, search, NNUE evaluation, and
multi-threading are in place. Current work is mostly strength tuning.

## Now

- **More and better data.** Scale up `datagen` self-play volume, raise the search
  depth used to label positions, and keep the opening set diverse so the network
  learns from stronger, broader targets.
- **Bigger networks.** A larger hidden layer and richer input features once a
  matching network trains.
- **Tuning.** Re-tune the search's pruning, reduction, and extension constants on
  the engine's own evaluation, and the NNUE training hyperparameters, each gated
  by the self-play match.

## Later

- MultiPV and pondering over UCI.
- Endgame tablebase support.

## Done

- Bitboard core: magic-bitboard sliders, incremental Zobrist keys (full, pawn,
  per-color non-pawn), make/unmake, SEE, threat-square generation.
- Staged legal move generation, perft-verified (startpos, Kiwipete,
  positions 3-6).
- Search: iterative deepening, aspiration windows, PVS, quiescence with SEE
  pruning, shared atomic TT, null-move / reverse-futility / late-move / futility
  / history / SEE pruning, history-aware LMR, killers, MVV-LVA + SEE ordering,
  continuation and correction history, singular extensions (multi-cut and
  negative extensions), and ProbCut.
- LazySMP multi-threading with a shared transposition table; UCI `Hash` and
  `Threads` options.
- NNUE evaluation: king-bucketed mirrored inputs, material-count output buckets,
  squared-clipped-ReLU, int16-quantised incremental accumulator with NEON SIMD
  on Apple Silicon; classical PSQT evaluator kept as a reference.
- Training pipeline: `datagen` self-play, text-to-bulletformat conversion, and
  Bullet-based network training (`trainer/`).
- `tools/selfplay.py` SPRT-style self-play gate for every strength-affecting
  change.
