//! Property-test strategies for numbers. Enabled by the `test-support` feature.

use proptest::prelude::*;

use crate::{Precision, WideFloat};

/// A precision, mostly small ones (where a bug in rounding shows) and sometimes a wide one.
pub fn precision() -> impl Strategy<Value = Precision> {
    prop_oneof![
        8 => (128u32..=300).prop_map(Precision::of),
        2 => (301u32..=1100).prop_map(Precision::of),
        1 => Just(Precision::of(4096)),
    ]
}

/// A number of exactly this precision (every bit random, so the leading bit is set), with an
/// exponent small enough that sums, products and the functions stay in range, zero now and then.
pub fn wide_float_of(precision: Precision) -> impl Strategy<Value = WideFloat> {
    let bytes = (precision.bits() as usize).div_ceil(8);
    prop_oneof![
        1 => Just(WideFloat::zero(precision)),
        9 => (any::<bool>(), proptest::collection::vec(any::<u8>(), bytes), -300i64..300).prop_map(
            move |(negative, mut mantissa, exponent)| {
                // The top `bytes * 8 - precision` bits of the top byte are dropped, and the top
                // bit that is left is set, so the mantissa has the full width.
                let spare = bytes * 8 - precision.bits() as usize;
                let top = mantissa.last_mut().expect("a precision has bytes");
                *top = (*top & (0xFF >> spare)) | (0x80 >> spare);
                WideFloat::from_parts(negative, &mantissa, exponent, precision)
                    .expect("the mantissa has the bits of the precision")
            },
        ),
    ]
}

/// A number of any precision.
pub fn wide_float() -> impl Strategy<Value = WideFloat> {
    precision().prop_flat_map(wide_float_of)
}

/// A number above zero, of this precision.
pub fn positive_wide_float_of(precision: Precision) -> impl Strategy<Value = WideFloat> {
    wide_float_of(precision)
        .prop_map(|number| number.abs())
        .prop_filter("above zero", |number| !number.is_zero())
}
