# Changelog

All notable changes to Bitfox are recorded here.

Versions follow [semantic versioning](https://semver.org): `MAJOR.MINOR.PATCH`.

- **MAJOR** - a new playing style or a rewrite of a core subsystem.
- **MINOR** - a search or evaluation feature that passes self-play testing.
- **PATCH** - a tuning tweak, bug fix, or maintenance change.

Releases are checked by self-play at real time control. The version in
`engine/Cargo.toml` is the version reported over UCI.

## [2.5.0]

Measured about +60 Elo over 2.1.0 at short time control. The search and
evaluation changes were each confirmed on their own by SPRT before being
kept; the rest are correctness and interface fixes.

### Search
- The soft time limit now scales with how stable the best move has been
  across iterations - it stops sooner once the move stops changing and
  lingers while the choice is still moving. +13 Elo.

### Evaluation
- Accumulator updates are applied lazily on the first evaluation that
  needs them, so moves whose node never gets evaluated (a transposition
  cutoff, a pruned branch) pay nothing for the update. +24 Elo.
- Each king bucket keeps its own cached accumulator, so a king move that
  crosses a bucket boundary refreshes from the cached state by the piece
  difference instead of rebuilding from scratch. +15 Elo.

### Transposition table
- The single-slot table is now organised into three-entry clusters, and a
  resize no longer wastes half the requested memory - a request of N MB
  now allocates N MB instead of N/2.

### Correctness
- A checkmate delivered on the hundredth half-move is scored as a win
  rather than a fifty-move draw, and two bishops confined to one square
  colour are recognised as insufficient material.
- Malformed FEN input is normalised at parse time: castling rights with no
  matching king and rook, and en passant squares with no pawn to capture,
  are dropped instead of driving move generation into an illegal state.
- Out-of-range `setoption` values are clamped to their advertised bounds
  instead of being stored raw.

### Interface
- The search runs on its own thread, so `stop` and `isready` are answered
  immediately during a search and `go infinite` can actually be
  interrupted.

## [2.1.0]

### Search
- Second cutoff-count tier: nodes whose child already produced several
  fail-highs reduce late moves further. +15 Elo (pooled over 900 games).
- Countermove heuristic: the quiet move that last refuted a given
  opponent move is tried early. +15 Elo (pooled over 900 games).

### GUI
- Pieces lift and scale up on drag, with a shadow and a faded ghost left on
  the origin square; moves slide into place with an eased 200ms animation.
- Board rendering picks the sharper piece asset automatically once the
  window is resized past the default size, avoiding blur.

## [2.0.0]

Bitfox 2. Measured +74 Elo at fixed nodes and +133 Elo at real time control
against 1.1.1, both confirmed by SPRT.

### Evaluation
- Stronger network trained on 59M mixed self-play positions. +118 Elo.

### Search
- Aspiration windows: score-centered window, adaptive delta growth, and a
  depth reduction on repeated non-decisive fail-highs. +42 Elo.
- Continuation correction history alongside the existing pawn and non-pawn
  correction history. +36 Elo.
- Minor and major piece correction history. +27 Elo.
- Cutoff-count based late-move reductions. +18 Elo.

### Endgame
- Syzygy tablebase probing (3-4-5 piece WDL) for exact endgame scores.

### GUI
- In-app version manager: browse and download GitHub releases without
  leaving the app. The engine catalog auto-selects the newest version.
- Settings (sound, premove, automatic updates) persist between runs.
- Optional automatic check for the newest engine version on startup.

### Removed
- The live depth/eval readout in the side panel. It read the engine's raw
  score without normalizing for its native scale, which produced misleading
  numbers; it will come back once fixed properly.

## [1.1.1]

### Packaging
- Added checked GUI package builds for Windows, Linux, and macOS.

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
