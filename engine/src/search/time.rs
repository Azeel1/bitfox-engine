use super::{Limits, Search};
use crate::board::Board;
use crate::types::Color;
use crate::MAX_PLY;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

impl Search {
    pub(super) fn prepare(&mut self, board: &Board, limits: &Limits) {
        self.nodes = 0;
        self.stop = false;
        self.seldepth = 0;
        self.start = Instant::now();
        self.node_limit = limits.nodes;
        self.max_depth = limits.depth.unwrap_or(MAX_PLY as i32).min(MAX_PLY as i32);
        self.killers = [[crate::types::Move::NONE; 2]; MAX_PLY + 2];
        *self.ss_eval = [crate::SCORE_NONE; MAX_PLY + 4];
        self.ss_piece = [super::PIECE_NONE; MAX_PLY + 4];

        self.timed = false;
        if let Some(mt) = limits.movetime {
            self.timed = true;
            let d = Duration::from_millis(mt.saturating_sub(5));
            self.soft = d;
            self.hard = d;
        } else if limits.wtime.is_some() || limits.btime.is_some() {
            self.timed = true;
            let (time, inc) = match board.side() {
                Color::White => (limits.wtime.unwrap_or(0), limits.winc),
                Color::Black => (limits.btime.unwrap_or(0), limits.binc),
            };
            let mtg = limits.movestogo.unwrap_or(25).max(1);
            let base = time / mtg + inc * 3 / 4;
            let soft = base;
            let hard = (base * 3).min(time.saturating_sub(10) / 2);
            self.soft = Duration::from_millis(soft.max(1));
            self.hard = Duration::from_millis(hard.max(1));
        }
        if limits.infinite {
            self.timed = false;
        }
    }

    #[inline]
    pub(super) fn check_stop(&mut self) {
        if self.is_main {
            if self.timed && self.start.elapsed() >= self.hard {
                self.stop = true;
            }
            if let Some(nl) = self.node_limit {
                if self.nodes >= nl {
                    self.stop = true;
                }
            }
            if self.stop {
                self.stop_signal.store(true, Ordering::Relaxed);
            }
        } else if self.stop_signal.load(Ordering::Relaxed) {
            self.stop = true;
        }
    }

    #[inline]
    pub(super) fn soft_expired(&self) -> bool {
        self.timed && self.start.elapsed() >= self.soft
    }
}
