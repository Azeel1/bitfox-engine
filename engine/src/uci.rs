use crate::board::Board;
use crate::search::{Limits, Search};
use std::io::{BufRead, Write};

pub fn run() {
    let mut board = Board::startpos();
    let mut search = Box::new(Search::new(64));

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let cmd = line.split_whitespace().next().unwrap_or("");
        match cmd {
            "uci" => {
                println!("id name Bitfox {}", env!("CARGO_PKG_VERSION"));
                println!("id author Bitfox Team");
                println!("option name Hash type spin default 64 min 1 max 4096");
                println!("option name Threads type spin default 1 min 1 max 256");
                println!("uciok");
            }
            "isready" => println!("readyok"),
            "ucinewgame" => {
                search.clear();
                board = Board::startpos();
            }
            "setoption" => set_option(&line, &mut search),
            "position" => set_position(&line, &mut board),
            "go" => {
                let limits = parse_go(&line);
                let best = search.think(&mut board, &limits, true);
                println!("bestmove {}", best.to_uci());
            }
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
                search.resize_tt(mb);
            }
        } else if name == "threads" {
            if let Ok(n) = value.parse::<usize>() {
                search.set_threads(n);
            }
        }
    }
}
