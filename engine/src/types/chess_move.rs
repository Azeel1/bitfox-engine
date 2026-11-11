use super::{PieceType, Square};

const SQUARE_MASK: u16 = 0x3f;

// 16-bit move encoding:
//   bits [0..5]   : origin square (0..63)
//   bits [6..11]  : target square (0..63)
//   bits [12..15] : move flag (quiet, castle, capture, ep, promotion)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Move(u16);

impl Move {
    pub const QUIET: u16 = 0;
    pub const DOUBLE_PAWN: u16 = 1;
    pub const KING_CASTLE: u16 = 2;
    pub const QUEEN_CASTLE: u16 = 3;
    pub const CAPTURE: u16 = 4;
    pub const EN_PASSANT: u16 = 5;
    pub const KNIGHT_PROMO: u16 = 8;
    pub const BISHOP_PROMO: u16 = 9;
    pub const ROOK_PROMO: u16 = 10;
    pub const QUEEN_PROMO: u16 = 11;
    pub const KNIGHT_PROMO_CAPTURE: u16 = 12;
    pub const BISHOP_PROMO_CAPTURE: u16 = 13;
    pub const ROOK_PROMO_CAPTURE: u16 = 14;
    pub const QUEEN_PROMO_CAPTURE: u16 = 15;

    pub const NONE: Move = Move(0);

    #[inline]
    pub const fn new(from: Square, to: Square, flag: u16) -> Move {
        Move((from.raw() as u16) | ((to.raw() as u16) << 6) | (flag << 12))
    }

    #[inline]
    pub const fn from_raw(raw: u16) -> Move {
        Move(raw)
    }

    #[inline]
    pub const fn raw(self) -> u16 {
        self.0
    }

    #[inline]
    pub const fn from(self) -> Square {
        let idx = self.0 & SQUARE_MASK;
        Square::new(idx as u8)
    }

    #[inline]
    pub const fn to(self) -> Square {
        let idx = (self.0 >> 6) & SQUARE_MASK;
        Square::new(idx as u8)
    }

    #[inline]
    pub const fn flag(self) -> u16 {
        self.0 >> 12
    }

    #[inline]
    pub const fn is_none(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub const fn is_capture(self) -> bool {
        self.flag() & 4 != 0
    }

    #[inline]
    pub const fn is_quiet(self) -> bool {
        self.flag() & 4 == 0 && self.flag() < 8
    }

    #[inline]
    pub const fn is_promotion(self) -> bool {
        self.flag() >= 8
    }

    #[inline]
    pub const fn is_en_passant(self) -> bool {
        self.flag() == Move::EN_PASSANT
    }

    #[inline]
    pub const fn is_castle(self) -> bool {
        matches!(self.flag(), Move::KING_CASTLE | Move::QUEEN_CASTLE)
    }

    #[inline]
    pub const fn is_double_pawn(self) -> bool {
        self.flag() == Move::DOUBLE_PAWN
    }

    #[inline]
    pub const fn promotion(self) -> Option<PieceType> {
        if self.is_promotion() {
            Some(PieceType::ALL[((self.flag() & 3) + 1) as usize])
        } else {
            None
        }
    }

    pub fn to_uci(self) -> String {
        if self.is_none() {
            return "0000".to_string();
        }
        let mut s = format!("{}{}", self.from(), self.to());
        if let Some(pt) = self.promotion() {
            let c = match pt {
                PieceType::Knight => 'n',
                PieceType::Bishop => 'b',
                PieceType::Rook => 'r',
                PieceType::Queen => 'q',
                _ => unreachable!(),
            };
            s.push(c);
        }
        s
    }
}

impl Default for Move {
    fn default() -> Move {
        Move::NONE
    }
}
