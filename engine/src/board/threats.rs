use super::Board;
use crate::movegen::{
    bishop_attacks, king_attacks, knight_attacks, pawn_attacks, queen_attacks, rook_attacks,
};
use crate::types::{Bitboard, Color, PieceType};

pub struct ThreatCtx {
    pub all: Bitboard,
    pub threatened: [Bitboard; 6],
    pub check: [Bitboard; 6],
}

impl Board {
    pub fn threats(&self, by: Color) -> Bitboard {
        let occ = self.occupancy();
        let mut attacked = Bitboard::EMPTY;

        for from in self.pieces(by, PieceType::Pawn) {
            attacked |= pawn_attacks(by, from);
        }
        for from in self.pieces(by, PieceType::Knight) {
            attacked |= knight_attacks(from);
        }
        let bishops = self.pieces(by, PieceType::Bishop) | self.pieces(by, PieceType::Queen);
        for from in bishops {
            attacked |= bishop_attacks(from, occ);
        }
        let rooks = self.pieces(by, PieceType::Rook) | self.pieces(by, PieceType::Queen);
        for from in rooks {
            attacked |= rook_attacks(from, occ);
        }
        attacked |= king_attacks(self.king_sq(by));

        attacked
    }

    #[inline]
    pub fn all_threats(&self) -> Bitboard {
        self.threats(self.side().flip())
    }

    pub fn threat_ctx(&self) -> ThreatCtx {
        let them = self.side().flip();
        let occ = self.occupancy();

        let mut pawn = Bitboard::EMPTY;
        for from in self.pieces(them, PieceType::Pawn) {
            pawn |= pawn_attacks(them, from);
        }
        let mut knight = Bitboard::EMPTY;
        for from in self.pieces(them, PieceType::Knight) {
            knight |= knight_attacks(from);
        }
        let mut bishop = Bitboard::EMPTY;
        for from in self.pieces(them, PieceType::Bishop) {
            bishop |= bishop_attacks(from, occ);
        }
        let mut rook = Bitboard::EMPTY;
        for from in self.pieces(them, PieceType::Rook) {
            rook |= rook_attacks(from, occ);
        }
        let mut queen = Bitboard::EMPTY;
        for from in self.pieces(them, PieceType::Queen) {
            queen |= queen_attacks(from, occ);
        }
        let king = king_attacks(self.king_sq(them));

        let all = pawn | knight | bishop | rook | queen | king;
        let minor = pawn | knight | bishop;
        let rook_th = minor | rook;
        let threatened = [Bitboard::EMPTY, pawn, pawn, minor, rook_th, Bitboard::EMPTY];

        let eksq = self.king_sq(them);
        let check = [
            pawn_attacks(them, eksq),
            knight_attacks(eksq),
            bishop_attacks(eksq, occ),
            rook_attacks(eksq, occ),
            queen_attacks(eksq, occ),
            Bitboard::EMPTY,
        ];

        ThreatCtx {
            all,
            threatened,
            check,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::board::Board;
    use crate::types::{Color, Square};

    fn sq(s: &str) -> Square {
        Square::from_str(s).unwrap()
    }

    #[test]
    fn startpos_threats() {
        let board = Board::startpos();
        let white = board.threats(Color::White);
        for s in ["a3", "b3", "c3", "d3", "e3", "f3", "g3", "h3"] {
            assert!(white.contains(sq(s)), "white should attack {s}");
        }
        assert!(white.contains(sq("b1")));
        assert!(white.contains(sq("e2")));
        assert!(!white.contains(sq("a4")));
        assert!(!white.contains(sq("e4")));

        let black = board.threats(Color::Black);
        for s in ["a6", "b6", "c6", "d6", "e6", "f6", "g6", "h6"] {
            assert!(black.contains(sq(s)), "black should attack {s}");
        }
        assert!(black.contains(sq("e7")));
        assert!(!black.contains(sq("e5")));
    }

    #[test]
    fn all_threats_is_opponent() {
        let board = Board::startpos();
        assert_eq!(board.all_threats(), board.threats(Color::Black));
    }

    #[test]
    fn slider_threats() {
        let board = Board::from_fen("4k3/8/8/8/3Q4/8/8/4K3 w - - 0 1").unwrap();
        let white = board.threats(Color::White);
        assert!(white.contains(sq("d8")));
        assert!(white.contains(sq("a4")));
        assert!(white.contains(sq("h4")));
        assert!(white.contains(sq("a1")));
        assert!(white.contains(sq("g7")));
        assert!(white.contains(sq("e2")));
        assert!(!white.contains(sq("c6")));
    }

    #[test]
    fn blocked_slider_threats() {
        let board = Board::from_fen("4k3/8/8/8/8/8/1P6/R3K3 w - - 0 1").unwrap();
        let white = board.threats(Color::White);
        assert!(white.contains(sq("b1")));
        assert!(white.contains(sq("c1")));
        assert!(white.contains(sq("d1")));
        assert!(white.contains(sq("a2")));
        assert!(white.contains(sq("a8")));
        assert!(white.contains(sq("a3")));
        assert!(white.contains(sq("c3")));
        assert!(!white.contains(sq("b3")));
    }
}
