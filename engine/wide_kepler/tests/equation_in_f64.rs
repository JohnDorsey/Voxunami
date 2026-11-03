//! The `f64` form of Kepler's equation (the first guess of the solver, and what putting a state on an orbit uses) against its definition at 256 bits.

use wide_float::{Precision, WideFloat};
use wide_kepler::equation_in_f64;

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
