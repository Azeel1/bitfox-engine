use crate::types::{Color, Piece, Square};
use std::sync::OnceLock;

pub struct Zobrist {
    pieces: [[u64; 64]; 12],
    side: u64,
    castling: [u64; 16],
    en_passant: [u64; 8],
}

static ZOBRIST: OnceLock<Zobrist> = OnceLock::new();

fn keys() -> &'static Zobrist {
    ZOBRIST.get_or_init(Zobrist::generate)
}

impl Zobrist {
    fn generate() -> Zobrist {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        };

        let mut pieces = [[0u64; 64]; 12];
        for piece in pieces.iter_mut() {
            for sq in piece.iter_mut() {
                *sq = next();
            }
        }
        let side = next();
        let mut castling = [0u64; 16];
        for c in castling.iter_mut() {
            *c = next();
        }
        let mut en_passant = [0u64; 8];
        for e in en_passant.iter_mut() {
            *e = next();
        }

        Zobrist {
            pieces,
            side,
            castling,
            en_passant,
        }
    }
}

#[inline]
pub fn piece(p: Piece, sq: Square) -> u64 {
    keys().pieces[p.index()][sq.index()]
}

#[inline]
pub fn side(color: Color) -> u64 {
    if color == Color::Black {
        keys().side
    } else {
        0
    }
}

#[inline]
pub fn side_toggle() -> u64 {
    keys().side
}

#[inline]
pub fn castling(rights: u8) -> u64 {
    keys().castling[rights as usize]
}

#[inline]
pub fn en_passant(file: u8) -> u64 {
    keys().en_passant[file as usize]
}
