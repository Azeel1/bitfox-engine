use crate::board::Board;
use crate::types::Color;

const SIZE: usize = 16384;
const MASK: usize = SIZE - 1;
const BUCKETS: usize = 16;
const MAX_HISTORY: i32 = 16384;

type Table = [[[i16; SIZE]; BUCKETS]; 2];

// one correction table per key (pawns, non-pawns per side, minors, majors),
// each split by the fifty-move counter
pub struct CorrHist {
    pawn: Box<Table>,
    non_pawn: [Box<Table>; 2],
    minor: Box<Table>,
    major: Box<Table>,
}

impl Default for CorrHist {
    fn default() -> CorrHist {
        CorrHist {
            pawn: super::alloc_zeroed_box(),
            non_pawn: [super::alloc_zeroed_box(), super::alloc_zeroed_box()],
            minor: super::alloc_zeroed_box(),
            major: super::alloc_zeroed_box(),
        }
    }
}

#[inline]
fn slot(stm: Color, key: u64, bucket: usize) -> (usize, usize, usize) {
    (stm.index(), bucket, key as usize & MASK)
}

#[inline]
fn apply(entry: &mut i16, bonus: i32) {
    let v = *entry as i32;
    *entry = (v + bonus - bonus.abs() * v / MAX_HISTORY) as i16;
}

impl CorrHist {
    pub fn clear(&mut self) {
        for table in std::iter::once(&mut self.pawn)
            .chain(&mut self.non_pawn)
            .chain([&mut self.minor, &mut self.major])
        {
            for entries in table.iter_mut().flatten() {
                entries.fill(0);
            }
        }
    }

    pub fn value(&self, board: &Board) -> i32 {
        let stm = board.side();
        let bucket = board.fiftymove_bucket();
        let (s, b, _) = slot(stm, 0, bucket);
        let pawn = self.pawn[s][b][board.pawn_key() as usize & MASK] as i32;
        let npw = self.non_pawn[0][s][b][board.non_pawn_key(Color::White) as usize & MASK] as i32;
        let npb = self.non_pawn[1][s][b][board.non_pawn_key(Color::Black) as usize & MASK] as i32;
        let minor = self.minor[s][b][board.minor_key() as usize & MASK] as i32;
        let major = self.major[s][b][board.major_key() as usize & MASK] as i32;
        pawn + npw + npb + minor + major
    }

    pub fn update(&mut self, board: &Board, depth: i32, diff: i32) {
        let bonus = (i64::from(depth) * i64::from(diff)).clamp(-4096, 4096) as i32;
        let stm = board.side();
        let bucket = board.fiftymove_bucket();
        let (s, b, _) = slot(stm, 0, bucket);
        apply(
            &mut self.pawn[s][b][board.pawn_key() as usize & MASK],
            bonus,
        );
        apply(
            &mut self.non_pawn[0][s][b][board.non_pawn_key(Color::White) as usize & MASK],
            bonus,
        );
        apply(
            &mut self.non_pawn[1][s][b][board.non_pawn_key(Color::Black) as usize & MASK],
            bonus,
        );
        apply(
            &mut self.minor[s][b][board.minor_key() as usize & MASK],
            bonus,
        );
        apply(
            &mut self.major[s][b][board.major_key() as usize & MASK],
            bonus,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correction_bonus_saturates_without_overflow() {
        let board = Board::startpos();
        for (depth, diff, expected) in [
            (256, 60_000, 4096),
            (256, -60_000, -4096),
            (i32::MIN, i32::MIN, 4096),
            (i32::MAX, i32::MIN, -4096),
        ] {
            let mut history = CorrHist::default();
            history.update(&board, depth, diff);
            assert_eq!(history.value(&board), 5 * expected);
        }
    }

    #[test]
    fn repeated_correction_updates_remain_within_symmetric_limits() {
        let board = Board::startpos();
        let mut history = CorrHist::default();
        for sign in [1, -1, 1] {
            for _ in 0..128 {
                history.update(&board, i32::MAX, sign * i32::MAX);
                assert!((-5 * MAX_HISTORY..=5 * MAX_HISTORY).contains(&history.value(&board)));
            }
            assert_eq!(history.value(&board), sign * 5 * MAX_HISTORY);
        }
    }

    #[test]
    fn clear_works_on_a_small_stack() {
        let mut history = CorrHist::default();
        for table in std::iter::once(&mut history.pawn)
            .chain(&mut history.non_pawn)
            .chain([&mut history.minor, &mut history.major])
        {
            for entries in table.iter_mut().flatten() {
                entries.fill(123);
            }
        }

        std::thread::Builder::new()
            .stack_size(128 * 1024)
            .spawn(move || {
                history.clear();
                for table in std::iter::once(&history.pawn)
                    .chain(&history.non_pawn)
                    .chain([&history.minor, &history.major])
                {
                    assert!(table.iter().flatten().flatten().all(|&entry| entry == 0));
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
