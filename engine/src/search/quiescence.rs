use super::{stat, Search};
use crate::board::Board;
use crate::movegen::MoveList;
use crate::tt::Bound;
use crate::types::Move;
use crate::{is_loss, mated_in, DRAW, INFINITY, MAX_PLY, TB_WIN_IN_MAX};
use std::mem::MaybeUninit;

impl Search {
    pub(super) fn qsearch(
        &mut self,
        board: &mut Board,
        mut alpha: i32,
        beta: i32,
        ply: usize,
    ) -> i32 {
        let original_alpha = alpha;
        if self.stop {
            return 0;
        }
        self.nodes += 1;
        if self.helper_stop_requested() {
            return 0;
        }
        stat!(self, qnodes);
        if self.nodes - self.published >= self.check_interval && self.check_stop() {
            return 0;
        }
        if ply > self.seldepth {
            self.seldepth = ply;
        }
        if board.is_search_draw(ply) {
            return DRAW;
        }
        if ply >= MAX_PLY {
            if board.generate_legal().is_empty() {
                return if board.in_check(board.side()) {
                    mated_in(ply)
                } else {
                    DRAW
                };
            }
            return self
                .nnue
                .eval(board)
                .clamp(-(TB_WIN_IN_MAX - 1), TB_WIN_IN_MAX - 1);
        }

        let us = board.side();
        let in_check = board.in_check(us);
        if !in_check && !board.has_legal_move() {
            return DRAW;
        }

        let key = board.tt_key();
        let probe = self.tt.probe(key, ply);

        if let Some(p) = &probe {
            if board.halfmove() < 90 {
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
        }

        let raw_eval = if in_check {
            0
        } else {
            probe
                .as_ref()
                .map(|p| p.eval)
                .unwrap_or_else(|| self.nnue.eval(board))
        };
        let eval = if in_check {
            -INFINITY
        } else {
            let correction = (self.corrhist.value(board) + self.contcorr_get(ply)) / 64;
            (raw_eval + correction).clamp(-(TB_WIN_IN_MAX - 1), TB_WIN_IN_MAX - 1)
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
                if refine && board.halfmove() < 90 {
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

        let mut list = MoveList::new();
        let mut scores = [MaybeUninit::<i32>::uninit(); 256];
        if in_check {
            let tc = board.threat_ctx();
            board.generate_pseudo(&mut list);
            for (i, &m) in list.as_slice().iter().enumerate() {
                scores[i].write(self.score_move(board, m, ply, &tc));
            }
        } else {
            board.generate_captures(&mut list);
            for (i, &m) in list.as_slice().iter().enumerate() {
                scores[i].write(self.score_noisy(board, m));
            }
        }
        let scores = unsafe {
            std::slice::from_raw_parts_mut(scores.as_mut_ptr().cast::<i32>(), list.len())
        };

        let tt_move = probe.as_ref().map(|p| p.mv).unwrap_or(Move::NONE);
        let tt_first = tt_move != Move::NONE
            && match list.as_slice().iter().position(|&m| m == tt_move) {
                Some(tt_idx) => {
                    list.moves.swap(0, tt_idx);
                    scores.swap(0, tt_idx);
                    scores[0] = 1_000_000;
                    true
                }
                None => false,
            };

        let mut best_move = Move::NONE;
        let mut legal = 0;
        let mut pruned_bound = -INFINITY;
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

            if !is_loss(best) && !in_check {
                let threshold = (alpha - eval) / 8 - self.params.qsearch_see_margin;
                if i == 0 && tt_first {
                    if !board.see(m, threshold) {
                        if threshold > 0 {
                            pruned_bound = alpha;
                        }
                        continue;
                    }
                } else if !see_from_zero(board, m, threshold, scores[i] > 0) {
                    if threshold > 0 {
                        pruned_bound = alpha;
                    }
                    continue;
                }
            }

            let (moved, captured) = super::move_info(board, m);
            board.make_move(m);
            if board.is_attacked(board.king_sq(us), board.side()) {
                board.unmake_move(m);
                continue;
            }
            legal += 1;
            self.tt.prefetch(board.tt_key());
            self.nnue.make(board, m, moved, captured);
            self.ss_piece[ply] = moved.index();
            self.ss_to[ply] = m.to().index();
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

        best = best.max(pruned_bound);
        let bound = if best >= beta {
            Bound::Lower
        } else if best > original_alpha {
            Bound::Exact
        } else {
            Bound::Upper
        };
        let stored_eval = if in_check { 0 } else { raw_eval };
        self.tt
            .store(key, best_move, best, stored_eval, 0, bound, ply);

        best
    }
}

#[inline]
fn see_from_zero(board: &Board, m: Move, threshold: i32, at_zero: bool) -> bool {
    if (at_zero && threshold <= 0) || (!at_zero && threshold >= 0) {
        at_zero
    } else {
        board.see(m, threshold)
    }
}

#[cfg(test)]
mod tests {
    use super::{see_from_zero, Search};
    use crate::board::Board;
    use crate::movegen::MoveList;
    use crate::search::Limits;
    use crate::tt::Bound;
    use crate::INFINITY;

    #[test]
    fn terminal_stalemate_at_qsearch_entry_is_a_draw() {
        let mut board = Board::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
        assert!(!board.in_check(board.side()));
        assert!(board.generate_legal().is_empty());
        let mut search = Search::new(1);
        search.prepare(&board, &Limits::default());
        search.nnue.refresh(&board);
        let static_eval = search.nnue.eval(&board);
        assert_ne!(static_eval, crate::DRAW);

        let score = search.qsearch(&mut board, -INFINITY, INFINITY, 0);

        assert_eq!(
            score,
            crate::DRAW,
            "stalemate was evaluated as {static_eval}"
        );
    }

    #[test]
    fn terminal_stalemate_precedes_stand_pat_cutoff() {
        let mut board = Board::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
        assert!(board.generate_legal().is_empty());
        let mut search = Search::new(1);
        search.prepare(&board, &Limits::default());
        search.nnue.refresh(&board);
        let static_eval = search.nnue.eval(&board);
        let beta = static_eval - 1;

        let score = search.qsearch(&mut board, beta - 1, beta, 0);

        assert_eq!(
            score,
            crate::DRAW,
            "stand-pat returned {score} for stalemate"
        );
    }

    #[test]
    fn terminal_stalemate_is_a_draw_at_qsearch_ply_limit() {
        let mut board = Board::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
        assert!(!board.in_check(board.side()));
        assert!(board.generate_legal().is_empty());
        let mut search = Search::new(1);
        search.prepare(&board, &Limits::default());
        search.nnue.refresh(&board);

        let score = search.qsearch(&mut board, -INFINITY, INFINITY, crate::MAX_PLY);

        assert_eq!(score, crate::DRAW);
    }

    #[test]
    fn terminal_checkmate_is_preserved_at_qsearch_ply_limit() {
        let mut board = Board::from_fen("7k/6Q1/5K2/8/8/8/8/8 b - - 100 1").unwrap();
        assert!(board.in_check(board.side()));
        assert!(board.generate_legal().is_empty());
        assert!(!board.is_search_draw(crate::MAX_PLY));
        let mut search = Search::new(1);
        search.prepare(&board, &Limits::default());
        search.nnue.refresh(&board);

        let score = search.qsearch(&mut board, -INFINITY, INFINITY, crate::MAX_PLY);

        assert_eq!(score, crate::mated_in(crate::MAX_PLY));
    }

    #[test]
    fn terminal_checkmate_is_found_below_the_qsearch_ply_limit() {
        let mut board = Board::from_fen("7k/6Q1/5K2/8/8/8/8/8 b - - 100 1").unwrap();
        let mut search = Search::new(1);
        search.prepare(&board, &Limits::default());
        search.nnue.refresh(&board);

        assert_eq!(
            search.qsearch(&mut board, -INFINITY, INFINITY, 1),
            crate::mated_in(1)
        );
    }

    #[test]
    fn qsearch_tt_does_not_reuse_a_later_rule50_draw() {
        let mut near = Board::from_fen("r4k2/8/8/8/8/8/7Q/K7 w - - 99 1").unwrap();
        let mut earlier = Board::from_fen("r4k2/8/8/8/8/8/7Q/K7 w - - 96 1").unwrap();
        assert!(near.in_check(near.side()));
        assert_eq!(near.tt_key(), earlier.tt_key());
        assert!(!near.generate_legal().is_empty());
        assert!(near
            .generate_legal()
            .as_slice()
            .iter()
            .all(|m| m.is_quiet()));

        let mut search = Search::new(1);
        search.prepare(&near, &Limits::default());
        search.nnue.refresh(&near);
        let late = search.qsearch(&mut near, -INFINITY, INFINITY, 0);
        assert_eq!(late, 0);
        let stored = search.tt.probe(near.tt_key(), 0).unwrap();
        assert!(matches!(stored.bound, Bound::Exact));

        search.prepare(&earlier, &Limits::default());
        search.nnue.refresh(&earlier);
        let warm = search.qsearch(&mut earlier, -INFINITY, INFINITY, 0);
        search.tt.clear();
        search.prepare(&earlier, &Limits::default());
        search.nnue.refresh(&earlier);
        let cold = search.qsearch(&mut earlier, -INFINITY, INFINITY, 0);

        assert_eq!(
            warm, cold,
            "clock99={late}, clock96 warm={warm}, cold={cold}"
        );
    }

    #[test]
    fn qsearch_does_not_refine_eval_from_another_rule50_clock() {
        let mut board = Board::from_fen("7k/8/8/8/8/8/Q7/K7 w - - 96 1").unwrap();
        let mut search = Search::new(1);
        search.prepare(&board, &Limits::default());
        search.nnue.refresh(&board);
        search.tt.store(
            board.tt_key(),
            crate::types::Move::NONE,
            0,
            500,
            0,
            Bound::Upper,
            0,
        );

        assert_eq!(search.qsearch(&mut board, -INFINITY, INFINITY, 0), 500);
    }

    #[test]
    fn cached_zero_see_matches_pseudo_captures() {
        const FENS: [&str; 8] = [
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
            "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
            "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
            "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
            "4r1k1/8/8/8/8/8/3pR3/4K3 w - - 0 1",
            "4r1k1/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
            "7r/6P1/8/8/8/8/8/K6k w - - 0 1",
        ];
        const THRESHOLDS: [i32; 9] = [-1000, -550, -100, -1, 0, 1, 100, 550, 1000];

        for fen in FENS {
            let board = Board::from_fen(fen).unwrap();
            let mut moves = MoveList::new();
            board.generate_captures(&mut moves);
            for &m in moves.as_slice() {
                let at_zero = board.see(m, 0);
                for threshold in THRESHOLDS {
                    assert_eq!(
                        see_from_zero(&board, m, threshold, at_zero),
                        board.see(m, threshold),
                        "{} at threshold {threshold}",
                        m.to_uci()
                    );
                }
            }
        }
    }

    #[test]
    fn qsearch_stores_an_exact_in_window_result() {
        let mut board = Board::startpos();
        let mut search = Search::new(1);
        search.prepare(&board, &Limits::default());
        search.nnue.refresh(&board);
        search.qsearch(&mut board, -INFINITY, INFINITY, 0);

        let probe = search
            .tt
            .probe(board.tt_key(), 0)
            .expect("stored qsearch result");
        assert!(matches!(probe.bound, Bound::Exact));
    }

    #[test]
    fn high_alpha_pruning_does_not_hide_a_free_queen_on_research() {
        let mut board = Board::from_fen("6k1/8/8/2q5/3P4/8/8/K7 w - - 0 1").unwrap();
        let mut search = Search::new(1);
        search.prepare(&board, &Limits::default());
        search.nnue.refresh(&board);
        let eval = search.nnue.eval(&board);
        let wide_alpha = eval + 20_000;
        let first = search.qsearch(&mut board, wide_alpha, wide_alpha + 1, 0);
        assert!(first <= wide_alpha);
        let pruned = search.tt.probe(board.tt_key(), 0).unwrap();
        assert!(matches!(pruned.bound, Bound::Upper));
        assert_eq!(pruned.score, wide_alpha);

        let alpha = eval + 1;
        let beta = alpha + 1;
        let warm = search.qsearch(&mut board, alpha, beta, 0);
        search.tt.clear();
        let cold = search.qsearch(&mut board, alpha, beta, 0);

        assert!(
            cold >= beta,
            "queen capture must exceed standing pat: {cold} < {beta}"
        );
        assert!(
            warm >= beta,
            "warm {warm}, cold {cold}, eval {eval}, initial {first}"
        );
    }

    #[test]
    fn qsearch_still_uses_an_upper_bound_within_its_window() {
        let mut board = Board::startpos();
        let mut search = Search::new(1);
        search.prepare(&board, &Limits::default());
        search.nnue.refresh(&board);
        search.tt.store(
            board.tt_key(),
            crate::types::Move::NONE,
            50,
            100,
            0,
            Bound::Upper,
            0,
        );

        assert_eq!(search.qsearch(&mut board, 50, 51, 0), 50);
        assert_eq!(search.nodes, 1);
    }
}
