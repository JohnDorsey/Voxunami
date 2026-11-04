//! The orbit a body is on, from its place and its speed (`Elements::from_state`): the inverse of `place_at`. Held to the truth of `reference/fit.txt` (made by
//! `reference/generate_fit.py`: elements chosen, the place and the speed worked out forward in mpmath, 256 bits more than the width asked for), and to what every state satisfies.

use wide_float::{Precision, WideFloat};
use wide_kepler::{Elements, KeplerError, Place, Time};

mod common;
use common::{loss_of as loss, reduced, unit};

const ROWS: &str = include_str!("reference/fit.txt");

/// How many bits of the width asked for a found element may lose, in the units of its own size (relative) or of 1 (an angle, a unit vector). Measured over the rows below: the
/// radius, the ratio, the period and the normal are the rows' own to the last bit, the angles within a bit.
const LOSS_ELEMENTS: i64 = 2;

/// How many bits of the width the place and the speed that the found elements give back may lose against the state the elements were found from. Measured: 2 and 1.
const LOSS_STATE: i64 = 4;

fn at(bits: u64) -> Precision {
    Precision::new(bits).unwrap()
}

fn rows() -> Vec<common::FitRow> {
    common::fit_rows(ROWS)
}

#[test]
fn the_orbit_found_is_the_orbit_the_state_came_from() {
    let mut worst = [i64::MIN; 6];
    let mut compared = 0;
    for row in rows() {
        let found = Elements::from_state(
            &row.position,
            &row.velocity,
            &row.mu,
            row.time,
            at(row.bits),
        )
        .unwrap_or_else(|error| panic!("{} bits, {:?}: {error}", row.bits, row.elements));
        // Every element has the width asked for, but the phase, which has the bits the orbit magnifies it by as well.
        for part in found.normal.iter().chain([
            &found.radius,
            &found.ratio,
            &found.periapsis,
            &found.period,
        ]) {
            assert_eq!(part.precision(), at(row.bits));
        }
        assert!(found.phase.precision() >= at(row.bits));
        // The phase is an angle from -pi to pi.
        let pi = WideFloat::pi(at(row.bits + 512));
        assert!(
            found.phase.abs().compare(&pi) != std::cmp::Ordering::Greater,
            "{:?}",
            found.phase
        );
        let one = WideFloat::from_i64(1, at(row.bits));
        let (n_found, n_row) = (
            unit(&found.normal, row.bits),
            unit(&row.elements.normal, row.bits),
        );
        // The angles are measured from an axis that the normal's x **as kept** decides (0.9 or more takes +z), so they are the same angles only if both normals take the same axis.
        let threshold = WideFloat::from_f64(0.9, at(row.bits)).unwrap();
        let takes_x = |normal: &[WideFloat; 3]| {
            normal[0].abs().compare(&threshold) == std::cmp::Ordering::Less
        };
        // And the angle of the nearest point is only as well known as the eccentricity vector is long: its error is 2^-(width + 128) over the eccentricity, so for an orbit that is
        // a circle to 2^-100 or nearer it is not a number the state can say (the next test says the body is where it was all the same, which is all that matters then).
        let eccentricity = &one - &row.elements.ratio;
        let says_the_angles = takes_x(&found.normal) == takes_x(&row.elements.normal)
            && eccentricity.floor_log2().is_some_and(|bits| bits >= -100);
        let same_axis = says_the_angles;
        compared += usize::from(same_axis);
        let losses = [
            loss(
                row.bits,
                &found.radius,
                &row.elements.radius,
                &row.elements.radius,
            ),
            loss(
                row.bits,
                &found.ratio,
                &row.elements.ratio,
                &row.elements.ratio,
            ),
            loss(
                row.bits,
                &found.period,
                &row.elements.period,
                &row.elements.period,
            ),
            (0..3)
                .map(|axis| loss(row.bits, &n_found[axis], &n_row[axis], &one))
                .max()
                .unwrap(),
            loss(
                row.bits,
                &reduced(&found.periapsis, row.bits),
                &reduced(&row.elements.periapsis, row.bits),
                &one,
            ),
            loss(
                row.bits,
                &reduced(&found.phase, row.bits),
                &reduced(&row.elements.phase, row.bits),
                &one,
            ),
        ];
        let names = ["radius", "ratio", "period", "normal", "periapsis", "phase"];
        for (index, (name, loss)) in names.iter().zip(losses).enumerate() {
            // (The two angles are not compared where the axis is not the same.)
            if index >= 4 && !same_axis {
                continue;
            }
            worst[index] = worst[index].max(loss);
            assert!(
                loss <= LOSS_ELEMENTS,
                "{} bits, ratio {:e}: the {name} loses {loss} bits ({:?} against {:?})",
                row.bits,
                row.elements.ratio.to_f64(),
                found,
                row.elements
            );
        }
    }
    println!("worst loss of radius, ratio, period, normal, periapsis, phase: {worst:?}");
    assert!(
        compared * 10 >= rows().len() * 8,
        "the angles were compared in {compared} of {} rows",
        rows().len()
    );
}

#[test]
fn the_orbit_found_puts_the_body_where_it_was_with_the_speed_it_had() {
    let mut worst = (i64::MIN, i64::MIN);
    for row in rows() {
        let found = Elements::from_state(
            &row.position,
            &row.velocity,
            &row.mu,
            row.time,
            at(row.bits),
        )
        .unwrap();
        let Place { position, velocity } = found.place_at(row.time, at(row.bits)).unwrap();
        let radius = &row.elements.radius;
        let speed = row
            .velocity
            .iter()
            .map(WideFloat::abs)
            .max_by(|a, b| a.compare(b))
            .unwrap();
        let place_loss = (0..3)
            .map(|axis| loss(row.bits, &position[axis], &row.position[axis], radius))
            .max()
            .unwrap();
        let speed_loss = (0..3)
            .map(|axis| loss(row.bits, &velocity[axis], &row.velocity[axis], &speed))
            .max()
            .unwrap();
        worst = (worst.0.max(place_loss), worst.1.max(speed_loss));
        assert!(
            place_loss <= LOSS_STATE && speed_loss <= LOSS_STATE,
            "{} bits, ratio {:e}: the place loses {place_loss} bits and the speed {speed_loss}",
            row.bits,
            row.elements.ratio.to_f64()
        );
    }
    println!(
        "worst loss of the place and the speed when the elements found are worked out again: {worst:?}"
    );
}

/// A negative control: the same elements, asked for the place one second later, are not at the state (the test above can fail).
#[test]
fn the_state_is_that_of_its_time_and_no_other() {
    let row = rows().into_iter().find(|row| row.bits == 256).unwrap();
    let found = Elements::from_state(
        &row.position,
        &row.velocity,
        &row.mu,
        row.time,
        at(row.bits),
    )
    .unwrap();
    let later = Time {
        units: row.time.units + i128::from(row.time.units_per_second),
        ..row.time
    };
    let place = found.place_at(later, at(row.bits)).unwrap();
    let apart = (0..3)
        .map(|axis| (&place.position[axis] - &row.position[axis]).abs().to_f64())
        .fold(0.0, f64::max);
    assert!(
        apart > 1.0e-6 * row.elements.radius.to_f64() * row.elements.ratio.to_f64().min(1.0),
        "{apart}"
    );
}

fn exactly(value: f64) -> WideFloat {
    WideFloat::from_f64(value, at(128)).unwrap()
}

fn state(position: [f64; 3], velocity: [f64; 3]) -> ([WideFloat; 3], [WideFloat; 3]) {
    (position.map(exactly), velocity.map(exactly))
}

#[test]
fn a_state_that_is_exactly_a_circle_is_a_circle_and_not_a_hair_off() {
    // A body 4 voxels along +x going 0.5 along +y about a mass parameter of 1: v^2 = mu / r, and every number is a double, so the eccentricity is exactly nothing.
    let (position, velocity) = state([4.0, 0.0, 0.0], [0.0, 0.5, 0.0]);
    for bits in [128u64, 256] {
        let found = Elements::from_state(
            &position,
            &velocity,
            &exactly(1.0),
            Time {
                units: 0,
                units_per_second: 20,
            },
            at(bits),
        )
        .unwrap();
        assert_eq!(
            found.ratio,
            WideFloat::from_i64(1, at(bits)),
            "a ratio of exactly one"
        );
        assert!(found.periapsis.is_zero());
        assert_eq!(found.radius, WideFloat::from_i64(4, at(bits)));
        // The body goes round counter-clockwise looking down +z; the circle starts toward +x, where the body is: a phase of nothing.
        assert!(found.phase.is_zero(), "{:?}", found.phase);
        // The period is 2 pi sqrt(a^3 / mu) = 16 pi.
        let sixteen_pi = WideFloat::pi(at(bits)).times_power_of_two(4).unwrap();
        assert!(loss(bits, &found.period, &sixteen_pi, &sixteen_pi) <= 1);
    }
}

#[test]
fn a_state_a_hair_under_the_speed_of_escape_is_the_huge_ellipse_the_energy_says() {
    // r = 1, mu = 2^100, and a speed across the radius of sqrt(2 mu) less 2^-60 of itself: 1 / a = 2 - v^2 / mu = 2^-58 and a little, so a is about 2^58 voxels, the ratio
    // about 2^-58 (the thinnest an orbit may be is 2^-64), and the energy is the difference of two numbers that agree to 58 bits.
    let precision = at(512);
    let one = WideFloat::from_i64(1, precision);
    let mu = one.times_power_of_two(100).unwrap();
    let root_two = WideFloat::from_i64(2, precision).sqrt().unwrap();
    let speed = &(&root_two * &one.times_power_of_two(50).unwrap())
        * &(&one - &one.times_power_of_two(-60).unwrap());
    let position = [
        one.clone(),
        WideFloat::zero(precision),
        WideFloat::zero(precision),
    ];
    let velocity = [
        WideFloat::zero(precision),
        speed.clone(),
        WideFloat::zero(precision),
    ];
    let found = Elements::from_state(
        &position,
        &velocity,
        &mu,
        Time {
            units: 0,
            units_per_second: 20,
        },
        at(256),
    )
    .unwrap();
    // The size the energy gives, in 4,096 bits: a = 1 / (2 / r - v^2 / mu).
    let high = at(4096);
    let expected = WideFloat::from_i64(1, high)
        .divide(
            &(&WideFloat::from_i64(2, high)
                - &speed
                    .rounded_to(high)
                    .square()
                    .divide(&mu.rounded_to(high))
                    .unwrap()),
        )
        .unwrap();
    assert!(
        loss(256, &found.radius, &expected, &expected) <= LOSS_ELEMENTS,
        "{:?} against {:?}",
        found.radius,
        expected
    );
    assert_eq!(found.radius.floor_log2(), Some(58));
}

#[test]
fn what_is_on_no_orbit_is_refused() {
    let time = Time {
        units: 0,
        units_per_second: 20,
    };
    let p = at(128);
    let (position, velocity) = state([4.0, 0.0, 0.0], [0.0, 0.5, 0.0]);
    let mu = exactly(1.0);
    assert!(Elements::from_state(&position, &velocity, &mu, time, p).is_ok());
    type Case = (&'static str, ([WideFloat; 3], [WideFloat; 3]), WideFloat);
    let cases: Vec<Case> = vec![
        (
            "on the centre",
            state([0.0; 3], [0.0, 0.5, 0.0]),
            mu.clone(),
        ),
        ("at rest", state([4.0, 0.0, 0.0], [0.0; 3]), mu.clone()),
        (
            "a straight fall",
            state([4.0, 0.0, 0.0], [-0.25, 0.0, 0.0]),
            mu.clone(),
        ),
        (
            "a straight flight away",
            state([4.0, 0.0, 0.0], [0.25, 0.0, 0.0]),
            mu.clone(),
        ),
        // v^2 = 2 mu / r exactly: a parabola, which is not made. (A state that goes faster than that is on a hyperbola: `fit_hyperbola.rs`.)
        (
            "a parabola",
            state([2.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            mu.clone(),
        ),
        (
            "a mass of nothing",
            state([4.0, 0.0, 0.0], [0.0, 0.5, 0.0]),
            exactly(0.0),
        ),
        (
            "a mass below nothing",
            state([4.0, 0.0, 0.0], [0.0, 0.5, 0.0]),
            exactly(-1.0),
        ),
    ];
    for (what, (position, velocity), mu) in cases {
        assert!(
            matches!(
                Elements::from_state(&position, &velocity, &mu, time, p),
                Err(KeplerError::Invalid(_))
            ),
            "{what} is on no orbit"
        );
    }
    // A time the clock cannot hold.
    assert!(matches!(
        Elements::from_state(
            &position,
            &velocity,
            &mu,
            Time {
                units: 1 << 96,
                units_per_second: 20
            },
            p
        ),
        Err(KeplerError::Invalid(_))
    ));
}

#[test]
fn an_orbit_is_found_whatever_the_width_of_the_state_it_is_given() {
    // The same exact state given as numbers of 128, 192 and 1,024 bits (a double is exact in all of them) is the same orbit to the width asked for.
    let time = Time {
        units: 12_345,
        units_per_second: 20,
    };
    let make = |bits: u64| {
        let p = at(bits);
        let w = |value: f64| WideFloat::from_f64(value, p).unwrap();
        Elements::from_state(
            &[w(1.5e9), w(-2.5e8), w(7.0e8)],
            &[w(-100.0), w(250.0), w(30.0)],
            &w(1.0e15),
            time,
            at(256),
        )
        .unwrap()
    };
    assert_eq!(make(128), make(192));
    assert_eq!(make(128), make(1024));
}

#[test]
fn a_normal_that_rounds_across_the_axis_the_plane_starts_from_still_gives_the_orbit_back() {
    // The circle starts toward +x unless the normal's x, **as kept**, is 0.9 or more (then toward +z). A normal whose x is 2^-135 under the double 0.9 takes +x as it is given
    // and +z once it is rounded to 128 bits, which is how the orbit found keeps it: the plane has to be made from the number kept, or the angles found are measured from the
    // wrong axis and the body is not where it was.
    let high = at(512);
    let x = &WideFloat::from_f64(0.9, high).unwrap()
        - &WideFloat::from_i64(1, high)
            .times_power_of_two(-135)
            .unwrap();
    let rest = (&WideFloat::from_i64(1, high) - &x.square())
        .sqrt()
        .unwrap();
    let fifths = |parts: i64| {
        WideFloat::from_i64(parts, high)
            .divide(&WideFloat::from_i64(5, high))
            .unwrap()
    };
    // (A direction whose squares add up to one to 512 bits: 3/5 and 4/5 of what is left over, and not the doubles 0.6 and 0.8, which are 1e-17 off.)
    let normal = [x.clone(), &rest * &fifths(3), &rest * &fifths(4)];
    let elements = Elements {
        normal,
        radius: WideFloat::from_i64(1_000_000, high),
        ratio: WideFloat::from_f64(0.3, high).unwrap(),
        periapsis: WideFloat::from_f64(0.7, high).unwrap(),
        period: WideFloat::from_i64(5000, high),
        phase: WideFloat::from_f64(1.0, high).unwrap(),
    };
    let time = Time {
        units: 777,
        units_per_second: 20,
    };
    let Place { position, velocity } = elements.place_at(time, high).unwrap();
    let rate = WideFloat::pi(high)
        .times_power_of_two(1)
        .unwrap()
        .divide(&elements.period)
        .unwrap();
    let mu = &(&rate.square() * &elements.radius.square()) * &elements.radius;
    let found = Elements::from_state(&position, &velocity, &mu, time, at(128)).unwrap();
    // The normal kept took the +z axis, as the given one did not.
    assert_ne!(
        found.normal[0]
            .abs()
            .compare(&WideFloat::from_f64(0.9, at(128)).unwrap()),
        std::cmp::Ordering::Less,
        "the normal kept is 0.9 or more: the +z axis"
    );
    let again = found.place_at(time, at(128)).unwrap();
    for (axis, (found, expected)) in again.position.iter().zip(&position).enumerate() {
        assert!(
            loss(128, found, expected, &elements.radius) <= LOSS_STATE,
            "axis {axis}: {found:?} against {expected:?}"
        );
    }
}
