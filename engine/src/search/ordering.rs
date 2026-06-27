use super::Search;
use crate::board::{Board, ThreatCtx};
use crate::types::{Move, PieceType};

const MVV: [i32; 6] = [100, 320, 330, 500, 900, 0];
const ESCAPE: [i32; 6] = [0, 4900, 4500, 7700, 11200, 0];
const INTO_THREAT: i32 = 4900;
const CHECK_BONUS: i32 = 5900;

impl Search {
    pub(super) fn score_move(
        &self,
        board: &Board,
        m: Move,
        tt_move: Move,
        ply: usize,
        tc: &ThreatCtx,
    ) -> i32 {
        if m == tt_move {
            return 1_000_000;
        }
        if m.is_capture() || m.is_promotion() {
            let victim = if m.is_en_passant() {
                PieceType::Pawn
            } else {
                board
                    .piece_on(m.to())
                    .map(|p| p.piece_type())
                    .unwrap_or(PieceType::Pawn)
            };
            let attacker = board
                .piece_on(m.from())
                .map(|p| p.piece_type())
                .unwrap_or(PieceType::Pawn);
            let promo = m.promotion().map(|pt| MVV[pt.index()]).unwrap_or(0);
            let mvv = 800_000 + MVV[victim.index()] * 16 - MVV[attacker.index()] + promo;
            let piece = board.piece_on(m.from()).map_or(super::PIECE_NONE, |p| p.index());
            let noisy = if piece < 12 {
                self.noisy_get(piece, m.to().index(), victim)
            } else {
                0
            };
            let scored = mvv + noisy;
            return if board.see(m, 0) { scored } else { scored - 1_600_000 };
        }
        if m == self.killers[ply][0] {
            return 700_000;
        }
        if m == self.killers[ply][1] {
            return 600_000;
        }
        let c = board.side();
        let from = m.from();
        let to = m.to();
        let from_th = tc.all.contains(from);
        let to_th = tc.all.contains(to);
        let piece = board.piece_on(from).map_or(super::PIECE_NONE, |p| p.index());
        let pt = board.piece_on(from).map_or(0, |p| p.piece_type().index());
        let mut score =
            self.history_get(c, from_th, to_th, m) + self.conthist_get(ply, piece, to.index());
        score += ESCAPE[pt] * tc.threatened[pt].contains(from) as i32;
        score += CHECK_BONUS * tc.check[pt].contains(to) as i32;
        score -= INTO_THREAT * tc.threatened[pt].contains(to) as i32;
        score
    }
}
