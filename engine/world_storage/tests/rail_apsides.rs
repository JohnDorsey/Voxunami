//! Where an orbit is in its turn, for the answer of a ship to a change of the mass of what it goes round (`docs/future-goals.md`, "A planet whose mass changes"; step B2 of
//! `docs/orbits-and-thrust-plan.md`): the mass parameter the elements say they were made with, and which end of the ellipse a body passes in a stretch of time. The ends are
//! found from the mean anomaly in whole numbers of turns, as the place is, so that a stretch of time that starts where another ends passes each end once and never twice.

use std::f64::consts::TAU;

use world_storage::rail::{Centre, Orbit, Rail, RailTime};

const TICKS_A_SECOND: u32 = 20;

fn tick(tick: u64) -> RailTime {
    RailTime::of_tick(tick, TICKS_A_SECOND)
}

fn half_tick(half_ticks: u64) -> RailTime {
    RailTime::of_half_ticks(half_ticks, TICKS_A_SECOND)
}

fn ellipse(ratio: f64, periapsis: f64, period: f64, phase: f64) -> Orbit {
    Orbit::elliptical(
        Centre::Point([0.0; 3]),
        [0.3, 0.8, -0.5],
        7_500.0,
        ratio,
        periapsis,
        period,
        phase,
    )
    .unwrap()
}

/// How far from the centre the body is at a tick, in voxels.
fn distance(orbit: Orbit, at: u64) -> f64 {
    let state = Rail {
        spin: None,
        orbit: Some(orbit),
    }
    .state(tick(at), [0.0; 3], [0.0; 3]);
    let place = state.offset.unwrap().in_voxels();
    (place[0] * place[0] + place[1] * place[1] + place[2] * place[2]).sqrt()
}

#[test]
fn the_mass_parameter_is_the_one_the_period_and_the_axis_say() {
    // mu = n^2 a^3, the law the period of an orbit is worked out with.
    let orbit = ellipse(0.5, 0.3, 154.0, 1.0);
    let rate = TAU / 154.0;
    assert_eq!(
        orbit.mass_parameter(),
        rate * rate * 7_500.0 * 7_500.0 * 7_500.0
    );
    // An orbit made from a state says the mass it was made with, to the rounding of two numbers, either way round.
    for period_sign in [1.0, -1.0] {
        let mu = 6.674e11;
        let period = TAU * (7_500.0f64 * 7_500.0 * 7_500.0 / mu).sqrt() * period_sign;
        let made = ellipse(0.5, 0.3, period, 1.0);
        assert!((made.mass_parameter() / mu - 1.0).abs() < 1e-14);
    }
}

#[test]
fn each_end_of_an_ellipse_is_passed_once_in_a_turn_by_stretches_that_follow_each_other() {
    // An orbit of 154 ticks and a bit (the period is not a whole number of ticks), over three turns, taking stretches of one tick, each starting where the last ended.
    let orbit = ellipse(0.5, 0.7, 7.7, 2.2);
    let ticks = 3 * 154 + 10;
    let mut periapsides = Vec::new();
    let mut apoapsides = Vec::new();
    for at in 1..=ticks {
        let passed = orbit.apsides_passed(tick(at - 1), tick(at));
        if passed.periapsis {
            periapsides.push(at);
        }
        if passed.apoapsis {
            apoapsides.push(at);
        }
    }
    assert_eq!(periapsides.len(), 3, "{periapsides:?}");
    assert_eq!(apoapsides.len(), 3, "{apoapsides:?}");
    for pair in periapsides.windows(2).chain(apoapsides.windows(2)) {
        let apart = pair[1] - pair[0];
        assert!((153..=155).contains(&apart), "{pair:?}");
    }
    // The tick in which the nearest point is passed is next to the tick at which the body is nearest (a body is at the nearest point of a stretch of ticks where it is passed).
    for &at in &periapsides {
        let nearest = (at - 2..=at + 1)
            .min_by(|&a, &b| distance(orbit, a).total_cmp(&distance(orbit, b)))
            .unwrap();
        assert!(nearest == at || nearest == at - 1, "{at} {nearest}");
        assert!(distance(orbit, at) < 3_800.0, "{}", distance(orbit, at));
    }
    for &at in &apoapsides {
        let farthest = (at - 2..=at + 1)
            .max_by(|&a, &b| distance(orbit, a).total_cmp(&distance(orbit, b)))
            .unwrap();
        assert!(farthest == at || farthest == at - 1, "{at} {farthest}");
        assert!(distance(orbit, at) > 11_000.0, "{}", distance(orbit, at));
    }
    // A stretch of no time passes nothing, and one that covers a turn passes both.
    let passed = orbit.apsides_passed(tick(40), tick(40));
    assert!(!passed.periapsis && !passed.apoapsis);
    let passed = orbit.apsides_passed(tick(40), tick(40 + 160));
    assert!(passed.periapsis && passed.apoapsis);
}

#[test]
fn it_is_the_same_the_other_way_round_and_by_half_ticks() {
    let forward = ellipse(0.4, 1.0, 7.7, 0.4);
    let backward = ellipse(0.4, 1.0, -7.7, 0.4);
    let count = |orbit: Orbit| {
        let mut periapsides = 0;
        let mut apoapsides = 0;
        // Stretches of one tick that begin and end half a tick on, as a ship takes them.
        for at in 1..=500u64 {
            let passed = orbit.apsides_passed(half_tick(2 * at - 1), half_tick(2 * at + 1));
            periapsides += u32::from(passed.periapsis);
            apoapsides += u32::from(passed.apoapsis);
        }
        (periapsides, apoapsides)
    };
    // 500 ticks of a period of 154: three turns and a bit, so three ends of each kind, or four.
    for (periapsides, apoapsides) in [count(forward), count(backward)] {
        assert!((3..=4).contains(&periapsides) && (3..=4).contains(&apoapsides));
    }
    // Going the other way the body passes the nearest point on the same ticks it would at the mirror time: a retrograde orbit with the phase reversed.
    let mirror = ellipse(0.4, 1.0, -7.7, -0.4);
    for at in 1..=400u64 {
        let there = forward.apsides_passed(tick(at - 1), tick(at));
        let back = mirror.apsides_passed(tick(at - 1), tick(at));
        assert_eq!(there.periapsis, back.periapsis, "tick {at}");
        assert_eq!(there.apoapsis, back.apoapsis, "tick {at}");
    }
}

#[test]
fn a_circle_has_no_ends() {
    let circle = ellipse(1.0, 0.0, 7.7, 0.0);
    for at in 1..=400u64 {
        let passed = circle.apsides_passed(tick(at - 1), tick(at));
        assert!(!passed.periapsis && !passed.apoapsis);
    }
}
