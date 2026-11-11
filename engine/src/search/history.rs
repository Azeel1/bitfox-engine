use super::{Search, CONTCORR_MAX, HIST_MAX, NOISY_MAX, PIECE_NONE};
use crate::board::Board;
use crate::types::{Color, Move, PieceType};

impl Search {
    pub(super) fn update_killers(&mut self, ply: usize, m: Move) {
        if self.killers[ply][0] != m {
            self.killers[ply][1] = self.killers[ply][0];
            self.killers[ply][0] = m;
        }
    }

    #[inline]
    pub(super) fn history_get(&self, color: Color, from_th: bool, to_th: bool, m: Move) -> i32 {
        self.history[color.index()][from_th as usize][to_th as usize][m.from().index()]
            [m.to().index()]
    }

    pub(super) fn update_history(
        &mut self,
        color: Color,
        from_th: bool,
        to_th: bool,
        m: Move,
        bonus: i32,
    ) {
        let e = &mut self.history[color.index()][from_th as usize][to_th as usize]
            [m.from().index()][m.to().index()];
        *e += bonus - *e * bonus.abs() / HIST_MAX;
    }

    #[inline]
    pub(super) fn noisy_get(&self, piece: usize, to: usize, captured: PieceType) -> i32 {
        self.noisy_hist[piece][to][captured.index()] as i32
    }

    #[inline]
    pub(super) fn capture_history_get(&self, board: &Board, m: Move) -> i32 {
        let Some(piece) = board.piece_on(m.from()) else {
            return 0;
        };
        let victim = if m.is_en_passant() {
            PieceType::Pawn
        } else {
            board
                .piece_on(m.to())
                .map_or(PieceType::Pawn, |p| p.piece_type())
        };
        self.noisy_get(piece.index(), m.to().index(), victim)
    }

    pub(super) fn update_noisy(
        &mut self,
        piece: usize,
        to: usize,
        captured: PieceType,
        bonus: i32,
    ) {
        let e = &mut self.noisy_hist[piece][to][captured.index()];
        let v = *e as i32;
        *e = (v + bonus - v * bonus.abs() / NOISY_MAX) as i16;
    }

    #[inline(always)]
    pub(super) fn conthist_get(&self, ply: usize, piece: usize, to: usize) -> i32 {
        let mut score = 0;
        for offset in [1usize, 2, 4] {
            if ply >= offset {
                let pp = self.ss_piece[ply - offset];
                let pt = self.ss_to[ply - offset];
                score += self.cont_hist[pp][pt][piece][to] as i32;
            }
        }
        score
    }

    pub(super) fn contcorr_get(&self, ply: usize) -> i32 {
        let mut sum = 0;
        if ply >= 2 && self.ss_piece[ply - 1] != PIECE_NONE {
            let p1 = self.ss_piece[ply - 1];
            let t1 = self.ss_to[ply - 1];
            for offset in [2usize, 4] {
                if ply >= offset && self.ss_piece[ply - offset] != PIECE_NONE {
                    let pp = self.ss_piece[ply - offset];
                    let pt = self.ss_to[ply - offset];
                    sum += self.cont_corr[pp][pt][p1][t1] as i32;
                }
            }
        }
        sum
    }

    pub(super) fn contcorr_update(&mut self, ply: usize, depth: i32, diff: i32) {
        if ply < 2 || self.ss_piece[ply - 1] == PIECE_NONE {
            return;
        }
        let bonus = (i64::from(depth) * i64::from(diff)).clamp(-4096, 4096) as i32;
        let p1 = self.ss_piece[ply - 1];
        let t1 = self.ss_to[ply - 1];
        for offset in [2usize, 4] {
            if ply >= offset && self.ss_piece[ply - offset] != PIECE_NONE {
                let pp = self.ss_piece[ply - offset];
                let pt = self.ss_to[ply - offset];
                let e = &mut self.cont_corr[pp][pt][p1][t1];
                let v = *e as i32;
                *e = (v + bonus - bonus.abs() * v / CONTCORR_MAX) as i16;
            }
        }
    }

    pub(super) fn update_conthist(&mut self, ply: usize, piece: usize, to: usize, bonus: i32) {
        for offset in [1usize, 2, 4] {
            if ply >= offset {
                let pp = self.ss_piece[ply - offset];
                let pt = self.ss_to[ply - offset];
                let e = &mut self.cont_hist[pp][pt][piece][to];
                let v = *e as i32;
                *e = (v + bonus - v * bonus.abs() / HIST_MAX) as i16;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_correction_saturates_without_overflow() {
        for (depth, diff, expected) in [
            (256, 60_000, 4096),
            (256, -60_000, -4096),
            (i32::MIN, i32::MIN, 4096),
            (i32::MAX, i32::MIN, -4096),
        ] {
            let mut search = Search::new(1);
            search.ss_piece[0] = 0;
            search.ss_to[0] = 16;
            search.ss_piece[1] = 6;
            search.ss_to[1] = 40;

            search.contcorr_update(2, depth, diff);

            assert_eq!(search.contcorr_get(2), expected);
        }
    }

    #[test]
    fn correction_and_capture_histories_remain_within_symmetric_limits() {
        let mut search = Search::new(1);
        search.ss_piece[0] = 0;
        search.ss_to[0] = 16;
        search.ss_piece[1] = 6;
        search.ss_to[1] = 40;
        for sign in [1, -1, 1] {
            for _ in 0..128 {
                search.contcorr_update(2, i32::MAX, sign * i32::MAX);
                search.update_noisy(0, 16, PieceType::Queen, sign * 4096);
                assert!((-CONTCORR_MAX..=CONTCORR_MAX).contains(&search.contcorr_get(2)));
                assert!((-NOISY_MAX..=NOISY_MAX).contains(&search.noisy_get(
                    0,
                    16,
                    PieceType::Queen
                )));
            }
            assert_eq!(search.contcorr_get(2), sign * CONTCORR_MAX);
            assert_eq!(search.noisy_get(0, 16, PieceType::Queen), sign * NOISY_MAX);
        }
    }

    #[test]
    fn capture_history_uses_the_moving_piece_and_capture_victim() {
        let mut search = Search::new(1);
        for (fen, uci, victim) in [
            ("7k/8/8/2q5/3P4/8/8/K7 w - - 0 1", "d4c5", PieceType::Queen),
            ("7k/8/8/3pP3/8/8/8/K7 w - d6 0 1", "e5d6", PieceType::Pawn),
            ("7k/P7/8/8/8/8/8/K7 w - - 0 1", "a7a8q", PieceType::Pawn),
            ("r6k/1P6/8/8/8/8/8/K7 w - - 0 1", "b7a8q", PieceType::Rook),
        ] {
            let mut board = Board::from_fen(fen).unwrap();
            let m = board
                .generate_legal()
                .as_slice()
                .iter()
                .copied()
                .find(|m| m.to_uci() == uci)
                .unwrap();
            let piece = board.piece_on(m.from()).unwrap().index();
            search.noisy_hist[piece][m.to().index()].fill(-1024);
            search.noisy_hist[piece][m.to().index()][victim.index()] = 4096;
            assert_eq!(search.capture_history_get(&board, m), 4096, "{uci}");
        }
    }
}
