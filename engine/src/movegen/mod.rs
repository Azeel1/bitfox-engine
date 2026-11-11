mod magic;
mod magic_numbers;

pub use magic::{
    bishop_attacks, bishop_rook_attacks, king_attacks, knight_attacks, pawn_attacks, queen_attacks,
    rook_attacks, warmup,
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
        let (diagonal, orthogonal) = bishop_rook_attacks(sq, occ);
        if (diagonal & bishops).any() {
            return true;
        }
        let rooks = self.pieces(by, PieceType::Rook) | self.pieces(by, PieceType::Queen);
        (orthogonal & rooks).any()
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
        self.gen_piece(
            list,
            knight_attacks_wrap,
            self.pieces(us, PieceType::Knight),
            occ,
            opp,
            targets,
        );
        self.gen_piece(
            list,
            bishop_attacks,
            self.pieces(us, PieceType::Bishop),
            occ,
            opp,
            targets,
        );
        self.gen_piece(
            list,
            rook_attacks,
            self.pieces(us, PieceType::Rook),
            occ,
            opp,
            targets,
        );
        self.gen_piece(
            list,
            queen_attacks,
            self.pieces(us, PieceType::Queen),
            occ,
            opp,
            targets,
        );
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
            let attacks = pawn_attacks(us, from);
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

            for to in attacks & cap_targets {
                if to.rank() == promo_rank {
                    for &flag in &PROMO_CAPTURE {
                        list.push(Move::new(from, to, flag));
                    }
                } else {
                    list.push(Move::new(from, to, Move::CAPTURE));
                }
            }

            if let Some(ep) = self.ep() {
                if attacks.contains(ep) {
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
                list.push(Move::new(
                    Square::new(4),
                    Square::new(2),
                    Move::QUEEN_CASTLE,
                ));
            }
        } else {
            if castling.has(crate::types::Castling::BLACK_KING)
                && between(61, 62)
                && !self.is_attacked(Square::new(61), them)
                && !self.is_attacked(Square::new(62), them)
            {
                list.push(Move::new(
                    Square::new(60),
                    Square::new(62),
                    Move::KING_CASTLE,
                ));
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
                list.push(Move::new(
                    Square::new(60),
                    Square::new(58),
                    Move::QUEEN_CASTLE,
                ));
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
            let attacks = pawn_attacks(us, from);
            if one.rank() == promo_rank && !occ.contains(one) {
                for &flag in &PROMO_PUSH {
                    list.push(Move::new(from, one, flag));
                }
            }
            for to in attacks & cap {
                if to.rank() == promo_rank {
                    for &flag in &PROMO_CAPTURE {
                        list.push(Move::new(from, to, flag));
                    }
                } else {
                    list.push(Move::new(from, to, Move::CAPTURE));
                }
            }
            if let Some(ep) = self.ep() {
                if attacks.contains(ep) {
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

    pub(crate) fn has_legal_move(&mut self) -> bool {
        let us = self.side();
        let occ = self.occupancy();
        let step = if us == Color::White { 8i8 } else { -8i8 };
        for from in self.pieces(us, PieceType::Pawn) {
            let to = Square::new((from.raw() as i8 + step) as u8);
            if occ.contains(to) {
                continue;
            }
            let next_occ = (occ & !from.bb()) | to.bb();
            if (self.attackers_to(self.king_sq(us), next_occ) & self.color_bb(us.flip())).is_empty()
            {
                return true;
            }
        }
        let mut pseudo = MoveList::new();
        self.generate_pseudo(&mut pseudo);
        for &m in pseudo.as_slice() {
            self.make_move(m);
            let legal = !self.is_attacked(self.king_sq(us), self.side());
            self.unmake_move(m);
            if legal {
                return true;
            }
        }
        false
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ucis(list: &MoveList) -> Vec<String> {
        list.as_slice().iter().map(|m| m.to_uci()).collect()
    }

    #[test]
    fn pawn_push_existence_respects_pins_checks_and_promotions() {
        let keys = |board: &Board| {
            [
                board.key(),
                board.pawn_key(),
                board.non_pawn_key(Color::White),
                board.non_pawn_key(Color::Black),
                board.minor_key(),
                board.major_key(),
            ]
        };
        for (fen, pawn_move, legal_push, has_move) in [
            ("8/8/8/8/2bb4/8/1Pk5/K7 w - - 0 1", "b2b3", false, false),
            ("k7/1pK5/8/2BB4/8/8/8/8 b - - 0 1", "b7b6", false, false),
            ("4r2k/8/8/8/8/8/4P3/4K3 w - - 0 1", "e2e3", true, true),
            ("7k/8/8/1b6/8/8/3P4/5K2 w - - 0 1", "d2d3", true, true),
            ("K6r/1Pk5/8/2b5/8/8/8/8 w - - 0 1", "b7b8q", true, true),
            ("8/8/8/8/2B5/8/1pK5/k6R b - - 0 1", "b2b1q", true, true),
            ("7k/p5Q1/5K2/8/8/8/8/8 b - - 0 1", "a7a6", false, false),
        ] {
            let mut board = Board::from_fen(fen).unwrap();
            let before_fen = board.to_fen();
            let before_keys = keys(&board);
            let legal = ucis(&board.generate_legal());
            assert_eq!(legal.iter().any(|m| m == pawn_move), legal_push, "{fen}");
            assert_eq!(!legal.is_empty(), has_move, "{fen}");
            for _ in 0..2 {
                assert_eq!(board.has_legal_move(), has_move, "{fen}");
                assert_eq!(board.to_fen(), before_fen, "{fen}");
                assert_eq!(keys(&board), before_keys, "{fen}");
                assert_eq!(ucis(&board.generate_legal()), legal, "{fen}");
            }
        }
    }

    #[test]
    fn terminal_fast_move_existence_matches_full_generation() {
        for fen in [
            Board::START_FEN,
            "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1",
            "7k/6Q1/5K2/8/8/8/8/8 b - - 100 1",
            "4r1k1/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
            "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
            "7k/P7/8/8/8/8/8/K7 w - - 0 1",
            "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
            "r4k2/8/8/8/8/8/7Q/K7 w - - 96 1",
        ] {
            let mut board = Board::from_fen(fen).unwrap();
            let before_fen = board.to_fen();
            let before_key = board.key();
            let expected = !board.generate_legal().is_empty();
            assert_eq!(board.has_legal_move(), expected, "{fen}");
            assert_eq!(board.to_fen(), before_fen);
            assert_eq!(board.key(), before_key);
            assert_eq!(board.has_legal_move(), expected, "repeated {fen}");
        }
    }

    #[test]
    fn legal_generation_restores_the_board() {
        let mut board = Board::startpos();
        let fen = board.to_fen();
        let key = board.key();
        let first = ucis(&board.generate_legal());
        let second = ucis(&board.generate_legal());
        assert_eq!(first, second);
        assert_eq!(board.to_fen(), fen);
        assert_eq!(board.key(), key);
    }

    #[test]
    fn pinned_en_passant_is_not_legal() {
        let mut board = Board::from_fen("4r1k1/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
        let mut pseudo = MoveList::new();
        board.generate_pseudo(&mut pseudo);
        assert!(ucis(&pseudo).iter().any(|m| m == "e5d6"));
        assert!(!ucis(&board.generate_legal()).iter().any(|m| m == "e5d6"));
    }

    #[test]
    fn castling_through_an_attacked_square_is_not_generated() {
        let board = Board::from_fen("4kr2/8/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
        let mut pseudo = MoveList::new();
        board.generate_pseudo(&mut pseudo);
        let moves = ucis(&pseudo);
        assert!(!moves.iter().any(|m| m == "e1g1"));
        assert!(moves.iter().any(|m| m == "e1c1"));
    }

    #[test]
    fn moves_never_capture_the_enemy_king() {
        let mut board = Board::from_fen("8/8/8/8/8/4k3/8/4K3 w - - 0 1").unwrap();
        let king = crate::types::Piece::new(Color::Black, PieceType::King);
        board.remove(king, Square::new(20));
        board.put(king, Square::new(12));
        let mut pseudo = MoveList::new();
        board.generate_pseudo(&mut pseudo);
        assert!(!ucis(&pseudo).iter().any(|m| m == "e1e2"));
    }
}
