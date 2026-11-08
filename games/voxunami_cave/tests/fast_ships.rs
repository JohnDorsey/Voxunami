//! What a ship that goes at the speed of a rocket costs the server.
//!
//! Found by the owner, who flew the dev rocket up from a planet and, when they got off it, saw it and themselves "float apart at 2 jumps a second": the server's ticks took
//! 700 milliseconds. A ship is given colliders of the terrain it will meet before it meets it, the bricks within a margin of its bounds that grows with its speed; the margin was a cube
//! all round the ship, so the bricks were the cube of its speed: 36,000 at 2 kilometres a second, 800,000 at 6, in every tick. They are the cube only up to a cap now, and past it a tube
//! the way the ship goes (`physics::SweptBricks`). Counted in bricks, not in time, as `docs/decisions` has it: what is simulated must not depend on how fast a machine is.

mod common;

use common::Scene;
use common::rocket::{
    a_ship_with_a_seat_and_four_engines_over_the_planet, sit_and_let_go_going_up, state_of,
};
use common::tick_and_follow;

#[test]
fn a_ship_going_six_kilometres_a_second_is_given_colliders_for_the_bricks_ahead_and_not_for_a_cube_of_them()
 {
    let (scene, report) = Scene::reporting_in_a_new_world();
    let (mut scene, mut player, grid) = a_ship_with_a_seat_and_four_engines_over_the_planet(scene);
    sit_and_let_go_going_up(&mut scene, &mut player, grid, 6000.0);
    // The first ticks make the bricks of the whole way ahead; after that the bricks that come into it as the ship goes.
    tick_and_follow(&mut scene, &mut player, 10);
    let (begun_at, _, _) = state_of(&scene, grid);
    let mut most_in_a_tick = 0;
    for _ in 0..60 {
        tick_and_follow(&mut scene, &mut player, 1);
        most_in_a_tick = most_in_a_tick.max(report().terrain_bricks_made_last_tick);
    }
    let (ended_at, _, _) = state_of(&scene, grid);
    let travelled = common::length([0, 1, 2].map(|axis| ended_at[axis] - begun_at[axis]));
    assert!(
        travelled > 15_000.0,
        "the ship went {travelled} voxels in three seconds, not at 6,000 a second"
    );
    // A tick takes the ship 300 voxels, 9 or 10 bricks, through a tube 9 or 10 bricks across: the most of any tick measured 2,000 bricks that come into it. In a cube of the margin it measured 30,422.
    assert!(
        most_in_a_tick < 3_000,
        "{most_in_a_tick} bricks were given colliders in one tick"
    );
}
