use bulletformat::{BulletFormat, ChessBoard};
use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TERMINATION_SIGNAL: AtomicI32 = AtomicI32::new(0);
const COMPLETION_COMMITTED: i32 = -1;

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

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

struct PartialOutput(PathBuf);

impl Drop for PartialOutput {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
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
            Err(error) if error.kind() == ErrorKind::NotFound => {}
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

fn create_partial(final_path: &Path) -> (PathBuf, File) {
    let name = final_path
        .file_name()
        .expect("conversion output must name a file")
        .to_string_lossy();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for attempt in 0..1024 {
        let path = final_path.with_file_name(format!(
            "{name}.partial.{}.{nonce:x}.{attempt}",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return (path, file),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("create partial conversion output: {error}"),
        }
    }
    panic!("could not allocate a partial conversion output")
}

fn convert(input_path: &Path, final_path: &Path) -> Result<usize, i32> {
    assert!(input_path.is_file(), "conversion input does not exist");
    assert!(
        !final_path.exists(),
        "conversion output already exists: {}",
        final_path.display()
    );
    let input = BufReader::new(File::open(input_path).expect("open conversion input"));
    let (partial_path, output) = create_partial(final_path);
    let partial = PartialOutput(partial_path.clone());
    let mut output = BufWriter::new(output);
    let mut buffer = Vec::with_capacity(16_384);
    let mut converted = 0usize;

    for (index, line) in input.lines().enumerate() {
        if let Some(signum) = termination_signal() {
            return Err(signum);
        }
        let line = line.expect("read conversion input");
        let position = line.parse::<ChessBoard>().unwrap_or_else(|error| {
            panic!("invalid training record at line {}: {error}", index + 1)
        });
        buffer.push(position);
        if buffer.len() == 16_384 {
            ChessBoard::write_to_bin(&mut output, &buffer).expect("write converted records");
            converted += buffer.len();
            buffer.clear();
        }
    }
    if let Some(signum) = termination_signal() {
        return Err(signum);
    }
    ChessBoard::write_to_bin(&mut output, &buffer).expect("write converted records");
    converted += buffer.len();
    assert!(converted > 0, "conversion input contains no records");
    output.flush().expect("flush conversion output");
    output.get_ref().sync_all().expect("sync conversion output");
    if let Some(signum) = termination_signal() {
        return Err(signum);
    }
    drop(output);
    std::fs::hard_link(&partial_path, final_path).expect("publish conversion output");
    let mut published = PublishedOutput::new(final_path);
    sync_parent_directory(final_path).expect("sync published conversion output");
    std::fs::remove_file(&partial_path).expect("remove partial conversion output link");
    sync_parent_directory(final_path).expect("sync conversion output directory");
    if let Err(signum) = commit_completion() {
        published
            .rollback()
            .expect("roll back interrupted conversion output");
        return Err(signum);
    }
    published.disarm();
    drop(partial);
    Ok(converted)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: convert <input.txt> <output.data>");
        eprintln!("  input: one position per line, `fen | white_score_cp | white_wdl`");
        std::process::exit(1);
    }
    if let Err(error) = install_signal_handlers() {
        eprintln!("could not install conversion signal handlers: {error}");
        std::process::exit(2);
    }
    match convert(Path::new(&args[1]), Path::new(&args[2])) {
        Ok(converted) => println!("Total Positions: {converted}"),
        Err(signum) => {
            eprintln!("conversion terminated by signal {signum}");
            std::process::exit(128 + signum);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_output() -> PathBuf {
        std::env::temp_dir().join(format!(
            "bitfox-convert-test-{}-{}",
            std::process::id(),
            TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn partial_outputs_are_unique_and_removed_by_the_guard() {
        let final_path = test_output();
        let (first, first_file) = create_partial(&final_path);
        let (second, second_file) = create_partial(&final_path);
        let first_guard = PartialOutput(first.clone());
        let second_guard = PartialOutput(second.clone());

        assert_ne!(first, second);
        assert!(first.exists());
        assert!(second.exists());
        drop((first_file, second_file, first_guard, second_guard));
        assert!(!first.exists());
        assert!(!second.exists());
    }

    #[test]
    fn termination_state_has_one_completion_boundary() {
        TERMINATION_SIGNAL.store(0, Ordering::Relaxed);
        request_termination(15);
        assert_eq!(termination_signal(), Some(15));

        let rolled_back = test_output();
        File::create(&rolled_back).unwrap();
        {
            let _published = PublishedOutput::new(&rolled_back);
            assert_eq!(commit_completion(), Err(15));
        }
        assert!(!rolled_back.exists());

        TERMINATION_SIGNAL.store(0, Ordering::Relaxed);
        let committed = test_output();
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
