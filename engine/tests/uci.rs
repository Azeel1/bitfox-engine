use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

struct UciChild {
    child: Child,
    input: ChildStdin,
    lines: Receiver<String>,
    stopped: bool,
}

impl UciChild {
    fn spawn() -> UciChild {
        let mut child = Command::new(env!("CARGO_BIN_EXE_bitfox"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start Bitfox");
        let input = child.stdin.take().expect("open Bitfox stdin");
        let output = child.stdout.take().expect("open Bitfox stdout");
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        UciChild {
            child,
            input,
            lines,
            stopped: false,
        }
    }

    fn send(&mut self, command: &str) {
        writeln!(self.input, "{command}").expect("write UCI command");
        self.input.flush().expect("flush UCI command");
    }

    fn collect_until(&self, expected: impl Fn(&str) -> bool) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let line = self
                .lines
                .recv_timeout(remaining)
                .unwrap_or_else(|error| panic!("UCI response timed out after {seen:?}: {error}"));
            let done = expected(&line);
            seen.push(line);
            if done {
                return seen;
            }
        }
    }

    fn wait_for(&self, expected: &str) {
        self.collect_until(|line| line == expected);
    }

    fn wait_for_prefix(&self, expected: &str) -> Vec<String> {
        self.collect_until(|line| line.starts_with(expected))
    }

    fn assert_waiting_for_stop(&mut self) {
        self.send("isready");
        let output = self.collect_until(|line| line == "readyok");
        assert!(
            output.iter().all(|line| !line.starts_with("bestmove ")),
            "infinite search returned before stop: {output:?}"
        );
        let deadline = Instant::now() + Duration::from_millis(50);
        loop {
            match self
                .lines
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(line) => assert!(
                    !line.starts_with("bestmove "),
                    "infinite search returned before stop: {line}"
                ),
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(error) => panic!("engine output closed while waiting for stop: {error}"),
            }
        }
    }

    fn stop_process(&mut self) {
        if self.stopped {
            return;
        }
        let _ = writeln!(self.input, "quit");
        let _ = self.input.flush();
        self.wait_for_exit();
    }

    fn wait_for_exit(&mut self) {
        if self.stopped {
            return;
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    self.stopped = true;
                    assert!(status.success(), "Bitfox exited with {status}");
                    return;
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                _ => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    self.stopped = true;
                    panic!("Bitfox did not exit after quit");
                }
            }
        }
    }
}

impl Drop for UciChild {
    fn drop(&mut self) {
        if self.stopped {
            return;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn initialize(engine: &mut UciChild) {
    engine.send("uci");
    engine.wait_for("uciok");
    engine.send("isready");
    engine.wait_for("readyok");
}

#[test]
fn bestmove_precedes_the_next_state_command_response() {
    let mut engine = UciChild::spawn();
    initialize(&mut engine);
    engine.send("position startpos");
    engine.send("go infinite");
    engine.wait_for_prefix("info depth ");
    engine.send("uci");
    let lines = engine.collect_until(|line| line == "uciok");
    let bestmove = lines
        .iter()
        .position(|line| line.starts_with("bestmove "))
        .expect("missing bestmove before uciok");
    let identity = lines
        .iter()
        .position(|line| line.starts_with("id name "))
        .expect("missing engine identity");
    assert!(
        bestmove < identity,
        "unexpected UCI output order: {lines:?}"
    );
    engine.stop_process();
}

#[test]
fn four_thread_search_stops_and_accepts_another_go() {
    let mut engine = UciChild::spawn();
    initialize(&mut engine);
    engine.send("setoption name Threads value 4");
    engine.send("isready");
    engine.wait_for("readyok");
    engine.send("position startpos");
    engine.send("go infinite");
    engine.wait_for_prefix("info depth ");
    engine.send("stop");
    let first = engine.wait_for_prefix("bestmove ");
    assert!(!first.last().unwrap().ends_with(" 0000"));
    engine.send("position startpos moves e2e4");
    engine.send("go depth 2");
    let second = engine.wait_for_prefix("bestmove ");
    assert!(!second.last().unwrap().ends_with(" 0000"));
    engine.stop_process();
}

#[test]
fn clear_and_resize_wait_for_the_active_helper_pool() {
    let mut engine = UciChild::spawn();
    initialize(&mut engine);
    engine.send("setoption name Threads value 4");
    engine.send("position startpos");
    engine.send("go infinite");
    engine.wait_for_prefix("info depth ");

    engine.send("setoption name Hash value 16");
    engine.wait_for_prefix("bestmove ");
    engine.send("ucinewgame");
    engine.send("setoption name Threads value 2");
    engine.send("position startpos moves e2e4");
    engine.send("go depth 3");
    let best = engine.wait_for_prefix("bestmove ");
    assert!(!best.last().unwrap().ends_with(" 0000"));
    engine.stop_process();
}

#[test]
fn isready_does_not_stop_an_infinite_search() {
    let mut engine = UciChild::spawn();
    initialize(&mut engine);
    engine.send("setoption name Threads value 4");
    engine.send("position startpos");
    engine.send("go infinite");
    engine.wait_for_prefix("info depth ");

    engine.send("isready");
    let lines = engine.collect_until(|line| line == "readyok");
    assert!(
        lines.iter().all(|line| !line.starts_with("bestmove ")),
        "isready stopped the search: {lines:?}"
    );

    engine.send("stop");
    engine.wait_for_prefix("bestmove ");
    engine.stop_process();
}

#[test]
fn expired_clocks_still_produce_a_bestmove() {
    let mut engine = UciChild::spawn();
    initialize(&mut engine);
    for (position, command) in [
        ("position startpos", "go wtime -50"),
        ("position startpos moves e2e4", "go btime -50"),
        ("position startpos", "go movetime -50"),
    ] {
        engine.send(position);
        engine.send(command);
        let output = engine.wait_for_prefix("bestmove ");
        assert!(!output.last().unwrap().ends_with(" 0000"));
    }
    engine.stop_process();
}

#[test]
fn tablebase_option_accepts_tabs_without_disabling_the_path() {
    let mut engine = UciChild::spawn();
    initialize(&mut engine);
    let path =
        std::env::temp_dir().join(format!("bitfox-missing  tablebases-{}", std::process::id()));
    engine.send(&format!(
        "setoption\tname\tSyzygyPath\tvalue\t{}",
        path.display()
    ));
    engine.wait_for(&format!(
        "info string no syzygy tables found at {}",
        path.display()
    ));
    engine.send("setoption\tname\tSyzygyPath\tvalue\t");
    engine.wait_for("info string syzygy tablebases disabled");
    engine.stop_process();
}

#[test]
fn quit_stops_and_joins_an_infinite_search() {
    let mut engine = UciChild::spawn();
    initialize(&mut engine);
    engine.send("setoption name Threads value 4");
    engine.send("position startpos");
    engine.send("go infinite");
    engine.wait_for_prefix("info depth ");

    let started = Instant::now();
    engine.send("quit");
    engine.wait_for_exit();
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "quit did not stop the search promptly"
    );
}

#[test]
fn infinite_depth_search_waits_for_stop_after_reaching_its_depth() {
    for threads in [1, 4] {
        let mut engine = UciChild::spawn();
        initialize(&mut engine);
        engine.send("setoption name Hash value 16");
        engine.send(&format!("setoption name Threads value {threads}"));
        for command in ["go infinite depth 2", "go depth 2 infinite"] {
            engine.send("ucinewgame");
            engine.send("position startpos");
            engine.send(command);
            let output = engine.collect_until(|line| line.starts_with("info depth 2 "));
            assert!(output.iter().all(|line| !line.starts_with("bestmove ")));
            engine.assert_waiting_for_stop();
            engine.send("stop");
            let stopped = engine.wait_for_prefix("bestmove ");
            assert!(!stopped.last().unwrap().ends_with(" 0000"));
            engine.send("position startpos moves e2e4");
            engine.send("go depth 2");
            let reused = engine.wait_for_prefix("bestmove ");
            assert!(reused.iter().any(|line| line.starts_with("info depth 2 ")));
            assert!(!reused.last().unwrap().ends_with(" 0000"));
        }
        engine.stop_process();
    }
}

#[test]
fn terminal_infinite_depth_search_waits_for_stop_and_accepts_reuse() {
    for threads in [1, 4] {
        let mut engine = UciChild::spawn();
        initialize(&mut engine);
        engine.send("setoption name Hash value 16");
        engine.send(&format!("setoption name Threads value {threads}"));
        for command in ["go infinite depth 2", "go depth 2 infinite"] {
            engine.send("position fen 7k/6Q1/5K2/8/8/8/8/8 b - - 0 1");
            engine.send("go depth 2");
            let finite = engine.wait_for_prefix("bestmove ");
            assert_eq!(finite.last().unwrap(), "bestmove 0000");
            engine.send(command);
            engine.assert_waiting_for_stop();
            engine.send("stop");
            let stopped = engine.wait_for_prefix("bestmove ");
            assert_eq!(stopped.last().unwrap(), "bestmove 0000");
            engine.send("position startpos");
            engine.send("go depth 2");
            let reused = engine.wait_for_prefix("bestmove ");
            assert!(reused.iter().any(|line| line.starts_with("info depth 2 ")));
            assert!(!reused.last().unwrap().ends_with(" 0000"));
        }
        engine.stop_process();
    }
}
