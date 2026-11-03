//! The place and the speed of a body on an ellipse in wide numbers: against the truth worked out by other code (`reference/orbits.txt`, from `reference/generate.py`: the
//! definition of `world_storage/tools/exact_ellipse_reference.py` in mpmath, 700 bits more than the width of each case), and against what every orbit must satisfy.

use wide_float::WideFloat;
use wide_kepler::{Elements, KeplerError, Place, Time};

mod common;
use common::{at, largest, loss, rows};

const ROWS: &str = include_str!("reference/orbits.txt");

/// How many bits of the width asked for the answer may lose: a place is within `radius x 2^-(width - LOSS)` and a speed within `speed x 2^-(width - LOSS)`. The reference rows
/// are within the last bit of the width (the test prints the worst); the bound is that and a few bits to spare.
const LOSS: i64 = 4;

#[test]
fn the_answer_loses_at_most_a_few_bits() {
    let mut worst_place = i64::MIN;
    let mut worst_velocity = i64::MIN;
    for row in rows(ROWS) {
        let Place { position, velocity } = row.elements.place_at(row.time, at(row.bits)).unwrap();
        let place_loss = loss(row.bits, &position, &row.place, &row.elements.radius);
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

#[test]
fn what_is_not_an_orbit_is_refused() {
    let p = at(128);
    let one = WideFloat::from_i64(1, p);
    let good = Elements {
        normal: [WideFloat::zero(p), one.clone(), WideFloat::zero(p)],
        radius: WideFloat::from_i64(1000, p),
        ratio: one.clone(),
        periapsis: WideFloat::zero(p),
        period: WideFloat::from_i64(600, p),
        phase: WideFloat::zero(p),
    };
    let time = Time {
        units: 1000,
        units_per_second: 20,
    };
    assert!(good.place_at(time, p).is_ok());
    type Mutation<'a> = Box<dyn Fn(&mut Elements) + 'a>;
    let mutations: Vec<(&str, Mutation)> = vec![
        (
            "no normal",
            Box::new(|e| e.normal = [WideFloat::zero(p), WideFloat::zero(p), WideFloat::zero(p)]),
        ),
        ("no radius", Box::new(|e| e.radius = WideFloat::zero(p))),
        (
            "a negative radius",
            Box::new(|e| e.radius = WideFloat::from_i64(-5, p)),
        ),
        (
            "a radius over 2^121",
            Box::new(|e| e.radius = WideFloat::from_i64(1, p).times_power_of_two(121).unwrap()),
        ),
        (
            "a ratio over one",
            Box::new(|e| e.ratio = WideFloat::from_i64(2, p)),
        ),
        (
            "a ratio of nothing",
            Box::new(|e| e.ratio = WideFloat::zero(p)),
        ),
        (
            "a ratio under 2^-64",
            Box::new(|e| e.ratio = WideFloat::from_i64(1, p).times_power_of_two(-65).unwrap()),
        ),
        ("no period", Box::new(|e| e.period = WideFloat::zero(p))),
        (
            "a period over 2^81",
            Box::new(|e| e.period = WideFloat::from_i64(1, p).times_power_of_two(81).unwrap()),
        ),
        (
            "a period under 2^-20",
            Box::new(|e| e.period = WideFloat::from_i64(1, p).times_power_of_two(-21).unwrap()),
        ),
        (
            "a phase over 2^63",
            Box::new(|e| e.phase = WideFloat::from_i64(1, p).times_power_of_two(64).unwrap()),
        ),
        (
            "a periapsis over 2^63",
            Box::new(|e| e.periapsis = WideFloat::from_i64(-1, p).times_power_of_two(64).unwrap()),
        ),
    ];
    for (what, change) in mutations {
        let mut elements = good.clone();
        change(&mut elements);
        assert!(
            matches!(elements.place_at(time, p), Err(KeplerError::Invalid(_))),
            "{what} is accepted"
        );
    }
    assert!(matches!(
        good.place_at(
            Time {
                units: 5,
                units_per_second: 0
            },
            p
        ),
        Err(KeplerError::Invalid(_))
    ));
    // The clock is under 2^96 units either way.
    let last = Time {
        units: (1 << 96) - 1,
        units_per_second: 20,
    };
    assert!(good.place_at(last, p).is_ok());
    for units in [1i128 << 96, -(1i128 << 96), i128::MAX, i128::MIN] {
        assert!(
            matches!(
                good.place_at(
                    Time {
                        units,
                        units_per_second: 20
                    },
                    p
                ),
                Err(KeplerError::Invalid(_))
            ),
            "{units} units is accepted"
        );
    }
}
