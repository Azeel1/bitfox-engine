use crate::types::Move;
use crate::{MATE_IN_MAX, MAX_PLY};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    None = 0,
    Exact = 1,
    Lower = 2,
    Upper = 3,
}

// data: key16 << 48 | mv16 << 32 | (score as u16) << 16 | (eval as u16)
// meta: depth (low 8) | gen_bound (next 8)
struct AtomicEntry {
    data: AtomicU64,
    meta: AtomicU64,
}

impl AtomicEntry {
    fn empty() -> AtomicEntry {
        AtomicEntry {
            data: AtomicU64::new(0),
            meta: AtomicU64::new(0),
        }
    }
}

pub struct Probe {
    pub mv: Move,
    pub score: i32,
    pub eval: i32,
    pub depth: i32,
    pub bound: Bound,
}

pub struct Tt {
    entries: Vec<AtomicEntry>,
    mask: u64,
    generation: AtomicU8,
}

impl Tt {
    pub fn new(mb: usize) -> Tt {
        let mut tt = Tt {
            entries: Vec::new(),
            mask: 0,
            generation: AtomicU8::new(0),
        };
        tt.resize(mb);
        tt
    }

    pub fn resize(&mut self, mb: usize) {
        let bytes = mb.max(1) * 1024 * 1024;
        let count = (bytes / std::mem::size_of::<AtomicEntry>()).next_power_of_two() / 2;
        let count = count.max(1024);
        self.entries = (0..count).map(|_| AtomicEntry::empty()).collect();
        self.mask = (count - 1) as u64;
        self.generation.store(0, Ordering::Relaxed);
    }

    pub fn clear(&self) {
        for e in self.entries.iter() {
            e.data.store(0, Ordering::Relaxed);
            e.meta.store(0, Ordering::Relaxed);
        }
        self.generation.store(0, Ordering::Relaxed);
    }

    pub fn new_search(&self) {
        let g = self.generation.load(Ordering::Relaxed).wrapping_add(1) & 0x3f;
        self.generation.store(g, Ordering::Relaxed);
    }

    #[inline]
    fn slot(&self, key: u64) -> usize {
        (key & self.mask) as usize
    }

    pub fn probe(&self, key: u64, ply: usize) -> Option<Probe> {
        let e = &self.entries[self.slot(key)];
        let data = e.data.load(Ordering::Relaxed);
        let meta = e.meta.load(Ordering::Relaxed);
        let depth = (meta & 0xff) as u8;
        if depth == 0 || (data >> 48) as u16 != (key >> 48) as u16 {
            return None;
        }
        let gen_bound = ((meta >> 8) & 0xff) as u8;
        let mut score = (data >> 16) as u16 as i16 as i32;
        if score >= MATE_IN_MAX {
            score -= ply as i32;
        } else if score <= -MATE_IN_MAX {
            score += ply as i32;
        }
        Some(Probe {
            mv: Move::from_raw((data >> 32) as u16),
            score,
            eval: data as u16 as i16 as i32,
            depth: depth as i32 - 1,
            bound: match gen_bound & 0x3 {
                1 => Bound::Exact,
                2 => Bound::Lower,
                _ => Bound::Upper,
            },
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn store(
        &self,
        key: u64,
        mv: Move,
        mut score: i32,
        eval: i32,
        depth: i32,
        bound: Bound,
        ply: usize,
    ) {
        let key16 = (key >> 48) as u16;
        let e = &self.entries[self.slot(key)];
        let data = e.data.load(Ordering::Relaxed);
        let meta = e.meta.load(Ordering::Relaxed);
        let cur_key = (data >> 48) as u16;
        let cur_depth = (meta & 0xff) as u8;
        let cur_gen = ((meta >> 8) as u8) >> 2;
        let generation = self.generation.load(Ordering::Relaxed);

        let replace = cur_key != key16
            || bound == Bound::Exact
            || cur_gen != generation
            || depth + 4 > cur_depth as i32;
        if !replace {
            return;
        }

        if score >= MATE_IN_MAX {
            score += ply as i32;
        } else if score <= -MATE_IN_MAX {
            score -= ply as i32;
        }

        let stored_mv = if mv.is_none() && cur_key == key16 {
            (data >> 32) as u16
        } else {
            mv.raw()
        };

        let new_data = (key16 as u64) << 48
            | (stored_mv as u64) << 32
            | ((score.clamp(-32000, 32000) as i16 as u16) as u64) << 16
            | (eval.clamp(-32000, 32000) as i16 as u16) as u64;
        let new_meta =
            (depth + 1).clamp(1, 255) as u64 | (((generation << 2) | bound as u8) as u64) << 8;

        e.data.store(new_data, Ordering::Relaxed);
        e.meta.store(new_meta, Ordering::Relaxed);
    }
}

const _: () = assert!(MAX_PLY <= 255);
