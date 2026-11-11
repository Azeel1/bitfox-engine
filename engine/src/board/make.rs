use super::zobrist;
use super::{Board, Undo};
use crate::types::{Castling, Color, Move, Piece, PieceType, Square};

const fn castle_masks() -> [u8; 64] {
    let mut m = [15u8; 64];
    m[Square::E1.index()] = 15 & !(Castling::WHITE_KING | Castling::WHITE_QUEEN);
    m[Square::A1.index()] = 15 & !Castling::WHITE_QUEEN;
    m[Square::H1.index()] = 15 & !Castling::WHITE_KING;
    m[Square::E8.index()] = 15 & !(Castling::BLACK_KING | Castling::BLACK_QUEEN);
    m[Square::A8.index()] = 15 & !Castling::BLACK_QUEEN;
    m[Square::H8.index()] = 15 & !Castling::BLACK_KING;
    m
}

static CASTLE_MASK: [u8; 64] = castle_masks();

fn rook_castle(to: Square) -> (Square, Square) {
    match to.raw() {
        6 => (Square::new(7), Square::new(5)),
        2 => (Square::new(0), Square::new(3)),
        62 => (Square::new(63), Square::new(61)),
        58 => (Square::new(56), Square::new(59)),
        _ => unreachable!(),
    }
}

impl Board {
    pub fn make_move(&mut self, m: Move) {
        let us = self.side;
        let them = us.flip();
        let from = m.from();
        let to = m.to();
        let piece = self.piece_on(from).expect("move from occupied square");

        let captured = if m.is_en_passant() {
            Some(Piece::new(them, PieceType::Pawn))
        } else if m.is_capture() {
            self.piece_on(to)
        } else {
            None
        };

        self.stack.push(Undo {
            captured,
            ep: self.ep,
            castling: self.castling,
            halfmove: self.halfmove,
            fullmove: self.fullmove,
            key: self.key,
            repetition_start: self.repetition_start,
        });

        if let Some(file) = self.ep_hash_file(us) {
            self.key ^= zobrist::en_passant(file);
        }
        self.ep = None;

        if m.is_en_passant() {
            let cap_sq = if us == Color::White {
                Square::new(to.raw() - 8)
            } else {
                Square::new(to.raw() + 8)
            };
            self.remove(Piece::new(them, PieceType::Pawn), cap_sq);
        } else if m.is_capture() {
            self.remove(captured.unwrap(), to);
        }

        self.remove(piece, from);
        match m.promotion() {
            Some(pt) => self.put(Piece::new(us, pt), to),
            None => self.put(piece, to),
        }

        if m.is_double_pawn() {
            let ep_sq = if us == Color::White {
                Square::new(from.raw() + 8)
            } else {
                Square::new(from.raw() - 8)
            };
            self.ep = Some(ep_sq);
            if let Some(file) = self.ep_hash_file(them) {
                self.key ^= zobrist::en_passant(file);
            }
        } else if m.is_castle() {
            let rook = Piece::new(us, PieceType::Rook);
            let (rfrom, rto) = rook_castle(to);
            self.remove(rook, rfrom);
            self.put(rook, rto);
        }

        let old_castling = self.castling.0;
        self.castling
            .retain(CASTLE_MASK[from.index()] & CASTLE_MASK[to.index()]);
        if self.castling.0 != old_castling {
            self.key ^= zobrist::castling(old_castling) ^ zobrist::castling(self.castling.0);
        }

        if piece.piece_type() == PieceType::Pawn || m.is_capture() {
            self.halfmove = 0;
        } else {
            self.halfmove = self.halfmove.saturating_add(1);
        }
        if us == Color::Black {
            self.fullmove = self.fullmove.saturating_add(1);
        }

        self.side = them;
        self.key ^= zobrist::side_toggle();
        self.history.push(self.key);
    }

    pub fn unmake_move(&mut self, m: Move) {
        let undo = self.stack.pop().expect("unmake without make");
        self.history.pop();
        self.side = self.side.flip();
        let us = self.side;
        let them = us.flip();
        let from = m.from();
        let to = m.to();

        if m.is_castle() {
            let rook = Piece::new(us, PieceType::Rook);
            let (rfrom, rto) = rook_castle(to);
            self.remove(rook, rto);
            self.put(rook, rfrom);
        }

        let piece = self.piece_on(to).expect("unmake from occupied square");
        self.remove(piece, to);
        if m.is_promotion() {
            self.put(Piece::new(us, PieceType::Pawn), from);
        } else {
            self.put(piece, from);
        }

        if let Some(cap) = undo.captured {
            if m.is_en_passant() {
                let cap_sq = if us == Color::White {
                    Square::new(to.raw() - 8)
                } else {
                    Square::new(to.raw() + 8)
                };
                self.put(Piece::new(them, PieceType::Pawn), cap_sq);
            } else {
                self.put(cap, to);
            }
        }

        self.ep = undo.ep;
        self.castling = undo.castling;
        self.halfmove = undo.halfmove;
        self.fullmove = undo.fullmove;
        self.key = undo.key;
        self.repetition_start = undo.repetition_start;
    }

    pub fn make_null(&mut self) {
        self.stack.push(Undo {
            captured: None,
            ep: self.ep,
            castling: self.castling,
            halfmove: self.halfmove,
            fullmove: self.fullmove,
            key: self.key,
            repetition_start: self.repetition_start,
        });
        if let Some(file) = self.ep_hash_file(self.side) {
            self.key ^= zobrist::en_passant(file);
        }
        self.ep = None;
        self.halfmove = self.halfmove.saturating_add(1);
        self.side = self.side.flip();
        self.key ^= zobrist::side_toggle();
        self.repetition_start = self.history.len();
        self.history.push(self.key);
    }

    pub fn unmake_null(&mut self) {
        let undo = self.stack.pop().expect("unmake_null without make_null");
        self.history.pop();
        self.side = self.side.flip();
        self.ep = undo.ep;
        self.halfmove = undo.halfmove;
        self.key = undo.key;
        self.repetition_start = undo.repetition_start;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(board: &mut Board, uci: &str) -> Move {
        let legal = board.generate_legal();
        legal
            .as_slice()
            .iter()
            .copied()
            .find(|m| m.to_uci() == uci)
            .expect("legal test move")
    }

    #[test]
    fn move_counters_saturate_and_unmake_restores_them() {
        let fen = "4k3/8/8/8/8/8/8/4K3 w - - 65535 65535";
        let mut board = Board::from_fen(fen).unwrap();
        let mv = find(&mut board, "e1d1");
        board.make_move(mv);
        assert_eq!(board.halfmove, u16::MAX);
        board.unmake_move(mv);
        assert_eq!(board.to_fen(), fen);

        let fen = "4k3/8/8/8/8/8/8/4K3 b - - 65535 65535";
        let mut board = Board::from_fen(fen).unwrap();
        let mv = find(&mut board, "e8d8");
        board.make_move(mv);
        assert_eq!(board.fullmove, u16::MAX);
        board.unmake_move(mv);
        assert_eq!(board.to_fen(), fen);
    }

    #[test]
    fn double_push_hash_matches_a_fresh_position() {
        let mut board = Board::from_fen("4k3/4p3/8/3P4/8/8/8/4K3 b - - 0 1").unwrap();
        let mv = find(&mut board, "e7e5");
        board.make_move(mv);
        let fresh = Board::from_fen(&board.to_fen()).unwrap();
        assert_eq!(board.key, fresh.key);
        board.unmake_move(mv);
    }

    #[test]
    fn null_move_clears_en_passant_and_restores_all_state() {
        let mut board = Board::from_fen("4k3/8/8/3Pp3/8/8/8/4K3 w - e6 0 2").unwrap();
        let before_fen = board.to_fen();
        let before_key = board.key;
        let before_history = board.history.clone();

        board.make_null();
        assert!(board.ep.is_none());
        assert_eq!(board.halfmove, 1);
        assert_eq!(board.side, Color::Black);
        assert_eq!(board.key, Board::from_fen(&board.to_fen()).unwrap().key);

        board.unmake_null();
        assert_eq!(board.to_fen(), before_fen);
        assert_eq!(board.key, before_key);
        assert_eq!(board.history, before_history);
    }
}
