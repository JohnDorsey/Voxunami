//! Making numbers, reading them back, and the rules about width.

mod common;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use common::rational;
use malachite_q::Rational;
use proptest::prelude::*;
use wide_float::test_support::{precision, wide_float};
use wide_float::{NumberError, Precision, WideFloat};

fn at(bits: u32) -> Precision {
    Precision::of(bits)
}

#[test]
fn a_precision_is_128_bits_to_2_to_the_24() {
    assert_eq!(
        Precision::MINIMUM.bits(),
        128,
        "the owner's floor for rails (2026-11-02)"
    );
    assert_eq!(Precision::MAXIMUM.bits(), 1 << 24);
    assert_eq!(Precision::new(128), Ok(Precision::MINIMUM));
    assert_eq!(Precision::new(1 << 24), Ok(Precision::MAXIMUM));
    for bits in [
        0,
        1,
        53,
        64,
        127,
        (1 << 24) + 1,
        u64::from(u32::MAX),
        u64::MAX,
    ] {
        assert!(
            Precision::new(bits).is_err(),
            "{bits} bits is not a precision"
        );
    }
    assert_eq!(Precision::MINIMUM.doubled(), at(256));
    assert_eq!(at(1000).widened_by(64), at(1064));
    // Past the most it stops at the most, so a loop that doubles ends.
    assert_eq!(Precision::MAXIMUM.doubled(), Precision::MAXIMUM);
    assert_eq!(at(1 << 23).doubled(), Precision::MAXIMUM);
    assert_eq!(Precision::MAXIMUM.widened_by(u32::MAX), Precision::MAXIMUM);
    assert_eq!(at(256).to_string(), "256 bits");
}

#[test]
#[should_panic(expected = "128 to 2^24")]
fn a_constant_precision_below_128_bits_is_refused() {
    let _ = Precision::of(127);
}

proptest! {
    #[test]
    fn every_integer_is_exact(value in any::<i128>(), width in precision()) {
        let number = WideFloat::from_i128(value, width);
        prop_assert_eq!(number.precision(), width);
        prop_assert_eq!(rational(&number), Rational::from(malachite_nz::integer::Integer::from(value)));
        prop_assert_eq!(number.floor_to_i128(), Some(value));
    }

    #[test]
    fn every_finite_double_is_exact_and_comes_back(bits in any::<u64>(), width in precision()) {
        let value = f64::from_bits(bits);
        prop_assume!(value.is_finite());
        let number = WideFloat::from_f64(value, width).unwrap();
        prop_assert_eq!(number.precision(), width);
        prop_assert_eq!(rational(&number), Rational::try_from(value).unwrap());
        // -0.0 comes back as 0.0: there is no negative zero.
        let expected = if value == 0.0 { 0.0 } else { value };
        prop_assert_eq!(number.to_f64().to_bits(), expected.to_bits());
    }

    #[test]
    fn the_text_of_a_number_reads_back_to_it(number in wide_float()) {
        let text = number.to_string();
        prop_assert_eq!(WideFloat::parse(&text, number.precision()).unwrap(), number.clone());
        prop_assert_eq!(format!("{number:?}"), format!("{text}@{}", number.precision().bits()));
    }

    #[test]
    fn the_parts_of_a_number_make_it_again(number in wide_float()) {
        match number.to_parts() {
            None => prop_assert!(number.is_zero()),
            Some(parts) => {
                prop_assert_eq!(parts.mantissa[0] & 1, 1, "the mantissa is odd");
                prop_assert_ne!(*parts.mantissa.last().unwrap(), 0, "and has no zero byte on top");
                let again = WideFloat::from_parts(parts.negative, &parts.mantissa, parts.exponent, number.precision()).unwrap();
                prop_assert_eq!(again, number);
            }
        }
    }

    #[test]
    fn the_floor_is_the_greatest_whole_number_not_above(number in wide_float()) {
        let exact = rational(&number);
        let floor = number.floor_to_i128();
        let expected = malachite_base::num::arithmetic::traits::Floor::floor(&exact);
        prop_assert_eq!(floor.map(malachite_nz::integer::Integer::from), i128::try_from(&expected).ok().map(malachite_nz::integer::Integer::from));
    }

    #[test]
    fn a_number_of_the_same_value_and_width_hashes_the_same(number in wide_float()) {
        let hash = |number: &WideFloat| {
            let mut hasher = DefaultHasher::new();
            number.hash(&mut hasher);
            hasher.finish()
        };
        let copy = number.rounded_to(number.precision());
        prop_assert_eq!(hash(&number), hash(&copy));
    }
}

#[test]
fn the_edges_of_the_integers() {
    let p = at(128);
    for (value, floor) in [
        (i128::MAX, Some(i128::MAX)),
        (i128::MIN, Some(i128::MIN)),
        (0, Some(0)),
        (-1, Some(-1)),
    ] {
        assert_eq!(WideFloat::from_i128(value, p).floor_to_i128(), floor);
    }
    // One past either end is not an i128: 2^127 and -2^127 - 1 (which a 128-bit number holds).
    let two_to_127 = WideFloat::parse("0x1p127", p).unwrap();
    assert_eq!(two_to_127.floor_to_i128(), None);
    assert_eq!(
        WideFloat::parse("-0x80000000000000000000000000000001p0", p)
            .unwrap()
            .floor_to_i128(),
        None
    );
    assert_eq!(
        WideFloat::parse("0x1p100000", p).unwrap().floor_to_i128(),
        None
    );
    // Below zero the floor goes down.
    assert_eq!(
        WideFloat::parse("-0x1p-3", p).unwrap().floor_to_i128(),
        Some(-1)
    );
    assert_eq!(
        WideFloat::parse("0x1p-3", p).unwrap().floor_to_i128(),
        Some(0)
    );
    assert_eq!(
        WideFloat::parse("-0x5p-1", p).unwrap().floor_to_i128(),
        Some(-3)
    );
}

#[test]
fn the_exponent_of_the_leading_bit() {
    let p = at(128);
    for (text, exponent) in [
        ("0x1p0", 0),
        ("0x3p0", 1),
        ("0x1p-1", -1),
        ("0x7p-3", -1),
        ("-0x1p100", 100),
        ("0x1fp-20", -16),
    ] {
        assert_eq!(
            WideFloat::parse(text, p).unwrap().floor_log2(),
            Some(exponent),
            "{text}"
        );
    }
    assert_eq!(WideFloat::zero(p).floor_log2(), None);
}

#[test]
fn the_text_form_is_what_is_documented() {
    let p = at(128);
    // An even mantissa is made odd, and zero is zero.
    for (given, written) in [
        ("0x4p0", "0x1p2"),
        ("0x3p-1", "0x3p-1"),
        ("-0x10p-4", "-0x1p0"),
        ("0xABp5", "0xabp5"),
        ("0x0p7", "0x0p0"),
        ("-0x0p0", "0x0p0"),
        ("0x000000000000000000000000000000001p0", "0x1p0"),
    ] {
        assert_eq!(
            WideFloat::parse(given, p).unwrap().to_string(),
            written,
            "{given}"
        );
    }
    assert_eq!(WideFloat::from_f64(1.5, p).unwrap().to_string(), "0x3p-1");
    assert_eq!(
        WideFloat::from_f64(-0.125, p).unwrap().to_string(),
        "-0x1p-3"
    );
    assert_eq!(WideFloat::from_i64(-1000, p).to_string(), "-0x7dp3");
    for bad in [
        "",
        "0x",
        "1",
        "0x1",
        "0x1p",
        "0xgp0",
        "0x1p0.5",
        "0x 1p0",
        "--0x1p0",
        "+0x1p0",
        "0X1p0",
        "0x1P0",
        "0x1p+",
        "p0",
        "0xp0",
        "0x1p99999999999999999999",
    ] {
        assert!(
            matches!(WideFloat::parse(bad, p), Err(NumberError::Malformed(_))),
            "{bad:?}"
        );
    }
    // Too many bits for the width: refused and not rounded.
    let too_wide = format!("0x{}p0", "f".repeat(33));
    assert_eq!(
        WideFloat::parse(&too_wide, p),
        Err(NumberError::DoesNotFit {
            needed: 132,
            allowed: 128
        })
    );
    // ... but zero bits at the bottom cost nothing.
    assert!(WideFloat::parse(&format!("0x{}p0", "f".repeat(31) + "00000"), p).is_ok());
}

#[test]
fn equal_values_of_different_widths_are_not_the_same_number() {
    let narrow = WideFloat::from_i64(7, at(128));
    let wide = WideFloat::from_i64(7, at(256));
    assert_ne!(narrow, wide, "a number is its value and its width");
    assert_eq!(narrow.compare(&wide), std::cmp::Ordering::Equal);
    assert_eq!(narrow.rounded_to(at(256)), wide);
    assert_eq!(&narrow + &wide, WideFloat::from_i64(14, at(256)));
    assert_eq!(-&narrow, WideFloat::from_i64(-7, at(128)));
}

#[test]
fn a_double_beyond_the_range_of_a_double_is_infinite_or_zero_and_nothing_else_is_wrong() {
    let p = at(128);
    assert_eq!(
        WideFloat::parse("0x1p2000", p).unwrap().to_f64(),
        f64::INFINITY
    );
    assert_eq!(
        WideFloat::parse("-0x1p2000", p).unwrap().to_f64(),
        f64::NEG_INFINITY
    );
    assert_eq!(WideFloat::parse("0x1p-2000", p).unwrap().to_f64(), 0.0);
    assert_eq!(
        WideFloat::parse("0x1p1023", p).unwrap().to_f64(),
        f64::from_bits(0x7FE0_0000_0000_0000)
    );
    // The least positive double, 2^-1074, and half of it (a tie, to even: zero).
    assert_eq!(
        WideFloat::parse("0x1p-1074", p).unwrap().to_f64(),
        f64::from_bits(1)
    );
    assert_eq!(WideFloat::parse("0x1p-1075", p).unwrap().to_f64(), 0.0);
    assert_eq!(
        WideFloat::parse("0x3p-1076", p).unwrap().to_f64(),
        f64::from_bits(1)
    );
}
