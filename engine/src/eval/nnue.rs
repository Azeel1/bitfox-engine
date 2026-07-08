use crate::board::Board;
use crate::types::{Color, Move, Piece, PieceType};
use std::sync::OnceLock;

use crate::MAX_PLY;

const HL: usize = 768;
const INPUT_BUCKETS: usize = 10;
const OUTPUT_BUCKETS: usize = 8;
const FEATURES: usize = 768 * INPUT_BUCKETS;
const SCALE: i32 = 400;
const QA: i32 = 255;
const QB: i32 = 64;

#[rustfmt::skip]
const BUCKET_LAYOUT: [usize; 32] = [
    0, 1, 2, 3,
    4, 5, 6, 7,
    8, 8, 8, 8,
    9, 9, 9, 9,
    9, 9, 9, 9,
    9, 9, 9, 9,
    9, 9, 9, 9,
    9, 9, 9, 9,
];

struct Net {
    l0w: Vec<i16>,
    l0b: Vec<i16>,
    l1w: Vec<i16>,
    l1b: Vec<i16>,
    buckets: [usize; 64],
}

static NET: OnceLock<Net> = OnceLock::new();

fn net() -> &'static Net {
    NET.get_or_init(|| {
        const RAW: &[u8] = include_bytes!("../../networks/bitfox.nnue");
        let read = |off: usize, len: usize| -> Vec<i16> {
            (0..len)
                .map(|i| i16::from_le_bytes([RAW[off + 2 * i], RAW[off + 2 * i + 1]]))
                .collect()
        };

        let mut o = 0;
        let l0w = read(o, FEATURES * HL);
        o += FEATURES * HL * 2;
        let l0b = read(o, HL);
        o += HL * 2;
        let l1w = read(o, OUTPUT_BUCKETS * 2 * HL);
        o += OUTPUT_BUCKETS * 2 * HL * 2;
        let l1b = read(o, OUTPUT_BUCKETS);

        const MIRROR: [usize; 8] = [0, 1, 2, 3, 3, 2, 1, 0];
        let mut buckets = [0usize; 64];
        for (idx, b) in buckets.iter_mut().enumerate() {
            *b = BUCKET_LAYOUT[(idx / 8) * 4 + MIRROR[idx % 8]];
        }

        Net {
            l0w,
            l0b,
            l1w,
            l1b,
            buckets,
        }
    })
}

#[inline]
fn screlu(x: i16) -> i32 {
    let v = (x as i32).clamp(0, QA);
    v * v
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
fn add_column(acc: &mut [i16; HL], col: &[i16]) {
    use std::arch::aarch64::{vaddq_s16, vld1q_s16, vst1q_s16};
    unsafe {
        let mut i = 0;
        while i < HL {
            let a = vld1q_s16(acc.as_ptr().add(i));
            let w = vld1q_s16(col.as_ptr().add(i));
            vst1q_s16(acc.as_mut_ptr().add(i), vaddq_s16(a, w));
            i += 8;
        }
    }
}

#[cfg(not(all(target_arch = "aarch64", target_feature = "neon")))]
fn add_column(acc: &mut [i16; HL], col: &[i16]) {
    for (a, &w) in acc.iter_mut().zip(col) {
        *a = a.wrapping_add(w);
    }
}

#[derive(Clone, Copy)]
struct Persp {
    pov: Color,
    orient: usize,
    flip: usize,
    bucket: usize,
}

impl Persp {
    fn new(board: &Board, pov: Color) -> Persp {
        let orient = if pov == Color::White { 0 } else { 56 };
        let king = board.king_sq(pov).index() ^ orient;
        let flip = if king & 7 > 3 { 7 } else { 0 };
        Persp {
            pov,
            orient,
            flip,
            bucket: net().buckets[king],
        }
    }

    #[inline]
    fn feature(&self, color: Color, pt: usize, sq: usize) -> usize {
        let s = (sq ^ self.orient) ^ self.flip;
        self.bucket * 768 + 384 * (color != self.pov) as usize + 64 * pt + s
    }
}

#[inline]
fn col(feature: usize) -> &'static [i16] {
    &net().l0w[feature * HL..feature * HL + HL]
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
    let w = &net.l1w[bucket * 2 * HL..bucket * 2 * HL + 2 * HL];

    let mut output = 0i32;
    for i in 0..HL {
        output += screlu(stm_acc[i]) * w[i] as i32;
        output += screlu(ntm_acc[i]) * w[HL + i] as i32;
    }

    output /= QA;
    output += net.l1b[bucket] as i32;
    output * SCALE / (QA * QB)
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
struct Accumulator {
    white: [i16; HL],
    black: [i16; HL],
    pending: [Pending; 2],
    clean: [bool; 2],
}

pub struct Nnue {
    stack: Box<[Accumulator]>,
    idx: usize,
}

impl Default for Nnue {
    fn default() -> Nnue {
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
            idx: 0,
        }
    }
}

impl Nnue {
    pub fn refresh(&mut self, board: &Board) {
        self.idx = 0;
        refresh_one(&mut self.stack[0].white, board, Color::White);
        refresh_one(&mut self.stack[0].black, board, Color::Black);
        self.stack[0].clean = [true, true];
    }

    pub fn eval(&mut self, board: &Board) -> i32 {
        self.materialize();
        let acc = &self.stack[self.idx];
        debug_assert_eq!(
            forward(&acc.white, &acc.black, board),
            evaluate(board),
            "incremental accumulator diverged"
        );
        forward(&acc.white, &acc.black, board)
    }

    fn materialize(&mut self) {
        for pov in [0usize, 1] {
            if self.stack[self.idx].clean[pov] {
                continue;
            }
            let mut start = self.idx;
            while !self.stack[start].clean[pov] {
                start -= 1;
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
                apply_pending(src_acc, dst_acc, &p);
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
                let acc = pov_acc(&mut self.stack[self.idx], pov);
                refresh_one(acc, board_after, pov);
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

fn apply_pending(src: &[i16; HL], dst: &mut [i16; HL], p: &Pending) {
    let a0 = col(p.adds[0] as usize);
    let s0 = col(p.subs[0] as usize);
    match (p.n_adds, p.n_subs) {
        (1, 1) => apply::<1, 1>(src, dst, [a0, a0], [s0, s0]),
        (1, 2) => apply::<1, 2>(src, dst, [a0, a0], [s0, col(p.subs[1] as usize)]),
        _ => apply::<2, 2>(
            src,
            dst,
            [a0, col(p.adds[1] as usize)],
            [s0, col(p.subs[1] as usize)],
        ),
    }
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
fn apply<const NA: usize, const NS: usize>(
    src: &[i16; HL],
    dst: &mut [i16; HL],
    adds: [&[i16]; 2],
    subs: [&[i16]; 2],
) {
    use std::arch::aarch64::{vaddq_s16, vld1q_s16, vst1q_s16, vsubq_s16};
    unsafe {
        let mut i = 0;
        while i < HL {
            let mut v = vld1q_s16(src.as_ptr().add(i));
            v = vaddq_s16(v, vld1q_s16(adds[0].as_ptr().add(i)));
            if NA == 2 {
                v = vaddq_s16(v, vld1q_s16(adds[1].as_ptr().add(i)));
            }
            v = vsubq_s16(v, vld1q_s16(subs[0].as_ptr().add(i)));
            if NS == 2 {
                v = vsubq_s16(v, vld1q_s16(subs[1].as_ptr().add(i)));
            }
            vst1q_s16(dst.as_mut_ptr().add(i), v);
            i += 8;
        }
    }
}

#[cfg(not(all(target_arch = "aarch64", target_feature = "neon")))]
fn apply<const NA: usize, const NS: usize>(
    src: &[i16; HL],
    dst: &mut [i16; HL],
    adds: [&[i16]; 2],
    subs: [&[i16]; 2],
) {
    for i in 0..HL {
        let mut v = src[i].wrapping_add(adds[0][i]);
        if NA == 2 {
            v = v.wrapping_add(adds[1][i]);
        }
        v = v.wrapping_sub(subs[0][i]);
        if NS == 2 {
            v = v.wrapping_sub(subs[1][i]);
        }
        dst[i] = v;
    }
}

#[inline]
fn pov_acc(acc: &mut Accumulator, pov: Color) -> &mut [i16; HL] {
    match pov {
        Color::White => &mut acc.white,
        Color::Black => &mut acc.black,
    }
}

fn king_bucket_changed(from: usize, to: usize, us: Color) -> bool {
    let orient = if us == Color::White { 0 } else { 56 };
    let of = from ^ orient;
    let ot = to ^ orient;
    net().buckets[of] != net().buckets[ot] || (of & 7 > 3) != (ot & 7 > 3)
}

fn rook_castle(king_to: usize) -> (usize, usize) {
    match king_to {
        6 => (7, 5),
        2 => (0, 3),
        62 => (63, 61),
        _ => (56, 59),
    }
}
