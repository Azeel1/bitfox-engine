use crate::MAX_PLY;

pub const INFINITY: i32 = 32_000;
pub const MATE: i32 = 31_000;
pub const MATE_IN_MAX: i32 = MATE - MAX_PLY as i32;
pub const DRAW: i32 = 0;
pub const SCORE_NONE: i32 = 32_001;

#[inline]
pub fn mate_in(ply: usize) -> i32 {
    MATE - ply as i32
}

#[inline]
pub fn mated_in(ply: usize) -> i32 {
    -MATE + ply as i32
}

#[inline]
pub fn is_decisive(score: i32) -> bool {
    score.abs() >= MATE_IN_MAX
}

#[inline]
pub fn is_win(score: i32) -> bool {
    score >= MATE_IN_MAX
}

#[inline]
pub fn is_loss(score: i32) -> bool {
    score <= -MATE_IN_MAX
}
