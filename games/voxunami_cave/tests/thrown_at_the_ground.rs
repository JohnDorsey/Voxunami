//! What is thrown at the ground of a planet comes back down on it, however far it was thrown (found 2026-11-17 by the investigation of the landing bounce: before this, 5 of 9
//! throws came down **through** the ground, `docs/history/build-log.md`, "The bounce, investigated").
//!
//! The ground of a planet has colliders where something is near, and none where nothing is: a thing that was thrown off the ground by a hill, and flew for twenty seconds over
//! the curve of the planet, had the ground it left let go, brick by brick, until the planet's voxel collider had no voxel. Rapier takes a collider with no bounds out of play
//! for good, and the bricks that were made later, where the thing came down, were never met (`engine/physics/tests/emptied_ground.rs` has the cause alone). The ground of a
//! planet that goes round on a rail is a kinematic body, which Rapier checks at the end of every step; one that is fixed was not hit.
//!
//! The scene is that of `group_lands_on_a_planet_on_a_rail.rs`: the wanderer on its rail, a thing made 1,200 voxels over the ground in the column 4,000 voxels from its axis (the
//! ground slopes 23.6 degrees there) and thrown at it at 300 voxels a second. It lands on a hill, is thrown off it (a bar of 6 by 1 by 1 rose 91 voxels, the chain 679) and comes
//! down again, tens of seconds later, a thousand voxels or more from where it hit first.

mod common;

use common::chains::chain_over;
use common::moving_planet::{
    WANDERER, orbit_of_the_home_planet, reporting_scene_with, wanderer_pack,
};
use common::{Scene, minus, ship_over, ship_voxels, top_of_the_ground};
use voxel_space::GridId;

/// How far under the sphere of the column the thing may be: the hills are 20 high, and a valley takes it 30 under the sphere. Through the ground it is hundreds.
const DEEPEST_ON_THE_GROUND: f64 = -60.0;

fn length(vector: [f64; 3]) -> f64 {
    (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt()
}

/// What a thing that is thrown at the ground does: how high it was over the sphere of the column at the lowest, after it had been thrown off the ground once, and
/// how high at the highest (to know that it was thrown off).
struct Flight {
    lowest_height: f64,
    highest_height_after_touching: f64,
}

fn throw_at_the_ground(
    make: impl FnOnce(&mut Scene, [f64; 3], [f64; 3]) -> u64,
    ticks: usize,
) -> Flight {
    let (mut scene, _report) =
        reporting_scene_with(&wanderer_pack(&orbit_of_the_home_planet(None), 340, 8.0));
    scene.ticks(1);
    let top = top_of_the_ground(&mut scene, WANDERER, 4_000, 0, 8_000..10_100) as f64;
    let ground = [4_000.0, top, 0.0];
    let ground_radius = length(ground);
    let up = ground.map(|part| part / ground_radius);
    let offset = [0, 1, 2].map(|axis| ground[axis] + up[axis] * 1_200.0);
    let against = up.map(|part| -300.0 * part);
    let thing = make(&mut scene, offset, against);
    let mut lowest_height = f64::INFINITY;
    let mut highest_height_after_touching: f64 = f64::NEG_INFINITY;
    let mut touched = false;
    for _ in 0..ticks {
        scene.ticks(1);
        let from_the_middle = minus(ship_voxels(&scene, thing), ship_voxels(&scene, WANDERER));
        let height = length(from_the_middle) - ground_radius;
        lowest_height = lowest_height.min(height);
        touched |= height < 5.0;
        if touched {
            highest_height_after_touching = highest_height_after_touching.max(height);
        }
    }
    Flight {
        lowest_height,
        highest_height_after_touching,
    }
}

#[test]
fn a_bar_that_is_thrown_off_the_ground_and_comes_down_again_lands_on_it() {
    let flight = throw_at_the_ground(
        |scene, offset, against| ship_over(scene, WANDERER, offset, false, against, [6, 1, 1]),
        420,
    );
    assert!(
        flight.highest_height_after_touching > 50.0,
        "(the bar was thrown off the ground {} voxels: the scene is not the one that was meant)",
        flight.highest_height_after_touching
    );
    assert!(
        flight.lowest_height > DEEPEST_ON_THE_GROUND,
        "the bar was {} voxels over the sphere of its column at the lowest: it came down through the ground",
        flight.lowest_height
    );
}

#[test]
fn a_chain_that_is_thrown_off_the_ground_and_comes_down_again_lands_on_it() {
    let flight = throw_at_the_ground(
        |scene, offset, against| {
            let cars = chain_over(scene, WANDERER, offset, against);
            cars[0].get() - GridId::FIRST_ALLOCATED
        },
        760,
    );
    assert!(
        flight.highest_height_after_touching > 300.0,
        "(the chain was thrown off the ground {} voxels: the scene is not the one that was meant)",
        flight.highest_height_after_touching
    );
    assert!(
        flight.lowest_height > DEEPEST_ON_THE_GROUND,
        "the first car was {} voxels over the sphere of its column at the lowest: the chain came down through the ground",
        flight.lowest_height
    );
}
