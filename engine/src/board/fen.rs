use super::zobrist;
use super::Board;
use crate::types::{Castling, Color, Piece, PieceType, Square};

impl Board {
    pub fn from_fen(fen: &str) -> Option<Board> {
        let mut board = Board::empty();
        let mut parts = fen.split_whitespace();

        let placement = parts.next()?;
        let mut rank: i32 = 7;
        let mut file: i32 = 0;
        for c in placement.chars() {
            match c {
                '/' => {
                    rank -= 1;
                    file = 0;
                }
                '1'..='8' => file += c as i32 - '0' as i32,
                _ => {
                    let piece = Piece::from_char(c)?;
                    if !(0..8).contains(&file) || !(0..8).contains(&rank) {
                        return None;
                    }
                    board.put(piece, Square::from_coords(file as u8, rank as u8));
                    file += 1;
                }
            }
        }
        if board.pieces(Color::White, PieceType::King).count() != 1
            || board.pieces(Color::Black, PieceType::King).count() != 1
        {
            return None;
        }

        board.side = match parts.next() {
            Some("b") => Color::Black,
            _ => Color::White,
        };

        if let Some(rights) = parts.next() {
            for c in rights.chars() {
                match c {
                    'K' => board.castling.0 |= Castling::WHITE_KING,
                    'Q' => board.castling.0 |= Castling::WHITE_QUEEN,
                    'k' => board.castling.0 |= Castling::BLACK_KING,
                    'q' => board.castling.0 |= Castling::BLACK_QUEEN,
                    _ => {}
                }
            }
        }

        board.ep = parts
            .next()
            .and_then(|s| if s == "-" { None } else { Square::from_str(s) });

        board.halfmove = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        board.fullmove = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);

        board.key = board.compute_key();
        board.history.push(board.key);
        Some(board)
    }

    fn compute_key(&self) -> u64 {
        let mut key = 0u64;
        for sq in 0..64u8 {
            let s = Square::new(sq);
            if let Some(p) = self.piece_on(s) {
                key ^= zobrist::piece(p, s);
            }
        }
        key ^= zobrist::side(self.side);
        key ^= zobrist::castling(self.castling.0);
        if let Some(ep) = self.ep {
            key ^= zobrist::en_passant(ep.file());
        }
        key
    }

    pub fn to_fen(&self) -> String {
        let mut fen = String::new();
        for rank in (0..8).rev() {
            let mut empty = 0;
            for file in 0..8 {
                let sq = Square::from_coords(file, rank);
                match self.piece_on(sq) {
                    Some(p) => {
                        if empty > 0 {
                            fen.push((b'0' + empty) as char);
                            empty = 0;
                        }
                        fen.push(p.to_char());
                    }
                    None => empty += 1,
                }
            }
            if empty > 0 {
                fen.push((b'0' + empty) as char);
            }
            if rank > 0 {
                fen.push('/');
            }
        }
        fen.push(' ');
        fen.push(if self.side == Color::White { 'w' } else { 'b' });
        fen.push(' ');
        if self.castling.0 == 0 {
            fen.push('-');
        } else {
            if self.castling.has(Castling::WHITE_KING) {
                fen.push('K');
            }
            if self.castling.has(Castling::WHITE_QUEEN) {
                fen.push('Q');
            }
            if self.castling.has(Castling::BLACK_KING) {
                fen.push('k');
            }
            if self.castling.has(Castling::BLACK_QUEEN) {
                fen.push('q');
            }
        }
        fen.push(' ');
        match self.ep {
            Some(sq) => fen.push_str(&sq.to_string()),
            None => fen.push('-'),
        }
        fen.push_str(&format!(" {} {}", self.halfmove, self.fullmove));
        fen
    }
}
