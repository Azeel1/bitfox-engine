use crate::board::Board;
use std::time::Instant;

fn parse_fen(args: &[String]) -> Board {
    if args.is_empty() {
        return Board::startpos();
    }
    let fen = args.join(" ");
    Board::from_fen(&fen).unwrap_or_else(|| {
        eprintln!("invalid fen: {fen}");
        std::process::exit(1);
    })
}

pub fn perft(args: &[String]) {
    let depth: u32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(5);
    let mut board = parse_fen(&args[1.min(args.len())..]);
    let start = Instant::now();
    let nodes = board.perft(depth);
    let secs = start.elapsed().as_secs_f64();
    println!("perft({depth}) = {nodes}");
    println!("time {secs:.3}s  nps {:.0}", nodes as f64 / secs.max(1e-9));
}

pub fn divide(args: &[String]) {
    let depth: u32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(1);
    let mut board = parse_fen(&args[1.min(args.len())..]);
    let moves = board.generate_legal();
    let mut total = 0u64;
    for i in 0..moves.len() {
        let m = moves.moves[i];
        board.make_move(m);
        let nodes = if depth <= 1 {
            1
        } else {
            board.perft(depth - 1)
        };
        board.unmake_move(m);
        total += nodes;
        println!("{}: {}", m.to_uci(), nodes);
    }
    println!("\ntotal: {total}");
}
