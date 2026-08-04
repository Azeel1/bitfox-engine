use bullet_lib::{
    game::{
        inputs::{get_num_buckets, ChessBucketsMirrored},
        outputs::MaterialCount,
    },
    nn::{optimiser::{AdamW, AdamWParams}, InitSettings, Shape},
    trainer::{
        save::SavedFormat,
        schedule::{lr, wdl, TrainingSchedule, TrainingSteps},
        settings::LocalSettings,
    },
    value::{loader::DirectSequentialDataLoader, ValueTrainerBuilder},
};
use std::env;

const SCALE: i32 = 400;
const QA: i16 = 255;
const QB: i16 = 64;

const NUM_OUTPUT_BUCKETS: usize = 8;
const L2: usize = 16;
const L3: usize = 32;

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
    let hl = env_parse("BITFOX_TRAIN_HL", 1024);
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
            SavedFormat::id("l1w").transpose().round().quantise::<i8>(QB),
            SavedFormat::id("l1b"),
            SavedFormat::id("l2w").transpose(),
            SavedFormat::id("l2b"),
            SavedFormat::id("l3w").transpose(),
            SavedFormat::id("l3b"),
        ])
        .loss_fn(|output, target| output.sigmoid().squared_error(target))
        .build(|builder, stm_inputs, ntm_inputs, output_buckets| {
            let l0f = builder.new_weights("l0f", Shape::new(hl, 768), InitSettings::Zeroed);
            let expanded = l0f.repeat(NUM_INPUT_BUCKETS);

            let mut l0 = builder.new_affine("l0", 768 * NUM_INPUT_BUCKETS, hl);
            l0.init_with_effective_input_size(32);
            l0.weights = l0.weights + expanded;

            let l1 = builder.new_affine("l1", hl, NUM_OUTPUT_BUCKETS * L2);
            let l2 = builder.new_affine("l2", L2, NUM_OUTPUT_BUCKETS * L3);
            let l3 = builder.new_affine("l3", L3, NUM_OUTPUT_BUCKETS);

            let ft = |input, start, end| l0.slice(start, end).forward(input).crelu();
            let stm_hidden = ft(stm_inputs, 0, hl / 2) * ft(stm_inputs, hl / 2, hl);
            let ntm_hidden = ft(ntm_inputs, 0, hl / 2) * ft(ntm_inputs, hl / 2, hl);

            let hidden = stm_hidden.concat(ntm_hidden);
            let hl2 = l1.forward(hidden).select(output_buckets).screlu();
            let hl3 = l2.forward(hl2).select(output_buckets).screlu();
            l3.forward(hl3).select(output_buckets)
        });

    let clipping = AdamWParams {
        max_weight: 0.99,
        min_weight: -0.99,
        ..Default::default()
    };
    trainer.optimiser.set_params_for_weight("l0w", clipping);
    trainer.optimiser.set_params_for_weight("l0f", clipping);

    let net_id = env_string("BITFOX_TRAIN_ID", "bitfox");
    let data_path = env_string("BITFOX_TRAIN_DATA", "data/bitfox.data");
    let output_directory = env_string("BITFOX_TRAIN_OUTPUT", "checkpoints");
    let resume_checkpoint = env::var("BITFOX_TRAIN_RESUME")
        .ok()
        .filter(|value| !value.is_empty());
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
    let data_paths = env::split_paths(&data_path)
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    for path in &data_paths {
        assert!(
            std::path::Path::new(path).exists(),
            "training data not found: {path}"
        );
    }
    let data_path_refs = data_paths.iter().map(String::as_str).collect::<Vec<_>>();
    let data_loader = DirectSequentialDataLoader::new(&data_path_refs);

    if let Some(path) = resume_checkpoint {
        assert!(
            std::path::Path::new(&path).exists(),
            "resume checkpoint not found: {path}"
        );
        println!("Resuming from checkpoint: {path}");
        trainer.load_from_checkpoint(&path);
    }

    trainer.run(&schedule, &settings, &data_loader);
}
