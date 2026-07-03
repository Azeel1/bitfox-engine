use crate::board::Board;
use shakmaty::fen::Fen;
use shakmaty::{CastlingMode, Chess};
use shakmaty_syzygy::{Tablebase, Wdl};
use std::sync::Arc;

pub struct Syzygy {
    tables: Tablebase<Chess>,
}

impl Syzygy {
    pub fn load(path: &str) -> Option<Arc<Syzygy>> {
        let mut tables = Tablebase::new();
        let mut added = 0;
        for dir in path.split(';') {
            if let Ok(n) = tables.add_directory(dir.trim()) {
                added += n;
            }
        }
        if added == 0 {
            return None;
        }
        Some(Arc::new(Syzygy { tables }))
    }

    #[inline]
    pub fn max_pieces(&self) -> usize {
        self.tables.max_pieces()
    }

    pub fn probe_wdl(&self, board: &Board) -> Option<Wdl> {
        let fen: Fen = board.to_fen().parse().ok()?;
        let pos: Chess = fen.into_position(CastlingMode::Standard).ok()?;
        self.tables.probe_wdl_after_zeroing(&pos).ok()
    }
}
