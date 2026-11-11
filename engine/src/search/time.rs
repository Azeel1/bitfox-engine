use super::{Limits, Search};
use crate::board::Board;
use crate::types::Color;
use crate::MAX_PLY;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

impl Search {
    pub(super) fn prepare(&mut self, board: &Board, limits: &Limits) {
        self.nodes = 0;
        self.published = 0;
        self.stats = super::SearchStats::default();
        self.stop = self.stop_signal.load(Ordering::Relaxed);
        self.seldepth = 0;
        self.best_score = 0;
        self.completed_depth = 0;
        self.pv_len = [0; MAX_PLY + 1];
        self.cutoff_cnt = [0; MAX_PLY + 4];
        self.start = limits.started_at.unwrap_or_else(Instant::now);
        self.node_limit = limits.nodes;
        self.check_interval = limits
            .nodes
            .map(|nodes| (nodes / 64).clamp(1, 2048))
            .unwrap_or(2048);
        self.max_depth = limits
            .depth
            .unwrap_or(MAX_PLY as i32)
            .clamp(1, MAX_PLY as i32);
        self.infinite = limits.infinite;
        self.killers = [[crate::types::Move::NONE; 2]; MAX_PLY + 2];
        *self.ss_eval = [crate::SCORE_NONE; MAX_PLY + 4];
        self.ss_piece = [super::PIECE_NONE; MAX_PLY + 4];

        self.timed = false;
        self.fixed_time = limits.movetime.is_some();
        if let Some(mt) = limits.movetime {
            self.timed = true;
            let d = Duration::from_millis(mt.saturating_sub(5));
            self.soft = d;
            self.hard = d;
        } else if limits.wtime.is_some() || limits.btime.is_some() {
            self.timed = true;
            let (time, inc) = match board.side() {
                Color::White => (limits.wtime.unwrap_or(0), limits.winc),
                Color::Black => (limits.btime.unwrap_or(0), limits.binc),
            };
            let mtg = limits.movestogo.unwrap_or(25).max(1);
            let base = (time / mtg).saturating_add(inc.saturating_mul(3) / 4);
            let hard = base.saturating_mul(3).min(time.saturating_sub(10) / 2);
            let soft = base.min(hard);
            self.soft = Duration::from_millis(soft.max(1));
            self.hard = Duration::from_millis(hard.max(1));
        }
        if limits.infinite {
            self.timed = false;
        }
        self.check_stop();
    }

    pub(super) fn check_stop(&mut self) -> bool {
        let searched = self.publish_nodes();

        if let Some(nl) = self.node_limit {
            if searched >= nl {
                self.stop = true;
            }
        }
        if self.timed && self.start.elapsed() >= self.hard {
            self.stop = true;
        }
        self.stop_requested()
    }

    #[inline]
    pub(super) fn stop_requested(&mut self) -> bool {
        if self.stop
            || self.stop_signal.load(Ordering::Relaxed)
            || (self.timed && self.start.elapsed() >= self.hard)
        {
            self.stop = true;
            self.stop_signal.store(true, Ordering::Relaxed);
        }
        self.stop
    }

    pub(super) fn publish_nodes(&mut self) -> u64 {
        let delta = self.nodes - self.published;
        self.published = self.nodes;
        if delta == 0 {
            self.shared_nodes.load(Ordering::Relaxed)
        } else {
            self.shared_nodes.fetch_add(delta, Ordering::Relaxed) + delta
        }
    }

    #[inline]
    pub(super) fn helper_stop_requested(&mut self) -> bool {
        if !self.is_main && self.nodes & 127 == 0 {
            return self.stop_requested();
        }
        self.stop
    }

    #[inline]
    pub(super) fn soft_expired(&self, stability: u32) -> bool {
        if !self.timed {
            return false;
        }
        if self.fixed_time {
            return self.start.elapsed() >= self.hard;
        }
        let scale = [140u32, 115, 100, 92, 86, 82][stability.min(5) as usize];
        self.start.elapsed() >= (self.soft * scale / 100).min(self.hard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_allocation_saturates_on_large_uci_values() {
        let mut search = Search::new(1);
        let limits = Limits {
            wtime: Some(u64::MAX),
            winc: u64::MAX,
            movestogo: Some(1),
            ..Default::default()
        };

        search.prepare(&Board::startpos(), &limits);

        assert!(search.timed);
        assert!(search.soft > Duration::ZERO);
        assert!(search.hard > Duration::ZERO);
        assert!(search.soft <= search.hard);
    }

    #[test]
    fn large_increment_keeps_soft_deadline_within_hard_deadline() {
        let mut search = Search::new(1);
        search.prepare(
            &Board::startpos(),
            &Limits {
                wtime: Some(5000),
                winc: 5000,
                ..Default::default()
            },
        );

        assert_eq!(search.hard, Duration::from_millis(2495));
        assert_eq!(search.soft, search.hard);
        search.start = Instant::now() - search.hard;
        for stability in 0..=5 {
            assert!(search.soft_expired(stability));
        }
    }

    #[test]
    fn fixed_movetime_is_not_shortened_by_a_stable_best_move() {
        let mut search = Search::new(1);
        search.prepare(
            &Board::startpos(),
            &Limits {
                movetime: Some(1000),
                ..Default::default()
            },
        );

        assert_eq!(search.hard, Duration::from_millis(995));
        search.start = Instant::now() - Duration::from_millis(900);
        for stability in 0..=5 {
            assert!(!search.soft_expired(stability));
        }
        search.start = Instant::now() - search.hard;
        for stability in 0..=5 {
            assert!(search.soft_expired(stability));
        }

        search.prepare(
            &Board::startpos(),
            &Limits {
                wtime: Some(60_000),
                ..Default::default()
            },
        );
        assert!(!search.fixed_time);
        search.start = Instant::now() - search.soft * 82 / 100;
        assert!(search.soft_expired(5));
    }

    #[test]
    fn invalid_depth_limits_are_bounded() {
        let mut search = Search::new(1);
        let board = Board::startpos();

        search.prepare(
            &board,
            &Limits {
                depth: Some(i32::MIN),
                ..Default::default()
            },
        );
        assert_eq!(search.max_depth, 1);

        search.prepare(
            &board,
            &Limits {
                depth: Some(i32::MAX),
                ..Default::default()
            },
        );
        assert_eq!(search.max_depth, MAX_PLY as i32);
    }

    #[test]
    fn prepare_clears_cutoff_counts() {
        let mut search = Search::new(1);
        search.cutoff_cnt.fill(7);
        search.prepare(&Board::startpos(), &Limits::default());
        assert_eq!(search.cutoff_cnt, [0; MAX_PLY + 4]);
    }
}
