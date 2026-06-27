use crate::types::{Bitboard, Color, Square};
use std::sync::OnceLock;

struct Magic {
    mask: u64,
    magic: u64,
    shift: u32,
    attacks: Box<[u64]>,
}

impl Magic {
    #[inline]
    fn index(&self, occ: u64) -> usize {
        (((occ & self.mask).wrapping_mul(self.magic)) >> self.shift) as usize
    }
}

struct Tables {
    knight: [u64; 64],
    king: [u64; 64],
    pawn: [[u64; 64]; 2],
    rook: Vec<Magic>,
    bishop: Vec<Magic>,
}

static TABLES: OnceLock<Tables> = OnceLock::new();

#[inline]
fn tables() -> &'static Tables {
    TABLES.get_or_init(Tables::generate)
}

struct Prng(u64);

impl Prng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn sparse(&mut self) -> u64 {
        self.next() & self.next() & self.next()
    }
}

fn slider_scan(sq: usize, occ: u64, directions: &[(i32, i32)]) -> u64 {
    let mut att = 0u64;
    let (r, f) = ((sq / 8) as i32, (sq % 8) as i32);
    for &(dr, df) in directions {
        let (mut rr, mut ff) = (r + dr, f + df);
        while (0..8).contains(&rr) && (0..8).contains(&ff) {
            let s = (rr * 8 + ff) as u64;
            att |= 1u64 << s;
            if occ & (1u64 << s) != 0 {
                break;
            }
            rr += dr;
            ff += df;
        }
    }
    att
}

const ROOK_DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
const BISHOP_DIRS: [(i32, i32); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];

fn relevant_mask(sq: usize, rook: bool) -> u64 {
    let dirs = if rook { &ROOK_DIRS } else { &BISHOP_DIRS };
    let full = slider_scan(sq, 0, dirs);
    let (r, f) = (sq / 8, sq % 8);
    let mut edges = 0u64;
    if r != 0 {
        edges |= 0x0000_0000_0000_00ff;
    }
    if r != 7 {
        edges |= 0xff00_0000_0000_0000;
    }
    if f != 0 {
        edges |= 0x0101_0101_0101_0101;
    }
    if f != 7 {
        edges |= 0x8080_8080_8080_8080;
    }
    full & !edges
}

fn build_magic(prng: &mut Prng, sq: usize, rook: bool) -> Magic {
    let dirs = if rook { &ROOK_DIRS } else { &BISHOP_DIRS };
    let mask = relevant_mask(sq, rook);
    let bits = mask.count_ones();
    let size = 1usize << bits;
    let shift = 64 - bits;

    let mut occupancies = Vec::with_capacity(size);
    let mut references = Vec::with_capacity(size);
    let mut occ = 0u64;
    loop {
        occupancies.push(occ);
        references.push(slider_scan(sq, occ, dirs));
        occ = occ.wrapping_sub(mask) & mask;
        if occ == 0 {
            break;
        }
    }

    let mut table = vec![0u64; size];
    let mut used = vec![0u32; size];
    let mut stamp = 0u32;
    loop {
        let magic = prng.sparse();
        if (mask.wrapping_mul(magic) >> 56).count_ones() < 6 {
            continue;
        }
        stamp += 1;
        let mut ok = true;
        for i in 0..occupancies.len() {
            let idx = ((occupancies[i].wrapping_mul(magic)) >> shift) as usize;
            if used[idx] != stamp {
                used[idx] = stamp;
                table[idx] = references[i];
            } else if table[idx] != references[i] {
                ok = false;
                break;
            }
        }
        if ok {
            return Magic {
                mask,
                magic,
                shift,
                attacks: table.into_boxed_slice(),
            };
        }
    }
}

impl Tables {
    fn generate() -> Tables {
        let mut knight = [0u64; 64];
        let mut king = [0u64; 64];
        let mut pawn = [[0u64; 64]; 2];

        const NOT_A: u64 = !0x0101_0101_0101_0101;
        const NOT_H: u64 = !0x8080_8080_8080_8080;
        const NOT_AB: u64 = !0x0303_0303_0303_0303;
        const NOT_GH: u64 = !0xc0c0_c0c0_c0c0_c0c0;

        for sq in 0..64 {
            let b = 1u64 << sq;
            let mut k = 0u64;
            k |= (b << 17) & NOT_A;
            k |= (b << 15) & NOT_H;
            k |= (b << 10) & NOT_AB;
            k |= (b << 6) & NOT_GH;
            k |= (b >> 17) & NOT_H;
            k |= (b >> 15) & NOT_A;
            k |= (b >> 10) & NOT_GH;
            k |= (b >> 6) & NOT_AB;
            knight[sq] = k;

            let mut kg = b;
            kg |= (b & NOT_A) >> 1;
            kg |= (b & NOT_H) << 1;
            let row = kg;
            kg |= row << 8;
            kg |= row >> 8;
            king[sq] = kg & !b;

            pawn[0][sq] = ((b & NOT_A) << 7) | ((b & NOT_H) << 9);
            pawn[1][sq] = ((b & NOT_H) >> 7) | ((b & NOT_A) >> 9);
        }

        let mut prng = Prng(0x9e37_79b9_7f4a_7c15);
        let mut rook = Vec::with_capacity(64);
        let mut bishop = Vec::with_capacity(64);
        for sq in 0..64 {
            rook.push(build_magic(&mut prng, sq, true));
            bishop.push(build_magic(&mut prng, sq, false));
        }

        Tables {
            knight,
            king,
            pawn,
            rook,
            bishop,
        }
    }
}

#[inline]
pub fn knight_attacks(sq: Square) -> Bitboard {
    Bitboard(tables().knight[sq.index()])
}

#[inline]
pub fn king_attacks(sq: Square) -> Bitboard {
    Bitboard(tables().king[sq.index()])
}

#[inline]
pub fn pawn_attacks(color: Color, sq: Square) -> Bitboard {
    Bitboard(tables().pawn[color.index()][sq.index()])
}

#[inline]
pub fn rook_attacks(sq: Square, occ: Bitboard) -> Bitboard {
    let m = &tables().rook[sq.index()];
    Bitboard(m.attacks[m.index(occ.0)])
}

#[inline]
pub fn bishop_attacks(sq: Square, occ: Bitboard) -> Bitboard {
    let m = &tables().bishop[sq.index()];
    Bitboard(m.attacks[m.index(occ.0)])
}

#[inline]
pub fn queen_attacks(sq: Square, occ: Bitboard) -> Bitboard {
    rook_attacks(sq, occ) | bishop_attacks(sq, occ)
}
