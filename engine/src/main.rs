use bitfox::tools::{bench, datagen, perft};
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("uci") => bitfox::uci::run(),
        Some("perft") => perft::perft(&args[2..]),
        Some("divide") => perft::divide(&args[2..]),
        Some("bench") => bench::bench(&args[2..]),
        Some("datagen") => datagen::datagen(&args[2..]),
        _ => bitfox::uci::run(),
    }
}
