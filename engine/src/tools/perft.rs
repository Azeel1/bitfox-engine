use crate::board::Board;
use std::time::Instant;

fn parse_fen(args: &[String]) -> Board {
    if args.is_empty() {
        return Board::startpos();
    }
    let fen = args.join(" ");
    Board::from_fen(&fen).unwrap_or_else(|| {
        eprintln!("invalid fen: {fen}");
        std::process::exit(1);
    })
}

fn parse_depth(args: &[String], default: u32) -> Result<u32, String> {
    let Some(value) = args.first() else {
        return Ok(default);
    };
    value.parse().map_err(|_| format!("invalid depth: {value}"))
}

fn parse_divide_depth(args: &[String]) -> Result<u32, String> {
    let depth = parse_depth(args, 1)?;
    if depth == 0 {
        return Err("divide depth must be positive".to_string());
    }
    Ok(depth)
}

fn depth_or_exit(result: Result<u32, String>) -> u32 {
    result.unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    })
}

pub fn perft(args: &[String]) {
    let depth = depth_or_exit(parse_depth(args, 5));
    let mut board = parse_fen(&args[1.min(args.len())..]);
    let start = Instant::now();
    let nodes = board.perft(depth);
    let secs = start.elapsed().as_secs_f64();
    println!("perft({depth}) = {nodes}");
    println!("time {secs:.3}s  nps {:.0}", nodes as f64 / secs.max(1e-9));
}

pub fn divide(args: &[String]) {
    let depth = depth_or_exit(parse_divide_depth(args));
    let mut board = parse_fen(&args[1.min(args.len())..]);
    let moves = board.generate_legal();
    let mut total = 0u64;
    for i in 0..moves.len() {
        let m = moves.moves[i];
        board.make_move(m);
        let nodes = if depth <= 1 {
            1
        } else {
            board.perft(depth - 1)
        };
        board.unmake_move(m);
        total += nodes;
        println!("{}: {}", m.to_uci(), nodes);
    }
    println!("\ntotal: {total}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_is_explicit_when_present() {
        assert_eq!(parse_depth(&[], 5), Ok(5));
        assert_eq!(parse_depth(&["7".to_string()], 5), Ok(7));
    }

    #[test]
    fn invalid_depth_is_reported() {
        assert_eq!(
            parse_depth(&["x".to_string()], 5),
            Err("invalid depth: x".to_string())
        );
        assert_eq!(
            parse_divide_depth(&["0".to_string()]),
            Err("divide depth must be positive".to_string())
        );
    }
}
