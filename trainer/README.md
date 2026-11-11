# bitfox-trainer

NNUE training for the bitfox chess engine, built on
[Bullet](https://github.com/jw1912/bullet). The default build uses Metal;
CUDA is available through the `cuda` feature.

The trainer is a separate Cargo package that imports
`engine/src/eval/contract.rs` so training and inference share the same
network layout. It does not modify the engine crate; a selected quantised
network is copied into the engine in a separate step.

## Pipeline

```
engine datagen  ->  text (fen | score | wdl)  ->  convert  ->  *.data  ->  Bullet  ->  quantised.bin
```

1. `bitfox datagen` plays fixed-node self-play games and writes one line per
   quiet position: `FEN | score | wdl`, both white-relative (`DATAFORMAT.md`).
   A seed and worker count produce the same worker streams and output order on
   repeated runs.
2. `convert/` turns that text into a `bulletformat` `ChessBoard` file (`.data`).
3. `src/train.rs` configures the trainer and streams the records on the GPU.
4. The run writes checkpoints under `checkpoints/`; each holds `quantised.bin`,
   the network the engine reads.

## Network architecture

Side-to-move-relative perspective network, king-bucketed inputs with output
buckets:

- **Inputs** - `(king_bucket, piece, square)` features from both perspectives.
  The board is mirrored so a king on the e-h files folds onto a-d, halving the
  king geometry before bucketing.
- **King buckets** - 10 buckets over the mirrored king square (`BUCKET_LAYOUT`).
- **Feature transformer** - the shared contract fixes `HL = 768` neurons per
  perspective; the two perspectives concatenate into a `2 * HL` accumulator.
- **Activation** - SCReLU, `clamp(x, 0, 1)^2`.
- **Output buckets** - 8, selected by piece count, so the head specialises by
  material.
- **Output** - one scalar per bucket (centipawn-scale eval).

### Quantisation

- Feature transformer (`l0`): int16, scale `QA = 255`.
- Output layer (`l1`): int16 weights scaled `QB = 64`, biases `QA * QB`.
- `SCALE = 400` maps sigmoid space back to centipawns.

`engine/src/eval/contract.rs` defines `HL`, `BUCKET_LAYOUT`, the input and
output bucket counts, `QA`, `QB`, `SCALE`, and the encoded network size for both
`eval/nnue.rs` and `trainer/src/train.rs`.

## Building and running

Run these Cargo commands from `trainer/`. The experiment commands below run
from the repository root.

```
# convert datagen text to the training format
cargo run --release --locked --manifest-path convert/Cargo.toml -- data.txt data/bitfox.data

# train with the default Metal backend
cargo run --release --locked --bin train

# or train with CUDA
CUDA_PATH=/usr/local/cuda cargo run --release --locked --no-default-features --features cuda --bin train
```

### Run settings

Every knob is an environment variable read at startup, so a run is fully
described by the command that launched it:

| Variable | Default | Meaning |
| -------- | ------- | ------- |
| `BITFOX_TRAIN_ID` | `bitfox` | Name of the run; checkpoint directories are `<id>-<superbatch>` |
| `BITFOX_TRAIN_DATA` | `data/bitfox.data` | Training data; several files can be joined with the platform path separator |
| `BITFOX_TRAIN_OUTPUT` | `checkpoints` | Where checkpoint directories are written |
| `BITFOX_TRAIN_HL` | `768` | Compatibility check; any other value is rejected |
| `BITFOX_TRAIN_SEED` | `198273612` | Deterministic weight-initialisation seed |
| `BITFOX_TRAIN_SUPERBATCHES` | 8 | Superbatches to run; also where the LR schedule bottoms out |
| `BITFOX_TRAIN_BATCHES_PER_SUPERBATCH` | 1000 | Batches of 16384 positions per superbatch |
| `BITFOX_TRAIN_SAVE_RATE` | 2 | Save a checkpoint every N superbatches |
| `BITFOX_TRAIN_WDL` | 0.4 | Weight of the game result against the search score in the target |
| `BITFOX_TRAIN_INITIAL_LR` | 0.001 | Learning rate at the start of the cosine decay |
| `BITFOX_TRAIN_FINAL_LR` | `initial * 0.027` | Learning rate at the last superbatch |
| `BITFOX_TRAIN_THREADS` | 6 | Data-loading threads |
| `BITFOX_TRAIN_START_SUPERBATCH` | 1 | First superbatch; values above 1 require `BITFOX_TRAIN_RESUME` |
| `BITFOX_TRAIN_RESUME` | unset | Checkpoint directory used to resume a schedule; required exactly when the first superbatch is above 1, and rejected when it is 1 |

Set `BITFOX_TRAIN_BATCHES_PER_SUPERBATCH` to about `positions / 16384` for one
epoch per superbatch, then size `BITFOX_TRAIN_SUPERBATCHES` to the epochs you
want. Keep `BITFOX_TRAIN_SAVE_RATE` low enough to get several checkpoints out of
a run: the last one is not reliably the strongest, and the only way to know
which is, is to play them.

```sh
BITFOX_TRAIN_ID=gen16-sl768 \
BITFOX_TRAIN_DATA=runs/gen16-combined.data \
BITFOX_TRAIN_OUTPUT=runs/gen16-sl768-ckpt \
BITFOX_TRAIN_HL=768 \
BITFOX_TRAIN_SEED=198273612 \
BITFOX_TRAIN_SUPERBATCHES=60 \
BITFOX_TRAIN_BATCHES_PER_SUPERBATCH=6000 \
BITFOX_TRAIN_SAVE_RATE=10 \
cargo run --release --locked --bin train
```

## Experiments

Use `experiment.py` to keep generated data, checkpoints, exported networks, and
match logs under one run directory:

```sh
python3 -m pip install -r tools/requirements.txt
python3 trainer/experiment.py gen1 \
  --positions 2000000 \
  --nodes 10000 \
  --threads 6 \
  --superbatches 8 \
  --batches-per-superbatch 1000 \
  --games 400 \
  --match-nodes 20000
```

The script writes to `trainer/runs/<name>/`, exports
`trainer/runs/<name>/networks/<name>.nnue`, builds a temporary candidate engine
with that network, then runs the self-play gate against the current engine.
`--threads` is used by both self-play data generation and training.
Use `--extra-data-bin <file>` to train over additional Bullet data files in the
same run.

To test an existing network without regenerating data:

```sh
python3 trainer/experiment.py gen1-test \
  --skip-datagen \
  --skip-convert \
  --skip-train \
  --network trainer/runs/gen1/networks/gen1.nnue \
  --games 400 \
  --match-nodes 20000
```

The experiment match is a node-limited screen for ranking checkpoints. It does
not test time management or LazySMP, so play a timed match against the current
network before shipping a new one.
