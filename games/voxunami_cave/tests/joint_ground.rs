//! A structure built on the ground of a planet and joined to it by two joint blocks that face each other is made a car by `/joint unlock`, with no `/ship create`, and stays held to the ground by a
//! ball joint (`docs/decisions/joints-and-cars.md`, "A structure on the ground becomes a car with `/joint unlock`").

mod common;

use common::docking::{Look, PLAIN, Placed, a_planet_under_the_patch, blocks_of, ships, stone};
use common::joints::{
    Report, axle_block, axle_end_toward, ball_end_toward, hinge_block, hinge_end_toward,
    joint_block, pairs_the_world_has_as_coupled, turn_of_the_second_in_the_first, until_coupled,
    world_of,
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
    a_structure_joined_to_the_ground_by(
        joint_block(),
        ball_end_toward(FaceDirection::PositiveY),
        ball_end_toward(FaceDirection::NegativeY),
    )
}

/// The same structure, joined to the ground by these two blocks as they are turned (the lower looks up and the upper looks down).
fn a_structure_joined_to_the_ground_by(
    block: u32,
    lower: Look,
    upper: Look,
) -> (Scene, Player, GridId, Report) {
    a_planet_with(vec![
        ([1, GROUND + 1, 6], block, lower),
        ([1, GROUND + 2, 6], block, upper),
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
    assert!(
        reply.contains("unlocked ship 0: ship 1 is cut out of it"),
        "{reply}"
    );
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
fn the_car_is_coupled_to_the_end_in_the_ground_and_stays_joined_to_it_as_it_swings() {
    let (mut scene, mut player, planet, report) = a_structure_on_the_ground();
    say(&mut scene, &mut player, "/joint unlock");
    let car = ships(&scene).into_iter().find(|id| *id != planet).unwrap();
    until_coupled(&mut scene, &mut player, &report, 1, 300);
    assert_eq!(
        pairs_the_world_has_as_coupled(&mut scene),
        [(planet, car)].into_iter().collect()
    );
    // The planet's own pull is on the structure, which stands on its joint: it tips and swings about the ball, and the ball stays where it was joined (the two pivots are one place).
    tick_and_follow(&mut scene, &mut player, 200);
    let pivot_of_the_car = world_of(&scene, car, [0.5, 0.0, 0.5]);
    let pivot_of_the_planet = world_of(&scene, planet, [1.5, GROUND as f64 + 2.0, 6.5]);
    let apart = common::length(common::joints::minus(pivot_of_the_car, pivot_of_the_planet));
    assert!(
        apart < 1e-6,
        "the pivots are {apart} voxels apart: the car is not held to the ground end"
    );
    let upright = scene
        .rig
        .server
        .world()
        .grids()
        .get(car)
        .unwrap()
        .pose
        .rotation_matrix()[1][1];
    assert!(
        upright < 0.9,
        "the car has not tipped over (a ball turns freely): its up is {upright} of the way up"
    );
}

/// A structure on the ground, made a car and coupled to the ground end (the planet is ship 0, the car ship 1).
fn a_car_joined_to_the_ground() -> (Scene, Player, GridId, GridId, Report) {
    let (mut scene, mut player, planet, report) = a_structure_on_the_ground();
    say(&mut scene, &mut player, "/joint unlock");
    let car = ships(&scene).into_iter().find(|id| *id != planet).unwrap();
    until_coupled(&mut scene, &mut player, &report, 1, 300);
    (scene, player, planet, car, report)
}

/// How far up the up of the car looks: 1 when it stands as it was built.
fn uprightness(scene: &Scene, car: GridId) -> f64 {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(car)
        .unwrap()
        .pose
        .rotation_matrix()[1][1]
}

#[test]
fn a_car_that_is_immobilized_on_the_ground_stands_where_it_is_and_one_that_is_mobilized_tips_over()
{
    let (mut scene, mut player, _planet, car, _report) = a_car_joined_to_the_ground();
    let reply = say(&mut scene, &mut player, "/joint immobilize 0");
    assert!(reply.contains("immobilized"), "{reply}");
    let before = uprightness(&scene, car);
    tick_and_follow(&mut scene, &mut player, 200);
    assert!(
        (uprightness(&scene, car) - before).abs() < 1e-3,
        "a rigid joint holds the car: {before} then {}",
        uprightness(&scene, car)
    );
    let reply = say(&mut scene, &mut player, "/joint mobilize");
    assert!(reply.contains("mobilized"), "{reply}");
    tick_and_follow(&mut scene, &mut player, 200);
    assert!(
        uprightness(&scene, car) < 0.9,
        "it tips over when it is let turn"
    );
}

#[test]
fn decoupling_a_car_from_the_ground_lets_it_go_and_it_is_not_coupled_again_while_it_is_held_apart()
{
    let (mut scene, mut player, _planet, car, report) = a_car_joined_to_the_ground();
    let reply = say(&mut scene, &mut player, "/joint decouple");
    assert!(reply.contains("decoupled"), "{reply}");
    tick_and_follow(&mut scene, &mut player, 100);
    assert_eq!(report().couplings, 0);
    assert!(pairs_the_world_has_as_coupled(&mut scene).is_empty());
    // It is free: the pivots go apart as it falls.
    let pivot_of_the_car = world_of(&scene, car, [0.5, 0.0, 0.5]);
    let pivot_of_the_planet = world_of(&scene, ship_id(0), [1.5, GROUND as f64 + 2.0, 6.5]);
    assert!(
        common::length(common::joints::minus(pivot_of_the_car, pivot_of_the_planet)) > 0.05,
        "the car is still where it was joined"
    );
}

#[test]
fn a_car_joined_to_the_ground_is_joined_again_by_the_bodies_after_the_world_is_saved_and_loaded() {
    let (scene, _player, planet, car, _report) = a_car_joined_to_the_ground();
    let (mut scene, report) = scene.restarted_reporting();
    // (A planet comes out of its file in the tick after the world is loaded; the bodies couple the ends that are together and are saved as a pair.)
    scene.ticks(60);
    assert_eq!(report().couplings, 1, "the bodies have the joint again");
    assert_eq!(
        pairs_the_world_has_as_coupled(&mut scene),
        [(planet, car)].into_iter().collect()
    );
    scene.ticks(100);
    let pivot_of_the_car = world_of(&scene, car, [0.5, 0.0, 0.5]);
    let pivot_of_the_planet = world_of(&scene, planet, [1.5, GROUND as f64 + 2.0, 6.5]);
    assert!(
        common::length(common::joints::minus(pivot_of_the_car, pivot_of_the_planet)) < 1e-6,
        "the car is held to the ground end again"
    );
}

#[test]
fn a_car_joined_to_the_ground_is_put_away_when_no_one_is_near_and_is_joined_again_when_someone_comes_back()
 {
    let (mut scene, mut player, planet, car, report) = a_car_joined_to_the_ground();
    let (eyes_x, eyes_y, eyes_z) = (3.0, GROUND as f64 - 63_680.0 + 6.0 + 399.0, 6.0);
    // Ships are kept by their distance from the players (the scenes keep them wherever they are, and the car is put in a file when nobody is within 300 voxels).
    scene
        .rig
        .server
        .set_ship_settings(server::ShipSettings::new(100.0, 300.0).unwrap());
    tick_and_follow(&mut scene, &mut player, 5);
    assert_eq!(report().couplings, 1, "nobody has gone");
    // A stone is built on the planet far from the car, and the player goes there: the planet's box reaches from the car to that stone, so the player is inside it, and what
    // is near the planet is not near the car (a car coupled to the ground is kept by its own distance).
    let stone_block = stone();
    let (block_channel, state_channel, orientation_channel) = (
        CaveContent::new().block_channel(),
        CaveContent::new().state_channel(),
        CaveContent::new().orientation_channel(),
    );
    scene.run(move |world| {
        let mut view = world.grid(planet).unwrap();
        let at = VoxelCoordinate::new(5_000, GROUND, 6);
        view.set(state_channel, at, 0).unwrap();
        view.set(orientation_channel, at, 0).unwrap();
        view.set(block_channel, at, stone_block).unwrap();
    });
    player.send_command(&format!("/tp 5000 {} {eyes_z}", eyes_y + 1.0));
    tick_and_follow(&mut scene, &mut player, 200);
    assert_eq!(
        report().couplings,
        0,
        "the car's joint is gone with its body"
    );
    assert_eq!(
        pairs_the_world_has_as_coupled(&mut scene),
        [(planet, car)].into_iter().collect(),
        "the pair is remembered"
    );
    // Back.
    player.send_command(&format!("/tp {eyes_x} {eyes_y} {eyes_z}"));
    tick_and_follow(&mut scene, &mut player, 300);
    assert_eq!(report().couplings, 1, "joined again");
    let pivot_of_the_car = world_of(&scene, car, [0.5, 0.0, 0.5]);
    let pivot_of_the_planet = world_of(&scene, planet, [1.5, GROUND as f64 + 2.0, 6.5]);
    assert!(
        common::length(common::joints::minus(pivot_of_the_car, pivot_of_the_planet)) < 1e-6,
        "the car is held to the ground end again"
    );
}

#[test]
fn when_the_ground_end_is_dug_out_the_coupling_and_its_ball_are_gone_and_the_car_is_free() {
    let (mut scene, mut player, planet, car, report) = a_car_joined_to_the_ground();
    tick_and_follow(&mut scene, &mut player, 3);
    let balls = |scene: &Scene| {
        scene
            .rig
            .server
            .world()
            .entities()
            .iter()
            .filter(|entity| entity.kind == grid_bodies::joint_ball_kind())
            .count()
    };
    assert_eq!(balls(&scene), 1);
    let content = CaveContent::new();
    let (block_channel, state_channel, orientation_channel) = (
        content.block_channel(),
        content.state_channel(),
        content.orientation_channel(),
    );
    scene.run(move |world| {
        let mut view = world.grid(planet).unwrap();
        let at = VoxelCoordinate::new(1, GROUND + 1, 6);
        view.set(block_channel, at, 0).unwrap();
        view.set(state_channel, at, 0).unwrap();
        view.set(orientation_channel, at, 0).unwrap();
    });
    tick_and_follow(&mut scene, &mut player, 5);
    assert_eq!(report().couplings, 0, "no end, no coupling");
    assert_eq!(balls(&scene), 0, "and no ball");
    assert!(pairs_the_world_has_as_coupled(&mut scene).is_empty());
    let _ = car;
}

#[test]
fn a_ship_made_over_the_ground_end_is_pulled_to_it_and_coupled_and_the_planet_does_not_move() {
    // One joint block on the ground (ball end up), and a car of a joint (ball end down) and a stone, made two voxels over it and weightless in the planet's pull.
    let (mut scene, mut player, planet, report) = a_planet_with(vec![(
        [1, GROUND + 1, 6],
        joint_block(),
        ball_end_toward(FaceDirection::PositiveY),
    )]);
    // (The planet pulls with next to nothing, so that the pull of the ends, which is 10 kN at most, is what moves the car.)
    scene.run(move |world| {
        let mut terrain = world.data().grids().get(planet).unwrap().terrain.unwrap();
        terrain.mass = 1.0;
        world.set_grid_terrain(planet, terrain).unwrap();
    });
    let planet_before = scene.rig.server.world().grids().get(planet).unwrap().pose;
    let centre = 399.0 - 63_680.0;
    common::docking::make(
        &mut scene,
        common::joints::pose(1.0, centre + 63_681.0 + 1.0, 6.0),
        false,
        vec![
            (
                [0, 0, 0],
                joint_block(),
                ball_end_toward(FaceDirection::NegativeY),
            ),
            ([0, 1, 0], stone(), PLAIN),
        ],
    );
    let car = ship_id(1);
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    assert_eq!(
        pairs_the_world_has_as_coupled(&mut scene),
        [(planet, car)].into_iter().collect()
    );
    let planet_after = scene.rig.server.world().grids().get(planet).unwrap().pose;
    assert_eq!(planet_before, planet_after, "the ground did not move");
    tick_and_follow(&mut scene, &mut player, 100);
    let pivot_of_the_car = world_of(&scene, car, [0.5, 0.0, 0.5]);
    let pivot_of_the_planet = world_of(&scene, planet, [1.5, GROUND as f64 + 2.0, 6.5]);
    assert!(
        common::length(common::joints::minus(pivot_of_the_car, pivot_of_the_planet)) < 1e-6,
        "the car is held to the ground end"
    );
}

#[test]
fn lock_writes_the_car_back_into_the_planet_and_unlock_makes_the_same_car_again() {
    let (mut scene, mut player, planet, car, report) = a_car_joined_to_the_ground();
    let car_blocks = blocks_of(&scene, car);
    let reply = say(&mut scene, &mut player, "/joint lock");
    assert!(reply.contains("locking"), "{reply}");
    tick_and_follow(&mut scene, &mut player, 60);
    assert_eq!(report().locks_made, 1);
    assert_eq!(
        ships(&scene),
        vec![planet],
        "the car is part of the planet again"
    );
    assert_eq!(
        block_of_the_planet(&mut scene, [1, GROUND + 2, 6]),
        joint_block()
    );
    for at in [[1, GROUND + 3, 6], [2, GROUND + 3, 6], [1, GROUND + 3, 7]] {
        assert_eq!(block_of_the_planet(&mut scene, at), stone(), "{at:?}");
    }
    // The ground end is still there, and nothing is coupled.
    assert_eq!(
        block_of_the_planet(&mut scene, [1, GROUND + 1, 6]),
        joint_block()
    );
    assert!(pairs_the_world_has_as_coupled(&mut scene).is_empty());
    // And a car again with the unlock: the same blocks.
    let reply = say(&mut scene, &mut player, "/joint unlock");
    assert!(reply.contains("unlocked"), "{reply}");
    let again = ships(&scene)
        .into_iter()
        .find(|id| *id != planet)
        .expect("a car");
    assert_eq!(blocks_of(&scene, again), car_blocks);
}

#[test]
fn the_joint_to_the_ground_has_one_ball_aboard_the_planet_that_is_the_rigid_colour_when_it_is_immobilized()
 {
    let (mut scene, mut player, planet, _car, _report) = a_car_joined_to_the_ground();
    let balls = |scene: &Scene| -> Vec<(GridId, bool)> {
        scene
            .rig
            .server
            .world()
            .entities()
            .iter()
            .filter(|entity| entity.kind == grid_bodies::joint_ball_kind())
            .map(|entity| {
                let rigid = entity
                    .components
                    .get::<grid_bodies::JointRigid>()
                    .expect("the component can be read")
                    .expect("a ball has it")
                    .0;
                (entity.pose.grid, rigid)
            })
            .collect()
    };
    tick_and_follow(&mut scene, &mut player, 3);
    assert_eq!(balls(&scene), vec![(planet, false)]);
    say(&mut scene, &mut player, "/joint immobilize 0");
    tick_and_follow(&mut scene, &mut player, 3);
    assert_eq!(balls(&scene), vec![(planet, true)]);
}

#[test]
fn two_joint_ends_that_face_each_other_on_the_ground_have_a_rigid_ball_before_anything_is_said_or_done()
 {
    let (mut scene, mut player, planet, _report) = a_structure_on_the_ground();
    tick_and_follow(&mut scene, &mut player, 3);
    let balls: Vec<_> = scene
        .rig
        .server
        .world()
        .entities()
        .iter()
        .filter(|entity| entity.kind == grid_bodies::joint_ball_kind())
        .map(|entity| {
            (
                entity.pose.grid,
                entity
                    .components
                    .get::<grid_bodies::JointRigid>()
                    .expect("the component can be read")
                    .expect("a ball has it")
                    .0,
            )
        })
        .collect();
    assert_eq!(balls, vec![(planet, true)]);
}

#[test]
fn two_hinge_ends_that_face_each_other_on_the_ground_have_a_rigid_cylinder_and_no_ball_and_the_car_cut_out_of_them_is_coupled_as_a_hinge()
 {
    let (mut scene, mut player, planet, report) = a_structure_joined_to_the_ground_by(
        hinge_block(),
        hinge_end_toward(FaceDirection::PositiveY, FaceDirection::PositiveX),
        hinge_end_toward(FaceDirection::NegativeY, FaceDirection::PositiveX),
    );
    let drawn = |scene: &Scene, kind: namespaced_id::NamespacedId| -> Vec<(GridId, bool)> {
        scene
            .rig
            .server
            .world()
            .entities()
            .iter()
            .filter(|entity| entity.kind == kind)
            .map(|entity| {
                (
                    entity.pose.grid,
                    entity
                        .components
                        .get::<grid_bodies::JointRigid>()
                        .expect("the component can be read")
                        .expect("it has the mark")
                        .0,
                )
            })
            .collect()
    };
    tick_and_follow(&mut scene, &mut player, 3);
    assert_eq!(
        drawn(&scene, grid_bodies::joint_hinge_kind()),
        vec![(planet, true)]
    );
    assert!(drawn(&scene, grid_bodies::joint_ball_kind()).is_empty());
    // Cut out as a car, it is coupled to the ground end as a hinge, whose cylinder is aboard the planet still, and turns now.
    say(&mut scene, &mut player, "/joint unlock");
    until_coupled(&mut scene, &mut player, &report, 1, 300);
    tick_and_follow(&mut scene, &mut player, 3);
    assert_eq!(report().hinges, 1);
    assert_eq!(
        drawn(&scene, grid_bodies::joint_hinge_kind()),
        vec![(planet, false)]
    );
    assert!(drawn(&scene, grid_bodies::joint_ball_kind()).is_empty());
}

/// How far the car's turn, in the axes of the ground, is from a turn about x (the entries that join x to y or to z, the greatest), and how far it has tipped about x (the sine of the angle).
fn turn_off_the_line_and_tip(scene: &Scene, car: GridId) -> (f64, f64) {
    let turn = turn_of_the_second_in_the_first(scene, ship_id(0), car);
    let off = [(0, 1), (0, 2), (1, 0), (2, 0)]
        .into_iter()
        .map(|(row, column)| turn[row][column].abs())
        .fold(0.0, f64::max);
    (off, turn[2][1].abs())
}

/// The structure joined by two blocks, cut out by `/joint unlock` and left to stand on its joint under the planet's pull for ten seconds: how it turned (`turn_off_the_line_and_tip`).
fn how_the_structure_turns_on_its_joint(block: u32, lower: Look, upper: Look) -> (f64, f64) {
    let (mut scene, mut player, planet, report) =
        a_structure_joined_to_the_ground_by(block, lower, upper);
    say(&mut scene, &mut player, "/joint unlock");
    let car = ships(&scene).into_iter().find(|id| *id != planet).unwrap();
    until_coupled(&mut scene, &mut player, &report, 1, 300);
    tick_and_follow(&mut scene, &mut player, 200);
    turn_off_the_line_and_tip(&scene, car)
}

#[test]
fn a_car_cut_out_of_the_ground_by_two_hinges_tips_about_the_line_of_the_hinges_and_no_other_and_by_two_balls_about_every_axis()
 {
    let (off, tip) = how_the_structure_turns_on_its_joint(
        hinge_block(),
        hinge_end_toward(FaceDirection::PositiveY, FaceDirection::PositiveX),
        hinge_end_toward(FaceDirection::NegativeY, FaceDirection::PositiveX),
    );
    assert!(tip > 0.1, "the structure tipped about the line by {tip}");
    assert!(
        off < 0.02,
        "a hinge on the ground turns about its line only: {off}"
    );
    // The control, which is what makes the test a test: the same structure on balls turns about the other axes as well (the stones are off the ball in x and in z).
    let (ball_off, _) = how_the_structure_turns_on_its_joint(
        joint_block(),
        ball_end_toward(FaceDirection::PositiveY),
        ball_end_toward(FaceDirection::NegativeY),
    );
    assert!(ball_off > 0.05, "{ball_off}");
}

#[test]
fn a_car_cut_out_of_the_ground_by_two_axles_spins_about_the_upright_and_nothing_else() {
    let (mut scene, mut player, planet, report) = a_structure_joined_to_the_ground_by(
        axle_block(),
        axle_end_toward(FaceDirection::PositiveY),
        axle_end_toward(FaceDirection::NegativeY),
    );
    say(&mut scene, &mut player, "/joint unlock");
    let car = ships(&scene).into_iter().find(|id| *id != planet).unwrap();
    until_coupled(&mut scene, &mut player, &report, 1, 300);
    assert_eq!(report().axles, 1);
    scene.run(move |world| {
        world
            .set_grid_velocity(car, [0.0; 3], [0.5, 0.8, 0.5])
            .unwrap();
    });
    tick_and_follow(&mut scene, &mut player, 40);
    let turn = turn_of_the_second_in_the_first(&scene, planet, car);
    let off = [(0, 1), (1, 0), (1, 2), (2, 1)]
        .into_iter()
        .map(|(row, column)| turn[row][column].abs())
        .fold(0.0, f64::max);
    assert!(
        off < 0.02,
        "an axle on the ground turns about its line only: {turn:?}"
    );
    assert!(
        turn[0][2].abs() > 0.1 || turn[0][0] < 0.9,
        "and it does turn: {turn:?}"
    );
}
