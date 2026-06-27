#![allow(clippy::missing_safety_doc)]

use crate::board::Board;
use crate::eval::evaluate;
use crate::search::{Limits, Search};
use crate::types::{Color, Move, PieceType, Square};
use std::cell::RefCell;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};

thread_local! {
    static SEARCH: RefCell<Search> = RefCell::new(Search::new(64));
}

#[repr(C)]
pub struct SearchResult {
    best: u32,
    score: c_int,
    depth: c_int,
    seldepth: c_int,
    nodes: u64,
    pv_len: c_int,
    pv: [u32; 64],
}

#[repr(C)]
pub struct MoveInfo {
    legal: c_int,
    capture: c_int,
    castle: c_int,
    ep: c_int,
    promo: c_int,
    check: c_int,
    status: c_int,
}

#[repr(C)]
pub struct PositionInfo {
    material: c_int,
    legal: c_int,
    captures: c_int,
    checks: c_int,
    promotions: c_int,
    in_check: c_int,
}

fn pack(m: Move) -> u32 {
    let from = m.from().index() as u32;
    let to = m.to().index() as u32;
    let promo = m.promotion().map(|pt| pt.index() as u32).unwrap_or(0);
    let flag = if m.is_castle() {
        3
    } else if m.is_en_passant() {
        2
    } else if m.is_double_pawn() {
        1
    } else {
        0
    };
    from | (to << 6) | (promo << 12) | (flag << 15)
}

unsafe fn board_ref<'a>(ptr: *mut Board) -> &'a mut Board {
    &mut *ptr
}

fn match_move(board: &mut Board, from: c_int, to: c_int, promo: c_int) -> Option<Move> {
    let legal = board.generate_legal();
    for i in 0..legal.len() {
        let m = legal.moves[i];
        let mp = m.promotion().map(|pt| pt.index() as c_int).unwrap_or(0);
        if m.from().index() as c_int == from && m.to().index() as c_int == to && mp == promo {
            return Some(m);
        }
    }
    None
}

fn status_of(board: &mut Board) -> c_int {
    if board.halfmove() >= 100 {
        return 3;
    }
    if board.is_threefold() {
        return 4;
    }
    if board.insufficient_material() {
        return 5;
    }
    let legal = board.generate_legal();
    if legal.is_empty() {
        return if board.in_check(board.side()) { 1 } else { 2 };
    }
    0
}

#[no_mangle]
pub extern "C" fn cc_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}

#[no_mangle]
pub extern "C" fn cc_new() -> *mut Board {
    Box::into_raw(Box::new(Board::startpos()))
}

#[no_mangle]
pub unsafe extern "C" fn cc_free(ptr: *mut Board) {
    if !ptr.is_null() {
        drop(Box::from_raw(ptr));
    }
}

#[no_mangle]
pub extern "C" fn cc_tt_clear() {
    SEARCH.with(|s| s.borrow_mut().clear());
}

#[no_mangle]
pub extern "C" fn cc_tt_resize(mb: c_int) {
    SEARCH.with(|s| s.borrow_mut().resize_tt(mb.max(1) as usize));
}

#[no_mangle]
pub unsafe extern "C" fn cc_set_fen(ptr: *mut Board, fen: *const c_char) -> c_int {
    let b = board_ref(ptr);
    let s = match CStr::from_ptr(fen).to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    match Board::from_fen(s) {
        Some(nb) => {
            *b = nb;
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn cc_get_fen(ptr: *mut Board, out: *mut c_char, size: c_int) -> c_int {
    let b = board_ref(ptr);
    let fen = b.to_fen();
    let bytes = fen.as_bytes();
    if bytes.len() + 1 > size as usize {
        return 0;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), out as *mut u8, bytes.len());
    *out.add(bytes.len()) = 0;
    1
}

#[no_mangle]
pub unsafe extern "C" fn cc_perft(ptr: *mut Board, depth: c_int) -> u64 {
    board_ref(ptr).perft(depth.max(0) as u32)
}

#[no_mangle]
pub unsafe extern "C" fn cc_gen_legal(ptr: *mut Board, out: *mut u32) -> c_int {
    let b = board_ref(ptr);
    let legal = b.generate_legal();
    for i in 0..legal.len() {
        *out.add(i) = pack(legal.moves[i]);
    }
    legal.len() as c_int
}

#[no_mangle]
pub unsafe extern "C" fn cc_legal_to(ptr: *mut Board, from: c_int, out: *mut c_int) -> c_int {
    let b = board_ref(ptr);
    let legal = b.generate_legal();
    let mut seen = 0u64;
    let mut count = 0;
    for i in 0..legal.len() {
        let m = legal.moves[i];
        if m.from().index() as c_int == from {
            let to = m.to().index();
            if seen & (1u64 << to) == 0 {
                seen |= 1u64 << to;
                *out.add(count) = to as c_int;
                count += 1;
            }
        }
    }
    count as c_int
}

#[no_mangle]
pub unsafe extern "C" fn cc_premove_to(ptr: *mut Board, from: c_int, out: *mut c_int) -> c_int {
    use crate::movegen::{
        bishop_attacks, king_attacks, knight_attacks, pawn_attacks, queen_attacks, rook_attacks,
    };
    use crate::types::Castling;
    if !(0..64).contains(&from) {
        return 0;
    }
    let b = board_ref(ptr);
    let sq = Square::new(from as u8);
    let piece = match b.piece_on(sq) {
        Some(p) => p,
        None => return 0,
    };
    let color = piece.color();
    let occ = b.occupancy();
    let own = b.color_bb(color);
    let mut dests = match piece.piece_type() {
        PieceType::Knight => knight_attacks(sq),
        PieceType::Bishop => bishop_attacks(sq, occ),
        PieceType::Rook => rook_attacks(sq, occ),
        PieceType::Queen => queen_attacks(sq, occ),
        PieceType::King => king_attacks(sq),
        PieceType::Pawn => pawn_attacks(color, sq),
    };
    match piece.piece_type() {
        PieceType::Pawn => {
            let f = sq.file();
            let r = sq.rank();
            if color == Color::White && r < 7 {
                let one = Square::from_coords(f, r + 1);
                if !occ.contains(one) {
                    dests |= one.bb();
                    if r == 1 {
                        let two = Square::from_coords(f, r + 2);
                        if !occ.contains(two) {
                            dests |= two.bb();
                        }
                    }
                }
            } else if color == Color::Black && r > 0 {
                let one = Square::from_coords(f, r - 1);
                if !occ.contains(one) {
                    dests |= one.bb();
                    if r == 6 {
                        let two = Square::from_coords(f, r - 2);
                        if !occ.contains(two) {
                            dests |= two.bb();
                        }
                    }
                }
            }
        }
        PieceType::King => {
            let c = b.castling();
            if color == Color::White && from == 4 {
                if c.has(Castling::WHITE_KING) {
                    dests |= Square::new(6).bb();
                }
                if c.has(Castling::WHITE_QUEEN) {
                    dests |= Square::new(2).bb();
                }
            } else if color == Color::Black && from == 60 {
                if c.has(Castling::BLACK_KING) {
                    dests |= Square::new(62).bb();
                }
                if c.has(Castling::BLACK_QUEEN) {
                    dests |= Square::new(58).bb();
                }
            }
        }
        _ => {}
    }
    let mut n = 0;
    for d in dests {
        if own.contains(d) {
            continue;
        }
        *out.add(n) = d.index() as c_int;
        n += 1;
    }
    n as c_int
}

#[no_mangle]
pub unsafe extern "C" fn cc_do_move(
    ptr: *mut Board,
    from: c_int,
    to: c_int,
    promo: c_int,
) -> c_int {
    let b = board_ref(ptr);
    match match_move(b, from, to, promo) {
        Some(m) => {
            b.make_move(m);
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn cc_apply(
    ptr: *mut Board,
    from: c_int,
    to: c_int,
    promo: c_int,
    info: *mut MoveInfo,
) -> c_int {
    let b = board_ref(ptr);
    let mi = &mut *info;
    *mi = MoveInfo {
        legal: 0,
        capture: 0,
        castle: 0,
        ep: 0,
        promo: 0,
        check: 0,
        status: 0,
    };
    let m = match match_move(b, from, to, promo) {
        Some(m) => m,
        None => return 0,
    };
    mi.capture = m.is_capture() as c_int;
    mi.castle = m.is_castle() as c_int;
    mi.ep = m.is_en_passant() as c_int;
    mi.promo = m.promotion().map(|pt| pt.index() as c_int).unwrap_or(0);
    b.make_move(m);
    mi.legal = 1;
    mi.check = b.in_check(b.side()) as c_int;
    mi.status = status_of(b);
    1
}

#[no_mangle]
pub unsafe extern "C" fn cc_search(
    ptr: *mut Board,
    max_depth: c_int,
    movetime_ms: c_int,
    out: *mut SearchResult,
) {
    let b = board_ref(ptr);
    let limits = Limits {
        depth: if max_depth > 0 { Some(max_depth) } else { None },
        movetime: if movetime_ms > 0 {
            Some(movetime_ms as u64)
        } else {
            None
        },
        infinite: max_depth <= 0 && movetime_ms <= 0,
        ..Default::default()
    };
    let r = &mut *out;
    SEARCH.with(|s| {
        let mut s = s.borrow_mut();
        let best = s.think(b, &limits, false);
        r.best = pack(best);
        r.score = s.best_score();
        r.depth = s.completed_depth();
        r.seldepth = s.seldepth() as c_int;
        r.nodes = s.node_count();
        let pv = s.root_pv();
        r.pv_len = pv.len().min(64) as c_int;
        for (i, m) in pv.iter().take(64).enumerate() {
            r.pv[i] = pack(*m);
        }
    });
}

#[no_mangle]
pub extern "C" fn cc_move_uci(packed: u32, buf: *mut c_char) {
    let from = (packed & 0x3f) as u8;
    let to = ((packed >> 6) & 0x3f) as u8;
    let promo = ((packed >> 12) & 0x7) as usize;
    let out = unsafe { std::slice::from_raw_parts_mut(buf as *mut u8, 6) };
    out[0] = b'a' + (from & 7);
    out[1] = b'1' + (from >> 3);
    out[2] = b'a' + (to & 7);
    out[3] = b'1' + (to >> 3);
    if promo > 0 {
        out[4] = b"nbrq"[promo - 1];
        out[5] = 0;
    } else {
        out[4] = 0;
    }
}

#[no_mangle]
pub unsafe extern "C" fn cc_piece_at(ptr: *mut Board, sq: c_int) -> c_int {
    if !(0..64).contains(&sq) {
        return -1;
    }
    match board_ref(ptr).piece_on(Square::new(sq as u8)) {
        Some(p) => p.index() as c_int,
        None => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn cc_side(ptr: *mut Board) -> c_int {
    board_ref(ptr).side().index() as c_int
}

#[no_mangle]
pub unsafe extern "C" fn cc_in_check(ptr: *mut Board) -> c_int {
    let b = board_ref(ptr);
    b.in_check(b.side()) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn cc_status(ptr: *mut Board) -> c_int {
    status_of(board_ref(ptr))
}

#[no_mangle]
pub unsafe extern "C" fn cc_evaluate(ptr: *mut Board) -> c_int {
    evaluate(board_ref(ptr))
}

#[no_mangle]
pub unsafe extern "C" fn cc_position_info(ptr: *mut Board, out: *mut PositionInfo) {
    let b = board_ref(ptr);
    let info = &mut *out;
    const VALUE: [c_int; 6] = [100, 320, 330, 500, 900, 0];
    let mut material = 0;
    for pt in PieceType::ALL {
        material += (b.pieces(Color::White, pt).count() as c_int) * VALUE[pt.index()];
        material += (b.pieces(Color::Black, pt).count() as c_int) * VALUE[pt.index()];
    }
    let legal = b.generate_legal();
    let mut captures = 0;
    let mut checks = 0;
    let mut promos = 0;
    for i in 0..legal.len() {
        let m = legal.moves[i];
        if m.is_capture() {
            captures += 1;
        }
        if m.is_promotion() {
            promos += 1;
        }
        b.make_move(m);
        if b.in_check(b.side()) {
            checks += 1;
        }
        b.unmake_move(m);
    }
    info.material = material;
    info.legal = legal.len() as c_int;
    info.captures = captures;
    info.checks = checks;
    info.promotions = promos;
    info.in_check = b.in_check(b.side()) as c_int;
}

#[no_mangle]
pub extern "C" fn cc_nnue_load(_path: *const c_char) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn cc_nnue_eval(ptr: *mut Board) -> c_int {
    evaluate(board_ref(ptr))
}

#[no_mangle]
pub unsafe extern "C" fn cc_nnue_eval_scratch(ptr: *mut Board) -> c_int {
    evaluate(board_ref(ptr))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Square;

    #[test]
    fn premove_rejects_out_of_range_from() {
        let mut board = Board::startpos();
        let mut out = [0; 8];
        let count = unsafe { cc_premove_to(&mut board as *mut Board, 64, out.as_mut_ptr()) };
        assert_eq!(count, 0);
    }

    #[test]
    fn premove_includes_king_castle_targets() {
        let mut board = Board::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1")
            .expect("valid castling test position");
        let mut out = [0; 8];
        let count = unsafe {
            cc_premove_to(
                &mut board as *mut Board,
                Square::E1.index() as c_int,
                out.as_mut_ptr(),
            )
        };
        let dests: std::collections::BTreeSet<_> = (0..count).map(|i| out[i as usize]).collect();
        assert!(dests.contains(&(Square::new(6).index() as c_int)));
        assert!(dests.contains(&(Square::new(2).index() as c_int)));
    }
}
