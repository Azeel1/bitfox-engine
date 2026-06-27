# Bitfox Engine Notes

## Board

- Squares use `0 = a1` through `63 = h8`.
- The position is six piece bitboards, two color bitboards, and a mailbox of
  `Option<Piece>` for fast square lookup.
- Three incremental Zobrist keys are maintained: the full key, a pawn-only key,
  and a non-pawn key per color. Make/unmake pushes an undo record (captured
  piece, en passant, castling rights, clocks, key) onto a stack.
- Slider attacks use magic-bitboard tables (`movegen/magic.rs`).
- Static exchange evaluation (`board/see.rs`) scores capture sequences for
  ordering and pruning; threat-square generation (`board/threats.rs`) reports
  the squares a side attacks, feeding both move ordering and history indexing.

## Move generation

Pseudo-legal moves are produced in stages and filtered to legal moves. The
generator is pinned by the standard perft positions - start position, Kiwipete,
and positions 3 through 6 - in `engine/tests/perft.rs`.

## Search

Iterative deepening with aspiration windows and principal-variation search
(negamax), over a shared atomic transposition table:

- a lock-free, generation-aged transposition table shared across threads
  (`tt.rs`), sized by the UCI `Hash` option;
- null-move pruning, reverse futility pruning, late move pruning, futility
  pruning, history pruning, and SEE-based pruning in the move loop;
- ProbCut and singular extensions (with multi-cut and negative extensions);
- late move reductions, history- and improving-aware, with a precomputed
  log-based reduction table;
- quiescence search with SEE pruning of losing captures;
- move ordering: TT move, MVV-LVA plus SEE for captures, killers, and the
  history stack;
- the history stack: threats-aware quiet history, capture/noisy history,
  continuation history, and correction history keyed on the pawn and per-color
  non-pawn keys and bucketed by the fifty-move counter;
- LazySMP: helper threads (`Threads` option) search in parallel and share the
  transposition table;
- principal variation, seldepth, score (including mate scores), and node/nps
  statistics reported over UCI.

## Evaluation

The default evaluator is NNUE (`eval/nnue.rs`):

- side-to-move-relative perspective network with king-bucketed,
  horizontally-mirrored input features (10 input buckets);
- a 768-neuron feature transformer per perspective, evaluated through an
  incremental accumulator that is updated on make/unmake and refreshed when the
  moving king crosses a bucket boundary;
- squared-clipped-ReLU activation, int16 quantised, with a NEON SIMD path on
  Apple Silicon and a scalar fallback elsewhere;
- eight material-count output buckets, selected by piece count, so the output
  head specialises by phase.

The network is quantised and embedded in the binary at build time.

A classical, PSQT-based evaluator (`eval/psqt.rs`) remains as a reference and a
cross-check.

## Correctness

The test suite (`engine/tests/`, run by `cargo test --release`) covers:

- perft against the standard positions;
- SEE outcomes (`see.rs`);
- evaluation behaviour (`eval.rs`);
- NNUE inference, including an incremental-equals-from-scratch accumulator check
  (`nnue.rs`).

Strength-affecting changes are additionally gated through `tools/selfplay.py`,
an SPRT-style node-limited self-play match against a saved baseline.
