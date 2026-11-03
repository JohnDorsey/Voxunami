//! The `f64` form of Kepler's equation (the first guess of the solver) against its definition at 256 bits.

use wide_float::{Precision, WideFloat};

use super::{
    bound_above_the_hyperbolic_root, equation_in_f64, hyperbolic_anomaly,
    refine_hyperbolic_anomaly, sinh_and_cosh, small_hyperbolic_pair,
};

fn wide(value: f64) -> WideFloat {
    WideFloat::from_f64(value, Precision::of(256)).unwrap()
}

/// E - e sin E and 1 - e cos E at 256 bits, from the definition.
fn worked_out(eccentric: f64, e: f64) -> (f64, f64) {
    let (sine, cosine) = wide(eccentric).sin_cos();
    let e = wide(e);
    let one = WideFloat::from_i64(1, Precision::of(256));
    (
        (&wide(eccentric) - &(&e * &sine)).to_f64(),
        (&one - &(&e * &cosine)).to_f64(),
    )
}

#[test]
fn the_equation_and_its_slope_are_the_definition_whether_the_anomaly_is_small_or_large() {
    // A ratio is 1 - e, kept apart from e so that a thin ellipse does not lose its digits: the pair (ratio, e) is exact here because both are given.
    for (ratio, e) in [
        (0.5, 0.5),
        (0.125, 0.875),
        (0.9375, 0.0625),
        (1.0, 0.0),
        (0.0625, 0.9375),
    ] {
        for eccentric in [
            1.0e-6,
            1.0e-3,
            0.01,
            0.25,
            0.4999,
            0.5,
            0.75,
            1.0,
            2.0,
            3.0,
            std::f64::consts::PI - 1.0e-5,
        ] {
            let (value, slope) = equation_in_f64(eccentric, ratio, e);
            let (expected_value, expected_slope) = worked_out(eccentric, e);
            assert!(
                (value - expected_value).abs() <= 1.0e-15 * expected_value.abs().max(1.0e-300),
                "E - e sin E at {eccentric} with e {e}: {value} against {expected_value}"
            );
            assert!(
                (slope - expected_slope).abs() <= 1.0e-15 * expected_slope,
                "1 - e cos E at {eccentric} with e {e}: {slope} against {expected_slope}"
            );
        }
    }
}

/// The solver of the hyperbola at 256 bits.
fn solved(size: f64, mean: f64) -> WideFloat {
    hyperbolic_anomaly(&wide(size), &wide(mean))
        .unwrap()
        .anomaly
}

#[test]
fn the_bound_above_the_root_is_above_the_root_of_every_hyperbola() {
    // The bound is what the solver is held under: a root above it could never be reached. From the thinnest hyperbola (where the root of the cubic `H^3 / 6 = M` is highest for a mean
    // anomaly) to the widest, and from a mean anomaly of a hundredth to 10^30.
    for size in [
        1.0 / 18446744073709551616.0,
        1.0e-12,
        1.0e-3,
        0.5,
        1.0,
        30.0,
        1.0e6,
        18446744073709551616.0,
    ] {
        for exponent in -7..=100 {
            let mean = libm::pow(2.0, f64::from(exponent) / 1.5);
            let root = solved(size, mean).to_f64();
            let bound = bound_above_the_hyperbolic_root(mean);
            assert!(
                root <= bound,
                "size {size:e}, mean anomaly {mean:e}: the root {root} is above the bound {bound}"
            );
        }
    }
}

#[test]
fn a_guess_a_long_way_from_the_root_is_brought_to_it_by_the_bound() {
    // Newton's method from the left of the root goes to the right of it by (M - g) / slope, which is 2^64 M on the thinnest hyperbola near its periapsis: without the bound that is
    // an anomaly of 10^21 and the next step takes a step of nothing. With it, a guess of a thousandth or of ten times the root gives the root.
    let precision = Precision::of(256);
    for (size, mean) in [
        (1.0 / 18446744073709551616.0, 100.0),
        (1.0 / 18446744073709551616.0, 0.3),
        (1.0e-6, 5.0e5),
        (1.0, 1.0e9),
        (2.0e3, 12.0),
    ] {
        let right = solved(size, mean);
        let bound =
            WideFloat::from_f64(bound_above_the_hyperbolic_root(mean) + 1.0, precision).unwrap();
        for guess in [1.0e-3, 0.5, 1.0, 3.0] {
            let found = refine_hyperbolic_anomaly(&wide(size), &wide(mean), wide(guess), &bound)
                .map(|found| found.anomaly)
                .unwrap_or_else(|error| {
                    panic!("size {size:e}, mean anomaly {mean}, guess {guess}: {error}")
                });
            let difference = (&found - &right).abs();
            let scale = right.floor_log2().unwrap();
            assert!(
                difference.is_zero() || difference.floor_log2().unwrap() - scale < -(256 - 66),
                "size {size:e}, mean anomaly {mean}, guess {guess}: {found:?} against {right:?}"
            );
        }
        // And from the right of the root too, up to the bound.
        let from_the_bound =
            refine_hyperbolic_anomaly(&wide(size), &wide(mean), bound.clone(), &bound)
                .unwrap()
                .anomaly;
        assert!(
            (&from_the_bound - &right).abs().is_zero()
                || (&from_the_bound - &right).abs().floor_log2().unwrap()
                    - right.floor_log2().unwrap()
                    < -(256 - 66)
        );
    }
}

#[test]
fn the_hyperbola_equation_holds_at_the_anomaly_found() {
    // e sinh H - H = M at 256 bits from the definition, for a thin, a middling and a wide hyperbola.
    for (size, mean) in [
        (1.0 / 18446744073709551616.0, 0.2),
        (0.25, 3.0),
        (1.0e4, 1.0e7),
    ] {
        let h = solved(size, mean);
        let e = &WideFloat::from_i64(1, Precision::of(256)) + &wide(size);
        let residual = &(&(&e * &h.sinh().unwrap()) - &h) - &wide(mean);
        assert!(
            residual.is_zero() || residual.abs().floor_log2().unwrap() < -(256 - 80),
            "{size:e} {mean}: {residual:?}"
        );
    }
}

/// `log2(|a - b| / |b|)`: very negative if they are equal.
fn relative_difference_bits(a: &WideFloat, b: &WideFloat) -> i64 {
    (a - b)
        .abs()
        .floor_log2()
        .map_or(i64::MIN / 2, |bits| bits - b.floor_log2().unwrap())
}

#[test]
fn the_series_of_sinh_and_cosh_are_the_functions() {
    // From a number the width cannot tell from nothing to one that makes a long series, of either sign: the same as the library's functions to a few bits of 256.
    for d in [
        1.0e-300, 9.0e-100, 1.0e-20, 1.0e-9, 0.01, 0.25, 0.4999, 0.5, 1.0, -1.0e-20, -0.3, -0.5,
    ] {
        let d = wide(d);
        let (sinh, cosh) = small_hyperbolic_pair(&d).unwrap();
        assert!(
            relative_difference_bits(&sinh, &d.sinh().unwrap()) <= -(256 - 3),
            "sinh {d:?}"
        );
        assert!(
            relative_difference_bits(&cosh, &d.cosh().unwrap()) <= -(256 - 3),
            "cosh {d:?}"
        );
    }
    let zero = WideFloat::zero(Precision::of(256));
    assert_eq!(
        small_hyperbolic_pair(&zero).unwrap(),
        (zero.clone(), WideFloat::from_i64(1, Precision::of(256)))
    );
}

#[test]
fn sinh_and_cosh_are_the_functions_from_nothing_to_far_and_across_the_half_where_the_way_changes() {
    for h in [1.0e-30, 0.01, 0.4999, 0.5, 0.5001, 1.0, 3.0, 17.0, 40.0] {
        let h = wide(h);
        let (sinh, cosh) = sinh_and_cosh(&h).unwrap();
        assert!(
            relative_difference_bits(&sinh, &h.sinh().unwrap()) <= -(256 - 4),
            "sinh {h:?}"
        );
        assert!(
            relative_difference_bits(&cosh, &h.cosh().unwrap()) <= -(256 - 4),
            "cosh {h:?}"
        );
    }
    // Over 8,192 bits the library's functions are asked for (the series is not the way there: see `SERIES_UP_TO_BITS`), and at 8,192 the series is.
    let h = |bits: u64| WideFloat::from_f64(0.3, Precision::new(bits).unwrap()).unwrap();
    let wider = h(9_000);
    assert_eq!(
        sinh_and_cosh(&wider).unwrap(),
        (wider.sinh().unwrap(), wider.cosh().unwrap())
    );
    let last = h(8_192);
    let (sinh, cosh) = sinh_and_cosh(&last).unwrap();
    assert!(relative_difference_bits(&sinh, &last.sinh().unwrap()) <= -(8_192 - 4));
    assert!(relative_difference_bits(&cosh, &last.cosh().unwrap()) <= -(8_192 - 4));
}

#[test]
fn the_pair_the_solver_keeps_is_the_pair_of_the_anomaly_it_found_from_any_guess() {
    // The pair is moved by each step and not worked out again, so it has to be checked against the one worked out from the answer: for a good guess (the way the program goes), and for
    // guesses that are not (steps that are held to the bound, steps that are large, and a step from a small anomaly to one a hundred orders smaller, where moving the pair would lose
    // what it has: the pair is worked out again there).
    let precision = Precision::of(256);
    let thinnest = 1.0 / 18446744073709551616.0;
    for (size, mean) in [
        (thinnest, 100.0),
        (thinnest, 0.3),
        (thinnest, 1.0e-30),
        (0.1, 6.0e6),
        (0.1, 0.2),
        (16.0, 1.0e9),
        (16.0, 0.002),
        (2.0e3, 12.0),
        (1.0, libm::pow(2.0, -190.0)),
    ] {
        let bound =
            WideFloat::from_f64(bound_above_the_hyperbolic_root(mean) + 1.0, precision).unwrap();
        let mut found = vec![hyperbolic_anomaly(&wide(size), &wide(mean)).unwrap()];
        for guess in [1.0e-3, 0.5, 3.0] {
            found.push(
                refine_hyperbolic_anomaly(&wide(size), &wide(mean), wide(guess), &bound).unwrap(),
            );
        }
        for found in found {
            let half = found.anomaly.times_power_of_two(-1).unwrap();
            let (sinh, cosh) = sinh_and_cosh(&half).unwrap();
            assert!(
                relative_difference_bits(&found.sinh_half, &sinh) <= -(256 - 6),
                "size {size:e}, mean {mean:e}: sinh {:?} against {sinh:?}",
                found.sinh_half
            );
            assert!(
                relative_difference_bits(&found.cosh_half, &cosh) <= -(256 - 6),
                "size {size:e}, mean {mean:e}: cosh"
            );
        }
    }
}
