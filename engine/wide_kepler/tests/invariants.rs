//! What every orbit satisfies, checked without a reference: the place and the speed worked out must obey the laws of the orbit they are on (the vis-viva equation, the
//! constancy of the angular momentum, the period), and the answer at one width must be the answer at another to the narrower one. A mistake in a formula that the reference
//! rows share would still show here, because these do not use the formulas of the code.

use proptest::prelude::*;
use wide_float::{Precision, WideFloat};
use wide_kepler::{Elements, Place, Time};

fn at(bits: u64) -> Precision {
    Precision::new(bits).unwrap()
}

fn wide(value: f64, precision: Precision) -> WideFloat {
    WideFloat::from_f64(value, precision).unwrap()
}

/// The elements of an orbit from numbers a program would write: doubles, which are exact in any width.
#[derive(Clone, Debug)]
struct Doubles {
    normal: [f64; 3],
    radius: f64,
    ratio: f64,
    periapsis: f64,
    period: f64,
    phase: f64,
}

impl Doubles {
    fn elements(&self, precision: Precision) -> Elements {
        Elements {
            normal: self.normal.map(|component| wide(component, precision)),
            radius: wide(self.radius, precision),
            ratio: wide(self.ratio, precision),
            periapsis: wide(self.periapsis, precision),
            period: wide(self.period, precision),
            phase: wide(self.phase, precision),
        }
    }
}

fn orbits() -> impl Strategy<Value = Doubles> {
    (
        proptest::array::uniform3(-1.0f64..1.0),
        1.0f64..1.0e15,
        prop_oneof![
            Just(1.0),
            0.001f64..1.0,
            (1u64..40).prop_map(|bits| f64::from_bits((1023 - bits) << 52))
        ],
        -10.0f64..10.0,
        prop_oneof![1.0f64..1.0e3, 1.0e3f64..1.0e8],
        -1.0e3f64..1.0e3,
    )
        .prop_filter("a normal that points somewhere", |(normal, ..)| {
            normal.iter().any(|c| c.abs() > 1.0e-3)
        })
        .prop_map(
            |(normal, radius, ratio, periapsis, period, phase)| Doubles {
                normal,
                radius,
                ratio,
                periapsis,
                period,
                phase,
            },
        )
}

fn time() -> impl Strategy<Value = Time> {
    (0i128..(1 << 50)).prop_map(|units| Time {
        units,
        units_per_second: 20,
    })
}

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

/// Whether `a` and `b` differ by at most `scale x 2^-bits`.
fn agree(a: &WideFloat, b: &WideFloat, scale: &WideFloat, bits: i64) -> bool {
    let bound = scale.abs().times_power_of_two(-bits).unwrap();
    (a - b).abs().compare(&bound).is_le()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(120))]

    /// v^2 = mu (2 / r - 1 / a), with mu = n^2 a^3 and n = 2 pi / period.
    #[test]
    fn the_speed_obeys_the_vis_viva_equation(orbit in orbits(), time in time()) {
        let width = at(192);
        let elements = orbit.elements(width);
        let Place { position, velocity } = elements.place_at(time, width).unwrap();
        let distance = dot(&position, &position).sqrt().unwrap();
        let a = &elements.radius;
        let n = WideFloat::pi(width).times_power_of_two(1).unwrap().divide(&elements.period).unwrap();
        let mu = &(&n.square() * &a.square()) * a;
        let two = WideFloat::from_i64(2, width);
        let expected = &mu * &(&two.divide(&distance).unwrap() - &WideFloat::from_i64(1, width).divide(a).unwrap());
        // (The positions are right to a x 2^-192 and the term in 1 / r magnifies that by a / r, which is up to a / (a ratio).)
        let magnification = a.divide(&distance).unwrap();
        let scale = &expected.abs() * &magnification;
        prop_assert!(agree(&dot(&velocity, &velocity), &expected, &scale, 192 - 24), "{:?} {:?}", orbit, time);
    }

    /// r x v = n a^2 sqrt(1 - e^2) along the normal, the way round the period says.
    #[test]
    fn the_angular_momentum_is_along_the_normal_and_the_same_size_at_every_time(orbit in orbits(), time in time()) {
        let width = at(192);
        let elements = orbit.elements(width);
        let Place { position, velocity } = elements.place_at(time, width).unwrap();
        let momentum = cross(&position, &velocity);
        let size = dot(&momentum, &momentum).sqrt().unwrap();
        let ratio = &elements.ratio;
        let minor = (ratio * &(&WideFloat::from_i64(2, width) - ratio)).sqrt().unwrap();
        let n = WideFloat::pi(width).times_power_of_two(1).unwrap().divide(&elements.period).unwrap();
        let expected = (&(&n * &elements.radius.square()) * &minor).abs();
        let scale = &expected * &WideFloat::from_i64(1 << 8, width);
        prop_assert!(agree(&size, &expected, &scale, 192 - 24 - 8), "size {:?} {:?}", orbit, time);
        // Along the normal: the unit normal dotted with the momentum is its size, with the sign of the period.
        let squares = &(&elements.normal[0].square() + &elements.normal[1].square()) + &elements.normal[2].square();
        let norm = squares.sqrt().unwrap();
        let unit = elements.normal.each_ref().map(|c| c.divide(&norm).unwrap());
        let along = dot(&momentum, &unit);
        let signed = if elements.period.is_negative() { -&expected } else { expected.clone() };
        prop_assert!(agree(&along, &signed, &scale, 192 - 24 - 8), "direction {:?} {:?}", orbit, time);
    }

    /// After a whole period (a whole number of units of the clock) the body is where it was.
    #[test]
    fn a_body_is_where_it_was_a_period_ago(
        orbit in orbits(), seconds in 1u32..2000, start in 0i128..(1 << 40), laps in 1i128..1000,
    ) {
        let width = at(192);
        let mut orbit = orbit;
        orbit.period = f64::from(seconds);
        let elements = orbit.elements(width);
        let units_in_a_period = i128::from(seconds) * 20;
        let before = elements.place_at(Time { units: start, units_per_second: 20 }, width).unwrap();
        let after = elements.place_at(Time { units: start + laps * units_in_a_period, units_per_second: 20 }, width).unwrap();
        let radius = &elements.radius;
        for axis in 0..3 {
            prop_assert!(agree(&before.position[axis], &after.position[axis], radius, 192 - 20), "{:?}", orbit);
        }
        let speed = dot(&before.velocity, &before.velocity).sqrt().unwrap();
        for axis in 0..3 {
            prop_assert!(agree(&before.velocity[axis], &after.velocity[axis], &speed, 192 - 20), "{:?}", orbit);
        }
    }

    /// The answer at 128 bits is the answer at 1,024 to 128 bits (to a few, as the reference rows say), whatever the width the elements were given in.
    #[test]
    fn a_narrow_answer_is_the_wide_answer_to_the_narrow_width(orbit in orbits(), time in time()) {
        let narrow = at(128);
        let wide_width = at(1024);
        let rough = orbit.elements(narrow).place_at(time, narrow).unwrap();
        let fine = orbit.elements(wide_width).place_at(time, wide_width).unwrap();
        let radius = wide(orbit.radius, wide_width);
        for axis in 0..3 {
            prop_assert!(agree(&rough.position[axis], &fine.position[axis], &radius, 128 - 6), "{:?}", orbit);
        }
        let speed = dot(&fine.velocity, &fine.velocity).sqrt().unwrap();
        for axis in 0..3 {
            prop_assert!(agree(&rough.velocity[axis], &fine.velocity[axis], &speed, 128 - 6), "{:?}", orbit);
        }
    }
}

/// A circle is a circle: the body stays one radius from the centre, goes at one radius a turn of the period, and a quarter of the period on it is a quarter of the way round.
#[test]
fn a_circle_is_a_circle() {
    let width = at(256);
    let circle = Doubles {
        normal: [0.0, 0.0, 1.0],
        radius: 7.0e6,
        ratio: 1.0,
        periapsis: 0.0,
        period: 5000.0,
        phase: 0.0,
    };
    let elements = circle.elements(width);
    // The plane has the +x axis (the circle starts toward +x taken into the plane, which is the plane itself here) and +y a quarter turn on.
    let start = elements
        .place_at(
            Time {
                units: 0,
                units_per_second: 20,
            },
            width,
        )
        .unwrap();
    let quarter = elements
        .place_at(
            Time {
                units: 25000,
                units_per_second: 20,
            },
            width,
        )
        .unwrap();
    let radius = &elements.radius;
    assert!(
        agree(&start.position[0], radius, radius, 250),
        "starts toward +x"
    );
    assert!(agree(
        &start.position[1],
        &WideFloat::zero(width),
        radius,
        250
    ));
    assert!(agree(
        &start.position[2],
        &WideFloat::zero(width),
        radius,
        250
    ));
    assert!(
        agree(&quarter.position[0], &WideFloat::zero(width), radius, 248),
        "a quarter on"
    );
    assert!(
        agree(&quarter.position[1], radius, radius, 248),
        "counter-clockwise looking down +z, so toward +y"
    );
    let rate = WideFloat::pi(width)
        .times_power_of_two(1)
        .unwrap()
        .divide(&elements.period)
        .unwrap();
    let speed = &rate * radius;
    assert!(
        agree(&start.velocity[1], &speed, &speed, 248),
        "the speed is the radius a turn"
    );
    assert!(agree(
        &start.velocity[0],
        &WideFloat::zero(width),
        &speed,
        248
    ));
}
