use super::{Limits, Search};
use crate::board::Board;
use crate::is_decisive;
use crate::types::Move;
use crate::INFINITY;
use std::sync::atomic::Ordering;
use std::sync::Arc;

impl Search {
    pub fn think(&mut self, board: &mut Board, limits: &Limits, print: bool) -> Move {
        self.stop_signal.store(false, Ordering::Relaxed);
        self.tt.new_search();

        if self.threads <= 1 {
            return self.run(board, limits, print);
        }

        std::thread::scope(|scope| {
            for _ in 1..self.threads {
                let tt = Arc::clone(&self.tt);
                let stop = Arc::clone(&self.stop_signal);
                let tb = self.tb.clone();
                let mut b = board.clone();
                let lim = limits.clone();
                scope.spawn(move || {
                    let mut helper = Search::worker(tt, stop);
                    helper.tb = tb;
                    helper.run(&mut b, &lim, false);
                });
            }
            let best = self.run(board, limits, print);
            self.stop_signal.store(true, Ordering::Relaxed);
            best
        })
    }

    fn run(&mut self, board: &mut Board, limits: &Limits, print: bool) -> Move {
        self.prepare(board, limits);
        self.nnue.refresh(board);

        let mut best = {
            let legal = board.generate_legal();
            if legal.is_empty() {
                return Move::NONE;
            }
            legal.moves[0]
        };

        let mut avg = crate::SCORE_NONE;
        let mut stability = 0u32;
        for depth in 1..=self.max_depth {
            self.seldepth = 0;
            let score = self.aspiration(board, depth, &mut avg);
            if self.stop && depth > 1 {
                break;
            }
            self.best_score = score;
            self.completed_depth = depth;
            if !self.pv[0][0].is_none() {
                if self.pv[0][0] == best {
                    stability += 1;
                } else {
                    stability = 0;
                }
                best = self.pv[0][0];
            }
            if print {
                self.print_info(depth, score);
            }
            if self.is_main && self.soft_expired(stability) {
                break;
            }
            if is_decisive(score) && depth >= 4 {
                break;
            }
        }
        best
    }

    pub(super) fn aspiration(&mut self, board: &mut Board, depth: i32, avg: &mut i32) -> i32 {
        if depth < 4 {
            let score = self.negamax(board, -INFINITY, INFINITY, depth, 0);
            *avg = score;
            return score;
        }
        let mut delta = 23 + *avg * *avg / 26394;
        let mut alpha = (*avg - delta).max(-INFINITY);
        let mut beta = (*avg + delta).min(INFINITY);
        let mut reduction = 0;
        loop {
            let score = self.negamax(board, alpha, beta, (depth - reduction).max(1), 0);
            if self.stop {
                return score;
            }
            if score <= alpha {
                alpha = (score - delta).max(-INFINITY);
                beta = (alpha + delta).min(beta);
                delta += 26 * delta / 128;
            } else if score >= beta {
                alpha = (beta - delta).max(alpha);
                beta = (score + delta).min(INFINITY);
                if !is_decisive(score) {
                    reduction += 1;
                } else {
                    reduction = reduction.min(1);
                }
                delta += 60 * delta / 128;
            } else {
                *avg = (*avg + score) / 2;
                return score;
            }
        }
    }
}
