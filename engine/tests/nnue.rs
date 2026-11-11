use bitfox::board::Board;
use bitfox::eval::{evaluate, Nnue};
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

fn find(board: &mut Board, uci: &str) -> Move {
    board
        .generate_legal()
        .as_slice()
        .iter()
        .copied()
        .find(|mv| mv.to_uci() == uci)
        .unwrap_or_else(|| panic!("move {uci} not legal at {}", board.to_fen()))
}

fn walk(board: &mut Board, nnue: &mut Nnue, depth: u32) {
    assert_eq!(
        nnue.eval(board),
        evaluate(board),
        "incremental accumulator diverged from full refresh at {}",
        board.to_fen()
    );
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

#[test]
fn deferred_updates_cover_special_moves_and_unmake() {
    for (fen, moves) in [
        (
            Board::START_FEN,
            &["e2e4", "a7a6", "e4e5", "d7d5", "e5d6"][..],
        ),
        (
            Board::START_FEN,
            &[
                "e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6", "b5a4", "g8f6", "e1g1",
            ][..],
        ),
        ("1r2k3/P7/8/8/8/8/8/4K3 w - - 0 1", &["a7b8q"][..]),
    ] {
        let mut board = Board::from_fen(fen).unwrap();
        let root = board.to_fen();
        let mut nnue = Nnue::default();
        nnue.refresh(&board);
        let mut played = Vec::new();

        for uci in moves {
            let mv = find(&mut board, uci);
            let (moved, captured) = move_info(&board, mv);
            board.make_move(mv);
            nnue.make(&board, mv, moved, captured);
            played.push(mv);
        }
        assert_eq!(nnue.eval(&board), evaluate(&board));

        while let Some(mv) = played.pop() {
            board.unmake_move(mv);
            nnue.unmake();
            assert_eq!(nnue.eval(&board), evaluate(&board));
        }
        assert_eq!(board.to_fen(), root);
    }
}
