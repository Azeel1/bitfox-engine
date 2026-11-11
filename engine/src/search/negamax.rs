use super::{move_info, stat, Search, PIECE_NONE};
use crate::board::Board;
use crate::movegen::MoveList;
use crate::tt::Bound;
use crate::types::{Move, PieceType};
use crate::{
    is_decisive, is_loss, is_win, mated_in, DRAW, INFINITY, MAX_PLY, SCORE_NONE, TB_WIN,
    TB_WIN_IN_MAX,
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
        if !root && board.is_search_draw(ply) {
            return DRAW;
        }
        if depth <= 0 {
            return self.qsearch(board, alpha, beta, ply);
        }

        self.nodes += 1;
        if self.helper_stop_requested() {
            return 0;
        }
        if self.nodes - self.published >= self.check_interval && self.check_stop() {
            return 0;
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
        self.cutoff_cnt[ply + 2] = 0;

        let us = board.side();
        let in_check = board.in_check(us);
        if in_check {
            depth += 1;
        }

        let excluded = self.excluded[ply];
        let is_excluded = excluded != Move::NONE;

        let key = board.tt_key();
        let probe = self.tt.probe(key, ply);
        stat!(self, tt_probes);
        if probe.is_some() {
            stat!(self, tt_hits);
        }
        let tt_move = probe.as_ref().map(|p| p.mv).unwrap_or(Move::NONE);
        let tt_score = probe.as_ref().map(|p| p.score).unwrap_or(SCORE_NONE);
        let tt_depth = probe.as_ref().map(|p| p.depth).unwrap_or(0);
        let tt_bound = probe.as_ref().map(|p| p.bound).unwrap_or(Bound::None);
        if !pv_node && !is_excluded {
            if let Some(p) = &probe {
                if p.depth >= depth && board.halfmove() < 90 {
                    let cut = match p.bound {
                        Bound::Exact => true,
                        Bound::Lower => p.score >= beta,
                        Bound::Upper => p.score <= alpha,
                        Bound::None => false,
                    };
                    if cut {
                        stat!(self, tt_cutoffs);
                        return p.score;
                    }
                }
            }
        }

        if let Some(tb) = &self.tb {
            if !root
                && !is_excluded
                && board.halfmove() == 0
                && board.castling().0 == 0
                && (board.occupancy().count() as usize) <= tb.max_pieces()
            {
                if let Some(wdl) = tb.probe_wdl(board) {
                    self.tb_hits += 1;
                    let score = match wdl {
                        shakmaty_syzygy::Wdl::Win => TB_WIN - ply as i32,
                        shakmaty_syzygy::Wdl::Loss if in_check && !board.has_legal_move() => {
                            mated_in(ply)
                        }
                        shakmaty_syzygy::Wdl::Loss => -TB_WIN + ply as i32,
                        _ => DRAW,
                    };
                    return score;
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
            (self.corrhist.value(board) + self.contcorr_get(ply)) / 64
        };
        let static_eval = if in_check {
            0
        } else {
            (raw_eval + correction).clamp(-(TB_WIN_IN_MAX - 1), TB_WIN_IN_MAX - 1)
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
            if depth <= 3
                && static_eval + self.params.razor_base + self.params.razor_mult * depth <= alpha
            {
                let v = self.qsearch(board, alpha - 1, alpha, ply);
                if v < alpha {
                    return v;
                }
            }
            if depth <= 8 && static_eval - self.params.rfp_margin * depth >= beta {
                return static_eval;
            }
            if depth >= 3
                && static_eval >= beta
                && board.has_non_pawn(us)
                && (root || self.ss_piece[ply - 1] != PIECE_NONE)
            {
                let r = self.params.nmp_base
                    + depth / 4
                    + ((static_eval - beta) / self.params.nmp_eval_div).min(self.params.nmp_max);
                self.ss_piece[ply] = PIECE_NONE;
                self.ss_to[ply] = 0;
                board.make_null();
                self.tt.prefetch(board.tt_key());
                let score = -self.negamax(board, -beta, -beta + 1, depth - r, ply + 1);
                board.unmake_null();
                if self.stop {
                    return 0;
                }
                if score >= beta {
                    stat!(self, null_cutoffs);
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
                    cap_scores[i] = self.score_move(board, m, ply, &tc);
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
                    self.tt.prefetch(board.tt_key());
                    self.nnue.make(board, m, moved, captured);
                    self.ss_piece[ply] = moved.index();
                    self.ss_to[ply] = m.to().index();
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
                        stat!(self, probcut_hits);
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
        let mut scores = [i32::MIN; 256];
        let tt_first = if tt_move != Move::NONE {
            if let Some(tt_idx) = list.as_slice().iter().position(|&m| m == tt_move) {
                list.moves.swap(0, tt_idx);
                scores[0] = 1_000_000;
                true
            } else {
                false
            }
        } else {
            false
        };

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
            if !(i == 0 && tt_first) {
                if scores[i] == i32::MIN {
                    scores[i] = self.score_move(board, list.moves[i], ply, &tc);
                }
                for j in (i + 1)..list.len() {
                    if scores[j] == i32::MIN {
                        scores[j] = self.score_move(board, list.moves[j], ply, &tc);
                    }
                    if scores[j] > scores[best_idx] {
                        best_idx = j;
                    }
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
                    self.capture_history_get(board, m)
                };
                let gives_check = board.gives_direct_check(m);

                if !in_check
                    && !gives_check
                    && quiet
                    && !is_win(beta)
                    && (legal + 1) as i32
                        >= (self.params.lmp_base
                            + self.params.lmp_improvement_mult * improvement
                            + self.params.lmp_depth_mult * depth * depth
                            + history / self.params.pruning_history_div)
                            / 1024
                {
                    skip_quiets = true;
                    continue;
                }

                let futility_value = static_eval
                    + self.params.futility_mult * depth
                    + history / self.params.pruning_history_div
                    + self.params.futility_eval_bonus * (static_eval >= beta) as i32
                    - self.params.futility_base;
                if !in_check
                    && !gives_check
                    && quiet
                    && depth <= self.params.futility_max_depth
                    && futility_value <= alpha
                {
                    if !is_decisive(best) && best < futility_value {
                        best = futility_value;
                    }
                    skip_quiets = true;
                    continue;
                }

                if !in_check
                    && quiet
                    && depth <= self.params.history_pruning_max_depth
                    && history < -self.params.history_pruning_mult * depth
                {
                    continue;
                }

                let threshold = if quiet {
                    (-self.params.see_quiet_depth_mult * depth * depth
                        + self.params.see_quiet_linear * depth
                        - history / self.params.see_history_div
                        + self.params.see_quiet_base)
                        .min(0)
                } else {
                    (-self.params.see_noisy_depth_mult * depth * depth
                        - self.params.see_noisy_linear * depth
                        - history / self.params.see_history_div)
                        .min(0)
                };
                let see_ge_zero = !(quiet || i == 0 && tt_first) && scores[i] > 0;
                if (!in_check || !quiet) && !see_ge_zero && !board.see(m, threshold) {
                    continue;
                }
            }

            let (moved, captured) = move_info(board, m);
            let moved_pc = moved.index();

            board.make_move(m);
            if board.is_attacked(board.king_sq(us), board.side()) {
                board.unmake_move(m);
                continue;
            }
            legal += 1;
            self.tt.prefetch(board.tt_key());
            self.nnue.make(board, m, moved, captured);
            self.ss_piece[ply] = moved_pc;
            self.ss_to[ply] = m.to().index();

            let new_depth = depth - 1 + if m == tt_move { tt_extension } else { 0 };
            let score;
            if legal == 1 {
                score = -self.negamax(board, -beta, -alpha, new_depth, ply + 1);
            } else {
                let mut r = 0;
                if depth >= 3 && !in_check {
                    let d = depth.min(63) as usize;
                    let n = legal.min(63);
                    let hist = if quiet {
                        r = self.lmr[d][n];
                        self.history_get(us, from_th, to_th, m)
                            + self.conthist_get(ply, moved_pc, m.to().index())
                    } else {
                        r = self.lmr_noisy[d][n];
                        let victim =
                            captured.map_or(PieceType::Pawn, |(piece, _)| piece.piece_type());
                        self.noisy_get(moved_pc, m.to().index(), victim)
                    };
                    if pv_node {
                        r -= 1;
                    }
                    if improvement > 0 {
                        r -= 1;
                    } else {
                        r += 1;
                    }
                    if self.cutoff_cnt[ply + 1] > 2 {
                        r += 1;
                        if self.cutoff_cnt[ply + 1] > 4 {
                            r += 1;
                        }
                    }
                    r -= hist / 8192;
                    r = r.clamp(0, new_depth - 1);
                }
                if r > 0 {
                    stat!(self, lmr_reductions);
                }
                let mut s = -self.negamax(board, -alpha - 1, -alpha, new_depth - r, ply + 1);
                if s > alpha && r > 0 {
                    stat!(self, lmr_researches);
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
                        self.cutoff_cnt[ply] += 1;
                        stat!(self, beta_cutoffs);
                        let bonus = (depth * depth).min(self.params.history_bonus_max);
                        if quiet {
                            let reward = (32 * depth - 16).clamp(0, self.params.history_bonus_max);
                            let penalty = (16 * depth - 8).clamp(0, self.params.history_bonus_max);
                            self.update_killers(ply, m);
                            self.update_history(us, from_th, to_th, m, reward);
                            self.update_conthist(ply, moved_pc, m.to().index(), reward);
                            if ply >= 1 && self.ss_piece[ply - 1] != PIECE_NONE {
                                self.countermove[self.ss_piece[ply - 1]][self.ss_to[ply - 1]] = m;
                            }
                            for q in quiets.iter().take(quiet_count) {
                                let qf = tc.all.contains(q.from());
                                let qt = tc.all.contains(q.to());
                                self.update_history(us, qf, qt, *q, -penalty);
                                let qpc =
                                    board.piece_on(q.from()).map_or(PIECE_NONE, |p| p.index());
                                self.update_conthist(ply, qpc, q.to().index(), -penalty);
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
            self.contcorr_update(ply, depth, best - static_eval);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::Limits;

    #[test]
    #[cfg(feature = "stats")]
    fn null_move_pruning_requires_a_real_previous_move() {
        let mut board = Board::startpos();
        board.make_null();
        for previous_piece in [PIECE_NONE, 0] {
            let mut search = Search::new(1);
            search.prepare(&board, &Limits::default());
            search.nnue.refresh(&board);
            search.params.rfp_margin = INFINITY;
            search.ss_piece[0] = previous_piece;
            let beta = search.nnue.eval(&board) - 1000;

            search.negamax(&mut board, beta - 1, beta, 3, 1);

            assert_eq!(
                search.stats.null_cutoffs,
                u64::from(previous_piece != PIECE_NONE)
            );
        }
        board.unmake_null();
    }

    #[test]
    #[ignore = "requires BITFOX_SYZYGY_TEST_DIR with a real KQvK.rtbw table"]
    fn terminal_capture_mate_precedes_syzygy_wdl() {
        let path = std::env::var("BITFOX_SYZYGY_TEST_DIR")
            .expect("set BITFOX_SYZYGY_TEST_DIR to a directory containing KQvK.rtbw");
        let mut board = Board::from_fen("7k/5Qr1/5K2/8/8/8/8/8 w - - 0 1").unwrap();
        let capture = board
            .generate_legal()
            .as_slice()
            .iter()
            .copied()
            .find(|m| m.to_uci() == "f7g7")
            .unwrap();
        assert!(capture.is_capture());
        board.make_move(capture);
        assert_eq!(board.halfmove(), 0);
        assert_eq!(board.castling().0, 0);
        assert!(board.in_check(board.side()));
        assert!(board.generate_legal().is_empty());

        let mut control = Search::new(1);
        control.prepare(&board, &Limits::default());
        control.nnue.refresh(&board);
        assert_eq!(
            control.negamax(&mut board, -INFINITY, INFINITY, 1, 1),
            mated_in(1)
        );

        let mut search = Search::new(1);
        assert!(search.set_tablebase(&path));
        let tb = search.tb.as_ref().unwrap();
        assert!(tb.max_pieces() >= 3);
        assert_eq!(tb.probe_wdl(&board), Some(shakmaty_syzygy::Wdl::Loss));
        search.prepare(&board, &Limits::default());
        search.nnue.refresh(&board);
        let score = search.negamax(&mut board, -INFINITY, INFINITY, 1, 1);
        assert_eq!(
            score,
            mated_in(1),
            "achieved mate returned {score}, ordinary tablebase loss is {}",
            -TB_WIN + 1
        );
    }

    #[test]
    fn terminal_stalemate_is_a_draw_at_negamax_ply_limit() {
        let mut board = Board::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
        let mut search = Search::new(1);
        search.prepare(&board, &Limits::default());
        search.nnue.refresh(&board);

        assert_eq!(
            search.negamax(&mut board, -INFINITY, INFINITY, 1, MAX_PLY),
            DRAW
        );
    }

    #[test]
    fn terminal_checkmate_is_preserved_at_negamax_ply_limit() {
        let mut board = Board::from_fen("7k/6Q1/5K2/8/8/8/8/8 b - - 100 1").unwrap();
        assert!(board.in_check(board.side()));
        assert!(board.generate_legal().is_empty());
        let mut search = Search::new(1);
        search.prepare(&board, &Limits::default());
        search.nnue.refresh(&board);

        assert_eq!(
            search.negamax(&mut board, -INFINITY, INFINITY, 1, MAX_PLY),
            mated_in(MAX_PLY)
        );
    }

    #[test]
    fn terminal_stalemate_is_a_draw_at_positive_full_window_depth() {
        let mut board = Board::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
        let mut search = Search::new(1);
        search.prepare(&board, &Limits::default());
        search.nnue.refresh(&board);

        assert_eq!(search.negamax(&mut board, -INFINITY, INFINITY, 1, 1), DRAW);
    }

    #[test]
    fn noisy_lmr_uses_the_captured_piece_history() {
        let mut board = Board::from_fen("7k/8/8/2q5/3P4/8/8/K7 w - - 0 1").unwrap();
        let mut search = Search::new(1);
        search.prepare(&board, &Limits::default());
        search.nnue.refresh(&board);
        search.params.razor_base = INFINITY;

        let legal = board.generate_legal();
        let first = legal
            .as_slice()
            .iter()
            .copied()
            .find(|m| m.to_uci() == "a1b1")
            .unwrap();
        let capture = legal
            .as_slice()
            .iter()
            .copied()
            .find(|m| m.to_uci() == "d4c5")
            .unwrap();
        let piece = board.piece_on(capture.from()).unwrap().index();
        search.noisy_hist[piece][capture.to().index()][PieceType::Queen.index()] = 8192;
        search.noisy_hist[piece][capture.to().index()][PieceType::Pawn.index()] = -8192;

        for &m in legal.as_slice() {
            if m != capture {
                board.make_move(m);
                search
                    .tt
                    .store(board.tt_key(), Move::NONE, -19000, 0, 16, Bound::Exact, 1);
                board.unmake_move(m);
            }
        }
        search
            .tt
            .store(board.tt_key(), first, 0, 0, 0, Bound::None, 0);

        board.make_move(capture);
        let capture_key = board.tt_key();
        let reply = board.generate_legal().moves[0];
        search
            .tt
            .store(capture_key, reply, 0, -25000, 0, Bound::None, 1);
        board.unmake_move(capture);

        search.negamax(&mut board, 19999, 20000, 6, 0);

        let searched = search.tt.probe(capture_key, 1).unwrap();
        assert_eq!(
            searched.depth, 5,
            "positive queen-capture history must preserve the capture's full depth"
        );
    }
}
