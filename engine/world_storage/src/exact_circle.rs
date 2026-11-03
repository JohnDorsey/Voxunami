//! Angles and the directions of the plane of an orbit in 128-bit fixed point, for making an orbit from a state ([`crate::fit_orbit`]) and for telling which end of an ellipse a
//! body passes ([`crate::rail::Orbit::apsides_passed`]). Where a body *is* on its orbit is worked out in wide numbers (`wide_kepler`), not here.
//!
//! The angle is a fraction of a turn in 128 bits and the two directions of the plane have 126 bits after the point. Everything is whole numbers (the `f64` is only a first
//! guess for a square root that is then corrected in whole numbers), so the same bits come out on every machine; and nothing is a table of an unknown source: the constants
//! below are made, and the values the tests check against are worked out at 120 digits, by `engine/world_storage/tools/exact_circle_reference.py` (it needs `mpmath`).

use crate::wide::Wide;

/// One, with 126 bits after the point.
pub(crate) const ONE: i128 = 1 << 126;

/// The sort of number a coordinate of a unit vector is held as: `value x 2^-126`.
pub(crate) type Fixed = i128;

/// One over two pi times 2^192, rounded down, as three 64-bit parts, the largest first.
pub(crate) const INVERSE_TAU: [u64; 3] =
    [0x28be60db9391054a, 0x7f09d5f47d4d3770, 0x36d8a5664f10e410];

/// `value` (an `f64` below `2^(127 - bits)` in size) as a fixed number with `bits` after the point, rounded toward nothing.
pub(crate) fn fixed_of(value: f64, bits: i32) -> Fixed {
    let (mantissa, exponent) = split(value);
    // value = mantissa x 2^exponent, and the fixed number is that x 2^bits.
    let shift = exponent + bits;
    let magnitude = if shift >= 0 {
        debug_assert!(shift < 75, "a number that does not fit");
        i128::from(mantissa) << shift
    } else if shift > -64 {
        i128::from(mantissa >> -shift)
    } else {
        0
    };
    if value < 0.0 { -magnitude } else { magnitude }
}

/// `value.abs()` as a whole number times a power of two, `(mantissa, exponent)`: exact for every `f64` that is a number.
pub(crate) fn split(value: f64) -> (u64, i32) {
    let bits = value.abs().to_bits();
    let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
    let fraction_bits = bits & ((1 << 52) - 1);
    if exponent_bits == 0 {
        (fraction_bits, -1074)
    } else {
        (fraction_bits | (1 << 52), exponent_bits - 1075)
    }
}

/// A fixed number as an `f64`, rounded once.
pub(crate) fn float_of(value: Fixed) -> f64 {
    value as f64 / ONE as f64
}

/// `first x second` with `bits` after the point in the answer, rounded to the nearest: the numbers have the points where they have them, and `bits` is the sum of
/// theirs less the answer's. The answer must fit, which it does for everything here: a unit vector's coordinates are at most one.
pub(crate) fn multiplied(first: Fixed, second: Fixed, bits: u32) -> Fixed {
    let negative = (first < 0) != (second < 0);
    let product = Wide::of_product(first.unsigned_abs(), second.unsigned_abs());
    let half = Wide::ONE.shifted_left(bits - 1);
    let rounded = product.plus(half).shifted_right(bits);
    debug_assert!(rounded.high == 0 && rounded.low >> 127 == 0);
    let magnitude = rounded.low as i128;
    if negative { -magnitude } else { magnitude }
}

/// `first x second`, 126 bits after the point.
fn product(first: Fixed, second: Fixed) -> Fixed {
    multiplied(first, second, 126)
}

/// `vector` made a unit vector. The size is found as one over a square root by Newton's method from the `f64` of it, in whole numbers: two steps take 53 bits to more
/// than the 124 held. (One over a root is as large as 2.3 for the vectors here, which does not fit with 126 bits after the point, so it has 124.)
fn normalised(vector: [Fixed; 3]) -> [Fixed; 3] {
    const ONE_124: i128 = 1 << 124;
    let square: Fixed = vector
        .iter()
        .map(|&component| product(component, component))
        .sum();
    let guess = 1.0 / libm::sqrt(float_of(square));
    let mut inverse = fixed_of(guess, 124);
    for _ in 0..2 {
        let inverse_squared = multiplied(inverse, inverse, 124);
        let error = ONE_124 - multiplied(square, inverse_squared, 126);
        inverse += multiplied(inverse, error, 125);
    }
    vector.map(|component| multiplied(component, inverse, 124))
}

/// The two directions in the plane of an orbit with this normal (a vector of at most one in each coordinate; it is made a unit vector), as unit vectors with 126 bits
/// after the point: the circle starts toward the first and goes toward the second. The first is the +x axis (the +z axis where the normal is close to +x) with its
/// part along the normal taken out; the second is the normal crossed with the first, so that the circle goes counter-clockwise looking down the normal.
pub(crate) fn plane(normal: [f64; 3]) -> ([Fixed; 3], [Fixed; 3]) {
    // (Which axis is taken is decided by the `f64` of the normal, as it always was, so that every saved phase still means the same place.)
    let reference = if normal[0].abs() < 0.9 { 0 } else { 2 };
    let normal = normalised(normal.map(|component| fixed_of(component, 126)));
    let along = normal[reference];
    let inside = [0, 1, 2].map(|axis| {
        let axis_part = if axis == reference { ONE } else { 0 };
        axis_part - product(normal[axis], along)
    });
    let first = normalised(inside);
    let second = [
        product(normal[1], first[2]) - product(normal[2], first[1]),
        product(normal[2], first[0]) - product(normal[0], first[2]),
        product(normal[0], first[1]) - product(normal[1], first[0]),
    ];
    (first, second)
}

/// `numerator / denominator` (below one) as a fraction of a turn with 128 bits after the point, rounded down. The denominator is below 2^127.
pub(crate) fn fraction_in_turns(numerator: u128, denominator: u128) -> u128 {
    debug_assert!(numerator < denominator && denominator < 1 << 127);
    let (mut remainder, mut turns) = (numerator, 0u128);
    for _ in 0..128 {
        remainder <<= 1;
        turns <<= 1;
        if remainder >= denominator {
            remainder -= denominator;
            turns |= 1;
        }
    }
    turns
}

/// An angle in radians (below 2^64 in size) as a fraction of a turn with 128 bits after the point, a negative angle coming round from the end: the radians times
/// one over two pi with 192 bits, so that it is right to the last of the 128.
pub(crate) fn turns_of_radians(radians: f64) -> u128 {
    let (mantissa, exponent) = split(radians);
    let inverse_high = u128::from(INVERSE_TAU[0]);
    let inverse_low = (u128::from(INVERSE_TAU[1]) << 64) | u128::from(INVERSE_TAU[2]);
    let times_mantissa = Wide::of_product(inverse_low, u128::from(mantissa)).plus(Wide {
        high: inverse_high * u128::from(mantissa),
        low: 0,
    });
    // radians = mantissa x 2^exponent, and turns x 2^128 = (mantissa x one over two pi x 2^192) x 2^exponent x 2^-64.
    let shift = 64 - exponent;
    let magnitude = if shift >= 0 {
        times_mantissa.shifted_right(shift as u32).low
    } else {
        times_mantissa.shifted_left((-shift) as u32).low
    };
    if radians < 0.0 {
        magnitude.wrapping_neg()
    } else {
        magnitude
    }
}

#[cfg(test)]
mod tests;
