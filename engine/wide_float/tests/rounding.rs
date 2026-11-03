//! Every operation rounds once, correctly, to the width of the wider argument: checked against the
//! exact result as a rational number and a rounding written out in integers (`common`), with
//! random numbers of every width from 128 bits to 4,096 and with the nasty cases (a sum that
//! loses most of its leading bits, numbers of different widths).

mod common;

use common::{rational, round_to_bits};
use malachite_base::num::arithmetic::traits::Abs;
use malachite_q::Rational;
use proptest::prelude::*;
use wide_float::test_support::{precision, wide_float, wide_float_of};
use wide_float::{Precision, WideFloat};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn a_sum_a_difference_and_a_product_are_the_exact_result_rounded_once(a in wide_float(), b in wide_float()) {
        let width = a.precision().max(b.precision());
        let (x, y) = (rational(&a), rational(&b));
        for (name, result, exact) in [
            ("sum", &a + &b, &x + &y),
            ("difference", &a - &b, &x - &y),
            ("product", &a * &b, &x * &y),
        ] {
            prop_assert_eq!(result.precision(), width, "the {} is as wide as the wider argument", name);
            prop_assert_eq!(rational(&result), round_to_bits(&exact, width.bits()), "the {}", name);
        }
    }

    #[test]
    fn a_quotient_is_the_exact_result_rounded_once(a in wide_float(), b in wide_float()) {
        prop_assume!(!b.is_zero());
        let width = a.precision().max(b.precision());
        let result = a.divide(&b).unwrap();
        prop_assert_eq!(result.precision(), width);
        prop_assert_eq!(rational(&result), round_to_bits(&(rational(&a) / rational(&b)), width.bits()));
    }

    /// Numbers that nearly cancel: the difference has few bits, all of them from the last places
    /// of the arguments, which is where an operation that rounded too early would be wrong.
    #[test]
    fn a_difference_of_nearly_equal_numbers_is_exact_rounded_once(
        (a, nudge, shift) in precision().prop_flat_map(|p| (wide_float_of(p), 1i64..8, 0i64..8)),
    ) {
        prop_assume!(!a.is_zero());
        let width = a.precision();
        let small = &a.times_power_of_two(-(width.bits() as i64) + shift).unwrap() * &WideFloat::from_i64(nudge, width);
        let b = &a + &small;
        let difference = &a - &b;
        prop_assert_eq!(rational(&difference), round_to_bits(&(rational(&a) - rational(&b)), width.bits()));
        // No bits of the result are invented: it is made of the last bits of the two numbers.
        prop_assert!(difference.floor_log2().is_none_or(|exponent| exponent <= a.floor_log2().unwrap()));
    }

    #[test]
    fn rounding_to_a_width_is_the_nearest_number_of_that_width(a in wide_float(), width in precision()) {
        let rounded = a.rounded_to(width);
        prop_assert_eq!(rounded.precision(), width);
        prop_assert_eq!(rational(&rounded), round_to_bits(&rational(&a), width.bits()));
        if width >= a.precision() {
            prop_assert_eq!(rational(&rounded), rational(&a), "widening loses nothing");
            prop_assert_eq!(rounded.rounded_to(a.precision()), a, "and can be undone");
        }
    }

    #[test]
    fn scaling_by_a_power_of_two_is_exact(a in wide_float(), exponent in -5000i64..5000) {
        let scaled = a.times_power_of_two(exponent).unwrap();
        prop_assert_eq!(scaled.precision(), a.precision());
        prop_assert_eq!(rational(&scaled), rational(&a) << exponent);
    }

    #[test]
    fn the_root_of_an_exact_square_is_exact(
        mantissa in 1u64..(1 << 60), exponent in -200i64..200, negative in any::<bool>(), width in precision(),
    ) {
        let root = WideFloat::from_parts(negative, &mantissa.to_le_bytes(), exponent, width).unwrap();
        let square = &root * &root;
        prop_assert_eq!(rational(&square), rational(&root) * rational(&root), "the square is exact");
        prop_assert_eq!(square.sqrt().unwrap(), root.abs());
    }

    #[test]
    fn a_value_compares_by_value_and_equals_by_value_and_width(a in wide_float(), width in precision()) {
        let widened = a.rounded_to(a.precision().max(width));
        prop_assert_eq!(a.compare(&widened), std::cmp::Ordering::Equal);
        prop_assert_eq!(a == widened, a.precision() == widened.precision());
        let negated = -&a;
        prop_assert_eq!(a.compare(&negated), if a.is_zero() { std::cmp::Ordering::Equal } else if a.is_negative() { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater });
        prop_assert_eq!(rational(&negated), -rational(&a));
    }

    #[test]
    fn a_double_is_its_nearest_double_and_a_wide_number_rounds_to_the_nearest(a in wide_float()) {
        prop_assume!(a.floor_log2().is_none_or(|exponent| (-1000..1000).contains(&exponent)));
        let double = a.to_f64();
        prop_assert!(double.is_finite());
        let exact = rational(&a);
        let error = |candidate: f64| (&exact - Rational::try_from(candidate).unwrap()).abs();
        let (below, above) = (f64::from_bits(double.to_bits().wrapping_sub(1)), f64::from_bits(double.to_bits() + 1));
        prop_assume!(double != 0.0 && double.is_normal());
        prop_assert!(error(double) <= error(below) && error(double) <= error(above));
    }
}

#[test]
fn numbers_of_different_widths_meet_at_the_wider() {
    let narrow = Precision::of(128);
    let wide = Precision::of(1024);
    let third_narrow = WideFloat::from_i64(1, narrow)
        .divide(&WideFloat::from_i64(3, narrow))
        .unwrap();
    let third_wide = WideFloat::from_i64(1, wide)
        .divide(&WideFloat::from_i64(3, wide))
        .unwrap();
    let sum = &third_narrow + &third_wide;
    assert_eq!(sum.precision(), wide);
    // The narrow third has 128 bits and is no closer to a third than that, so the sum is the exact
    // sum of the two rounded to 1,024 bits, and not two thirds.
    assert_eq!(
        rational(&sum),
        round_to_bits(&(rational(&third_narrow) + rational(&third_wide)), 1024)
    );
    assert_ne!(sum, third_wide.times_power_of_two(1).unwrap());
}
