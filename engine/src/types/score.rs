use crate::MAX_PLY;

pub const INFINITY: i32 = 32_000;
pub const MATE: i32 = 31_000;
pub const MATE_IN_MAX: i32 = MATE - MAX_PLY as i32;
pub const TB_WIN: i32 = MATE_IN_MAX - 1;
pub const TB_WIN_IN_MAX: i32 = TB_WIN - MAX_PLY as i32;
pub const DRAW: i32 = 0;
pub const SCORE_NONE: i32 = 32_001;

const _: () = assert!(TB_WIN < MATE_IN_MAX);
const _: () = assert!(TB_WIN_IN_MAX > 0);
const _: () = assert!(TB_WIN_IN_MAX + MAX_PLY as i32 == TB_WIN);

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
    score >= TB_WIN_IN_MAX || score <= -TB_WIN_IN_MAX
}

#[inline]
pub fn is_win(score: i32) -> bool {
    score >= TB_WIN_IN_MAX
}

#[inline]
pub fn is_loss(score: i32) -> bool {
    score <= -TB_WIN_IN_MAX
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decisive_classification_includes_all_tablebase_distances() {
        for ply in 0..=MAX_PLY {
            let win = TB_WIN - ply as i32;
            assert!(is_decisive(win));
            assert!(is_win(win));
            assert!(!is_loss(win));
            assert!(is_decisive(-win));
            assert!(is_loss(-win));
            assert!(!is_win(-win));
            assert!(win < MATE_IN_MAX);
        }
        for score in [0, TB_WIN_IN_MAX - 1, -TB_WIN_IN_MAX + 1] {
            assert!(!is_decisive(score));
            assert!(!is_win(score));
            assert!(!is_loss(score));
        }
        for score in [MATE_IN_MAX, MATE, i32::MAX] {
            assert!(is_decisive(score));
            assert!(is_win(score));
            assert!(!is_loss(score));
        }
        for score in [-MATE_IN_MAX, -MATE, i32::MIN] {
            assert!(is_decisive(score));
            assert!(is_loss(score));
            assert!(!is_win(score));
        }
    }

    #[test]
    fn decisive_handles_the_full_integer_range() {
        assert!(is_decisive(i32::MIN));
        assert!(is_decisive(i32::MAX));
        assert!(!is_decisive(0));
    }
}
