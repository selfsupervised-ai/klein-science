// provenance: original
//! The probe's `#[cube]` kernels. Each one writes one output word per input element, guarded by
//! the element count, and is launched over a cube count spread to the device limits.
//!
//! The u32 `mul_hi` kernel follows the shape of CubeCL 0.11.0's own runtime test
//! (`cubecl-core-0.11.0/src/runtime_tests/binary.rs`, `test_mulhi_kernel`), with scalar words.
//!
//! Integer arithmetic in a kernel wraps, as on every GPU; each kernel is checked bit-exact
//! against its CPU reference before it is timed.

use cubecl::prelude::*;

/// The Montgomery modulus: the 31-bit prime 15 * 2^27 + 1.
pub const P: u32 = 2013265921;

/// `-P^-1 mod 2^32`, the Montgomery constant for R = 2^32.
pub const NEG_P_INV: u32 = neg_inv_mod_2_32(P);

/// `-m^-1 mod 2^32` for odd `m`, by Newton's iteration (each step doubles the correct low bits:
/// 1 bit from `m` itself, then 2, 4, 8, 16, 32).
pub const fn neg_inv_mod_2_32(m: u32) -> u32 {
    let mut inv = m;
    let mut i = 0;
    while i < 5 {
        inv = inv.wrapping_mul(2u32.wrapping_sub(m.wrapping_mul(inv)));
        i += 1;
    }
    inv.wrapping_neg()
}

/// `out[i] = mul_hi(lhs[i], rhs[i])`, the high word of the 64-bit product, as CubeCL lowers it.
/// On Metal that lowering widens to a 64-bit multiply and shifts (research Pitfall D).
#[cube(launch_unchecked)]
pub fn mulhi_u32(lhs: &[u32], rhs: &[u32], output: &mut [u32]) {
    if ABSOLUTE_POS < lhs.len() {
        output[ABSOLUTE_POS] = lhs[ABSOLUTE_POS].mul_hi(rhs[ABSOLUTE_POS]);
    }
}

/// The high word of `a * b` from 16-bit limbs, with 32-bit multiplies and adds only.
///
/// With `a = a1 * 2^16 + a0` and `b = b1 * 2^16 + b0`, each limb product fits in 32 bits, and
/// the middle column (the carry out of bit 31) is below 3 * 2^16, so nothing overflows.
#[cube]
pub fn mulhi_limb16(a: u32, b: u32) -> u32 {
    let a0 = a & 0xFFFFu32;
    let a1 = a >> 16u32;
    let b0 = b & 0xFFFFu32;
    let b1 = b >> 16u32;
    let p00 = a0 * b0;
    let p01 = a0 * b1;
    let p10 = a1 * b0;
    let p11 = a1 * b1;
    let mid = (p00 >> 16u32) + (p01 & 0xFFFFu32) + (p10 & 0xFFFFu32);
    p11 + (p01 >> 16u32) + (p10 >> 16u32) + (mid >> 16u32)
}

/// `out[i]` = the high word of `lhs[i] * rhs[i]` by [`mulhi_limb16`].
#[cube(launch_unchecked)]
pub fn mulhi_u32_limb16(lhs: &[u32], rhs: &[u32], output: &mut [u32]) {
    if ABSOLUTE_POS < lhs.len() {
        output[ABSOLUTE_POS] = mulhi_limb16(lhs[ABSOLUTE_POS], rhs[ABSOLUTE_POS]);
    }
}

/// `out[i] = lhs[i] * rhs[i]` in u64, wrapping.
#[cube(launch_unchecked)]
pub fn mul_u64_wrapping(lhs: &[u64], rhs: &[u64], output: &mut [u64]) {
    if ABSOLUTE_POS < lhs.len() {
        output[ABSOLUTE_POS] = lhs[ABSOLUTE_POS] * rhs[ABSOLUTE_POS];
    }
}

/// Montgomery product `a * b * 2^-32 mod P` for `a, b < P`, in u32 words.
///
/// `t = a * b` is `hi * 2^32 + lo`; `m = lo * NEG_P_INV` makes `t + m * P` divisible by 2^32.
/// Its low words cancel, so they carry exactly when `lo != 0`, and the quotient is
/// `hi + mul_hi(m, P) + carry`, below `2P < 2^32`. One conditional subtraction reduces it.
#[cube]
pub fn mont_mul(a: u32, b: u32) -> u32 {
    let lo = a * b;
    let hi = a.mul_hi(b);
    let m = lo * NEG_P_INV;
    let carry = select(lo != 0u32, 1u32, 0u32);
    let r = hi + m.mul_hi(P) + carry;
    select(r >= P, r - P, r)
}

/// `out[i] = mont_mul(lhs[i], rhs[i])`.
#[cube(launch_unchecked)]
pub fn mont_mul_u32(lhs: &[u32], rhs: &[u32], output: &mut [u32]) {
    if ABSOLUTE_POS < lhs.len() {
        output[ABSOLUTE_POS] = mont_mul(lhs[ABSOLUTE_POS], rhs[ABSOLUTE_POS]);
    }
}

/// The watchdog trial's kernel: `x = lhs[i]`, then `loops` times `x = mont_mul(x, rhs[i])`.
/// Every iteration feeds the next, so the compiler cannot drop any of them.
#[cube(launch_unchecked)]
pub fn mont_loop_u32(lhs: &[u32], rhs: &[u32], output: &mut [u32], loops: u32) {
    if ABSOLUTE_POS < lhs.len() {
        let b = rhs[ABSOLUTE_POS];
        let mut x = lhs[ABSOLUTE_POS];
        for _ in 0..loops {
            x = mont_mul(x, b);
        }
        output[ABSOLUTE_POS] = x;
    }
}

/// `out[i] = a[i] * b[i] + c[i]`, written as a separate multiply and add. Equal bits to the CPU's
/// fused `mul_add` on the lanes where fusing changes the result show FMA contraction.
#[cube(launch_unchecked)]
pub fn fma_probe_f32(a: &[f32], b: &[f32], c: &[f32], output: &mut [f32]) {
    if ABSOLUTE_POS < a.len() {
        output[ABSOLUTE_POS] = a[ABSOLUTE_POS] * b[ABSOLUTE_POS] + c[ABSOLUTE_POS];
    }
}

/// `left[i] = (a[i] + b[i]) + c[i]` and `cancel[i] = (a[i] + b[i]) - a[i]`, in that order. Bits
/// equal to `a + (b + c)` or to `b` where the written order gives other bits show reassociation.
#[cube(launch_unchecked)]
pub fn reassoc_probe_f32(a: &[f32], b: &[f32], c: &[f32], left: &mut [f32], cancel: &mut [f32]) {
    if ABSOLUTE_POS < a.len() {
        let x = a[ABSOLUTE_POS];
        let y = b[ABSOLUTE_POS];
        left[ABSOLUTE_POS] = (x + y) + c[ABSOLUTE_POS];
        cancel[ABSOLUTE_POS] = (x + y) - x;
    }
}
