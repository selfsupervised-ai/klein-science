// provenance: original
//! The probe's `#[cube]` kernels. Each one writes one output word per input element, guarded by
//! the element count, and is launched over a cube count spread to the device limits.
//!
//! The u32 `mul_hi` kernel follows the shape of CubeCL 0.11.0's own runtime test
//! (`cubecl-core-0.11.0/src/runtime_tests/binary.rs`, `test_mulhi_kernel`), with scalar words.

use cubecl::prelude::*;

/// `out[i] = mul_hi(lhs[i], rhs[i])`, the high word of the 64-bit product, as CubeCL lowers it.
/// On Metal that lowering widens to a 64-bit multiply and shifts (research Pitfall D).
#[cube(launch_unchecked)]
pub fn mulhi_u32(lhs: &[u32], rhs: &[u32], output: &mut [u32]) {
    if ABSOLUTE_POS < lhs.len() {
        output[ABSOLUTE_POS] = lhs[ABSOLUTE_POS].mul_hi(rhs[ABSOLUTE_POS]);
    }
}
