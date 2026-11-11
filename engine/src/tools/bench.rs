use crate::board::Board;
use crate::search::{Limits, Search};
use crate::MAX_PLY;
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

fn settings(args: &[String]) -> (i32, usize, usize) {
    assert!(args.len() <= 3, "usage: bench [depth] [threads] [hash_mb]");
    let depth: i32 = args
        .first()
        .map(|value| value.parse().expect("bench depth must be an integer"))
        .unwrap_or(DEFAULT_DEPTH);
    let threads: usize = args
        .get(1)
        .map(|value| value.parse().expect("bench threads must be an integer"))
        .unwrap_or(1);
    let hash_mb: usize = args
        .get(2)
        .map(|value| value.parse().expect("bench hash must be an integer"))
        .unwrap_or(16);
    assert!(
        (1..=MAX_PLY as i32).contains(&depth),
        "bench depth is outside the search range"
    );
    assert!(
        (1..=256).contains(&threads),
        "bench threads must be from 1 to 256"
    );
    assert!(
        (1..=4096).contains(&hash_mb),
        "bench hash must be from 1 to 4096 MB"
    );
    (depth, threads, hash_mb)
}

pub fn bench(args: &[String]) {
    let (depth, threads, hash_mb) = settings(args);
    let mut search = Search::new(hash_mb);
    search.set_threads(threads);
    let mut total_nodes = 0u64;
    #[cfg(feature = "stats")]
    let mut qnodes = 0u64;
    #[cfg(feature = "stats")]
    let mut tt_probes = 0u64;
    #[cfg(feature = "stats")]
    let mut tt_hits = 0u64;
    #[cfg(feature = "stats")]
    let mut tt_cutoffs = 0u64;
    #[cfg(feature = "stats")]
    let mut null_cutoffs = 0u64;
    #[cfg(feature = "stats")]
    let mut probcut_hits = 0u64;
    #[cfg(feature = "stats")]
    let mut lmr_reductions = 0u64;
    #[cfg(feature = "stats")]
    let mut lmr_researches = 0u64;
    #[cfg(feature = "stats")]
    let mut beta_cutoffs = 0u64;
    #[cfg(feature = "stats")]
    let mut hashfull_sum = 0u64;
    #[cfg(feature = "stats")]
    let mut smp_stats = crate::search::SearchStats::default();
    let start = Instant::now();

    for fen in POSITIONS {
        let mut board = Board::from_fen(fen).expect("valid bench fen");
        search.clear();
        let limits = Limits {
            depth: Some(depth),
            ..Default::default()
        };
        search.think(&mut board, &limits, false);
        total_nodes += search.node_count();
        #[cfg(feature = "stats")]
        {
            let s = search.stats();
            qnodes += s.qnodes;
            tt_probes += s.tt_probes;
            tt_hits += s.tt_hits;
            tt_cutoffs += s.tt_cutoffs;
            null_cutoffs += s.null_cutoffs;
            probcut_hits += s.probcut_hits;
            lmr_reductions += s.lmr_reductions;
            lmr_researches += s.lmr_researches;
            beta_cutoffs += s.beta_cutoffs;
            hashfull_sum += search.hashfull() as u64;
            smp_stats.merge(s);
        }
    }

    let elapsed = start.elapsed().as_secs_f64();
    let nps = (total_nodes as f64 / elapsed.max(1e-9)) as u64;
    println!("simd {}", crate::eval::simd_backend());
    println!(
        "{total_nodes} nodes {nps} nps {} threads {} MB",
        threads, hash_mb
    );

    #[cfg(feature = "stats")]
    {
        let tt_hit_pct = (tt_hits * 100).checked_div(tt_probes).unwrap_or(0);
        let research_pct = (lmr_researches * 100)
            .checked_div(lmr_reductions)
            .unwrap_or(0);
        let hashfull_avg = hashfull_sum / POSITIONS.len() as u64;
        println!(
            "qnodes {qnodes} tt_hit% {tt_hit_pct} tt_cutoffs {tt_cutoffs} \
            null_cutoffs {null_cutoffs} probcut_hits {probcut_hits} \
            lmr_reductions {lmr_reductions} lmr_researches {lmr_researches} research% {research_pct} beta_cutoffs {beta_cutoffs} hashfull {hashfull_avg}"
        );
        println!(
            "smp_roots {} candidates {} disagreements {} \
            main_wins {} helper_wins {} distance_roots {} excluded {} winner_depth_gap {}",
            smp_stats.smp_roots,
            smp_stats.smp_candidates,
            smp_stats.smp_disagreements,
            smp_stats.smp_main_wins,
            smp_stats.smp_helper_wins,
            smp_stats.smp_distance_roots,
            smp_stats.smp_excluded,
            smp_stats.smp_winner_depth_gap,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_settings_are_exact_and_bounded() {
        let args = ["13", "4", "64"].map(str::to_string);
        assert_eq!(settings(&args), (13, 4, 64));
        assert_eq!(settings(&[]), (DEFAULT_DEPTH, 1, 16));
    }
}
