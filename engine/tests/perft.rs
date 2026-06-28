use bitfox::board::Board;

fn perft(fen: &str, depth: u32) -> u64 {
    let mut board = Board::from_fen(fen).expect("valid fen");
    board.perft(depth)
}

const STARTPOS: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const KIWIPETE: &str = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
const POSITION_3: &str = "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1";
const POSITION_4: &str = "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1";
const POSITION_5: &str = "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8";
const POSITION_6: &str = "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10";

#[test]
fn perft_startpos() {
    assert_eq!(perft(STARTPOS, 1), 20);
    assert_eq!(perft(STARTPOS, 2), 400);
    assert_eq!(perft(STARTPOS, 3), 8902);
    assert_eq!(perft(STARTPOS, 4), 197281);
    assert_eq!(perft(STARTPOS, 5), 4865609);
    assert_eq!(perft(STARTPOS, 6), 119060324);
}

#[test]
fn perft_kiwipete() {
    assert_eq!(perft(KIWIPETE, 1), 48);
    assert_eq!(perft(KIWIPETE, 2), 2039);
    assert_eq!(perft(KIWIPETE, 3), 97862);
    assert_eq!(perft(KIWIPETE, 4), 4085603);
    assert_eq!(perft(KIWIPETE, 5), 193690690);
}

#[test]
fn perft_position_3() {
    assert_eq!(perft(POSITION_3, 5), 674624);
    assert_eq!(perft(POSITION_3, 6), 11030083);
    assert_eq!(perft(POSITION_3, 7), 178633661);
}

#[test]
fn perft_position_4() {
    assert_eq!(perft(POSITION_4, 4), 422333);
    assert_eq!(perft(POSITION_4, 5), 15833292);
}

#[test]
fn perft_position_5() {
    assert_eq!(perft(POSITION_5, 3), 62379);
    assert_eq!(perft(POSITION_5, 4), 2103487);
    assert_eq!(perft(POSITION_5, 5), 89941194);
}

#[test]
fn perft_position_6() {
    assert_eq!(perft(POSITION_6, 4), 3894594);
    assert_eq!(perft(POSITION_6, 5), 164075551);
}
