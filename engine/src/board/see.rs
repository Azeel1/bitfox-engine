use super::Board;
use crate::movegen::{
    bishop_attacks, king_attacks, knight_attacks, pawn_attacks, queen_attacks, rook_attacks,
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

        (pawn_attacks(Color::White, sq) & self.pieces(Color::Black, PieceType::Pawn))
            | (pawn_attacks(Color::Black, sq) & self.pieces(Color::White, PieceType::Pawn))
            | (knight_attacks(sq) & self.by_type(PieceType::Knight))
            | (king_attacks(sq) & self.by_type(PieceType::King))
            | (bishop_attacks(sq, occ) & bishops)
            | (rook_attacks(sq, occ) & rooks)
    }

    pub fn see(&self, mv: Move, threshold: i32) -> bool {
        if mv.is_castle() {
            return true;
        }

        let to = mv.to();
        let from = mv.from();

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

        let bishops = self.by_type(PieceType::Bishop) | self.by_type(PieceType::Queen);
        let rooks = self.by_type(PieceType::Rook) | self.by_type(PieceType::Queen);

        let mut attackers = self.attackers_to(to, occ) & occ;
        let mut stm = self.side().flip();

        loop {
            let our = attackers & self.color_bb(stm);
            if our.is_empty() {
                break;
            }

            let attacker = self.least_valuable_attacker(our, stm);

            if attacker == PieceType::King && (attackers & self.color_bb(stm.flip())).any() {
                break;
            }

            occ.clear((self.pieces(stm, attacker) & our).lsb());
            stm = stm.flip();

            balance = -balance - 1 - attacker.see_value();
            if balance >= 0 {
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

    fn least_valuable_attacker(&self, attackers: Bitboard, stm: Color) -> PieceType {
        for pt in PieceType::ALL {
            if (self.pieces(stm, pt) & attackers).any() {
                return pt;
            }
        }
        unreachable!("least_valuable_attacker called with empty intersection")
    }
}
