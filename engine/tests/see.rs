use bitfox::board::Board;
use bitfox::types::{Move, PieceType, Square};

fn find(board: &mut Board, uci: &str) -> Move {
    let list = board.generate_legal();
    for i in 0..list.len() {
        if list.moves[i].to_uci() == uci {
            return list.moves[i];
        }
    }
    panic!("move {uci} not legal in position");
}

fn move_gain(board: &Board, mv: Move) -> i32 {
    let mut gain = if mv.is_en_passant() {
        PieceType::Pawn.see_value()
    } else {
        board
            .piece_on(mv.to())
            .map_or(0, |piece| piece.piece_type().see_value())
    };
    if let Some(promo) = mv.promotion() {
        gain += promo.see_value() - PieceType::Pawn.see_value();
    }
    gain
}

fn unambiguous_recapture(board: &mut Board, target: Square) -> i32 {
    let legal = board.generate_legal();
    let mut recaptures = legal
        .as_slice()
        .iter()
        .copied()
        .filter(|mv| mv.to() == target && mv.is_capture());
    let Some(mv) = recaptures.next() else {
        return 0;
    };
    assert!(
        recaptures.next().is_none(),
        "ambiguous exchange at {}",
        board.to_fen()
    );

    let gain = move_gain(board, mv);
    board.make_move(mv);
    let score = gain - unambiguous_recapture(board, target);
    board.unmake_move(mv);
    score.max(0)
}

fn unambiguous_exchange(board: &mut Board, mv: Move) -> i32 {
    let gain = move_gain(board, mv);
    let target = mv.to();
    board.make_move(mv);
    let score = gain - unambiguous_recapture(board, target);
    board.unmake_move(mv);
    score
}

#[test]
fn free_pawn_capture_is_worth_one_pawn() {
    let mut board = Board::from_fen("4k3/8/8/3p4/8/8/8/3QK3 w - - 0 1").unwrap();
    let mv = find(&mut board, "d1d5");
    assert!(
        board.see(mv, 0),
        "winning a free pawn must pass threshold 0"
    );
    assert!(
        board.see(mv, 100),
        "exact pawn value passes at threshold 100"
    );
    assert!(!board.see(mv, 101), "cannot win more than a pawn");
}

#[test]
fn rook_takes_pawn_defended_by_pawn_loses_the_exchange() {
    let mut board = Board::from_fen("4k3/8/2p5/3p4/8/8/8/3RK3 w - - 0 1").unwrap();
    let mv = find(&mut board, "d1d5");
    assert!(!board.see(mv, 0), "rook for a defended pawn is losing");
    assert!(board.see(mv, -550), "net value is +pawn -rook = -550");
    assert!(!board.see(mv, -549), "value is not better than -550");
}

#[test]
fn equal_trade_passes_at_zero() {
    let mut board = Board::from_fen("4k3/8/4p3/3n4/8/2N5/8/4K3 w - - 0 1").unwrap();
    let mv = find(&mut board, "c3d5");
    assert!(
        board.see(mv, 0),
        "knight for a knight is an acceptable even trade"
    );
    assert!(!board.see(mv, 1), "an even trade is not a winning one");
}

#[test]
fn xray_through_rook_is_counted() {
    let mut board = Board::from_fen("3rk3/3r4/8/3p4/8/3R4/3R4/3QK3 w - - 0 1").unwrap();
    let mv = find(&mut board, "d3d5");
    assert!(
        board.see(mv, 0),
        "stacked rooks and queen win the d-file exchange"
    );
}

#[test]
fn castling_has_zero_exchange_value() {
    let mut board = Board::from_fen("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
    let mv = find(&mut board, "e1g1");
    assert!(board.see(mv, 0), "castling is never a capture");
}

#[test]
fn recapture_promotion_counts_the_promoted_piece() {
    let mut board = Board::from_fen("Rr5k/1P6/8/8/8/8/8/4K3 b - - 0 1").unwrap();
    let mv = find(&mut board, "b8a8");
    assert!(board.see(mv, -1150));
    assert!(!board.see(mv, -1149));
}

#[test]
fn see_matches_unambiguous_exchange_cases() {
    let pawn = PieceType::Pawn.see_value();
    let rook = PieceType::Rook.see_value();
    let queen = PieceType::Queen.see_value();
    let cases = [
        ("4k3/8/8/3p4/8/8/8/3QK3 w - - 0 1", "d1d5", pawn),
        ("4k3/8/2p5/3p4/8/8/8/3RK3 w - - 0 1", "d1d5", pawn - rook),
        ("4k3/8/4p3/3n4/8/2N5/8/4K3 w - - 0 1", "c3d5", 0),
        ("3rk3/3r4/8/3p4/8/3R4/3R4/3QK3 w - - 0 1", "d3d5", pawn),
        ("4k3/4n3/8/3p4/5N2/8/8/4R1K1 w - - 0 1", "f4d5", pawn),
        (
            "4k3/4r3/4p3/5Q2/8/8/8/4R1K1 w - - 0 1",
            "f5e6",
            pawn - queen + rook,
        ),
        ("3r3k/8/8/3pP3/8/8/8/4K3 w - d6 0 1", "e5d6", 0),
    ];

    for (fen, uci, expected) in cases {
        let mut board = Board::from_fen(fen).unwrap();
        let mv = find(&mut board, uci);
        let value = unambiguous_exchange(&mut board, mv);
        assert_eq!(value, expected, "{uci} at {fen}");
        assert!(board.see(mv, value), "{uci} at {fen}");
        assert!(!board.see(mv, value + 1), "{uci} at {fen}");
    }
}
