#[derive(Default, Clone)]
pub struct Limits {
    pub depth: Option<i32>,
    pub movetime: Option<u64>,
    pub wtime: Option<u64>,
    pub btime: Option<u64>,
    pub winc: u64,
    pub binc: u64,
    pub movestogo: Option<u64>,
    pub nodes: Option<u64>,
    pub infinite: bool,
}
