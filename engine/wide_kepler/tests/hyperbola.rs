//! The place and the speed of a body on a hyperbola in wide numbers: against the truth worked out by other code, by another way (`reference/hyperbola.txt`, from `reference/generate_hyperbola.py`:
//! by the true anomaly and the angular momentum, where the Rust goes by the hyperbolic functions of the anomaly), and against what every hyperbola must satisfy without a reference: the speed
//! at each distance (the energy), the angular momentum, the mirror image about the apsis line, and the other way round being the same path run backwards.
//!
//! A hyperbola is a `ratio` (1 - e) below nothing; `radius` is the size of its semi-major axis; `period` is 2 pi over the mean motion (negative for the other way round); `phase` is the mean
//! anomaly at time 0 **as it is**, not taken modulo a turn (a hyperbola goes round no turn).

use wide_float::{Precision, WideFloat};
use wide_kepler::{Elements, KeplerError, Place, Time};

mod common;
use common::{at, largest, loss, rows};

const ROWS: &str = include_str!("reference/hyperbola.txt");

/// How many bits of the width asked for the answer may lose, in units of the larger of the radius and the size of the place (a body far out is far from a hyperbola's size, and an answer is
/// as good as a part of the place): the same bound as the ellipse's. The test prints the worst.
const LOSS: i64 = 4;

fn wide(value: f64, precision: Precision) -> WideFloat {
    WideFloat::from_f64(value, precision).unwrap()
}

#[test]
fn the_answer_loses_at_most_a_few_bits() {
    let mut worst_place = i64::MIN;
    let mut worst_velocity = i64::MIN;
    let all = rows(ROWS);
    assert!(all.len() >= 150, "{} rows", all.len());
    for row in all {
        let Place { position, velocity } = row.elements.place_at(row.time, at(row.bits)).unwrap();
        let scale = [largest(&row.place), row.elements.radius.clone()]
            .into_iter()
            .max_by(|a, b| a.compare(b))
            .unwrap();
        let place_loss = loss(row.bits, &position, &row.place, &scale);
        let velocity_loss = loss(row.bits, &velocity, &row.velocity, &largest(&row.velocity));
        println!(
            "{:>5} bits  ratio {:>12.3e}  place loses {place_loss:>3}  speed loses {velocity_loss:>3}",
            row.bits,
            row.elements.ratio.to_f64()
        );
        worst_place = worst_place.max(place_loss);
        worst_velocity = worst_velocity.max(velocity_loss);
        assert!(
            place_loss <= LOSS,
            "{} bits, {:?}: the place loses {place_loss} bits",
            row.bits,
            row.elements
        );
        assert!(
            velocity_loss <= LOSS,
            "{} bits, {:?}: the speed loses {velocity_loss} bits",
            row.bits,
            row.elements
        );
    }
    println!("worst: the place loses {worst_place} bits, the speed {worst_velocity}");
}

/// A hyperbola in the plane z = 0, going counter-clockwise (the normal is +z), the periapsis along +x, at the periapsis at time 0: `ratio` is -`excess` (e = 1 + `excess`), `radius` is
/// |a|, and `period` is the notional period 2 pi / n.
fn hyperbola(excess: f64, radius: f64, period: f64, precision: Precision) -> Elements {
    Elements {
        normal: [
            wide(0.0, precision),
            wide(0.0, precision),
            wide(1.0, precision),
        ],
        radius: wide(radius, precision),
        ratio: wide(-excess, precision),
        periapsis: wide(0.0, precision),
        period: wide(period, precision),
        phase: wide(0.0, precision),
    }
}

fn time(units: i128) -> Time {
    Time {
        units,
        units_per_second: 20,
    }
}

/// The hyperbolas the invariants are tried on: barely one, thin, middling, wide and extreme, in a few sizes and speeds.
fn cases(precision: Precision) -> Vec<Elements> {
    vec![
        hyperbola(1.0 / 18446744073709551616.0, 1.0e6, 5_000.0, precision),
        hyperbola(1.0 / 1048576.0, 7.0e4, 600.0, precision),
        hyperbola(0.3, 12_345.678, 100.0, precision),
        hyperbola(1.0, 5.0e3, 40.0, precision),
        hyperbola(16.0, 4.4e5, 3_600.0, precision),
        hyperbola(1.0e6, 4.0, 1.0, precision),
        hyperbola(18446744073709551616.0, 9.0e9, 86_400.0, precision),
    ]
}

const TIMES: [i128; 9] = [
    0,
    1,
    777,
    123_456,
    98_765_432,
    40_000_000_000,
    1 << 50,
    (1 << 70) + 12_345,
    (1 << 90) + 1,
];

fn dot(a: &[WideFloat; 3], b: &[WideFloat; 3]) -> WideFloat {
    &(&(&a[0] * &b[0]) + &(&a[1] * &b[1])) + &(&a[2] * &b[2])
}

fn cross(a: &[WideFloat; 3], b: &[WideFloat; 3]) -> [WideFloat; 3] {
    [
        &(&a[1] * &b[2]) - &(&a[2] * &b[1]),
        &(&a[2] * &b[0]) - &(&a[0] * &b[2]),
        &(&a[0] * &b[1]) - &(&a[1] * &b[0]),
    ]
}

/// `log2` of `|x - y| / scale`, very negative if they are equal.
fn relative_difference(x: &WideFloat, y: &WideFloat, scale: &WideFloat) -> i64 {
    (x - y).abs().floor_log2().map_or(i64::MIN / 2, |exponent| {
        exponent - scale.floor_log2().unwrap()
    })
}

#[test]
fn the_speed_at_each_distance_is_the_one_of_the_energy_and_the_angular_momentum_is_the_one_of_the_shape()
 {
    // v^2 = mu (2 / r + 1 / |a|) with mu = n^2 |a|^3 (n = 2 pi / period), and |r x v| = sqrt(mu p) with p = |a| (e^2 - 1): checked in wide numbers of 256 bits from places of 256.
    let p = at(256);
    let pi = WideFloat::pi(p);
    let mut worst = i64::MIN;
    for elements in cases(p) {
        let n = pi
            .times_power_of_two(1)
            .unwrap()
            .divide(&elements.period)
            .unwrap();
        let radius = &elements.radius;
        let mu = &(&n.square() * &radius.square()) * radius;
        let one = WideFloat::from_i64(1, p);
        let e = &one - &elements.ratio;
        let parameter = radius * &(&e.square() - &one);
        let angular_momentum_squared = &mu * &parameter;
        for units in TIMES {
            let Place { position, velocity } = elements.place_at(time(units), p).unwrap();
            let distance = dot(&position, &position).sqrt().unwrap();
            let speed_squared = dot(&velocity, &velocity);
            let energy = &mu
                * &(&WideFloat::from_i64(2, p).divide(&distance).unwrap()
                    + &one.divide(radius).unwrap());
            let by_energy = relative_difference(&speed_squared, &energy, &energy);
            let momentum = cross(&position, &velocity);
            // (The angular momentum is the small difference of large products where the body is far out and going almost straight away, as on a hyperbola that is barely one: it is
            // compared in units of (distance x speed)^2, which is what its error is made of, and not of its own size.)
            let scale = &distance.square() * &speed_squared;
            let by_momentum = relative_difference(
                &dot(&momentum, &momentum),
                &angular_momentum_squared,
                &scale,
            );
            worst = worst.max(by_energy).max(by_momentum);
            assert!(
                by_energy <= -(256 - 24) && by_momentum <= -(256 - 24),
                "{:?} at {units} units: energy off by 2^{by_energy}, angular momentum by 2^{by_momentum}",
                elements.ratio
            );
        }
    }
    println!("worst: 2^{worst} of the value");
}

#[test]
fn before_the_periapsis_is_the_mirror_image_of_after_it_to_the_bit() {
    // The periapsis is at time 0, along +x, in the plane z = 0: a place at -t is the place at t with its y changed in sign, and the speed at -t is the speed at t with its x changed in sign.
    let p = at(192);
    for elements in cases(p) {
        for units in TIMES {
            let after = elements.place_at(time(units), p).unwrap();
            let before = elements.place_at(time(-units), p).unwrap();
            assert_eq!(before.position[0], after.position[0], "x at {units}");
            assert_eq!(before.position[1], -&after.position[1], "y at {units}");
            assert_eq!(before.position[2], after.position[2]);
            assert_eq!(before.velocity[0], -&after.velocity[0], "vx at {units}");
            assert_eq!(before.velocity[1], after.velocity[1], "vy at {units}");
        }
    }
}

#[test]
fn the_other_way_round_is_the_same_path_run_backwards_to_the_bit() {
    // A negative period: the place at t is the place of the same hyperbola the first way round at -t, and the speed is the first way's at -t reversed.
    let p = at(192);
    for forward in cases(p) {
        let mut backward = forward.clone();
        backward.period = -&forward.period;
        for units in TIMES {
            let there = backward.place_at(time(units), p).unwrap();
            let here = forward.place_at(time(-units), p).unwrap();
            assert_eq!(there.position, here.position, "place at {units}");
            assert_eq!(
                there.velocity,
                [0, 1, 2].map(|axis| -&here.velocity[axis]),
                "speed at {units}"
            );
        }
    }
}

#[test]
fn the_phase_is_the_mean_anomaly_at_time_0_and_is_not_taken_modulo_a_turn() {
    // A phase of 2 pi x 1000 + 0.5 is not the phase 0.5: a hyperbola goes round no turn. And a time and a phase that cancel give the periapsis: the body at time t with the phase -rate t
    // (rounded) is at the periapsis, to the rounding of the phase.
    let p = at(256);
    let base = hyperbola(0.25, 1000.0, 100.0, p);
    let mut half = base.clone();
    half.phase = wide(0.5, p);
    let mut turned = base.clone();
    turned.phase = &WideFloat::pi(p).times_power_of_two(1).unwrap() * &wide(1000.0, p);
    turned.phase = &turned.phase + &wide(0.5, p);
    let near = half.place_at(time(0), p).unwrap();
    let far = turned.place_at(time(0), p).unwrap();
    assert_ne!(near.position, far.position);
    // The speed of the one with the large phase is much more nearly the speed at infinity: it is a long way out.
    let speed = |place: &Place| dot(&place.velocity, &place.velocity).sqrt().unwrap();
    let infinity = &WideFloat::pi(p)
        .times_power_of_two(1)
        .unwrap()
        .divide(&base.period)
        .unwrap()
        * &base.radius;
    assert!(relative_difference(&speed(&far), &infinity, &infinity) < -10);
    assert!(relative_difference(&speed(&near), &infinity, &infinity) > -10);
    // Cancelling: 20 units a second, 4,000 units is 200 seconds.
    let rate = &WideFloat::pi(p)
        .times_power_of_two(1)
        .unwrap()
        .divide(&base.period)
        .unwrap()
        * &wide(200.0, p);
    let mut at_periapsis = base.clone();
    at_periapsis.phase = -&rate;
    let place = at_periapsis.place_at(time(4_000), p).unwrap();
    let nearest = &base.radius * &wide(0.25, p);
    // (The phase and the rate it cancels are rounded to 256 bits, one of them from the other's 448: the body is at the periapsis to that, not to the bit.)
    assert!(
        place.position[1].abs().floor_log2().unwrap() < base.radius.floor_log2().unwrap() - 200
    );
    assert!(relative_difference(&place.position[0], &nearest, &nearest) < -200);
}

#[test]
fn what_is_not_a_hyperbola_is_refused_and_what_is_one_is_not() {
    let p = at(128);
    let good = hyperbola(0.5, 1000.0, 600.0, p);
    let when = time(1000);
    assert!(good.place_at(when, p).is_ok());
    let refused = |what: &str, change: &dyn Fn(&mut Elements)| {
        let mut elements = good.clone();
        change(&mut elements);
        assert!(
            matches!(elements.place_at(when, p), Err(KeplerError::Invalid(_))),
            "{what} is accepted"
        );
    };
    let power = |exponent: i64| {
        WideFloat::from_i64(1, p)
            .times_power_of_two(exponent)
            .unwrap()
    };
    refused("a ratio of nothing (a parabola)", &|e| {
        e.ratio = WideFloat::zero(p)
    });
    refused("a ratio of -2^-65", &|e| e.ratio = -&power(-65));
    refused("a ratio of -2^65", &|e| e.ratio = -&power(65));
    refused("a phase of 2^121", &|e| e.phase = power(121));
    refused("a phase of -2^121", &|e| e.phase = -&power(121));
    // And at the limits themselves a hyperbola is one.
    for (what, change) in [
        (
            "a ratio of -2^-64",
            Box::new(|e: &mut Elements| e.ratio = -&power(-64)) as Box<dyn Fn(&mut Elements)>,
        ),
        (
            "a ratio of -2^64",
            Box::new(|e: &mut Elements| e.ratio = -&power(64)),
        ),
        (
            "a phase of 2^120",
            Box::new(|e: &mut Elements| e.phase = power(120)),
        ),
        (
            "a phase of -2^120",
            Box::new(|e: &mut Elements| e.phase = -&power(120)),
        ),
    ] {
        let mut elements = good.clone();
        change(&mut elements);
        assert!(elements.place_at(when, p).is_ok(), "{what} is refused");
    }
    // The ellipse's own limits are as they were: its phase is at most 2^63.
    let mut ellipse = good.clone();
    ellipse.ratio = wide(0.5, p);
    assert!(ellipse.place_at(when, p).is_ok());
    ellipse.phase = power(64);
    assert!(matches!(
        ellipse.place_at(when, p),
        Err(KeplerError::Invalid(_))
    ));
}

#[test]
fn a_hyperbola_goes_round_no_turn() {
    let p = at(128);
    let good = hyperbola(0.5, 1000.0, 600.0, p);
    assert!(matches!(
        good.mean_anomaly_in_turns(time(1000), p),
        Err(KeplerError::Invalid(_))
    ));
}

#[test]
fn a_prepared_hyperbola_gives_the_place_of_place_at_whenever_it_is_asked() {
    let p = at(192);
    for elements in cases(p) {
        let prepared = elements.prepare(p).unwrap();
        for units in TIMES.iter().chain(TIMES.iter().rev()) {
            assert_eq!(
                prepared.place_at(time(*units)),
                elements.place_at(time(*units), p),
                "{:?} at {units}",
                elements.ratio
            );
        }
    }
}
