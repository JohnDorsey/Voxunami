//! A ship under power does not turn when it goes from one frame of the solver to the next.
//!
//! The solver keeps ships in frames, places with an origin of their own, and a ship that goes twice as far from the origin of its frame as a ship may be in one (32 km) is made a body
//! again in another. Found by the owner flying the dev rocket up from a planet: at about 95 km (the planet's 64 km and the first 32 km of the flight) it suddenly turned, a ninety
//! degrees or so, and then did again every 10 to 20 seconds. The engines pushed the ship in the tick its new body was made, and Rapier turned the push about the origin of the
//! ship and not about its centre of mass (see `engine/physics/tests/forces.rs`): for that tick it spun at 0.6 radians a second, and the seat took nine ticks to stop it.

mod common;

use common::rocket::{
    a_ship_with_a_seat_and_four_engines_over_the_planet, angle_between, sit_and_let_go_going_up,
    state_of,
};
use common::{Scene, length, tick_and_follow};

#[test]
fn a_ship_pushed_by_its_engines_does_not_turn_when_it_goes_from_one_frame_of_the_solver_to_the_next()
 {
    let (mut scene, mut player, grid) =
        a_ship_with_a_seat_and_four_engines_over_the_planet(Scene::in_a_new_world());
    // Let go, going up at 1,500 voxels a second, with the throttle up.
    sit_and_let_go_going_up(&mut scene, &mut player, grid, 1500.0);

    let (begun_at, mut before, _) = state_of(&scene, grid);
    let (mut most_turned_in_a_tick, mut most_turning) = (0.0f64, 0.0f64);
    // 40 seconds: 60 km and more, past the first frame's limit of 32 km.
    for _ in 0..800 {
        tick_and_follow(&mut scene, &mut player, 1);
        let (_, turn, turning) = state_of(&scene, grid);
        most_turned_in_a_tick = most_turned_in_a_tick.max(angle_between(before, turn));
        most_turning = most_turning.max(length(turning));
        before = turn;
    }
    let (ended_at, _, _) = state_of(&scene, grid);
    let travelled = length([0, 1, 2].map(|axis| ended_at[axis] - begun_at[axis]));
    assert!(
        travelled > 40_000.0,
        "the ship went {travelled} voxels, which is not past the limit of a frame"
    );
    // Straight through the centre of mass the engines turn it at most by the little that the seat and the engines weigh unevenly: under a thousandth of a radian a second (it
    // measured 0.00086), a twentieth of that in a tick. The jolt was 0.62 radians a second and 0.03 radians in a tick.
    assert!(
        most_turning < 0.01,
        "turned at {most_turning} radians a second"
    );
    assert!(
        most_turned_in_a_tick < 0.001,
        "turned {most_turned_in_a_tick} radians in a tick"
    );
}
