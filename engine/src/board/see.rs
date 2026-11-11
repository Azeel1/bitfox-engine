use super::Board;
use crate::movegen::{
    bishop_attacks, bishop_rook_attacks, king_attacks, knight_attacks, pawn_attacks, queen_attacks,
    rook_attacks,
};
use crate::types::{Bitboard, Color, Move, PieceType, Square};

impl Board {
    pub fn gives_direct_check(&self, m: Move) -> bool {
        let us = self.side();
        let them = us.flip();
        let ksq = self.king_sq(them);
        let from = m.from();
        let to = m.to();

        let pt = m.promotion().unwrap_or_else(|| {
            self.piece_on(from)
                .map_or(PieceType::Pawn, |p| p.piece_type())
        });

        let mut occ = self.occupancy();
        occ.clear(from);
        occ.set(to);

        match pt {
            PieceType::Pawn => pawn_attacks(us, to).contains(ksq),
            PieceType::Knight => knight_attacks(to).contains(ksq),
            PieceType::Bishop => bishop_attacks(to, occ).contains(ksq),
            PieceType::Rook => rook_attacks(to, occ).contains(ksq),
            PieceType::Queen => queen_attacks(to, occ).contains(ksq),
            PieceType::King => {
                if m.is_castle() {
                    let (rfrom, rto) = match to.raw() {
                        6 => (7u8, 5u8),
                        2 => (0, 3),
                        62 => (63, 61),
                        _ => (56, 59),
                    };
                    occ.clear(Square::new(rfrom));
                    occ.set(Square::new(rto));
                    rook_attacks(Square::new(rto), occ).contains(ksq)
                } else {
                    false
                }
            }
        }
    }

    pub fn attackers_to(&self, sq: Square, occ: Bitboard) -> Bitboard {
        let bishops = self.by_type(PieceType::Bishop) | self.by_type(PieceType::Queen);
        let rooks = self.by_type(PieceType::Rook) | self.by_type(PieceType::Queen);
        let (diagonal, orthogonal) = bishop_rook_attacks(sq, occ);

        (pawn_attacks(Color::White, sq) & self.pieces(Color::Black, PieceType::Pawn))
            | (pawn_attacks(Color::Black, sq) & self.pieces(Color::White, PieceType::Pawn))
            | (knight_attacks(sq) & self.by_type(PieceType::Knight))
            | (king_attacks(sq) & self.by_type(PieceType::King))
            | (diagonal & bishops)
            | (orthogonal & rooks)
    }

    // SEE against a threshold. Exchanges on the back ranks, where a pawn
    // recapture also promotes, go through the slower promotion-aware path.
    pub fn see(&self, mv: Move, threshold: i32) -> bool {
        if mv.is_castle() {
            return threshold <= 0;
        }

        let to = mv.to();
        let from = mv.from();

        if self.has_promotion_recapture(to) {
            return self.see_exchange_value(mv) >= threshold;
        }

        let mut balance = self.see_gain(mv) - threshold;
        if balance < 0 {
            return false;
        }

        let next_victim = if let Some(promo) = mv.promotion() {
            promo.see_value()
        } else {
            self.piece_on(from).unwrap().piece_type().see_value()
        };

        balance -= next_victim;
        if balance >= 0 {
            return true;
        }
        let mut occ = self.occupancy();
        occ.clear(from);
        if mv.is_en_passant() {
            occ.clear(Square::new(to.raw() ^ 8));
        }
        occ.set(to);

        let bishops = self.by_type(PieceType::Bishop) | self.by_type(PieceType::Queen);
        let rooks = self.by_type(PieceType::Rook) | self.by_type(PieceType::Queen);

        let mut attackers = self.attackers_to(to, occ) & occ;
        let mut stm = self.side().flip();

        loop {
            let our = attackers & self.color_bb(stm);
            let Some((attacker, from)) = self.least_valuable_legal_attacker(our, stm, to, occ)
            else {
                break;
            };

            occ.clear(from);
            stm = stm.flip();

            balance = -balance - 1 - attacker.see_value();
            if balance >= 0 || attacker == PieceType::King {
                break;
            }

            if matches!(
                attacker,
                PieceType::Pawn | PieceType::Bishop | PieceType::Queen
            ) {
                attackers |= bishop_attacks(to, occ) & bishops;
            }
            if matches!(attacker, PieceType::Rook | PieceType::Queen) {
                attackers |= rook_attacks(to, occ) & rooks;
            }
            attackers &= occ;
        }

        stm != self.side()
    }

    fn has_promotion_recapture(&self, target: Square) -> bool {
        match target.rank() {
            7 => (pawn_attacks(Color::Black, target) & self.pieces(Color::White, PieceType::Pawn))
                .any(),
            0 => (pawn_attacks(Color::White, target) & self.pieces(Color::Black, PieceType::Pawn))
                .any(),
            _ => false,
        }
    }

    fn see_exchange_value(&self, mv: Move) -> i32 {
        let target = mv.to();
        let from = mv.from();
        let mut gains = [0i32; 32];
        gains[0] = self.see_gain(mv);

        let mut victim = mv
            .promotion()
            .unwrap_or_else(|| self.piece_on(from).unwrap().piece_type());
        let mut occ = self.occupancy();
        occ.clear(from);
        if mv.is_en_passant() {
            occ.clear(Square::new(target.raw() ^ 8));
        }
        occ.set(target);

        let bishops = self.by_type(PieceType::Bishop) | self.by_type(PieceType::Queen);
        let rooks = self.by_type(PieceType::Rook) | self.by_type(PieceType::Queen);
        let mut attackers = self.attackers_to(target, occ) & occ;
        let mut stm = self.side().flip();
        let mut depth = 0usize;

        loop {
            let ours = attackers & self.color_bb(stm);
            let Some((attacker, attacker_from)) =
                self.least_valuable_legal_attacker(ours, stm, target, occ)
            else {
                break;
            };

            depth += 1;
            let promoted = attacker == PieceType::Pawn
                && ((stm == Color::White && target.rank() == 7)
                    || (stm == Color::Black && target.rank() == 0));
            let result = if promoted { PieceType::Queen } else { attacker };
            gains[depth] =
                victim.see_value() + result.see_value() - attacker.see_value() - gains[depth - 1];
            victim = result;

            occ.clear(attacker_from);
            if attacker == PieceType::King {
                break;
            }
            if matches!(
                attacker,
                PieceType::Pawn | PieceType::Bishop | PieceType::Queen
            ) {
                attackers |= bishop_attacks(target, occ) & bishops;
            }
            if matches!(attacker, PieceType::Rook | PieceType::Queen) {
                attackers |= rook_attacks(target, occ) & rooks;
            }
            attackers &= occ;
            stm = stm.flip();
        }

        while depth > 0 {
            gains[depth - 1] = -(-gains[depth - 1]).max(gains[depth]);
            depth -= 1;
        }
        gains[0]
    }

    fn see_gain(&self, mv: Move) -> i32 {
        let mut value = if mv.is_en_passant() {
            PieceType::Pawn.see_value()
        } else {
            self.piece_on(mv.to())
                .map_or(0, |p| p.piece_type().see_value())
        };

        if let Some(promo) = mv.promotion() {
            value += promo.see_value() - PieceType::Pawn.see_value();
        }
        value
    }

    fn least_valuable_legal_attacker(
        &self,
        attackers: Bitboard,
        stm: Color,
        target: Square,
        occ: Bitboard,
    ) -> Option<(PieceType, Square)> {
        if attackers.is_empty() {
            return None;
        }
        let king = self.king_sq(stm);
        let them = stm.flip();
        for pt in PieceType::ALL {
            for from in self.pieces(stm, pt) & attackers {
                let mut after = occ;
                after.clear(from);
                let king_after = if pt == PieceType::King { target } else { king };
                if !self.see_square_attacked(king_after, them, after, target) {
                    return Some((pt, from));
                }
            }
        }
        None
    }

    fn see_square_attacked(
        &self,
        sq: Square,
        by: Color,
        occ: Bitboard,
        captured_target: Square,
    ) -> bool {
        let active = occ & !captured_target.bb();
        if (pawn_attacks(by.flip(), sq) & self.pieces(by, PieceType::Pawn) & active).any()
            || (knight_attacks(sq) & self.pieces(by, PieceType::Knight) & active).any()
            || (king_attacks(sq) & self.pieces(by, PieceType::King) & active).any()
        {
            return true;
        }
        let (diagonal, orthogonal) = bishop_rook_attacks(sq, occ);
        let bishops = self.pieces(by, PieceType::Bishop) | self.pieces(by, PieceType::Queen);
        let rooks = self.pieces(by, PieceType::Rook) | self.pieces(by, PieceType::Queen);
        (diagonal & bishops & active).any() || (orthogonal & rooks & active).any()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(board: &mut Board, uci: &str) -> Move {
        board
            .generate_legal()
            .as_slice()
            .iter()
            .copied()
            .find(|m| m.to_uci() == uci)
            .expect("legal test move")
    }

    #[test]
    fn castling_has_zero_see_gain() {
        let mut board = Board::from_fen("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
        let mv = find(&mut board, "e1g1");
        assert!(board.see(mv, 0));
        assert!(!board.see(mv, 1));
    }

    #[test]
    fn en_passant_values_the_captured_pawn() {
        let mut board = Board::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
        let mv = find(&mut board, "e5d6");
        assert!(board.see(mv, PieceType::Pawn.see_value()));
        assert!(!board.see(mv, PieceType::Pawn.see_value() + 1));
    }

    #[test]
    fn quiet_promotion_counts_the_material_gain() {
        let mut board = Board::from_fen("7k/P7/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mv = find(&mut board, "a7a8q");
        let gain = PieceType::Queen.see_value() - PieceType::Pawn.see_value();
        assert!(board.see(mv, gain));
        assert!(!board.see(mv, gain + 1));
    }

    #[test]
    fn pinned_defender_cannot_recapture() {
        let mut board = Board::from_fen("4k3/4n3/8/3p4/5N2/8/8/4R1K1 w - - 0 1").unwrap();
        let mv = find(&mut board, "f4d5");
        assert!(board.see(mv, PieceType::Pawn.see_value()));
        assert!(!board.see(mv, PieceType::Pawn.see_value() + 1));
    }

    #[test]
    fn pinned_defender_can_recapture_along_the_pin() {
        let mut board = Board::from_fen("4k3/4r3/4p3/5Q2/8/8/8/4R1K1 w - - 0 1").unwrap();
        let mv = find(&mut board, "f5e6");
        let gain = PieceType::Pawn.see_value() - PieceType::Queen.see_value()
            + PieceType::Rook.see_value();
        assert!(board.see(mv, gain));
        assert!(!board.see(mv, gain + 1));
    }

    #[test]
    fn a_recapture_must_resolve_a_discovered_check() {
        for (fen, uci) in [
            ("4k3/8/5n2/3p4/4B3/8/8/K3R3 w - - 0 1", "e4d5"),
            ("k3r3/8/8/4b3/3P4/5N2/8/4K3 b - - 0 1", "e5d4"),
            ("6k1/8/8/3pPn2/8/8/B7/K7 w - d6 0 1", "e5d6"),
        ] {
            let mut board = Board::from_fen(fen).unwrap();
            let mv = find(&mut board, uci);
            let before = board.to_fen();
            let key = board.key();
            for threshold in [-74, -29, 0, 1, 100] {
                assert!(board.see(mv, threshold), "{uci} at {threshold}: {fen}");
            }
            assert!(!board.see(mv, 101), "{uci}: {fen}");
            assert_eq!(board.to_fen(), before);
            assert_eq!(board.key(), key);
        }
    }

    #[test]
    fn a_later_exchange_cannot_ignore_its_king_in_check() {
        let mut board = Board::from_fen("k3r3/8/8/4b3/3p4/5N2/8/3QK3 w - - 0 1").unwrap();
        let mv = find(&mut board, "d1d4");
        let loss = PieceType::Pawn.see_value() - PieceType::Queen.see_value();
        assert!(board.see(mv, loss));
        assert!(!board.see(mv, loss + 1));
        assert!(!board.see(mv, -1000));
    }

    #[test]
    fn king_recaptures_are_checked_on_the_destination_square() {
        for (fen, gain) in [
            (
                "4k3/4p3/8/8/1B6/8/8/K3R3 w - - 0 1",
                PieceType::Pawn.see_value(),
            ),
            (
                "4k3/4p3/8/8/8/8/8/K3R3 w - - 0 1",
                PieceType::Pawn.see_value() - PieceType::Rook.see_value(),
            ),
        ] {
            let mut board = Board::from_fen(fen).unwrap();
            let mv = find(&mut board, "e1e7");
            assert!(board.see(mv, gain), "{fen}");
            assert!(!board.see(mv, gain + 1), "{fen}");
        }
    }
}
