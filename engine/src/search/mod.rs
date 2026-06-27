mod corrhist;
mod history;
mod iterative;
mod limits;
mod negamax;
mod ordering;
mod pv;
mod quiescence;
mod time;

pub use limits::Limits;

use crate::tt::Tt;
use crate::types::Move;
use crate::MAX_PLY;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const PIECE_NONE: usize = 12;

// [prev_piece][prev_to][piece][to]
pub type ContHist = [[[[i16; 64]; 13]; 64]; 13];

// [color][from_threatened][to_threatened][from][to]
pub type QuietHist = [[[[[i32; 64]; 64]; 2]; 2]; 2];

// [moved_piece][to][captured_type]
pub type NoisyHist = [[[i16; 6]; 64]; 12];

const HIST_MAX: i32 = 16384;
const NOISY_MAX: i32 = 12800;

pub struct Search {
    tt: Arc<Tt>,
    stop_signal: Arc<AtomicBool>,
    is_main: bool,
    threads: usize,
    nnue: crate::eval::Nnue,
    killers: [[Move; 2]; MAX_PLY + 2],
    excluded: [Move; MAX_PLY + 4],
    history: Box<QuietHist>,
    noisy_hist: Box<NoisyHist>,
    cont_hist: Box<ContHist>,
    corrhist: corrhist::CorrHist,
    ss_eval: Box<[i32; MAX_PLY + 4]>,
    ss_piece: [usize; MAX_PLY + 4],
    ss_to: [usize; MAX_PLY + 4],
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
        let mut lmr = [[0i32; 64]; 64];
        for (depth, row) in lmr.iter_mut().enumerate().skip(1) {
            for (played, r) in row.iter_mut().enumerate().skip(1) {
                *r = (0.85 + (depth as f32).ln() * (played as f32).ln() / 2.15) as i32;
            }
        }
        Search {
            tt,
            stop_signal,
            is_main,
            threads: 1,
            nnue: crate::eval::Nnue::default(),
            killers: [[Move::NONE; 2]; MAX_PLY + 2],
            excluded: [Move::NONE; MAX_PLY + 4],
            history: zeroed_box(),
            noisy_hist: zeroed_box(),
            cont_hist: zeroed_box(),
            corrhist: corrhist::CorrHist::default(),
            ss_eval: Box::new([crate::SCORE_NONE; MAX_PLY + 4]),
            ss_piece: [PIECE_NONE; MAX_PLY + 4],
            ss_to: [0; MAX_PLY + 4],
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
        self.cont_hist = zeroed_box();
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
