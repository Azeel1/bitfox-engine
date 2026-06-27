use super::{move_info, Search, PIECE_NONE};
use crate::board::Board;
use crate::movegen::MoveList;
use crate::tt::Bound;
use crate::types::{Move, PieceType};
use crate::{
    is_decisive, is_loss, is_win, mated_in, DRAW, INFINITY, MATE_IN_MAX, MAX_PLY, SCORE_NONE,
};

impl Search {
    pub(super) fn negamax(
        &mut self,
        board: &mut Board,
        mut alpha: i32,
        beta: i32,
        mut depth: i32,
        ply: usize,
    ) -> i32 {
        self.pv_len[ply] = 0;
        let pv_node = beta - alpha > 1;
        let root = ply == 0;

        if self.stop {
            return 0;
        }
        if !root && board.is_draw() {
            return DRAW;
        }
        if depth <= 0 {
            return self.qsearch(board, alpha, beta, ply);
        }

        self.nodes += 1;
        if self.nodes % 2048 == 0 {
            self.check_stop();
        }
        if ply >= MAX_PLY {
            return self.nnue.eval(board);
        }

        let us = board.side();
        let in_check = board.in_check(us);
        if in_check {
            depth += 1;
        }

        let excluded = self.excluded[ply];
        let is_excluded = excluded != Move::NONE;

        let key = board.key();
        let probe = self.tt.probe(key, ply);
        let tt_move = probe.as_ref().map(|p| p.mv).unwrap_or(Move::NONE);
        let tt_score = probe.as_ref().map(|p| p.score).unwrap_or(SCORE_NONE);
        let tt_depth = probe.as_ref().map(|p| p.depth).unwrap_or(0);
        let tt_bound = probe.as_ref().map(|p| p.bound).unwrap_or(Bound::None);
        if !pv_node && !is_excluded {
            if let Some(p) = &probe {
                if p.depth >= depth {
                    let cut = match p.bound {
                        Bound::Exact => true,
                        Bound::Lower => p.score >= beta,
                        Bound::Upper => p.score <= alpha,
                        Bound::None => false,
                    };
                    if cut {
                        return p.score;
                    }
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
        let correction = if in_check {
            0
        } else {
            self.corrhist.value(board)
        };
        let static_eval = if in_check {
            0
        } else {
            (raw_eval + correction).clamp(-(MATE_IN_MAX - 1), MATE_IN_MAX - 1)
        };

        self.ss_eval[ply] = if in_check { SCORE_NONE } else { static_eval };
        let improvement = if in_check {
            0
        } else if ply >= 2 && self.ss_eval[ply - 2] != SCORE_NONE {
            static_eval - self.ss_eval[ply - 2]
        } else if ply >= 4 && self.ss_eval[ply - 4] != SCORE_NONE {
            static_eval - self.ss_eval[ply - 4]
        } else {
            0
        };

        if !pv_node && !in_check && !is_excluded && !is_decisive(beta) {
            if depth <= 3 && static_eval + 300 + 200 * depth <= alpha {
                let v = self.qsearch(board, alpha - 1, alpha, ply);
                if v < alpha {
                    return v;
                }
            }
            if depth <= 8 && static_eval - 80 * depth >= beta {
                return static_eval;
            }
            if depth >= 3 && static_eval >= beta && board.has_non_pawn(us) {
                let r = 3 + depth / 4 + ((static_eval - beta) / 200).min(3);
                board.make_null();
                let score = -self.negamax(board, -beta, -beta + 1, depth - r, ply + 1);
                board.unmake_null();
                if self.stop {
                    return 0;
                }
                if score >= beta {
                    return if is_decisive(score) { beta } else { score };
                }
            }
        }
        let mut tt_extension = 0;
        let potential_singularity = !is_excluded
            && depth >= 6 + pv_node as i32
            && tt_move != Move::NONE
            && tt_bound != Bound::Upper
            && tt_depth >= depth - 3
            && probe.is_some()
            && !is_decisive(tt_score);
        if ply > 0 && potential_singularity {
            let singular_beta = tt_score - 2 * depth;
            let singular_depth = (depth - 1) / 2;
            self.excluded[ply] = tt_move;
            let s = self.negamax(board, singular_beta - 1, singular_beta, singular_depth, ply);
            self.excluded[ply] = Move::NONE;
            if self.stop {
                return 0;
            }
            if s < singular_beta {
                tt_extension = 1;
            } else if s >= beta && !is_decisive(s) {
                return s;
            } else if tt_score >= beta {
                tt_extension = -2;
            }
        }

        let tc = board.threat_ctx();

        if !pv_node && !in_check && !is_excluded && depth >= 5 && !is_decisive(beta) {
            let probcut_beta = beta + 200;
            let tt_blocks = probe.is_some() && tt_depth >= depth - 3 && tt_score < probcut_beta;
            if !tt_blocks {
                let mut caps = MoveList::new();
                board.generate_captures(&mut caps);
                let mut cap_scores = [0i32; 256];
                for (i, &m) in caps.as_slice().iter().enumerate() {
                    cap_scores[i] = self.score_move(board, m, Move::NONE, ply, &tc);
                }
                for i in 0..caps.len() {
                    let mut bi = i;
                    for j in (i + 1)..caps.len() {
                        if cap_scores[j] > cap_scores[bi] {
                            bi = j;
                        }
                    }
                    caps.moves.swap(i, bi);
                    cap_scores.swap(i, bi);
                    let m = caps.moves[i];
                    if m == excluded || !board.see(m, probcut_beta - static_eval) {
                        continue;
                    }
                    let (moved, captured) = move_info(board, m);
                    board.make_move(m);
                    if board.is_attacked(board.king_sq(us), board.side()) {
                        board.unmake_move(m);
                        continue;
                    }
                    self.nnue.make(board, m, moved, captured);
                    let mut score = -self.qsearch(board, -probcut_beta, -probcut_beta + 1, ply + 1);
                    if score >= probcut_beta {
                        score = -self.negamax(
                            board,
                            -probcut_beta,
                            -probcut_beta + 1,
                            depth - 4,
                            ply + 1,
                        );
                    }
                    board.unmake_move(m);
                    self.nnue.unmake();
                    if self.stop {
                        return 0;
                    }
                    if score >= probcut_beta {
                        self.tt
                            .store(key, m, score, raw_eval, depth - 3, Bound::Lower, ply);
                        return score;
                    }
                }
            }
        }

        if depth >= 4 && tt_move == Move::NONE && !is_excluded {
            depth -= 1;
        }

        let mut list = MoveList::new();
        board.generate_pseudo(&mut list);
        let mut scores = [0i32; 256];
        for (i, &m) in list.as_slice().iter().enumerate() {
            scores[i] = self.score_move(board, m, tt_move, ply, &tc);
        }

        let mut best = -INFINITY;
        let mut best_move = Move::NONE;
        let mut bound = Bound::Upper;
        let mut legal = 0;
        let mut quiets: [Move; 48] = [Move::NONE; 48];
        let mut quiet_count = 0;
        let mut noisies: [Move; 32] = [Move::NONE; 32];
        let mut noisy_count = 0;
        let mut skip_quiets = false;

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
            if m == excluded {
                continue;
            }
            let quiet = m.is_quiet();

            if quiet && skip_quiets {
                continue;
            }

            let from_th = tc.all.contains(m.from());
            let to_th = tc.all.contains(m.to());

            if ply > 0 && !is_loss(best) {
                let history = if quiet {
                    self.history_get(us, from_th, to_th, m)
                } else {
                    0
                };
                let gives_check = board.gives_direct_check(m);

                if !in_check
                    && !gives_check
                    && quiet
                    && !is_win(beta)
                    && (legal + 1) as i32
                        >= (2818
                            + 78 * improvement / 16
                            + 1351 * depth * depth
                            + 74 * history / 1024)
                            / 1024
                {
                    skip_quiets = true;
                    continue;
                }

                let futility_value = static_eval
                    + 79 * depth
                    + 55 * history / 1024
                    + 77 * (static_eval >= beta) as i32
                    - 127;
                if !in_check && !gives_check && quiet && depth < 14 && futility_value <= alpha {
                    if !is_decisive(best) && best < futility_value {
                        best = futility_value;
                    }
                    skip_quiets = true;
                    continue;
                }

                if !in_check && quiet && depth < 5 && history < -948 * depth {
                    continue;
                }

                let threshold = if quiet {
                    (-12 * depth * depth + 56 * depth - 27 * history / 1024 + 27).min(0)
                } else {
                    (-7 * depth * depth - 36 * depth - 39 * history / 1024 + 14).min(0)
                };
                if (!in_check || !quiet) && !board.see(m, threshold) {
                    continue;
                }
            }

            let moved_pc = board.piece_on(m.from()).map_or(PIECE_NONE, |p| p.index());
            let (moved, captured) = move_info(board, m);

            board.make_move(m);
            if board.is_attacked(board.king_sq(us), board.side()) {
                board.unmake_move(m);
                continue;
            }
            legal += 1;
            self.nnue.make(board, m, moved, captured);
            self.ss_piece[ply] = moved_pc;
            self.ss_to[ply] = m.to().index();

            let new_depth = depth - 1 + if m == tt_move { tt_extension } else { 0 };
            let score;
            if legal == 1 {
                score = -self.negamax(board, -beta, -alpha, new_depth, ply + 1);
            } else {
                let mut r = 0;
                if depth >= 3 && quiet && !in_check {
                    r = self.lmr[depth.min(63) as usize][legal.min(63)];
                    if pv_node {
                        r -= 1;
                    }
                    if improvement > 0 {
                        r -= 1;
                    } else {
                        r += 1;
                    }
                    let hist = self.history_get(us, from_th, to_th, m)
                        + self.conthist_get(ply, moved_pc, m.to().index());
                    r -= hist / 8192;
                    r = r.clamp(0, new_depth - 1);
                }
                let mut s = -self.negamax(board, -alpha - 1, -alpha, new_depth - r, ply + 1);
                if s > alpha && r > 0 {
                    s = -self.negamax(board, -alpha - 1, -alpha, new_depth, ply + 1);
                }
                if s > alpha && s < beta {
                    s = -self.negamax(board, -beta, -alpha, new_depth, ply + 1);
                }
                score = s;
            }
            board.unmake_move(m);
            self.nnue.unmake();

            if self.stop {
                return 0;
            }

            if score > best {
                best = score;
                best_move = m;
                if score > alpha {
                    alpha = score;
                    bound = Bound::Exact;
                    self.update_pv(ply, m);
                    if score >= beta {
                        bound = Bound::Lower;
                        let bonus = (depth * depth).min(1200);
                        if quiet {
                            self.update_killers(ply, m);
                            self.update_history(us, from_th, to_th, m, bonus);
                            self.update_conthist(ply, moved_pc, m.to().index(), bonus);
                            for q in quiets.iter().take(quiet_count) {
                                let qf = tc.all.contains(q.from());
                                let qt = tc.all.contains(q.to());
                                self.update_history(us, qf, qt, *q, -bonus);
                                let qpc =
                                    board.piece_on(q.from()).map_or(PIECE_NONE, |p| p.index());
                                self.update_conthist(ply, qpc, q.to().index(), -bonus);
                            }
                        } else {
                            let cap = capture_victim(board, m);
                            self.update_noisy(moved_pc, m.to().index(), cap, bonus);
                        }
                        for c in noisies.iter().take(noisy_count) {
                            let cpc = board.piece_on(c.from()).map_or(PIECE_NONE, |p| p.index());
                            if cpc < 12 {
                                let cap = capture_victim(board, *c);
                                self.update_noisy(cpc, c.to().index(), cap, -bonus);
                            }
                        }
                        break;
                    }
                }
            }
            if quiet {
                if quiet_count < quiets.len() {
                    quiets[quiet_count] = m;
                    quiet_count += 1;
                }
            } else if noisy_count < noisies.len() {
                noisies[noisy_count] = m;
                noisy_count += 1;
            }
        }

        if legal == 0 {
            if is_excluded {
                return alpha;
            }
            return if in_check { mated_in(ply) } else { DRAW };
        }

        let skip_corr = in_check
            || is_excluded
            || !best_move.is_quiet()
            || (bound == Bound::Upper && best >= static_eval)
            || (bound == Bound::Lower && best <= static_eval);
        if !skip_corr {
            self.corrhist.update(board, depth, best - static_eval);
        }

        if !is_excluded {
            self.tt
                .store(key, best_move, best, raw_eval, depth, bound, ply);
        }
        best
    }
}

#[inline]
fn capture_victim(board: &Board, m: Move) -> PieceType {
    if m.is_en_passant() {
        PieceType::Pawn
    } else {
        board
            .piece_on(m.to())
            .map(|p| p.piece_type())
            .unwrap_or(PieceType::Pawn)
    }
}
