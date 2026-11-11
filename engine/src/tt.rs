use crate::types::Move;
use crate::{INFINITY, MAX_PLY, TB_WIN_IN_MAX};
use portable_atomic::AtomicU128;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    None = 0,
    Exact = 1,
    Lower = 2,
    Upper = 3,
}

impl Bound {
    #[inline(always)]
    const fn from_bits(bits: u8) -> Bound {
        match bits & 0b11 {
            1 => Bound::Exact,
            2 => Bound::Lower,
            3 => Bound::Upper,
            _ => Bound::None,
        }
    }
}

// Each TT slot is stored as a single 128-bit atomic word:
//   bits [0..15]   : static eval (i16)
//   bits [16..31]  : search score (i16, mate/TB adjusted to node ply)
//   bits [32..47]  : best move (u16)
//   bits [48..55]  : depth + 1 (u8, 0 = empty)
//   bits [56..57]  : bound type (2 bits)
//   bits [58..63]  : generation counter (6 bits)
//   bits [64..127] : full 64-bit position key
struct AtomicEntry {
    value: AtomicU128,
}

impl AtomicEntry {
    fn empty() -> AtomicEntry {
        AtomicEntry {
            value: AtomicU128::new(0),
        }
    }
}

const CLUSTER_SIZE: usize = 4;

#[repr(align(64))]
struct Cluster {
    entries: [AtomicEntry; CLUSTER_SIZE],
}

impl Cluster {
    fn empty() -> Cluster {
        Cluster {
            entries: std::array::from_fn(|_| AtomicEntry::empty()),
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
    clusters: Vec<Cluster>,
    mask: u64,
    generation: AtomicU8,
}

impl Tt {
    pub fn new(mb: usize) -> Tt {
        let mut tt = Tt {
            clusters: Vec::new(),
            mask: 0,
            generation: AtomicU8::new(0),
        };
        tt.resize(mb);
        tt
    }

    pub fn resize(&mut self, mb: usize) {
        let bytes = mb.max(1) * 1024 * 1024;
        let cluster_bytes = std::mem::size_of::<Cluster>();
        let count = (bytes / cluster_bytes).max(1);
        self.clusters = (0..count).map(|_| Cluster::empty()).collect();
        self.mask = if count.is_power_of_two() {
            (count - 1) as u64
        } else {
            u64::MAX
        };
        self.generation.store(0, Ordering::Relaxed);
    }

    pub fn clear(&self) {
        for c in self.clusters.iter() {
            for e in c.entries.iter() {
                e.value.store(0, Ordering::Relaxed);
            }
        }
        self.generation.store(0, Ordering::Relaxed);
    }

    pub fn new_search(&self) {
        let g = self.generation.load(Ordering::Relaxed).wrapping_add(1) & 0x3f;
        self.generation.store(g, Ordering::Relaxed);
    }

    pub fn hashfull(&self) -> u32 {
        let sample_clusters = self.clusters.len().clamp(1, 1000 / CLUSTER_SIZE);
        let generation = self.generation.load(Ordering::Relaxed);
        let mut used = 0usize;
        let mut total = 0usize;
        for c in &self.clusters[..sample_clusters] {
            for e in c.entries.iter() {
                total += 1;
                let payload = e.value.load(Ordering::Relaxed) as u64;
                let depth = (payload >> 48) as u8;
                let gen = (payload >> 58) as u8;
                if depth != 0 && gen == generation {
                    used += 1;
                }
            }
        }
        if total == 0 {
            return 0;
        }
        (used * 1000 / total) as u32
    }

    #[inline]
    fn cluster_idx(&self, key: u64) -> usize {
        if self.mask == u64::MAX {
            return self.non_power_of_two_idx(key);
        }
        (key & self.mask) as usize
    }

    #[inline(always)]
    fn non_power_of_two_idx(&self, key: u64) -> usize {
        ((key as u128 * self.clusters.len() as u128) >> 64) as usize
    }

    #[inline(always)]
    pub(crate) fn prefetch(&self, key: u64) {
        let ptr = self.clusters[self.cluster_idx(key)..].as_ptr();
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use std::arch::x86_64::{_mm_prefetch, _MM_HINT_T0};
            _mm_prefetch::<_MM_HINT_T0>(ptr.cast());
        }
        #[cfg(target_arch = "aarch64")]
        unsafe {
            std::arch::asm!(
                "prfm pldl1keep, [{0}]",
                in(reg) ptr,
                options(nostack, preserves_flags, readonly)
            );
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        let _ = ptr;
    }

    #[inline(always)]
    pub(crate) fn probe(&self, key: u64, ply: usize) -> Option<Probe> {
        let ply = ply as i32;
        let cluster = &self.clusters[self.cluster_idx(key)];
        for e in cluster.entries.iter() {
            let value = e.value.load(Ordering::Relaxed);
            let payload = value as u64;
            let depth = (payload >> 48) as u8;
            if depth == 0 || (value >> 64) as u64 != key {
                continue;
            }
            let gen_bound = (payload >> 56) as u8;
            let mut score = (payload >> 16) as u16 as i16 as i32;
            if score >= TB_WIN_IN_MAX {
                score -= ply;
            } else if score <= -TB_WIN_IN_MAX {
                score += ply;
            }
            return Some(Probe {
                mv: Move::from_raw((payload >> 32) as u16),
                score,
                eval: payload as u16 as i16 as i32,
                depth: depth as i32 - 1,
                bound: Bound::from_bits(gen_bound),
            });
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn store(
        &self,
        key: u64,
        mv: Move,
        mut score: i32,
        eval: i32,
        depth: i32,
        bound: Bound,
        ply: usize,
    ) {
        let ply = ply as i32;
        let cluster = &self.clusters[self.cluster_idx(key)];
        let generation = self.generation.load(Ordering::Relaxed);

        if score >= TB_WIN_IN_MAX {
            score = score.saturating_add(ply);
        } else if score <= -TB_WIN_IN_MAX {
            score = score.saturating_sub(ply);
        }

        loop {
            let mut target = None;
            let mut empty = None;
            let mut replacement = 0usize;
            let mut best_quality = i32::MAX;
            let mut snapshots = [0u128; CLUSTER_SIZE];
            for (i, e) in cluster.entries.iter().enumerate() {
                let value = e.value.load(Ordering::Relaxed);
                snapshots[i] = value;
                let payload = value as u64;
                let cur_depth = (payload >> 48) as u8;
                if cur_depth != 0 && (value >> 64) as u64 == key {
                    target = Some(i);
                    break;
                }
                if cur_depth == 0 {
                    if empty.is_none() {
                        empty = Some(i);
                    }
                } else if empty.is_none() {
                    let cur_gen = (payload >> 58) as u8;
                    let age = generation.wrapping_sub(cur_gen) & 0x3f;
                    let quality = cur_depth as i32 - age as i32 * 2;
                    if quality < best_quality {
                        best_quality = quality;
                        replacement = i;
                    }
                }
            }
            let target = target.or(empty).unwrap_or(replacement);
            let value = snapshots[target];
            let payload = value as u64;
            let cur_key = (value >> 64) as u64;
            let cur_depth = (payload >> 48) as u8;
            let cur_gen = (payload >> 58) as u8;

            let replace = cur_key != key
                || bound == Bound::Exact
                || cur_gen != generation
                || depth + 4 > cur_depth as i32;
            if !replace {
                return;
            }

            let stored_mv = if mv.is_none() && cur_key == key {
                (payload >> 32) as u16
            } else {
                mv.raw()
            };
            let new_payload = (stored_mv as u64) << 32
                | ((score.clamp(-INFINITY + 1, INFINITY - 1) as i16 as u16) as u64) << 16
                | (eval.clamp(-32000, 32000) as i16 as u16) as u64
                | ((depth + 1).clamp(1, 255) as u64) << 48
                | (((generation << 2) | bound as u8) as u64) << 56;
            let new_value = ((key as u128) << 64) | new_payload as u128;
            if cluster.entries[target]
                .value
                .compare_exchange(value, new_value, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
        }
    }
}

const _: () = assert!(MAX_PLY <= 255);
const _: () = assert!(std::mem::align_of::<Cluster>() == 64);
const _: () = assert!(std::mem::size_of::<Cluster>() == 64);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MATE_IN_MAX, TB_WIN};
    use std::sync::{Arc, Barrier};

    #[test]
    fn prefetch_accepts_every_table_size() {
        for mb in [1usize, 3, 16, 100] {
            let tt = Tt::new(mb);
            for key in [0u64, 1, u64::MAX, 0x9e37_79b9_7f4a_7c15] {
                tt.prefetch(key);
            }
        }
    }

    #[test]
    fn resize_uses_the_requested_budget() {
        for mb in [1usize, 3, 4, 16, 64, 128, 256, 512] {
            let tt = Tt::new(mb);
            let requested = mb * 1024 * 1024;
            let cluster_bytes = std::mem::size_of::<Cluster>();
            let actual = tt.clusters.len() * cluster_bytes;
            assert!(
                actual <= requested,
                "{mb}MB: used {actual} bytes, over budget"
            );
            assert!(requested - actual < cluster_bytes);
        }
    }

    #[test]
    fn cluster_stays_on_one_cache_line() {
        assert_eq!(std::mem::size_of::<AtomicEntry>(), 16);
        assert_eq!(std::mem::align_of::<Cluster>(), 64);
        assert_eq!(std::mem::size_of::<Cluster>(), 64);
    }

    #[test]
    fn non_power_of_two_hash_uses_its_full_budget() {
        let tt = Tt::new(100);
        let requested = 100 * 1024 * 1024;
        assert!(!tt.clusters.len().is_power_of_two());
        assert_eq!(
            tt.clusters.len() * std::mem::size_of::<Cluster>(),
            requested
        );
        for key in [0, 1, u64::MAX / 2, u64::MAX - 1, u64::MAX] {
            assert!(tt.cluster_idx(key) < tt.clusters.len());
        }
    }

    #[test]
    fn cluster_holds_multiple_distinct_keys() {
        let tt = Tt::new(1);
        let idx = 5u64 & tt.mask;
        let k0 = (0x1111u64 << 48) | idx;
        let k1 = (0x2222u64 << 48) | idx;
        let k2 = (0x3333u64 << 48) | idx;
        let k3 = (0x4444u64 << 48) | idx;
        tt.store(k0, Move::NONE, 10, 10, 5, Bound::Exact, 0);
        tt.store(k1, Move::NONE, 20, 20, 5, Bound::Exact, 0);
        tt.store(k2, Move::NONE, 30, 30, 5, Bound::Exact, 0);
        tt.store(k3, Move::NONE, 40, 40, 5, Bound::Exact, 0);
        assert_eq!(tt.probe(k0, 0).unwrap().score, 10);
        assert_eq!(tt.probe(k1, 0).unwrap().score, 20);
        assert_eq!(tt.probe(k2, 0).unwrap().score, 30);
        assert_eq!(tt.probe(k3, 0).unwrap().score, 40);
    }

    #[test]
    fn fifth_distinct_key_evicts_lowest_quality_slot() {
        let tt = Tt::new(1);
        let idx = 7u64 & tt.mask;
        let k0 = (0x1111u64 << 48) | idx;
        let k1 = (0x2222u64 << 48) | idx;
        let k2 = (0x3333u64 << 48) | idx;
        let k3 = (0x4444u64 << 48) | idx;
        let k4 = (0x5555u64 << 48) | idx;
        tt.store(k0, Move::NONE, 1, 1, 1, Bound::Exact, 0);
        tt.store(k1, Move::NONE, 2, 2, 8, Bound::Exact, 0);
        tt.store(k2, Move::NONE, 3, 3, 8, Bound::Exact, 0);
        tt.store(k3, Move::NONE, 4, 4, 8, Bound::Exact, 0);
        tt.store(k4, Move::NONE, 5, 5, 8, Bound::Exact, 0);
        assert!(tt.probe(k0, 0).is_none());
        assert_eq!(tt.probe(k1, 0).unwrap().score, 2);
        assert_eq!(tt.probe(k2, 0).unwrap().score, 3);
        assert_eq!(tt.probe(k3, 0).unwrap().score, 4);
        assert_eq!(tt.probe(k4, 0).unwrap().score, 5);
    }

    #[test]
    fn matching_key_is_updated_in_place_not_duplicated() {
        let tt = Tt::new(1);
        let idx = 9u64 & tt.mask;
        let key = (0x1111u64 << 48) | idx;
        tt.store(key, Move::NONE, 1, 1, 2, Bound::Exact, 0);
        tt.store(key, Move::NONE, 99, 99, 10, Bound::Exact, 0);
        assert_eq!(tt.probe(key, 0).unwrap().score, 99);
        assert_eq!(tt.probe(key, 0).unwrap().depth, 10);
    }

    #[test]
    fn full_key_distinguishes_same_cluster_and_signature() {
        let tt = Tt::new(1);
        let idx = 11u64 & tt.mask;
        let k0 = (0x1111u64 << 48) | idx;
        let k1 = k0 ^ (1u64 << 32);
        tt.store(k0, Move::NONE, 10, 10, 5, Bound::Exact, 0);
        tt.store(k1, Move::NONE, 20, 20, 5, Bound::Exact, 0);
        assert_eq!(tt.probe(k0, 0).unwrap().score, 10);
        assert_eq!(tt.probe(k1, 0).unwrap().score, 20);
    }

    #[test]
    fn an_entry_is_published_as_one_value() {
        let tt = Tt::new(1);
        let key = (0x1234u64 << 48) | 13;
        tt.store(key, Move::NONE, 10, 10, 5, Bound::Exact, 0);
        let entry = &tt.clusters[tt.cluster_idx(key)].entries[0];
        let first = entry.value.load(Ordering::Relaxed);
        tt.store(key, Move::NONE, 11, 10, 5, Bound::Exact, 0);
        let second = entry.value.load(Ordering::Relaxed);
        assert_ne!(first, second);

        std::thread::scope(|scope| {
            scope.spawn(|| {
                for score in (0..200_000).map(|i| 10 + i % 2) {
                    tt.store(key, Move::NONE, score, 10, 5, Bound::Exact, 0);
                }
            });
            for _ in 0..200_000 {
                let value = entry.value.load(Ordering::Relaxed);
                assert!(value == first || value == second);
            }
        });
    }

    #[test]
    fn contended_cluster_never_returns_another_keys_payload() {
        let tt = Tt::new(1);
        let index = 23u64 & tt.mask;
        let barrier = Arc::new(Barrier::new(8));

        std::thread::scope(|scope| {
            for worker in 0..8u64 {
                let barrier = Arc::clone(&barrier);
                let tt = &tt;
                scope.spawn(move || {
                    let key = ((worker + 1) << 48) | index;
                    let score = 100 + worker as i32;
                    let eval = -100 - worker as i32;
                    barrier.wait();
                    for depth in (1..=16).cycle().take(20_000) {
                        tt.store(key, Move::NONE, score, eval, depth, Bound::Exact, 0);
                        if let Some(probe) = tt.probe(key, 0) {
                            assert_eq!(probe.score, score);
                            assert_eq!(probe.eval, eval);
                        }
                    }
                });
            }
        });
    }

    #[test]
    fn none_bound_round_trips() {
        let tt = Tt::new(1);
        tt.store(17, Move::NONE, 10, 20, 3, Bound::None, 0);
        assert!(tt.probe(17, 0).unwrap().bound == Bound::None);
    }

    #[test]
    fn packed_scores_never_alias_search_infinity() {
        let tt = Tt::new(1);
        tt.store(21, Move::NONE, i32::MAX, 0, 3, Bound::Exact, 0);
        tt.store(22, Move::NONE, i32::MIN, 0, 3, Bound::Exact, 0);
        assert_eq!(tt.probe(21, 0).unwrap().score, INFINITY - 1);
        assert_eq!(tt.probe(22, 0).unwrap().score, -INFINITY + 1);
    }

    #[test]
    fn mate_scores_are_normalized_between_plies() {
        let tt = Tt::new(1);
        let win = MATE_IN_MAX + 10;
        let loss = -win;
        tt.store(1, Move::NONE, win, 0, 5, Bound::Exact, 7);
        tt.store(2, Move::NONE, loss, 0, 5, Bound::Exact, 7);
        assert_eq!(tt.probe(1, 3).unwrap().score, win + 4);
        assert_eq!(tt.probe(2, 3).unwrap().score, loss - 4);
    }

    #[test]
    fn tablebase_scores_are_normalized_between_plies() {
        let tt = Tt::new(1);
        let win = TB_WIN - 7;
        let loss = -win;
        tt.store(1, Move::NONE, win, 0, 5, Bound::Exact, 7);
        tt.store(2, Move::NONE, loss, 0, 5, Bound::Exact, 7);
        assert_eq!(tt.probe(1, 3).unwrap().score, win + 4);
        assert_eq!(tt.probe(2, 3).unwrap().score, loss - 4);
    }

    #[test]
    fn generation_wraps_after_sixty_four_searches() {
        let tt = Tt::new(1);
        for _ in 0..64 {
            tt.new_search();
        }
        assert_eq!(tt.generation.load(Ordering::Relaxed), 0);
    }
}
