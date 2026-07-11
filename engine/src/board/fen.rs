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

        let has = |b: &Board, sq: Square, p: Piece| b.piece_on(sq) == Some(p);
        let wk = Piece::new(Color::White, PieceType::King);
        let wr = Piece::new(Color::White, PieceType::Rook);
        let bk = Piece::new(Color::Black, PieceType::King);
        let br = Piece::new(Color::Black, PieceType::Rook);
        if let Some(rights) = parts.next() {
            for c in rights.chars() {
                match c {
                    'K' if has(&board, Square::new(4), wk) && has(&board, Square::new(7), wr) => {
                        board.castling.0 |= Castling::WHITE_KING
                    }
                    'Q' if has(&board, Square::new(4), wk) && has(&board, Square::new(0), wr) => {
                        board.castling.0 |= Castling::WHITE_QUEEN
                    }
                    'k' if has(&board, Square::new(60), bk) && has(&board, Square::new(63), br) => {
                        board.castling.0 |= Castling::BLACK_KING
                    }
                    'q' if has(&board, Square::new(60), bk) && has(&board, Square::new(56), br) => {
                        board.castling.0 |= Castling::BLACK_QUEEN
                    }
                    _ => {}
                }
            }
        }

        board.ep = parts
            .next()
            .and_then(|s| if s == "-" { None } else { Square::from_str(s) })
            .filter(|&sq| {
                let expected_rank = if board.side == Color::White { 5 } else { 2 };
                let behind = Square::new(sq.raw() ^ 8);
                sq.rank() == expected_rank
                    && board.piece_on(behind)
                        == Some(Piece::new(board.side.flip(), PieceType::Pawn))
            });

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn castling_right_without_matching_king_and_rook_is_dropped() {
        let board = Board::from_fen("7k/8/8/8/8/8/8/K7 w K - 0 1").unwrap();
        assert_eq!(board.castling.0, 0);
    }

    #[test]
    fn castling_right_with_matching_king_and_rook_is_kept() {
        let board = Board::from_fen("4k3/8/8/8/8/8/8/R3K3 w Q - 0 1").unwrap();
        assert!(board.castling.has(Castling::WHITE_QUEEN));
    }

    #[test]
    fn en_passant_square_on_the_wrong_rank_is_rejected() {
        let board = Board::from_fen("4k3/8/8/8/8/8/3P4/4K3 w - e3 0 1").unwrap();
        assert!(board.ep.is_none());
    }

    #[test]
    fn en_passant_square_without_a_pawn_behind_it_is_rejected() {
        let board = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - e6 0 1").unwrap();
        assert!(board.ep.is_none());
    }

    #[test]
    fn legitimate_en_passant_square_is_kept() {
        let board =
            Board::from_fen("rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq e6 0 2")
                .unwrap();
        assert_eq!(board.ep, Some(Square::from_str("e6").unwrap()));
    }
}
