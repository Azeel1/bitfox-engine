use bitfox::board::Board;
use bitfox::eval::Nnue;
use bitfox::types::{Move, Piece, PieceType};

fn move_info(board: &Board, m: Move) -> (Piece, Option<(Piece, usize)>) {
    let moved = board.piece_on(m.from()).unwrap();
    let captured = if m.is_en_passant() {
        Some((
            Piece::new(board.side().flip(), PieceType::Pawn),
            m.to().index() ^ 8,
        ))
    } else if m.is_capture() {
        board.piece_on(m.to()).map(|p| (p, m.to().index()))
    } else {
        None
    };
    (moved, captured)
}

fn walk(board: &mut Board, nnue: &mut Nnue, depth: u32) {
    // In debug builds this asserts the incremental accumulator equals a full refresh.
    nnue.eval(board);
    if depth == 0 {
        return;
    }
    let legal = board.generate_legal();
    for i in 0..legal.len() {
        let m = legal.moves[i];
        let (moved, captured) = move_info(board, m);
        board.make_move(m);
        nnue.make(board, m, moved, captured);
        walk(board, nnue, depth - 1);
        board.unmake_move(m);
        nnue.unmake();
    }
}

#[test]
fn incremental_matches_full_refresh() {
    let fens = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 b - - 0 1",
        "4k3/8/8/8/8/8/4P3/R3K2R w KQ - 0 1",
    ];
    for fen in fens {
        let mut board = Board::from_fen(fen).unwrap();
        let mut nnue = Nnue::default();
        nnue.refresh(&board);
        walk(&mut board, &mut nnue, 3);
    }
}
