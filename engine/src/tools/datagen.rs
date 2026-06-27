use crate::board::Board;
use crate::search::{Limits, Search};
use crate::types::Color;
use std::fs::File;
use std::io::{BufWriter, Write};
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

pub fn datagen(args: &[String]) {
    let target: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(100_000);
    let nodes: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(5_000);
    let path = args.get(2).map(String::as_str).unwrap_or("data.txt");
    let openings: Vec<String> = match args.get(3) {
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
    let mut rng = Rng(seed);
    let mut search = Search::new(16);
    let file = File::create(path).expect("create datagen output");
    let mut out = BufWriter::new(file);

    let mut written = 0usize;
    let mut games = 0usize;

    while written < target {
        let mut board = if openings.is_empty() {
            Board::startpos()
        } else {
            match Board::from_fen(&openings[rng.below(openings.len())]) {
                Some(b) => b,
                None => continue,
            }
        };
        let opening_plies = if openings.is_empty() {
            6 + rng.below(5)
        } else {
            rng.below(3)
        };
        let mut aborted = false;
        for _ in 0..opening_plies {
            let legal = board.generate_legal();
            if legal.is_empty() {
                aborted = true;
                break;
            }
            board.make_move(legal.moves[rng.below(legal.len())]);
        }
        if aborted {
            continue;
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

        for (fen, score, side) in samples {
            let white_score = if side == Color::White { score } else { -score };
            writeln!(out, "{fen} | {white_score} | {result:.1}").expect("write sample");
            written += 1;
        }

        games += 1;
        if games % 50 == 0 {
            out.flush().ok();
            eprintln!("datagen: {written}/{target} positions, {games} games");
        }
    }

    out.flush().ok();
    eprintln!("datagen done: {written} positions in {games} games -> {path}");
}
