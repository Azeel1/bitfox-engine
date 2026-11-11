mod fen;
mod make;
mod see;
mod threats;
mod zobrist;

pub use threats::ThreatCtx;

use crate::movegen::{bishop_rook_attacks, king_attacks, knight_attacks, pawn_attacks};
use crate::types::{Bitboard, Castling, Color, Piece, PieceType, Square};

#[derive(Clone, Copy)]
pub(crate) struct Undo {
    pub captured: Option<Piece>,
    pub ep: Option<Square>,
    pub castling: Castling,
    pub halfmove: u16,
    pub fullmove: u16,
    pub key: u64,
    pub repetition_start: usize,
}

#[derive(Clone)]
pub struct Board {
    pieces: [Bitboard; 6],
    colors: [Bitboard; 2],
    mailbox: [Option<Piece>; 64],
    king_squares: [Square; 2],
    side: Color,
    ep: Option<Square>,
    castling: Castling,
    halfmove: u16,
    fullmove: u16,
    key: u64,
    pawn_key: u64,
    non_pawn_key: [u64; 2],
    minor_key: u64,
    major_key: u64,
    history: Vec<u64>,
    repetition_start: usize,
    stack: Vec<Undo>,
}

impl Board {
    pub const START_FEN: &'static str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    pub(super) fn empty() -> Board {
        Board {
            pieces: [Bitboard::EMPTY; 6],
            colors: [Bitboard::EMPTY; 2],
            mailbox: [None; 64],
            king_squares: [Square::new(0); 2],
            side: Color::White,
            ep: None,
            castling: Castling::default(),
            halfmove: 0,
            fullmove: 1,
            key: 0,
            pawn_key: 0,
            non_pawn_key: [0; 2],
            minor_key: 0,
            major_key: 0,
            history: Vec::with_capacity(256),
            repetition_start: 0,
            stack: Vec::with_capacity(256),
        }
    }

    pub fn startpos() -> Board {
        Board::from_fen(Board::START_FEN).expect("valid start fen")
    }

    #[inline]
    pub(crate) fn put(&mut self, piece: Piece, sq: Square) {
        self.pieces[piece.piece_type().index()] |= sq.bb();
        self.colors[piece.color().index()] |= sq.bb();
        self.mailbox[sq.index()] = Some(piece);
        if piece.piece_type() == PieceType::King {
            self.king_squares[piece.color().index()] = sq;
        }
        self.toggle_keys(piece, sq);
    }

    #[inline]
    pub(crate) fn remove(&mut self, piece: Piece, sq: Square) {
        self.pieces[piece.piece_type().index()] &= !sq.bb();
        self.colors[piece.color().index()] &= !sq.bb();
        self.mailbox[sq.index()] = None;
        self.toggle_keys(piece, sq);
    }

    #[inline]
    fn toggle_keys(&mut self, piece: Piece, sq: Square) {
        let k = zobrist::piece(piece, sq);
        self.key ^= k;
        match piece.piece_type() {
            PieceType::Pawn => self.pawn_key ^= k,
            PieceType::Knight | PieceType::Bishop => {
                self.non_pawn_key[piece.color().index()] ^= k;
                self.minor_key ^= k;
            }
            PieceType::Rook | PieceType::Queen => {
                self.non_pawn_key[piece.color().index()] ^= k;
                self.major_key ^= k;
            }
            PieceType::King => {
                self.non_pawn_key[piece.color().index()] ^= k;
                self.minor_key ^= k;
                self.major_key ^= k;
            }
        }
    }

    #[inline]
    pub fn side(&self) -> Color {
        self.side
    }

    #[inline]
    pub fn occupancy(&self) -> Bitboard {
        self.colors[0] | self.colors[1]
    }

    #[inline]
    pub fn color_bb(&self, color: Color) -> Bitboard {
        self.colors[color.index()]
    }

    #[inline]
    pub fn pieces(&self, color: Color, pt: PieceType) -> Bitboard {
        self.pieces[pt.index()] & self.colors[color.index()]
    }

    #[inline]
    pub fn by_type(&self, pt: PieceType) -> Bitboard {
        self.pieces[pt.index()]
    }

    #[inline]
    pub fn piece_on(&self, sq: Square) -> Option<Piece> {
        self.mailbox[sq.index()]
    }

    #[inline]
    pub fn king_sq(&self, color: Color) -> Square {
        self.king_squares[color.index()]
    }

    #[inline]
    pub fn has_non_pawn(&self, color: Color) -> bool {
        (self.color_bb(color) & !(self.by_type(PieceType::Pawn) | self.by_type(PieceType::King)))
            .any()
    }

    #[inline]
    pub fn ep(&self) -> Option<Square> {
        self.ep
    }

    pub(crate) fn ep_hash_file(&self, side: Color) -> Option<u8> {
        let ep = self.ep?;
        let captured = Square::new(ep.raw() ^ 8);
        let them = side.flip();
        if self.piece_on(ep).is_some()
            || self.piece_on(captured) != Some(Piece::new(them, PieceType::Pawn))
        {
            return None;
        }

        let candidates = pawn_attacks(side.flip(), ep) & self.pieces(side, PieceType::Pawn);
        for from in candidates {
            let mut occ = self.occupancy();
            occ.clear(from);
            occ.clear(captured);
            occ.set(ep);

            let king = self.king_sq(side);
            let enemy_pawns = self.pieces(them, PieceType::Pawn) & !captured.bb();
            if (pawn_attacks(side, king) & enemy_pawns).any()
                || (knight_attacks(king) & self.pieces(them, PieceType::Knight)).any()
                || (king_attacks(king) & self.pieces(them, PieceType::King)).any()
            {
                continue;
            }
            let (diagonal, orthogonal) = bishop_rook_attacks(king, occ);
            let bishops =
                self.pieces(them, PieceType::Bishop) | self.pieces(them, PieceType::Queen);
            let rooks = self.pieces(them, PieceType::Rook) | self.pieces(them, PieceType::Queen);
            if (diagonal & bishops).is_empty() && (orthogonal & rooks).is_empty() {
                return Some(ep.file());
            }
        }
        None
    }

    #[inline]
    pub fn castling(&self) -> Castling {
        self.castling
    }

    #[inline]
    pub fn halfmove(&self) -> u16 {
        self.halfmove
    }

    #[inline]
    pub fn key(&self) -> u64 {
        self.key
    }

    #[inline]
    pub fn tt_key(&self) -> u64 {
        let bucket = self.fiftymove_bucket();
        if bucket == 0 {
            return self.key;
        }
        let mut clock = (bucket as u64).wrapping_add(0x9e37_79b9_7f4a_7c15);
        clock = (clock ^ (clock >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        clock = (clock ^ (clock >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        self.key ^ (clock ^ (clock >> 31))
    }

    #[inline]
    pub fn pawn_key(&self) -> u64 {
        self.pawn_key
    }

    #[inline]
    pub fn non_pawn_key(&self, color: Color) -> u64 {
        self.non_pawn_key[color.index()]
    }

    #[inline]
    pub fn minor_key(&self) -> u64 {
        self.minor_key
    }

    #[inline]
    pub fn major_key(&self) -> u64 {
        self.major_key
    }

    #[inline]
    pub fn fiftymove_bucket(&self) -> usize {
        (self.halfmove.saturating_sub(8) as usize / 8).min(15)
    }

    pub fn is_repetition(&self) -> bool {
        let len = self.history.len();
        let window = self.halfmove as usize;
        if len < 5 || window < 4 {
            return false;
        }
        let mut i = len as isize - 3;
        let stop = ((len as isize - 1) - window as isize).max(self.repetition_start as isize);
        while i >= stop && i >= 0 {
            if self.history[i as usize] == self.key {
                return true;
            }
            i -= 2;
        }
        false
    }

    pub fn is_threefold(&self) -> bool {
        let len = self.history.len();
        let window = self.halfmove as usize;
        if len < 9 || window < 8 {
            return false;
        }
        let mut count = 0;
        let mut i = len as isize - 3;
        let stop = ((len as isize - 1) - window as isize).max(self.repetition_start as isize);
        while i >= stop && i >= 0 {
            if self.history[i as usize] == self.key {
                count += 1;
                if count >= 2 {
                    return true;
                }
            }
            i -= 2;
        }
        false
    }

    pub fn is_search_repetition(&self, ply: usize) -> bool {
        let len = self.history.len();
        let window = self.halfmove as usize;
        if len < 5 || window < 4 {
            return false;
        }
        let mut prior_matches = 0;
        let mut i = len as isize - 3;
        let stop = ((len as isize - 1) - window as isize).max(self.repetition_start as isize);
        while i >= stop && i >= 0 {
            if self.history[i as usize] == self.key {
                let distance = len - 1 - i as usize;
                if distance <= ply {
                    return true;
                }
                prior_matches += 1;
                if prior_matches >= 2 {
                    return true;
                }
            }
            i -= 2;
        }
        false
    }

    pub fn insufficient_material(&self) -> bool {
        if (self.pieces[PieceType::Pawn.index()]
            | self.pieces[PieceType::Rook.index()]
            | self.pieces[PieceType::Queen.index()])
        .any()
        {
            return false;
        }
        let knights = self.pieces[PieceType::Knight.index()];
        let bishops = self.pieces[PieceType::Bishop.index()];
        if (knights | bishops).count() <= 1 {
            return true;
        }
        const DARK_SQUARES: u64 = 0xAA55AA55AA55AA55;
        knights.is_empty() && (bishops.0 & DARK_SQUARES == 0 || bishops.0 & !DARK_SQUARES == 0)
    }

    pub fn is_draw(&self) -> bool {
        (self.halfmove >= 100 && (!self.in_check(self.side) || self.clone().has_legal_move()))
            || self.is_threefold()
            || self.insufficient_material()
    }

    pub fn is_forced_draw(&mut self) -> bool {
        if self.is_threefold() || self.insufficient_material() {
            return true;
        }
        self.halfmove >= 100 && self.has_legal_move()
    }

    pub fn is_search_draw(&mut self, ply: usize) -> bool {
        if self.is_search_repetition(ply) || self.insufficient_material() {
            return true;
        }
        self.halfmove >= 100 && self.has_legal_move()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mate_delivered_at_halfmove_100_is_not_a_draw() {
        let mut board = Board::from_fen("7k/6Q1/5K2/8/8/8/8/8 b - - 100 1").unwrap();
        assert!(!board.is_draw());
        assert!(!board.is_forced_draw());
        assert!(board.generate_legal().is_empty());
        assert!(board.in_check(Color::Black));
    }

    #[test]
    fn non_mating_position_at_halfmove_100_is_a_draw() {
        let mut board = Board::from_fen("7k/8/6K1/8/8/8/8/6Q1 b - - 100 1").unwrap();
        assert!(board.is_draw());
        assert!(board.is_forced_draw());
    }

    #[test]
    fn draw_checks_match_complete_move_generation() {
        for position in [
            "7k/6Q1/5K2/8/8/8/8/8 b - -",
            "7k/5Q2/6K1/8/8/8/8/8 b - -",
            "7k/8/6K1/8/8/8/8/6Q1 b - -",
            "4r1k1/8/8/8/8/8/8/4K3 w - -",
            "4r1k1/8/8/8/8/8/3B4/4K3 w - -",
            "r3k2r/8/8/8/8/8/8/R3K2R w KQkq -",
            "4r1k1/8/8/8/8/8/4P3/4K3 w - -",
            "4k3/8/8/8/8/8/8/4K3 w - -",
        ] {
            for clock in [0, 98, 99, 100, 101, u16::MAX] {
                let fen = format!("{position} {clock} 1");
                let mut board = Board::from_fen(&fen).unwrap();
                let before = board.clone();
                let has_legal = !before.clone().generate_legal().is_empty();
                let material_draw = board.insufficient_material();
                let game_draw =
                    material_draw || (clock >= 100 && (!board.in_check(board.side()) || has_legal));
                let forced_draw = material_draw || (clock >= 100 && has_legal);
                for _ in 0..2 {
                    assert_eq!(board.is_draw(), game_draw, "{fen}");
                    assert_eq!(board.is_forced_draw(), forced_draw, "{fen}");
                    assert_eq!(board.is_search_draw(0), forced_draw, "{fen}");
                    assert_eq!(board.to_fen(), before.to_fen());
                    assert_eq!(board.key, before.key);
                    assert_eq!(board.pawn_key, before.pawn_key);
                    assert_eq!(board.non_pawn_key, before.non_pawn_key);
                    assert_eq!(board.minor_key, before.minor_key);
                    assert_eq!(board.major_key, before.major_key);
                    assert_eq!(board.history, before.history);
                    assert_eq!(board.repetition_start, before.repetition_start);
                    assert_eq!(board.stack.len(), before.stack.len());
                }
            }
        }
    }

    #[test]
    fn two_knights_can_deliver_checkmate() {
        let mut board = Board::from_fen("7k/5K2/5NN1/8/8/8/8/8 b - - 0 1").unwrap();
        assert!(!board.insufficient_material());
        assert!(board.in_check(Color::Black));
        assert!(board.generate_legal().is_empty());
    }

    #[test]
    fn same_color_bishops_are_insufficient_material() {
        let board = Board::from_fen("4k3/8/8/8/8/8/8/2B1K1b1 w - - 0 1").unwrap();
        assert!(board.insufficient_material());
    }

    #[test]
    fn opposite_color_bishops_are_not_insufficient_material() {
        let board = Board::from_fen("4k3/8/8/8/8/8/8/2B1K2b w - - 0 1").unwrap();
        assert!(!board.insufficient_material());
    }

    fn keys_from_scratch(board: &Board) -> (u64, u64, [u64; 2], u64, u64) {
        let mut key = 0u64;
        let mut pawn = 0u64;
        let mut non_pawn = [0u64; 2];
        let mut minor = 0u64;
        let mut major = 0u64;
        for sq in 0..64u8 {
            let s = Square::new(sq);
            let Some(p) = board.piece_on(s) else { continue };
            let k = zobrist::piece(p, s);
            key ^= k;
            let c = p.color().index();
            match p.piece_type() {
                PieceType::Pawn => pawn ^= k,
                PieceType::Knight | PieceType::Bishop => {
                    non_pawn[c] ^= k;
                    minor ^= k;
                }
                PieceType::Rook | PieceType::Queen => {
                    non_pawn[c] ^= k;
                    major ^= k;
                }
                PieceType::King => {
                    non_pawn[c] ^= k;
                    minor ^= k;
                    major ^= k;
                }
            }
        }
        key ^= zobrist::side(board.side);
        key ^= zobrist::castling(board.castling.0);
        if let Some(file) = board.ep_hash_file(board.side) {
            key ^= zobrist::en_passant(file);
        }
        (key, pawn, non_pawn, minor, major)
    }

    fn assert_keys_consistent(board: &Board) {
        let (key, pawn, non_pawn, minor, major) = keys_from_scratch(board);
        let fen = board.to_fen();
        for color in [Color::White, Color::Black] {
            assert_eq!(
                board.king_sq(color),
                board.pieces(color, PieceType::King).lsb(),
                "king square diverged at {fen}"
            );
        }
        assert_eq!(board.key, key, "main key diverged at {fen}");
        assert_eq!(board.pawn_key, pawn, "pawn key diverged at {fen}");
        assert_eq!(
            board.non_pawn_key, non_pawn,
            "non-pawn key diverged at {fen}"
        );
        assert_eq!(board.minor_key, minor, "minor key diverged at {fen}");
        assert_eq!(board.major_key, major, "major key diverged at {fen}");
    }

    fn walk_keys(board: &mut Board, depth: u32) {
        assert_keys_consistent(board);
        if depth == 0 {
            return;
        }
        let legal = board.generate_legal();
        for i in 0..legal.len() {
            let m = legal.moves[i];
            board.make_move(m);
            walk_keys(board, depth - 1);
            board.unmake_move(m);
            assert_keys_consistent(board);
        }
    }

    fn find(board: &mut Board, uci: &str) -> crate::types::Move {
        board
            .generate_legal()
            .as_slice()
            .iter()
            .copied()
            .find(|mv| mv.to_uci() == uci)
            .expect("legal test move")
    }

    #[test]
    fn incremental_keys_match_a_fresh_computation() {
        for fen in [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
            "rnbqkbnr/pp1ppppp/8/2pP4/8/8/PPP1PPPP/RNBQKBNR w KQkq c6 0 3",
            "n1n5/PPPk4/8/8/8/8/4Kppp/5N1N b - - 0 1",
        ] {
            let mut board = Board::from_fen(fen).expect("valid test fen");
            walk_keys(&mut board, 3);
        }
    }

    #[test]
    fn special_moves_restore_board_and_zobrist_state() {
        for (fen, uci) in [
            ("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", "e1c1"),
            ("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1", "e5d6"),
            ("1r2k3/P7/8/8/8/8/8/4K3 w - - 0 1", "a7b8q"),
            ("r3k3/8/8/8/8/8/8/R3K3 w Qq - 0 1", "a1a8"),
        ] {
            let mut board = Board::from_fen(fen).unwrap();
            let before_history = board.history.clone();
            let mv = find(&mut board, uci);
            board.make_move(mv);
            assert_keys_consistent(&board);
            let fresh = Board::from_fen(&board.to_fen()).unwrap();
            assert_eq!(board.key, fresh.key);
            assert_eq!(board.pawn_key, fresh.pawn_key);
            assert_eq!(board.non_pawn_key, fresh.non_pawn_key);
            assert_eq!(board.minor_key, fresh.minor_key);
            assert_eq!(board.major_key, fresh.major_key);

            board.unmake_move(mv);
            assert_eq!(board.to_fen(), fen);
            assert_eq!(board.history, before_history);
            assert_keys_consistent(&board);
        }
    }

    #[test]
    fn uncapturable_en_passant_does_not_hide_repetition() {
        let mut board = Board::from_fen("4k1n1/8/8/4p3/8/8/8/4K1N1 w - e6 0 1").unwrap();
        for (ply, uci) in [
            "g1f3", "g8f6", "f3g1", "f6g8", "g1f3", "g8f6", "f3g1", "f6g8",
        ]
        .into_iter()
        .enumerate()
        {
            let mv = board
                .generate_legal()
                .as_slice()
                .iter()
                .copied()
                .find(|m| m.to_uci() == uci)
                .expect("legal repetition move");
            board.make_move(mv);
            if ply == 3 {
                assert!(board.is_repetition());
                assert!(!board.is_threefold());
                assert!(!board.is_search_repetition(0));
                assert!(board.is_search_repetition(4));
            }
        }
        assert!(board.is_threefold());
        assert!(board.is_search_repetition(0));
    }

    #[test]
    fn tt_key_includes_the_fifty_move_clock() {
        let fresh = Board::from_fen("8/8/8/8/8/1K6/8/1k6 w - - 0 1").unwrap();
        let old = Board::from_fen("8/8/8/8/8/1K6/8/1k6 w - - 98 1").unwrap();
        assert_eq!(fresh.key(), old.key());
        assert_ne!(fresh.tt_key(), old.tt_key());

        let limit = Board::from_fen("8/8/8/8/8/1K6/8/1k6 w - - 100 1").unwrap();
        let beyond = Board::from_fen("8/8/8/8/8/1K6/8/1k6 w - - 101 1").unwrap();
        assert_eq!(limit.tt_key(), beyond.tt_key());

        let at = |clock: u32| {
            Board::from_fen(&format!("8/8/8/8/8/1K6/8/1k6 w - - {clock} 1"))
                .unwrap()
                .tt_key()
        };
        assert_eq!(at(0), at(15));
        assert_ne!(at(15), at(16));
        assert_eq!(at(16), at(23));
        assert_ne!(at(23), at(24));
        assert_eq!(at(128), at(255));
    }
}
