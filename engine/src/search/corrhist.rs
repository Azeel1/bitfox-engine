use crate::board::Board;
use crate::types::Color;

const SIZE: usize = 16384;
const MASK: usize = SIZE - 1;
const BUCKETS: usize = 16;
const MAX_HISTORY: i32 = 14605;

type Table = [[[i16; SIZE]; BUCKETS]; 2];

pub struct CorrHist {
    pawn: Box<Table>,
    non_pawn: [Box<Table>; 2],
}

impl Default for CorrHist {
    fn default() -> CorrHist {
        CorrHist {
            pawn: zeroed_table(),
            non_pawn: [zeroed_table(), zeroed_table()],
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
        *self.pawn = zeroed();
        *self.non_pawn[0] = zeroed();
        *self.non_pawn[1] = zeroed();
    }

    pub fn value(&self, board: &Board) -> i32 {
        let stm = board.side();
        let bucket = board.fiftymove_bucket();
        let (s, b, _) = slot(stm, 0, bucket);
        let pawn = self.pawn[s][b][board.pawn_key() as usize & MASK] as i32;
        let npw = self.non_pawn[0][s][b][board.non_pawn_key(Color::White) as usize & MASK] as i32;
        let npb = self.non_pawn[1][s][b][board.non_pawn_key(Color::Black) as usize & MASK] as i32;
        (pawn + npw + npb) / 64
    }

    pub fn update(&mut self, board: &Board, depth: i32, diff: i32) {
        let bonus = (148 * depth * diff / 128).clamp(-4678, 2496);
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
    }
}

fn zeroed<T>() -> T {
    unsafe { std::mem::zeroed() }
}

fn zeroed_table() -> Box<Table> {
    unsafe {
        let layout = std::alloc::Layout::new::<Table>();
        let ptr = std::alloc::alloc_zeroed(layout);
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        Box::from_raw(ptr.cast())
    }
}
