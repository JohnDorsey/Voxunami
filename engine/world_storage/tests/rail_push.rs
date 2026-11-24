//! A push that a rail takes without losing anything (4c-5-4b of `docs/decisions/joints-and-cars.md`): a ship on its rail is pushed a little each tick, and the rail is made again from
//! the place and the speed it gave, with the change of speed added ([`Orbit::with_speed_changed_at`]). The state the new orbit is made from has to be the **exact** state of the old
//! one ([`Orbit::wide_state_at`]), not the one a rail's `state` rounds to an `f64` speed: phase 0 measured that the rounded route loses a long orbit (240 voxels of its far end in a
//! minute at a push of 1e-3, and a push of 1e-9 not at all there), and that the exact route loses nothing that matters against the same run at 512 bits.
//!
//! The run it is compared with is written here from `wide_kepler` alone, so it does not depend on the code under test.

use wide_float::{Precision, WideFloat};
use wide_kepler::{Elements, Place, Time};
use world_storage::rail::{Centre, Orbit, Rail, RailTime};

const TICKS_A_SECOND: u32 = 20;

fn at(bits: u64) -> Precision {
    Precision::new(bits).unwrap()
}

fn number(value: f64, precision: Precision) -> WideFloat {
    WideFloat::from_f64(value, precision).unwrap()
}

/// What an orbit that has its nearest point at `periapsis` and its farthest at `apoapsis` goes at its nearest point: place and velocity, in `x` and `y` (round
/// counter-clockwise looking down `z`), exact to `precision` bits.
fn state_at_the_periapsis(
    mu: f64,
    periapsis: f64,
    apoapsis: f64,
    precision: Precision,
) -> ([WideFloat; 3], [WideFloat; 3]) {
    let wide = |value: f64| number(value, at(512));
    let one = WideFloat::from_i64(1, at(512));
    let two = WideFloat::from_i64(2, at(512));
    let axis = (&wide(periapsis) + &wide(apoapsis))
        .times_power_of_two(-1)
        .unwrap();
    let inverse = &two.divide(&wide(periapsis)).unwrap() - &one.divide(&axis).unwrap();
    let speed = (&wide(mu) * &inverse).sqrt().unwrap();
    let zero = WideFloat::zero(precision);
    (
        [number(periapsis, precision), zero.clone(), zero.clone()],
        [zero.clone(), speed.rounded_to(precision), zero],
    )
}

/// The change of speed of one tick of a push of `push` voxels a second squared, along a direction of the world: the same `f64`s in every run.
fn change_of_speed_in_a_tick(push: f64) -> [f64; 3] {
    let direction = [0.3f64, 0.9, 0.3];
    let length = direction.iter().map(|part| part * part).sum::<f64>().sqrt();
    let step = push / f64::from(TICKS_A_SECOND);
    direction.map(|part| part / length * step)
}

fn time_of(tick: u64) -> Time {
    Time {
        units: i128::from(tick),
        units_per_second: TICKS_A_SECOND,
    }
}

/// The orbit at the start, from the state at the nearest point, with elements of 128 bits.
fn start_orbit(mu: f64, periapsis: f64, apoapsis: f64) -> Orbit {
    let (place, velocity) = state_at_the_periapsis(mu, periapsis, apoapsis, at(128));
    Orbit::from_wide_state(
        Centre::Point([0.0; 3]),
        &place,
        &velocity,
        &number(mu, at(128)),
        RailTime::of_tick(0, TICKS_A_SECOND),
        at(128),
    )
    .unwrap()
}

/// The same state, pushed every tick for `ticks` ticks, worked out at 512 bits with nothing rounded between ticks: what the rail's route is to equal.
fn reference_after(mu: f64, periapsis: f64, apoapsis: f64, push: f64, ticks: u64) -> Elements {
    let precision = at(512);
    let (place, velocity) = state_at_the_periapsis(mu, periapsis, apoapsis, at(128));
    let widen = |state: &[WideFloat; 3]| state.each_ref().map(|part| part.rounded_to(precision));
    let mu = number(mu, precision);
    let mut elements = Elements::from_state(
        &widen(&place),
        &widen(&velocity),
        &mu,
        time_of(0),
        precision,
    )
    .unwrap();
    let change = change_of_speed_in_a_tick(push);
    for tick in 1..=ticks {
        let Place { position, velocity } = elements.place_at(time_of(tick), precision).unwrap();
        let pushed: [WideFloat; 3] =
            [0, 1, 2].map(|axis| &velocity[axis] + &number(change[axis], precision));
        elements = Elements::from_state(&position, &pushed, &mu, time_of(tick), precision).unwrap();
    }
    elements
}

/// The same pushes through the route of `how`, from the orbit at the start.
fn pushed_every_tick(
    start: &Orbit,
    mu: f64,
    push: f64,
    ticks: u64,
    how: impl Fn(&Orbit, RailTime, [f64; 3], f64) -> Orbit,
) -> Orbit {
    let change = change_of_speed_in_a_tick(push);
    let mut orbit = start.clone();
    for tick in 1..=ticks {
        orbit = how(&orbit, RailTime::of_tick(tick, TICKS_A_SECOND), change, mu);
    }
    orbit
}

/// How far from the reference a body is at `tick` and how far its orbit's farthest point is, in voxels, worked out at 512 bits.
fn errors(elements: &Elements, reference: &Elements, tick: u64) -> (f64, f64) {
    let precision = at(512);
    let widened = |elements: &Elements| Elements {
        normal: elements
            .normal
            .each_ref()
            .map(|part| part.rounded_to(precision)),
        radius: elements.radius.rounded_to(precision),
        ratio: elements.ratio.rounded_to(precision),
        periapsis: elements.periapsis.rounded_to(precision),
        period: elements.period.rounded_to(precision),
        phase: elements.phase.rounded_to(precision),
    };
    let place = |elements: &Elements| {
        widened(elements)
            .place_at(time_of(tick), precision)
            .unwrap()
            .position
    };
    let (here, there) = (place(elements), place(reference));
    let body = (0..3)
        .map(|axis| (&here[axis] - &there[axis]).abs().to_f64())
        .fold(0.0, f64::max);
    let farthest = |elements: &Elements| {
        let elements = widened(elements);
        &elements.radius * &(&number(2.0, precision) - &elements.ratio)
    };
    let apoapsis = (&farthest(elements) - &farthest(reference)).abs().to_f64();
    (body, apoapsis)
}

/// The route of the rail's `state` today: the place and the speed rounded, `f64`s, and the orbit made from them (what a push did before the exact state could be read).
fn through_doubles(orbit: &Orbit, time: RailTime, change: [f64; 3], mu: f64) -> Orbit {
    let state = Rail {
        spin: None,
        orbit: Some(orbit.clone()),
    }
    .state(time, [0.0; 3], [0.0; 3]);
    let velocity = [0, 1, 2].map(|axis| state.velocity[axis] + change[axis]);
    Orbit::from_state(
        orbit.around,
        state.offset.expect("an orbit has an offset"),
        velocity,
        mu,
        time,
    )
    .unwrap()
}

fn through_the_exact_state(orbit: &Orbit, time: RailTime, change: [f64; 3], mu: f64) -> Orbit {
    orbit.with_speed_changed_at(time, change, mu).unwrap()
}

const OWNERS_MU: f64 = 4.0e14;
const OWNERS_PERIAPSIS: f64 = 7.0e6;
/// A light year, in voxels: the owner's orbit goes out that far, with a ratio of 1.5e-9.
const OWNERS_APOAPSIS: f64 = 9.46e15;
const TICKS: u64 = 200;

#[test]
fn a_push_every_tick_on_an_orbit_a_light_year_long_loses_the_last_digits_of_the_width_and_no_more()
{
    // A push of 1e-9 voxels a second squared is a million times under the owner's 1 percent of the speed: each tick adds 5e-11 to a speed of 1e4.
    let push = 1.0e-9;
    let start = start_orbit(OWNERS_MU, OWNERS_PERIAPSIS, OWNERS_APOAPSIS);
    let reference = reference_after(OWNERS_MU, OWNERS_PERIAPSIS, OWNERS_APOAPSIS, push, TICKS);
    let orbit = pushed_every_tick(&start, OWNERS_MU, push, TICKS, through_the_exact_state);
    let (body, apoapsis) = errors(&orbit.elements, &reference, TICKS);
    // (Measured 2026-11-19: 1e-12 voxels after 200 ticks here, 7e-12 after 1,200 in phase 0; the bound is a thousand times that.)
    assert!(apoapsis < 1.0e-9, "the far end is {apoapsis:e} voxels off");
    assert!(body < 1.0e-9, "the body is {body:e} voxels off");
}

#[test]
fn a_push_every_tick_on_an_orbit_a_light_year_long_through_doubles_loses_the_push() {
    // Why the exact state is read: the same run through the place and the speed of `Rail::state` is off by far more than the push made.
    let push = 1.0e-9;
    let start = start_orbit(OWNERS_MU, OWNERS_PERIAPSIS, OWNERS_APOAPSIS);
    let reference = reference_after(OWNERS_MU, OWNERS_PERIAPSIS, OWNERS_APOAPSIS, push, TICKS);
    let orbit = pushed_every_tick(&start, OWNERS_MU, push, TICKS, through_doubles);
    let (_, apoapsis) = errors(&orbit.elements, &reference, TICKS);
    assert!(
        apoapsis > 1.0e3,
        "the far end is only {apoapsis:e} voxels off: the rounding no longer loses the push, so the exact state need not be read"
    );
}

#[test]
fn a_push_every_tick_on_a_near_circular_orbit_keeps_the_body_where_the_wide_run_has_it() {
    let mu = 761.0 * 761.0 * 70_016.0;
    let (periapsis, apoapsis) = (70_016.0, 70_016.0 * 1.002);
    let push = 0.5;
    let start = start_orbit(mu, periapsis, apoapsis);
    let reference = reference_after(mu, periapsis, apoapsis, push, TICKS);
    let orbit = pushed_every_tick(&start, mu, push, TICKS, through_the_exact_state);
    let (body, far_end) = errors(&orbit.elements, &reference, TICKS);
    assert!(body < 1.0e-24, "the body is {body:e} voxels off");
    assert!(far_end < 1.0e-24, "the far end is {far_end:e} voxels off");
}

#[test]
fn the_wide_state_is_the_state_the_orbit_was_made_from_at_the_time_it_was_made() {
    let mu = 761.0 * 761.0 * 70_016.0;
    let (place, velocity) = state_at_the_periapsis(mu, 70_016.0, 70_016.0 * 20.0, at(128));
    let time = RailTime::of_tick(37, TICKS_A_SECOND);
    let orbit = Orbit::from_wide_state(
        Centre::Point([0.0; 3]),
        &place,
        &velocity,
        &number(mu, at(128)),
        time,
        at(128),
    )
    .unwrap();
    let state = orbit.wide_state_at(time);
    for axis in 0..3 {
        // (128 bits of the elements and a place worked out in more: the state comes back to a part in 2^100.)
        let place_error = (&state.position[axis] - &place[axis]).abs().to_f64();
        let velocity_error = (&state.velocity[axis] - &velocity[axis]).abs().to_f64();
        assert!(
            place_error < 1.0e-24 * 70_016.0 * 20.0,
            "place {axis}: {place_error:e}"
        );
        assert!(
            velocity_error < 1.0e-24 * 1_000.0,
            "velocity {axis}: {velocity_error:e}"
        );
    }
}

#[test]
fn the_wide_state_rounds_to_what_the_rail_says() {
    let mu = 761.0 * 761.0 * 70_016.0;
    let orbit = start_orbit(mu, 70_016.0, 70_016.0 * 20.0);
    let rail = Rail {
        spin: None,
        orbit: Some(orbit.clone()),
    };
    for tick in [0, 1, 99, 4_000] {
        let time = RailTime::of_tick(tick, TICKS_A_SECOND);
        let wide = orbit.wide_state_at(time);
        let said = rail.state(time, [0.0; 3], [0.0; 3]);
        let position = said.position.expect("an orbit has a position");
        let speed = wide.velocity.each_ref().map(WideFloat::to_f64);
        assert_eq!(said.velocity, speed, "tick {tick}");
        let place = wide.position.each_ref().map(WideFloat::to_f64);
        for (axis, (said, wide)) in position.iter().zip(place).enumerate() {
            assert!(
                (said - wide).abs() <= 1.0e-9,
                "tick {tick} axis {axis}: {said} against {wide}"
            );
        }
    }
}

#[test]
fn a_push_makes_an_orbit_that_starts_at_the_time_of_the_push_with_no_hand_overs() {
    let mu = 761.0 * 761.0 * 70_016.0;
    let orbit = start_orbit(mu, 70_016.0, 70_016.0 * 20.0);
    let time = RailTime::of_tick(10, TICKS_A_SECOND);
    let pushed = orbit
        .with_speed_changed_at(time, [0.0, 0.0, 0.01], mu)
        .unwrap();
    assert_eq!(pushed.since, Some(time));
    assert_eq!(pushed.changes, 0);
    assert_eq!(pushed.around, orbit.around);
}

#[test]
fn a_push_of_nothing_leaves_the_body_where_it_was_and_going_as_it_was() {
    let mu = 761.0 * 761.0 * 70_016.0;
    let orbit = start_orbit(mu, 70_016.0, 70_016.0 * 20.0);
    let time = RailTime::of_tick(10, TICKS_A_SECOND);
    let pushed = orbit.with_speed_changed_at(time, [0.0; 3], mu).unwrap();
    for later in [10, 11, 500] {
        let later = RailTime::of_tick(later, TICKS_A_SECOND);
        let (before, after) = (orbit.wide_state_at(later), pushed.wide_state_at(later));
        for axis in 0..3 {
            let scale = before.position[axis].abs().to_f64().max(1.0);
            let error = (&before.position[axis] - &after.position[axis])
                .abs()
                .to_f64();
            assert!(error < 1.0e-20 * scale, "axis {axis}: {error:e}");
        }
    }
}
