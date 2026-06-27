# Changelog

All notable changes to Bitfox are recorded here.

Versions follow [semantic versioning](https://semver.org): `MAJOR.MINOR.PATCH`.

- **MAJOR** - a new playing style or a rewrite of a core subsystem.
- **MINOR** - a search or evaluation feature that passes self-play testing.
- **PATCH** - a tuning tweak, bug fix, or maintenance change.

Releases are checked by fixed-node self-play (`tools/selfplay.py`). The version
in `engine/Cargo.toml` is the version reported over UCI.

## [1.1.0]

### Search
- Threat-aware quiet move ordering: a bonus for moving an attacked piece to
  safety, a bonus for quiet checks, and a penalty for moving onto a square
  controlled by a lower-valued piece. Worth about +30 Elo in self-play.

### Fixed
- Game-over detection now requires a genuine threefold repetition. A single
  repeated position no longer ends the game as a draw.

## [1.0.0]

First tagged release. Consolidates the development series into a stable build
and freezes the UCI interface and the evaluation network format.

## [0.5.0]

### Search
- Lazy SMP multithreading over a shared, atomically updated transposition table.
- New `Threads` UCI option.

## [0.4.0]

### Search
- Singular extensions with multi-cut and negative extensions.
- ProbCut.
- Correction history (pawn and non-pawn) applied to the static evaluation.

## [0.3.0]

### Evaluation
- Replaced the hand-written evaluation with an NNUE network: king-bucketed,
  perspective-mirrored inputs and material-count output buckets.

### Search
- SEE-ordered captures and continuation history in move ordering.

## [0.2.0]

### Search
- Move ordering: hash move, MVV-LVA captures, killers, and butterfly history.
- Pruning suite: null-move, reverse futility, late-move pruning, futility, and
  SEE-based pruning, with history-driven late-move reductions.

## [0.1.0]

Initial engine.

- Bitboard board representation with magic-bitboard sliding attacks, verified
  against perft.
- Principal-variation search with iterative deepening and aspiration windows.
- Quiescence search and a transposition table.
- UCI front end and the desktop board C ABI.
