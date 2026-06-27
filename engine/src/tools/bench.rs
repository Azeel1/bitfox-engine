use crate::board::Board;
use crate::search::{Limits, Search};
use std::time::Instant;

const DEFAULT_DEPTH: i32 = 12;

const POSITIONS: &[&str] = &[
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 1",
    "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
    "r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4",
    "2rq1rk1/pp1bppbp/2np1np1/8/3NP3/2N1BP2/PPPQ2PP/2KR1B1R w - - 0 11",
    "8/8/8/2k5/2pP4/8/B7/4K3 b - d3 0 1",
    "r3k2r/pb3p2/5npp/n2p4/1p1PPB2/6P1/P2N1PBP/R3K2R w KQkq - 0 16",
    "8/3k4/8/8/8/8/3K1R2/8 w - - 0 1",
    "3r2k1/p2r1p1p/1p2p1p1/q4n2/3P4/PQ5P/1P1RNPP1/3R2K1 b - - 0 1",
    "8/p3k3/1p6/2p5/2P5/1P6/P3K3/8 w - - 0 1",
];

pub fn bench(args: &[String]) {
    let depth: i32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_DEPTH);
    let mut search = Search::new(16);
    let mut total_nodes = 0u64;
    let start = Instant::now();

    for fen in POSITIONS {
        let mut board = Board::from_fen(fen).expect("valid bench fen");
        search.clear();
        let limits = Limits { depth: Some(depth), ..Default::default() };
        search.think(&mut board, &limits, false);
        total_nodes += search.node_count();
    }

    let elapsed = start.elapsed().as_secs_f64();
    let nps = (total_nodes as f64 / elapsed.max(1e-9)) as u64;
    println!("{total_nodes} nodes {nps} nps");
}
