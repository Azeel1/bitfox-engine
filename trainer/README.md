# bitfox-trainer

NNUE training for the bitfox chess engine, built on
[Bullet](https://github.com/jw1912/bullet). The default build uses Metal;
CUDA is available through the `cuda` feature.

This directory is self-contained: it neither depends on nor modifies the engine
crate under `engine/`. Its only product is a quantised network the engine loads.

## Pipeline

```
engine datagen  ->  text (fen | score | wdl)  ->  convert  ->  *.data  ->  Bullet  ->  quantised.bin
```

1. `bitfox datagen` plays fixed-node self-play games and writes one line per
   quiet position: `FEN | score | wdl`, both white-relative (`DATAFORMAT.md`).
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
- **Feature transformer** - `HL = 768` neurons per perspective; the two
  perspectives concatenate into a `2 * HL` accumulator.
- **Activation** - SCReLU, `clamp(x, 0, 1)^2`.
- **Output buckets** - 8, selected by piece count, so the head specialises by
  material.
- **Output** - one scalar per bucket (centipawn-scale eval).

### Quantisation

- Feature transformer (`l0`): int16, scale `QA = 255`.
- Output layer (`l1`): int16 weights scaled `QB = 64`, biases `QA * QB`.
- `SCALE = 400` maps sigmoid space back to centipawns.

`HL`, `BUCKET_LAYOUT`, `NUM_OUTPUT_BUCKETS`, `QA`, `QB`, `SCALE` are the contract
between trainer and engine - change them in both `src/train.rs` and the engine's
`eval/nnue.rs`.

## Building and running

```
# convert datagen text to the training format
cargo run --release --manifest-path convert/Cargo.toml -- data.txt data/bitfox.data

# train with the default Metal backend
cargo run --release --bin train

# or train with CUDA
CUDA_PATH=/usr/local/cuda cargo run --release --no-default-features --features cuda --bin train
```

Set `batches_per_superbatch` to about `positions / 16384` for one epoch per
superbatch, then size `end_superbatch` to the epochs you want.

## Experiments

Use `experiment.py` to keep generated data, checkpoints, exported networks, and
match logs under one run directory:

```sh
python3 trainer/experiment.py gen1 \
  --positions 2000000 \
  --nodes 10000 \
  --superbatches 8 \
  --batches-per-superbatch 1000 \
  --games 400 \
  --match-nodes 20000
```

The script writes to `trainer/runs/<name>/`, exports
`trainer/runs/<name>/networks/<name>.nnue`, builds a temporary candidate engine
with that network, then runs the self-play gate against the current engine.

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
