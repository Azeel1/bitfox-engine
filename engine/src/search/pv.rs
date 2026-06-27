use super::Search;
use crate::types::Move;
use crate::{MATE, MATE_IN_MAX};

impl Search {
    pub(super) fn update_pv(&mut self, ply: usize, m: Move) {
        self.pv[ply][0] = m;
        let child = self.pv_len[ply + 1];
        for i in 0..child {
            self.pv[ply][i + 1] = self.pv[ply + 1][i];
        }
        self.pv_len[ply] = child + 1;
    }

    pub(super) fn print_info(&self, depth: i32, score: i32) {
        let elapsed = self.start.elapsed().as_millis().max(1) as u64;
        let nps = self.nodes * 1000 / elapsed;
        let score_str = if score.abs() >= MATE_IN_MAX {
            let mate = if score > 0 {
                (MATE - score + 1) / 2
            } else {
                -(MATE + score + 1) / 2
            };
            format!("mate {mate}")
        } else {
            format!("cp {score}")
        };
        let mut pv = String::new();
        for i in 0..self.pv_len[0] {
            if i > 0 {
                pv.push(' ');
            }
            pv.push_str(&self.pv[0][i].to_uci());
        }
        println!(
            "info depth {depth} seldepth {} score {score_str} nodes {} nps {nps} time {elapsed} pv {pv}",
            self.seldepth.max(depth as usize),
            self.nodes
        );
    }
}
