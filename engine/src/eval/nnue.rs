use crate::board::Board;
use crate::types::{Color, Move, Piece, PieceType};

use super::contract::{
    BUCKET_LAYOUT, FEATURES, HL, INPUT_BUCKETS, NETWORK_BYTES, OUTPUT_BUCKETS, PARAMETER_BYTES,
    PIECE_FEATURES, QA, QB, SCALE,
};
use super::simd::{activate_both, add_column, apply, sub_column};
use crate::MAX_PLY;

const RAW: &[u8] = include_bytes!("../../networks/bitfox.nnue");

const _: () = assert!(HL.is_multiple_of(8));
const _: () = assert!(RAW.len() == NETWORK_BYTES);
const _: () = assert!(NETWORK_BYTES - PARAMETER_BYTES < 64);

#[repr(C, align(64))]
struct Net {
    l0w: [i16; FEATURES * HL],
    l0b: [i16; HL],
    l1w: [i16; OUTPUT_BUCKETS * 2 * HL],
    l1b: [i16; OUTPUT_BUCKETS],
    _pad: [u8; NETWORK_BYTES - PARAMETER_BYTES],
}

#[cfg(not(target_endian = "little"))]
compile_error!("bitfox.nnue is stored little-endian");

// the file is laid out exactly like Net, so it can be used in place
static NET: Net = unsafe {
    std::mem::transmute::<[u8; NETWORK_BYTES], Net>(*include_bytes!("../../networks/bitfox.nnue"))
};

const BUCKETS: [usize; 64] = {
    const MIRROR: [usize; 8] = [0, 1, 2, 3, 3, 2, 1, 0];
    let mut buckets = [0usize; 64];
    let mut idx = 0;
    while idx < 64 {
        buckets[idx] = BUCKET_LAYOUT[(idx / 8) * 4 + MIRROR[idx % 8]];
        idx += 1;
    }
    buckets
};

#[inline(always)]
fn net() -> &'static Net {
    &NET
}

#[derive(Clone, Copy)]
struct Persp {
    pov: Color,
    orient: usize,
    flip: usize,
    bucket: usize,
}

impl Persp {
    #[inline]
    fn new(board: &Board, pov: Color) -> Persp {
        let orient = if pov == Color::White { 0 } else { 56 };
        let king = board.king_sq(pov).index() ^ orient;
        let flip = if king & 7 > 3 { 7 } else { 0 };
        Persp {
            pov,
            orient,
            flip,
            bucket: BUCKETS[king],
        }
    }

    #[inline]
    fn feature(&self, color: Color, pt: usize, sq: usize) -> usize {
        let s = (sq ^ self.orient) ^ self.flip;
        self.bucket * PIECE_FEATURES + 6 * 64 * (color != self.pov) as usize + 64 * pt + s
    }
}

#[inline(always)]
fn column(weights: &[i16], feature: usize) -> &[i16; HL] {
    let begin = feature * HL;
    debug_assert!(begin + HL <= weights.len());
    unsafe { &*(weights.as_ptr().add(begin) as *const [i16; HL]) }
}

#[inline]
fn col(feature: usize) -> &'static [i16; HL] {
    column(&net().l0w, feature)
}

fn refresh_one(acc: &mut [i16; HL], board: &Board, pov: Color) {
    acc.copy_from_slice(&net().l0b);
    let p = Persp::new(board, pov);
    for pt in PieceType::ALL {
        let pti = pt.index();
        for color in [Color::White, Color::Black] {
            for sq in board.pieces(color, pt) {
                add_column(acc, col(p.feature(color, pti, sq.index())));
            }
        }
    }
}

fn forward(white: &[i16; HL], black: &[i16; HL], board: &Board) -> i32 {
    let net = net();
    let (stm_acc, ntm_acc) = match board.side() {
        Color::White => (white, black),
        Color::Black => (black, white),
    };

    let bucket =
        ((board.occupancy().count() as i32 - 2) / 4).clamp(0, OUTPUT_BUCKETS as i32 - 1) as usize;
    let mut output = activate_both(
        stm_acc,
        column(&net.l1w, bucket * 2),
        ntm_acc,
        column(&net.l1w, bucket * 2 + 1),
    );

    output /= QA;
    output += net.l1b[bucket] as i32;
    (i64::from(output) * i64::from(SCALE) / i64::from(QA * QB)) as i32
}

pub fn evaluate(board: &Board) -> i32 {
    let mut white = [0i16; HL];
    let mut black = [0i16; HL];
    refresh_one(&mut white, board, Color::White);
    refresh_one(&mut black, board, Color::Black);
    forward(&white, &black, board)
}

#[derive(Clone, Copy, Default)]
struct Pending {
    adds: [u32; 2],
    subs: [u32; 2],
    n_adds: u8,
    n_subs: u8,
}

#[derive(Clone, Copy)]
#[repr(C, align(64))]
struct Accumulator {
    white: [i16; HL],
    black: [i16; HL],
    pending: [Pending; 2],
    clean: [bool; 2],
}

#[derive(Clone, Copy)]
#[repr(C, align(64))]
struct CacheEntry {
    acc: [i16; HL],
    pieces: [[crate::types::Bitboard; 6]; 2],
}

const CACHE_KEYS: usize = INPUT_BUCKETS * 2;

pub struct Nnue {
    stack: Box<[Accumulator]>,
    cache: Box<[[CacheEntry; CACHE_KEYS]; 2]>,
    idx: usize,
}

impl Default for Nnue {
    fn default() -> Nnue {
        let mut seed = CacheEntry {
            acc: [0; HL],
            pieces: [[crate::types::Bitboard::EMPTY; 6]; 2],
        };
        seed.acc.copy_from_slice(&net().l0b);
        Nnue {
            stack: vec![
                Accumulator {
                    white: [0; HL],
                    black: [0; HL],
                    pending: [Pending::default(); 2],
                    clean: [false; 2],
                };
                MAX_PLY + 8
            ]
            .into_boxed_slice(),
            cache: Box::new([[seed; CACHE_KEYS]; 2]),
            idx: 0,
        }
    }
}

impl Nnue {
    fn refresh_cached(&mut self, board: &Board, pov: Color, ply: usize) {
        let p = Persp::new(board, pov);
        let pi = (pov == Color::Black) as usize;
        let key = p.bucket * 2 + (p.flip != 0) as usize;
        let entry = &mut self.cache[pi][key];

        for color in [Color::White, Color::Black] {
            let ci = color.index();
            for pt in PieceType::ALL {
                let pti = pt.index();
                let now = board.pieces(color, pt);
                let was = entry.pieces[ci][pti];
                for sq in now & !was {
                    add_column(&mut entry.acc, col(p.feature(color, pti, sq.index())));
                }
                for sq in was & !now {
                    sub_column(&mut entry.acc, col(p.feature(color, pti, sq.index())));
                }
                entry.pieces[ci][pti] = now;
            }
        }

        let acc = pov_acc(&mut self.stack[ply], pov);
        acc.copy_from_slice(&entry.acc);
    }

    pub fn refresh(&mut self, board: &Board) {
        self.idx = 0;
        self.refresh_cached(board, Color::White, 0);
        self.refresh_cached(board, Color::Black, 0);
        self.stack[0].clean = [true, true];
    }

    pub fn eval(&mut self, board: &Board) -> i32 {
        self.materialize(board);
        let acc = &self.stack[self.idx];
        debug_assert_eq!(
            forward(&acc.white, &acc.black, board),
            evaluate(board),
            "incremental accumulator diverged"
        );
        forward(&acc.white, &acc.black, board)
    }

    fn materialize(&mut self, board: &Board) {
        let weights = &net().l0w;
        for pov in [0usize, 1] {
            if self.stack[self.idx].clean[pov] {
                continue;
            }
            let mut start = self.idx;
            while start > 0 && !self.stack[start].clean[pov] {
                start -= 1;
            }
            if !self.stack[start].clean[pov] {
                let color = if pov == 0 { Color::White } else { Color::Black };
                self.refresh_cached(board, color, self.idx);
                self.stack[self.idx].clean[pov] = true;
                continue;
            }
            for j in start..self.idx {
                let (lo, hi) = self.stack.split_at_mut(j + 1);
                let src = &lo[j];
                let dst = &mut hi[0];
                let p = dst.pending[pov];
                let (src_acc, dst_acc) = if pov == 0 {
                    (&src.white, &mut dst.white)
                } else {
                    (&src.black, &mut dst.black)
                };
                apply_pending(src_acc, dst_acc, &p, weights);
                dst.clean[pov] = true;
            }
        }
    }

    pub fn make(
        &mut self,
        board_after: &Board,
        m: Move,
        moved: Piece,
        captured: Option<(Piece, usize)>,
    ) {
        self.idx += 1;

        let us = moved.color();
        let from = m.from().index();
        let to = m.to().index();
        let moved_pt = moved.piece_type().index();
        let result_pt = m.promotion().map_or(moved_pt, |p| p.index());

        let rook = if m.is_castle() {
            let (rf, rt) = rook_castle(to);
            Some((rf, rt))
        } else {
            None
        };

        for pov in [Color::White, Color::Black] {
            let pi = (pov == Color::Black) as usize;
            let refresh = moved.piece_type() == PieceType::King
                && pov == us
                && king_bucket_changed(from, to, us);
            if refresh {
                self.refresh_cached(board_after, pov, self.idx);
                self.stack[self.idx].clean[pi] = true;
                continue;
            }

            let p = Persp::new(board_after, pov);
            let mut pending = Pending::default();

            pending.subs[0] = p.feature(us, moved_pt, from) as u32;
            pending.n_subs = 1;
            pending.adds[0] = p.feature(us, result_pt, to) as u32;
            pending.n_adds = 1;

            if let Some((cap, csq)) = captured {
                pending.subs[1] = p.feature(cap.color(), cap.piece_type().index(), csq) as u32;
                pending.n_subs = 2;
            }
            if let Some((rf, rt)) = rook {
                pending.subs[1] = p.feature(us, PieceType::Rook.index(), rf) as u32;
                pending.n_subs = 2;
                pending.adds[1] = p.feature(us, PieceType::Rook.index(), rt) as u32;
                pending.n_adds = 2;
            }

            self.stack[self.idx].pending[pi] = pending;
            self.stack[self.idx].clean[pi] = false;
        }
    }

    pub fn unmake(&mut self) {
        self.idx -= 1;
    }
}

fn apply_pending(src: &[i16; HL], dst: &mut [i16; HL], p: &Pending, weights: &[i16]) {
    let a0 = column(weights, p.adds[0] as usize);
    let s0 = column(weights, p.subs[0] as usize);
    match (p.n_adds, p.n_subs) {
        (1, 1) => apply::<1, 1>(src, dst, [a0, a0], [s0, s0]),
        (1, 2) => apply::<1, 2>(
            src,
            dst,
            [a0, a0],
            [s0, column(weights, p.subs[1] as usize)],
        ),
        _ => apply::<2, 2>(
            src,
            dst,
            [a0, column(weights, p.adds[1] as usize)],
            [s0, column(weights, p.subs[1] as usize)],
        ),
    }
}

#[inline]
fn pov_acc(acc: &mut Accumulator, pov: Color) -> &mut [i16; HL] {
    match pov {
        Color::White => &mut acc.white,
        Color::Black => &mut acc.black,
    }
}

#[inline]
fn king_bucket_changed(from: usize, to: usize, us: Color) -> bool {
    let orient = if us == Color::White { 0 } else { 56 };
    let of = from ^ orient;
    let ot = to ^ orient;
    BUCKETS[of] != BUCKETS[ot] || (of & 7 > 3) != (ot & 7 > 3)
}

fn rook_castle(king_to: usize) -> (usize, usize) {
    match king_to {
        6 => (7, 5),
        2 => (0, 3),
        62 => (63, 61),
        _ => (56, 59),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_accumulator_refreshes_before_evaluation() {
        let board = Board::startpos();
        let mut nnue = Nnue::default();
        assert_eq!(nnue.eval(&board), evaluate(&board));
    }

    #[test]
    fn every_feature_column_fits_the_network() {
        let weights = &net().l0w;
        assert_eq!(weights.len(), FEATURES * HL);
        assert_eq!(column(weights, 0).len(), HL);
        assert_eq!(column(weights, FEATURES - 1).len(), HL);

        for bucket in 0..INPUT_BUCKETS {
            for pov in [Color::White, Color::Black] {
                for flip in [0, 7] {
                    let p = Persp {
                        pov,
                        orient: if pov == Color::White { 0 } else { 56 },
                        flip,
                        bucket,
                    };
                    for color in [Color::White, Color::Black] {
                        for pt in PieceType::ALL {
                            for sq in 0..64 {
                                assert!(p.feature(color, pt.index(), sq) < FEATURES);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn fused_output_products_fit_i16_for_the_embedded_network() {
        let largest = net().l1w.iter().map(|&w| i32::from(w).abs()).max().unwrap();
        assert!(
            largest * QA <= i32::from(i16::MAX),
            "output weight {largest} wraps the fused product"
        );
    }

    #[test]
    fn output_accumulation_fits_i32_for_the_embedded_network() {
        let weights = &net().l1w;
        for bucket in 0..OUTPUT_BUCKETS {
            let begin = bucket * 2 * HL;
            let sum: i64 = weights[begin..begin + 2 * HL]
                .iter()
                .map(|&w| i64::from(w).abs())
                .sum();
            let accumulated = sum * i64::from(QA * QA);
            assert!(accumulated <= i64::from(i32::MAX));
            let activated = accumulated / i64::from(QA) + i64::from(net().l1b[bucket]).abs();
            let scaled = activated * i64::from(SCALE) / i64::from(QA * QB);
            assert!(scaled <= i64::from(i32::MAX));
        }
    }

    #[test]
    fn legal_accumulators_fit_i16_for_the_embedded_network() {
        for neuron in 0..HL {
            let largest = (0..FEATURES)
                .map(|feature| i32::from(net().l0w[feature * HL + neuron]).abs())
                .max()
                .unwrap();
            let bound = i32::from(net().l0b[neuron]).abs() + 32 * largest;
            assert!(
                bound <= i32::from(i16::MAX),
                "neuron {neuron} bound is {bound}"
            );
        }
    }

    #[test]
    fn embedded_network_padding_is_bounded() {
        assert_eq!(RAW.len(), NETWORK_BYTES);
    }

    #[test]
    fn vector_column_updates_match_wrapping_scalar_math() {
        let mut acc = std::array::from_fn(|i| (i as i16).wrapping_mul(193));
        let column = std::array::from_fn(|i| (i as i16).wrapping_mul(-271));
        let before = acc;
        let expected_add = std::array::from_fn(|i| before[i].wrapping_add(column[i]));
        add_column(&mut acc, &column);
        assert_eq!(acc, expected_add);

        let expected_sub = std::array::from_fn(|i| expected_add[i].wrapping_sub(column[i]));
        sub_column(&mut acc, &column);
        assert_eq!(acc, expected_sub);
        assert_eq!(acc, before);
    }

    #[test]
    fn vector_pending_updates_match_wrapping_scalar_math() {
        let src = std::array::from_fn(|i| (i as i16).wrapping_mul(313));
        let a0 = std::array::from_fn(|i| (i as i16).wrapping_mul(-157));
        let a1 = std::array::from_fn(|i| (i as i16).wrapping_mul(211));
        let s0 = std::array::from_fn(|i| (i as i16).wrapping_mul(-97));
        let s1 = std::array::from_fn(|i| (i as i16).wrapping_mul(149));

        for (adds, subs) in [(1, 1), (1, 2), (2, 2)] {
            let mut dst = [0; HL];
            match (adds, subs) {
                (1, 1) => apply::<1, 1>(&src, &mut dst, [&a0, &a1], [&s0, &s1]),
                (1, 2) => apply::<1, 2>(&src, &mut dst, [&a0, &a1], [&s0, &s1]),
                _ => apply::<2, 2>(&src, &mut dst, [&a0, &a1], [&s0, &s1]),
            }
            let expected = std::array::from_fn(|i| {
                let mut value = src[i].wrapping_add(a0[i]);
                if adds == 2 {
                    value = value.wrapping_add(a1[i]);
                }
                value = value.wrapping_sub(s0[i]);
                if subs == 2 {
                    value = value.wrapping_sub(s1[i]);
                }
                value
            });
            assert_eq!(dst, expected);
        }
    }

    #[test]
    fn embedded_network_known_positions() {
        let fens = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        ];
        let scores = fens
            .iter()
            .map(|fen| evaluate(&Board::from_fen(fen).unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(scores, [80, -151, 33]);
    }
}

#[cfg(test)]
mod special_move_accumulator_tests {
    use super::*;

    fn assert_full_accumulators(nnue: &mut Nnue, board: &Board) {
        nnue.materialize(board);
        let mut white = [0; HL];
        let mut black = [0; HL];
        refresh_one(&mut white, board, Color::White);
        refresh_one(&mut black, board, Color::Black);
        let acc = &nnue.stack[nnue.idx];
        assert_eq!(acc.white, white, "white accumulator at {}", board.to_fen());
        assert_eq!(acc.black, black, "black accumulator at {}", board.to_fen());
        assert_eq!(nnue.eval(board), evaluate(board), "{}", board.to_fen());
    }

    #[test]
    fn exact_accumulators_survive_special_moves_and_unmake() {
        let cases: &[(&str, &[&str])] = &[
            ("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1", &["e1g1"]),
            ("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1", &["e1c1"]),
            ("r3k2r/8/8/8/8/8/8/4K3 b kq - 0 1", &["e8g8"]),
            ("r3k2r/8/8/8/8/8/8/4K3 b kq - 0 1", &["e8c8"]),
            ("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1", &["e5d6"]),
            ("4k3/8/8/8/3Pp3/8/8/4K3 b - d3 0 1", &["e4d3"]),
            ("1r2k3/P7/8/8/8/8/8/4K3 w - - 0 1", &["a7a8q"]),
            ("1r2k3/P7/8/8/8/8/8/4K3 w - - 0 1", &["a7b8n"]),
            ("4k3/8/8/8/8/8/p7/1R2K3 b - - 0 1", &["a2a1q"]),
            ("4k3/8/8/8/8/8/p7/1R2K3 b - - 0 1", &["a2b1n"]),
            (Board::START_FEN, &["e2e4", "a7a6", "e4e5", "d7d5", "e5d6"]),
            (
                Board::START_FEN,
                &[
                    "e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6", "b5a4", "g8f6", "e1g1",
                ],
            ),
        ];
        let mut nnue = Nnue::default();
        for &(fen, moves) in cases {
            for immediate in [false, true] {
                let mut board = Board::from_fen(fen).unwrap();
                nnue.refresh(&board);
                assert_full_accumulators(&mut nnue, &board);
                let mut played = Vec::new();
                for &uci in moves {
                    let mv = board
                        .generate_legal()
                        .as_slice()
                        .iter()
                        .copied()
                        .find(|m| m.to_uci() == uci)
                        .unwrap_or_else(|| panic!("{uci} not legal at {}", board.to_fen()));
                    let moved = board.piece_on(mv.from()).unwrap();
                    let captured = if mv.is_en_passant() {
                        Some((
                            Piece::new(board.side().flip(), PieceType::Pawn),
                            mv.to().index() ^ 8,
                        ))
                    } else if mv.is_capture() {
                        board.piece_on(mv.to()).map(|p| (p, mv.to().index()))
                    } else {
                        None
                    };
                    board.make_move(mv);
                    nnue.make(&board, mv, moved, captured);
                    played.push(mv);
                    if immediate {
                        assert_full_accumulators(&mut nnue, &board);
                    }
                }
                assert_full_accumulators(&mut nnue, &board);
                while let Some(mv) = played.pop() {
                    board.unmake_move(mv);
                    nnue.unmake();
                    assert_full_accumulators(&mut nnue, &board);
                }
                assert_eq!(board.to_fen(), fen);
            }
        }
    }
}
