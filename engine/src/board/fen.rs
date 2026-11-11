use super::zobrist;
use super::Board;
use crate::types::{Castling, Color, Piece, PieceType, Square};

impl Board {
    pub fn from_fen(fen: &str) -> Option<Board> {
        let mut board = Board::empty();
        let mut parts = fen.split_whitespace();

        let placement = parts.next()?;
        let mut ranks = placement.split('/');
        let mut counts = [[0u8; 6]; 2];
        for rank in (0..8u8).rev() {
            let mut file = 0u8;
            for c in ranks.next()?.chars() {
                if let Some(empty) = c.to_digit(10) {
                    if !(1..=8).contains(&empty) || file as u32 + empty > 8 {
                        return None;
                    }
                    file += empty as u8;
                } else {
                    let piece = Piece::from_char(c)?;
                    if file >= 8
                        || (piece.piece_type() == PieceType::Pawn && (rank == 0 || rank == 7))
                    {
                        return None;
                    }
                    board.put(piece, Square::from_coords(file, rank));
                    counts[piece.color().index()][piece.piece_type().index()] += 1;
                    file += 1;
                }
            }
            if file != 8 {
                return None;
            }
        }
        if ranks.next().is_some() {
            return None;
        }
        for count in counts {
            let total: u16 = count.iter().map(|&n| n as u16).sum();
            let promotion_excess = count[PieceType::Knight.index()].saturating_sub(2)
                + count[PieceType::Bishop.index()].saturating_sub(2)
                + count[PieceType::Rook.index()].saturating_sub(2)
                + count[PieceType::Queen.index()].saturating_sub(1);
            if total > 16
                || count[PieceType::Pawn.index()] > 8
                || promotion_excess > 8 - count[PieceType::Pawn.index()]
            {
                return None;
            }
        }
        if board.pieces(Color::White, PieceType::King).count() != 1
            || board.pieces(Color::Black, PieceType::King).count() != 1
        {
            return None;
        }

        board.side = match parts.next()? {
            "w" => Color::White,
            "b" => Color::Black,
            _ => return None,
        };
        let has = |b: &Board, sq: Square, p: Piece| b.piece_on(sq) == Some(p);
        let wk = Piece::new(Color::White, PieceType::King);
        let wr = Piece::new(Color::White, PieceType::Rook);
        let bk = Piece::new(Color::Black, PieceType::King);
        let br = Piece::new(Color::Black, PieceType::Rook);
        let rights = parts.next()?;
        if rights != "-" {
            let mut seen = 0u8;
            for c in rights.chars() {
                let right = match c {
                    'K' => Castling::WHITE_KING,
                    'Q' => Castling::WHITE_QUEEN,
                    'k' => Castling::BLACK_KING,
                    'q' => Castling::BLACK_QUEEN,
                    _ => return None,
                };
                if seen & right != 0 {
                    return None;
                }
                seen |= right;
                let valid = match right {
                    Castling::WHITE_KING => {
                        has(&board, Square::new(4), wk) && has(&board, Square::new(7), wr)
                    }
                    Castling::WHITE_QUEEN => {
                        has(&board, Square::new(4), wk) && has(&board, Square::new(0), wr)
                    }
                    Castling::BLACK_KING => {
                        has(&board, Square::new(60), bk) && has(&board, Square::new(63), br)
                    }
                    _ => has(&board, Square::new(60), bk) && has(&board, Square::new(56), br),
                };
                if valid {
                    board.castling.0 |= right;
                }
            }
        }

        board.ep = match parts.next()? {
            "-" => None,
            s => Some(Square::from_str(s)?).filter(|&sq| {
                let expected_rank = if board.side == Color::White { 5 } else { 2 };
                let behind = Square::new(sq.raw() ^ 8);
                sq.rank() == expected_rank
                    && board.piece_on(sq).is_none()
                    && board.piece_on(behind)
                        == Some(Piece::new(board.side.flip(), PieceType::Pawn))
            }),
        };

        board.halfmove = match parts.next() {
            Some(field) => field.parse().ok()?,
            None => 0,
        };
        board.fullmove = match parts.next() {
            Some(field) => field.parse().ok()?,
            None => 1,
        };
        if board.fullmove == 0 || parts.next().is_some() {
            return None;
        }
        if let Some(ep) = board.ep {
            let origin = if board.side == Color::White {
                Square::new(ep.raw() + 8)
            } else {
                Square::new(ep.raw() - 8)
            };
            if board.halfmove != 0 || board.piece_on(origin).is_some() {
                board.ep = None;
            }
        }

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
        if let Some(file) = self.ep_hash_file(self.side) {
            key ^= zobrist::en_passant(file);
        }
        key
    }

    pub fn to_fen(&self) -> String {
        use std::fmt::Write;
        let mut out = String::with_capacity(90);
        for rank in (0..8u8).rev() {
            let mut gap = 0u8;
            for file in 0..8u8 {
                let sq = Square::from_coords(file, rank);
                if let Some(piece) = self.piece_on(sq) {
                    if gap != 0 {
                        out.push(char::from(b'0' + gap));
                        gap = 0;
                    }
                    out.push(piece.to_char());
                } else {
                    gap += 1;
                }
            }
            if gap != 0 {
                out.push(char::from(b'0' + gap));
            }
            if rank != 0 {
                out.push('/');
            }
        }
        out.push(' ');
        out.push(match self.side {
            Color::White => 'w',
            Color::Black => 'b',
        });
        out.push(' ');
        if self.castling.0 == 0 {
            out.push('-');
        } else {
            for (right, ch) in [
                (Castling::WHITE_KING, 'K'),
                (Castling::WHITE_QUEEN, 'Q'),
                (Castling::BLACK_KING, 'k'),
                (Castling::BLACK_QUEEN, 'q'),
            ] {
                if self.castling.has(right) {
                    out.push(ch);
                }
            }
        }
        out.push(' ');
        match self.ep {
            Some(sq) => {
                let _ = write!(out, "{sq}");
            }
            None => out.push('-'),
        }
        let _ = write!(out, " {} {}", self.halfmove, self.fullmove);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagrams_can_show_a_nonmoving_king_in_check() {
        let board =
            Board::from_fen("rnbqk1nr/pppp1ppp/4p3/8/1b1PN3/P7/1PP1PPPP/R1BQKBNR b KQkq - 0 3")
                .unwrap();
        assert!(board.in_check(Color::White));
        assert!(!board.in_check(Color::Black));
    }

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

    #[test]
    fn malformed_placement_and_side_are_rejected() {
        assert!(Board::from_fen("4k3/8/8/8/8/8/8/4K2 w - - 0 1").is_none());
        assert!(Board::from_fen("4k3/8/8/8/8/8/8/4K3/8 w - - 0 1").is_none());
        assert!(Board::from_fen("4k3/8/8/8/8/8/8/4K3 x - - 0 1").is_none());
        assert!(Board::from_fen("P3k3/8/8/8/8/8/8/4K3 w - - 0 1").is_none());
        assert!(Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - e9 0 1").is_none());
        assert!(Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - bad 1").is_none());
        assert!(Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 0").is_none());
    }

    #[test]
    fn missing_move_counters_default_to_a_fresh_game_clock() {
        let short = Board::from_fen("4k3/8/8/3Pp3/8/8/8/4K3 w - e6").unwrap();
        assert_eq!(short.halfmove, 0);
        assert_eq!(short.fullmove, 1);
        assert_eq!(short.ep, Some(Square::from_str("e6").unwrap()));
        assert_eq!(short.to_fen(), "4k3/8/8/3Pp3/8/8/8/4K3 w - e6 0 1");

        let halfmove_only = Board::from_fen("4k3/8/8/8/8/8/8/4K3 b - - 12").unwrap();
        assert_eq!(halfmove_only.halfmove, 12);
        assert_eq!(halfmove_only.fullmove, 1);

        assert!(Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - bm Ke2;").is_none());
        assert!(Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1 7").is_none());
        assert!(Board::from_fen("4k3/8/8/8/8/8/8/4K3 w -").is_none());
    }

    #[test]
    fn occupied_en_passant_target_is_dropped() {
        let board = Board::from_fen("4k3/8/4B3/4p3/8/8/8/4K3 w - e6 0 2").unwrap();
        assert!(board.ep.is_none());
    }

    #[test]
    fn impossible_en_passant_origin_is_dropped() {
        let board = Board::from_fen("4k3/4p3/8/3Pp3/8/8/8/4K3 w - e6 0 2").unwrap();
        assert!(board.ep.is_none());
    }

    #[test]
    fn en_passant_requires_a_reset_halfmove_clock() {
        let board = Board::from_fen("4k3/8/8/3Pp3/8/8/8/4K3 w - e6 1 2").unwrap();
        assert!(board.ep.is_none());
    }

    #[test]
    fn castling_rights_are_serialized_canonically() {
        let board = Board::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w qkQK - 0 1").unwrap();
        assert_eq!(board.to_fen(), "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
    }

    #[test]
    fn uncapturable_en_passant_does_not_change_the_position_key() {
        let with_ep = Board::from_fen("4k3/8/8/4p3/8/8/8/4K3 w - e6 0 2").unwrap();
        let without_ep = Board::from_fen("4k3/8/8/4p3/8/8/8/4K3 w - - 0 2").unwrap();
        assert_eq!(with_ep.key, without_ep.key);
    }

    #[test]
    fn legal_en_passant_changes_the_position_key() {
        let with_ep = Board::from_fen("4k3/8/8/3Pp3/8/8/8/4K3 w - e6 0 2").unwrap();
        let without_ep = Board::from_fen("4k3/8/8/3Pp3/8/8/8/4K3 w - - 0 2").unwrap();
        assert_ne!(with_ep.key, without_ep.key);
    }

    #[test]
    fn illegal_pinned_en_passant_does_not_change_the_position_key() {
        let with_ep = Board::from_fen("4r1k1/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
        let without_ep = Board::from_fen("4r1k1/8/8/3pP3/8/8/8/4K3 w - - 0 1").unwrap();
        assert_eq!(with_ep.key, without_ep.key);
    }
}
