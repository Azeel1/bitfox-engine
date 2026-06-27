mod nnue;
mod psqt;

pub use nnue::{evaluate, Nnue};

#[allow(unused_imports)]
pub use psqt::evaluate as classical_evaluate;
