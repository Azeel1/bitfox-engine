use super::Search;
use crate::board::Board;
use crate::movegen::MoveList;
use crate::tt::Bound;
use crate::types::Move;
use crate::{is_loss, mated_in, DRAW, INFINITY, MATE_IN_MAX, MAX_PLY};

impl Search {
    pub(super) fn qsearch(&mut self, board: &mut Board, mut alpha: i32, beta: i32, ply: usize) -> i32 {
        if self.stop {
            return 0;
        }
        self.nodes += 1;
        if self.nodes % 2048 == 0 {
            self.check_stop();
        }
        if ply > self.seldepth {
            self.seldepth = ply;
        }
        if board.is_draw() {
            return DRAW;
        }
        if ply >= MAX_PLY {
            return self.nnue.eval(board);
        }

        let us = board.side();
        let in_check = board.in_check(us);

        let key = board.key();
        let probe = self.tt.probe(key, ply);

        if let Some(p) = &probe {
            let cut = match p.bound {
                Bound::Lower => p.score >= beta,
                Bound::Upper => p.score <= alpha,
                Bound::Exact => true,
                Bound::None => false,
            };
            if cut {
                return p.score;
            }
        }

        let raw_eval = if in_check {
            0
        } else {
            probe.as_ref().map(|p| p.eval).unwrap_or_else(|| self.nnue.eval(board))
        };
        let eval = if in_check {
            -INFINITY
        } else {
            (raw_eval + self.corrhist.value(board)).clamp(-(MATE_IN_MAX - 1), MATE_IN_MAX - 1)
        };

        let mut best = eval;
        if !in_check {
            if let Some(p) = &probe {
                let refine = match p.bound {
                    Bound::Lower => p.score > best,
                    Bound::Upper => p.score < best,
                    Bound::Exact => true,
                    Bound::None => false,
                };
                if refine {
                    best = p.score;
                }
            }
            if best >= beta {
                return best;
            }
            if best > alpha {
                alpha = best;
            }
        }

        let tc = board.threat_ctx();

        let mut list = MoveList::new();
        if in_check {
            board.generate_pseudo(&mut list);
        } else {
            board.generate_captures(&mut list);
        }
        let mut scores = [0i32; 256];
        for (i, &m) in list.as_slice().iter().enumerate() {
            scores[i] = self.score_move(board, m, Move::NONE, ply, &tc);
        }

        let mut best_move = Move::NONE;
        let mut legal = 0;
        for i in 0..list.len() {
            let mut best_idx = i;
            for j in (i + 1)..list.len() {
                if scores[j] > scores[best_idx] {
                    best_idx = j;
                }
            }
            list.moves.swap(i, best_idx);
            scores.swap(i, best_idx);
            let m = list.moves[i];

            if !is_loss(best) && !in_check && !board.see(m, (alpha - eval) / 8 - 74) {
                continue;
            }

            let (moved, captured) = super::move_info(board, m);
            board.make_move(m);
            if board.is_attacked(board.king_sq(us), board.side()) {
                board.unmake_move(m);
                continue;
            }
            legal += 1;
            self.nnue.make(board, m, moved, captured);
            let score = -self.qsearch(board, -beta, -alpha, ply + 1);
            board.unmake_move(m);
            self.nnue.unmake();

            if self.stop {
                return 0;
            }
            if score > best {
                best = score;
                if score > alpha {
                    best_move = m;
                    alpha = score;
                    if score >= beta {
                        break;
                    }
                }
            }
        }

        if in_check && legal == 0 {
            return mated_in(ply);
        }

        let bound = if best >= beta { Bound::Lower } else { Bound::Upper };
        let stored_eval = if in_check { 0 } else { raw_eval };
        self.tt.store(key, best_move, best, stored_eval, 0, bound, ply);

        best
    }
}
