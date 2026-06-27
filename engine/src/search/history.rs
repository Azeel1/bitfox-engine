use super::{Search, HIST_MAX, NOISY_MAX};
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
