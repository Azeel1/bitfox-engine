use super::{Limits, Search};
use crate::board::Board;
use crate::types::Move;
use crate::{INFINITY, TB_WIN_IN_MAX};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

struct HelperJob {
    search: Box<Search>,
    board: Box<Board>,
    limits: Arc<Limits>,
    result: mpsc::Sender<HelperResult>,
}

struct HelperResult {
    index: usize,
    search: Result<Box<Search>, ()>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SmpCandidate {
    slot: usize,
    root_move: Move,
    score: i32,
    depth: i32,
    pv_len: usize,
}

impl SmpCandidate {
    fn from_search(slot: usize, search: &Search) -> SmpCandidate {
        SmpCandidate {
            slot,
            root_move: search.pv[0][0],
            score: search.best_score,
            depth: search.completed_depth,
            pv_len: search.pv_len[0],
        }
    }

    fn is_valid(self) -> bool {
        self.depth > 0
            && self.pv_len > 0
            && !self.root_move.is_none()
            && self.score > -INFINITY
            && self.score < INFINITY
    }

    fn has_distance_score(self) -> bool {
        self.score >= TB_WIN_IN_MAX || self.score <= -TB_WIN_IN_MAX
    }
}

fn preferred_candidate(candidate: SmpCandidate, current: SmpCandidate) -> bool {
    candidate.depth > current.depth
        || (candidate.depth == current.depth && candidate.slot < current.slot)
}

#[cfg(feature = "stats")]
fn distinct_root_moves(candidates: &[SmpCandidate]) -> usize {
    candidates
        .iter()
        .enumerate()
        .filter(|(index, candidate)| {
            candidate.is_valid()
                && !candidates[..*index].iter().any(|previous| {
                    previous.is_valid() && previous.root_move == candidate.root_move
                })
        })
        .count()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SmpSelection {
    slot: usize,
    candidates: usize,
    distinct_moves: usize,
    distance_score: bool,
    winner_depth_gap: i32,
}

#[cold]
#[inline(never)]
fn select_smp(candidates: &[SmpCandidate]) -> Option<SmpSelection> {
    let mut leader = None;
    let mut distance_leader = None;
    let mut valid = 0usize;

    for &candidate in candidates {
        if !candidate.is_valid() {
            continue;
        }
        valid += 1;
        if leader.is_none_or(|current| preferred_candidate(candidate, current)) {
            leader = Some(candidate);
        }
        if candidate.has_distance_score()
            && distance_leader.is_none_or(|current: SmpCandidate| {
                candidate.score > current.score
                    || (candidate.score == current.score && preferred_candidate(candidate, current))
            })
        {
            distance_leader = Some(candidate);
        }
    }

    let leader = leader?;
    let selected = distance_leader.unwrap_or(leader);

    Some(SmpSelection {
        slot: selected.slot,
        candidates: valid,
        distinct_moves: {
            #[cfg(feature = "stats")]
            {
                distinct_root_moves(candidates)
            }
            #[cfg(not(feature = "stats"))]
            {
                0
            }
        },
        distance_score: distance_leader.is_some(),
        winner_depth_gap: leader.depth - selected.depth,
    })
}

enum HelperCommand {
    Search(HelperJob),
    Shutdown,
}

struct HelperThread {
    sender: mpsc::Sender<HelperCommand>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl HelperThread {
    fn new(index: usize) -> std::io::Result<HelperThread> {
        let (sender, receiver) = mpsc::channel();
        let handle = std::thread::Builder::new()
            .name(format!("bitfox-helper-{}", index + 1))
            .spawn(move || {
                while let Ok(command) = receiver.recv() {
                    match command {
                        HelperCommand::Search(job) => {
                            let HelperJob {
                                mut search,
                                mut board,
                                limits,
                                result,
                            } = job;
                            let completed =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    search.run(&mut board, &limits, false);
                                }));
                            let search = completed.map(|_| search).map_err(|_| ());
                            let _ = result.send(HelperResult { index, search });
                        }
                        HelperCommand::Shutdown => break,
                    }
                }
            })?;
        Ok(HelperThread {
            sender,
            handle: Some(handle),
        })
    }

    fn start(&self, job: HelperJob) -> Result<(), HelperJob> {
        self.sender
            .send(HelperCommand::Search(job))
            .map_err(|error| match error.0 {
                HelperCommand::Search(job) => job,
                HelperCommand::Shutdown => unreachable!(),
            })
    }
}

impl Drop for HelperThread {
    fn drop(&mut self) {
        let _ = self.sender.send(HelperCommand::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[derive(Default)]
pub(super) struct HelperPool {
    workers: Vec<HelperThread>,
}

impl HelperPool {
    fn resize(&mut self, target: usize) -> usize {
        self.workers.truncate(target);
        while self.workers.len() < target {
            let index = self.workers.len();
            match HelperThread::new(index) {
                Ok(worker) => self.workers.push(worker),
                Err(_) => break,
            }
        }
        self.workers.len()
    }

    fn start(&self, index: usize, job: HelperJob) -> Result<(), HelperJob> {
        self.workers[index].start(job)
    }
}

struct StopHelpers(Arc<AtomicBool>);

impl Drop for StopHelpers {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

impl Search {
    fn new_helper(&self) -> Box<Search> {
        let mut helper = Box::new(Search::worker(
            Arc::clone(&self.tt),
            Arc::clone(&self.stop_signal),
            Arc::clone(&self.shared_nodes),
            self.params,
        ));
        helper.tb = self.tb.clone();
        helper
    }

    pub(super) fn configure_helpers(&mut self) {
        let target = self.threads.saturating_sub(1);
        let actual = self.helper_pool.resize(target);
        self.threads = actual + 1;
        self.helpers.truncate(actual);
        while self.helpers.len() < actual {
            let helper = self.new_helper();
            self.helpers.push(helper);
        }
    }

    pub fn think(&mut self, board: &mut Board, limits: &Limits, print: bool) -> Move {
        self.reset_stop();
        self.think_started(board, limits, print)
    }

    pub(crate) fn reset_stop(&self) {
        self.stop_signal.store(false, Ordering::Relaxed);
    }

    pub(crate) fn think_started(
        &mut self,
        board: &mut Board,
        limits: &Limits,
        print: bool,
    ) -> Move {
        self.shared_nodes.store(0, Ordering::Relaxed);
        self.tt.new_search();
        self.prepare(board, limits);

        let best = if self.stop || self.threads <= 1 || limits.nodes.is_some() {
            self.run_prepared(board, print)
        } else {
            let stop_helpers = StopHelpers(Arc::clone(&self.stop_signal));
            let (result_tx, result_rx) = mpsc::channel();
            let helper_limits = Arc::new(limits.clone());
            let mut ready = std::iter::repeat_with(|| None)
                .take(self.helpers.len())
                .collect::<Vec<_>>();
            let mut candidates = Vec::with_capacity(ready.len() + 1);
            let mut dispatched = 0usize;
            for (index, helper) in std::mem::take(&mut self.helpers).into_iter().enumerate() {
                if self.stop_requested() {
                    ready[index] = Some(helper);
                    continue;
                }
                let job = HelperJob {
                    search: helper,
                    board: Box::new(board.clone()),
                    limits: Arc::clone(&helper_limits),
                    result: result_tx.clone(),
                };
                match self.helper_pool.start(index, job) {
                    Ok(()) => dispatched += 1,
                    Err(job) => ready[index] = Some(job.search),
                }
            }
            drop(result_tx);

            let mut best = self.run_prepared(board, print);
            stop_helpers.0.store(true, Ordering::Relaxed);
            for _ in 0..dispatched {
                match result_rx.recv() {
                    Ok(HelperResult {
                        index,
                        search: Ok(helper),
                    }) => {
                        #[cfg(feature = "stats")]
                        {
                            self.stats.merge(&helper.stats);
                        }
                        candidates.push(SmpCandidate::from_search(index + 1, &helper));
                        ready[index] = Some(helper);
                    }
                    Ok(HelperResult {
                        search: Err(()), ..
                    }) => {}
                    Err(_) => break,
                }
            }
            candidates.push(SmpCandidate::from_search(0, self));
            if let Some(selection) = select_smp(&candidates) {
                #[cfg(feature = "stats")]
                {
                    self.stats.smp_roots += 1;
                    self.stats.smp_candidates += selection.candidates as u64;
                    self.stats.smp_disagreements += u64::from(selection.distinct_moves > 1);
                    self.stats.smp_main_wins += u64::from(selection.slot == 0);
                    self.stats.smp_helper_wins += u64::from(selection.slot > 0);
                    self.stats.smp_distance_roots += u64::from(selection.distance_score);
                    self.stats.smp_excluded += (candidates.len() - selection.candidates) as u64;
                    self.stats.smp_winner_depth_gap += selection.winner_depth_gap as u64;
                }
                if selection.slot > 0 {
                    self.adopt_helper_result(ready[selection.slot - 1].as_ref().unwrap());
                    best = self.pv[0][0];
                }
            }
            self.helpers = ready
                .into_iter()
                .map(|helper| helper.unwrap_or_else(|| self.new_helper()))
                .collect();
            best
        };
        if print && !best.is_none() && (self.stop || self.threads > 1) {
            self.print_info(self.completed_depth, self.best_score);
        }
        best
    }

    pub(super) fn adopt_helper_result(&mut self, helper: &Search) {
        let len = helper.pv_len[0];
        self.pv[0][..len].copy_from_slice(&helper.pv[0][..len]);
        self.pv_len[0] = len;
        self.best_score = helper.best_score;
        self.completed_depth = helper.completed_depth;
        self.seldepth = helper.seldepth;
    }

    fn run(&mut self, board: &mut Board, limits: &Limits, print: bool) -> Move {
        self.prepare(board, limits);
        self.run_prepared(board, print)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tt::Tt;
    use crate::SCORE_NONE;
    use std::sync::atomic::AtomicU64;
    use std::time::Duration;

    fn select_smp_leader(candidates: &[SmpCandidate]) -> Option<usize> {
        select_smp(candidates).map(|selection| selection.slot)
    }

    fn candidate(slot: usize, raw_move: u16, score: i32, depth: i32) -> SmpCandidate {
        SmpCandidate {
            slot,
            root_move: Move::from_raw(raw_move),
            score,
            depth,
            pv_len: 1,
        }
    }

    #[test]
    fn smp_leader_is_independent_of_result_order() {
        let candidates = [
            candidate(0, 1, 20, 10),
            candidate(1, 2, 15, 11),
            candidate(2, 3, 30, 11),
        ];
        let reversed = [candidates[2], candidates[0], candidates[1]];
        assert_eq!(select_smp_leader(&candidates), Some(1));
        assert_eq!(select_smp_leader(&reversed), Some(1));
    }

    #[test]
    fn smp_leader_prefers_main_at_equal_depth() {
        let candidates = [candidate(2, 3, 80, 12), candidate(0, 1, -20, 12)];
        assert_eq!(select_smp_leader(&candidates), Some(0));
    }

    #[test]
    fn smp_leader_ignores_incomplete_results() {
        let mut empty_pv = candidate(1, 2, 10, 12);
        empty_pv.pv_len = 0;
        let candidates = [
            candidate(0, 1, 10, 0),
            empty_pv,
            candidate(2, 0, 10, 12),
            candidate(3, 3, SCORE_NONE, 12),
            candidate(4, 4, INFINITY, 12),
            candidate(5, 5, -INFINITY, 12),
        ];
        assert_eq!(select_smp_leader(&candidates), None);
    }

    #[test]
    fn distance_scores_are_not_replaced_by_cp_scores() {
        let candidates = [candidate(0, 1, TB_WIN_IN_MAX, 9), candidate(1, 2, 400, 14)];
        assert_eq!(select_smp_leader(&candidates), Some(0));
    }

    #[test]
    fn distance_scores_keep_the_best_proven_result() {
        let candidates = [
            candidate(1, 1, -TB_WIN_IN_MAX - 20, 12),
            candidate(2, 2, -TB_WIN_IN_MAX, 10),
            candidate(3, 3, 200, 14),
        ];
        assert_eq!(select_smp_leader(&candidates), Some(2));
        let reversed = [candidates[2], candidates[1], candidates[0]];
        assert_eq!(select_smp_leader(&reversed), Some(2));
    }

    #[test]
    fn a_shallower_agreement_does_not_replace_the_deepest_move() {
        let candidates = [
            candidate(0, 1, 40, 12),
            candidate(1, 2, 10, 12),
            candidate(2, 2, 20, 11),
            candidate(3, 2, 30, 11),
        ];
        let selection = select_smp(&candidates).unwrap();
        assert_eq!(selection.slot, 0);
        assert_eq!(selection.candidates, 4);
        assert_eq!(selection.winner_depth_gap, 0);
        #[cfg(feature = "stats")]
        assert_eq!(selection.distinct_moves, 2);
    }

    #[test]
    fn agreeing_helpers_do_not_outrank_the_leader_on_score() {
        let candidates = [
            candidate(0, 1, 2_000, 12),
            candidate(1, 2, -2_000, 12),
            candidate(2, 2, -2_000, 11),
            candidate(3, 2, -2_000, 11),
        ];
        assert_eq!(select_smp_leader(&candidates), Some(0));
    }

    #[test]
    fn equal_depth_results_keep_the_lowest_slot() {
        let two_each = [
            candidate(0, 1, -100, 12),
            candidate(1, 2, 900, 12),
            candidate(2, 1, 20, 11),
            candidate(3, 2, 30, 11),
        ];
        let split = [
            candidate(0, 1, 10, 12),
            candidate(1, 2, 20, 12),
            candidate(2, 2, 30, 11),
            candidate(3, 3, 40, 11),
        ];
        assert_eq!(select_smp_leader(&two_each), Some(0));
        assert_eq!(select_smp_leader(&split), Some(0));
    }

    #[test]
    fn shallow_helpers_are_counted_but_never_selected() {
        let candidates = [
            candidate(0, 1, 10, 12),
            candidate(1, 2, 20, 10),
            candidate(2, 2, 30, 10),
            candidate(3, 2, 40, 10),
        ];
        let selection = select_smp(&candidates).unwrap();
        assert_eq!(selection.slot, 0);
        assert_eq!(selection.candidates, 4);
    }

    #[test]
    fn the_deepest_result_wins_over_agreeing_peers() {
        let candidates = [
            candidate(0, 1, 10, 13),
            candidate(1, 2, 20, 12),
            candidate(2, 2, 30, 13),
            candidate(3, 2, 40, 13),
        ];
        let selection = select_smp(&candidates).unwrap();
        assert_eq!(selection.slot, 0);
        assert_eq!(selection.winner_depth_gap, 0);
    }

    fn helper_job(
        result: mpsc::Sender<HelperResult>,
        stop: &Arc<AtomicBool>,
        nodes: &Arc<AtomicU64>,
    ) -> HelperJob {
        HelperJob {
            search: Box::new(Search::worker(
                Arc::new(Tt::new(1)),
                Arc::clone(stop),
                Arc::clone(nodes),
                crate::search::Params::default(),
            )),
            board: Box::new(Board::startpos()),
            limits: Arc::new(Limits {
                depth: Some(1),
                ..Default::default()
            }),
            result,
        }
    }

    #[test]
    fn an_abandoned_result_does_not_retire_the_worker() {
        let mut pool = HelperPool::default();
        assert_eq!(pool.resize(1), 1);
        let stop = Arc::new(AtomicBool::new(false));
        let nodes = Arc::new(AtomicU64::new(0));

        let (abandoned_tx, abandoned_rx) = mpsc::channel();
        drop(abandoned_rx);
        assert!(pool
            .start(0, helper_job(abandoned_tx, &stop, &nodes))
            .is_ok());

        let (result_tx, result_rx) = mpsc::channel();
        assert!(pool.start(0, helper_job(result_tx, &stop, &nodes)).is_ok());
        let response = result_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("worker did not accept a second job");
        assert_eq!(response.index, 0);
        assert!(response.search.is_ok());
    }

    #[test]
    fn an_undispatched_helper_cannot_return_a_previous_positions_move() {
        let mut search = Search::new(1);
        search.set_threads(2);
        let stale = Board::startpos()
            .generate_legal()
            .as_slice()
            .iter()
            .copied()
            .find(|m| m.to_uci() == "e2e4")
            .unwrap();
        search.helpers[0].pv[0][0] = stale;
        search.helpers[0].pv_len[0] = 1;
        search.helpers[0].completed_depth = 64;
        search.helpers[0].best_score = 100;

        let worker = &mut search.helper_pool.workers[0];
        worker.sender.send(HelperCommand::Shutdown).unwrap();
        worker.handle.take().unwrap().join().unwrap();

        let mut board = Board::startpos();
        board.make_move(stale);
        let best = search.think(
            &mut board,
            &Limits {
                depth: Some(1),
                ..Default::default()
            },
            false,
        );
        assert!(
            board.generate_legal().as_slice().contains(&best),
            "undispatched helper returned stale move {}",
            best.to_uci()
        );
        assert_eq!(search.completed_depth, 1);
        assert_eq!(search.helpers.len(), 1);
    }
}
