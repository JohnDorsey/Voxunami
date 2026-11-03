//! The orbit a body is on when it is going faster than it would to escape (`Elements::from_state` for a hyperbola): the inverse of `place_at`. Held to the truth of
//! `reference/fit_hyperbola.txt` (made by `reference/generate_fit_hyperbola.py`: elements chosen, the place and the speed worked out forward in mpmath by the true anomaly, 256 bits more
//! than the width asked for), and to what every state satisfies. A state at exactly the speed of escape is a parabola, which is not made.
//!
//! The phase of a hyperbola is the mean anomaly at time 0 **as it is**, and may have 99 whole bits (the clock is under 2^96 units and a mean motion is under 2^23 a second): it is kept to
//! the bits of its size and the width, or the body would not be where it was.

use wide_float::WideFloat;
use wide_kepler::{Elements, KeplerError, Place, Time};

mod common;
use common::{at, fit_rows, loss_of as loss, reduced, unit};

const ROWS: &str = include_str!("reference/fit_hyperbola.txt");

/// How many bits of the width asked for a found element may lose, in the units of its own size (relative) or of 1 (an angle, a unit vector) or of the larger of 1 and the phase.
/// Measured over the rows below (the test prints the worst).
const LOSS_ELEMENTS: i64 = 2;

/// How many bits of the width the place and the speed that the found elements give back may lose against the state the elements were found from, in units of the larger of the radius
/// and the size of the place (a body far out is far from the size of its hyperbola). Measured: see the test.
const LOSS_STATE: i64 = 4;

fn exactly(value: f64, bits: u64) -> WideFloat {
    WideFloat::from_f64(value, at(bits)).unwrap()
}

#[test]
fn the_orbit_found_is_the_orbit_the_state_came_from() {
    let mut worst = [i64::MIN; 6];
    let rows = fit_rows(ROWS);
    assert!(rows.len() >= 70, "{} rows", rows.len());
    for row in &rows {
        let found = Elements::from_state(
            &row.position,
            &row.velocity,
            &row.mu,
            row.time,
            at(row.bits),
        )
        .unwrap_or_else(|error| panic!("{} bits, {:?}: {error}", row.bits, row.elements));
        assert!(found.is_a_hyperbola(), "{:?}", found.ratio);
        // Every element has the width asked for, but the phase, which has the bits of its size as well, and those the orbit magnifies it by.
        for part in found.normal.iter().chain([
            &found.radius,
            &found.ratio,
            &found.periapsis,
            &found.period,
        ]) {
            assert_eq!(part.precision(), at(row.bits));
        }
        let whole_bits = found
            .phase
            .abs()
            .floor_log2()
            .map_or(0, |bits| (bits + 1).max(0)) as u64;
        assert!(
            found.phase.precision().bits() >= row.bits + whole_bits,
            "{} bits and {whole_bits} whole",
            found.phase.precision().bits()
        );
        let one = WideFloat::from_i64(1, at(row.bits));
        let (n_found, n_row) = (
            unit(&found.normal, row.bits),
            unit(&row.elements.normal, row.bits),
        );
        // The angle of the periapsis is measured from an axis that the normal's x **as kept** decides (0.9 or more takes +z): it is the same angle only if both normals take the same axis.
        let threshold = WideFloat::from_f64(0.9, at(row.bits)).unwrap();
        let takes_x = |normal: &[WideFloat; 3]| {
            normal[0].abs().compare(&threshold) == std::cmp::Ordering::Less
        };
        let same_axis = takes_x(&found.normal) == takes_x(&row.elements.normal);
        // The thinner the hyperbola, the more a change of the mean anomaly moves the body (by the size of the ratio to the power of a half) and its speed (to the power of one and
        // a half): the phase has to be right to 2^-(width + 1.5 m) of an absolute number, m being the bits the ratio is under 1.
        let magnified_by = row
            .elements
            .ratio
            .abs()
            .floor_log2()
            .map_or(0, |bits| (-bits).max(0)) as u64;
        let phase_needs = (3 * magnified_by).div_ceil(2) as i64;
        // The mean anomaly at the time of the state, `phase + 2 pi t / period`, from the row's numbers: the phase may be the sum of this and a clock term of 99 bits that cancels it
        // (a body at its periapsis a long time after time 0), and then the phase must be right to the bits of the width less those of this number, which are few: that is what the
        // extra bits of the working are for. Far out this number is large, a change of it moves the body less than its own size, and the phase is only as good as it is.
        let high = at(row.bits + 512);
        let rate = WideFloat::pi(high)
            .times_power_of_two(1)
            .unwrap()
            .divide(&row.elements.period.rounded_to(high))
            .unwrap();
        let seconds = WideFloat::from_i128(row.time.units, high)
            .divide(&WideFloat::from_i64(
                i64::from(row.time.units_per_second),
                high,
            ))
            .unwrap();
        let mean = &row.elements.phase.rounded_to(high) + &(&rate * &seconds);
        let scale = [one.clone().rounded_to(high), mean.abs()]
            .into_iter()
            .max_by(|a, b| a.compare(b))
            .unwrap();
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
            // The phase is not reduced: it is compared as it is, in the units of the mean anomaly at the time of the state (not of its own size: a body a long time from time 0 has a
            // phase of 99 bits), and its loss is counted from the bits it must have, which are those of the width and of what the thinness of the orbit magnifies it by.
            loss(row.bits, &found.phase, &row.elements.phase, &scale) + phase_needs,
        ];
        let names = ["radius", "ratio", "period", "normal", "periapsis", "phase"];
        for (index, (name, loss)) in names.iter().zip(losses).enumerate() {
            if index == 4 && !same_axis {
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
}

#[test]
fn the_orbit_found_puts_the_body_where_it_was_with_the_speed_it_had() {
    let mut worst = (i64::MIN, i64::MIN);
    for row in fit_rows(ROWS) {
        let found = Elements::from_state(
            &row.position,
            &row.velocity,
            &row.mu,
            row.time,
            at(row.bits),
        )
        .unwrap();
        let Place { position, velocity } = found.place_at(row.time, at(row.bits)).unwrap();
        let size = row
            .position
            .iter()
            .map(WideFloat::abs)
            .max_by(|a, b| a.compare(b))
            .unwrap();
        let scale = [size, row.elements.radius.clone()]
            .into_iter()
            .max_by(|a, b| a.compare(b))
            .unwrap();
        let speed = row
            .velocity
            .iter()
            .map(WideFloat::abs)
            .max_by(|a, b| a.compare(b))
            .unwrap();
        let place_loss = (0..3)
            .map(|axis| loss(row.bits, &position[axis], &row.position[axis], &scale))
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
    let row = fit_rows(ROWS)
        .into_iter()
        .find(|row| row.bits == 256)
        .unwrap();
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
        apart > 1.0e-6 * row.elements.radius.to_f64() * row.elements.ratio.to_f64().abs().min(1.0),
        "{apart}"
    );
}

#[test]
fn a_state_at_exactly_the_speed_of_escape_is_a_parabola_and_is_refused_and_a_hair_over_is_a_hyperbola()
 {
    let p = at(256);
    let time = Time {
        units: 0,
        units_per_second: 20,
    };
    let mu = exactly(1.0, 256);
    let position = [exactly(2.0, 256), exactly(0.0, 256), exactly(0.0, 256)];
    let state = |speed: WideFloat| [exactly(0.0, 256), speed, exactly(0.0, 256)];
    // r = 2, mu = 1: the speed of escape is sqrt(2 mu / r) = 1, exactly.
    assert!(matches!(
        Elements::from_state(&position, &state(exactly(1.0, 256)), &mu, time, p),
        Err(KeplerError::Invalid(_))
    ));
    // A speed of 1 + 2^-40, across the radius: the body is at its periapsis, 2 voxels from the centre, 1 / a = 2 / r - v^2 / mu = -(2^-39 + 2^-80), so |a| = 1 / (2^-39 + 2^-80) and
    // the size of the ratio is the periapsis over |a|, 2 (2^-39 + 2^-80).
    let power = |exponent: i64| {
        WideFloat::from_i64(1, at(1024))
            .times_power_of_two(exponent)
            .unwrap()
    };
    let speed =
        &WideFloat::from_i64(1, p) + &WideFloat::from_i64(1, p).times_power_of_two(-40).unwrap();
    let found = Elements::from_state(&position, &state(speed), &mu, time, p).unwrap();
    let excess = &power(-39) + &power(-80);
    let radius = WideFloat::from_i64(1, at(1024)).divide(&excess).unwrap();
    let ratio = -&(&WideFloat::from_i64(2, at(1024)) * &excess);
    assert!(
        loss(256, &found.radius, &radius, &radius) <= LOSS_ELEMENTS,
        "{:?} against {radius:?}",
        found.radius
    );
    assert!(
        loss(256, &found.ratio, &ratio, &ratio) <= LOSS_ELEMENTS,
        "{:?} against {ratio:?}",
        found.ratio
    );
    // And at 1 + 2^-100 the hyperbola is thinner than an orbit may be (2^-98 is under 2^-64): the state is one, the orbit is refused for its limits.
    let speed =
        &WideFloat::from_i64(1, p) + &WideFloat::from_i64(1, p).times_power_of_two(-100).unwrap();
    assert!(matches!(
        Elements::from_state(&position, &state(speed), &mu, time, p),
        Err(KeplerError::Invalid(_))
    ));
}

#[test]
fn a_hyperbola_is_found_whatever_the_width_of_the_state_it_is_given() {
    // The same exact state given as numbers of 128, 192 and 1,024 bits (a double is exact in all of them) is the same orbit to the width asked for.
    let time = Time {
        units: 12_345,
        units_per_second: 20,
    };
    let make = |bits: u64| {
        (
            [exactly(3.0, bits), exactly(4.0, bits), exactly(0.0, bits)],
            [exactly(-1.5, bits), exactly(2.0, bits), exactly(0.3, bits)],
            exactly(1.0, bits),
        )
    };
    let (position, velocity, mu) = make(128);
    let reference = Elements::from_state(&position, &velocity, &mu, time, at(128)).unwrap();
    assert!(reference.is_a_hyperbola());
    for bits in [192, 1024] {
        let (position, velocity, mu) = make(bits);
        assert_eq!(
            Elements::from_state(&position, &velocity, &mu, time, at(128)).unwrap(),
            reference
        );
    }
}

#[test]
fn a_hyperbola_found_again_from_where_it_goes_next_is_the_same_hyperbola() {
    // The orbit is a property of the path, not of the moment the state was taken at: the state a thousand seconds later, from the orbit found, is on the same orbit, with the same phase
    // (the mean anomaly at time 0 as it is).
    let mut worst = i64::MIN;
    for row in fit_rows(ROWS)
        .into_iter()
        .filter(|row| row.bits == 256)
        .take(10)
    {
        let first =
            Elements::from_state(&row.position, &row.velocity, &row.mu, row.time, at(256)).unwrap();
        let later = Time {
            units: row.time.units + 1000 * i128::from(row.time.units_per_second),
            ..row.time
        };
        // (The state has 256 bits more than the fit is asked for, as the reference rows do: the energy of a hyperbola that is barely one is the difference of two numbers that agree to
        // 61 bits, and a state held only to the width would give the ratio to that much less.)
        let Place { position, velocity } = first.place_at(later, at(512)).unwrap();
        let second = Elements::from_state(&position, &velocity, &row.mu, later, at(256)).unwrap();
        let one = WideFloat::from_i64(1, at(256));
        let scale = [one.clone(), first.phase.abs()]
            .into_iter()
            .max_by(|a, b| a.compare(b))
            .unwrap();
        for (name, loss) in [
            (
                "radius",
                loss(256, &second.radius, &first.radius, &first.radius),
            ),
            (
                "ratio",
                loss(256, &second.ratio, &first.ratio, &first.ratio),
            ),
            (
                "period",
                loss(256, &second.period, &first.period, &first.period),
            ),
            ("phase", loss(256, &second.phase, &first.phase, &scale)),
        ] {
            worst = worst.max(loss);
            assert!(
                loss <= 8,
                "{name} loses {loss} bits: {second:?} against {first:?}"
            );
        }
    }
    println!(
        "worst loss between the orbit found and the orbit found from where it goes next: {worst}"
    );
}

#[test]
fn what_a_state_cannot_be_is_still_refused() {
    let time = Time {
        units: 0,
        units_per_second: 20,
    };
    let p = at(128);
    let mu = exactly(1.0, 128);
    // A fast straight flight away has no plane to go round in; a mass of nothing pulls nothing.
    for (what, position, velocity, mu) in [
        (
            "a straight flight away at the speed of a hyperbola",
            [4.0, 0.0, 0.0],
            [2.5, 0.0, 0.0],
            mu.clone(),
        ),
        (
            "a mass of nothing",
            [4.0, 0.0, 0.0],
            [0.0, 1.5, 0.0],
            exactly(0.0, 128),
        ),
    ] {
        let state = |v: [f64; 3]| v.map(|component| exactly(component, 128));
        assert!(
            matches!(
                Elements::from_state(&state(position), &state(velocity), &mu, time, p),
                Err(KeplerError::Invalid(_))
            ),
            "{what} is on no orbit"
        );
    }
}
