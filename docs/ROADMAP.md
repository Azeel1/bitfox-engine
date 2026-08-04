# Bitfox Roadmap

Board representation, move generation, search, NNUE evaluation, and
multi-threading are in place. Current work is mostly strength tuning.

## Now

- **A deeper network head.** The evaluation runs from the feature transformer
  straight to one output per bucket. Widening that transformer was tried and
  lost; adding depth after it - pairwise-multiplied accumulator halves feeding
  two small layers before the output - is the change worth making, and is what
  the extra data below is for.
- **More and better data.** Scale up `datagen` self-play volume and raise the
  search depth used to label positions. Training length is not the constraint -
  the current schedule sees a fraction of what the network could absorb - but
  training longer only helps with enough distinct positions to train on.
- **Tuning.** Re-tune the pruning, reduction, and extension constants against
  the engine's own evaluation, and the NNUE training hyperparameters, each gated
  by a self-play match.

## Later

- MultiPV and pondering over UCI.

## Done

- Bitboard core: magic-bitboard sliders, incremental Zobrist keys (full, pawn,
  per-color non-pawn), make/unmake, SEE, threat-square generation.
- Pseudo-legal move generation with legality resolved on make, perft-verified
  (startpos, Kiwipete, positions 3-6).
- Search: iterative deepening, aspiration windows, PVS, quiescence with SEE
  pruning, null-move / reverse-futility / late-move / futility / history / SEE
  pruning, history-aware LMR, killers, countermoves, MVV-LVA + SEE ordering,
  continuation and correction history, singular extensions (multi-cut and
  negative extensions), and ProbCut.
- Transposition table: lock-free, generation-aged, three-entry clusters, sized
  exactly to the requested `Hash`.
- Time management that scales the soft limit by best-move stability.
- LazySMP multi-threading with a shared transposition table; UCI `Hash` and
  `Threads` options, plus the search constants exposed as tunable options.
- The search runs on its own thread, so `stop` and `isready` are answered while
  it is still thinking.
- NNUE evaluation: king-bucketed mirrored inputs, material-count output buckets,
  squared-clipped-ReLU, int16-quantised accumulator with deferred updates, a
  per-bucket accumulator cache, and NEON on Apple Silicon; classical PSQT
  evaluator kept as a reference.
- Syzygy tablebase probing (3-4-5 piece WDL).
- Training pipeline: `datagen` self-play, text-to-bulletformat conversion, and
  Bullet-based network training (`trainer/`).
- Self-play gating at a real time control on a local OpenBench instance, with
  `tools/selfplay.py` kept for quick fixed-node smoke tests.
