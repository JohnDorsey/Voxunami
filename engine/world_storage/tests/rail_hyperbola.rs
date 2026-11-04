//! A rail whose orbit is a hyperbola (E16 of `docs/orbits-and-thrust-plan.md`): a ship going faster than it would to escape goes round its planet once, on a ratio below nothing, and the rail
//! holds it as it holds an ellipse (the same elements, the same bytes, the same exact offset), where `wide_kepler` places it (E14).

use voxel_space::GridId;
use wide_float::{Precision, WideFloat};
use wide_kepler::{Elements, Time};
use world_storage::rail::{
    Centre, Orbit, Rail, RailError, RailMemo, RailTime, decode_rail, encode_rail,
};

const NOWHERE: [f64; 3] = [0.0; 3];
const TICKS_A_SECOND: u32 = 20;
const HALF_A_STEP: f64 = 1.000_001 / 8_589_934_592.0;

fn at(bits: u64) -> Precision {
    Precision::new(bits).unwrap()
}

fn time(tick: u64) -> RailTime {
    RailTime::of_tick(tick, TICKS_A_SECOND)
}

/// A hyperbola of e = 1 + `excess`, the periapsis along the direction `periapsis` radians from where a circle starts, in the plane z = 0.
fn hyperbola(excess: f64, radius: f64, period: f64, phase: f64) -> Orbit {
    Orbit::hyperbolic(
        Centre::Grid(GridId::new(9)),
        [0.0, 0.0, 1.0],
        radius,
        excess,
        0.3,
        period,
        phase,
    )
    .unwrap()
}

fn rail_of(orbit: Orbit) -> Rail {
    Rail {
        spin: None,
        orbit: Some(orbit),
    }
}

#[test]
fn the_views_of_a_hyperbola_are_those_of_one() {
    let orbit = hyperbola(0.5, 5.0e6, 1234.5, -35_000.0);
    assert!(orbit.is_a_hyperbola());
    assert!(!orbit.is_circle());
    assert_eq!(orbit.ratio(), -0.5);
    assert_eq!(orbit.eccentricity(), 1.5);
    assert_eq!(orbit.nearest_distance(), 2.5e6);
    assert_eq!(orbit.farthest_distance(), f64::INFINITY);
    let rate = std::f64::consts::TAU / 1234.5;
    let mu = rate * rate * 5.0e6 * 5.0e6 * 5.0e6;
    assert!((orbit.mass_parameter() - mu).abs() <= 1.0e-12 * mu);
    // An ellipse is not one, and keeps its views.
    let ellipse = Orbit::elliptical(
        Centre::Point(NOWHERE),
        [0.0, 0.0, 1.0],
        1000.0,
        0.5,
        0.0,
        600.0,
        0.0,
    )
    .unwrap();
    assert!(!ellipse.is_a_hyperbola());
    assert_eq!(
        (ellipse.nearest_distance(), ellipse.farthest_distance()),
        (500.0, 1500.0)
    );
}

#[test]
fn what_is_no_hyperbola_is_refused() {
    let around = Centre::Point(NOWHERE);
    let normal = [0.0, 0.0, 1.0];
    // A parabola, a hyperbola thinner than 2^-64 and one wider than 2^64, and a phase over 2^120.
    for (what, excess, phase) in [
        ("a parabola", 0.0, 0.0),
        ("a ratio of -2^-65", 1.0 / 36893488147419103232.0, 0.0),
        ("a ratio of -2^65", 36893488147419103232.0, 0.0),
        ("a phase of 2^121", 0.5, 2.7e36),
    ] {
        assert!(
            matches!(
                Orbit::hyperbolic(around, normal, 1.0e6, excess, 0.0, 100.0, phase),
                Err(RailError::Invalid(_))
            ),
            "{what} is accepted"
        );
    }
    assert!(
        Orbit::hyperbolic(
            around,
            normal,
            1.0e6,
            1.0 / 18446744073709551616.0,
            0.0,
            100.0,
            1.0e36
        )
        .is_ok()
    );
}

/// The place of the wide evaluator, to the half step of the exact offset.
#[test]
fn a_rail_places_a_hyperbola_where_the_wide_evaluator_does_to_the_step() {
    for (excess, radius, period, phase) in [
        (0.5, 5.0e6, 1234.5, -35_000.0),
        (16.0, 4.4e5, 3_600.0, 2.0),
        (1.0e-6, 1.0e9, 86_400.0, -1.0e5),
        (3.0, 2000.0, 61.0, 0.0),
    ] {
        let orbit = hyperbola(excess, radius, period, phase);
        let rail = rail_of(orbit.clone());
        for tick in [0, 1, 777, 123_456, 98_765_432, 1_000_000_000] {
            let state = rail.state(time(tick), NOWHERE, NOWHERE);
            let offset = state.offset.expect("a hyperbola has an offset");
            let wide = orbit
                .elements
                .place_at(
                    Time {
                        units: i128::from(tick),
                        units_per_second: TICKS_A_SECOND,
                    },
                    at(512),
                )
                .unwrap();
            for axis in 0..3 {
                let rail_voxels = &WideFloat::from_i128(offset.bricks[axis], at(512))
                    .times_power_of_two(5)
                    .unwrap()
                    + &WideFloat::from_f64(offset.rest[axis], at(512)).unwrap();
                let apart = (&rail_voxels - &wide.position[axis]).abs().to_f64();
                assert!(
                    apart <= HALF_A_STEP,
                    "e {}, tick {tick}, axis {axis}: {apart:e} voxels apart",
                    1.0 + excess
                );
            }
            let speed = wide.velocity.each_ref().map(WideFloat::to_f64);
            assert_eq!(state.velocity, speed, "tick {tick}");
        }
    }
}

#[test]
fn a_hyperbola_rail_is_where_the_energy_and_the_angular_momentum_say() {
    // Vis-viva and the angular momentum in f64 from the rail's own offset and speed: v^2 = mu (2 / r + 1 / a), |r x v| = sqrt(mu a (e^2 - 1)).
    let orbit = hyperbola(0.8, 3.0e5, 900.0, -2.0e4);
    let (mu, a) = (orbit.mass_parameter(), orbit.radius());
    let e = orbit.eccentricity();
    let rail = rail_of(orbit);
    for tick in [0, 100_000, 5_000_000, 400_000_000] {
        let state = rail.state(time(tick), NOWHERE, NOWHERE);
        let r = state.offset.unwrap().in_voxels();
        let v = state.velocity;
        let distance = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
        let speed_squared = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
        assert!(
            (speed_squared - mu * (2.0 / distance + 1.0 / a)).abs() <= 1.0e-9 * speed_squared,
            "tick {tick}"
        );
        let h = [
            r[1] * v[2] - r[2] * v[1],
            r[2] * v[0] - r[0] * v[2],
            r[0] * v[1] - r[1] * v[0],
        ];
        let momentum = (h[0] * h[0] + h[1] * h[1] + h[2] * h[2]).sqrt();
        assert!(
            (momentum - (mu * a * (e * e - 1.0)).sqrt()).abs() <= 1.0e-8 * momentum,
            "tick {tick}"
        );
    }
}

#[test]
fn a_hyperbola_comes_back_from_its_bytes_equal_to_the_bit_and_the_bytes_are_those_of_a_second_program()
 {
    let rail = rail_of(hyperbola(0.25, 5.0e6, 1234.5, -35_000.0));
    // The bytes of the second program (`golden_v3.py`, the saved form written from its description): a rail about a ship, a ratio of -0.25 and a periapsis of 0.5, in one orbit
    // of numbers that are all doubles.
    let golden = "0201098001008001010001018001008001010c032d31018001010102a50980010206021711800102030101800101010101";
    let expected: Vec<u8> = (0..golden.len() / 2)
        .map(|i| u8::from_str_radix(&golden[2 * i..2 * i + 2], 16).unwrap())
        .collect();
    let orbit = Orbit::hyperbolic(
        Centre::Grid(GridId::new(9)),
        [0.0, 1.0, 0.0],
        5.0e6,
        0.25,
        0.5,
        1234.5,
        -35_000.0,
    )
    .unwrap();
    assert_eq!(encode_rail(&rail_of(orbit)), expected);
    assert_eq!(decode_rail(&encode_rail(&rail)).unwrap(), rail);
    // Wide numbers: a ratio of 192 bits and a phase of 320 (a long way from time 0, and kept to the bits of its size).
    let mut elements: Elements = hyperbola(0.5, 5.0e6, 1234.5, 0.0).elements;
    elements.ratio = -&WideFloat::from_f64(0.5, at(192))
        .unwrap()
        .divide(&WideFloat::from_i64(7, at(192)))
        .unwrap();
    elements.phase = WideFloat::from_f64(-3.5e4, at(320))
        .unwrap()
        .divide(&WideFloat::from_i64(3, at(320)))
        .unwrap();
    let wide = rail_of(Orbit::from_elements(Centre::Grid(GridId::new(9)), elements).unwrap());
    assert_eq!(decode_rail(&encode_rail(&wide)).unwrap(), wide);
}

#[test]
fn the_periapsis_is_passed_once_and_the_apoapsis_never() {
    // A hyperbola whose mean anomaly is nothing at 100 seconds (tick 2000), with a mean motion of 2 pi / 1000: the phase is -2 pi / 10.
    let phase = -std::f64::consts::TAU / 10.0;
    let forward = hyperbola(0.5, 1.0e6, 1000.0, phase);
    let backward = hyperbola(0.5, 1.0e6, -1000.0, -phase);
    for orbit in [&forward, &backward] {
        let passes = |from: u64, to: u64| orbit.apsides_passed(time(from), time(to));
        assert!(
            passes(1980, 2020).periapsis,
            "a stretch that has the periapsis in it"
        );
        assert!(!passes(1000, 1990).periapsis, "before it");
        assert!(!passes(2010, 9000).periapsis, "after it");
        assert!(passes(0, 400_000).periapsis, "a long one");
        for (from, to) in [(0, 2020), (1980, 400_000), (0, 400_000)] {
            assert!(!passes(from, to).apoapsis, "a hyperbola has no apoapsis");
        }
    }
}

#[test]
fn a_place_too_far_for_the_world_is_none_and_nothing_panics() {
    // A radius of 2^100 voxels and a phase of 2^119 radians: the body is 2^219 voxels out, where there are no bricks to say it in.
    let mut elements = hyperbola(0.5, 1.0, 100.0, 0.0).elements;
    elements.radius = WideFloat::from_i64(1, at(128))
        .times_power_of_two(100)
        .unwrap();
    elements.phase = WideFloat::from_i64(1, at(128))
        .times_power_of_two(119)
        .unwrap();
    let rail = rail_of(Orbit::from_elements(Centre::Grid(GridId::new(9)), elements).unwrap());
    let state = rail.state(time(5), NOWHERE, NOWHERE);
    assert!(state.offset.is_none() && state.position.is_none());
    let mut memory = RailMemo::default();
    let remembered = memory.state(GridId::new(1), &rail, time(5), NOWHERE, NOWHERE);
    assert_eq!(remembered, state);
}

#[test]
fn a_state_going_faster_than_it_would_to_escape_is_a_hyperbola_rail_that_puts_the_body_where_it_was()
 {
    let (place, velocity, mu) = ([10_000.0, 0.0, 0.0], [0.0, 0.0, 12_000.0], 6.674e11);
    let at_time = time(2_000);
    let orbit =
        Orbit::from_place(Centre::Grid(GridId::new(9)), place, velocity, mu, at_time).unwrap();
    assert!(orbit.is_a_hyperbola());
    assert!((orbit.mass_parameter() - mu).abs() <= 1.0e-9 * mu);
    let state = rail_of(orbit).state(at_time, NOWHERE, NOWHERE);
    let found = state.offset.unwrap().in_voxels();
    for axis in 0..3 {
        assert!(
            (found[axis] - place[axis]).abs() <= 1.0e-9,
            "place {axis}: {found:?}"
        );
        assert!(
            (state.velocity[axis] - velocity[axis]).abs() <= 1.0e-12 * 12_000.0,
            "speed {axis}: {:?}",
            state.velocity
        );
    }
    // At exactly the speed of escape (sqrt(2 mu / r) = 1 for r = 2 and mu = 1) the orbit is a parabola, and no orbit.
    assert!(
        Orbit::from_place(
            Centre::Point(NOWHERE),
            [2.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            1.0,
            time(0)
        )
        .is_err()
    );
}

#[test]
fn the_memory_gives_a_hyperbola_rail_what_the_rail_gives_and_prepares_it_once() {
    let rail = rail_of(hyperbola(0.5, 5.0e6, 1234.5, -35_000.0));
    let mut memory = RailMemo::default();
    for tick in [0, 1, 5, 123_456, 1_000_000_000, 2] {
        assert_eq!(
            memory.state(
                GridId::new(1),
                &rail,
                time(tick),
                [1.0e6, 2.0, -3.0],
                [4.0, 5.0, 6.0]
            ),
            rail.state(time(tick), [1.0e6, 2.0, -3.0], [4.0, 5.0, 6.0]),
            "tick {tick}"
        );
    }
    assert_eq!((memory.evaluations(), memory.preparations()), (6, 1));
}

#[test]
fn the_periapsis_is_ahead_of_a_hyperbola_until_it_is_passed_and_always_of_an_ellipse() {
    let phase = -std::f64::consts::TAU / 10.0;
    let forward = hyperbola(0.5, 1.0e6, 1000.0, phase);
    let backward = hyperbola(0.5, 1.0e6, -1000.0, -phase);
    for orbit in [&forward, &backward] {
        assert!(orbit.periapsis_ahead(time(0)));
        assert!(
            orbit.periapsis_ahead(time(1000)),
            "50 seconds in, 100 to go"
        );
        assert!(!orbit.periapsis_ahead(time(3000)), "150 seconds in");
        assert!(!orbit.periapsis_ahead(time(400_000)));
    }
    let ellipse = Orbit::elliptical(
        Centre::Point(NOWHERE),
        [0.0, 0.0, 1.0],
        1000.0,
        0.5,
        0.0,
        600.0,
        0.0,
    )
    .unwrap();
    assert!(ellipse.periapsis_ahead(time(0)) && ellipse.periapsis_ahead(time(999_999)));
}
