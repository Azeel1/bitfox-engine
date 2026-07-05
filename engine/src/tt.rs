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

const CLUSTER_SIZE: usize = 3;

#[repr(align(64))]
struct Cluster {
    entries: [AtomicEntry; CLUSTER_SIZE],
}

impl Cluster {
    fn empty() -> Cluster {
        Cluster {
            entries: [
                AtomicEntry::empty(),
                AtomicEntry::empty(),
                AtomicEntry::empty(),
            ],
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
        let raw = (bytes / cluster_bytes).max(1);
        let count = if raw.is_power_of_two() {
            raw
        } else {
            raw.next_power_of_two() / 2
        };
        let count = count.max(1);
        self.clusters = (0..count).map(|_| Cluster::empty()).collect();
        self.mask = (count - 1) as u64;
        self.generation.store(0, Ordering::Relaxed);
    }

    pub fn clear(&self) {
        for c in self.clusters.iter() {
            for e in c.entries.iter() {
                e.data.store(0, Ordering::Relaxed);
                e.meta.store(0, Ordering::Relaxed);
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
                let meta = e.meta.load(Ordering::Relaxed);
                let depth = (meta & 0xff) as u8;
                let gen = ((meta >> 8) as u8) >> 2;
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
        (key & self.mask) as usize
    }

    pub fn probe(&self, key: u64, ply: usize) -> Option<Probe> {
        let key16 = (key >> 48) as u16;
        let cluster = &self.clusters[self.cluster_idx(key)];
        for e in cluster.entries.iter() {
            let data = e.data.load(Ordering::Relaxed);
            let meta = e.meta.load(Ordering::Relaxed);
            let depth = (meta & 0xff) as u8;
            if depth == 0 || (data >> 48) as u16 != key16 {
                continue;
            }
            let gen_bound = ((meta >> 8) & 0xff) as u8;
            let mut score = (data >> 16) as u16 as i16 as i32;
            if score >= MATE_IN_MAX {
                score -= ply as i32;
            } else if score <= -MATE_IN_MAX {
                score += ply as i32;
            }
            return Some(Probe {
                mv: Move::from_raw((data >> 32) as u16),
                score,
                eval: data as u16 as i16 as i32,
                depth: depth as i32 - 1,
                bound: match gen_bound & 0x3 {
                    1 => Bound::Exact,
                    2 => Bound::Lower,
                    _ => Bound::Upper,
                },
            });
        }
        None
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
        let cluster = &self.clusters[self.cluster_idx(key)];
        let generation = self.generation.load(Ordering::Relaxed);
        let mut target = None;
        for (i, e) in cluster.entries.iter().enumerate() {
            let data = e.data.load(Ordering::Relaxed);
            let meta = e.meta.load(Ordering::Relaxed);
            let cur_depth = (meta & 0xff) as u8;
            if cur_depth != 0 && (data >> 48) as u16 == key16 {
                target = Some(i);
                break;
            }
        }
        let target = target.unwrap_or_else(|| {
            let mut best = 0usize;
            let mut best_quality = i32::MAX;
            for (i, e) in cluster.entries.iter().enumerate() {
                let meta = e.meta.load(Ordering::Relaxed);
                let cur_depth = (meta & 0xff) as u8;
                if cur_depth == 0 {
                    return i;
                }
                let cur_gen = ((meta >> 8) as u8) >> 2;
                let age = generation.wrapping_sub(cur_gen) & 0x3f;
                let quality = cur_depth as i32 - age as i32 * 2;
                if quality < best_quality {
                    best_quality = quality;
                    best = i;
                }
            }
            best
        });

        let e = &cluster.entries[target];
        let data = e.data.load(Ordering::Relaxed);
        let meta = e.meta.load(Ordering::Relaxed);
        let cur_key = (data >> 48) as u16;
        let cur_depth = (meta & 0xff) as u8;
        let cur_gen = ((meta >> 8) as u8) >> 2;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_uses_the_full_power_of_two_budget() {
        for mb in [1usize, 4, 16, 64, 128, 256, 512] {
            let tt = Tt::new(mb);
            let requested = mb * 1024 * 1024;
            let actual = tt.clusters.len() * std::mem::size_of::<Cluster>();
            assert!(tt.clusters.len().is_power_of_two());
            assert!(
                actual <= requested,
                "{mb}MB: used {actual} bytes, over budget"
            );
            let raw = requested / std::mem::size_of::<Cluster>();
            if raw.is_power_of_two() {
                assert_eq!(actual, requested, "{mb}MB: wasted half the requested Hash");
            }
        }
    }

    #[test]
    fn resize_rounds_down_for_non_power_of_two_sizes() {
        let tt = Tt::new(100);
        let requested = 100 * 1024 * 1024;
        let raw = requested / std::mem::size_of::<Cluster>();
        let expected_clusters = raw.next_power_of_two() / 2;
        assert!(tt.clusters.len().is_power_of_two());
        assert_eq!(tt.clusters.len(), expected_clusters);
    }

    #[test]
    fn cluster_holds_multiple_distinct_keys() {
        let tt = Tt::new(1);
        let idx = 5u64 & tt.mask;
        let k0 = (0x1111u64 << 48) | idx;
        let k1 = (0x2222u64 << 48) | idx;
        let k2 = (0x3333u64 << 48) | idx;
        tt.store(k0, Move::NONE, 10, 10, 5, Bound::Exact, 0);
        tt.store(k1, Move::NONE, 20, 20, 5, Bound::Exact, 0);
        tt.store(k2, Move::NONE, 30, 30, 5, Bound::Exact, 0);
        assert_eq!(tt.probe(k0, 0).unwrap().score, 10);
        assert_eq!(tt.probe(k1, 0).unwrap().score, 20);
        assert_eq!(tt.probe(k2, 0).unwrap().score, 30);
    }

    #[test]
    fn fourth_distinct_key_evicts_lowest_quality_slot() {
        let tt = Tt::new(1);
        let idx = 7u64 & tt.mask;
        let k0 = (0x1111u64 << 48) | idx;
        let k1 = (0x2222u64 << 48) | idx;
        let k2 = (0x3333u64 << 48) | idx;
        let k3 = (0x4444u64 << 48) | idx;
        tt.store(k0, Move::NONE, 1, 1, 1, Bound::Exact, 0);
        tt.store(k1, Move::NONE, 2, 2, 8, Bound::Exact, 0);
        tt.store(k2, Move::NONE, 3, 3, 8, Bound::Exact, 0);
        tt.store(k3, Move::NONE, 4, 4, 8, Bound::Exact, 0);
        assert!(tt.probe(k0, 0).is_none());
        assert_eq!(tt.probe(k1, 0).unwrap().score, 2);
        assert_eq!(tt.probe(k2, 0).unwrap().score, 3);
        assert_eq!(tt.probe(k3, 0).unwrap().score, 4);
    }

    #[test]
    fn matching_key_is_updated_in_place_not_duplicated() {
        let tt = Tt::new(1);
        let idx = 9u64 & tt.mask;
        let k0 = (0x1111u64 << 48) | idx;
        tt.store(k0, Move::NONE, 1, 1, 2, Bound::Exact, 0);
        tt.store(k0, Move::NONE, 99, 99, 10, Bound::Exact, 0);
        assert_eq!(tt.probe(k0, 0).unwrap().score, 99);
        assert_eq!(tt.probe(k0, 0).unwrap().depth, 10);
    }
}
