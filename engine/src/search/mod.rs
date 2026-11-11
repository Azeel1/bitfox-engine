mod corrhist;
mod history;
mod iterative;
mod limits;
mod negamax;
mod ordering;
mod params;
mod pv;
mod quiescence;
mod smp;
mod syzygy;
mod time;

pub use limits::Limits;
pub use params::{Params, UCI_OPTIONS};

use crate::tt::Tt;
use crate::types::Move;
use crate::MAX_PLY;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const PIECE_NONE: usize = 12;

#[derive(Default, Clone, Copy)]
pub struct SearchStats {
    pub qnodes: u64,
    pub tt_probes: u64,
    pub tt_hits: u64,
    pub tt_cutoffs: u64,
    pub null_cutoffs: u64,
    pub probcut_hits: u64,
    pub lmr_reductions: u64,
    pub lmr_researches: u64,
    pub beta_cutoffs: u64,
    #[cfg(feature = "stats")]
    pub smp_roots: u64,
    #[cfg(feature = "stats")]
    pub smp_candidates: u64,
    #[cfg(feature = "stats")]
    pub smp_disagreements: u64,
    #[cfg(feature = "stats")]
    pub smp_main_wins: u64,
    #[cfg(feature = "stats")]
    pub smp_helper_wins: u64,
    #[cfg(feature = "stats")]
    pub smp_distance_roots: u64,
    #[cfg(feature = "stats")]
    pub smp_excluded: u64,
    #[cfg(feature = "stats")]
    pub smp_winner_depth_gap: u64,
}

#[cfg(feature = "stats")]
impl SearchStats {
    pub(crate) fn merge(&mut self, other: &SearchStats) {
        self.qnodes += other.qnodes;
        self.tt_probes += other.tt_probes;
        self.tt_hits += other.tt_hits;
        self.tt_cutoffs += other.tt_cutoffs;
        self.null_cutoffs += other.null_cutoffs;
        self.probcut_hits += other.probcut_hits;
        self.lmr_reductions += other.lmr_reductions;
        self.lmr_researches += other.lmr_researches;
        self.beta_cutoffs += other.beta_cutoffs;
        self.smp_roots += other.smp_roots;
        self.smp_candidates += other.smp_candidates;
        self.smp_disagreements += other.smp_disagreements;
        self.smp_main_wins += other.smp_main_wins;
        self.smp_helper_wins += other.smp_helper_wins;
        self.smp_distance_roots += other.smp_distance_roots;
        self.smp_excluded += other.smp_excluded;
        self.smp_winner_depth_gap += other.smp_winner_depth_gap;
    }
}

macro_rules! stat {
    ($self:expr, $field:ident) => {
        #[cfg(feature = "stats")]
        {
            $self.stats.$field += 1;
        }
    };
}
pub(crate) use stat;

// indexed by [piece][to], threat state and capture target
pub type ContHist = [[[[i16; 64]; 13]; 64]; 13];

pub type QuietHist = [[[[[i32; 64]; 64]; 2]; 2]; 2];

pub type NoisyHist = [[[i16; 6]; 64]; 12];

pub type ContCorr = [[[[i16; 64]; 13]; 64]; 13];

const HIST_MAX: i32 = 16384;
const NOISY_MAX: i32 = 16384;
const CONTCORR_MAX: i32 = 16384;

pub struct Search {
    tt: Arc<Tt>,
    stop_signal: Arc<AtomicBool>,
    shared_nodes: Arc<AtomicU64>,
    published: u64,
    is_main: bool,
    threads: usize,
    tb: Option<Arc<syzygy::Syzygy>>,
    tb_hits: u64,
    nnue: crate::eval::Nnue,
    killers: [[Move; 2]; MAX_PLY + 2],
    excluded: [Move; MAX_PLY + 4],
    history: Box<QuietHist>,
    noisy_hist: Box<NoisyHist>,
    countermove: Box<[[Move; 64]; 13]>,
    cont_hist: Box<ContHist>,
    cont_corr: Box<ContCorr>,
    corrhist: corrhist::CorrHist,
    ss_eval: Box<[i32; MAX_PLY + 4]>,
    ss_piece: [usize; MAX_PLY + 4],
    ss_to: [usize; MAX_PLY + 4],
    cutoff_cnt: [i32; MAX_PLY + 4],
    stats: SearchStats,
    params: Params,
    pv: Box<[[Move; MAX_PLY + 1]; MAX_PLY + 1]>,
    pv_len: [usize; MAX_PLY + 1],
    lmr: [[i32; 64]; 64],
    lmr_noisy: [[i32; 64]; 64],
    nodes: u64,
    seldepth: usize,
    start: Instant,
    soft: Duration,
    hard: Duration,
    timed: bool,
    fixed_time: bool,
    infinite: bool,
    node_limit: Option<u64>,
    check_interval: u64,
    max_depth: i32,
    stop: bool,
    best_score: i32,
    completed_depth: i32,
    helpers: Vec<Box<Search>>,
    helper_pool: smp::HelperPool,
}

impl Search {
    pub fn new(tt_mb: usize) -> Search {
        crate::movegen::warmup();
        let params = Params::default();
        Search::build(
            Arc::new(Tt::new(tt_mb)),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
            true,
            params,
        )
    }

    fn worker(
        tt: Arc<Tt>,
        stop_signal: Arc<AtomicBool>,
        shared_nodes: Arc<AtomicU64>,
        params: Params,
    ) -> Search {
        Search::build(tt, stop_signal, shared_nodes, false, params)
    }

    fn build(
        tt: Arc<Tt>,
        stop_signal: Arc<AtomicBool>,
        shared_nodes: Arc<AtomicU64>,
        is_main: bool,
        params: Params,
    ) -> Search {
        let lmr = build_lmr(&params);
        let lmr_noisy = build_lmr_noisy(&params);
        Search {
            tt,
            stop_signal,
            shared_nodes,
            published: 0,
            is_main,
            threads: 1,
            tb: None,
            tb_hits: 0,
            nnue: crate::eval::Nnue::default(),
            killers: [[Move::NONE; 2]; MAX_PLY + 2],
            excluded: [Move::NONE; MAX_PLY + 4],
            history: alloc_zeroed_box(),
            noisy_hist: alloc_zeroed_box(),
            countermove: alloc_zeroed_box(),
            cont_hist: alloc_zeroed_box(),
            cont_corr: alloc_zeroed_box(),
            corrhist: corrhist::CorrHist::default(),
            ss_eval: Box::new([crate::SCORE_NONE; MAX_PLY + 4]),
            ss_piece: [PIECE_NONE; MAX_PLY + 4],
            ss_to: [0; MAX_PLY + 4],
            cutoff_cnt: [0; MAX_PLY + 4],
            stats: SearchStats::default(),
            params,
            pv: Box::new([[Move::NONE; MAX_PLY + 1]; MAX_PLY + 1]),
            pv_len: [0; MAX_PLY + 1],
            lmr,
            lmr_noisy,
            nodes: 0,
            seldepth: 0,
            start: Instant::now(),
            soft: Duration::ZERO,
            hard: Duration::ZERO,
            timed: false,
            fixed_time: false,
            infinite: false,
            node_limit: None,
            check_interval: 2048,
            max_depth: MAX_PLY as i32,
            stop: false,
            best_score: 0,
            completed_depth: 0,
            helpers: Vec::new(),
            helper_pool: smp::HelperPool::default(),
        }
    }

    pub fn clear(&mut self) {
        self.tt.clear();
        self.clear_heuristics();
        for helper in &mut self.helpers {
            helper.clear_heuristics();
        }
    }

    fn clear_heuristics(&mut self) {
        self.killers = [[Move::NONE; 2]; MAX_PLY + 2];
        self.history = alloc_zeroed_box();
        self.noisy_hist = alloc_zeroed_box();
        self.countermove = alloc_zeroed_box();
        self.cont_hist = alloc_zeroed_box();
        self.cont_corr = alloc_zeroed_box();
        self.corrhist.clear();
    }

    pub fn resize_tt(&mut self, mb: usize) {
        self.helpers.clear();
        Arc::get_mut(&mut self.tt)
            .expect("search helpers must be idle before resizing the TT")
            .resize(mb);
        self.configure_helpers();
    }

    pub fn set_threads(&mut self, n: usize) {
        self.threads = n.clamp(1, 256);
        self.configure_helpers();
    }

    pub fn set_param(&mut self, name: &str, value: i32) -> bool {
        if !self.params.set(name, value) {
            return false;
        }
        self.lmr = build_lmr(&self.params);
        self.lmr_noisy = build_lmr_noisy(&self.params);
        for helper in &mut self.helpers {
            helper.params = self.params;
            helper.lmr = self.lmr;
            helper.lmr_noisy = self.lmr_noisy;
        }
        true
    }

    pub fn set_tablebase(&mut self, path: &str) -> bool {
        if path.is_empty() || path == "<empty>" {
            self.tb = None;
            for helper in &mut self.helpers {
                helper.tb = None;
            }
            return true;
        }
        self.tb = syzygy::Syzygy::load(path);
        for helper in &mut self.helpers {
            helper.tb = self.tb.clone();
        }
        self.tb.is_some()
    }

    pub fn stop_signal(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop_signal)
    }

    #[inline]
    pub fn best_score(&self) -> i32 {
        self.best_score
    }

    #[inline]
    pub fn completed_depth(&self) -> i32 {
        self.completed_depth
    }

    #[inline]
    pub fn seldepth(&self) -> usize {
        self.seldepth
    }

    #[inline]
    pub fn node_count(&self) -> u64 {
        self.shared_nodes.load(Ordering::Relaxed) + self.nodes - self.published
    }

    #[inline]
    pub fn root_pv(&self) -> &[Move] {
        &self.pv[0][..self.pv_len[0]]
    }

    #[inline]
    pub fn stats(&self) -> &SearchStats {
        &self.stats
    }

    #[inline]
    pub fn hashfull(&self) -> u32 {
        self.tt.hashfull()
    }
}

fn build_lmr(params: &Params) -> [[i32; 64]; 64] {
    lmr_table(params.lmr_base, params.lmr_div)
}

fn build_lmr_noisy(params: &Params) -> [[i32; 64]; 64] {
    lmr_table(params.lmr_noisy_base, params.lmr_noisy_div)
}

fn lmr_table(base_param: i32, div_param: i32) -> [[i32; 64]; 64] {
    let base = base_param as f32 / 100.0;
    let div = div_param as f32 / 100.0;
    let mut lmr = [[0i32; 64]; 64];
    for (depth, row) in lmr.iter_mut().enumerate().skip(1) {
        for (played, r) in row.iter_mut().enumerate().skip(1) {
            *r = (base + (depth as f32).ln() * (played as f32).ln() / div) as i32;
        }
    }
    lmr
}

pub(super) fn move_info(
    board: &crate::board::Board,
    m: Move,
) -> (crate::types::Piece, Option<(crate::types::Piece, usize)>) {
    use crate::types::{Piece, PieceType};
    let moved = board.piece_on(m.from()).unwrap();
    let captured = if m.is_en_passant() {
        Some((
            Piece::new(board.side().flip(), PieceType::Pawn),
            m.to().index() ^ 8,
        ))
    } else if m.is_capture() {
        board.piece_on(m.to()).map(|p| (p, m.to().index()))
    } else {
        None
    };
    (moved, captured)
}

// the history tables are several MB and a helper thread's stack can't hold
// them, so they're zeroed straight on the heap
pub(super) fn alloc_zeroed_box<T>() -> Box<T> {
    let layout = std::alloc::Layout::new::<T>();
    assert!(layout.size() > 0);
    unsafe {
        let raw = std::alloc::alloc_zeroed(layout).cast::<T>();
        let Some(ptr) = std::ptr::NonNull::new(raw) else {
            std::alloc::handle_alloc_error(layout);
        };
        Box::from_raw(ptr.as_ptr())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;

    #[test]
    fn helper_threads_return_a_legal_move_and_join() {
        for threads in [1usize, 2, 4] {
            let mut board = Board::from_fen(
                "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            )
            .expect("valid test position");
            let legal = board.generate_legal();
            let mut search = Search::new(8);
            search.set_threads(threads);
            let limits = Limits {
                depth: Some(7),
                ..Default::default()
            };
            let best = search.think(&mut board, &limits, false);
            assert!(!best.is_none(), "threads={threads} produced no move");
            let legal_moves = (0..legal.len()).map(|i| legal.moves[i]);
            assert!(
                legal_moves.into_iter().any(|m| m == best),
                "threads={threads} produced {}, which is not legal here",
                best.to_uci()
            );
        }
    }

    #[test]
    fn a_node_limit_counts_every_thread() {
        for threads in [1usize, 2, 4] {
            let mut board = Board::startpos();
            let mut search = Search::new(8);
            search.set_threads(threads);
            let limits = Limits {
                nodes: Some(40_000),
                ..Default::default()
            };
            search.think(&mut board, &limits, false);
            let searched = search.node_count();
            assert_eq!(searched, search.shared_nodes.load(Ordering::Relaxed));
            assert!(
                searched <= 60_000,
                "threads={threads} searched {searched} against a 40000 node limit"
            );
        }
    }

    #[test]
    fn fixed_node_search_is_independent_of_thread_setting() {
        let mut reference = None;
        for threads in [1usize, 4] {
            let mut board = Board::startpos();
            let mut search = Search::new(8);
            search.set_threads(threads);
            let best = search.think(
                &mut board,
                &Limits {
                    nodes: Some(40_000),
                    ..Default::default()
                },
                false,
            );
            let result = (
                best,
                search.best_score(),
                search.completed_depth(),
                search.node_count(),
            );
            if let Some(expected) = reference {
                assert_eq!(result, expected);
            } else {
                reference = Some(result);
            }
        }
    }

    #[test]
    fn a_tiny_node_limit_is_checked_before_a_full_batch() {
        for threads in [1usize, 2, 4] {
            let mut board = Board::startpos();
            let mut search = Search::new(8);
            search.set_threads(threads);
            let limits = Limits {
                nodes: Some(1),
                ..Default::default()
            };

            search.think(&mut board, &limits, false);

            let searched = search.node_count();
            assert!(
                searched <= 64,
                "threads={threads} searched {searched} against a one-node limit"
            );
            assert_eq!(searched, search.shared_nodes.load(Ordering::Relaxed));
        }
    }

    #[test]
    fn a_stopped_search_does_not_reuse_an_old_root_pv() {
        let mut search = Search::new(8);
        let mut first = Board::startpos();
        let full = Limits {
            depth: Some(4),
            ..Default::default()
        };
        search.think(&mut first, &full, false);
        assert!(search.pv_len[0] > 0);

        let mut second = Board::from_fen("8/8/8/8/8/8/1K6/7k w - - 0 1").unwrap();
        let legal = second.generate_legal();
        let short = Limits {
            nodes: Some(1),
            ..Default::default()
        };
        let best = search.think(&mut second, &short, false);

        assert!((0..legal.len()).any(|index| legal.moves[index] == best));
        assert_eq!(search.completed_depth(), 0);
        assert_eq!(search.best_score(), 0);
        assert_eq!(search.root_pv().first(), Some(&best));
    }

    #[test]
    fn helper_threads_stop_when_the_main_search_does() {
        let mut board = Board::startpos();
        let mut search = Search::new(8);
        search.set_threads(4);
        let limits = Limits {
            movetime: Some(200),
            ..Default::default()
        };
        let start = Instant::now();
        let best = search.think(&mut board, &limits, false);
        let elapsed = start.elapsed();
        assert!(!best.is_none());
        assert!(
            elapsed < Duration::from_secs(5),
            "a 200ms search with four threads took {elapsed:?} to join"
        );
    }

    #[test]
    fn helper_threads_release_shared_state_before_clear() {
        let mut search = Search::new(8);
        search.set_threads(4);

        for mb in [16usize, 8, 16, 8] {
            let mut board = Board::startpos();
            let limits = Limits {
                depth: Some(4),
                ..Default::default()
            };
            let best = search.think(&mut board, &limits, false);
            assert!(!best.is_none());
            search.clear();
            assert_eq!(search.hashfull(), 0);
            search.resize_tt(mb);
        }
    }

    #[test]
    fn helper_search_state_is_reused_between_moves() {
        let mut search = Search::new(8);
        search.set_threads(4);
        let addresses = |search: &Search| {
            search
                .helpers
                .iter()
                .map(|helper| (&**helper as *const Search) as usize)
                .collect::<Vec<_>>()
        };
        let initial = addresses(&search);

        for fen in [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2",
        ] {
            let mut board = Board::from_fen(fen).unwrap();
            search.think(
                &mut board,
                &Limits {
                    depth: Some(5),
                    ..Default::default()
                },
                false,
            );
            assert_eq!(addresses(&search), initial);
        }
    }

    #[test]
    fn a_deeper_completed_helper_result_can_be_adopted() {
        let mut search = Search::new(1);
        let mut helper = Search::worker(
            Arc::clone(&search.tt),
            Arc::clone(&search.stop_signal),
            Arc::clone(&search.shared_nodes),
            search.params,
        );
        let mut board = Board::startpos();
        let best = board.generate_legal().moves[0];
        helper.pv[0][0] = best;
        helper.pv_len[0] = 1;
        helper.best_score = 37;
        helper.completed_depth = 9;
        helper.seldepth = 14;

        search.adopt_helper_result(&helper);

        assert_eq!(search.root_pv(), [best]);
        assert_eq!(search.best_score(), 37);
        assert_eq!(search.completed_depth(), 9);
        assert_eq!(search.seldepth(), 14);
    }

    #[test]
    fn helper_threads_inherit_search_params() {
        let mut search = Search::new(8);
        assert!(search.set_param("RazorBase", 417));
        assert!(search.set_param("LmrBase", 104));

        let helper = Search::worker(
            Arc::clone(&search.tt),
            Arc::clone(&search.stop_signal),
            Arc::clone(&search.shared_nodes),
            search.params,
        );

        assert_eq!(helper.params.razor_base, 417);
        assert_eq!(helper.params.lmr_base, 104);
        assert_eq!(helper.lmr, build_lmr(&search.params));
    }

    #[test]
    fn existing_helpers_receive_noisy_lmr_parameter_changes() {
        let mut search = Search::new(1);
        search.set_threads(2);
        let original = search.lmr_noisy;

        assert!(search.set_param("LmrNoisyBase", 140));
        assert!(search.set_param("LmrNoisyDiv", 200));

        assert_ne!(search.lmr_noisy, original);
        for helper in &search.helpers {
            assert_eq!(helper.params.lmr_noisy_base, 140);
            assert_eq!(helper.params.lmr_noisy_div, 200);
            assert_eq!(helper.lmr_noisy, search.lmr_noisy);
        }
    }

    #[test]
    fn thread_count_is_bounded_for_library_callers() {
        let mut search = Search::new(1);
        search.set_threads(0);
        assert_eq!(search.threads, 1);
        search.set_threads(usize::MAX);
        assert_eq!(search.threads, 256);
    }

    #[test]
    fn dispatch_delay_counts_against_the_time_limit() {
        let board = Board::startpos();
        let mut search = Search::new(8);
        let started_at = Instant::now() - Duration::from_millis(100);
        let limits = Limits {
            movetime: Some(50),
            started_at: Some(started_at),
            ..Default::default()
        };

        search.prepare(&board, &limits);

        assert_eq!(search.start, started_at);
        assert!(search.stop);
        assert!(search.stop_signal.load(Ordering::Relaxed));
    }

    #[test]
    fn an_expired_multithread_job_does_not_start_helpers() {
        let mut board = Board::startpos();
        let legal = board.generate_legal();
        let mut search = Search::new(8);
        search.set_threads(4);
        let limits = Limits {
            movetime: Some(50),
            started_at: Some(Instant::now() - Duration::from_millis(100)),
            ..Default::default()
        };

        let best = search.think_started(&mut board, &limits, false);

        assert!((0..legal.len()).any(|index| legal.moves[index] == best));
        assert_eq!(search.node_count(), 0);
    }

    #[test]
    fn helpers_obey_the_same_hard_deadline() {
        let search = Search::new(8);
        let mut helper = Search::worker(
            Arc::clone(&search.tt),
            Arc::clone(&search.stop_signal),
            Arc::clone(&search.shared_nodes),
            search.params,
        );
        let limits = Limits {
            movetime: Some(50),
            started_at: Some(Instant::now() - Duration::from_millis(100)),
            ..Default::default()
        };

        helper.prepare(&Board::startpos(), &limits);

        assert!(helper.stop);
        assert!(search.stop_signal.load(Ordering::Relaxed));
    }

    #[test]
    fn stop_requested_before_search_entry_is_not_cleared() {
        let mut board = Board::startpos();
        let mut search = Search::new(8);
        search.reset_stop();
        search.stop_signal.store(true, Ordering::Relaxed);
        let limits = Limits {
            depth: Some(1),
            ..Default::default()
        };

        search.think_started(&mut board, &limits, false);

        assert!(search.stop_signal.load(Ordering::Relaxed));
    }

    #[test]
    fn an_externally_stopped_search_can_be_cleared_resized_and_reused() {
        let mut search = Search::new(8);
        search.set_threads(4);
        let stop = search.stop_signal();
        let mut board = Board::startpos();

        std::thread::scope(|scope| {
            let handle = scope.spawn(|| {
                search.think(
                    &mut board,
                    &Limits {
                        infinite: true,
                        ..Default::default()
                    },
                    false,
                )
            });
            std::thread::sleep(Duration::from_millis(20));
            stop.store(true, Ordering::Relaxed);
            assert!(!handle.join().expect("search thread panicked").is_none());
        });

        search.clear();
        search.resize_tt(16);
        search.set_threads(2);
        let best = search.think(
            &mut board,
            &Limits {
                depth: Some(3),
                ..Default::default()
            },
            false,
        );
        assert!(!best.is_none());
        assert_eq!(search.threads, 2);
    }
}
