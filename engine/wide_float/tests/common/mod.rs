//! A second opinion for the tests: numbers as exact rationals (`malachite-q`, whose integer code
//! is not the code of the floating-point type under test), and the rounding of a rational to a
//! width written out here.

#![allow(dead_code)]

use malachite_base::num::arithmetic::traits::{Abs, Floor};
use malachite_base::num::conversion::traits::PowerOf2Digits;
use malachite_base::num::logic::traits::SignificantBits;
use malachite_nz::integer::Integer;
use malachite_nz::natural::Natural;
use malachite_q::Rational;
use wide_float::WideFloat;

/// The exact value of a number.
pub fn rational(number: &WideFloat) -> Rational {
    match number.to_parts() {
        None => Rational::from(0u32),
        Some(parts) => {
            let mantissa = Natural::from_power_of_2_digits_asc(8, parts.mantissa.iter().copied())
                .expect("bytes are digits");
            let value = Rational::from(mantissa) << parts.exponent;
            if parts.negative { -value } else { value }
        }
    }
}

/// `value` rounded to the nearest number of `bits` bits, a tie going to the even one: the rule,
/// written in integers.
pub fn round_to_bits(value: &Rational, bits: u64) -> Rational {
    if *value == 0u32 {
        return value.clone();
    }
    let negative = *value < 0u32;
    let magnitude = value.clone().abs();
    // The exponent `e` that puts the magnitude over 2^e in [2^(bits-1), 2^bits).
    let mut exponent = magnitude.numerator_ref().significant_bits() as i64
        - magnitude.denominator_ref().significant_bits() as i64
        - bits as i64;
    let scaled = |exponent: i64| magnitude.clone() >> exponent;
    let upper = Rational::from(Natural::from(1u32) << bits);
    let lower = Rational::from(Natural::from(1u32) << (bits - 1));
    while scaled(exponent) >= upper {
        exponent += 1;
    }
    while scaled(exponent) < lower {
        exponent -= 1;
    }
    let scaled = scaled(exponent);
    let whole: Integer = (&scaled).floor();
    let rest = &scaled - Rational::from(&whole);
    let half = Rational::from_unsigneds(1u32, 2u32);
    let rounded = if rest > half || (rest == half && whole.to_odd()) {
        whole + Integer::from(1)
    } else {
        whole
    };
    let result = Rational::from(rounded) << exponent;
    if negative { -result } else { result }
}

trait ToOdd {
    fn to_odd(&self) -> bool;
}

impl ToOdd for Integer {
    fn to_odd(&self) -> bool {
        use malachite_base::num::logic::traits::BitAccess;
        self.get_bit(0)
    }
}
