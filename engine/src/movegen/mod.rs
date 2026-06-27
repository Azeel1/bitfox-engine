mod magic;

pub use magic::{
    bishop_attacks, king_attacks, knight_attacks, pawn_attacks, queen_attacks, rook_attacks,
};

use crate::board::Board;
use crate::types::{Bitboard, Color, Move, PieceType, Square};

pub struct MoveList {
    pub moves: [Move; 256],
    pub len: usize,
}

impl MoveList {
    #[inline]
    pub fn new() -> MoveList {
        MoveList {
            moves: [Move::NONE; 256],
            len: 0,
        }
    }

    #[inline]
    pub fn push(&mut self, m: Move) {
        self.moves[self.len] = m;
        self.len += 1;
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn as_slice(&self) -> &[Move] {
        &self.moves[..self.len]
    }
}

impl Default for MoveList {
    fn default() -> MoveList {
        MoveList::new()
    }
}

const PROMO_PUSH: [u16; 4] = [
    Move::QUEEN_PROMO,
    Move::ROOK_PROMO,
    Move::BISHOP_PROMO,
    Move::KNIGHT_PROMO,
];
const PROMO_CAPTURE: [u16; 4] = [
    Move::QUEEN_PROMO_CAPTURE,
    Move::ROOK_PROMO_CAPTURE,
    Move::BISHOP_PROMO_CAPTURE,
    Move::KNIGHT_PROMO_CAPTURE,
];

impl Board {
    pub fn is_attacked(&self, sq: Square, by: Color) -> bool {
        if (pawn_attacks(by.flip(), sq) & self.pieces(by, PieceType::Pawn)).any() {
            return true;
        }
        if (knight_attacks(sq) & self.pieces(by, PieceType::Knight)).any() {
            return true;
        }
        if (king_attacks(sq) & self.pieces(by, PieceType::King)).any() {
            return true;
        }
        let occ = self.occupancy();
        let bishops = self.pieces(by, PieceType::Bishop) | self.pieces(by, PieceType::Queen);
        if (bishop_attacks(sq, occ) & bishops).any() {
            return true;
        }
        let rooks = self.pieces(by, PieceType::Rook) | self.pieces(by, PieceType::Queen);
        (rook_attacks(sq, occ) & rooks).any()
    }

    #[inline]
    pub fn in_check(&self, color: Color) -> bool {
        self.is_attacked(self.king_sq(color), color.flip())
    }

    pub fn generate_pseudo(&self, list: &mut MoveList) {
        let us = self.side();
        let them = us.flip();
        let occ = self.occupancy();
        let own = self.color_bb(us);
        let opp = self.color_bb(them);
        let enemy_king = self.pieces(them, PieceType::King);
        let targets = !(own | enemy_king);

        self.gen_pawns(list, us, occ, opp, enemy_king);
        self.gen_piece(list, knight_attacks_wrap, self.pieces(us, PieceType::Knight), occ, opp, targets);
        self.gen_piece(list, bishop_attacks, self.pieces(us, PieceType::Bishop), occ, opp, targets);
        self.gen_piece(list, rook_attacks, self.pieces(us, PieceType::Rook), occ, opp, targets);
        self.gen_piece(list, queen_attacks, self.pieces(us, PieceType::Queen), occ, opp, targets);
        self.gen_king(list, us, them, occ, opp, targets);
    }

    fn gen_pawns(
        &self,
        list: &mut MoveList,
        us: Color,
        occ: Bitboard,
        opp: Bitboard,
        enemy_king: Bitboard,
    ) {
        let promo_rank = if us == Color::White { 7 } else { 0 };
        let start_rank = if us == Color::White { 1 } else { 6 };
        let forward: i8 = if us == Color::White { 8 } else { -8 };
        let cap_targets = opp & !enemy_king;

        for from in self.pieces(us, PieceType::Pawn) {
            let one = Square::new((from.raw() as i8 + forward) as u8);
            if !occ.contains(one) {
                if one.rank() == promo_rank {
                    for &flag in &PROMO_PUSH {
                        list.push(Move::new(from, one, flag));
                    }
                } else {
                    list.push(Move::new(from, one, Move::QUIET));
                    if from.rank() == start_rank {
                        let two = Square::new((from.raw() as i8 + 2 * forward) as u8);
                        if !occ.contains(two) {
                            list.push(Move::new(from, two, Move::DOUBLE_PAWN));
                        }
                    }
                }
            }

            for to in pawn_attacks(us, from) & cap_targets {
                if to.rank() == promo_rank {
                    for &flag in &PROMO_CAPTURE {
                        list.push(Move::new(from, to, flag));
                    }
                } else {
                    list.push(Move::new(from, to, Move::CAPTURE));
                }
            }

            if let Some(ep) = self.ep() {
                if pawn_attacks(us, from).contains(ep) {
                    list.push(Move::new(from, ep, Move::EN_PASSANT));
                }
            }
        }
    }

    fn gen_piece(
        &self,
        list: &mut MoveList,
        attacks: fn(Square, Bitboard) -> Bitboard,
        pieces: Bitboard,
        occ: Bitboard,
        opp: Bitboard,
        targets: Bitboard,
    ) {
        for from in pieces {
            for to in attacks(from, occ) & targets {
                let flag = if opp.contains(to) {
                    Move::CAPTURE
                } else {
                    Move::QUIET
                };
                list.push(Move::new(from, to, flag));
            }
        }
    }

    fn gen_king(
        &self,
        list: &mut MoveList,
        us: Color,
        them: Color,
        occ: Bitboard,
        opp: Bitboard,
        targets: Bitboard,
    ) {
        let from = self.king_sq(us);
        for to in king_attacks(from) & targets {
            let flag = if opp.contains(to) {
                Move::CAPTURE
            } else {
                Move::QUIET
            };
            list.push(Move::new(from, to, flag));
        }

        if self.is_attacked(from, them) {
            return;
        }
        let castling = self.castling();
        let between = |a: u8, b: u8| -> bool {
            let mut bb = Bitboard::EMPTY;
            bb.set(Square::new(a));
            bb.set(Square::new(b));
            (occ & bb).is_empty()
        };
        if us == Color::White {
            if castling.has(crate::types::Castling::WHITE_KING)
                && between(5, 6)
                && !self.is_attacked(Square::new(5), them)
                && !self.is_attacked(Square::new(6), them)
            {
                list.push(Move::new(Square::new(4), Square::new(6), Move::KING_CASTLE));
            }
            if castling.has(crate::types::Castling::WHITE_QUEEN)
                && (occ & {
                    let mut bb = Bitboard::EMPTY;
                    bb.set(Square::new(1));
                    bb.set(Square::new(2));
                    bb.set(Square::new(3));
                    bb
                })
                .is_empty()
                && !self.is_attacked(Square::new(3), them)
                && !self.is_attacked(Square::new(2), them)
            {
                list.push(Move::new(Square::new(4), Square::new(2), Move::QUEEN_CASTLE));
            }
        } else {
            if castling.has(crate::types::Castling::BLACK_KING)
                && between(61, 62)
                && !self.is_attacked(Square::new(61), them)
                && !self.is_attacked(Square::new(62), them)
            {
                list.push(Move::new(Square::new(60), Square::new(62), Move::KING_CASTLE));
            }
            if castling.has(crate::types::Castling::BLACK_QUEEN)
                && (occ & {
                    let mut bb = Bitboard::EMPTY;
                    bb.set(Square::new(57));
                    bb.set(Square::new(58));
                    bb.set(Square::new(59));
                    bb
                })
                .is_empty()
                && !self.is_attacked(Square::new(59), them)
                && !self.is_attacked(Square::new(58), them)
            {
                list.push(Move::new(Square::new(60), Square::new(58), Move::QUEEN_CASTLE));
            }
        }
    }

    pub fn generate_captures(&self, list: &mut MoveList) {
        let us = self.side();
        let them = us.flip();
        let occ = self.occupancy();
        let opp = self.color_bb(them);
        let enemy_king = self.pieces(them, PieceType::King);
        let cap = opp & !enemy_king;
        let promo_rank = if us == Color::White { 7 } else { 0 };
        let forward: i8 = if us == Color::White { 8 } else { -8 };

        for from in self.pieces(us, PieceType::Pawn) {
            let one = Square::new((from.raw() as i8 + forward) as u8);
            if one.rank() == promo_rank && !occ.contains(one) {
                for &flag in &PROMO_PUSH {
                    list.push(Move::new(from, one, flag));
                }
            }
            for to in pawn_attacks(us, from) & cap {
                if to.rank() == promo_rank {
                    for &flag in &PROMO_CAPTURE {
                        list.push(Move::new(from, to, flag));
                    }
                } else {
                    list.push(Move::new(from, to, Move::CAPTURE));
                }
            }
            if let Some(ep) = self.ep() {
                if pawn_attacks(us, from).contains(ep) {
                    list.push(Move::new(from, ep, Move::EN_PASSANT));
                }
            }
        }
        for from in self.pieces(us, PieceType::Knight) {
            for to in knight_attacks(from) & cap {
                list.push(Move::new(from, to, Move::CAPTURE));
            }
        }
        for from in self.pieces(us, PieceType::Bishop) {
            for to in bishop_attacks(from, occ) & cap {
                list.push(Move::new(from, to, Move::CAPTURE));
            }
        }
        for from in self.pieces(us, PieceType::Rook) {
            for to in rook_attacks(from, occ) & cap {
                list.push(Move::new(from, to, Move::CAPTURE));
            }
        }
        for from in self.pieces(us, PieceType::Queen) {
            for to in queen_attacks(from, occ) & cap {
                list.push(Move::new(from, to, Move::CAPTURE));
            }
        }
        for to in king_attacks(self.king_sq(us)) & cap {
            list.push(Move::new(self.king_sq(us), to, Move::CAPTURE));
        }
    }

    pub fn generate_legal(&mut self) -> MoveList {
        let mut pseudo = MoveList::new();
        self.generate_pseudo(&mut pseudo);
        let us = self.side();
        let mut legal = MoveList::new();
        for i in 0..pseudo.len {
            let m = pseudo.moves[i];
            self.make_move(m);
            if !self.is_attacked(self.king_sq(us), self.side()) {
                legal.push(m);
            }
            self.unmake_move(m);
        }
        legal
    }

    pub fn perft(&mut self, depth: u32) -> u64 {
        if depth == 0 {
            return 1;
        }
        let mut list = MoveList::new();
        self.generate_pseudo(&mut list);
        let us = self.side();
        let mut nodes = 0u64;
        for i in 0..list.len {
            let m = list.moves[i];
            self.make_move(m);
            if !self.is_attacked(self.king_sq(us), self.side()) {
                nodes += if depth == 1 { 1 } else { self.perft(depth - 1) };
            }
            self.unmake_move(m);
        }
        nodes
    }
}

fn knight_attacks_wrap(sq: Square, _occ: Bitboard) -> Bitboard {
    knight_attacks(sq)
}
