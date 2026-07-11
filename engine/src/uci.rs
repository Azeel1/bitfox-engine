use crate::board::Board;
use crate::search::{Limits, Search, UCI_OPTIONS};
use std::io::{BufRead, Write};
use std::sync::atomic::Ordering;
use std::sync::mpsc;

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
    let mut search_slot = Some(search);
    let (tx, rx) = mpsc::channel::<(Box<Search>, crate::types::Move)>();
    let mut searching = false;

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let cmd = line.split_whitespace().next().unwrap_or("");

        if searching {
            match cmd {
                "stop" => {
                    stop_flag.store(true, Ordering::Relaxed);
                    let _ = std::io::stdout().flush();
                    continue;
                }
                "isready" => {
                    println!("readyok");
                    let _ = std::io::stdout().flush();
                    continue;
                }
                "quit" => {
                    stop_flag.store(true, Ordering::Relaxed);
                    if let Ok((_, best)) = rx.recv() {
                        println!("bestmove {}", best.to_uci());
                    }
                    break;
                }
                _ => {
                    if let Ok((s, best)) = rx.recv() {
                        search_slot = Some(s);
                        println!("bestmove {}", best.to_uci());
                    }
                    searching = false;
                }
            }
        }

        let search = search_slot.as_mut().unwrap();
        match cmd {
            "uci" => {
                println!("id name Bitfox {}", env!("CARGO_PKG_VERSION"));
                println!("id author Bitfox Team");
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
                let limits = parse_go(&line);
                stop_flag.store(false, Ordering::Relaxed);
                let mut s = search_slot.take().unwrap();
                let mut b = board.clone();
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let best = s.think(&mut b, &limits, true);
                    let _ = tx.send((s, best));
                });
                searching = true;
            }
            "stop" => {}
            "quit" => break,
            _ => {}
        }
        let _ = std::io::stdout().flush();
    }
}

fn apply_move(board: &mut Board, mv: &str) -> bool {
    let legal = board.generate_legal();
    for i in 0..legal.len() {
        let m = legal.moves[i];
        if m.to_uci() == mv {
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
            "movetime" => limits.movetime = it.next().and_then(|s| s.parse().ok()),
            "wtime" => limits.wtime = it.next().and_then(|s| s.parse().ok()),
            "btime" => limits.btime = it.next().and_then(|s| s.parse().ok()),
            "winc" => limits.winc = it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            "binc" => limits.binc = it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            "movestogo" => limits.movestogo = it.next().and_then(|s| s.parse().ok()),
            "nodes" => limits.nodes = it.next().and_then(|s| s.parse().ok()),
            "infinite" => limits.infinite = true,
            _ => {}
        }
    }
    limits
}

fn set_option(line: &str, search: &mut Search) {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let name = tokens
        .iter()
        .position(|&t| t == "name")
        .and_then(|i| tokens.get(i + 1))
        .map(|s| s.to_ascii_lowercase());
    let value = tokens
        .iter()
        .position(|&t| t == "value")
        .and_then(|i| tokens.get(i + 1));
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
            let path = line
                .split_once(" value ")
                .map(|(_, v)| v.trim())
                .unwrap_or("");
            if !path.is_empty() && path != "<empty>" {
                if search.set_tablebase(path) {
                    println!("info string syzygy tablebases loaded");
                } else {
                    println!("info string no syzygy tables found at {path}");
                }
            }
        } else if let Ok(v) = value.parse::<i32>() {
            search.set_param(&name, v);
        }
    }
}
