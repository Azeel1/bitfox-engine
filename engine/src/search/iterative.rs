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
                let mut b = board.clone();
                let lim = limits.clone();
                scope.spawn(move || {
                    let mut helper = Search::worker(tt, stop);
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

        let mut prev = 0;
        for depth in 1..=self.max_depth {
            self.seldepth = 0;
            let score = self.aspiration(board, depth, prev);
            if self.stop && depth > 1 {
                break;
            }
            prev = score;
            self.best_score = score;
            self.completed_depth = depth;
            if !self.pv[0][0].is_none() {
                best = self.pv[0][0];
            }
            if print {
                self.print_info(depth, score);
            }
            if self.is_main && self.soft_expired() {
                break;
            }
            if is_decisive(score) && depth >= 4 {
                break;
            }
        }
        best
    }

    pub(super) fn aspiration(&mut self, board: &mut Board, depth: i32, prev: i32) -> i32 {
        if depth < 4 {
            return self.negamax(board, -INFINITY, INFINITY, depth, 0);
        }
        let mut delta = 25;
        let mut alpha = (prev - delta).max(-INFINITY);
        let mut beta = (prev + delta).min(INFINITY);
        loop {
            let score = self.negamax(board, alpha, beta, depth, 0);
            if self.stop {
                return score;
            }
            if score <= alpha {
                beta = (alpha + beta) / 2;
                alpha = (score - delta).max(-INFINITY);
            } else if score >= beta {
                beta = (score + delta).min(INFINITY);
            } else {
                return score;
            }
            delta += delta / 2;
        }
    }
}
