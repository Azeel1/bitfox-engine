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

Moves are generated pseudo-legally in one pass and scored up front; the search
then picks them off by selection sort and discovers illegality by making the
move and testing whether the king is left attacked. Quiescence and ProbCut ask
for captures and promotions only. `generate_legal` filters to strictly legal
moves and exists for the UCI and FFI boundaries, not for the search. The
generator is pinned by the standard perft positions - start position, Kiwipete,
and positions 3 through 6 - in `engine/tests/perft.rs`.

## Search

Iterative deepening with aspiration windows and principal-variation search
(negamax), over a shared atomic transposition table:

- a lock-free, generation-aged transposition table shared across threads
  (`tt.rs`), sized by the UCI `Hash` option, organised into three-entry
  clusters so colliding keys displace each other far less often;
- null-move pruning, reverse futility pruning, late move pruning, futility
  pruning, history pruning, and SEE-based pruning in the move loop;
- ProbCut and singular extensions (with multi-cut and negative extensions);
- late move reductions, history- and improving-aware, with a precomputed
  log-based reduction table, reduced further at nodes whose child has already
  produced several fail-highs;
- quiescence search with SEE pruning of losing captures;
- move ordering: TT move, MVV-LVA plus SEE for captures, killers, the
  countermove that last refuted the opponent's move, and the history stack;
- the history stack: threats-aware quiet history, capture/noisy history,
  continuation history, and correction history over the pawn, per-color
  non-pawn, minor-piece and major-piece keys plus a continuation term, all
  bucketed by the fifty-move counter;
- time management that scales the soft limit by how stable the best move has
  been across iterations, so a settled choice stops early and an unsettled one
  gets longer;
- LazySMP: helper threads (`Threads` option) search in parallel and share the
  transposition table;
- principal variation, seldepth, score (including mate scores), and node/nps
  statistics reported over UCI.

## Evaluation

The default evaluator is NNUE (`eval/nnue.rs`):

- side-to-move-relative perspective network with king-bucketed,
  horizontally-mirrored input features (10 input buckets);
- a 768-neuron feature transformer per perspective, evaluated through an
  incremental accumulator whose updates are recorded on make/unmake but only
  applied on the first evaluation that needs them, so a node that never gets
  evaluated pays nothing;
- a per-king-bucket accumulator cache, so crossing a bucket boundary rebuilds
  from the cached state by the piece difference rather than from scratch;
- squared-clipped-ReLU activation, int16 quantised; accumulator adds and
  subtracts use NEON on Apple Silicon with a scalar fallback elsewhere, while
  the output layer is left to the autovectoriser, which measured faster here
  than hand-written intrinsics;
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

Strength-affecting changes are additionally gated by self-play at a real time
control on a local OpenBench instance, run as an SPRT against the current
`main`. `tools/selfplay.py` remains for quick node-limited smoke tests, but a
change is not kept on the strength of a fixed-node result alone: several
candidates that measured clearly positive at fixed nodes turned out neutral or
negative once the clock was involved.
