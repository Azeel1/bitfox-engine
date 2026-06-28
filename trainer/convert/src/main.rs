use bulletformat::ChessBoard;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: convert <input.txt> <output.data>");
        eprintln!("  input: one position per line, `fen | white_score_cp | white_wdl`");
        std::process::exit(1);
    }

    bulletformat::convert_from_text::<ChessBoard>(&args[1], &args[2]).expect("conversion failed");
}
