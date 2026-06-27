use crate::board::Board;
use crate::movegen::{bishop_attacks, knight_attacks, queen_attacks, rook_attacks};
use crate::types::{Bitboard, Color, PieceType};

const VALUE: [i32; 6] = [100, 320, 330, 500, 900, 0];
const PHASE: [i32; 6] = [0, 1, 1, 2, 4, 0];
const PHASE_MAX: i32 = 24;
const BISHOP_PAIR: i32 = 30;
const TEMPO: i32 = 15;
const KNIGHT_MOB: i32 = 4;
const BISHOP_MOB: i32 = 4;
const ROOK_MOB: i32 = 2;
const QUEEN_MOB: i32 = 1;
const ROOK_OPEN: i32 = 20;
const ROOK_SEMI: i32 = 10;
const DOUBLED: i32 = 12;
const ISOLATED: i32 = 15;
const SHIELD: i32 = 9;

#[rustfmt::skip]
const PST: [[i32; 64]; 5] = [
    [
          0,   0,   0,   0,   0,   0,   0,   0,
         50,  50,  50,  50,  50,  50,  50,  50,
         10,  10,  20,  30,  30,  20,  10,  10,
          5,   5,  10,  25,  25,  10,   5,   5,
          0,   0,   0,  20,  20,   0,   0,   0,
          5,  -5, -10,   0,   0, -10,  -5,   5,
          5,  10,  10, -20, -20,  10,  10,   5,
          0,   0,   0,   0,   0,   0,   0,   0,
    ],
    [
        -50, -40, -30, -30, -30, -30, -40, -50,
        -40, -20,   0,   0,   0,   0, -20, -40,
        -30,   0,  10,  15,  15,  10,   0, -30,
        -30,   5,  15,  20,  20,  15,   5, -30,
        -30,   0,  15,  20,  20,  15,   0, -30,
        -30,   5,  10,  15,  15,  10,   5, -30,
        -40, -20,   0,   5,   5,   0, -20, -40,
        -50, -40, -30, -30, -30, -30, -40, -50,
    ],
    [
        -20, -10, -10, -10, -10, -10, -10, -20,
        -10,   0,   0,   0,   0,   0,   0, -10,
        -10,   0,   5,  10,  10,   5,   0, -10,
        -10,   5,   5,  10,  10,   5,   5, -10,
        -10,   0,  10,  10,  10,  10,   0, -10,
        -10,  10,  10,  10,  10,  10,  10, -10,
        -10,   5,   0,   0,   0,   0,   5, -10,
        -20, -10, -10, -10, -10, -10, -10, -20,
    ],
    [
          0,   0,   0,   0,   0,   0,   0,   0,
          5,  10,  10,  10,  10,  10,  10,   5,
         -5,   0,   0,   0,   0,   0,   0,  -5,
         -5,   0,   0,   0,   0,   0,   0,  -5,
         -5,   0,   0,   0,   0,   0,   0,  -5,
         -5,   0,   0,   0,   0,   0,   0,  -5,
         -5,   0,   0,   0,   0,   0,   0,  -5,
          0,   0,   0,   5,   5,   0,   0,   0,
    ],
    [
        -20, -10, -10,  -5,  -5, -10, -10, -20,
        -10,   0,   0,   0,   0,   0,   0, -10,
        -10,   0,   5,   5,   5,   5,   0, -10,
         -5,   0,   5,   5,   5,   5,   0,  -5,
          0,   0,   5,   5,   5,   5,   0,  -5,
        -10,   5,   5,   5,   5,   5,   0, -10,
        -10,   0,   5,   0,   0,   0,   0, -10,
        -20, -10, -10,  -5,  -5, -10, -10, -20,
    ],
];

#[rustfmt::skip]
const KING_MG: [i32; 64] = [
    -30, -40, -40, -50, -50, -40, -40, -30,
    -30, -40, -40, -50, -50, -40, -40, -30,
    -30, -40, -40, -50, -50, -40, -40, -30,
    -30, -40, -40, -50, -50, -40, -40, -30,
    -20, -30, -30, -40, -40, -30, -30, -20,
    -10, -20, -20, -20, -20, -20, -20, -10,
     20,  20,   0,   0,   0,   0,  20,  20,
     20,  30,  10,   0,   0,  10,  30,  20,
];

#[rustfmt::skip]
const KING_EG: [i32; 64] = [
    -50, -40, -30, -20, -20, -30, -40, -50,
    -30, -20, -10,   0,   0, -10, -20, -30,
    -30, -10,  20,  30,  30,  20, -10, -30,
    -30, -10,  30,  40,  40,  30, -10, -30,
    -30, -10,  30,  40,  40,  30, -10, -30,
    -30, -10,  20,  30,  30,  20, -10, -30,
    -30, -30,   0,   0,   0,   0, -30, -30,
    -50, -30, -30, -30, -30, -30, -30, -50,
];

pub fn evaluate(board: &Board) -> i32 {
    let mut score = 0i32;
    let mut phase = 0i32;

    for pt in [
        PieceType::Pawn,
        PieceType::Knight,
        PieceType::Bishop,
        PieceType::Rook,
        PieceType::Queen,
    ] {
        let table = &PST[pt.index()];
        let value = VALUE[pt.index()];
        for sq in board.pieces(Color::White, pt) {
            score += value + table[sq.flip_vertical().index()];
            phase += PHASE[pt.index()];
        }
        for sq in board.pieces(Color::Black, pt) {
            score -= value + table[sq.index()];
            phase += PHASE[pt.index()];
        }
    }

    let occ = board.occupancy();
    let white = board.color_bb(Color::White);
    let black = board.color_bb(Color::Black);
    for sq in board.pieces(Color::White, PieceType::Knight) {
        score += KNIGHT_MOB * (knight_attacks(sq) & !white).count() as i32;
    }
    for sq in board.pieces(Color::Black, PieceType::Knight) {
        score -= KNIGHT_MOB * (knight_attacks(sq) & !black).count() as i32;
    }
    for sq in board.pieces(Color::White, PieceType::Bishop) {
        score += BISHOP_MOB * (bishop_attacks(sq, occ) & !white).count() as i32;
    }
    for sq in board.pieces(Color::Black, PieceType::Bishop) {
        score -= BISHOP_MOB * (bishop_attacks(sq, occ) & !black).count() as i32;
    }
    for sq in board.pieces(Color::White, PieceType::Rook) {
        score += ROOK_MOB * (rook_attacks(sq, occ) & !white).count() as i32;
    }
    for sq in board.pieces(Color::Black, PieceType::Rook) {
        score -= ROOK_MOB * (rook_attacks(sq, occ) & !black).count() as i32;
    }
    for sq in board.pieces(Color::White, PieceType::Queen) {
        score += QUEEN_MOB * (queen_attacks(sq, occ) & !white).count() as i32;
    }
    for sq in board.pieces(Color::Black, PieceType::Queen) {
        score -= QUEEN_MOB * (queen_attacks(sq, occ) & !black).count() as i32;
    }

    let wp = board.pieces(Color::White, PieceType::Pawn);
    let bp = board.pieces(Color::Black, PieceType::Pawn);

    for f in 0..8u32 {
        let file = file_bb(f);
        let wcount = (file & wp).count() as i32;
        let bcount = (file & bp).count() as i32;
        if wcount > 1 {
            score -= DOUBLED * (wcount - 1);
        }
        if bcount > 1 {
            score += DOUBLED * (bcount - 1);
        }
        let adj = (if f > 0 { file_bb(f - 1) } else { Bitboard::EMPTY })
            | (if f < 7 { file_bb(f + 1) } else { Bitboard::EMPTY });
        if wcount > 0 && (adj & wp).is_empty() {
            score -= ISOLATED * wcount;
        }
        if bcount > 0 && (adj & bp).is_empty() {
            score += ISOLATED * bcount;
        }
    }

    for sq in board.pieces(Color::White, PieceType::Rook) {
        let file = Bitboard(0x0101_0101_0101_0101u64 << sq.file());
        if (file & (wp | bp)).is_empty() {
            score += ROOK_OPEN;
        } else if (file & wp).is_empty() {
            score += ROOK_SEMI;
        }
    }
    for sq in board.pieces(Color::Black, PieceType::Rook) {
        let file = Bitboard(0x0101_0101_0101_0101u64 << sq.file());
        if (file & (wp | bp)).is_empty() {
            score -= ROOK_OPEN;
        } else if (file & bp).is_empty() {
            score -= ROOK_SEMI;
        }
    }

    let phase = phase.min(PHASE_MAX);
    let wk = board.king_sq(Color::White);
    let bk = board.king_sq(Color::Black);
    let wk_i = wk.flip_vertical().index();
    let bk_i = bk.index();
    score += (KING_MG[wk_i] * phase + KING_EG[wk_i] * (PHASE_MAX - phase)) / PHASE_MAX;
    score -= (KING_MG[bk_i] * phase + KING_EG[bk_i] * (PHASE_MAX - phase)) / PHASE_MAX;

    let mut shield = 0i32;
    {
        let wf = wk.file() as u32;
        if wk.rank() <= 1 {
            let files = file_bb(wf)
                | (if wf > 0 { file_bb(wf - 1) } else { Bitboard::EMPTY })
                | (if wf < 7 { file_bb(wf + 1) } else { Bitboard::EMPTY });
            let zone = Bitboard((0xFFu64 << ((wk.rank() as u32 + 1) * 8)) | (0xFFu64 << ((wk.rank() as u32 + 2) * 8)));
            shield += SHIELD * (files & zone & wp).count() as i32;
        }
        let bf = bk.file() as u32;
        if bk.rank() >= 6 {
            let files = file_bb(bf)
                | (if bf > 0 { file_bb(bf - 1) } else { Bitboard::EMPTY })
                | (if bf < 7 { file_bb(bf + 1) } else { Bitboard::EMPTY });
            let zone = Bitboard((0xFFu64 << ((bk.rank() as u32 - 1) * 8)) | (0xFFu64 << ((bk.rank() as u32 - 2) * 8)));
            shield -= SHIELD * (files & zone & bp).count() as i32;
        }
    }
    score += shield * phase / PHASE_MAX;

    if board.pieces(Color::White, PieceType::Bishop).count() >= 2 {
        score += BISHOP_PAIR;
    }
    if board.pieces(Color::Black, PieceType::Bishop).count() >= 2 {
        score -= BISHOP_PAIR;
    }

    let stm = if board.side() == Color::Black { -score } else { score };
    stm + TEMPO
}

fn file_bb(file: u32) -> Bitboard {
    Bitboard(0x0101_0101_0101_0101u64 << file)
}
