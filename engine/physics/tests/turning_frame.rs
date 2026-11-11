//! A character in a frame that turns (a ship that spins): what it goes at, in the axes of the frame, turns the other way as the frame does, which is the Coriolis pull. It must be
//! an exact turn of the speed and not a push at right angles to it: a push (an Euler step) of `2 w dt` across a speed grows the speed by `sqrt(1 + (2 w dt)^2)` each step, which at
//! one radian a second and sixty steps a second is 1.4 times in ten seconds.

mod common;

use common::{position, spawn};
use physics::CharacterBody;

fn floating(velocity: [f64; 3]) -> CharacterBody {
    let mut body = spawn(5.0, 100.0, 5.0);
    body.reframe(position(5.0, 100.0, 5.0), velocity, [0.0, 0.0, 0.0, 1.0]);
    body
}

fn length(vector: [f64; 3]) -> f64 {
    (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt()
}

fn close(actual: [f64; 3], expected: [f64; 3], tolerance: f64, what: &str) {
    for axis in 0..3 {
        assert!(
            (actual[axis] - expected[axis]).abs() <= tolerance,
            "{what}: {actual:?} against {expected:?}"
        );
    }
}

const QUARTER: f64 = std::f64::consts::FRAC_PI_2;

#[test]
fn no_turn_leaves_the_velocity_alone_to_the_bit() {
    let mut body = floating([1.5, -2.0, 3.25]);
    body.turn_velocity([0.0; 3]);
    assert_eq!(body.velocity(), [1.5, -2.0, 3.25]);
}

#[test]
fn a_quarter_turn_about_y_takes_x_to_minus_z_and_z_to_x() {
    // (A turn about +y by a quarter, the right-handed way: what was along +x is along -z.)
    let mut body = floating([1.0, 0.0, 0.0]);
    body.turn_velocity([0.0, QUARTER, 0.0]);
    close(body.velocity(), [0.0, 0.0, -1.0], 1e-15, "x");
    let mut body = floating([0.0, 0.0, 1.0]);
    body.turn_velocity([0.0, QUARTER, 0.0]);
    close(body.velocity(), [1.0, 0.0, 0.0], 1e-15, "z");
}

#[test]
fn what_is_along_the_axis_does_not_turn() {
    let mut body = floating([0.0, 7.5, 0.0]);
    body.turn_velocity([0.0, 1.3, 0.0]);
    close(body.velocity(), [0.0, 7.5, 0.0], 1e-15, "along y");
    // Any axis: the part of the velocity along it is kept.
    let axis = [2.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0];
    let mut body = floating([3.0 * axis[0], 3.0 * axis[1], 3.0 * axis[2]]);
    body.turn_velocity([0.9 * axis[0], 0.9 * axis[1], 0.9 * axis[2]]);
    close(
        body.velocity(),
        [2.0, 1.0, 2.0],
        1e-14,
        "along a slanted axis",
    );
}

#[test]
fn the_speed_is_kept_whatever_the_turn() {
    for (velocity, turn) in [
        ([3.0, -4.0, 12.0], [0.01, 0.0, 0.0]),
        ([300.0, 5.0, -2.0], [0.0, 0.0, 0.017]),
        ([1.0, 2.0, 3.0], [0.3, -0.2, 0.5]),
        ([-7.0, 0.1, 9.0], [2.0, 3.0, -1.0]),
    ] {
        let mut body = floating(velocity);
        body.turn_velocity(turn);
        let (before, after) = (length(velocity), length(body.velocity()));
        assert!(
            (after - before).abs() <= 1e-14 * before,
            "{velocity:?} turned by {turn:?}: {before} then {after}"
        );
    }
}

#[test]
fn many_small_turns_are_one_big_one_and_the_speed_does_not_grow() {
    // Ten seconds at a radian a second, in sixtieths: six hundred turns of 1/60 of a radian about z.
    let mut body = floating([5.0, 0.0, 0.0]);
    for _ in 0..600 {
        body.turn_velocity([0.0, 0.0, 1.0 / 60.0]);
    }
    let mut once = floating([5.0, 0.0, 0.0]);
    once.turn_velocity([0.0, 0.0, 10.0]);
    close(body.velocity(), once.velocity(), 1e-11, "600 turns and one");
    // (A push of 2 w dt across the speed each step would have made it 1.4 times as fast.)
    assert!(
        (length(body.velocity()) - 5.0).abs() < 1e-11,
        "{:?}",
        body.velocity()
    );
}

#[test]
fn a_turn_and_its_opposite_give_the_velocity_back() {
    let mut body = floating([1.0, -2.0, 3.0]);
    body.turn_velocity([0.4, 0.1, -0.3]);
    body.turn_velocity([-0.4, -0.1, 0.3]);
    close(body.velocity(), [1.0, -2.0, 3.0], 1e-14, "there and back");
}

#[test]
fn the_turn_of_the_velocity_moves_the_character_nowhere() {
    let mut body = floating([1.0, 2.0, 3.0]);
    let before = body.eye_position();
    body.turn_velocity([0.5, 0.5, 0.5]);
    assert_eq!(body.eye_position(), before);
}
