mod corrhist;
mod history;
mod iterative;
mod limits;
mod negamax;
mod ordering;
mod params;
mod pv;
mod quiescence;
mod syzygy;
mod time;

pub use limits::Limits;
pub use params::{Params, UCI_OPTIONS};

use crate::tt::Tt;
use crate::types::Move;
use crate::MAX_PLY;
use std::sync::atomic::AtomicBool;
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

pub type ContHist = [[[[i16; 64]; 13]; 64]; 13];

pub type QuietHist = [[[[[i32; 64]; 64]; 2]; 2]; 2];

pub type NoisyHist = [[[i16; 6]; 64]; 12];

pub type ContCorr = [[[[i16; 64]; 13]; 64]; 13];

const HIST_MAX: i32 = 16384;
const NOISY_MAX: i32 = 12800;
const CONTCORR_MAX: i32 = 16418;

pub struct Search {
    tt: Arc<Tt>,
    stop_signal: Arc<AtomicBool>,
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
    nodes: u64,
    seldepth: usize,
    start: Instant,
    soft: Duration,
    hard: Duration,
    timed: bool,
    node_limit: Option<u64>,
    max_depth: i32,
    stop: bool,
    best_score: i32,
    completed_depth: i32,
}

impl Search {
    pub fn new(tt_mb: usize) -> Search {
        Search::build(
            Arc::new(Tt::new(tt_mb)),
            Arc::new(AtomicBool::new(false)),
            true,
        )
    }

    fn worker(tt: Arc<Tt>, stop_signal: Arc<AtomicBool>) -> Search {
        Search::build(tt, stop_signal, false)
    }

    fn build(tt: Arc<Tt>, stop_signal: Arc<AtomicBool>, is_main: bool) -> Search {
        let params = Params::default();
        let lmr = build_lmr(&params);
        Search {
            tt,
            stop_signal,
            is_main,
            threads: 1,
            tb: None,
            tb_hits: 0,
            nnue: crate::eval::Nnue::default(),
            killers: [[Move::NONE; 2]; MAX_PLY + 2],
            excluded: [Move::NONE; MAX_PLY + 4],
            history: zeroed_box(),
            noisy_hist: zeroed_box(),
            countermove: zeroed_box(),
            cont_hist: zeroed_box(),
            cont_corr: zeroed_box(),
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
            nodes: 0,
            seldepth: 0,
            start: Instant::now(),
            soft: Duration::ZERO,
            hard: Duration::ZERO,
            timed: false,
            node_limit: None,
            max_depth: MAX_PLY as i32,
            stop: false,
            best_score: 0,
            completed_depth: 0,
        }
    }

    pub fn clear(&mut self) {
        self.tt.clear();
        self.killers = [[Move::NONE; 2]; MAX_PLY + 2];
        self.history = zeroed_box();
        self.noisy_hist = zeroed_box();
        self.countermove = zeroed_box();
        self.cont_hist = zeroed_box();
        self.cont_corr = zeroed_box();
        self.corrhist.clear();
    }

    pub fn resize_tt(&mut self, mb: usize) {
        if let Some(tt) = Arc::get_mut(&mut self.tt) {
            tt.resize(mb);
        }
    }

    pub fn set_threads(&mut self, n: usize) {
        self.threads = n.max(1);
    }

    pub fn set_param(&mut self, name: &str, value: i32) -> bool {
        if !self.params.set(name, value) {
            return false;
        }
        self.lmr = build_lmr(&self.params);
        true
    }

    pub fn set_tablebase(&mut self, path: &str) -> bool {
        self.tb = syzygy::Syzygy::load(path);
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
        self.nodes
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
    let base = params.lmr_base as f32 / 100.0;
    let div = params.lmr_div as f32 / 100.0;
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

fn zeroed_box<T>() -> Box<T> {
    unsafe {
        let layout = std::alloc::Layout::new::<T>();
        let ptr = std::alloc::alloc_zeroed(layout);
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        Box::from_raw(ptr.cast())
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
}
