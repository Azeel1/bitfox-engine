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

#[path = "../../engine/src/eval/contract.rs"]
mod nnue_contract;
use nnue_contract::{
    BUCKET_LAYOUT, FEATURES, HL, INPUT_BUCKETS, NETWORK_BYTES, OUTPUT_BUCKETS, PARAMETER_BYTES, QA,
    QB, SCALE,
};

const NUM_INPUT_BUCKETS: usize = get_num_buckets(&BUCKET_LAYOUT);
const _: () = assert!(NUM_INPUT_BUCKETS == INPUT_BUCKETS);
const _: () = assert!(FEATURES == 768 * NUM_INPUT_BUCKETS);
const _: () = assert!(NETWORK_BYTES >= PARAMETER_BYTES && NETWORK_BYTES - PARAMETER_BYTES < 64);

fn env_string(key: &str, default: &str) -> String {
    match env::var(key) {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => default.to_string(),
        Err(error) => panic!("read {key}: {error}"),
    }
}

fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    match env::var(key) {
        Ok(value) => value
            .parse()
            .unwrap_or_else(|_| panic!("invalid {key}: {value}")),
        Err(env::VarError::NotPresent) => default,
        Err(error) => panic!("read {key}: {error}"),
    }
}

fn checkpoint_superbatch(path: &std::path::Path, net_id: &str) -> Option<usize> {
    let name = path.file_name()?.to_str()?;
    name.strip_prefix(net_id)?.strip_prefix('-')?.parse().ok()
}

fn main() {
    let hl = env_parse("BITFOX_TRAIN_HL", HL);
    let train_seed = env_parse("BITFOX_TRAIN_SEED", 198_273_612u64);
    assert_eq!(
        hl, HL,
        "BITFOX_TRAIN_HL must match the current engine architecture"
    );
    let mut trainer = ValueTrainerBuilder::default()
        .dual_perspective()
        .optimiser(AdamW)
        .seed(train_seed)
        .inputs(ChessBucketsMirrored::new(BUCKET_LAYOUT))
        .output_buckets(MaterialCount::<OUTPUT_BUCKETS>)
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
                .quantise::<i16>(QA as i16),
            SavedFormat::id("l0b").round().quantise::<i16>(QA as i16),
            SavedFormat::id("l1w")
                .round()
                .quantise::<i16>(QB as i16)
                .transpose(),
            SavedFormat::id("l1b")
                .round()
                .quantise::<i16>((QA * QB) as i16),
        ])
        .loss_fn(|output, target| output.sigmoid().squared_error(target))
        .build(|builder, stm_inputs, ntm_inputs, output_buckets| {
            let l0f = builder.new_weights("l0f", Shape::new(hl, 768), InitSettings::Zeroed);
            let expanded = l0f.repeat(NUM_INPUT_BUCKETS);

            let mut l0 = builder.new_affine("l0", 768 * NUM_INPUT_BUCKETS, hl);
            l0.weights = l0.weights + expanded;

            let l1 = builder.new_affine("l1", 2 * hl, OUTPUT_BUCKETS);

            let stm_hidden = l0.forward(stm_inputs).screlu();
            let ntm_hidden = l0.forward(ntm_inputs).screlu();
            let hidden = stm_hidden.concat(ntm_hidden);
            l1.forward(hidden).select(output_buckets)
        });

    let net_id = env_string("BITFOX_TRAIN_ID", "bitfox");
    let data_path = env_string("BITFOX_TRAIN_DATA", "data/bitfox.data");
    let output_directory = env_string("BITFOX_TRAIN_OUTPUT", "checkpoints");
    assert!(
        !net_id.is_empty()
            && net_id != "."
            && net_id != ".."
            && std::path::Path::new(&net_id).file_name() == Some(std::ffi::OsStr::new(&net_id)),
        "BITFOX_TRAIN_ID must be one path component"
    );
    assert!(
        !output_directory.is_empty(),
        "BITFOX_TRAIN_OUTPUT must not be empty"
    );
    let resume_checkpoint = match env::var("BITFOX_TRAIN_RESUME") {
        Ok(value) if !value.is_empty() => Some(value),
        Ok(_) | Err(env::VarError::NotPresent) => None,
        Err(error) => panic!("read BITFOX_TRAIN_RESUME: {error}"),
    };
    let threads = env_parse("BITFOX_TRAIN_THREADS", 6);
    let superbatches = env_parse("BITFOX_TRAIN_SUPERBATCHES", 8);
    let batches_per_superbatch = env_parse("BITFOX_TRAIN_BATCHES_PER_SUPERBATCH", 1000);
    let start_superbatch = env_parse("BITFOX_TRAIN_START_SUPERBATCH", 1);
    let save_rate = env_parse("BITFOX_TRAIN_SAVE_RATE", 2);
    let wdl_value: f32 = env_parse("BITFOX_TRAIN_WDL", 0.4);
    let initial_lr: f32 = env_parse("BITFOX_TRAIN_INITIAL_LR", 0.001);
    let final_lr: f32 = env_parse("BITFOX_TRAIN_FINAL_LR", initial_lr * 0.3 * 0.3 * 0.3);
    assert!(
        (1..=255).contains(&threads),
        "BITFOX_TRAIN_THREADS must be from 1 to 255"
    );
    assert!(
        superbatches > 0,
        "BITFOX_TRAIN_SUPERBATCHES must be positive"
    );
    assert!(
        batches_per_superbatch > 0,
        "BITFOX_TRAIN_BATCHES_PER_SUPERBATCH must be positive"
    );
    assert!(
        start_superbatch > 0 && start_superbatch <= superbatches,
        "BITFOX_TRAIN_START_SUPERBATCH must be within the schedule"
    );
    assert!(
        (start_superbatch == 1) == resume_checkpoint.is_none(),
        "BITFOX_TRAIN_RESUME is required exactly when starting mid-schedule"
    );
    assert!(save_rate > 0, "BITFOX_TRAIN_SAVE_RATE must be positive");
    assert!(
        wdl_value.is_finite() && (0.0..=1.0).contains(&wdl_value),
        "BITFOX_TRAIN_WDL must be between zero and one"
    );
    assert!(
        initial_lr.is_finite() && initial_lr > 0.0,
        "BITFOX_TRAIN_INITIAL_LR must be positive"
    );
    assert!(
        final_lr.is_finite() && final_lr > 0.0,
        "BITFOX_TRAIN_FINAL_LR must be positive"
    );
    for superbatch in start_superbatch..=superbatches {
        if superbatch % save_rate == 0 || superbatch == superbatches {
            let checkpoint =
                std::path::Path::new(&output_directory).join(format!("{net_id}-{superbatch}"));
            assert!(
                !checkpoint.exists(),
                "training checkpoint already exists: {}",
                checkpoint.display()
            );
        }
    }

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
            std::path::Path::new(path).is_file(),
            "training data not found: {path}"
        );
    }
    let data_path_refs = data_paths.iter().map(String::as_str).collect::<Vec<_>>();
    let data_loader = DirectSequentialDataLoader::new(&data_path_refs);

    if let Some(path) = resume_checkpoint {
        let checkpoint_path = std::path::Path::new(&path);
        assert!(
            checkpoint_path.is_dir(),
            "resume checkpoint not found: {}",
            checkpoint_path.display()
        );
        assert_eq!(
            checkpoint_superbatch(checkpoint_path, &schedule.net_id),
            Some(start_superbatch - 1),
            "resume checkpoint must be {}-{}",
            schedule.net_id,
            start_superbatch - 1
        );
        println!("Resuming from checkpoint: {}", checkpoint_path.display());
        trainer.load_from_checkpoint(&path);
    }

    trainer.run(&schedule, &settings, &data_loader);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_number_must_match_the_run_name() {
        assert_eq!(
            checkpoint_superbatch(std::path::Path::new("runs/gen3-6"), "gen3"),
            Some(6)
        );
        assert_eq!(
            checkpoint_superbatch(std::path::Path::new("runs/other-6"), "gen3"),
            None
        );
        assert_eq!(
            checkpoint_superbatch(std::path::Path::new("runs/gen3-final"), "gen3"),
            None
        );
    }
}
