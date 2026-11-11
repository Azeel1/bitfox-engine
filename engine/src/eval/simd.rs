// SIMD kernels for the NNUE accumulator and the output layer.
//
// Every kernel gives exactly the same result as the scalar loops, i16 wrapping
// included, so the search doesn't depend on which one runs. x86_64 picks
// AVX-512BW, AVX2 or SSE2 once at startup, aarch64 uses NEON, anything else
// gets the scalar loops.

use super::contract::{HL, QA};
#[cfg(all(
    target_arch = "x86_64",
    not(all(target_feature = "avx512f", target_feature = "avx512bw"))
))]
use std::sync::atomic::{AtomicU8, Ordering};

// widest step is the AVX-512 output loop, 64 lanes
const _: () = assert!(HL.is_multiple_of(64));

#[cfg(all(
    target_arch = "x86_64",
    not(all(target_feature = "avx512f", target_feature = "avx512bw"))
))]
#[inline(always)]
fn x86_level() -> u8 {
    // 0 until the first call, then 1 = SSE2, 2 = AVX2, 3 = AVX-512BW
    static LEVEL: AtomicU8 = AtomicU8::new(0);
    let cached = LEVEL.load(Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }
    init_x86_level(&LEVEL)
}

#[cfg(all(
    target_arch = "x86_64",
    not(all(target_feature = "avx512f", target_feature = "avx512bw"))
))]
#[cold]
fn init_x86_level(cell: &AtomicU8) -> u8 {
    let level = if std::arch::is_x86_feature_detected!("avx512f")
        && std::arch::is_x86_feature_detected!("avx512bw")
    {
        3
    } else if cfg!(target_feature = "avx2") || std::arch::is_x86_feature_detected!("avx2") {
        2
    } else {
        1
    };
    cell.store(level, Ordering::Relaxed);
    level
}

// SCReLU for both perspectives in one pass. The x86 kernels multiply v * w in
// 16 bits; build.rs rejects any net where |w| * QA wouldn't fit.
#[inline(always)]
pub fn activate_both(
    stm: &[i16; HL],
    w_stm: &[i16; HL],
    ntm: &[i16; HL],
    w_ntm: &[i16; HL],
) -> i32 {
    #[cfg(all(
        target_arch = "x86_64",
        target_feature = "avx512f",
        target_feature = "avx512bw"
    ))]
    {
        unsafe { avx512::activate_both(stm, w_stm, ntm, w_ntm) }
    }
    #[cfg(all(
        target_arch = "x86_64",
        not(all(target_feature = "avx512f", target_feature = "avx512bw"))
    ))]
    {
        match x86_level() {
            3 => unsafe { avx512::activate_both(stm, w_stm, ntm, w_ntm) },
            2 => unsafe { avx2::activate_both(stm, w_stm, ntm, w_ntm) },
            _ => unsafe { sse2::activate_both(stm, w_stm, ntm, w_ntm) },
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        // LLVM vectorises this fine on aarch64, a hand-written NEON version was slower
        scalar::activate(stm, w_stm).wrapping_add(scalar::activate(ntm, w_ntm))
    }
}

#[inline]
pub fn add_column(acc: &mut [i16; HL], col: &[i16; HL]) {
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    {
        neon::add_column(acc, col)
    }
    #[cfg(all(
        target_arch = "x86_64",
        target_feature = "avx512f",
        target_feature = "avx512bw"
    ))]
    {
        unsafe { avx512::add_column(acc, col) }
    }
    #[cfg(all(
        target_arch = "x86_64",
        not(all(target_feature = "avx512f", target_feature = "avx512bw"))
    ))]
    {
        match x86_level() {
            3 => unsafe { avx512::add_column(acc, col) },
            2 => unsafe { avx2::add_column(acc, col) },
            _ => unsafe { sse2::add_column(acc, col) },
        }
    }
    #[cfg(not(any(
        all(target_arch = "aarch64", target_feature = "neon"),
        target_arch = "x86_64"
    )))]
    {
        scalar::add_column(acc, col)
    }
}

#[inline]
pub fn sub_column(acc: &mut [i16; HL], col: &[i16; HL]) {
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    {
        neon::sub_column(acc, col)
    }
    #[cfg(all(
        target_arch = "x86_64",
        target_feature = "avx512f",
        target_feature = "avx512bw"
    ))]
    {
        unsafe { avx512::sub_column(acc, col) }
    }
    #[cfg(all(
        target_arch = "x86_64",
        not(all(target_feature = "avx512f", target_feature = "avx512bw"))
    ))]
    {
        match x86_level() {
            3 => unsafe { avx512::sub_column(acc, col) },
            2 => unsafe { avx2::sub_column(acc, col) },
            _ => unsafe { sse2::sub_column(acc, col) },
        }
    }
    #[cfg(not(any(
        all(target_arch = "aarch64", target_feature = "neon"),
        target_arch = "x86_64"
    )))]
    {
        scalar::sub_column(acc, col)
    }
}

// dst = src + adds - subs in one pass
#[inline]
pub fn apply<const NA: usize, const NS: usize>(
    src: &[i16; HL],
    dst: &mut [i16; HL],
    adds: [&[i16; HL]; 2],
    subs: [&[i16; HL]; 2],
) {
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    {
        neon::apply::<NA, NS>(src, dst, adds, subs)
    }
    #[cfg(all(
        target_arch = "x86_64",
        target_feature = "avx512f",
        target_feature = "avx512bw"
    ))]
    {
        unsafe { avx512::apply::<NA, NS>(src, dst, adds, subs) }
    }
    #[cfg(all(
        target_arch = "x86_64",
        not(all(target_feature = "avx512f", target_feature = "avx512bw"))
    ))]
    {
        match x86_level() {
            3 => unsafe { avx512::apply::<NA, NS>(src, dst, adds, subs) },
            2 => unsafe { avx2::apply::<NA, NS>(src, dst, adds, subs) },
            _ => unsafe { sse2::apply::<NA, NS>(src, dst, adds, subs) },
        }
    }
    #[cfg(not(any(
        all(target_arch = "aarch64", target_feature = "neon"),
        target_arch = "x86_64"
    )))]
    {
        scalar::apply::<NA, NS>(src, dst, adds, subs)
    }
}

pub fn backend() -> &'static str {
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    {
        "neon"
    }
    #[cfg(all(
        target_arch = "x86_64",
        target_feature = "avx512f",
        target_feature = "avx512bw"
    ))]
    {
        "avx512bw"
    }
    #[cfg(all(
        target_arch = "x86_64",
        not(all(target_feature = "avx512f", target_feature = "avx512bw"))
    ))]
    {
        match x86_level() {
            3 => "avx512bw (runtime)",
            2 => {
                if cfg!(target_feature = "avx2") {
                    "avx2"
                } else {
                    "avx2 (runtime)"
                }
            }
            _ => "sse2",
        }
    }
    #[cfg(not(any(
        all(target_arch = "aarch64", target_feature = "neon"),
        target_arch = "x86_64"
    )))]
    {
        "scalar"
    }
}

// reference loops, every kernel is tested against these
#[allow(dead_code)]
mod scalar {
    use super::{HL, QA};

    pub fn activate(acc: &[i16; HL], w: &[i16; HL]) -> i32 {
        let mut sum = 0i32;
        for (&x, &q) in acc.iter().zip(w.iter()) {
            let v = x.clamp(0, QA as i16);
            sum = sum.wrapping_add(i32::from(v.wrapping_mul(q)) * i32::from(v));
        }
        sum
    }

    pub fn add_column(acc: &mut [i16; HL], col: &[i16; HL]) {
        for (a, &w) in acc.iter_mut().zip(col) {
            *a = a.wrapping_add(w);
        }
    }

    pub fn sub_column(acc: &mut [i16; HL], col: &[i16; HL]) {
        for (a, &w) in acc.iter_mut().zip(col) {
            *a = a.wrapping_sub(w);
        }
    }

    pub fn apply<const NA: usize, const NS: usize>(
        src: &[i16; HL],
        dst: &mut [i16; HL],
        adds: [&[i16; HL]; 2],
        subs: [&[i16; HL]; 2],
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
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
mod neon {
    use super::HL;
    use std::arch::aarch64::*;

    #[inline]
    pub fn add_column(acc: &mut [i16; HL], col: &[i16; HL]) {
        unsafe {
            let mut i = 0;
            while i < HL {
                let a0 = vld1q_s16(acc.as_ptr().add(i));
                let a1 = vld1q_s16(acc.as_ptr().add(i + 8));
                let a2 = vld1q_s16(acc.as_ptr().add(i + 16));
                let a3 = vld1q_s16(acc.as_ptr().add(i + 24));
                let w0 = vld1q_s16(col.as_ptr().add(i));
                let w1 = vld1q_s16(col.as_ptr().add(i + 8));
                let w2 = vld1q_s16(col.as_ptr().add(i + 16));
                let w3 = vld1q_s16(col.as_ptr().add(i + 24));
                vst1q_s16(acc.as_mut_ptr().add(i), vaddq_s16(a0, w0));
                vst1q_s16(acc.as_mut_ptr().add(i + 8), vaddq_s16(a1, w1));
                vst1q_s16(acc.as_mut_ptr().add(i + 16), vaddq_s16(a2, w2));
                vst1q_s16(acc.as_mut_ptr().add(i + 24), vaddq_s16(a3, w3));
                i += 32;
            }
        }
    }

    #[inline]
    pub fn sub_column(acc: &mut [i16; HL], col: &[i16; HL]) {
        unsafe {
            let mut i = 0;
            while i < HL {
                let a0 = vld1q_s16(acc.as_ptr().add(i));
                let a1 = vld1q_s16(acc.as_ptr().add(i + 8));
                let a2 = vld1q_s16(acc.as_ptr().add(i + 16));
                let a3 = vld1q_s16(acc.as_ptr().add(i + 24));
                let w0 = vld1q_s16(col.as_ptr().add(i));
                let w1 = vld1q_s16(col.as_ptr().add(i + 8));
                let w2 = vld1q_s16(col.as_ptr().add(i + 16));
                let w3 = vld1q_s16(col.as_ptr().add(i + 24));
                vst1q_s16(acc.as_mut_ptr().add(i), vsubq_s16(a0, w0));
                vst1q_s16(acc.as_mut_ptr().add(i + 8), vsubq_s16(a1, w1));
                vst1q_s16(acc.as_mut_ptr().add(i + 16), vsubq_s16(a2, w2));
                vst1q_s16(acc.as_mut_ptr().add(i + 24), vsubq_s16(a3, w3));
                i += 32;
            }
        }
    }

    #[inline]
    pub fn apply<const NA: usize, const NS: usize>(
        src: &[i16; HL],
        dst: &mut [i16; HL],
        adds: [&[i16; HL]; 2],
        subs: [&[i16; HL]; 2],
    ) {
        unsafe {
            let mut i = 0;
            while i < HL {
                let mut v0 = vld1q_s16(src.as_ptr().add(i));
                let mut v1 = vld1q_s16(src.as_ptr().add(i + 8));
                let mut v2 = vld1q_s16(src.as_ptr().add(i + 16));
                let mut v3 = vld1q_s16(src.as_ptr().add(i + 24));
                v0 = vaddq_s16(v0, vld1q_s16(adds[0].as_ptr().add(i)));
                v1 = vaddq_s16(v1, vld1q_s16(adds[0].as_ptr().add(i + 8)));
                v2 = vaddq_s16(v2, vld1q_s16(adds[0].as_ptr().add(i + 16)));
                v3 = vaddq_s16(v3, vld1q_s16(adds[0].as_ptr().add(i + 24)));
                if NA == 2 {
                    v0 = vaddq_s16(v0, vld1q_s16(adds[1].as_ptr().add(i)));
                    v1 = vaddq_s16(v1, vld1q_s16(adds[1].as_ptr().add(i + 8)));
                    v2 = vaddq_s16(v2, vld1q_s16(adds[1].as_ptr().add(i + 16)));
                    v3 = vaddq_s16(v3, vld1q_s16(adds[1].as_ptr().add(i + 24)));
                }
                v0 = vsubq_s16(v0, vld1q_s16(subs[0].as_ptr().add(i)));
                v1 = vsubq_s16(v1, vld1q_s16(subs[0].as_ptr().add(i + 8)));
                v2 = vsubq_s16(v2, vld1q_s16(subs[0].as_ptr().add(i + 16)));
                v3 = vsubq_s16(v3, vld1q_s16(subs[0].as_ptr().add(i + 24)));
                if NS == 2 {
                    v0 = vsubq_s16(v0, vld1q_s16(subs[1].as_ptr().add(i)));
                    v1 = vsubq_s16(v1, vld1q_s16(subs[1].as_ptr().add(i + 8)));
                    v2 = vsubq_s16(v2, vld1q_s16(subs[1].as_ptr().add(i + 16)));
                    v3 = vsubq_s16(v3, vld1q_s16(subs[1].as_ptr().add(i + 24)));
                }
                vst1q_s16(dst.as_mut_ptr().add(i), v0);
                vst1q_s16(dst.as_mut_ptr().add(i + 8), v1);
                vst1q_s16(dst.as_mut_ptr().add(i + 16), v2);
                vst1q_s16(dst.as_mut_ptr().add(i + 24), v3);
                i += 32;
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
mod avx512 {
    use super::{HL, QA};
    use std::arch::x86_64::*;

    #[target_feature(enable = "avx512f,avx512bw")]
    pub unsafe fn activate_both(
        stm: &[i16; HL],
        w_stm: &[i16; HL],
        ntm: &[i16; HL],
        w_ntm: &[i16; HL],
    ) -> i32 {
        let zero = _mm512_setzero_si512();
        let qa = _mm512_set1_epi16(QA as i16);
        let sa = stm.as_ptr() as *const __m512i;
        let sq = w_stm.as_ptr() as *const __m512i;
        let na = ntm.as_ptr() as *const __m512i;
        let nq = w_ntm.as_ptr() as *const __m512i;
        let mut s0 = zero;
        let mut s1 = zero;
        let mut s2 = zero;
        let mut s3 = zero;
        let mut i = 0;
        while i < HL / 32 {
            let sv0 = _mm512_min_epi16(_mm512_max_epi16(_mm512_loadu_si512(sa.add(i)), zero), qa);
            let sv1 = _mm512_min_epi16(
                _mm512_max_epi16(_mm512_loadu_si512(sa.add(i + 1)), zero),
                qa,
            );
            let nv0 = _mm512_min_epi16(_mm512_max_epi16(_mm512_loadu_si512(na.add(i)), zero), qa);
            let nv1 = _mm512_min_epi16(
                _mm512_max_epi16(_mm512_loadu_si512(na.add(i + 1)), zero),
                qa,
            );

            let sp0 = _mm512_mullo_epi16(sv0, _mm512_loadu_si512(sq.add(i)));
            let sp1 = _mm512_mullo_epi16(sv1, _mm512_loadu_si512(sq.add(i + 1)));
            let np0 = _mm512_mullo_epi16(nv0, _mm512_loadu_si512(nq.add(i)));
            let np1 = _mm512_mullo_epi16(nv1, _mm512_loadu_si512(nq.add(i + 1)));

            s0 = _mm512_add_epi32(s0, _mm512_madd_epi16(sp0, sv0));
            s1 = _mm512_add_epi32(s1, _mm512_madd_epi16(sp1, sv1));
            s2 = _mm512_add_epi32(s2, _mm512_madd_epi16(np0, nv0));
            s3 = _mm512_add_epi32(s3, _mm512_madd_epi16(np1, nv1));
            i += 2;
        }
        _mm512_reduce_add_epi32(_mm512_add_epi32(
            _mm512_add_epi32(s0, s1),
            _mm512_add_epi32(s2, s3),
        ))
    }

    #[target_feature(enable = "avx512f,avx512bw")]
    pub unsafe fn add_column(acc: &mut [i16; HL], col: &[i16; HL]) {
        let a = acc.as_mut_ptr() as *mut __m512i;
        let c = col.as_ptr() as *const __m512i;
        for i in 0..HL / 32 {
            let v = _mm512_add_epi16(_mm512_loadu_si512(a.add(i)), _mm512_loadu_si512(c.add(i)));
            _mm512_storeu_si512(a.add(i), v);
        }
    }

    #[target_feature(enable = "avx512f,avx512bw")]
    pub unsafe fn sub_column(acc: &mut [i16; HL], col: &[i16; HL]) {
        let a = acc.as_mut_ptr() as *mut __m512i;
        let c = col.as_ptr() as *const __m512i;
        for i in 0..HL / 32 {
            let v = _mm512_sub_epi16(_mm512_loadu_si512(a.add(i)), _mm512_loadu_si512(c.add(i)));
            _mm512_storeu_si512(a.add(i), v);
        }
    }

    #[target_feature(enable = "avx512f,avx512bw")]
    pub unsafe fn apply<const NA: usize, const NS: usize>(
        src: &[i16; HL],
        dst: &mut [i16; HL],
        adds: [&[i16; HL]; 2],
        subs: [&[i16; HL]; 2],
    ) {
        let s = src.as_ptr() as *const __m512i;
        let d = dst.as_mut_ptr() as *mut __m512i;
        let a0 = adds[0].as_ptr() as *const __m512i;
        let a1 = adds[1].as_ptr() as *const __m512i;
        let s0 = subs[0].as_ptr() as *const __m512i;
        let s1 = subs[1].as_ptr() as *const __m512i;
        for i in 0..HL / 32 {
            let mut v =
                _mm512_add_epi16(_mm512_loadu_si512(s.add(i)), _mm512_loadu_si512(a0.add(i)));
            if NA == 2 {
                v = _mm512_add_epi16(v, _mm512_loadu_si512(a1.add(i)));
            }
            v = _mm512_sub_epi16(v, _mm512_loadu_si512(s0.add(i)));
            if NS == 2 {
                v = _mm512_sub_epi16(v, _mm512_loadu_si512(s1.add(i)));
            }
            _mm512_storeu_si512(d.add(i), v);
        }
    }
}

#[cfg(all(
    target_arch = "x86_64",
    not(all(target_feature = "avx512f", target_feature = "avx512bw"))
))]
mod avx2 {
    use super::{HL, QA};
    use std::arch::x86_64::*;

    #[inline]
    #[target_feature(enable = "avx2")]
    unsafe fn hsum(v: __m256i) -> i32 {
        let s = _mm_add_epi32(_mm256_castsi256_si128(v), _mm256_extracti128_si256::<1>(v));
        let s = _mm_add_epi32(s, _mm_shuffle_epi32::<0b01_00_11_10>(s));
        let s = _mm_add_epi32(s, _mm_shuffle_epi32::<0b10_11_00_01>(s));
        _mm_cvtsi128_si32(s)
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn activate_both(
        stm: &[i16; HL],
        w_stm: &[i16; HL],
        ntm: &[i16; HL],
        w_ntm: &[i16; HL],
    ) -> i32 {
        let zero = _mm256_setzero_si256();
        let qa = _mm256_set1_epi16(QA as i16);
        let sa = stm.as_ptr() as *const __m256i;
        let sq = w_stm.as_ptr() as *const __m256i;
        let na = ntm.as_ptr() as *const __m256i;
        let nq = w_ntm.as_ptr() as *const __m256i;
        let mut s0 = zero;
        let mut s1 = zero;
        let mut s2 = zero;
        let mut s3 = zero;
        let mut i = 0;
        while i < HL / 16 {
            let sv0 = _mm256_min_epi16(_mm256_max_epi16(_mm256_loadu_si256(sa.add(i)), zero), qa);
            let sv1 = _mm256_min_epi16(
                _mm256_max_epi16(_mm256_loadu_si256(sa.add(i + 1)), zero),
                qa,
            );
            let nv0 = _mm256_min_epi16(_mm256_max_epi16(_mm256_loadu_si256(na.add(i)), zero), qa);
            let nv1 = _mm256_min_epi16(
                _mm256_max_epi16(_mm256_loadu_si256(na.add(i + 1)), zero),
                qa,
            );

            let sp0 = _mm256_mullo_epi16(sv0, _mm256_loadu_si256(sq.add(i)));
            let sp1 = _mm256_mullo_epi16(sv1, _mm256_loadu_si256(sq.add(i + 1)));
            let np0 = _mm256_mullo_epi16(nv0, _mm256_loadu_si256(nq.add(i)));
            let np1 = _mm256_mullo_epi16(nv1, _mm256_loadu_si256(nq.add(i + 1)));

            s0 = _mm256_add_epi32(s0, _mm256_madd_epi16(sp0, sv0));
            s1 = _mm256_add_epi32(s1, _mm256_madd_epi16(sp1, sv1));
            s2 = _mm256_add_epi32(s2, _mm256_madd_epi16(np0, nv0));
            s3 = _mm256_add_epi32(s3, _mm256_madd_epi16(np1, nv1));
            i += 2;
        }
        hsum(_mm256_add_epi32(
            _mm256_add_epi32(s0, s1),
            _mm256_add_epi32(s2, s3),
        ))
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn add_column(acc: &mut [i16; HL], col: &[i16; HL]) {
        let a = acc.as_mut_ptr() as *mut __m256i;
        let c = col.as_ptr() as *const __m256i;
        for i in 0..HL / 16 {
            let v = _mm256_add_epi16(_mm256_loadu_si256(a.add(i)), _mm256_loadu_si256(c.add(i)));
            _mm256_storeu_si256(a.add(i), v);
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn sub_column(acc: &mut [i16; HL], col: &[i16; HL]) {
        let a = acc.as_mut_ptr() as *mut __m256i;
        let c = col.as_ptr() as *const __m256i;
        for i in 0..HL / 16 {
            let v = _mm256_sub_epi16(_mm256_loadu_si256(a.add(i)), _mm256_loadu_si256(c.add(i)));
            _mm256_storeu_si256(a.add(i), v);
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn apply<const NA: usize, const NS: usize>(
        src: &[i16; HL],
        dst: &mut [i16; HL],
        adds: [&[i16; HL]; 2],
        subs: [&[i16; HL]; 2],
    ) {
        let s = src.as_ptr() as *const __m256i;
        let d = dst.as_mut_ptr() as *mut __m256i;
        let a0 = adds[0].as_ptr() as *const __m256i;
        let a1 = adds[1].as_ptr() as *const __m256i;
        let s0 = subs[0].as_ptr() as *const __m256i;
        let s1 = subs[1].as_ptr() as *const __m256i;
        for i in 0..HL / 16 {
            let mut v =
                _mm256_add_epi16(_mm256_loadu_si256(s.add(i)), _mm256_loadu_si256(a0.add(i)));
            if NA == 2 {
                v = _mm256_add_epi16(v, _mm256_loadu_si256(a1.add(i)));
            }
            v = _mm256_sub_epi16(v, _mm256_loadu_si256(s0.add(i)));
            if NS == 2 {
                v = _mm256_sub_epi16(v, _mm256_loadu_si256(s1.add(i)));
            }
            _mm256_storeu_si256(d.add(i), v);
        }
    }
}

#[cfg(all(
    target_arch = "x86_64",
    not(all(target_feature = "avx512f", target_feature = "avx512bw"))
))]
mod sse2 {
    use super::{HL, QA};
    use std::arch::x86_64::*;

    // SSE2 is baseline on x86_64, nothing to detect
    pub unsafe fn activate_both(
        stm: &[i16; HL],
        w_stm: &[i16; HL],
        ntm: &[i16; HL],
        w_ntm: &[i16; HL],
    ) -> i32 {
        let zero = _mm_setzero_si128();
        let qa = _mm_set1_epi16(QA as i16);
        let sa = stm.as_ptr() as *const __m128i;
        let sq = w_stm.as_ptr() as *const __m128i;
        let na = ntm.as_ptr() as *const __m128i;
        let nq = w_ntm.as_ptr() as *const __m128i;
        let mut s0 = zero;
        let mut s1 = zero;
        let mut s2 = zero;
        let mut s3 = zero;
        let mut i = 0;
        while i < HL / 8 {
            let sv0 = _mm_min_epi16(_mm_max_epi16(_mm_loadu_si128(sa.add(i)), zero), qa);
            let sv1 = _mm_min_epi16(_mm_max_epi16(_mm_loadu_si128(sa.add(i + 1)), zero), qa);
            let nv0 = _mm_min_epi16(_mm_max_epi16(_mm_loadu_si128(na.add(i)), zero), qa);
            let nv1 = _mm_min_epi16(_mm_max_epi16(_mm_loadu_si128(na.add(i + 1)), zero), qa);
            let sp0 = _mm_mullo_epi16(sv0, _mm_loadu_si128(sq.add(i)));
            let sp1 = _mm_mullo_epi16(sv1, _mm_loadu_si128(sq.add(i + 1)));
            let np0 = _mm_mullo_epi16(nv0, _mm_loadu_si128(nq.add(i)));
            let np1 = _mm_mullo_epi16(nv1, _mm_loadu_si128(nq.add(i + 1)));
            s0 = _mm_add_epi32(s0, _mm_madd_epi16(sp0, sv0));
            s1 = _mm_add_epi32(s1, _mm_madd_epi16(sp1, sv1));
            s2 = _mm_add_epi32(s2, _mm_madd_epi16(np0, nv0));
            s3 = _mm_add_epi32(s3, _mm_madd_epi16(np1, nv1));
            i += 2;
        }
        let t = _mm_add_epi32(_mm_add_epi32(s0, s1), _mm_add_epi32(s2, s3));
        let t = _mm_add_epi32(t, _mm_shuffle_epi32::<0b01_00_11_10>(t));
        let t = _mm_add_epi32(t, _mm_shuffle_epi32::<0b10_11_00_01>(t));
        _mm_cvtsi128_si32(t)
    }

    pub unsafe fn add_column(acc: &mut [i16; HL], col: &[i16; HL]) {
        let a = acc.as_mut_ptr() as *mut __m128i;
        let c = col.as_ptr() as *const __m128i;
        for i in 0..HL / 8 {
            let v = _mm_add_epi16(_mm_loadu_si128(a.add(i)), _mm_loadu_si128(c.add(i)));
            _mm_storeu_si128(a.add(i), v);
        }
    }

    pub unsafe fn sub_column(acc: &mut [i16; HL], col: &[i16; HL]) {
        let a = acc.as_mut_ptr() as *mut __m128i;
        let c = col.as_ptr() as *const __m128i;
        for i in 0..HL / 8 {
            let v = _mm_sub_epi16(_mm_loadu_si128(a.add(i)), _mm_loadu_si128(c.add(i)));
            _mm_storeu_si128(a.add(i), v);
        }
    }

    pub unsafe fn apply<const NA: usize, const NS: usize>(
        src: &[i16; HL],
        dst: &mut [i16; HL],
        adds: [&[i16; HL]; 2],
        subs: [&[i16; HL]; 2],
    ) {
        let s = src.as_ptr() as *const __m128i;
        let d = dst.as_mut_ptr() as *mut __m128i;
        let a0 = adds[0].as_ptr() as *const __m128i;
        let a1 = adds[1].as_ptr() as *const __m128i;
        let s0 = subs[0].as_ptr() as *const __m128i;
        let s1 = subs[1].as_ptr() as *const __m128i;
        for i in 0..HL / 8 {
            let mut v = _mm_add_epi16(_mm_loadu_si128(s.add(i)), _mm_loadu_si128(a0.add(i)));
            if NA == 2 {
                v = _mm_add_epi16(v, _mm_loadu_si128(a1.add(i)));
            }
            v = _mm_sub_epi16(v, _mm_loadu_si128(s0.add(i)));
            if NS == 2 {
                v = _mm_sub_epi16(v, _mm_loadu_si128(s1.add(i)));
            }
            _mm_storeu_si128(d.add(i), v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lanes(seed: u32) -> [i16; HL] {
        let mut x = seed.wrapping_mul(0x9E37_79B9) | 1;
        std::array::from_fn(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as i16
        })
    }

    mod dispatched {
        pub use super::super::{activate_both, add_column, apply, sub_column};
    }

    // random accumulators, both legal-range and full-range output weights
    macro_rules! check_kernels {
        ($m:ident) => {
            #[allow(unused_unsafe)]
            for seed in 0..256 {
                let (a, b) = (lanes(seed), lanes(seed + 1000));
                for (wa, wb) in [
                    (
                        lanes(seed + 2000).map(|v| v % 129),
                        lanes(seed + 3000).map(|v| v % 129),
                    ),
                    (lanes(seed + 4000), lanes(seed + 5000)),
                ] {
                    assert_eq!(
                        unsafe { $m::activate_both(&a, &wa, &b, &wb) },
                        scalar::activate(&a, &wa).wrapping_add(scalar::activate(&b, &wb))
                    );
                }

                let cols = [6000, 7000, 8000, 9000].map(|k| lanes(seed + k));
                let (mut fast, mut slow) = (a, a);
                unsafe { $m::add_column(&mut fast, &cols[0]) };
                scalar::add_column(&mut slow, &cols[0]);
                assert_eq!(fast, slow);
                unsafe { $m::sub_column(&mut fast, &cols[1]) };
                scalar::sub_column(&mut slow, &cols[1]);
                assert_eq!(fast, slow);

                let adds = [&cols[0], &cols[1]];
                let subs = [&cols[2], &cols[3]];
                let (mut fast, mut slow) = ([0; HL], [0; HL]);
                unsafe { $m::apply::<1, 1>(&a, &mut fast, adds, subs) };
                scalar::apply::<1, 1>(&a, &mut slow, adds, subs);
                assert_eq!(fast, slow);
                unsafe { $m::apply::<1, 2>(&a, &mut fast, adds, subs) };
                scalar::apply::<1, 2>(&a, &mut slow, adds, subs);
                assert_eq!(fast, slow);
                unsafe { $m::apply::<2, 2>(&a, &mut fast, adds, subs) };
                scalar::apply::<2, 2>(&a, &mut slow, adds, subs);
                assert_eq!(fast, slow);
            }
        };
    }

    #[test]
    fn dispatched_kernels_match_scalar() {
        check_kernels!(dispatched);
    }

    #[cfg(all(
        target_arch = "x86_64",
        not(all(target_feature = "avx512f", target_feature = "avx512bw"))
    ))]
    #[test]
    fn sse2_kernels_match_scalar() {
        check_kernels!(sse2);
    }

    #[cfg(all(
        target_arch = "x86_64",
        not(all(target_feature = "avx512f", target_feature = "avx512bw"))
    ))]
    #[test]
    fn avx2_kernels_match_scalar() {
        if std::arch::is_x86_feature_detected!("avx2") {
            check_kernels!(avx2);
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx512_kernels_match_scalar() {
        if std::arch::is_x86_feature_detected!("avx512f")
            && std::arch::is_x86_feature_detected!("avx512bw")
        {
            check_kernels!(avx512);
        }
    }
}
