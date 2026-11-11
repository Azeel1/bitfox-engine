# Bitfox Engine Notes

## Board

- Squares use `0 = a1` through `63 = h8`.
- The position is six piece bitboards, two color bitboards, and a mailbox of
  `Option<Piece>` for fast square lookup.
- Incremental Zobrist keys cover the full position, pawns, non-pawns per color,
  minor pieces, and major pieces. Make/unmake pushes an undo record (captured
  piece, en passant, castling rights, clocks, key) onto a stack.
- TT probes mix a halfmove-clock bucket into the position key. Clocks 0–15
  share the position key; later buckets span eight plies and saturate at bucket
  15. At halfmove 90 or later, main search and quiescence disable direct TT
  score cutoffs, and quiescence also disables TT score refinement. Move ordering
  and the cached raw static evaluation remain available.
- Repetition draws in game state require three occurrences. Search draw
  detection also cuts a cycle that repeats inside the current search path, but
  a repeated position inherited from before the root still needs two prior
  occurrences.
- Slider attacks use magic-bitboard tables (`movegen/magic.rs`).
- Static exchange evaluation (`board/see.rs`) scores capture sequences for
  ordering and pruning; threat-square generation (`board/threats.rs`) reports
  the squares a side attacks, feeding both move ordering and history indexing.

## Move generation

Moves are generated pseudo-legally in one pass. The main search pulls the TT
move to the front when present, then scores the remaining moves lazily as
selection sort reaches them. Quiescence and ProbCut score shorter lists of
captures and promotions directly. Legality is resolved by making a move and
testing whether the king is left attacked. `generate_legal` filters to strictly
legal moves and exists for the UCI and FFI boundaries, not for the search. The
generator is pinned by the standard perft positions - start position, Kiwipete,
and positions 3 through 6 - in `engine/tests/perft.rs`.

## Search

Iterative deepening with aspiration windows and principal-variation search
(negamax), over a shared atomic transposition table:

- a generation-aged transposition table shared across threads (`tt.rs`), sized
  by the UCI `Hash` option and organised into cache-line-aligned 64-byte
  clusters of four entries. Each full-key entry is published as one 128-bit
  atomic snapshot;
- razoring, null-move pruning, reverse futility pruning, internal iterative
  reductions, late move pruning, futility pruning, history pruning, and
  SEE-based pruning in the move loop;
- ProbCut and singular extensions (with multi-cut and negative extensions);
- late move reductions, history- and improving-aware, with a precomputed
  log-based reduction table, reduced further at nodes whose child has already
  produced several fail-highs;
- quiescence search with SEE pruning of losing captures;
- move ordering: TT move, MVV-LVA plus SEE for captures, killers, the
  countermove that last refuted the opponent's move, and the history stack;
- the history stack: threats-aware quiet history, capture/noisy history,
  continuation history, and correction history over the pawn, per-color
  non-pawn, minor-piece and major-piece keys (bucketed by the fifty-move
  counter) plus a continuation correction term;
- time management that scales the soft limit by how stable the best move has
  been across iterations, so a settled choice stops early and an unsettled one
  gets longer;
- LazySMP: `Threads - 1` persistent helpers search in parallel and share the
  transposition table, stop flag, and aggregate node count. The pool is reused
  across searches. A fixed-node search bypasses the helpers and runs on the
  main worker so its result is independent of the `Threads` setting;
- principal variation, seldepth, score (including mate scores), and node/nps
  statistics reported over UCI.

## UCI command scope

The supported `go` fields are `depth`, `nodes`, `movetime`, `wtime`, `btime`,
`winc`, `binc`, `movestogo`, and `infinite`. An explicit `infinite` disables
clock stopping. If a depth or node limit finishes the calculation first, the
engine still waits for `stop` before returning `bestmove`.

`searchmoves` and `mate` are currently unsupported and ignored. Do not rely on
them to restrict root moves or request a mate-limited search. In particular,
`go mate N` without a supported limit behaves as `go infinite` and requires
`stop` to return `bestmove`.

## Evaluation

The default evaluator is NNUE (`eval/nnue.rs`):

- side-to-move-relative perspective network with king-bucketed,
  horizontally-mirrored input features (10 input buckets);
- a 768-neuron feature transformer per perspective, evaluated through an
  incremental accumulator. Ordinary move updates are recorded on make/unmake
  and applied when an evaluation needs them;
- a per-king-bucket accumulator cache, so crossing a bucket boundary rebuilds
  from the cached state by the piece difference rather than from scratch;
- squared-clipped-ReLU activation, int16 quantised. On x86_64 the accumulator
  updates and the output layer use AVX-512, AVX2 or SSE2, picked once at
  startup. On aarch64 the accumulator uses NEON and the output layer is left
  to the autovectoriser, which measured faster there than hand-written
  intrinsics. Every kernel matches the scalar code exactly, so node counts are
  the same on every machine;
- eight material-count output buckets, selected by piece count, so the output
  head specialises by phase.

The network is quantised and embedded in the binary at build time. Its
768-wide layout and size are defined in `eval/contract.rs`, which is shared by
both the evaluator and `trainer/src/train.rs`.

A classical, PSQT-based evaluator (`eval/psqt.rs`) remains as a reference and a
cross-check.

## Correctness

`cargo test --profile release-test --locked` runs the integration tests under
`engine/tests/` and the unit tests beside the code:

- perft against the standard positions;
- SEE outcomes (`see.rs`);
- evaluation behaviour (`eval.rs`);
- NNUE inference, including an incremental-equals-from-scratch accumulator check
  (`nnue.rs`);
- the same check for every Zobrist key the board maintains - main, pawn,
  per-colour non-pawn, minor and major - walked over a legal tree and compared
  against a fresh computation on the way down and after unmake;
- transposition table sizing, 64-byte cluster layout, four-way replacement,
  halfmove buckets, and score-reuse guards near the fifty-move boundary;
- threefold game-state draws and the separate search-path repetition rule;
- FEN parsing of castling rights and en passant squares that do not match the
  position;
- UCI option clamping;
- C ABI layout, buffer capacities, error paths, and owned searcher handles.

Strength-affecting changes are checked by paired Fastchess matches at a real
time control, with `tools/selfplay.py` kept for quick node-limited screens.
