use bitfox::board::Board;
use bitfox::types::Move;

fn find(board: &mut Board, uci: &str) -> Move {
    let list = board.generate_legal();
    for i in 0..list.len() {
        if list.moves[i].to_uci() == uci {
            return list.moves[i];
        }
    }
    panic!("move {uci} not legal in position");
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
fn castling_always_passes() {
    let mut board = Board::from_fen("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
    let mv = find(&mut board, "e1g1");
    assert!(board.see(mv, 0), "castling is never a capture");
}
