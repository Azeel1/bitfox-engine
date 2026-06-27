use super::Bitboard;
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Square(pub(super) u8);

impl Square {
    pub const A1: Square = Square(0);
    pub const E1: Square = Square(4);
    pub const H1: Square = Square(7);
    pub const A8: Square = Square(56);
    pub const E8: Square = Square(60);
    pub const H8: Square = Square(63);

    #[inline]
    pub const fn new(index: u8) -> Square {
        Square(index)
    }

    #[inline]
    pub const fn from_coords(file: u8, rank: u8) -> Square {
        debug_assert!(file < 8 && rank < 8);
        Square(rank * 8 + file)
    }

    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    #[inline]
    pub const fn raw(self) -> u8 {
        self.0
    }

    #[inline]
    pub const fn file(self) -> u8 {
        self.0 & 7
    }

    #[inline]
    pub const fn rank(self) -> u8 {
        self.0 >> 3
    }

    #[inline]
    pub const fn bb(self) -> Bitboard {
        Bitboard(1u64 << self.0)
    }

    #[inline]
    pub const fn flip_vertical(self) -> Square {
        Square(self.0 ^ 56)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Square> {
        let b = s.as_bytes();
        if b.len() != 2 {
            return None;
        }
        let file = b[0].wrapping_sub(b'a');
        let rank = b[1].wrapping_sub(b'1');
        if file > 7 || rank > 7 {
            return None;
        }
        Some(Square::from_coords(file, rank))
    }
}

impl fmt::Display for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}",
            (b'a' + self.file()) as char,
            (b'1' + self.rank()) as char
        )
    }
}
