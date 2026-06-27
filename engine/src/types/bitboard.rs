use super::Square;
use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Not};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Bitboard(pub u64);

impl Bitboard {
    pub const EMPTY: Bitboard = Bitboard(0);
    pub const FULL: Bitboard = Bitboard(!0);
    pub const FILE_A: Bitboard = Bitboard(0x0101_0101_0101_0101);
    pub const FILE_H: Bitboard = Bitboard(0x8080_8080_8080_8080);
    pub const RANK_1: Bitboard = Bitboard(0x0000_0000_0000_00ff);
    pub const RANK_8: Bitboard = Bitboard(0xff00_0000_0000_0000);

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub const fn any(self) -> bool {
        self.0 != 0
    }

    #[inline]
    pub const fn count(self) -> u32 {
        self.0.count_ones()
    }

    #[inline]
    pub const fn contains(self, sq: Square) -> bool {
        self.0 & (1u64 << sq.raw()) != 0
    }

    #[inline]
    pub fn lsb(self) -> Square {
        debug_assert!(self.0 != 0, "lsb on empty bitboard");
        Square::new(self.0.trailing_zeros() as u8)
    }

    #[inline]
    pub fn pop_lsb(&mut self) -> Square {
        let sq = self.lsb();
        self.0 &= self.0 - 1;
        sq
    }

    #[inline]
    pub fn set(&mut self, sq: Square) {
        self.0 |= 1u64 << sq.raw();
    }

    #[inline]
    pub fn clear(&mut self, sq: Square) {
        self.0 &= !(1u64 << sq.raw());
    }
}

impl Iterator for Bitboard {
    type Item = Square;

    #[inline]
    fn next(&mut self) -> Option<Square> {
        if self.0 == 0 {
            None
        } else {
            Some(self.pop_lsb())
        }
    }
}

macro_rules! bitboard_op {
    ($trait:ident, $method:ident, $assign:ident, $assign_method:ident, $op:tt) => {
        impl $trait for Bitboard {
            type Output = Bitboard;
            #[inline]
            fn $method(self, rhs: Bitboard) -> Bitboard {
                Bitboard(self.0 $op rhs.0)
            }
        }
        impl $assign for Bitboard {
            #[inline]
            fn $assign_method(&mut self, rhs: Bitboard) {
                self.0 = self.0 $op rhs.0;
            }
        }
    };
}

bitboard_op!(BitAnd, bitand, BitAndAssign, bitand_assign, &);
bitboard_op!(BitOr, bitor, BitOrAssign, bitor_assign, |);
bitboard_op!(BitXor, bitxor, BitXorAssign, bitxor_assign, ^);

impl Not for Bitboard {
    type Output = Bitboard;
    #[inline]
    fn not(self) -> Bitboard {
        Bitboard(!self.0)
    }
}
