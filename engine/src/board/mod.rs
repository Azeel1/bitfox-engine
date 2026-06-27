mod fen;
mod make;
mod see;
mod threats;
mod zobrist;

pub use threats::ThreatCtx;

use crate::types::{Bitboard, Castling, Color, Piece, PieceType, Square};

#[derive(Clone, Copy)]
pub(crate) struct Undo {
    pub captured: Option<Piece>,
    pub ep: Option<Square>,
    pub castling: Castling,
    pub halfmove: u16,
    pub fullmove: u16,
    pub key: u64,
}

#[derive(Clone)]
pub struct Board {
    pieces: [Bitboard; 6],
    colors: [Bitboard; 2],
    mailbox: [Option<Piece>; 64],
    side: Color,
    ep: Option<Square>,
    castling: Castling,
    halfmove: u16,
    fullmove: u16,
    key: u64,
    pawn_key: u64,
    non_pawn_key: [u64; 2],
    history: Vec<u64>,
    stack: Vec<Undo>,
}

impl Board {
    pub const START_FEN: &'static str =
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    pub(super) fn empty() -> Board {
        Board {
            pieces: [Bitboard::EMPTY; 6],
            colors: [Bitboard::EMPTY; 2],
            mailbox: [None; 64],
            side: Color::White,
            ep: None,
            castling: Castling::default(),
            halfmove: 0,
            fullmove: 1,
            key: 0,
            pawn_key: 0,
            non_pawn_key: [0; 2],
            history: Vec::with_capacity(256),
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
        if piece.piece_type() == PieceType::Pawn {
            self.pawn_key ^= k;
        } else {
            self.non_pawn_key[piece.color().index()] ^= k;
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
        self.pieces(color, PieceType::King).lsb()
    }

    #[inline]
    pub fn has_non_pawn(&self, color: Color) -> bool {
        (self.color_bb(color)
            & !(self.by_type(PieceType::Pawn) | self.by_type(PieceType::King)))
        .any()
    }

    #[inline]
    pub fn ep(&self) -> Option<Square> {
        self.ep
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
    pub fn pawn_key(&self) -> u64 {
        self.pawn_key
    }

    #[inline]
    pub fn non_pawn_key(&self, color: Color) -> u64 {
        self.non_pawn_key[color.index()]
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
        let stop = (len as isize - 1) - window as isize;
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
        let stop = (len as isize - 1) - window as isize;
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

    pub fn insufficient_material(&self) -> bool {
        if (self.pieces[PieceType::Pawn.index()]
            | self.pieces[PieceType::Rook.index()]
            | self.pieces[PieceType::Queen.index()])
        .any()
        {
            return false;
        }
        let minors = self.pieces[PieceType::Knight.index()] | self.pieces[PieceType::Bishop.index()];
        minors.count() <= 1
    }

    pub fn is_draw(&self) -> bool {
        self.halfmove >= 100 || self.is_repetition() || self.insufficient_material()
    }
}
