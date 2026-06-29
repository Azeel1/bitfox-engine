use bullet_lib::{
    game::{
        inputs::{get_num_buckets, ChessBucketsMirrored},
        outputs::MaterialCount,
    },
    nn::{optimiser::AdamW, InitSettings, Shape},
    trainer::{
        save::SavedFormat,
        schedule::{lr, wdl, TrainingSchedule, TrainingSteps},
        settings::LocalSettings,
    },
    value::{loader::DirectSequentialDataLoader, ValueTrainerBuilder},
};
use std::env;

const HL: usize = 768;
const SCALE: i32 = 400;
const QA: i16 = 255;
const QB: i16 = 64;

const NUM_OUTPUT_BUCKETS: usize = 8;

#[rustfmt::skip]
const BUCKET_LAYOUT: [usize; 32] = [
    0, 1, 2, 3,
    4, 5, 6, 7,
    8, 8, 8, 8,
    9, 9, 9, 9,
    9, 9, 9, 9,
    9, 9, 9, 9,
    9, 9, 9, 9,
    9, 9, 9, 9,
];

const NUM_INPUT_BUCKETS: usize = get_num_buckets(&BUCKET_LAYOUT);

fn env_string(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let mut trainer = ValueTrainerBuilder::default()
        .dual_perspective()
        .optimiser(AdamW)
        .inputs(ChessBucketsMirrored::new(BUCKET_LAYOUT))
        .output_buckets(MaterialCount::<NUM_OUTPUT_BUCKETS>)
        .save_format(&[
            SavedFormat::id("l0w")
                .transform(|store, weights| {
                    let factoriser = store.get("l0f").values.f32().repeat(NUM_INPUT_BUCKETS);
                    weights
                        .into_iter()
                        .zip(factoriser)
                        .map(|(a, b)| a + b)
                        .collect()
                })
                .round()
                .quantise::<i16>(QA),
            SavedFormat::id("l0b").round().quantise::<i16>(QA),
            SavedFormat::id("l1w")
                .round()
                .quantise::<i16>(QB)
                .transpose(),
            SavedFormat::id("l1b").round().quantise::<i16>(QA * QB),
        ])
        .loss_fn(|output, target| output.sigmoid().squared_error(target))
        .build(|builder, stm_inputs, ntm_inputs, output_buckets| {
            let l0f = builder.new_weights("l0f", Shape::new(HL, 768), InitSettings::Zeroed);
            let expanded = l0f.repeat(NUM_INPUT_BUCKETS);

            let mut l0 = builder.new_affine("l0", 768 * NUM_INPUT_BUCKETS, HL);
            l0.weights = l0.weights + expanded;

            let l1 = builder.new_affine("l1", 2 * HL, NUM_OUTPUT_BUCKETS);

            let stm_hidden = l0.forward(stm_inputs).screlu();
            let ntm_hidden = l0.forward(ntm_inputs).screlu();
            let hidden = stm_hidden.concat(ntm_hidden);
            l1.forward(hidden).select(output_buckets)
        });

    let net_id = env_string("BITFOX_TRAIN_ID", "bitfox");
    let data_path = env_string("BITFOX_TRAIN_DATA", "data/bitfox.data");
    let output_directory = env_string("BITFOX_TRAIN_OUTPUT", "checkpoints");
    let threads = env_parse("BITFOX_TRAIN_THREADS", 6);
    let superbatches = env_parse("BITFOX_TRAIN_SUPERBATCHES", 8);
    let batches_per_superbatch = env_parse("BITFOX_TRAIN_BATCHES_PER_SUPERBATCH", 1000);
    let start_superbatch = env_parse("BITFOX_TRAIN_START_SUPERBATCH", 1);
    let save_rate = env_parse("BITFOX_TRAIN_SAVE_RATE", 2);
    let wdl_value = env_parse("BITFOX_TRAIN_WDL", 0.4);
    let initial_lr = env_parse("BITFOX_TRAIN_INITIAL_LR", 0.001);
    let final_lr = env_parse("BITFOX_TRAIN_FINAL_LR", initial_lr * 0.3 * 0.3 * 0.3);

    let schedule = TrainingSchedule {
        net_id,
        eval_scale: SCALE as f32,
        steps: TrainingSteps {
            batch_size: 16_384,
            batches_per_superbatch,
            start_superbatch,
            end_superbatch: superbatches,
        },
        wdl_scheduler: wdl::ConstantWDL { value: wdl_value },
        lr_scheduler: lr::CosineDecayLR {
            initial_lr,
            final_lr,
            final_superbatch: superbatches,
        },
        save_rate,
    };

    let settings = LocalSettings {
        threads,
        test_set: None,
        output_directory: output_directory.as_str(),
        batch_queue_size: 32,
    };
    assert!(
        std::path::Path::new(&data_path).exists(),
        "training data not found: {data_path}"
    );
    let data_loader = DirectSequentialDataLoader::new(&[data_path.as_str()]);

    trainer.run(&schedule, &settings, &data_loader);
}
