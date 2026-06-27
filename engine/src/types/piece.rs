use super::Color;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum PieceType {
    Pawn = 0,
    Knight = 1,
    Bishop = 2,
    Rook = 3,
    Queen = 4,
    King = 5,
}

impl PieceType {
    pub const ALL: [PieceType; 6] = [
        PieceType::Pawn,
        PieceType::Knight,
        PieceType::Bishop,
        PieceType::Rook,
        PieceType::Queen,
        PieceType::King,
    ];

    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[inline]
    pub const fn from_index(i: usize) -> PieceType {
        PieceType::ALL[i]
    }

    #[inline]
    pub const fn see_value(self) -> i32 {
        const VALUES: [i32; 6] = [100, 450, 450, 650, 1250, 0];
        VALUES[self as usize]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Piece(u8);

impl Piece {
    #[inline]
    pub const fn new(color: Color, pt: PieceType) -> Piece {
        Piece((color as u8) * 6 + pt as u8)
    }

    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    #[inline]
    pub const fn from_index(i: usize) -> Piece {
        Piece(i as u8)
    }

    #[inline]
    pub const fn color(self) -> Color {
        if self.0 < 6 {
            Color::White
        } else {
            Color::Black
        }
    }

    #[inline]
    pub const fn piece_type(self) -> PieceType {
        PieceType::ALL[(self.0 % 6) as usize]
    }

    pub fn to_char(self) -> char {
        b"PNBRQKpnbrqk"[self.0 as usize] as char
    }

    pub fn from_char(c: char) -> Option<Piece> {
        let (color, upper) = if c.is_ascii_uppercase() {
            (Color::White, c)
        } else {
            (Color::Black, c.to_ascii_uppercase())
        };
        let pt = match upper {
            'P' => PieceType::Pawn,
            'N' => PieceType::Knight,
            'B' => PieceType::Bishop,
            'R' => PieceType::Rook,
            'Q' => PieceType::Queen,
            'K' => PieceType::King,
            _ => return None,
        };
        Some(Piece::new(color, pt))
    }
}
