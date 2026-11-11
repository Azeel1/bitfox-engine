use bitfox::board::Board;
use bitfox::types::Move;

fn play(board: &mut Board, uci: &str) -> Move {
    let mv = board
        .generate_legal()
        .as_slice()
        .iter()
        .copied()
        .find(|mv| mv.to_uci() == uci)
        .expect("legal test move");
    board.make_move(mv);
    mv
}

#[test]
fn null_moves_do_not_create_repetitions() {
    let mut board = Board::startpos();
    let original = board.key();

    board.make_null();
    play(&mut board, "b8c6");
    board.make_null();
    play(&mut board, "c6b8");
    assert_eq!(board.key(), original);
    assert!(!board.is_repetition());
    assert!(!board.is_search_repetition(4));
    assert!(!board.is_search_draw(4));

    board.make_null();
    play(&mut board, "b8c6");
    board.make_null();
    play(&mut board, "c6b8");
    assert!(!board.is_threefold());
}

#[test]
fn legal_cycles_after_a_null_move_are_still_detected() {
    let mut board = Board::startpos();
    board.make_null();
    for uci in ["g8f6", "g1f3", "f6g8", "f3g1"] {
        play(&mut board, uci);
    }
    assert!(board.is_repetition());
    assert!(board.is_search_repetition(4));
    assert!(!board.is_threefold());
    assert!(!board.is_search_repetition(0));
}

#[test]
fn unmake_null_restores_the_real_repetition_history() {
    let mut board = Board::startpos();
    for uci in ["g1f3", "g8f6", "f3g1", "f6g8"] {
        play(&mut board, uci);
    }
    assert!(board.is_repetition());
    let before = board.to_fen();
    let before_key = board.key();
    board.make_null();
    let mv = play(&mut board, "b8c6");
    board.make_null();
    let reply = play(&mut board, "g8f6");
    board.unmake_move(reply);
    board.unmake_null();
    board.unmake_move(mv);
    board.unmake_null();
    assert_eq!(board.to_fen(), before);
    assert_eq!(board.key(), before_key);
    assert!(board.is_repetition());
    assert!(board.is_search_repetition(4));
    for uci in ["g1f3", "g8f6", "f3g1", "f6g8"] {
        play(&mut board, uci);
    }
    assert!(board.is_threefold());
}
