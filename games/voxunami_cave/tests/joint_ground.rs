//! A structure built on the ground of a planet and joined to it by two joint blocks that face each other is made a car by `/joint unlock`, with no `/ship create`, and stays held to the ground by a
//! ball joint (`docs/decisions/joints-and-cars.md`, "A structure on the ground becomes a car with `/joint unlock`").

mod common;

use common::docking::{PLAIN, Placed, a_planet_under_the_patch, blocks_of, ships, stone};
use common::joints::{
    Report, ball_end_toward, joint_block, pairs_the_world_has_as_coupled, until_coupled, world_of,
};
use common::{Player, Scene, say, ship_id, tick_and_follow};
use server::GridView;
use voxel_space::{FaceDirection, GridId, VoxelCoordinate};
use voxunami_cave::content::CaveContent;

/// The top of the ground of the patch is the voxel at this height of the planet (`a_planet_under_the_patch`).
const GROUND: i128 = 63_679;

/// A planet with the ground that it makes, and on it at x 1, z 6 two joint blocks facing each other (the lower one's ball end up, the upper one's down) with three stones on the upper one:
/// the structure. A player flies beside it. The planet is ship 0.
fn a_structure_on_the_ground() -> (Scene, Player, GridId, Report) {
    a_planet_with(vec![
        (
            [1, GROUND + 1, 6],
            joint_block(),
            ball_end_toward(FaceDirection::PositiveY),
        ),
        (
            [1, GROUND + 2, 6],
            joint_block(),
            ball_end_toward(FaceDirection::NegativeY),
        ),
        ([1, GROUND + 3, 6], stone(), PLAIN),
        ([2, GROUND + 3, 6], stone(), PLAIN),
        ([1, GROUND + 3, 7], stone(), PLAIN),
    ])
}

/// The planet, with these blocks built on it (voxels of the planet) and a player flying beside them.
fn a_planet_with(blocks: Vec<Placed>) -> (Scene, Player, GridId, Report) {
    let (mut scene, report) = Scene::reporting();
    let centre = a_planet_under_the_patch(&mut scene);
    let planet = ship_id(0);
    let mut player = scene
        .rig
        .join([3.0, centre + GROUND as f64 + 6.0, 6.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    let content = CaveContent::new();
    let (block_channel, state_channel, orientation_channel) = (
        content.block_channel(),
        content.state_channel(),
        content.orientation_channel(),
    );
    scene.run(move |world| {
        let mut view = world.grid(planet).unwrap();
        for ([x, y, z], block, look) in blocks {
            let at = VoxelCoordinate::new(x, y, z);
            view.set(state_channel, at, look.state).unwrap();
            view.set(orientation_channel, at, look.orientation).unwrap();
            view.set(block_channel, at, block).unwrap();
        }
    });
    tick_and_follow(&mut scene, &mut player, 3);
    (scene, player, planet, Box::new(report))
}

fn block_of_the_planet(scene: &mut Scene, at: [i128; 3]) -> u32 {
    let found = std::sync::Arc::new(std::sync::Mutex::new(u32::MAX));
    let slot = std::sync::Arc::clone(&found);
    let channel = CaveContent::new().block_channel();
    scene.run(move |world| {
        *slot.lock().unwrap() = world
            .grid(ship_id(0))
            .unwrap()
            .get(channel, VoxelCoordinate::new(at[0], at[1], at[2]))
            .unwrap();
    });
    std::mem::take(&mut *found.lock().unwrap())
}

#[test]
fn unlock_cuts_the_structure_that_is_not_ground_out_of_the_planet_as_a_car_and_leaves_the_ground_end()
 {
    let (mut scene, mut player, planet, _report) = a_structure_on_the_ground();
    let reply = say(&mut scene, &mut player, "/joint unlock");
    assert!(reply.contains("unlocked"), "{reply}");
    let cars: Vec<GridId> = ships(&scene)
        .into_iter()
        .filter(|id| *id != planet)
        .collect();
    assert_eq!(cars.len(), 1, "one car, made of the structure: {cars:?}");
    let car = blocks_of(&scene, cars[0]);
    assert_eq!(
        car.len(),
        4,
        "the upper joint and the three stones: {car:?}"
    );
    assert!(car.values().any(|(block, ..)| *block == joint_block()));
    // The planet keeps the lower joint (the ground end) and its ground, and the cells of the car are air.
    assert_eq!(
        block_of_the_planet(&mut scene, [1, GROUND + 1, 6]),
        joint_block()
    );
    for at in [
        [1, GROUND + 2, 6],
        [1, GROUND + 3, 6],
        [2, GROUND + 3, 6],
        [1, GROUND + 3, 7],
    ] {
        assert_eq!(block_of_the_planet(&mut scene, at), 0, "{at:?}");
    }
    assert_ne!(block_of_the_planet(&mut scene, [1, GROUND, 6]), 0);
}

#[test]
fn a_pair_of_joint_ends_with_both_sides_on_the_ground_is_not_unlocked() {
    // Two joints side by side on the ground, facing each other, each with a stone on it that stands on the ground: neither side would leave.
    let (mut scene, mut player, planet, _report) = a_planet_with(vec![
        (
            [1, GROUND + 1, 6],
            joint_block(),
            ball_end_toward(FaceDirection::PositiveX),
        ),
        (
            [2, GROUND + 1, 6],
            joint_block(),
            ball_end_toward(FaceDirection::NegativeX),
        ),
    ]);
    let reply = say(&mut scene, &mut player, "/joint unlock");
    assert!(
        reply.contains("could not unlock") && reply.contains("ground"),
        "{reply}"
    );
    assert_eq!(ships(&scene), vec![planet], "nothing left the planet");
    assert_eq!(
        block_of_the_planet(&mut scene, [1, GROUND + 1, 6]),
        joint_block()
    );
    assert_eq!(
        block_of_the_planet(&mut scene, [2, GROUND + 1, 6]),
        joint_block()
    );
}

#[test]
fn when_neither_side_stands_on_the_ground_the_smaller_one_is_the_car() {
    // A pair over the ground, in the air: five stones on the lower joint and one on the upper.
    let mut blocks = vec![
        (
            [1, GROUND + 11, 6],
            joint_block(),
            ball_end_toward(FaceDirection::PositiveY),
        ),
        (
            [1, GROUND + 12, 6],
            joint_block(),
            ball_end_toward(FaceDirection::NegativeY),
        ),
        ([1, GROUND + 13, 6], stone(), PLAIN),
    ];
    for y in 6..11 {
        blocks.push(([1, GROUND + y, 6], stone(), PLAIN));
    }
    let (mut scene, mut player, planet, _report) = a_planet_with(blocks);
    let reply = say(&mut scene, &mut player, "/joint unlock");
    assert!(reply.contains("unlocked"), "{reply}");
    let cars: Vec<GridId> = ships(&scene)
        .into_iter()
        .filter(|id| *id != planet)
        .collect();
    assert_eq!(cars.len(), 1, "{cars:?}");
    assert_eq!(
        blocks_of(&scene, cars[0]).len(),
        2,
        "the upper joint and its stone (the smaller side)"
    );
    assert_eq!(
        block_of_the_planet(&mut scene, [1, GROUND + 11, 6]),
        joint_block()
    );
    assert_eq!(block_of_the_planet(&mut scene, [1, GROUND + 7, 6]), stone());
}

#[test]
#[ignore = "phase 2 of the ground plan: a planet is not a coupling partner yet"]
fn the_car_is_coupled_to_the_end_in_the_ground_and_stays_joined_to_it_as_it_swings() {
    let (mut scene, mut player, planet, report) = a_structure_on_the_ground();
    say(&mut scene, &mut player, "/joint unlock");
    let car = ships(&scene).into_iter().find(|id| *id != planet).unwrap();
    until_coupled(&mut scene, &mut player, &report, 1, 300);
    assert_eq!(
        pairs_the_world_has_as_coupled(&mut scene),
        [(planet, car)].into_iter().collect()
    );
    // The planet's own pull is on the structure, which stands on its joint: it tips and swings about the ball, and the ball stays where it was joined (a half metre above
    // the ground end, in the middle of the gap between the two tips).
    tick_and_follow(&mut scene, &mut player, 200);
    let pivot_of_the_car = world_of(&scene, car, [0.5, 0.5, 0.5]);
    let pivot_of_the_planet = world_of(&scene, planet, [1.5, GROUND as f64 + 2.0, 6.5]);
    let apart = common::length(common::joints::minus(pivot_of_the_car, pivot_of_the_planet));
    assert!(
        apart < 0.5 + 1.5,
        "the car's block is {apart} voxels from the ground's"
    );
}
