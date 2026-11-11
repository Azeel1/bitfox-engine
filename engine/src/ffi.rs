#![allow(clippy::missing_safety_doc)]

use crate::board::Board;
use crate::eval::evaluate;
use crate::search::{Limits, Search};
use crate::types::{Bitboard, Castling, Color, Move, PieceType, Square};
use crate::MAX_PLY;
use std::cell::RefCell;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

const CC_ABI_VERSION: u32 = 1;
const MOVE_OUTPUT_CAPACITY: usize = 256;
const SQUARE_OUTPUT_CAPACITY: usize = 32;
const UCI_BUFFER_SIZE: usize = 6;
const DEFAULT_TT_MB: c_int = 64;
const MAX_TT_MB: c_int = 4096;

const CC_RESULT_OK: c_int = 0;
const CC_RESULT_NULL_POINTER: c_int = -1;
const CC_RESULT_INVALID_ARGUMENT: c_int = -2;
const CC_RESULT_BUFFER_TOO_SMALL: c_int = -3;
const CC_RESULT_BUSY: c_int = -4;
const CC_RESULT_INTERNAL_ERROR: c_int = -5;

thread_local! {
    static SEARCH: RefCell<Search> = RefCell::new(Search::new(DEFAULT_TT_MB as usize));
}

pub struct CcSearcher {
    search: Mutex<Search>,
    start_stop: Mutex<()>,
    stop: Arc<AtomicBool>,
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

impl SearchResult {
    const EMPTY: SearchResult = SearchResult {
        best: 0,
        score: 0,
        depth: 0,
        seldepth: 0,
        nodes: 0,
        pv_len: 0,
        pv: [0; 64],
    };
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

impl MoveInfo {
    const EMPTY: MoveInfo = MoveInfo {
        legal: 0,
        capture: 0,
        castle: 0,
        ep: 0,
        promo: 0,
        check: 0,
        status: 0,
    };
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

impl PositionInfo {
    const EMPTY: PositionInfo = PositionInfo {
        material: 0,
        legal: 0,
        captures: 0,
        checks: 0,
        promotions: 0,
        in_check: 0,
    };
}

unsafe fn write_output<T: Copy>(
    values: &[T],
    out: *mut T,
    capacity: usize,
    required: *mut usize,
) -> c_int {
    if required.is_null() {
        return CC_RESULT_NULL_POINTER;
    }
    required.write(values.len());
    if capacity > 0 && out.is_null() {
        return CC_RESULT_NULL_POINTER;
    }
    if capacity < values.len() {
        return CC_RESULT_BUFFER_TOO_SMALL;
    }
    if !values.is_empty() {
        std::ptr::copy_nonoverlapping(values.as_ptr(), out, values.len());
    }
    CC_RESULT_OK
}

unsafe fn write_text(
    value: &[u8],
    out: *mut c_char,
    capacity: usize,
    required: *mut usize,
) -> c_int {
    if required.is_null() {
        return CC_RESULT_NULL_POINTER;
    }
    required.write(value.len() + 1);
    if capacity > 0 && out.is_null() {
        return CC_RESULT_NULL_POINTER;
    }
    if capacity <= value.len() {
        return CC_RESULT_BUFFER_TOO_SMALL;
    }
    std::ptr::copy_nonoverlapping(value.as_ptr(), out.cast::<u8>(), value.len());
    *out.add(value.len()) = 0;
    CC_RESULT_OK
}

fn square_target_values(targets: Bitboard) -> ([c_int; 64], usize) {
    let mut values = [0; 64];
    let mut len = 0;
    for square in targets {
        values[len] = square.index() as c_int;
        len += 1;
    }
    (values, len)
}

fn valid_tt_size(mb: c_int) -> bool {
    (1..=MAX_TT_MB).contains(&mb)
}

fn with_thread_search(f: impl FnOnce(&mut Search)) -> c_int {
    SEARCH.with(|cell| {
        let Ok(mut search) = cell.try_borrow_mut() else {
            return CC_RESULT_BUSY;
        };
        f(&mut search);
        CC_RESULT_OK
    })
}

fn try_search_handle(handle: &CcSearcher) -> Result<MutexGuard<'_, Search>, c_int> {
    match handle.search.try_lock() {
        Ok(search) => Ok(search),
        Err(TryLockError::WouldBlock) => Err(CC_RESULT_BUSY),
        Err(TryLockError::Poisoned(_)) => Err(CC_RESULT_INTERNAL_ERROR),
    }
}

fn begin_owned_search(
    handle: &CcSearcher,
    board: *mut Board,
) -> Result<MutexGuard<'_, Search>, c_int> {
    let _control = handle
        .start_stop
        .lock()
        .map_err(|_| CC_RESULT_INTERNAL_ERROR)?;
    let search = try_search_handle(handle)?;
    if board.is_null() {
        return Err(CC_RESULT_NULL_POINTER);
    }
    search.reset_stop();
    Ok(search)
}

fn legal_targets(board: &mut Board, from: c_int) -> Bitboard {
    if !(0..64).contains(&from) {
        return Bitboard::EMPTY;
    }
    let mut targets = Bitboard::EMPTY;
    let legal = board.generate_legal();
    for m in legal.as_slice() {
        if m.from().index() as c_int == from {
            targets |= m.to().bb();
        }
    }
    targets
}

fn premove_targets(board: &Board, from: c_int) -> Bitboard {
    use crate::movegen::{
        bishop_attacks, king_attacks, knight_attacks, pawn_attacks, queen_attacks, rook_attacks,
    };

    if !(0..64).contains(&from) {
        return Bitboard::EMPTY;
    }
    let sq = Square::new(from as u8);
    let Some(piece) = board.piece_on(sq) else {
        return Bitboard::EMPTY;
    };
    let color = piece.color();
    let occ = board.occupancy();
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
            let castling = board.castling();
            if color == Color::White && from == 4 {
                if castling.has(Castling::WHITE_KING) {
                    dests |= Square::new(6).bb();
                }
                if castling.has(Castling::WHITE_QUEEN) {
                    dests |= Square::new(2).bb();
                }
            } else if color == Color::Black && from == 60 {
                if castling.has(Castling::BLACK_KING) {
                    dests |= Square::new(62).bb();
                }
                if castling.has(Castling::BLACK_QUEEN) {
                    dests |= Square::new(58).bb();
                }
            }
        }
        _ => {}
    }
    dests & !board.color_bb(color)
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

unsafe fn board_ref<'a>(ptr: *mut Board) -> Option<&'a Board> {
    ptr.as_ref()
}

unsafe fn board_mut<'a>(ptr: *mut Board) -> Option<&'a mut Board> {
    ptr.as_mut()
}

macro_rules! board_or {
    ($ptr:expr, $fallback:expr) => {
        match board_ref($ptr) {
            Some(b) => b,
            None => return $fallback,
        }
    };
}

macro_rules! board_mut_or {
    ($ptr:expr, $fallback:expr) => {
        match board_mut($ptr) {
            Some(b) => b,
            None => return $fallback,
        }
    };
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
    let legal = board.generate_legal();
    if legal.is_empty() {
        return if board.in_check(board.side()) { 1 } else { 2 };
    }
    if board.halfmove() >= 100 {
        return 3;
    }
    if board.is_threefold() {
        return 4;
    }
    if board.insufficient_material() {
        return 5;
    }
    0
}

#[no_mangle]
pub extern "C" fn cc_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}

#[no_mangle]
pub extern "C" fn cc_abi_version() -> u32 {
    CC_ABI_VERSION
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
pub extern "C" fn cc_tt_clear_ex() -> c_int {
    with_thread_search(Search::clear)
}

#[no_mangle]
pub extern "C" fn cc_tt_resize_ex(mb: c_int) -> c_int {
    if !valid_tt_size(mb) {
        return CC_RESULT_INVALID_ARGUMENT;
    }
    with_thread_search(|search| search.resize_tt(mb as usize))
}

#[no_mangle]
pub extern "C" fn cc_tt_clear() {
    let _ = cc_tt_clear_ex();
}

#[no_mangle]
pub extern "C" fn cc_tt_resize(mb: c_int) {
    let _ = with_thread_search(|search| search.resize_tt(mb.clamp(1, MAX_TT_MB) as usize));
}

#[no_mangle]
pub unsafe extern "C" fn cc_searcher_new(tt_mb: c_int, out: *mut *mut CcSearcher) -> c_int {
    if out.is_null() {
        return CC_RESULT_NULL_POINTER;
    }
    out.write(std::ptr::null_mut());
    if !valid_tt_size(tt_mb) {
        return CC_RESULT_INVALID_ARGUMENT;
    }
    let search = Search::new(tt_mb as usize);
    let stop = search.stop_signal();
    out.write(Box::into_raw(Box::new(CcSearcher {
        search: Mutex::new(search),
        start_stop: Mutex::new(()),
        stop,
    })));
    CC_RESULT_OK
}

#[no_mangle]
pub unsafe extern "C" fn cc_searcher_free(ptr: *mut CcSearcher) {
    if !ptr.is_null() {
        drop(Box::from_raw(ptr));
    }
}

#[no_mangle]
pub unsafe extern "C" fn cc_searcher_clear(ptr: *mut CcSearcher) -> c_int {
    let Some(handle) = ptr.as_ref() else {
        return CC_RESULT_NULL_POINTER;
    };
    let mut search = match try_search_handle(handle) {
        Ok(search) => search,
        Err(result) => return result,
    };
    search.clear();
    CC_RESULT_OK
}

#[no_mangle]
pub unsafe extern "C" fn cc_searcher_resize(ptr: *mut CcSearcher, tt_mb: c_int) -> c_int {
    if !valid_tt_size(tt_mb) {
        return CC_RESULT_INVALID_ARGUMENT;
    }
    let Some(handle) = ptr.as_ref() else {
        return CC_RESULT_NULL_POINTER;
    };
    let mut search = match try_search_handle(handle) {
        Ok(search) => search,
        Err(result) => return result,
    };
    search.resize_tt(tt_mb as usize);
    CC_RESULT_OK
}

#[no_mangle]
pub unsafe extern "C" fn cc_searcher_set_threads(ptr: *mut CcSearcher, threads: c_int) -> c_int {
    if !(1..=256).contains(&threads) {
        return CC_RESULT_INVALID_ARGUMENT;
    }
    let Some(handle) = ptr.as_ref() else {
        return CC_RESULT_NULL_POINTER;
    };
    let mut search = match try_search_handle(handle) {
        Ok(search) => search,
        Err(result) => return result,
    };
    search.set_threads(threads as usize);
    CC_RESULT_OK
}

#[no_mangle]
pub unsafe extern "C" fn cc_searcher_stop(ptr: *mut CcSearcher) -> c_int {
    let Some(handle) = ptr.as_ref() else {
        return CC_RESULT_NULL_POINTER;
    };
    let _control = match handle.start_stop.lock() {
        Ok(control) => control,
        Err(_) => return CC_RESULT_INTERNAL_ERROR,
    };
    handle.stop.store(true, Ordering::Relaxed);
    CC_RESULT_OK
}

#[no_mangle]
pub unsafe extern "C" fn cc_set_fen(ptr: *mut Board, fen: *const c_char) -> c_int {
    if fen.is_null() {
        return 0;
    }
    let b = board_mut_or!(ptr, 0);
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
pub unsafe extern "C" fn cc_get_fen_n(
    ptr: *mut Board,
    out: *mut c_char,
    capacity: usize,
    required: *mut usize,
) -> c_int {
    if required.is_null() {
        return CC_RESULT_NULL_POINTER;
    }
    required.write(0);
    if capacity > 0 && out.is_null() {
        return CC_RESULT_NULL_POINTER;
    }
    let Some(b) = board_ref(ptr) else {
        return CC_RESULT_NULL_POINTER;
    };
    let fen = b.to_fen();
    write_text(fen.as_bytes(), out, capacity, required)
}

#[no_mangle]
pub unsafe extern "C" fn cc_get_fen(ptr: *mut Board, out: *mut c_char, size: c_int) -> c_int {
    if size <= 0 {
        return 0;
    }
    let mut required = 0;
    (cc_get_fen_n(ptr, out, size as usize, &mut required) == CC_RESULT_OK) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn cc_perft(ptr: *mut Board, depth: c_int) -> u64 {
    if depth < 0 || depth as usize > MAX_PLY {
        return 0;
    }
    board_mut_or!(ptr, 0).perft(depth as u32)
}

#[no_mangle]
pub unsafe extern "C" fn cc_gen_legal_n(
    ptr: *mut Board,
    out: *mut u32,
    capacity: usize,
    required: *mut usize,
) -> c_int {
    if required.is_null() {
        return CC_RESULT_NULL_POINTER;
    }
    required.write(0);
    if capacity > 0 && out.is_null() {
        return CC_RESULT_NULL_POINTER;
    }
    let Some(b) = board_mut(ptr) else {
        return CC_RESULT_NULL_POINTER;
    };
    let legal = b.generate_legal();
    let mut moves = [0; MOVE_OUTPUT_CAPACITY];
    for (packed, mv) in moves.iter_mut().zip(legal.as_slice()) {
        *packed = pack(*mv);
    }
    write_output(&moves[..legal.len()], out, capacity, required)
}

#[no_mangle]
pub unsafe extern "C" fn cc_gen_legal(ptr: *mut Board, out: *mut u32) -> c_int {
    let mut required = 0;
    if cc_gen_legal_n(ptr, out, MOVE_OUTPUT_CAPACITY, &mut required) == CC_RESULT_OK {
        required as c_int
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn cc_legal_to_n(
    ptr: *mut Board,
    from: c_int,
    out: *mut c_int,
    capacity: usize,
    required: *mut usize,
) -> c_int {
    if required.is_null() {
        return CC_RESULT_NULL_POINTER;
    }
    required.write(0);
    if capacity > 0 && out.is_null() {
        return CC_RESULT_NULL_POINTER;
    }
    if !(0..64).contains(&from) {
        return CC_RESULT_INVALID_ARGUMENT;
    }
    let Some(b) = board_mut(ptr) else {
        return CC_RESULT_NULL_POINTER;
    };
    let (targets, len) = square_target_values(legal_targets(b, from));
    write_output(&targets[..len], out, capacity, required)
}

#[no_mangle]
pub unsafe extern "C" fn cc_legal_to(ptr: *mut Board, from: c_int, out: *mut c_int) -> c_int {
    let mut required = 0;
    if cc_legal_to_n(ptr, from, out, SQUARE_OUTPUT_CAPACITY, &mut required) == CC_RESULT_OK {
        required as c_int
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn cc_premove_to_n(
    ptr: *mut Board,
    from: c_int,
    out: *mut c_int,
    capacity: usize,
    required: *mut usize,
) -> c_int {
    if required.is_null() {
        return CC_RESULT_NULL_POINTER;
    }
    required.write(0);
    if capacity > 0 && out.is_null() {
        return CC_RESULT_NULL_POINTER;
    }
    if !(0..64).contains(&from) {
        return CC_RESULT_INVALID_ARGUMENT;
    }
    let Some(b) = board_ref(ptr) else {
        return CC_RESULT_NULL_POINTER;
    };
    let (targets, len) = square_target_values(premove_targets(b, from));
    write_output(&targets[..len], out, capacity, required)
}

#[no_mangle]
pub unsafe extern "C" fn cc_premove_to(ptr: *mut Board, from: c_int, out: *mut c_int) -> c_int {
    let mut required = 0;
    if cc_premove_to_n(ptr, from, out, SQUARE_OUTPUT_CAPACITY, &mut required) == CC_RESULT_OK {
        required as c_int
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn cc_do_move(
    ptr: *mut Board,
    from: c_int,
    to: c_int,
    promo: c_int,
) -> c_int {
    let b = board_mut_or!(ptr, 0);
    match match_move(b, from, to, promo) {
        Some(m) => {
            b.make_move(m);
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn cc_apply_ex(
    ptr: *mut Board,
    from: c_int,
    to: c_int,
    promo: c_int,
    info: *mut MoveInfo,
) -> c_int {
    if info.is_null() {
        return CC_RESULT_NULL_POINTER;
    }
    info.write(MoveInfo::EMPTY);
    let Some(b) = board_mut(ptr) else {
        return CC_RESULT_NULL_POINTER;
    };
    let mi = &mut *info;
    let m = match match_move(b, from, to, promo) {
        Some(m) => m,
        None => return CC_RESULT_INVALID_ARGUMENT,
    };
    mi.capture = m.is_capture() as c_int;
    mi.castle = m.is_castle() as c_int;
    mi.ep = m.is_en_passant() as c_int;
    mi.promo = m.promotion().map(|pt| pt.index() as c_int).unwrap_or(0);
    b.make_move(m);
    mi.legal = 1;
    mi.check = b.in_check(b.side()) as c_int;
    mi.status = status_of(b);
    CC_RESULT_OK
}

#[no_mangle]
pub unsafe extern "C" fn cc_apply(
    ptr: *mut Board,
    from: c_int,
    to: c_int,
    promo: c_int,
    info: *mut MoveInfo,
) -> c_int {
    (cc_apply_ex(ptr, from, to, promo, info) == CC_RESULT_OK) as c_int
}

fn search_limits(max_depth: c_int, movetime_ms: c_int) -> Result<Limits, c_int> {
    if max_depth < 0
        || movetime_ms < 0
        || max_depth > MAX_PLY as c_int
        || (max_depth == 0 && movetime_ms == 0)
    {
        return Err(CC_RESULT_INVALID_ARGUMENT);
    }
    Ok(Limits {
        depth: (max_depth > 0).then_some(max_depth),
        movetime: (movetime_ms > 0).then_some(movetime_ms as u64),
        ..Default::default()
    })
}

fn run_started_search(
    search: &mut Search,
    board: &mut Board,
    limits: &Limits,
    result: &mut SearchResult,
) {
    let best = search.think_started(board, limits, false);
    record_search_result(search, best, result);
}

fn run_search(search: &mut Search, board: &mut Board, limits: &Limits, result: &mut SearchResult) {
    let best = search.think(board, limits, false);
    record_search_result(search, best, result);
}

fn record_search_result(search: &Search, best: Move, result: &mut SearchResult) {
    result.best = pack(best);
    result.score = search.best_score();
    result.depth = search.completed_depth();
    result.seldepth = search.seldepth() as c_int;
    result.nodes = search.node_count();
    let pv = search.root_pv();
    result.pv_len = pv.len().min(64) as c_int;
    for (index, mv) in pv.iter().take(64).enumerate() {
        result.pv[index] = pack(*mv);
    }
}

#[no_mangle]
pub unsafe extern "C" fn cc_search_ex(
    ptr: *mut Board,
    max_depth: c_int,
    movetime_ms: c_int,
    out: *mut SearchResult,
) -> c_int {
    if out.is_null() {
        return CC_RESULT_NULL_POINTER;
    }
    out.write(SearchResult::EMPTY);
    let limits = match search_limits(max_depth, movetime_ms) {
        Ok(limits) => limits,
        Err(result) => return result,
    };
    let Some(board) = board_mut(ptr) else {
        return CC_RESULT_NULL_POINTER;
    };
    with_thread_search(|search| run_search(search, board, &limits, &mut *out))
}

#[no_mangle]
pub unsafe extern "C" fn cc_search(
    ptr: *mut Board,
    max_depth: c_int,
    movetime_ms: c_int,
    out: *mut SearchResult,
) {
    let _ = cc_search_ex(ptr, max_depth, movetime_ms, out);
}

#[no_mangle]
pub unsafe extern "C" fn cc_searcher_run(
    searcher: *mut CcSearcher,
    ptr: *mut Board,
    max_depth: c_int,
    movetime_ms: c_int,
    out: *mut SearchResult,
) -> c_int {
    if out.is_null() {
        return CC_RESULT_NULL_POINTER;
    }
    out.write(SearchResult::EMPTY);
    let limits = match search_limits(max_depth, movetime_ms) {
        Ok(limits) => limits,
        Err(result) => return result,
    };
    let Some(handle) = searcher.as_ref() else {
        return CC_RESULT_NULL_POINTER;
    };
    let mut search = match begin_owned_search(handle, ptr) {
        Ok(search) => search,
        Err(result) => return result,
    };
    let Some(board) = board_mut(ptr) else {
        return CC_RESULT_NULL_POINTER;
    };
    run_started_search(&mut search, board, &limits, &mut *out);
    CC_RESULT_OK
}

#[no_mangle]
pub unsafe extern "C" fn cc_move_uci_n(
    packed: u32,
    out: *mut c_char,
    capacity: usize,
    required: *mut usize,
) -> c_int {
    if required.is_null() {
        return CC_RESULT_NULL_POINTER;
    }
    required.write(0);
    if capacity > 0 && out.is_null() {
        return CC_RESULT_NULL_POINTER;
    }
    if packed >> 17 != 0 || ((packed >> 12) & 0x7) > 4 {
        return CC_RESULT_INVALID_ARGUMENT;
    }
    let (formatted, len) = format_uci(packed);
    write_output(&formatted[..len], out.cast::<u8>(), capacity, required)
}

fn format_uci(packed: u32) -> ([u8; UCI_BUFFER_SIZE], usize) {
    let mut out = [0; UCI_BUFFER_SIZE];
    if packed == 0 {
        out[..5].copy_from_slice(b"0000\0");
        return (out, 5);
    }
    let from = (packed & 0x3f) as u8;
    let to = ((packed >> 6) & 0x3f) as u8;
    let promo = ((packed >> 12) & 0x7) as usize;
    out[0] = b'a' + (from & 7);
    out[1] = b'1' + (from >> 3);
    out[2] = b'a' + (to & 7);
    out[3] = b'1' + (to >> 3);
    if (1..=4).contains(&promo) {
        out[4] = b"nbrq"[promo - 1];
        out[5] = 0;
        (out, 6)
    } else {
        out[4] = 0;
        (out, 5)
    }
}

#[no_mangle]
pub unsafe extern "C" fn cc_move_uci(packed: u32, buf: *mut c_char) {
    if buf.is_null() {
        return;
    }
    let (formatted, len) = format_uci(packed);
    std::ptr::copy_nonoverlapping(formatted.as_ptr(), buf.cast::<u8>(), len);
}

#[no_mangle]
pub unsafe extern "C" fn cc_piece_at(ptr: *mut Board, sq: c_int) -> c_int {
    if !(0..64).contains(&sq) {
        return -1;
    }
    match board_or!(ptr, -1).piece_on(Square::new(sq as u8)) {
        Some(p) => p.index() as c_int,
        None => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn cc_side(ptr: *mut Board) -> c_int {
    board_or!(ptr, 0).side().index() as c_int
}

#[no_mangle]
pub unsafe extern "C" fn cc_in_check(ptr: *mut Board) -> c_int {
    let b = board_or!(ptr, 0);
    b.in_check(b.side()) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn cc_status(ptr: *mut Board) -> c_int {
    status_of(board_mut_or!(ptr, 0))
}

#[no_mangle]
pub unsafe extern "C" fn cc_evaluate(ptr: *mut Board) -> c_int {
    evaluate(board_or!(ptr, 0))
}

#[no_mangle]
pub unsafe extern "C" fn cc_position_info_ex(ptr: *mut Board, out: *mut PositionInfo) -> c_int {
    if out.is_null() {
        return CC_RESULT_NULL_POINTER;
    }
    out.write(PositionInfo::EMPTY);
    let Some(b) = board_mut(ptr) else {
        return CC_RESULT_NULL_POINTER;
    };
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
    CC_RESULT_OK
}

#[no_mangle]
pub unsafe extern "C" fn cc_position_info(ptr: *mut Board, out: *mut PositionInfo) {
    let _ = cc_position_info_ex(ptr, out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Square;

    #[test]
    fn immutable_queries_can_share_a_board() {
        let board = Board::startpos();
        let expected_eval = evaluate(&board);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let board = &board;
                scope.spawn(move || {
                    let ptr = std::ptr::from_ref(board).cast_mut();
                    for _ in 0..16 {
                        let mut fen = [0; 128];
                        let mut targets = [0; SQUARE_OUTPUT_CAPACITY];
                        unsafe {
                            assert_eq!(cc_side(ptr), 0);
                            assert_eq!(cc_in_check(ptr), 0);
                            assert_eq!(cc_piece_at(ptr, 4), 5);
                            assert_eq!(cc_evaluate(ptr), expected_eval);
                            assert_eq!(cc_get_fen(ptr, fen.as_mut_ptr(), 128), 1);
                            assert_eq!(
                                CStr::from_ptr(fen.as_ptr()).to_str().unwrap(),
                                Board::START_FEN
                            );
                            assert_eq!(cc_premove_to(ptr, 12, targets.as_mut_ptr()), 4);
                        }
                    }
                });
            }
        });
        assert_eq!(board.to_fen(), Board::START_FEN);
    }

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

    #[test]
    fn a_null_board_is_refused_rather_than_dereferenced() {
        let null = std::ptr::null_mut::<Board>();
        let mut moves = [0u32; 256];
        let mut squares = [0i32; 32];
        let mut fen = [0 as c_char; 128];
        unsafe {
            assert_eq!(cc_perft(null, 1), 0);
            assert_eq!(cc_side(null), 0);
            assert_eq!(cc_in_check(null), 0);
            assert_eq!(cc_status(null), 0);
            assert_eq!(cc_evaluate(null), 0);
            assert_eq!(cc_piece_at(null, 0), -1);
            assert_eq!(cc_gen_legal(null, moves.as_mut_ptr()), 0);
            assert_eq!(cc_legal_to(null, 0, squares.as_mut_ptr()), 0);
            assert_eq!(cc_premove_to(null, 0, squares.as_mut_ptr()), 0);
            assert_eq!(cc_get_fen(null, fen.as_mut_ptr(), 128), 0);
            assert_eq!(cc_do_move(null, 0, 0, 0), 0);
        }
    }

    #[test]
    fn a_null_output_buffer_is_refused() {
        let mut board = Board::startpos();
        let b = &mut board as *mut Board;
        unsafe {
            assert_eq!(cc_gen_legal(b, std::ptr::null_mut()), 0);
            assert_eq!(cc_legal_to(b, 12, std::ptr::null_mut()), 0);
            assert_eq!(cc_premove_to(b, 12, std::ptr::null_mut()), 0);
            assert_eq!(cc_get_fen(b, std::ptr::null_mut(), 128), 0);
            assert_eq!(cc_set_fen(b, std::ptr::null()), 0);
            cc_move_uci(0, std::ptr::null_mut());
        }
    }

    #[test]
    fn invalid_scalar_arguments_are_bounded() {
        let mut board = Board::startpos();
        let b = &mut board as *mut Board;
        let mut out = [0; SQUARE_OUTPUT_CAPACITY];
        unsafe {
            assert_eq!(cc_legal_to(b, -1, out.as_mut_ptr()), 0);
            assert_eq!(cc_legal_to(b, 64, out.as_mut_ptr()), 0);
            assert_eq!(cc_perft(b, -1), 0);
            assert_eq!(cc_perft(b, MAX_PLY as c_int + 1), 0);
        }
    }

    #[test]
    fn move_uci_handles_null_moves_and_invalid_promotions() {
        let mut out = [b'x' as c_char; 6];
        unsafe { cc_move_uci(0, out.as_mut_ptr()) };
        assert_eq!(
            &out[..5],
            &[
                b'0' as c_char,
                b'0' as c_char,
                b'0' as c_char,
                b'0' as c_char,
                0,
            ]
        );

        let packed = 12 | (28 << 6) | (7 << 12);
        unsafe { cc_move_uci(packed, out.as_mut_ptr()) };
        assert_eq!(
            &out[..5],
            &[
                b'e' as c_char,
                b'2' as c_char,
                b'e' as c_char,
                b'4' as c_char,
                0
            ]
        );
    }

    #[test]
    fn null_boards_leave_struct_outputs_initialized() {
        let null = std::ptr::null_mut::<Board>();
        let mut move_info = MoveInfo {
            legal: 1,
            capture: 1,
            castle: 1,
            ep: 1,
            promo: 1,
            check: 1,
            status: 1,
        };
        let mut search = SearchResult {
            best: 1,
            score: 1,
            depth: 1,
            seldepth: 1,
            nodes: 1,
            pv_len: 1,
            pv: [1; 64],
        };
        let mut position = PositionInfo {
            material: 1,
            legal: 1,
            captures: 1,
            checks: 1,
            promotions: 1,
            in_check: 1,
        };
        unsafe {
            assert_eq!(cc_apply(null, 0, 0, 0, &mut move_info), 0);
            cc_search(null, 1, 0, &mut search);
            cc_position_info(null, &mut position);
        }
        assert_eq!(move_info.legal, 0);
        assert_eq!(move_info.status, 0);
        assert_eq!(search.best, 0);
        assert_eq!(search.nodes, 0);
        assert_eq!(search.pv_len, 0);
        assert_eq!(search.pv, [0; 64]);
        assert_eq!(position.material, 0);
        assert_eq!(position.legal, 0);
    }

    #[test]
    fn search_requires_a_positive_limit() {
        let mut board = Board::startpos();
        let mut result = SearchResult {
            best: 1,
            score: 1,
            depth: 1,
            seldepth: 1,
            nodes: 1,
            pv_len: 1,
            pv: [1; 64],
        };
        unsafe { cc_search(&mut board, 0, 0, &mut result) };
        assert_eq!(result.best, 0);
        assert_eq!(result.nodes, 0);
        assert_eq!(result.pv_len, 0);

        result.best = 1;
        unsafe { cc_search(&mut board, MAX_PLY as c_int + 1, 0, &mut result) };
        assert_eq!(result.best, 0);
        assert_eq!(result.nodes, 0);

        result.best = 1;
        unsafe { cc_search(&mut board, -1, 10, &mut result) };
        assert_eq!(result.best, 0);
        result.best = 1;
        unsafe { cc_search(&mut board, 1, -1, &mut result) };
        assert_eq!(result.best, 0);
    }

    #[test]
    fn terminal_search_scores_distinguish_checkmate_from_stalemate() {
        let mut searcher = std::ptr::null_mut();
        unsafe {
            assert_eq!(cc_searcher_new(1, &mut searcher), CC_RESULT_OK);
        }
        for (fen, expected_status, expected_score) in [
            ("7k/6Q1/6K1/8/8/8/8/8 b - - 0 1", 1, -crate::MATE),
            ("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1", 2, crate::DRAW),
        ] {
            let mut board = Board::from_fen(fen).unwrap();
            for owned in [false, true] {
                let mut result = SearchResult::EMPTY;
                unsafe {
                    assert_eq!(cc_status(&mut board), expected_status);
                    let status = if owned {
                        cc_searcher_run(searcher, &mut board, 1, 0, &mut result)
                    } else {
                        cc_search_ex(&mut board, 1, 0, &mut result)
                    };
                    assert_eq!(status, CC_RESULT_OK);
                }
                assert_eq!(result.best, 0);
                assert_eq!(result.pv_len, 0);
                assert_eq!(result.score, expected_score, "owned={owned}, {fen}");
                assert_eq!(board.to_fen(), fen);
            }
        }
        unsafe { cc_searcher_free(searcher) };
    }

    #[test]
    fn bounded_move_outputs_report_required_capacity() {
        let mut board = Board::startpos();
        let board = &mut board as *mut Board;
        let mut required = usize::MAX;
        let mut short = [u32::MAX; 4];

        unsafe {
            assert_eq!(
                cc_gen_legal_n(board, short.as_mut_ptr(), short.len(), std::ptr::null_mut()),
                CC_RESULT_NULL_POINTER
            );
            assert_eq!(
                cc_gen_legal_n(board, std::ptr::null_mut(), short.len(), &mut required),
                CC_RESULT_NULL_POINTER
            );
            assert_eq!(
                cc_gen_legal_n(board, short.as_mut_ptr(), short.len(), &mut required),
                CC_RESULT_BUFFER_TOO_SMALL
            );
        }
        assert_eq!(required, 20);
        assert_eq!(short, [u32::MAX; 4]);

        required = 0;
        unsafe {
            assert_eq!(
                cc_gen_legal_n(board, std::ptr::null_mut(), 0, &mut required),
                CC_RESULT_BUFFER_TOO_SMALL
            );
        }
        assert_eq!(required, 20);

        let mut exact = [0; 20];
        unsafe {
            assert_eq!(
                cc_gen_legal_n(board, exact.as_mut_ptr(), exact.len(), &mut required),
                CC_RESULT_OK
            );
        }
        assert_eq!(required, exact.len());
        assert!(exact.iter().all(|mv| *mv != 0));
    }

    #[test]
    fn bounded_square_outputs_do_not_write_a_prefix() {
        let mut board = Board::startpos();
        let board = &mut board as *mut Board;
        let mut required = 0;
        let mut short = [99];
        unsafe {
            assert_eq!(
                cc_legal_to_n(board, 12, short.as_mut_ptr(), short.len(), &mut required),
                CC_RESULT_BUFFER_TOO_SMALL
            );
            assert_eq!(
                cc_legal_to_n(board, -1, short.as_mut_ptr(), short.len(), &mut required),
                CC_RESULT_INVALID_ARGUMENT
            );
        }
        assert_eq!(short, [99]);
        assert_eq!(required, 0);
    }

    #[test]
    fn bounded_text_outputs_include_the_terminator_in_required() {
        let mut board = Board::startpos();
        let board = &mut board as *mut Board;
        let mut required = 0;
        let mut fen = [b'x' as c_char; 8];
        unsafe {
            assert_eq!(
                cc_get_fen_n(board, fen.as_mut_ptr(), fen.len(), &mut required),
                CC_RESULT_BUFFER_TOO_SMALL
            );
        }
        assert!(required > fen.len());
        assert_eq!(fen, [b'x' as c_char; 8]);

        let packed = 52 | (60 << 6) | (4 << 12);
        let mut uci = [b'x' as c_char; UCI_BUFFER_SIZE];
        unsafe {
            assert_eq!(
                cc_move_uci_n(packed, uci.as_mut_ptr(), 5, &mut required),
                CC_RESULT_BUFFER_TOO_SMALL
            );
        }
        assert_eq!(required, UCI_BUFFER_SIZE);
        assert_eq!(uci, [b'x' as c_char; UCI_BUFFER_SIZE]);

        unsafe {
            assert_eq!(
                cc_move_uci_n(packed, uci.as_mut_ptr(), uci.len(), &mut required),
                CC_RESULT_OK
            );
        }
        assert_eq!(required, UCI_BUFFER_SIZE);
        assert_eq!(
            &uci,
            &[
                b'e' as c_char,
                b'7' as c_char,
                b'e' as c_char,
                b'8' as c_char,
                b'q' as c_char,
                0,
            ]
        );

        unsafe {
            assert_eq!(
                cc_move_uci_n(
                    12 | (28 << 6) | (7 << 12),
                    uci.as_mut_ptr(),
                    uci.len(),
                    &mut required
                ),
                CC_RESULT_INVALID_ARGUMENT
            );
        }
        assert_eq!(required, 0);
    }

    #[test]
    fn checked_legacy_search_reports_busy_and_invalid_settings() {
        assert_eq!(cc_tt_resize_ex(0), CC_RESULT_INVALID_ARGUMENT);
        assert_eq!(cc_tt_resize_ex(MAX_TT_MB + 1), CC_RESULT_INVALID_ARGUMENT);

        let mut search = SearchResult::EMPTY;
        let mut position = PositionInfo::EMPTY;
        let mut move_info = MoveInfo::EMPTY;
        unsafe {
            assert_eq!(
                cc_search_ex(std::ptr::null_mut(), 1, 0, &mut search),
                CC_RESULT_NULL_POINTER
            );
            assert_eq!(
                cc_position_info_ex(std::ptr::null_mut(), &mut position),
                CC_RESULT_NULL_POINTER
            );
            assert_eq!(
                cc_apply_ex(std::ptr::null_mut(), 0, 0, 0, &mut move_info),
                CC_RESULT_NULL_POINTER
            );
        }

        let mut board = Board::startpos();
        let mut result = SearchResult::EMPTY;
        result.best = 1;
        result.nodes = 1;
        SEARCH.with(|cell| {
            let _borrow = cell.borrow_mut();
            assert_eq!(cc_tt_clear_ex(), CC_RESULT_BUSY);
            assert_eq!(
                unsafe { cc_search_ex(&mut board, 1, 0, &mut result) },
                CC_RESULT_BUSY
            );
        });
        assert_eq!(result.best, 0);
        assert_eq!(result.nodes, 0);
    }

    #[test]
    fn owned_searcher_has_explicit_state_and_busy_result() {
        assert_eq!(cc_abi_version(), CC_ABI_VERSION);
        let mut searcher = std::ptr::null_mut();
        unsafe {
            assert_eq!(
                cc_searcher_new(1, std::ptr::null_mut()),
                CC_RESULT_NULL_POINTER
            );
            assert_eq!(
                cc_searcher_clear(std::ptr::null_mut()),
                CC_RESULT_NULL_POINTER
            );
            assert_eq!(
                cc_searcher_set_threads(std::ptr::null_mut(), 1),
                CC_RESULT_NULL_POINTER
            );
            assert_eq!(
                cc_searcher_stop(std::ptr::null_mut()),
                CC_RESULT_NULL_POINTER
            );
            assert_eq!(
                cc_searcher_new(0, &mut searcher),
                CC_RESULT_INVALID_ARGUMENT
            );
        }
        assert!(searcher.is_null());

        unsafe {
            assert_eq!(cc_searcher_new(1, &mut searcher), CC_RESULT_OK);
        }
        assert!(!searcher.is_null());

        let mut board = Board::startpos();
        let mut result = SearchResult::EMPTY;
        result.best = 1;
        result.nodes = 1;
        unsafe {
            assert_eq!(
                cc_searcher_set_threads(searcher, 0),
                CC_RESULT_INVALID_ARGUMENT
            );
            assert_eq!(cc_searcher_set_threads(searcher, 2), CC_RESULT_OK);
            assert_eq!(cc_searcher_stop(searcher), CC_RESULT_OK);
            assert!((*searcher).stop.load(Ordering::Relaxed));
            let guard = (*searcher).search.lock().expect("search mutex");
            assert_eq!(cc_searcher_clear(searcher), CC_RESULT_BUSY);
            assert_eq!(
                cc_searcher_run(searcher, &mut board, 1, 0, &mut result),
                CC_RESULT_BUSY
            );
            assert_eq!(result.best, 0);
            assert_eq!(result.nodes, 0);
            drop(guard);

            assert_eq!(cc_searcher_resize(searcher, 2), CC_RESULT_OK);
            assert_eq!(
                cc_searcher_run(searcher, &mut board, 1, 0, &mut result),
                CC_RESULT_OK
            );
            assert_ne!(result.best, 0);
            assert!(result.nodes > 0);
            cc_searcher_free(searcher);
        }
    }

    #[test]
    fn search_handle_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CcSearcher>();
    }

    #[test]
    fn ffi_struct_layout_matches_the_c_abi() {
        assert_eq!(std::mem::size_of::<MoveInfo>(), 28);
        assert_eq!(std::mem::align_of::<MoveInfo>(), 4);
        assert_eq!(std::mem::size_of::<PositionInfo>(), 24);
        assert_eq!(std::mem::align_of::<PositionInfo>(), 4);
        assert_eq!(std::mem::size_of::<SearchResult>(), 288);
        assert_eq!(std::mem::align_of::<SearchResult>(), 8);
        assert_eq!(std::mem::offset_of!(SearchResult, nodes), 16);
        assert_eq!(std::mem::offset_of!(SearchResult, pv_len), 24);
        assert_eq!(std::mem::offset_of!(SearchResult, pv), 28);
    }

    #[test]
    fn per_square_output_fits_the_callers_buffer() {
        const CALLER_BUFFER: usize = 32;
        assert_eq!(CALLER_BUFFER, SQUARE_OUTPUT_CAPACITY);
        let mut worst = 0;
        for fen in [
            "3Q4/8/k7/8/3Q4/8/8/K2Q4 w - - 0 1",
            "k7/3PPP2/8/8/8/8/8/K7 w - - 0 1",
            "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        ] {
            let mut board = Board::from_fen(fen).expect("valid test fen");
            for from in 0..64 {
                let legal_count = legal_targets(&mut board, from).count() as usize;
                let premove_count = premove_targets(&board, from).count() as usize;
                let mut out = [0i32; CALLER_BUFFER];
                let legal =
                    unsafe { cc_legal_to(&mut board as *mut Board, from, out.as_mut_ptr()) };
                let premove =
                    unsafe { cc_premove_to(&mut board as *mut Board, from, out.as_mut_ptr()) };
                assert_eq!(legal as usize, legal_count.min(CALLER_BUFFER));
                assert_eq!(premove as usize, premove_count.min(CALLER_BUFFER));
                worst = worst.max(legal_count.max(premove_count));
            }
        }
        assert!(
            worst <= CALLER_BUFFER,
            "wrote {worst} entries for one square, callers only reserve {CALLER_BUFFER}"
        );
    }

    fn assert_owned_stop_after_start_boundary(threads: c_int) {
        let mut searcher = std::ptr::null_mut();
        unsafe {
            assert_eq!(cc_searcher_new(1, &mut searcher), CC_RESULT_OK);
            assert_eq!(cc_searcher_set_threads(searcher, threads), CC_RESULT_OK);
        }
        let handle = unsafe { &*searcher };
        let mut board = Board::startpos();
        let mut search = begin_owned_search(handle, &mut board).expect("begin owned search");
        std::thread::scope(|scope| {
            scope
                .spawn(|| unsafe {
                    let ptr = std::ptr::from_ref(handle).cast_mut();
                    assert_eq!(cc_searcher_clear(ptr), CC_RESULT_BUSY);
                    assert_eq!(cc_searcher_stop(ptr), CC_RESULT_OK);
                })
                .join()
                .expect("stop caller");
        });
        assert!(handle.stop.load(Ordering::Relaxed));
        let mut result = SearchResult::EMPTY;
        let limits = search_limits(3, 0).unwrap();
        run_started_search(&mut search, &mut board, &limits, &mut result);
        let cancelled_depth = result.depth;
        let cancelled_nodes = result.nodes;
        let retained_stop = handle.stop.load(Ordering::Relaxed);
        let best = result.best;
        let final_fen = board.to_fen();
        drop(search);
        unsafe { cc_searcher_free(searcher) };
        assert_eq!(
            cancelled_depth, 0,
            "threads={threads}: start erased cancellation"
        );
        assert_eq!(
            cancelled_nodes, 0,
            "threads={threads}: cancelled search ran nodes"
        );
        assert!(retained_stop);
        assert_ne!(best, 0);
        assert_eq!(final_fen, Board::START_FEN);
    }

    #[test]
    fn owned_stop_after_start_boundary_is_not_lost_1t() {
        assert_owned_stop_after_start_boundary(1);
    }

    #[test]
    fn owned_stop_after_start_boundary_is_not_lost_4t() {
        assert_owned_stop_after_start_boundary(4);
    }

    #[test]
    fn owned_busy_run_does_not_erase_active_stop_and_reuse_resets_idle_stop() {
        for threads in [1, 4] {
            let mut searcher = std::ptr::null_mut();
            unsafe {
                assert_eq!(cc_searcher_new(1, &mut searcher), CC_RESULT_OK);
                assert_eq!(cc_searcher_set_threads(searcher, threads), CC_RESULT_OK);
            }
            let handle = unsafe { &*searcher };
            let mut board = Board::startpos();
            let guard = begin_owned_search(handle, &mut board).expect("begin owned search");
            unsafe { assert_eq!(cc_searcher_stop(searcher), CC_RESULT_OK) };
            let mut result = SearchResult::EMPTY;
            result.best = 1;
            result.nodes = 1;
            unsafe {
                assert_eq!(
                    cc_searcher_run(searcher, &mut board, 3, 0, &mut result),
                    CC_RESULT_BUSY
                );
            }
            assert_eq!(result.best, 0);
            assert_eq!(result.nodes, 0);
            assert!(handle.stop.load(Ordering::Relaxed));
            drop(guard);
            unsafe {
                assert_eq!(cc_searcher_clear(searcher), CC_RESULT_OK);
                assert_eq!(cc_searcher_resize(searcher, 2), CC_RESULT_OK);
                assert_eq!(cc_searcher_stop(searcher), CC_RESULT_OK);
                assert_eq!(
                    cc_searcher_run(searcher, &mut board, 3, 0, &mut result),
                    CC_RESULT_OK
                );
                assert_eq!(
                    result.depth, 3,
                    "threads={threads}: stale idle stop blocked reuse"
                );
                assert!(result.nodes > 0);
                assert_ne!(result.best, 0);
                assert_eq!(board.to_fen(), Board::START_FEN);
                cc_searcher_free(searcher);
            }
        }
    }

    #[test]
    fn owned_null_board_preserves_idle_stop_and_busy_error_priority() {
        let mut searcher = std::ptr::null_mut();
        unsafe { assert_eq!(cc_searcher_new(1, &mut searcher), CC_RESULT_OK) };
        let handle = unsafe { &*searcher };
        let mut result = SearchResult::EMPTY;
        unsafe {
            assert_eq!(cc_searcher_stop(searcher), CC_RESULT_OK);
            assert_eq!(
                cc_searcher_run(searcher, std::ptr::null_mut(), 3, 0, &mut result),
                CC_RESULT_NULL_POINTER
            );
        }
        assert!(handle.stop.load(Ordering::Relaxed));
        assert_eq!(result.best, 0);
        assert_eq!(result.nodes, 0);
        let guard = handle.search.lock().expect("search mutex");
        unsafe {
            assert_eq!(
                cc_searcher_run(searcher, std::ptr::null_mut(), 3, 0, &mut result),
                CC_RESULT_BUSY
            );
        }
        assert!(handle.stop.load(Ordering::Relaxed));
        drop(guard);
        unsafe { cc_searcher_free(searcher) };
    }
}
