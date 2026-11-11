use crate::board::Board;
use crate::search::{Limits, Search, UCI_OPTIONS};
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

const STOP_TIMEOUT: Duration = Duration::from_secs(5);

fn recv_before<T>(receiver: &Receiver<T>, deadline: Instant) -> Result<T, RecvTimeoutError> {
    receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
}

fn request_stop(search_stop: &AtomicBool, external_stop: &AtomicBool) {
    external_stop.store(true, Ordering::Relaxed);
    search_stop.store(true, Ordering::Relaxed);
}

pub fn run() {
    let mut board = Board::startpos();
    let mut search = Box::new(Search::new(64));

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let default_tb = dir.join("syzygy");
            if default_tb.is_dir() {
                search.set_tablebase(&default_tb.to_string_lossy());
            }
        }
    }

    let stop_flag = search.stop_signal();
    let external_stop = Arc::new(AtomicBool::new(false));
    let worker_external_stop = Arc::clone(&external_stop);
    let mut search_slot = Some(search);
    let (job_tx, job_rx) = mpsc::channel::<(Box<Search>, Board, Limits)>();
    let (result_tx, result_rx) = mpsc::channel::<Box<Search>>();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        while let Ok((mut search, mut board, limits)) = job_rx.recv() {
            let best = search.think_started(&mut board, &limits, true);
            if limits.infinite {
                while !worker_external_stop.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            println!("bestmove {}", best.to_uci());
            let _ = std::io::stdout().flush();
            if result_tx.send(search).is_err() {
                break;
            }
        }
        let _ = done_tx.send(());
    });
    let mut searching = false;
    let mut shutdown_deadline = None;

    let stdin = std::io::stdin();
    'input: for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let cmd = line.split_whitespace().next().unwrap_or("");

        if searching {
            match cmd {
                "stop" => {
                    request_stop(&stop_flag, &external_stop);
                    let _ = std::io::stdout().flush();
                    continue;
                }
                "isready" => {
                    println!("readyok");
                    let _ = std::io::stdout().flush();
                    continue;
                }
                "quit" => {
                    request_stop(&stop_flag, &external_stop);
                    let deadline = Instant::now() + STOP_TIMEOUT;
                    shutdown_deadline = Some(deadline);
                    if recv_before(&result_rx, deadline).is_ok() {
                        searching = false;
                    }
                    break 'input;
                }
                _ => match result_rx.try_recv() {
                    Ok(s) => {
                        search_slot = Some(s);
                        searching = false;
                    }
                    Err(mpsc::TryRecvError::Empty) if requires_idle(cmd) => {
                        request_stop(&stop_flag, &external_stop);
                        let deadline = Instant::now() + STOP_TIMEOUT;
                        match recv_before(&result_rx, deadline) {
                            Ok(s) => {
                                search_slot = Some(s);
                                searching = false;
                            }
                            Err(_) => {
                                shutdown_deadline = Some(deadline);
                                break 'input;
                            }
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => break 'input,
                },
            }
        }

        let search = search_slot.as_mut().unwrap();
        match cmd {
            "uci" => {
                println!("id name Bitfox {}", env!("CARGO_PKG_VERSION"));
                println!("id author Azeel");
                println!("option name Hash type spin default 64 min 1 max 4096");
                println!("option name Threads type spin default 1 min 1 max 256");
                println!("option name SyzygyPath type string default <empty>");
                for p in UCI_OPTIONS {
                    println!(
                        "option name {} type spin default {} min {} max {}",
                        p.name, p.default, p.min, p.max
                    );
                }
                println!("uciok");
            }
            "isready" => println!("readyok"),
            "ucinewgame" => {
                search.clear();
                board = Board::startpos();
            }
            "setoption" => set_option(&line, search),
            "position" => set_position(&line, &mut board),
            "go" => {
                let started_at = Instant::now();
                let mut limits = parse_go(&line);
                limits.started_at = Some(started_at);
                let s = search_slot.take().unwrap();
                external_stop.store(false, Ordering::Relaxed);
                s.reset_stop();
                match job_tx.send((s, board.clone(), limits)) {
                    Ok(()) => searching = true,
                    Err(_) => break,
                }
            }
            "stop" => {}
            "quit" => break,
            _ => {}
        }
        let _ = std::io::stdout().flush();
    }
    let deadline = shutdown_deadline.unwrap_or_else(|| Instant::now() + STOP_TIMEOUT);
    if searching {
        request_stop(&stop_flag, &external_stop);
        let _ = recv_before(&result_rx, deadline);
    }
    drop(job_tx);
    let worker_stopped = match recv_before(&done_rx, deadline) {
        Ok(()) | Err(RecvTimeoutError::Disconnected) => true,
        Err(RecvTimeoutError::Timeout) => false,
    };
    if worker_stopped {
        let _ = worker.join();
    }
}

fn requires_idle(cmd: &str) -> bool {
    matches!(cmd, "uci" | "ucinewgame" | "setoption" | "position" | "go")
}

fn apply_move(board: &mut Board, mv: &str) -> bool {
    let legal = board.generate_legal();
    for i in 0..legal.len() {
        let m = legal.moves[i];
        if m.to_uci().eq_ignore_ascii_case(mv) {
            board.make_move(m);
            return true;
        }
    }
    false
}

fn set_position(line: &str, board: &mut Board) {
    let mut it = line.split_whitespace().peekable();
    it.next();
    match it.next() {
        Some("startpos") => *board = Board::startpos(),
        Some("fen") => {
            let mut fen = String::new();
            while let Some(&t) = it.peek() {
                if t == "moves" {
                    break;
                }
                fen.push_str(it.next().unwrap());
                fen.push(' ');
            }
            match Board::from_fen(fen.trim()) {
                Some(b) => *board = b,
                None => return,
            }
        }
        _ => return,
    }
    if it.peek() == Some(&"moves") {
        it.next();
    }
    for mv in it {
        if !apply_move(board, mv) {
            break;
        }
    }
}

fn parse_go(line: &str) -> Limits {
    let mut limits = Limits::default();
    let mut it = line.split_whitespace();
    it.next();
    while let Some(token) = it.next() {
        match token {
            "depth" => limits.depth = it.next().and_then(|s| s.parse().ok()),
            "movetime" => limits.movetime = it.next().and_then(parse_millis),
            "wtime" => limits.wtime = it.next().and_then(parse_millis),
            "btime" => limits.btime = it.next().and_then(parse_millis),
            "winc" => limits.winc = it.next().and_then(parse_millis).unwrap_or(0),
            "binc" => limits.binc = it.next().and_then(parse_millis).unwrap_or(0),
            "movestogo" => limits.movestogo = it.next().and_then(|s| s.parse().ok()),
            "nodes" => limits.nodes = it.next().and_then(|s| s.parse().ok()),
            "infinite" => limits.infinite = true,
            _ => {}
        }
    }
    if limits.depth.is_none()
        && limits.movetime.is_none()
        && limits.wtime.is_none()
        && limits.btime.is_none()
        && limits.nodes.is_none()
    {
        limits.infinite = true;
    }
    limits
}

fn parse_millis(value: &str) -> Option<u64> {
    value
        .parse::<i128>()
        .ok()
        .map(|value| value.clamp(0, i128::from(u64::MAX)) as u64)
}

fn option_value(line: &str) -> Option<&str> {
    let mut remaining = line;
    while !remaining.is_empty() {
        remaining = remaining.trim_start();
        let end = remaining
            .find(char::is_whitespace)
            .unwrap_or(remaining.len());
        let (token, rest) = remaining.split_at(end);
        if token == "value" {
            return Some(rest.trim());
        }
        remaining = rest;
    }
    None
}

fn set_option(line: &str, search: &mut Search) {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let name = tokens
        .iter()
        .position(|&t| t == "name")
        .and_then(|i| tokens.get(i + 1))
        .map(|s| s.to_ascii_lowercase());
    let value = option_value(line);
    if let (Some(name), Some(value)) = (name, value) {
        if name == "hash" {
            if let Ok(mb) = value.parse::<usize>() {
                search.resize_tt(mb.clamp(1, 4096));
            }
        } else if name == "threads" {
            if let Ok(n) = value.parse::<usize>() {
                search.set_threads(n.clamp(1, 256));
            }
        } else if name == "syzygypath" {
            let path = value;
            if path.is_empty() || path == "<empty>" {
                search.set_tablebase("");
                println!("info string syzygy tablebases disabled");
            } else if search.set_tablebase(path) {
                println!("info string syzygy tablebases loaded");
            } else {
                println!("info string no syzygy tables found at {path}");
            }
        } else if let Ok(v) = value.parse::<i32>() {
            search.set_param(&name, v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_changing_commands_are_not_dropped_during_search() {
        for cmd in ["uci", "ucinewgame", "setoption", "position", "go"] {
            assert!(requires_idle(cmd));
        }
        for cmd in ["", "stop", "isready", "quit", "ponderhit"] {
            assert!(!requires_idle(cmd));
        }
    }

    #[test]
    fn positions_accept_short_fens_and_uppercase_promotions() {
        let mut board = Board::startpos();
        set_position(
            "position fen 7k/P7/8/8/8/8/8/K7 w - - moves a7a8Q",
            &mut board,
        );
        assert_eq!(board.to_fen(), "Q6k/8/8/8/8/8/8/K7 b - - 0 1");

        set_position(
            "position fen 7k/8/8/8/8/8/8/K7 w - - moves A1B1",
            &mut board,
        );
        assert_eq!(board.to_fen(), "7k/8/8/8/8/8/8/1K6 b - - 1 1");

        set_position("position fen 7k/8/8/8/8/8/8/K6R w - moves h1h2", &mut board);
        assert_eq!(board.to_fen(), "7k/8/8/8/8/8/8/1K6 b - - 1 1");
    }

    #[test]
    fn go_parser_keeps_independent_limits() {
        let limits =
            parse_go("go wtime 1000 btime 900 winc 12 binc 13 movestogo 20 depth 8 nodes 5000");
        assert_eq!(limits.wtime, Some(1000));
        assert_eq!(limits.btime, Some(900));
        assert_eq!(limits.winc, 12);
        assert_eq!(limits.binc, 13);
        assert_eq!(limits.movestogo, Some(20));
        assert_eq!(limits.depth, Some(8));
        assert_eq!(limits.nodes, Some(5000));
        assert!(!limits.infinite);
    }

    #[test]
    fn go_without_a_stopping_condition_is_infinite() {
        assert!(parse_go("go").infinite);
        assert!(parse_go("go winc 10").infinite);
        assert!(!parse_go("go depth 8").infinite);
    }

    #[test]
    fn negative_clocks_remain_finite_limits() {
        for command in ["go wtime -5", "go btime -5", "go movetime -5"] {
            let limits = parse_go(command);
            assert!(!limits.infinite, "{command}");
            assert_eq!(limits.wtime.or(limits.btime).or(limits.movetime), Some(0));
        }
        let limits = parse_go("go wtime -1 btime 200 winc -8 binc 10");
        assert_eq!(limits.wtime, Some(0));
        assert_eq!(limits.btime, Some(200));
        assert_eq!(limits.winc, 0);
        assert_eq!(limits.binc, 10);
        assert_eq!(parse_millis("18446744073709551615"), Some(u64::MAX));
        assert_eq!(parse_millis("bad"), None);
    }

    #[test]
    fn tablebase_paths_preserve_spaces_with_arbitrary_token_whitespace() {
        for command in [
            "setoption name SyzygyPath value /some  tables/",
            "setoption\tname\tSyzygyPath\tvalue\t/some  tables/\t",
            "setoption  name SyzygyPath\t value  /some  tables/ ",
        ] {
            assert_eq!(option_value(command), Some("/some  tables/"));
        }
        assert_eq!(option_value("setoption name SyzygyPath"), None);
        assert_eq!(
            option_value("setoption name ValueScale value 12"),
            Some("12")
        );
    }

    #[test]
    fn explicit_infinite_is_retained_with_other_limits() {
        for command in [
            "go",
            "go infinite",
            "go winc 10",
            "go binc 10",
            "go infinite depth 4",
            "go depth 4 infinite",
            "go infinite nodes 16",
            "go infinite movetime 1",
            "go infinite wtime 100",
            "go infinite btime 100",
            "go infinite wtime -1",
        ] {
            assert!(parse_go(command).infinite, "{command}");
        }
        for command in ["go depth 4", "go nodes 16", "go movetime 1"] {
            assert!(!parse_go(command).infinite, "{command}");
        }
    }

    #[test]
    fn external_cancellation_is_distinct_from_normal_helper_stop() {
        let search_stop = AtomicBool::new(true);
        let external_stop = AtomicBool::new(false);
        assert!(!external_stop.load(Ordering::Relaxed));
        request_stop(&search_stop, &external_stop);
        assert!(search_stop.load(Ordering::Relaxed));
        assert!(external_stop.load(Ordering::Relaxed));
    }
}
