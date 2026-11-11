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
    rook: Vec<Magic>,
    bishop: Vec<Magic>,
}

static TABLES: OnceLock<Tables> = OnceLock::new();

struct LeaperTables {
    knight: [u64; 64],
    king: [u64; 64],
    pawn: [[u64; 64]; 2],
}

impl LeaperTables {
    const fn generate() -> LeaperTables {
        const NOT_A: u64 = !0x0101_0101_0101_0101;
        const NOT_H: u64 = !0x8080_8080_8080_8080;
        const NOT_AB: u64 = !0x0303_0303_0303_0303;
        const NOT_GH: u64 = !0xc0c0_c0c0_c0c0_c0c0;

        let mut knight = [0u64; 64];
        let mut king = [0u64; 64];
        let mut pawn = [[0u64; 64]; 2];
        let mut sq = 0;
        while sq < 64 {
            let b = 1u64 << sq;
            knight[sq] = ((b << 17) & NOT_A)
                | ((b << 15) & NOT_H)
                | ((b << 10) & NOT_AB)
                | ((b << 6) & NOT_GH)
                | ((b >> 17) & NOT_H)
                | ((b >> 15) & NOT_A)
                | ((b >> 10) & NOT_GH)
                | ((b >> 6) & NOT_AB);

            let mut kg = b | ((b & NOT_A) >> 1) | ((b & NOT_H) << 1);
            let row = kg;
            kg |= row << 8;
            kg |= row >> 8;
            king[sq] = kg & !b;

            pawn[0][sq] = ((b & NOT_A) << 7) | ((b & NOT_H) << 9);
            pawn[1][sq] = ((b & NOT_H) >> 7) | ((b & NOT_A) >> 9);
            sq += 1;
        }

        LeaperTables { knight, king, pawn }
    }
}

static LEAPERS: LeaperTables = LeaperTables::generate();

#[inline]
fn tables() -> &'static Tables {
    TABLES.get_or_init(Tables::generate)
}

pub fn warmup() {
    let _ = tables();
}

#[cfg(test)]
struct Prng(u64);

#[cfg(test)]
impl Prng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
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

fn build_magic(sq: usize, rook: bool, magic: u64) -> Magic {
    let dirs = if rook { &ROOK_DIRS } else { &BISHOP_DIRS };
    let mask = relevant_mask(sq, rook);
    let bits = mask.count_ones();
    let size = 1usize << bits;
    let shift = 64 - bits;
    let mut table = vec![0u64; size];
    let mut occupied = vec![false; size];
    let mut occ = 0u64;
    loop {
        let index = (occ.wrapping_mul(magic) >> shift) as usize;
        let attacks = slider_scan(sq, occ, dirs);
        assert!(!occupied[index] || table[index] == attacks);
        table[index] = attacks;
        occupied[index] = true;
        occ = occ.wrapping_sub(mask) & mask;
        if occ == 0 {
            break;
        }
    }
    Magic {
        mask,
        magic,
        shift,
        attacks: table.into_boxed_slice(),
    }
}

impl Tables {
    fn generate() -> Tables {
        let mut rook = Vec::with_capacity(64);
        let mut bishop = Vec::with_capacity(64);
        for sq in 0..64 {
            rook.push(build_magic(sq, true, super::magic_numbers::ROOK[sq]));
            bishop.push(build_magic(sq, false, super::magic_numbers::BISHOP[sq]));
        }

        Tables { rook, bishop }
    }
}

#[inline]
pub fn knight_attacks(sq: Square) -> Bitboard {
    Bitboard(LEAPERS.knight[sq.index()])
}

#[inline]
pub fn king_attacks(sq: Square) -> Bitboard {
    Bitboard(LEAPERS.king[sq.index()])
}

#[inline]
pub fn pawn_attacks(color: Color, sq: Square) -> Bitboard {
    Bitboard(LEAPERS.pawn[color.index()][sq.index()])
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

#[inline(always)]
pub fn bishop_rook_attacks(sq: Square, occ: Bitboard) -> (Bitboard, Bitboard) {
    let tables = tables();
    let bishop = &tables.bishop[sq.index()];
    let rook = &tables.rook[sq.index()];
    (
        Bitboard(bishop.attacks[bishop.index(occ.0)]),
        Bitboard(rook.attacks[rook.index(occ.0)]),
    )
}

#[inline]
pub fn queen_attacks(sq: Square, occ: Bitboard) -> Bitboard {
    let (bishop, rook) = bishop_rook_attacks(sq, occ);
    bishop | rook
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_slider_occupancy_matches_ray_scans() {
        for rook in [false, true] {
            let directions = if rook { &ROOK_DIRS } else { &BISHOP_DIRS };
            for raw in 0..64 {
                let sq = Square::new(raw);
                let mask = relevant_mask(raw as usize, rook);
                let mut occ = 0;
                loop {
                    let actual = if rook {
                        rook_attacks(sq, Bitboard(occ))
                    } else {
                        bishop_attacks(sq, Bitboard(occ))
                    };
                    assert_eq!(actual.0, slider_scan(raw as usize, occ, directions));
                    occ = occ.wrapping_sub(mask) & mask;
                    if occ == 0 {
                        break;
                    }
                }
            }
        }
    }

    #[test]
    fn magic_attacks_match_ray_scans() {
        let mut rng = Prng(0x1234_5678_9abc_def0);
        for raw in 0..64 {
            let sq = Square::new(raw);
            for _ in 0..256 {
                let occ = Bitboard(rng.next());
                assert_eq!(
                    rook_attacks(sq, occ),
                    Bitboard(slider_scan(raw as usize, occ.0, &ROOK_DIRS))
                );
                assert_eq!(
                    bishop_attacks(sq, occ),
                    Bitboard(slider_scan(raw as usize, occ.0, &BISHOP_DIRS))
                );
            }
        }
    }

    #[test]
    fn combined_slider_lookup_matches_individual_lookups() {
        let mut rng = Prng(0xfedc_ba98_7654_3210);
        for raw in 0..64 {
            let sq = Square::new(raw);
            let occ = Bitboard(rng.next());
            let (bishop, rook) = bishop_rook_attacks(sq, occ);
            assert_eq!(bishop, bishop_attacks(sq, occ));
            assert_eq!(rook, rook_attacks(sq, occ));
            assert_eq!(bishop | rook, queen_attacks(sq, occ));
        }
    }

    #[test]
    fn warmup_is_idempotent() {
        warmup();
        let first = tables() as *const Tables;
        warmup();
        assert_eq!(first, tables() as *const Tables);
    }
}
