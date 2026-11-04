//! When a body leaves a sphere round its centre (E16 phase B of `docs/orbits-and-thrust-plan.md`): the first instant, on a grid of 2^-31 of a second, at which it is at least a
//! distance away. The instant is found in closed form (the distance is `a (1 - e cos E)` for an ellipse and `|a| (e cosh H - 1)` for a hyperbola, so the anomaly at the crossing is
//! an arc cosine and the time follows from Kepler's equation) and rounded **up** to the grid, so it does not depend on any tick rate. These tests do not look at the formulas: they ask
//! the place of the body (`Elements::place_at`, which the reference rows hold to its last bit) at the instant and at the ones before it, and they check the cases that have a
//! number by hand.

use proptest::prelude::*;
use wide_float::{Precision, WideFloat};
use wide_kepler::{Elements, Time};

/// Units of a second of the instants a body leaves a sphere at.
const GRID: u32 = 1 << 31;

fn at(bits: u64) -> Precision {
    Precision::new(bits).unwrap()
}

fn wide(value: f64, precision: Precision) -> WideFloat {
    WideFloat::from_f64(value, precision).unwrap()
}

fn orbit(
    radius: f64,
    ratio: f64,
    periapsis: f64,
    period: f64,
    phase: f64,
    normal: [f64; 3],
) -> Elements {
    let precision = at(128);
    Elements {
        normal: normal.map(|part| wide(part, precision)),
        radius: wide(radius, precision),
        ratio: wide(ratio, precision),
        periapsis: wide(periapsis, precision),
        period: wide(period, precision),
        phase: wide(phase, precision),
    }
}

fn tick(units: i128) -> Time {
    Time {
        units,
        units_per_second: 20,
    }
}

fn on_the_grid(units: i128) -> Time {
    Time {
        units,
        units_per_second: GRID,
    }
}

/// The square of the distance from the centre at a time, in 256 bits.
fn distance_squared(elements: &Elements, time: Time) -> WideFloat {
    let place = elements.place_at(time, at(256)).unwrap().position;
    &(&place[0].square() + &place[1].square()) + &place[2].square()
}

fn is_outside(elements: &Elements, distance: f64, time: Time) -> bool {
    let limit = wide(distance, at(256)).square();
    distance_squared(elements, time).compare(&limit).is_ge()
}

/// The grid instant at or after a time, in units of the grid.
fn grid_units_after(time: Time) -> i128 {
    let numerator = time.units * i128::from(GRID);
    let per_second = i128::from(time.units_per_second);
    numerator.div_euclid(per_second) + i128::from(numerator.rem_euclid(per_second) != 0)
}

fn seconds_of(time: Time) -> f64 {
    time.units as f64 / f64::from(time.units_per_second)
}

#[test]
fn an_ellipse_leaves_a_sphere_at_the_time_a_hand_worked_example_says() {
    // a = 1000, e = 1/2, so the body is 1,200 voxels out where cos E = (1 - 1.2) / 0.5 = -0.4: E = 1.98231, M = E - e sin E = 1.52405 radians, and a period of 1,000 seconds is
    // 0.0062832 radians a second: 242.5 seconds after the nearest point. (The first direction of the plane is x; the periapsis of 0 puts the nearest point there.)
    let elements = orbit(1000.0, 0.5, 0.0, 1000.0, 0.0, [0.0, 0.0, 1.0]);
    let eccentric = libm::acos(-0.4);
    let mean = eccentric - 0.5 * libm::sin(eccentric);
    let expected = mean * 1000.0 / std::f64::consts::TAU;
    let found = elements
        .leaves_at(&wide(1200.0, at(128)), tick(0), at(128))
        .unwrap()
        .expect("it goes out to 1,500");
    assert_eq!(found.units_per_second, GRID);
    assert!(
        (seconds_of(found) - expected).abs() < 1e-6,
        "{} against {expected}",
        seconds_of(found)
    );
}

#[test]
fn the_other_way_round_it_leaves_after_the_same_time() {
    // Going the other way, the body is at the nearest point at time 0 as well and is 1,200 out at the same time after it.
    let forward = orbit(1000.0, 0.5, 0.0, 1000.0, 0.0, [0.0, 0.0, 1.0]);
    let backward = orbit(1000.0, 0.5, 0.0, -1000.0, 0.0, [0.0, 0.0, 1.0]);
    let distance = wide(1200.0, at(128));
    assert_eq!(
        forward.leaves_at(&distance, tick(0), at(128)).unwrap(),
        backward.leaves_at(&distance, tick(0), at(128)).unwrap()
    );
}

#[test]
fn a_hyperbola_leaves_a_sphere_at_the_time_a_hand_worked_example_says() {
    // |a| = 1000, e = 2 (a ratio of -1), 5,000 voxels out where cosh H = (5 + 1) / 2 = 3: H = 1.76275, M = e sinh H - H = 3.89411 radians. A period of 2 pi seconds is a radian a second,
    // and a phase of -1 radian puts the periapsis at 1 s: 3.89411 + 1 = 4.89411 seconds.
    let elements = orbit(
        1000.0,
        -1.0,
        0.0,
        std::f64::consts::TAU,
        -1.0,
        [0.0, 0.0, 1.0],
    );
    let anomaly = libm::acosh(3.0);
    let mean = 2.0 * libm::sinh(anomaly) - anomaly;
    let found = elements
        .leaves_at(&wide(5000.0, at(128)), tick(0), at(128))
        .unwrap()
        .expect("a hyperbola goes out for ever");
    // (The period is the double nearest 2 pi, so the rate is a radian a second to 1e-16.)
    assert!(
        (seconds_of(found) - (mean + 1.0)).abs() < 1e-6,
        "{} against {}",
        seconds_of(found),
        mean + 1.0
    );
    // The other way round it is the same.
    let back = orbit(
        1000.0,
        -1.0,
        0.0,
        -std::f64::consts::TAU,
        1.0,
        [0.0, 0.0, 1.0],
    );
    assert_eq!(
        back.leaves_at(&wide(5000.0, at(128)), tick(0), at(128))
            .unwrap(),
        elements
            .leaves_at(&wide(5000.0, at(128)), tick(0), at(128))
            .unwrap()
    );
}

#[test]
fn an_ellipse_that_does_not_reach_the_distance_never_leaves() {
    let elements = orbit(1000.0, 0.5, 0.3, 1000.0, 0.7, [0.0, 0.0, 1.0]);
    // The far end is at 1,500: the distance there is touched and not left, and under it is left.
    for distance in [1500.0, 1500.5, 1.0e9] {
        assert_eq!(
            elements
                .leaves_at(&wide(distance, at(128)), tick(3), at(128))
                .unwrap(),
            None,
            "{distance}"
        );
    }
    assert!(
        elements
            .leaves_at(&wide(1499.0, at(128)), tick(3), at(128))
            .unwrap()
            .is_some()
    );
}

#[test]
fn a_body_that_is_already_out_leaves_at_the_first_instant_of_the_grid_at_or_after_the_time() {
    // A hyperbola 9,000 voxels out going away: at the time asked it is out already.
    let elements = orbit(
        1000.0,
        -1.0,
        0.0,
        std::f64::consts::TAU,
        20.0,
        [0.0, 0.0, 1.0],
    );
    let asked = tick(7);
    assert!(is_outside(&elements, 5000.0, asked));
    let found = elements
        .leaves_at(&wide(5000.0, at(128)), asked, at(128))
        .unwrap()
        .unwrap();
    assert_eq!(found, on_the_grid(grid_units_after(asked)));
}

#[test]
fn an_ellipse_body_in_the_stretch_that_is_out_is_out_at_the_time_asked_either_way_round() {
    // a = 1000 and e = 1/2: 1,200 voxels out from a mean anomaly of 1.524 to 4.759. A phase of 2 is in the stretch; going either way round, it is out.
    for period in [1000.0, -1000.0] {
        let elements = orbit(1000.0, 0.5, 0.0, period, 2.0, [0.0, 0.0, 1.0]);
        assert!(is_outside(&elements, 1200.0, tick(0)));
        assert_eq!(
            elements
                .leaves_at(&wide(1200.0, at(128)), tick(0), at(128))
                .unwrap(),
            Some(on_the_grid(0)),
            "period {period}"
        );
    }
}

#[test]
fn a_hyperbola_body_that_is_out_coming_in_is_out_too_and_does_not_wait_for_the_periapsis() {
    // 5,000 voxels is a mean anomaly of 3.894 either side of the periapsis. A phase of -20 at time 0 is 16 radians before it: out, and coming in; one of +20, going away.
    for (phase, period) in [(-20.0, 6.0), (20.0, 6.0), (20.0, -6.0), (-20.0, -6.0)] {
        let elements = orbit(1000.0, -1.0, 0.0, period, phase, [0.0, 0.0, 1.0]);
        assert!(is_outside(&elements, 5000.0, tick(0)), "{phase} {period}");
        assert_eq!(
            elements
                .leaves_at(&wide(5000.0, at(128)), tick(0), at(128))
                .unwrap(),
            Some(on_the_grid(0)),
            "{phase} {period}"
        );
    }
}

#[test]
fn a_stretch_too_short_for_the_grid_is_never_an_instant_where_the_body_is_in() {
    // 1,500 is the far end of the ellipse. A distance a hair under it is exceeded for a stretch of time (about 5e-11 s here) that is shorter than a step of the grid (4.7e-10 s): by the
    // phase, an instant of the grid is in it or is not, and whatever is said, an instant given is one at which the body is out.
    let distance = &wide(1500.0, at(192)) - &wide(3.0e-24, at(192));
    let (mut nones, mut somes) = (0, 0);
    for step in 0..120 {
        // (A period of 1000 s: moving the time by a tenth of a nanosecond moves the window across the grid.)
        let elements = orbit(
            1000.0,
            0.5,
            0.0,
            1000.0,
            std::f64::consts::PI - 1.0e-6 + f64::from(step) * 1.0e-12,
            [0.0, 0.0, 1.0],
        );
        let found = elements.leaves_at(&distance, tick(0), at(128)).unwrap();
        match found {
            None => nones += 1,
            Some(instant) => {
                somes += 1;
                let place = elements.place_at(instant, at(256)).unwrap().position;
                let squared = &(&place[0].square() + &place[1].square()) + &place[2].square();
                assert!(
                    squared
                        .compare(&distance.rounded_to(at(256)).square())
                        .is_ge(),
                    "step {step}: in at {instant:?}"
                );
            }
        }
    }
    assert!(nones > 0 && somes > 0, "{nones} none and {somes} some");
}

#[test]
fn the_instant_is_on_the_grid_whatever_the_tick_rate_is() {
    // The same orbit and distance asked from two times that are the same time at two tick rates: one instant.
    let elements = orbit(1000.0, 0.5, 0.3, 1000.0, 0.7, [0.0, 0.0, 1.0]);
    let distance = wide(1200.0, at(128));
    let slow = elements
        .leaves_at(
            &distance,
            Time {
                units: 40,
                units_per_second: 20,
            },
            at(128),
        )
        .unwrap();
    let fast = elements
        .leaves_at(
            &distance,
            Time {
                units: 4800,
                units_per_second: 2400,
            },
            at(128),
        )
        .unwrap();
    assert_eq!(slow, fast);
}

#[test]
fn what_cannot_be_asked_is_refused() {
    let elements = orbit(1000.0, 0.5, 0.0, 1000.0, 0.0, [0.0, 0.0, 1.0]);
    for distance in [0.0, -5.0] {
        assert!(
            elements
                .leaves_at(&wide(distance, at(128)), tick(0), at(128))
                .is_err(),
            "{distance}"
        );
    }
    assert!(
        elements
            .leaves_at(
                &wide(1200.0, at(128)),
                Time {
                    units: 0,
                    units_per_second: 0
                },
                at(128)
            )
            .is_err()
    );
}

#[test]
fn a_crossing_too_far_in_the_future_for_the_clock_is_never() {
    // A period of 2^80 seconds and a body about to start: the crossing is a quarter period away, 2^78 seconds, and the clock holds 2^65 seconds of the grid.
    let elements = orbit(1.0e6, 0.5, 0.0, (1u128 << 80) as f64, 0.0, [0.0, 0.0, 1.0]);
    assert_eq!(
        elements
            .leaves_at(&wide(1.2e6, at(128)), tick(0), at(128))
            .unwrap(),
        None
    );
}

/// Orbits with their sphere and a time to start from: inside the sphere at the start.
fn cases() -> impl Strategy<Value = (Elements, f64, Time)> {
    (
        1.0e3f64..1.0e9,
        prop_oneof![0.05f64..1.0, -20.0f64..-0.05],
        -3.0f64..3.0,
        prop_oneof![100.0f64..1.0e7, -1.0e7f64..-100.0],
        -50.0f64..50.0,
        proptest::array::uniform3(-1.0f64..1.0),
        0.0f64..3.0,
        0i128..2_000_000,
    )
        .prop_filter_map(
            "a normal that is not nothing",
            |(radius, ratio, periapsis, period, phase, normal, reach, units)| {
                if normal.iter().all(|part| part.abs() < 1e-3) {
                    return None;
                }
                let elements = orbit(radius, ratio, periapsis, period, phase, normal);
                // The distance, as a multiple of the semi-major axis, from a bit over the periapsis to a bit over the far end (an ellipse) or to 30 axes (a hyperbola).
                let nearest = radius * ratio.abs();
                let farthest = if ratio > 0.0 {
                    radius * (2.0 - ratio)
                } else {
                    radius * 30.0
                };
                let distance = nearest + (farthest * 1.1 - nearest) * (reach / 3.0);
                Some((elements, distance, tick(units)))
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, max_global_rejects: 100_000, ..ProptestConfig::default() })]

    /// The instant is the first on the grid at which the body is out: it is out there, it was in at the instant before (unless that is before the time asked from), and it was in at
    /// instants between.
    #[test]
    fn the_instant_is_the_first_of_the_grid_at_which_the_body_is_out(
        (elements, distance, from) in cases(),
        fractions in proptest::collection::vec(0u32..1000, 6),
    ) {
        prop_assume!(!is_outside(&elements, distance, from));
        let found = elements
            .leaves_at(&wide(distance, at(128)), from, at(128))
            .unwrap();
        let Some(found) = found else {
            // Never: only an ellipse that does not reach the distance, or only touches it (within a millionth: a window of the grid too short for an instant to be in it is far narrower).
            prop_assert!(elements.ratio.is_positive(), "a hyperbola goes out");
            prop_assert!(
                distance >= elements.radius.to_f64() * (2.0 - elements.ratio.to_f64()) * (1.0 - 1e-6),
                "the ellipse reaches {distance}"
            );
            return Ok(());
        };
        prop_assert_eq!(found.units_per_second, GRID);
        let first = grid_units_after(from);
        prop_assert!(found.units >= first);
        prop_assert!(is_outside(&elements, distance, found), "out at {found:?}");
        if found.units > first {
            prop_assert!(
                !is_outside(&elements, distance, on_the_grid(found.units - 1)),
                "in at the instant before {found:?}"
            );
        }
        // And in at instants between the start and the crossing.
        for fraction in fractions {
            let between = first + (found.units - first) * i128::from(fraction) / 1000;
            if between < found.units {
                prop_assert!(
                    !is_outside(&elements, distance, on_the_grid(between)),
                    "in at {between}, before {found:?}"
                );
            }
        }
    }

    /// The tick rate has nothing to do with it: asked from the same time written at another rate, the answer is the same.
    #[test]
    fn the_answer_is_the_same_from_a_time_written_in_other_units(
        (elements, distance, from) in cases(),
        factor in 2u32..50,
    ) {
        let rewritten = Time {
            units: from.units * i128::from(factor),
            units_per_second: from.units_per_second * factor,
        };
        prop_assert_eq!(
            elements.leaves_at(&wide(distance, at(128)), from, at(128)).unwrap(),
            elements.leaves_at(&wide(distance, at(128)), rewritten, at(128)).unwrap()
        );
    }

    /// A wider answer is the same instant (the grid is far coarser than the width).
    #[test]
    fn a_wider_working_gives_the_same_instant((elements, distance, from) in cases()) {
        prop_assert_eq!(
            elements.leaves_at(&wide(distance, at(128)), from, at(128)).unwrap(),
            elements.leaves_at(&wide(distance, at(128)), from, at(512)).unwrap()
        );
    }
}
