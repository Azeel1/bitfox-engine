use bitfox::board::Board;
use bitfox::eval::classical_evaluate as evaluate;

fn mirror_fen(fen: &str) -> String {
    let parts: Vec<&str> = fen.split_whitespace().collect();
    let ranks: Vec<&str> = parts[0].split('/').collect();
    let board: Vec<String> = ranks
        .iter()
        .rev()
        .map(|r| {
            r.chars()
                .map(|c| {
                    if c.is_ascii_uppercase() {
                        c.to_ascii_lowercase()
                    } else if c.is_ascii_lowercase() {
                        c.to_ascii_uppercase()
                    } else {
                        c
                    }
                })
                .collect()
        })
        .collect();
    let side = if parts.get(1) == Some(&"w") { "b" } else { "w" };
    let castle: String = parts
        .get(2)
        .unwrap_or(&"-")
        .chars()
        .map(|c| match c {
            'K' => 'k',
            'Q' => 'q',
            'k' => 'K',
            'q' => 'Q',
            x => x,
        })
        .collect();
    let ep = match parts.get(3) {
        Some(&"-") | None => "-".to_string(),
        Some(sq) => {
            let b = sq.as_bytes();
            format!("{}{}", b[0] as char, (b'1' + b'8' - b[1]) as char)
        }
    };
    format!("{} {} {} {} 0 1", board.join("/"), side, castle, ep)
}

#[test]
fn startpos_is_balanced() {
    // The starting position is perfectly symmetric, so the evaluation must be
    // identical whichever side is to move (any side-to-move bonus is the same).
    let white_to_move = evaluate(&Board::startpos());
    let black_to_move =
        evaluate(&Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1").unwrap());
    assert_eq!(white_to_move, black_to_move);
}

#[test]
fn evaluation_is_color_symmetric() {
    let fens = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r1bqkbnr/pppp1ppp/2n5/1B2p3/4P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "8/8/8/4k3/8/3K4/4P3/8 w - - 0 1",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 b - - 0 1",
    ];
    for fen in fens {
        let a = evaluate(&Board::from_fen(fen).unwrap());
        let b = evaluate(&Board::from_fen(&mirror_fen(fen)).unwrap());
        assert_eq!(a, b, "eval not symmetric for {fen} (mirror {})", mirror_fen(fen));
    }
}
