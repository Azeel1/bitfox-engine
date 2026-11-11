use crate::board::Board;
use crate::search::{Limits, Search};
use crate::types::Color;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static TERMINATION_SIGNAL: AtomicI32 = AtomicI32::new(0);
const COMPLETION_COMMITTED: i32 = -1;
const DEFAULT_TOTAL_HASH_MB: usize = 256;
const MAX_TOTAL_HASH_MB: usize = 4096;
const MAX_WORKER_HASH_MB: usize = 16;

#[cfg(any(unix, test))]
extern "C" fn request_termination(signum: i32) {
    let _ = TERMINATION_SIGNAL.compare_exchange(0, signum, Ordering::SeqCst, Ordering::SeqCst);
}

#[cfg(unix)]
fn install_signal_handler(signum: i32) -> io::Result<()> {
    let previous = unsafe {
        libc::signal(
            signum,
            request_termination as *const () as libc::sighandler_t,
        )
    };
    if previous == libc::SIG_ERR {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
fn install_signal_handlers() -> io::Result<()> {
    TERMINATION_SIGNAL.store(0, Ordering::Relaxed);
    for signum in [libc::SIGINT, libc::SIGTERM] {
        install_signal_handler(signum).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("install handler for signal {signum}: {error}"),
            )
        })?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn install_signal_handlers() -> io::Result<()> {
    TERMINATION_SIGNAL.store(0, Ordering::Relaxed);
    Ok(())
}

fn termination_signal() -> Option<i32> {
    let signum = TERMINATION_SIGNAL.load(Ordering::Relaxed);
    (signum > 0).then_some(signum)
}

fn commit_completion() -> Result<(), i32> {
    match TERMINATION_SIGNAL.compare_exchange(
        0,
        COMPLETION_COMMITTED,
        Ordering::SeqCst,
        Ordering::SeqCst,
    ) {
        Ok(_) => Ok(()),
        Err(signum) => Err(signum),
    }
}

fn check_termination() -> io::Result<()> {
    match termination_signal() {
        Some(signum) => Err(io::Error::new(
            io::ErrorKind::Interrupted,
            format!("datagen terminated by signal {signum}"),
        )),
        None => Ok(()),
    }
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

struct PublishedOutput(Option<PathBuf>);

impl PublishedOutput {
    fn new(path: &Path) -> Self {
        Self(Some(path.to_path_buf()))
    }

    fn disarm(&mut self) {
        self.0 = None;
    }

    fn rollback(&mut self) -> io::Result<()> {
        let Some(path) = self.0.take() else {
            return Ok(());
        };
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                self.0 = Some(path);
                return Err(error);
            }
        }
        sync_parent_directory(&path)
    }
}

impl Drop for PublishedOutput {
    fn drop(&mut self) {
        let _ = self.rollback();
    }
}

struct AbortOnFailure {
    aborted: Arc<AtomicBool>,
    armed: bool,
}

impl AbortOnFailure {
    fn new(aborted: Arc<AtomicBool>) -> Self {
        Self {
            aborted,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for AbortOnFailure {
    fn drop(&mut self) {
        if self.armed {
            self.aborted.store(true, Ordering::Release);
        }
    }
}

struct WorkersDone(Arc<AtomicBool>);

impl Drop for WorkersDone {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[derive(Default)]
struct PartialOutputs {
    paths: Vec<PathBuf>,
}

impl PartialOutputs {
    fn track(&mut self, path: PathBuf) {
        self.paths.push(path);
    }

    fn publish_completed(&mut self, partial_path: &Path, final_path: &Path) -> io::Result<()> {
        std::fs::hard_link(partial_path, final_path).map_err(|error| {
            self.paths.retain(|path| path != partial_path);
            io::Error::new(
                error.kind(),
                format!(
                    "publish {}: {error}; completed data preserved at {}",
                    final_path.display(),
                    partial_path.display()
                ),
            )
        })
    }

    fn cleanup(&mut self) -> io::Result<()> {
        let mut failed = Vec::new();
        let mut first_error = None;
        for path in std::mem::take(&mut self.paths).into_iter().rev() {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(io::Error::new(
                            error.kind(),
                            format!("remove {}: {error}", path.display()),
                        ));
                    }
                    failed.push(path);
                }
            }
        }
        self.paths = failed;
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for PartialOutputs {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn create_partial_output(final_path: &Path) -> io::Result<(PathBuf, File)> {
    let final_name = final_path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "datagen output must name a file",
        )
    })?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);

    for attempt in 0..256u16 {
        let mut name = final_name.to_os_string();
        name.push(format!(
            ".partial.{:08x}.{timestamp:032x}.{sequence:016x}.{attempt:04x}",
            std::process::id()
        ));
        let path = final_path.with_file_name(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve a unique partial datagen output",
    ))
}

fn worker_output_path(partial_path: &Path, worker: usize) -> io::Result<PathBuf> {
    let mut name = partial_path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid partial output"))?
        .to_os_string();
    name.push(format!(".worker.{worker}"));
    Ok(partial_path.with_file_name(name))
}

fn openings_and_workers(args: &[String]) -> (Option<&str>, usize) {
    let openings = args
        .get(3)
        .map(String::as_str)
        .filter(|value| *value != "-");
    let workers = args
        .get(4)
        .map(|value| {
            value
                .parse::<usize>()
                .expect("datagen threads must be an integer")
        })
        .unwrap_or(1);
    (openings, workers)
}

fn worker_hash_mb(workers: usize, total_hash_mb: usize) -> usize {
    assert!(workers > 0, "datagen must have at least one worker");
    assert!(
        total_hash_mb >= workers,
        "datagen total hash must provide at least 1 MB per worker"
    );
    (total_hash_mb / workers).min(MAX_WORKER_HASH_MB)
}

fn total_hash_budget_mb(args: &[String]) -> usize {
    let total = args
        .get(6)
        .map(|value| {
            value
                .parse::<usize>()
                .expect("datagen total hash must be an integer")
        })
        .unwrap_or(DEFAULT_TOTAL_HASH_MB);
    assert!(
        total <= MAX_TOTAL_HASH_MB,
        "datagen total hash must not exceed {MAX_TOTAL_HASH_MB} MB"
    );
    total
}

struct Rng(u64);

impl Rng {
    fn for_worker(seed: u64, worker: usize) -> Self {
        let salt = 0x9e37_79b9_7f4a_7c15u64.wrapping_mul((worker as u64).wrapping_add(1));
        let mut state = seed ^ salt;
        state = (state ^ (state >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        state = (state ^ (state >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        state ^= state >> 31;
        Self(if state == 0 {
            0x9e37_79b9_7f4a_7c15
        } else {
            state
        })
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const ADJUDICATE_SCORE: i32 = 2000;
const ADJUDICATE_PLIES: i32 = 5;

fn generate_game(
    rng: &mut Rng,
    search: &mut Search,
    openings: &[String],
    nodes: u64,
    aborted: &AtomicBool,
) -> Vec<(String, i32, f64)> {
    if aborted.load(Ordering::Acquire) {
        return Vec::new();
    }
    let mut board = if openings.is_empty() {
        Board::startpos()
    } else {
        Board::from_fen(&openings[rng.below(openings.len())]).expect("validated opening")
    };
    let opening_plies = if openings.is_empty() {
        6 + rng.below(5)
    } else {
        rng.below(3)
    };
    for _ in 0..opening_plies {
        let legal = board.generate_legal();
        if legal.is_empty() {
            return Vec::new();
        }
        board.make_move(legal.moves[rng.below(legal.len())]);
    }

    search.clear();
    let mut samples: Vec<(String, i32, Color)> = Vec::new();
    let mut decisive = 0i32;
    let result;

    loop {
        if aborted.load(Ordering::Acquire) || termination_signal().is_some() {
            return Vec::new();
        }
        let legal = board.generate_legal();
        if legal.is_empty() {
            result = if board.in_check(board.side()) {
                if board.side() == Color::White {
                    0.0
                } else {
                    1.0
                }
            } else {
                0.5
            };
            break;
        }
        if board.is_draw() {
            result = 0.5;
            break;
        }
        if samples.len() >= 400 {
            result = 0.5;
            break;
        }

        let limits = Limits {
            nodes: Some(nodes),
            ..Default::default()
        };
        if aborted.load(Ordering::Acquire) {
            return Vec::new();
        }
        search.reset_stop();
        if aborted.load(Ordering::Acquire) || termination_signal().is_some() {
            return Vec::new();
        }
        let best = search.think_started(&mut board, &limits, false);
        if aborted.load(Ordering::Acquire) {
            return Vec::new();
        }
        if best.is_none() {
            result = 0.5;
            break;
        }
        let score = search.best_score();

        if !board.in_check(board.side()) && best.is_quiet() && score.abs() < 20_000 {
            samples.push((board.to_fen(), score, board.side()));
        }

        let white_score = if board.side() == Color::White {
            score
        } else {
            -score
        };
        if white_score.abs() >= ADJUDICATE_SCORE {
            if decisive != 0 && decisive.signum() != white_score.signum() {
                decisive = white_score.signum();
            } else {
                decisive += white_score.signum();
            }
            if decisive.abs() >= ADJUDICATE_PLIES {
                result = if decisive > 0 { 1.0 } else { 0.0 };
                break;
            }
        } else {
            decisive = 0;
        }

        board.make_move(best);
    }

    samples
        .into_iter()
        .map(|(fen, score, side)| {
            let white_score = if side == Color::White { score } else { -score };
            (fen, white_score, result)
        })
        .collect()
}

fn write_datagen(
    final_path: &Path,
    target: usize,
    nodes: u64,
    workers: usize,
    worker_hash_mb: usize,
    seed: u64,
    openings: Vec<String>,
) -> io::Result<(usize, usize)> {
    let (partial_path, partial_file) = create_partial_output(final_path)?;
    let mut partials = PartialOutputs::default();
    partials.track(partial_path.clone());

    let mut worker_paths = Vec::with_capacity(workers);
    let mut worker_outputs = Vec::with_capacity(workers);
    for worker in 0..workers {
        let worker_path = worker_output_path(&partial_path, worker)?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&worker_path)?;
        partials.track(worker_path.clone());
        worker_paths.push(worker_path);
        worker_outputs.push((worker, file));
    }

    let mut workers_with_search = Vec::with_capacity(workers);
    let mut search_stops = Vec::with_capacity(workers);
    for (worker, file) in worker_outputs {
        let search = Search::new(worker_hash_mb);
        check_termination()?;
        search_stops.push(search.stop_signal());
        workers_with_search.push((worker, file, search));
    }
    check_termination()?;

    let openings = Arc::new(openings);
    let written = Arc::new(AtomicUsize::new(0));
    let games = Arc::new(AtomicUsize::new(0));
    let aborted = Arc::new(AtomicBool::new(false));
    let workers_done = Arc::new(AtomicBool::new(false));

    std::thread::scope(|scope| -> io::Result<()> {
        let done_guard = WorkersDone(Arc::clone(&workers_done));
        let monitor_done = Arc::clone(&workers_done);
        let monitor_aborted = Arc::clone(&aborted);
        let monitor = scope.spawn(move || {
            while !monitor_done.load(Ordering::Acquire) {
                if termination_signal().is_some() || monitor_aborted.load(Ordering::Acquire) {
                    for stop in &search_stops {
                        stop.store(true, Ordering::Relaxed);
                    }
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });

        let mut handles = Vec::with_capacity(workers);
        for (worker, file, mut search) in workers_with_search {
            let openings = Arc::clone(&openings);
            let written = Arc::clone(&written);
            let games = Arc::clone(&games);
            let aborted = Arc::clone(&aborted);
            handles.push(scope.spawn(move || -> io::Result<()> {
                let mut abort_on_failure = AbortOnFailure::new(Arc::clone(&aborted));
                let mut rng = Rng::for_worker(seed, worker);
                let mut out = BufWriter::new(file);
                let quota = target / workers + usize::from(worker < target % workers);
                let mut local_written = 0usize;
                let mut empty_games = 0usize;

                while local_written < quota {
                    if aborted.load(Ordering::Acquire) {
                        return Err(io::Error::new(
                            io::ErrorKind::Interrupted,
                            "datagen stopped after another worker failed",
                        ));
                    }
                    let rows = generate_game(&mut rng, &mut search, &openings, nodes, &aborted);
                    if aborted.load(Ordering::Acquire) {
                        return Err(io::Error::new(
                            io::ErrorKind::Interrupted,
                            "datagen stopped after another worker failed",
                        ));
                    }
                    check_termination()?;
                    if rows.is_empty() {
                        empty_games += 1;
                        if empty_games >= 10_000 {
                            return Err(io::Error::other(
                                "datagen produced no positions in 10000 consecutive games",
                            ));
                        }
                        continue;
                    }
                    empty_games = 0;

                    let count = rows.len().min(quota - local_written);
                    for (fen, score, result) in rows.into_iter().take(count) {
                        writeln!(out, "{fen} | {score} | {result:.1}")?;
                    }
                    local_written += count;
                    let total = written.fetch_add(count, Ordering::Relaxed) + count;

                    let game_no = games.fetch_add(1, Ordering::Relaxed) + 1;
                    if game_no.is_multiple_of(50) {
                        out.flush()?;
                        eprintln!("datagen: {total}/{target} positions, {game_no} games");
                    }
                }
                out.flush()?;
                out.get_ref().sync_all()?;
                abort_on_failure.disarm();
                Ok(())
            }));
        }

        let mut first_error = None;
        for handle in handles {
            match handle.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    let replace = first_error.as_ref().is_none_or(|current: &io::Error| {
                        current.kind() == io::ErrorKind::Interrupted
                            && error.kind() != io::ErrorKind::Interrupted
                    });
                    if replace {
                        first_error = Some(error);
                    }
                }
                Err(_) => {
                    first_error = Some(io::Error::other("datagen worker panicked"));
                }
            }
        }
        drop(done_guard);
        let _ = monitor.join();
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    })?;

    check_termination()?;
    let mut out = BufWriter::new(partial_file);
    for worker_path in &worker_paths {
        let mut input = File::open(worker_path)?;
        std::io::copy(&mut input, &mut out)?;
        check_termination()?;
    }
    out.flush()?;
    out.get_ref().sync_all()?;
    check_termination()?;
    drop(out);
    partials.publish_completed(&partial_path, final_path)?;
    let mut published = PublishedOutput::new(final_path);
    sync_parent_directory(final_path)?;
    let cleanup = partials.cleanup();
    let directory_sync = sync_parent_directory(final_path);
    cleanup?;
    directory_sync?;
    if let Err(signum) = commit_completion() {
        published.rollback().map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("roll back interrupted datagen output: {error}"),
            )
        })?;
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            format!("datagen terminated by signal {signum}"),
        ));
    }
    published.disarm();

    Ok((
        written.load(Ordering::Relaxed),
        games.load(Ordering::Relaxed),
    ))
}

pub fn datagen(args: &[String]) {
    assert!(
        args.len() <= 7,
        "usage: datagen positions nodes output [openings|-] [threads] [seed] [total-hash-mb]"
    );
    let target: usize = args
        .first()
        .map(|value| value.parse().expect("datagen positions must be an integer"))
        .unwrap_or(100_000);
    let nodes: u64 = args
        .get(1)
        .map(|value| value.parse().expect("datagen nodes must be an integer"))
        .unwrap_or(5_000);
    assert!(target > 0, "datagen positions must be positive");
    assert!(nodes > 0, "datagen nodes must be positive");
    let path = args.get(2).map(String::as_str).unwrap_or("data.txt");
    let (openings_path, workers) = openings_and_workers(args);
    assert!(
        (1..=256).contains(&workers),
        "datagen threads must be from 1 to 256"
    );
    let openings = match openings_path {
        Some(openings_path) => {
            let text = std::fs::read_to_string(openings_path)
                .unwrap_or_else(|error| panic!("read openings {openings_path}: {error}"));
            let openings = text
                .lines()
                .enumerate()
                .filter_map(|(index, line)| {
                    let line = line.trim();
                    if line.is_empty() {
                        return None;
                    }
                    let mut board = Board::from_fen(line).unwrap_or_else(|| {
                        panic!("invalid opening at {openings_path}:{}", index + 1)
                    });
                    assert!(
                        !board.generate_legal().is_empty() && !board.is_draw(),
                        "terminal opening at {openings_path}:{}",
                        index + 1
                    );
                    Some(line.to_string())
                })
                .collect::<Vec<_>>();
            assert!(
                !openings.is_empty(),
                "opening file contains no positions: {openings_path}"
            );
            openings
        }
        None => Vec::new(),
    };

    let seed = match args.get(5) {
        Some(value) => {
            let value = value
                .parse::<u64>()
                .expect("datagen seed must be an integer");
            assert!(value != 0, "datagen seed must be non-zero");
            value
        }
        None => {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64
                | 1
        }
    };
    let final_path = Path::new(path);
    assert!(
        !final_path.exists(),
        "datagen output already exists: {path}"
    );
    let workers = workers.min(target);
    let total_hash_mb = total_hash_budget_mb(args);
    let worker_hash_mb = worker_hash_mb(workers, total_hash_mb);
    install_signal_handlers()
        .unwrap_or_else(|error| panic!("datagen could not install signal handlers: {error}"));
    let (written, games) = match write_datagen(
        final_path,
        target,
        nodes,
        workers,
        worker_hash_mb,
        seed,
        openings,
    ) {
        Ok(result) => result,
        Err(error) if error.kind() == io::ErrorKind::Interrupted => {
            let signum = termination_signal().unwrap_or(1);
            eprintln!("{error}");
            std::process::exit(128 + signum);
        }
        Err(error) => panic!("datagen failed: {error}"),
    };
    eprintln!(
        "datagen done: {written} positions in {games} games, seed {seed}, {workers} threads, \
         {worker_hash_mb} MB hash/thread ({total_hash_mb} MB budget) -> {path}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_aborted_game_does_not_clear_stop_or_start_searching() {
        let aborted = AtomicBool::new(true);
        let mut rng = Rng::for_worker(1, 0);
        let initial_rng = rng.0;
        let mut search = Search::new(1);
        let stop = search.stop_signal();
        stop.store(true, Ordering::Relaxed);

        let rows = generate_game(&mut rng, &mut search, &[], 1, &aborted);

        assert!(rows.is_empty());
        assert_eq!(search.node_count(), 0);
        assert!(stop.load(Ordering::Relaxed));
        assert_eq!(rng.0, initial_rng);
    }

    #[test]
    fn worker_streams_distinguish_adjacent_and_boundary_seeds() {
        let mut states = std::collections::HashSet::new();
        let mut prefixes = std::collections::HashSet::new();
        for (seed, workers) in [
            (20_260_904, 30),
            (20_260_905, 76),
            (0, 76),
            (1, 76),
            (2, 76),
            (3, 76),
            (1 << 63, 76),
            ((1 << 63) + 1, 76),
            (u64::MAX - 1, 76),
            (u64::MAX, 76),
        ] {
            for worker in 0..workers {
                let mut rng = Rng::for_worker(seed, worker);
                assert_ne!(rng.0, 0, "seed {seed}, worker {worker}");
                assert!(states.insert(rng.0), "seed {seed}, worker {worker}");
                let prefix: [u64; 8] = std::array::from_fn(|_| rng.next());
                assert!(prefixes.insert(prefix), "seed {seed}, worker {worker}");
            }
        }
    }

    #[test]
    fn worker_streams_use_every_seed_bit() {
        for worker in 0..76 {
            let mut reference = Rng::for_worker(20_260_904, worker);
            let reference: [u64; 8] = std::array::from_fn(|_| reference.next());
            for bit in 0..64 {
                let mut rng = Rng::for_worker(20_260_904 ^ (1u64 << bit), worker);
                let prefix: [u64; 8] = std::array::from_fn(|_| rng.next());
                assert_ne!(prefix, reference, "worker {worker}, bit {bit}");
            }
        }
    }

    #[test]
    fn worker_streams_restart_deterministically_and_escape_zero() {
        for seed in [0, 1, 2, 0x9e37_79b9_7f4a_7c15, u64::MAX - 1, u64::MAX] {
            for worker in [0, 1, 29, 30, 75, usize::MAX] {
                let mut first = Rng::for_worker(seed, worker);
                let mut second = Rng::for_worker(seed, worker);
                assert_ne!(first.0, 0, "seed {seed}, worker {worker}");
                for _ in 0..16 {
                    assert_eq!(first.next(), second.next());
                    assert_ne!(first.0, 0, "seed {seed}, worker {worker}");
                    assert_eq!(first.below(37), second.below(37));
                }
            }
        }
    }

    fn test_output(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "bitfox-datagen-{name}-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn partial_output_names_are_unique() {
        let final_path = test_output("unique");
        let mut partials = PartialOutputs::default();
        let (first, first_file) = create_partial_output(&final_path).unwrap();
        partials.track(first.clone());
        let (second, second_file) = create_partial_output(&final_path).unwrap();
        partials.track(second.clone());
        drop((first_file, second_file));

        assert_ne!(first, second);
        assert!(first.exists());
        assert!(second.exists());
        partials.cleanup().unwrap();
        assert!(!first.exists());
        assert!(!second.exists());
    }

    #[test]
    fn a_numeric_opening_path_is_not_treated_as_threads() {
        let args = ["1", "1", "out", "123", "4"].map(str::to_string);
        assert_eq!(openings_and_workers(&args), (Some("123"), 4));

        let args = ["1", "1", "out", "123"].map(str::to_string);
        assert_eq!(openings_and_workers(&args), (Some("123"), 1));
    }

    #[test]
    fn total_hash_budget_is_split_and_capped_per_worker() {
        assert_eq!(worker_hash_mb(1, 256), 16);
        assert_eq!(worker_hash_mb(16, 256), 16);
        assert_eq!(worker_hash_mb(32, 256), 8);
        assert_eq!(worker_hash_mb(256, 256), 1);
        assert_eq!(worker_hash_mb(4, 20), 5);
    }

    #[test]
    fn total_hash_budget_is_configurable_from_the_cli() {
        assert_eq!(total_hash_budget_mb(&[]), DEFAULT_TOTAL_HASH_MB);
        let args = ["1", "1", "out", "-", "4", "1", "128"].map(str::to_string);
        assert_eq!(total_hash_budget_mb(&args), 128);
    }

    #[test]
    #[should_panic(expected = "at least 1 MB per worker")]
    fn total_hash_budget_must_cover_every_worker() {
        worker_hash_mb(17, 16);
    }

    #[test]
    fn failed_run_removes_worker_and_merged_partials() {
        let final_path = test_output("failure");
        let partial_path;
        let worker_path;
        {
            let mut partials = PartialOutputs::default();
            let (path, file) = create_partial_output(&final_path).unwrap();
            partial_path = path;
            partials.track(partial_path.clone());
            worker_path = worker_output_path(&partial_path, 0).unwrap();
            let worker_file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&worker_path)
                .unwrap();
            partials.track(worker_path.clone());
            drop((file, worker_file));
        }

        assert!(!partial_path.exists());
        assert!(!worker_path.exists());
    }

    #[test]
    fn publication_failure_preserves_completed_data_without_replacing_output() {
        let final_path = test_output("existing-publication");
        std::fs::write(&final_path, b"existing output").unwrap();
        let partial_path;
        let worker_path;
        {
            let mut partials = PartialOutputs::default();
            let (path, mut file) = create_partial_output(&final_path).unwrap();
            partial_path = path;
            partials.track(partial_path.clone());
            file.write_all(b"completed records").unwrap();
            file.sync_all().unwrap();
            drop(file);
            worker_path = worker_output_path(&partial_path, 0).unwrap();
            std::fs::write(&worker_path, b"worker records").unwrap();
            partials.track(worker_path.clone());

            let error = partials
                .publish_completed(&partial_path, &final_path)
                .unwrap_err();
            assert!(error
                .to_string()
                .contains(&partial_path.display().to_string()));
            assert!(error.to_string().contains("completed data preserved"));
        }

        assert_eq!(std::fs::read(&final_path).unwrap(), b"existing output");
        assert_eq!(std::fs::read(&partial_path).unwrap(), b"completed records");
        assert!(!worker_path.exists());
        std::fs::remove_file(final_path).unwrap();
        std::fs::remove_file(partial_path).unwrap();
    }

    #[test]
    fn publication_io_failure_preserves_the_completed_partial() {
        let final_path = test_output("missing-publication-directory").join("output.txt");
        let partial_path;
        {
            let mut partials = PartialOutputs::default();
            let (path, mut file) = create_partial_output(&test_output("completed")).unwrap();
            partial_path = path;
            partials.track(partial_path.clone());
            file.write_all(b"completed records").unwrap();
            file.sync_all().unwrap();
            drop(file);
            let error = partials
                .publish_completed(&partial_path, &final_path)
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::NotFound);
        }
        assert_eq!(std::fs::read(&partial_path).unwrap(), b"completed records");
        assert!(!final_path.exists());
        std::fs::remove_file(partial_path).unwrap();
    }

    #[test]
    fn successful_publication_remains_tracked_for_cleanup() {
        let final_path = test_output("successful-publication");
        let partial_path;
        {
            let mut partials = PartialOutputs::default();
            let (path, mut file) = create_partial_output(&final_path).unwrap();
            partial_path = path;
            partials.track(partial_path.clone());
            file.write_all(b"completed records").unwrap();
            file.sync_all().unwrap();
            drop(file);
            partials
                .publish_completed(&partial_path, &final_path)
                .unwrap();
            assert!(partial_path.exists());
        }
        assert!(!partial_path.exists());
        assert_eq!(std::fs::read(&final_path).unwrap(), b"completed records");
        std::fs::remove_file(final_path).unwrap();
    }

    #[test]
    fn unwinding_removes_tracked_partials() {
        let final_path = test_output("panic");
        let mut partial_path = None;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut partials = PartialOutputs::default();
            let (path, file) = create_partial_output(&final_path).unwrap();
            partial_path = Some(path.clone());
            partials.track(path);
            drop(file);
            panic!("worker failed");
        }));

        assert!(result.is_err());
        assert!(!partial_path.unwrap().exists());
    }

    #[test]
    fn cleanup_continues_after_a_missing_partial() {
        let final_path = test_output("missing");
        let mut partials = PartialOutputs::default();
        let (remaining, remaining_file) = create_partial_output(&final_path).unwrap();
        partials.track(remaining.clone());
        let (missing, missing_file) = create_partial_output(&final_path).unwrap();
        partials.track(missing.clone());
        drop((remaining_file, missing_file));
        std::fs::remove_file(&missing).unwrap();

        partials.cleanup().unwrap();
        assert!(!remaining.exists());
        assert!(!missing.exists());
    }

    #[test]
    fn worker_abort_guard_only_fires_on_failure() {
        let aborted = Arc::new(AtomicBool::new(false));
        {
            let _guard = AbortOnFailure::new(Arc::clone(&aborted));
        }
        assert!(aborted.load(Ordering::Acquire));

        aborted.store(false, Ordering::Release);
        {
            let mut guard = AbortOnFailure::new(Arc::clone(&aborted));
            guard.disarm();
        }
        assert!(!aborted.load(Ordering::Acquire));
    }

    #[test]
    fn termination_state_has_one_completion_boundary() {
        TERMINATION_SIGNAL.store(0, Ordering::Relaxed);
        request_termination(15);
        assert_eq!(termination_signal(), Some(15));

        let rolled_back = test_output("signaled-publication");
        File::create(&rolled_back).unwrap();
        {
            let _published = PublishedOutput::new(&rolled_back);
            assert_eq!(commit_completion(), Err(15));
        }
        assert!(!rolled_back.exists());

        TERMINATION_SIGNAL.store(0, Ordering::Relaxed);
        let committed = test_output("committed-publication");
        File::create(&committed).unwrap();
        let mut published = PublishedOutput::new(&committed);
        assert_eq!(commit_completion(), Ok(()));
        published.disarm();
        request_termination(15);
        assert_eq!(termination_signal(), None);
        assert!(committed.exists());
        std::fs::remove_file(committed).unwrap();
        TERMINATION_SIGNAL.store(0, Ordering::Relaxed);
    }

    #[cfg(unix)]
    #[test]
    fn invalid_signal_registration_is_reported() {
        assert!(install_signal_handler(libc::SIGKILL).is_err());
    }
}
