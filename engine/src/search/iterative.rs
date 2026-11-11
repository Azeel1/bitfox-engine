use super::Search;
use crate::board::Board;
use crate::types::Move;
use crate::{is_decisive, mated_in, DRAW, MATE_IN_MAX};
use crate::{INFINITY, MAX_PLY};

impl Search {
    pub(super) fn run_prepared(&mut self, board: &mut Board, print: bool) -> Move {
        self.nnue.refresh(board);

        let mut best = {
            let legal = board.generate_legal();
            if legal.is_empty() {
                self.best_score = if board.in_check(board.side()) {
                    mated_in(0)
                } else {
                    DRAW
                };
                self.publish_nodes();
                return Move::NONE;
            }
            legal.moves[0]
        };
        let mut completed_pv = [Move::NONE; MAX_PLY + 1];
        completed_pv[0] = best;
        let mut completed_pv_len = 1;

        let mut avg = crate::SCORE_NONE;
        let mut stability = 0u32;
        for depth in 1..=self.max_depth {
            if self.stop_requested() {
                break;
            }
            self.seldepth = 0;
            let score = self.aspiration(board, depth, &mut avg);
            if self.stop {
                break;
            }
            self.best_score = score;
            self.completed_depth = depth;
            if self.pv_len[0] > 0 {
                if self.pv[0][0] == best {
                    stability += 1;
                } else {
                    stability = 0;
                }
                best = self.pv[0][0];
                completed_pv_len = self.pv_len[0];
                completed_pv[..completed_pv_len].copy_from_slice(&self.pv[0][..completed_pv_len]);
            }
            if print {
                self.print_info(depth, score);
            }
            if self.is_main && self.soft_expired(stability) {
                break;
            }
            if !self.infinite && score >= MATE_IN_MAX && depth >= 4 {
                break;
            }
        }
        self.pv[0][..completed_pv_len].copy_from_slice(&completed_pv[..completed_pv_len]);
        self.pv_len[0] = completed_pv_len;
        self.publish_nodes();
        best
    }

    pub(super) fn aspiration(&mut self, board: &mut Board, depth: i32, avg: &mut i32) -> i32 {
        if depth < 4 {
            let score = self.negamax(board, -INFINITY, INFINITY, depth, 0);
            *avg = score;
            return score;
        }
        let mut delta = 20 + (*avg).abs() / 16;
        let mut alpha = (*avg - delta).max(-INFINITY);
        let mut beta = (*avg + delta).min(INFINITY);
        let mut reduction = 0;
        loop {
            let score = self.negamax(board, alpha, beta, (depth - reduction).max(1), 0);
            if self.stop {
                return score;
            }
            if score <= alpha {
                // fail low: widen down and go back to full depth
                alpha = (score - delta).max(-INFINITY);
                reduction = 0;
                delta += delta / 3;
            } else if score >= beta {
                // fail high: widen up, and drop a ply unless the score is decisive
                beta = (score + delta).min(INFINITY);
                reduction = if is_decisive(score) {
                    reduction.min(1)
                } else {
                    reduction + 1
                };
                delta += delta / 2;
            } else {
                *avg = (*avg + score) / 2;
                return score;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::Limits;

    #[test]
    fn aspiration_recovers_from_failures_in_both_directions() {
        for initial_avg in [-2000, 2000] {
            let mut board = Board::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
            let mut search = Search::new(1);
            search.prepare(&board, &Limits::default());
            search.nnue.refresh(&board);
            let mut avg = initial_avg;

            assert_eq!(search.aspiration(&mut board, 4, &mut avg), DRAW);
            assert_eq!(avg, initial_avg / 2);
            assert!(!search.stop);
        }
    }

    #[test]
    fn stopped_aspiration_preserves_the_completed_average() {
        let mut board = Board::startpos();
        let mut search = Search::new(1);
        search.prepare(&board, &Limits::default());
        search.nnue.refresh(&board);
        search.stop = true;
        let mut avg = 2000;

        search.aspiration(&mut board, 4, &mut avg);

        assert_eq!(avg, 2000);
    }
}
