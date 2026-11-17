//! A chain of cars coupled by joints that comes down onto a planet that **goes round on its rail and spins** (the owner, 2026-11-16: planets on rails are the planets of the finished
//! game; step 4c-5-3 of `docs/decisions/joints-and-cars.md` was built and tested over a planet that stands still): the chain is alone and has a frame of its own while it comes down
//! (the world carries it with the planet and its frame goes with the planet's displacement and its own motion, once each: `Carrying` in `grid_bodies`), the ground of the planet comes
//! at it in that frame (a planet that goes round and turns, a kinematic body in the frame), and when it touches the ground it is in the frame of the planet, where it rests as a ship
//! does, on ground that turns under it.
//!
//! The wanderer goes round the home planet in 20,000 seconds (125.7 voxels a second) and turns once in 2,000 about its y axis, and the chain comes down in the column 4,000 voxels
//! from the axis, where the ground goes at about 12 voxels a second.

mod common;

use common::chains::chain_over;
use common::moving_planet::{
    WANDERER, orbit_of_the_home_planet, reporting_scene_with, wanderer_pack,
};
use common::{minus, ship_voxels, top_of_the_ground};
use voxel_space::GridId;

/// The wanderer turns once in this many seconds about its y axis.
const SPIN_PERIOD: f64 = 2_000.0;

/// How far the chain is over the ground when it is made, and how fast it goes at it, in voxels and voxels a second. (It has to be made over the reach of the ground, 10,880 voxels
/// from the middle of the wanderer, and the room the world keeps for it, to be alone and have a frame of its own at all; falling from rest it comes down at about 125 voxels a second,
/// where one that is thrown at the ground at 300 is thrown up from it again, 180 voxels, and tumbles for a long time.)
const HEIGHT: f64 = 1_200.0;
const SPEED_AT_THE_GROUND: f64 = 0.0;

fn length(vector: [f64; 3]) -> f64 {
    (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt()
}

#[test]
fn a_chain_that_comes_down_on_a_planet_that_goes_round_and_spins_has_its_own_frame_on_the_way_and_rests_on_the_ground_after()
 {
    let (mut scene, report) = reporting_scene_with(&wanderer_pack(
        &orbit_of_the_home_planet(Some(SPIN_PERIOD)),
        340,
        8.0,
    ));
    scene.ticks(1);
    let top = top_of_the_ground(&mut scene, WANDERER, 4_000, 0, 8_000..10_100) as f64;
    let ground = [4_000.0, top, 0.0];
    let ground_radius = length(ground);
    let up = ground.map(|part| part / ground_radius);
    let cars = chain_over(
        &mut scene,
        WANDERER,
        [0, 1, 2].map(|axis| ground[axis] + up[axis] * HEIGHT),
        up.map(|part| -SPEED_AT_THE_GROUND * part),
    );
    let number = |car: GridId| car.get() - GridId::FIRST_ALLOCATED;
    let from_the_middle = |scene: &common::Scene, car: GridId| {
        minus(
            ship_voxels(scene, number(car)),
            ship_voxels(scene, WANDERER),
        )
    };
    let spacing = |scene: &common::Scene| {
        length(minus(
            from_the_middle(scene, cars[2]),
            from_the_middle(scene, cars[0]),
        ))
    };
    let spacing_at_the_start = spacing(&scene);
    let mut had_its_own_frame_on_the_way = false;
    let mut heights = Vec::new();
    for _ in 0..700 {
        scene.ticks(1);
        let height = length(from_the_middle(&scene, cars[0])) - ground_radius;
        if height > 100.0 && report().group_frames == 1 {
            had_its_own_frame_on_the_way = true;
        }
        heights.push(height);
    }
    assert!(
        had_its_own_frame_on_the_way,
        "the chain had no frame of its own on the way down"
    );
    assert_eq!(
        report().group_frames,
        0,
        "the chain is in a frame of its own on the ground: it has to be in the frame of the planet there"
    );
    // (Over the sphere of the column it came down over: the hills are 20 high, so on the ground is within 25 voxels of it either way. Ran: -3.5, and it settles by about the 660th tick.)
    let at_the_end = heights[699];
    assert!(
        (-25.0..25.0).contains(&at_the_end),
        "the chain is {at_the_end} voxels over the ground at the end: it did not come down to it, or went through it"
    );
    let a_second_before = heights[679];
    assert!(
        (at_the_end - a_second_before).abs() < 0.5,
        "the chain went from {a_second_before} to {at_the_end} voxels over the ground in the last second: it does not rest"
    );
    let held_together = spacing(&scene);
    assert!(
        (held_together - spacing_at_the_start).abs() < 1.0,
        "the first and the last car were {spacing_at_the_start} voxels apart and are {held_together}: the joints did not hold"
    );
}
