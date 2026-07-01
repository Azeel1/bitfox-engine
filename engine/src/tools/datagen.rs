use crate::board::Board;
use crate::search::{Limits, Search};
use crate::types::Color;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

struct Rng(u64);

impl Rng {
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

fn reserve_position(written: &AtomicUsize, target: usize) -> bool {
    let mut current = written.load(Ordering::Relaxed);
    loop {
        if current >= target {
            return false;
        }
        match written.compare_exchange_weak(
            current,
            current + 1,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return true,
            Err(next) => current = next,
        }
    }
}

fn generate_game(
    rng: &mut Rng,
    search: &mut Search,
    openings: &[String],
    nodes: u64,
) -> Vec<(String, i32, f64)> {
    let mut board = if openings.is_empty() {
        Board::startpos()
    } else {
        match Board::from_fen(&openings[rng.below(openings.len())]) {
            Some(b) => b,
            None => return Vec::new(),
        }
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
    let result;

    loop {
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
        let best = search.think(&mut board, &limits, false);
        if best.is_none() {
            result = 0.5;
            break;
        }
        let score = search.best_score();

        if !board.in_check(board.side()) && best.is_quiet() && score.abs() < 20_000 {
            samples.push((board.to_fen(), score, board.side()));
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

pub fn datagen(args: &[String]) {
    let target: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(100_000);
    let nodes: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(5_000);
    let path = args.get(2).map(String::as_str).unwrap_or("data.txt");
    let mut openings_path = args.get(3).map(String::as_str);
    let workers = match (args.get(3), args.get(4)) {
        (Some(value), None) => match value.parse::<usize>() {
            Ok(n) => {
                openings_path = None;
                n.max(1)
            }
            Err(_) => 1,
        },
        (_, Some(value)) => value.parse::<usize>().unwrap_or(1).max(1),
        _ => 1,
    };
    if openings_path == Some("-") {
        openings_path = None;
    }

    let openings: Vec<String> = match openings_path {
        Some(p) => std::fs::read_to_string(p)
            .map(|s| {
                s.lines()
                    .map(str::to_string)
                    .filter(|l| !l.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        None => Vec::new(),
    };

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
        | 1;
    let file = File::create(path).expect("create datagen output");
    let out = Arc::new(Mutex::new(BufWriter::new(file)));
    let openings = Arc::new(openings);
    let written = Arc::new(AtomicUsize::new(0));
    let games = Arc::new(AtomicUsize::new(0));

    std::thread::scope(|scope| {
        for worker in 0..workers {
            let out = Arc::clone(&out);
            let openings = Arc::clone(&openings);
            let written = Arc::clone(&written);
            let games = Arc::clone(&games);
            scope.spawn(move || {
                let worker_seed = seed ^ 0x9e37_79b9_7f4a_7c15u64.wrapping_mul(worker as u64 + 1);
                let mut rng = Rng(worker_seed);
                let mut search = Search::new(16);

                while written.load(Ordering::Relaxed) < target {
                    let rows = generate_game(&mut rng, &mut search, &openings, nodes);
                    if rows.is_empty() {
                        continue;
                    }

                    {
                        let mut out = out.lock().expect("datagen output lock");
                        for (fen, score, result) in rows {
                            if !reserve_position(&written, target) {
                                break;
                            }
                            writeln!(out, "{fen} | {score} | {result:.1}").expect("write sample");
                        }
                    }

                    let game_no = games.fetch_add(1, Ordering::Relaxed) + 1;
                    if game_no % 50 == 0 {
                        let total = written.load(Ordering::Relaxed);
                        if let Ok(mut out) = out.lock() {
                            out.flush().ok();
                        }
                        eprintln!("datagen: {total}/{target} positions, {game_no} games");
                    }
                }
            });
        }
    });

    if let Ok(mut out) = out.lock() {
        out.flush().ok();
    }
    let written = written.load(Ordering::Relaxed);
    let games = games.load(Ordering::Relaxed);
    eprintln!("datagen done: {written} positions in {games} games -> {path}");
}
