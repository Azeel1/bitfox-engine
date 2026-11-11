#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}

struct OwnedChild(Child);

impl std::ops::Deref for OwnedChild {
    type Target = Child;

    fn deref(&self) -> &Child {
        &self.0
    }
}

impl std::ops::DerefMut for OwnedChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[test]
fn sigterm_removes_datagen_partials() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "bitfox-datagen-signal-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let output = root.join("output.txt");

    let child = Command::new(env!("CARGO_BIN_EXE_bitfox"))
        .args(["datagen", "1000000", "100000000"])
        .arg(&output)
        .args(["-", "1", "1"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut child = OwnedChild(child);

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if partials(&root).next().is_some() {
            break;
        }
        if child.try_wait().unwrap().is_some() {
            panic!("datagen exited before the signal test");
        }
        assert!(
            Instant::now() < deadline,
            "datagen did not create a partial output"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    std::thread::sleep(Duration::from_millis(100));
    let signaled_at = Instant::now();
    assert_eq!(unsafe { kill(child.id() as i32, 15) }, 0);
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("datagen did not stop after SIGTERM");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(status.code(), Some(143));
    assert!(signaled_at.elapsed() < Duration::from_secs(5));
    assert!(!output.exists());
    assert!(partials(&root).next().is_none());
    fs::remove_dir_all(root).unwrap();
}

fn partials(root: &Path) -> impl Iterator<Item = PathBuf> {
    fs::read_dir(root).unwrap().filter_map(|entry| {
        let path = entry.ok()?.path();
        path.file_name()?
            .to_string_lossy()
            .contains(".partial.")
            .then_some(path)
    })
}
