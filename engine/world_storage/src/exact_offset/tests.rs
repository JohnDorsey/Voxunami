use proptest::prelude::*;
use wide_float::{Precision, WideFloat};

use super::*;

const STEP: f64 = 1.0 / 4_294_967_296.0;

fn voxels(value: f64) -> WideFloat {
    WideFloat::from_f64(value, Precision::of(128)).unwrap()
}

fn rest_of(value: f64) -> (i128, f64) {
    bricks_and_rest(&voxels(value)).expect("a place of a few voxels has bricks")
}

#[test]
fn bricks_and_a_rest_are_the_place_rounded_to_a_step_of_2_to_the_minus_32() {
    // 50 voxels: a brick and 18.
    assert_eq!(rest_of(50.0), (1, 18.0));
    // Minus 50 voxels is two bricks back and 14 forward.
    assert_eq!(rest_of(-50.0), (-2, 14.0));
    assert_eq!(rest_of(0.25), (0, 0.25));
    assert_eq!(rest_of(0.0), (0, 0.0));
    // Less than half a step is nothing, and half a step is a step.
    assert_eq!(rest_of(STEP / 2.0 - STEP / 1024.0), (0, 0.0));
    assert_eq!(rest_of(STEP / 2.0), (0, STEP));
    // Minus half a step is nothing too: half a step is rounded up.
    assert_eq!(rest_of(-STEP / 2.0), (0, 0.0));
    assert_eq!(rest_of(-STEP / 2.0 - STEP / 1024.0), (-1, 32.0 - STEP));
}

#[test]
fn a_rest_that_rounds_up_to_a_whole_brick_is_the_next_brick() {
    // Just under a brick (a step and a half under it is a step under it, a quarter of a step under it is the brick).
    assert_eq!(rest_of(32.0 - STEP / 4.0), (1, 0.0));
    assert_eq!(rest_of(32.0 - STEP), (0, 32.0 - STEP));
    // A tiny number below nothing is just under the brick before nothing: it is nothing.
    assert_eq!(rest_of(-1.0e-300), (0, 0.0));
    let tiny = WideFloat::from_i64(-1, Precision::of(128))
        .times_power_of_two(-1000)
        .unwrap();
    assert_eq!(bricks_and_rest(&tiny), Some((0, 0.0)));
}

#[test]
fn the_largest_place_an_orbit_has_is_a_count_of_bricks_that_fits() {
    // A radius of 2^120 voxels, straight along: 2^115 bricks. Twice it, at the far end of an ellipse that is nearly a line, is 2^116.
    let far = WideFloat::from_i64(1, Precision::of(192))
        .times_power_of_two(121)
        .unwrap();
    assert_eq!(bricks_and_rest(&far), Some((1 << 116, 0.0)));
    assert_eq!(bricks_and_rest(&-&far), Some((-(1 << 116), 0.0)));
}

#[test]
fn a_place_with_no_i128_of_bricks_to_say_it_is_none_and_never_a_panic() {
    // A body on a hyperbola can be farther than the world is wide. An `i128` of bricks holds up to 2^127 of 32 voxels: 2^132 voxels.
    let power = |exponent: i64, sign: i64| {
        WideFloat::from_i64(sign, Precision::of(192))
            .times_power_of_two(exponent)
            .unwrap()
    };
    assert!(bricks_and_rest(&power(131, 1)).is_some());
    assert!(bricks_and_rest(&power(131, -1)).is_some());
    // (Minus 2^132 voxels is minus 2^127 bricks, which is `i128::MIN` and fits; plus 2^132 is one over `i128::MAX`.)
    assert!(bricks_and_rest(&power(132, -1)).is_some());
    for exponent in [132, 133, 200, 500] {
        assert_eq!(bricks_and_rest(&power(exponent, 1)), None, "2^{exponent}");
    }
    for exponent in [133, 200, 500] {
        assert_eq!(bricks_and_rest(&power(exponent, -1)), None, "-2^{exponent}");
    }
    let place = [power(10, 1), power(300, 1), power(10, 1)];
    assert!(ExactOffset::of_voxels(&place).is_none());
    assert!(ExactOffset::of_voxels(&[power(10, 1), power(11, -1), power(12, 1)]).is_some());
}

proptest! {
    /// A place that is a whole number of steps comes back as the bricks and the rest those steps make, whatever the width (the numbers are exact).
    #[test]
    fn a_place_of_whole_steps_is_taken_apart_exactly(
        steps in -(1i128 << 100)..(1i128 << 100),
        extra in 0u32..200,
    ) {
        let precision = Precision::of(128).widened_by(extra);
        let place = WideFloat::from_i128(steps, precision).times_power_of_two(-32).unwrap();
        let expected_bricks = steps.div_euclid(STEPS_IN_A_BRICK);
        let expected_rest = steps.rem_euclid(STEPS_IN_A_BRICK) as f64 * STEP;
        prop_assert_eq!(bricks_and_rest(&place), Some((expected_bricks, expected_rest)));
    }

    /// Anything between two steps is the nearer, half a step up, as an exact whole number of steps says.
    #[test]
    fn a_place_between_steps_is_the_nearer_step(
        steps in -(1i128 << 100)..(1i128 << 100),
        sixteenths in 0i128..16,
    ) {
        let precision = Precision::of(192);
        let place = WideFloat::from_i128(steps * 16 + sixteenths, precision).times_power_of_two(-36).unwrap();
        // (A sixteenth of a step: 8 and above rounds up.)
        let nearest = steps + i128::from(sixteenths >= 8);
        let expected = (nearest.div_euclid(STEPS_IN_A_BRICK), nearest.rem_euclid(STEPS_IN_A_BRICK) as f64 * STEP);
        prop_assert_eq!(bricks_and_rest(&place), Some(expected));
    }
}
