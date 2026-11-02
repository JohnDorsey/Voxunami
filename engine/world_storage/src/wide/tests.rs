use proptest::prelude::*;

use super::*;

fn wide(high: u128, low: u128) -> Wide {
    Wide { high, low }
}

proptest! {
    #[test]
    fn minus_undoes_plus(high in any::<u128>(), low in any::<u128>(), other_high in any::<u128>(), other_low in any::<u128>()) {
        let (first, second) = (wide(high, low), wide(other_high, other_low));
        prop_assert_eq!(first.plus(second).minus(second), first);
        prop_assert_eq!(first.minus(first), Wide::ZERO);
    }

    #[test]
    fn numbers_are_ordered_by_their_high_half_and_then_their_low(
        high in 0u128..4, low in any::<u128>(), other_high in 0u128..4, other_low in any::<u128>(),
    ) {
        let expected = (high, low) >= (other_high, other_low);
        prop_assert_eq!(wide(high, low).is_at_least(wide(other_high, other_low)), expected);
    }

    #[test]
    fn a_quotient_is_the_whole_number_of_times_the_divisor_goes(high in any::<u128>(), low in any::<u128>(), divisor in 1u128..(1 << 127)) {
        let number = wide(high, low);
        let quotient = number.divided_by(divisor);
        if high >= divisor {
            // (2^128 or more times: the largest there is.)
            prop_assert_eq!(quotient, u128::MAX);
        } else {
            // quotient x divisor <= number < (quotient + 1) x divisor, that is, the rest is not negative and is below the divisor.
            let product = Wide::of_product(quotient, divisor);
            prop_assert!(number.is_at_least(product));
            let rest = number.minus(product);
            prop_assert_eq!(rest.high, 0);
            prop_assert!(rest.low < divisor, "rest {} of {}", rest.low, divisor);
        }
    }

    #[test]
    fn a_quotient_of_2_to_the_128_or_more_is_the_largest_there_is_with_any_low_half(
        divisor in 1u128..(1 << 127), extra in 0u128..(1 << 100), low in any::<u128>(),
    ) {
        // A high half equal to the divisor is the edge: the quotient is 2^128 and a little, which does not fit. (Long division would lose bits of the remainder here.)
        let high = divisor.saturating_add(extra);
        prop_assert_eq!(wide(high, low).divided_by(divisor), u128::MAX);
        prop_assert_eq!(wide(divisor, low).divided_by(divisor), u128::MAX);
    }

    #[test]
    fn a_product_divided_by_one_of_its_factors_is_the_other(first in any::<u128>(), second in 1u128..(1 << 127)) {
        prop_assert_eq!(Wide::of_product(first, second).divided_by(second), first);
        // and with something less than a whole divisor over, the same
        prop_assert_eq!(Wide::of_product(first, second).plus(wide(0, second - 1)).divided_by(second), first);
    }

    #[test]
    fn the_root_of_a_square_is_the_number(root in 0u128..(1 << 127)) {
        prop_assert_eq!(Wide::of_product(root, root).square_root(), root);
    }

    #[test]
    fn the_root_of_a_number_between_two_squares_is_the_lower_root(root in 1u128..(1 << 127), more in any::<u128>()) {
        // From root^2 up to (root + 1)^2 - 1, which is root^2 + 2 root.
        let more = more % (2 * root).max(1).saturating_add(1);
        let number = Wide::of_product(root, root).plus(wide(0, more));
        prop_assert_eq!(number.square_root(), root);
        prop_assert_eq!(Wide::of_product(root, root).minus(wide(0, 1)).square_root(), root - 1);
    }

    #[test]
    fn the_root_of_any_number_is_the_largest_whole_number_that_squares_to_no_more(high in 0u128..(1 << 126), low in any::<u128>()) {
        let number = wide(high, low);
        let root = number.square_root();
        prop_assert!(number.is_at_least(Wide::of_product(root, root)));
        prop_assert!(!number.is_at_least(Wide::of_product(root + 1, root + 1)));
    }
}

#[test]
fn some_roots_and_quotients_that_can_be_read_off() {
    assert_eq!(Wide::ZERO.square_root(), 0);
    assert_eq!(Wide::ONE.square_root(), 1);
    assert_eq!(wide(0, 2).square_root(), 1);
    assert_eq!(wide(0, 4).square_root(), 2);
    // 2^252 is the square of 2^126, which is one with 126 bits after the point: the unit vectors of `exact_circle`.
    assert_eq!(Wide::ONE.shifted_left(252).square_root(), 1 << 126);
    // The largest number there can be to take a root of: 2^254 - 1 has the root 2^127 - 1.
    assert_eq!(
        Wide::ONE.shifted_left(254).minus(Wide::ONE).square_root(),
        (1 << 127) - 1
    );
    assert_eq!(wide(0, 1).divided_by(1), 1);
    assert_eq!(wide(0, 7).divided_by(2), 3);
    assert_eq!(wide(0, u128::MAX).divided_by(1), u128::MAX);
    assert_eq!(wide(1, 0).divided_by(2), 1 << 127);
    // The largest divisor, 2^127 - 1: 2^254 = (2^127 - 1)(2^127 + 1) + 1, so it goes in 2^127 + 1 times; and 2^128 times (as many as the high half is) does not fit.
    assert_eq!(wide(1 << 126, 0).divided_by((1 << 127) - 1), (1 << 127) + 1);
    assert_eq!(
        wide((1 << 127) - 1, 0).divided_by((1 << 127) - 1),
        u128::MAX
    );
}
