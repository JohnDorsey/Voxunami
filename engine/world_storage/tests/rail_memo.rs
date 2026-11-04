//! What an orbit said of a body at a time, remembered for the tick (`RailMemo`): the server puts the ships on their rails at the start of a tick and again after the logic of it,
//! and an orbit is a pure function of the time, so the second question needs no working out. The memory must never change an answer: what it gives is what the rail gives, to the
//! bit, and it forgets whatever the question depends on that changed.

use voxel_space::GridId;
use world_storage::rail::{Centre, Orbit, Rail, RailMemo, RailTime, Spin};

const NOWHERE: [f64; 3] = [0.0; 3];

fn grid(number: u64) -> GridId {
    GridId::new(GridId::FIRST_ALLOCATED + number)
}

fn time(tick: u64) -> RailTime {
    RailTime::of_tick(tick, 20)
}

fn orbit(radius: f64, period: f64) -> Orbit {
    Orbit::elliptical(
        Centre::Point([100.0, 0.0, -50.0]),
        [0.3, 0.9, -0.2],
        radius,
        0.7,
        1.1,
        period,
        0.4,
    )
    .unwrap()
}

fn rail(radius: f64, period: f64) -> Rail {
    Rail {
        spin: Some(Spin::new([0.0, 1.0, 0.0], 3600.0, 0.5).unwrap()),
        orbit: Some(orbit(radius, period)),
    }
}

#[test]
fn what_is_remembered_is_what_the_rail_says_to_the_bit() {
    let mut memory = RailMemo::default();
    for (radius, period) in [(6.8e6, 5581.0), (3.84e8, -2.36e6), (1.5e11, 3.156e7)] {
        let rail = rail(radius, period);
        for tick in [0, 1, 12_345, 987_654_321] {
            let (centre, velocity) = ([1.0e6, 2.0, -3.0], [4.0, 5.0, 6.0]);
            let expected = rail.state(time(tick), centre, velocity);
            // The first time it is worked out, the second time it is remembered, and both are the rail's answer.
            assert_eq!(
                memory.state(grid(1), &rail, time(tick), centre, velocity),
                expected
            );
            assert_eq!(
                memory.state(grid(1), &rail, time(tick), centre, velocity),
                expected
            );
        }
    }
}

#[test]
fn the_same_question_is_worked_out_once() {
    let mut memory = RailMemo::default();
    let rail = rail(6.8e6, 5581.0);
    memory.state(grid(1), &rail, time(77), NOWHERE, NOWHERE);
    assert_eq!((memory.evaluations(), memory.reused()), (1, 0));
    memory.state(grid(1), &rail, time(77), NOWHERE, NOWHERE);
    assert_eq!((memory.evaluations(), memory.reused()), (1, 1));
    // What the orbit goes round is not part of what the orbit says, so the answer is reused though the centre has moved, and is still right.
    let moved = memory.state(grid(1), &rail, time(77), [9.0, 8.0, 7.0], [1.0, 2.0, 3.0]);
    assert_eq!((memory.evaluations(), memory.reused()), (1, 2));
    assert_eq!(
        moved,
        rail.state(time(77), [9.0, 8.0, 7.0], [1.0, 2.0, 3.0])
    );
}

#[test]
fn another_time_or_another_orbit_or_another_ship_is_worked_out_again() {
    let mut memory = RailMemo::default();
    let rail = rail(6.8e6, 5581.0);
    memory.state(grid(1), &rail, time(77), NOWHERE, NOWHERE);
    // Another time.
    let later = memory.state(grid(1), &rail, time(78), NOWHERE, NOWHERE);
    assert_eq!(memory.evaluations(), 2);
    assert_eq!(later, rail.state(time(78), NOWHERE, NOWHERE));
    // The same time with another orbit on the ship (a rail that was set again).
    let other = Rail {
        orbit: Some(orbit(6.8e6 + 1.0e-6, 5581.0)),
        ..rail.clone()
    };
    let changed = memory.state(grid(1), &other, time(78), NOWHERE, NOWHERE);
    assert_eq!(memory.evaluations(), 3);
    assert_eq!(changed, other.state(time(78), NOWHERE, NOWHERE));
    assert_ne!(changed.offset, later.offset);
    // Another ship with the same rail.
    memory.state(grid(2), &other, time(78), NOWHERE, NOWHERE);
    assert_eq!(memory.evaluations(), 4);
    assert_eq!(memory.remembered(), 2);
}

#[test]
fn the_spin_is_never_remembered_and_is_always_right() {
    // A spin is a few `f64` operations and has no memory; an orbit that is the same with another spin is reused, and the answer has the new spin.
    let mut memory = RailMemo::default();
    let first = rail(6.8e6, 5581.0);
    let second = Rail {
        spin: Some(Spin::new([1.0, 0.0, 0.0], 100.0, 2.0).unwrap()),
        ..first.clone()
    };
    memory.state(grid(1), &first, time(5), NOWHERE, NOWHERE);
    let answer = memory.state(grid(1), &second, time(5), NOWHERE, NOWHERE);
    assert_eq!((memory.evaluations(), memory.reused()), (1, 1));
    assert_eq!(answer, second.state(time(5), NOWHERE, NOWHERE));
    // A rail that only turns has nothing to remember.
    let spinning = Rail {
        spin: first.spin,
        orbit: None,
    };
    let state = memory.state(grid(3), &spinning, time(5), NOWHERE, NOWHERE);
    assert_eq!(state, spinning.state(time(5), NOWHERE, NOWHERE));
    assert_eq!((memory.evaluations(), memory.remembered()), (1, 1));
}

#[test]
fn what_is_not_on_a_rail_any_more_is_forgotten() {
    let mut memory = RailMemo::default();
    for number in 1..=4 {
        memory.state(
            grid(number),
            &rail(6.8e6, 5581.0),
            time(5),
            NOWHERE,
            NOWHERE,
        );
    }
    assert_eq!(memory.remembered(), 4);
    memory.keep_only(|ship| ship == grid(2) || ship == grid(4));
    assert_eq!(memory.remembered(), 2);
    // What was forgotten is worked out again if it is asked for.
    memory.state(grid(1), &rail(6.8e6, 5581.0), time(5), NOWHERE, NOWHERE);
    assert_eq!(memory.evaluations(), 5);
}

/// An orbit as `orbit` makes it with the periapsis moved by `nudge` radians: a different orbit by a hair, which no rounding of the answer would show.
fn orbit_nudged(nudge: f64) -> Orbit {
    Orbit::elliptical(
        Centre::Point([100.0, 0.0, -50.0]),
        [0.3, 0.9, -0.2],
        6.8e6,
        0.7,
        1.1 + nudge,
        5581.0,
        0.4,
    )
    .unwrap()
}

#[test]
fn an_orbit_is_prepared_once_however_many_times_it_is_asked_and_every_answer_is_the_rails() {
    let mut memory = RailMemo::default();
    let rail = rail(6.8e6, 5581.0);
    // Fifty ticks, then a long jump and back: only the time changes, so what does not depend on it is worked out once.
    let ticks: Vec<u64> = (0..50).chain([987_654_321, 5, 987_654_322, 0]).collect();
    for &tick in &ticks {
        let (centre, velocity) = ([1.0e6, 2.0, -3.0], [4.0, 5.0, 6.0]);
        assert_eq!(
            memory.state(grid(1), &rail, time(tick), centre, velocity),
            rail.state(time(tick), centre, velocity),
            "tick {tick}"
        );
    }
    assert_eq!(
        (memory.evaluations(), memory.reused(), memory.preparations()),
        (ticks.len() as u64, 0, 1)
    );
    // The same rail on another ship is a memory of its own.
    memory.state(grid(2), &rail, time(3), NOWHERE, NOWHERE);
    assert_eq!(memory.preparations(), 2);
}

#[test]
fn an_orbit_that_changed_is_prepared_again_and_never_answers_with_the_old_one() {
    let mut memory = RailMemo::default();
    let (before, after) = (orbit_nudged(0.0), orbit_nudged(1.0e-15));
    assert_ne!(before, after);
    let on = |orbit: &Orbit| Rail {
        spin: None,
        orbit: Some(orbit.clone()),
    };
    // Before, after, after at another time, before again: each answer is its own rail's, and each change of orbit prepares again, but a change of time does not.
    for (orbit, tick, preparations) in [
        (&before, 10, 1),
        (&after, 10, 2),
        (&after, 11, 2),
        (&before, 12, 3),
        (&before, 13, 3),
    ] {
        let rail = on(orbit);
        assert_eq!(
            memory.state(grid(1), &rail, time(tick), NOWHERE, NOWHERE),
            rail.state(time(tick), NOWHERE, NOWHERE),
            "tick {tick}"
        );
        assert_eq!(memory.preparations(), preparations, "tick {tick}");
    }
}

#[test]
fn what_was_forgotten_is_prepared_again() {
    let mut memory = RailMemo::default();
    let rail = rail(6.8e6, 5581.0);
    memory.state(grid(1), &rail, time(5), NOWHERE, NOWHERE);
    memory.state(grid(1), &rail, time(6), NOWHERE, NOWHERE);
    assert_eq!(memory.preparations(), 1);
    memory.keep_only(|_| false);
    memory.state(grid(1), &rail, time(7), NOWHERE, NOWHERE);
    assert_eq!(memory.preparations(), 2);
}
