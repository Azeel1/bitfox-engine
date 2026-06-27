#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Castling(pub u8);

impl Castling {
    pub const WHITE_KING: u8 = 1;
    pub const WHITE_QUEEN: u8 = 2;
    pub const BLACK_KING: u8 = 4;
    pub const BLACK_QUEEN: u8 = 8;

    #[inline]
    pub const fn has(self, right: u8) -> bool {
        self.0 & right != 0
    }

    #[inline]
    pub fn retain(&mut self, keep: u8) {
        self.0 &= keep;
    }

    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}
