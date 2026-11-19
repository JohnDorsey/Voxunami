//! A ship that comes into memory is made a few bricks a tick, not all at once.
//!
//! Found by looking into why the first tick of a world that holds a ship cost so much (`docs/history/build-log.md`, "The first tick of a world that holds a ship"): a brick of a ship
//! costs 1.5 to 7.5 ms to give colliders and a mass, so a ship of 20,480 bricks that came into memory stopped the server for about two and a half minutes, and the terrain colliders
//! under a ship that overlapped the ground, made in the same tick, cost more. A ship that is too big for a tick's budget is *being made*: it is held where it is (it does not move,
//! weigh or collide as a whole) until its bricks are made, then until the terrain it needs is, and then it goes on as it would have. A ship that fits the budget is made whole in the
//! tick it comes, as it always was: the first two tests are what it did before there was a budget, and pass without it.

mod common;

use common::{Scene, make_ship, pose_at, ship_id};
use grid_bodies::{SETTLING_TERRAIN_BRICKS_PER_TICK, SHIP_BRICKS_PER_TICK};
use server::GridView;
use voxel_space::{GridPose, VoxelCoordinate};

/// Where there is no planet near (500,000 voxels up, as `ship_frame_carry.rs` has it).
const HIGH: f64 = 500_000.0;

/// A plate of stone one voxel thick and wide and `bricks` bricks long (a brick has 32 of its voxels), not held, made in one tick.
fn make_a_plate(scene: &mut Scene, y: f64, bricks: i128) {
    make_ship(
        scene,
        pose_at(0.0, y, 0.0),
        false,
        [32 * bricks, 1, 1],
        |_| true,
    );
}

/// What the world has of a ship: where it is (voxels), what it weighs, where its centre of mass is, and how fast it goes.
fn state_of(scene: &Scene, number: u64) -> ([f64; 3], f64, [f64; 3], [f64; 3]) {
    let ship = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(number))
        .expect("the ship is in memory");
    (
        ship.pose.position.in_voxels(),
        ship.mass,
        ship.centre_of_mass,
        ship.linear_velocity,
    )
}

/// Gives the ship a speed along z, in the tick after it was made.
fn let_it_go_along_z(scene: &mut Scene, number: u64, speed: f64) {
    scene.run(move |world| {
        world
            .set_grid_velocity(ship_id(number), [0.0, 0.0, speed], [0.0; 3])
            .unwrap();
    });
}

#[test]
fn a_ship_of_a_few_bricks_is_made_whole_in_the_tick_it_comes_and_weighs_and_goes_at_once() {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    make_a_plate(&mut scene, HIGH, 3);
    assert_eq!(
        report().ship_bricks_made,
        3,
        "all three bricks in the tick the ship came"
    );
    let (at, mass, centre, _) = state_of(&scene, 1);
    assert!(mass > 0.0, "it weighs something at once");
    assert!(
        (centre[0] - 48.0).abs() < 1e-9,
        "its centre of mass is in the middle of the plate: {centre:?}"
    );
    let_it_go_along_z(&mut scene, 1, 3.0);
    scene.ticks(20);
    let (later, ..) = state_of(&scene, 1);
    assert!(
        later[2] - at[2] > 1.0,
        "it went along z at 3 voxels a second: {at:?} to {later:?}"
    );
}

#[test]
fn a_ship_of_a_few_bricks_is_made_the_same_way_beside_a_ship_that_is_made_whole() {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    make_a_plate(&mut scene, HIGH, 2);
    make_ship(
        &mut scene,
        pose_at(0.0, HIGH + 100.0, 0.0),
        false,
        [32, 1, 1],
        |_| true,
    );
    assert_eq!(report().ship_bricks_made, 3);
    assert!(state_of(&scene, 1).1 > 0.0 && state_of(&scene, 2).1 > 0.0);
}

#[test]
fn a_ship_of_more_bricks_than_a_tick_has_budget_for_is_made_a_few_bricks_a_tick() {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let bricks = 3 * SHIP_BRICKS_PER_TICK as i128 + 1;
    make_a_plate(&mut scene, HIGH, bricks);
    let mut made = vec![report().ship_bricks_made];
    for _ in 0..(bricks as usize) {
        scene.ticks(1);
        made.push(report().ship_bricks_made);
    }
    let per_tick = SHIP_BRICKS_PER_TICK as u64;
    let expected: Vec<u64> = (1..=4)
        .map(|tick| (tick * per_tick).min(bricks as u64))
        .chain(std::iter::repeat_n(bricks as u64, bricks as usize - 3))
        .collect();
    assert_eq!(
        made, expected,
        "the bricks made after each tick: {per_tick} a tick, in all {bricks}"
    );
    assert_eq!(report().ship_bricks_made_most_in_a_tick, per_tick);
}

#[test]
fn a_ship_that_is_being_made_stays_where_it_is_and_keeps_the_weight_the_world_had_and_goes_on_when_it_is_made()
 {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let bricks = 3 * SHIP_BRICKS_PER_TICK as i128 + 1;
    make_a_plate(&mut scene, HIGH, bricks);
    let (at, mass_at_first, ..) = state_of(&scene, 1);
    // (Given its speed while it is being made: the world has it, and the bodies must not.)
    let_it_go_along_z(&mut scene, 1, 3.0);
    let mut ticks_held = 0;
    while report().ships_being_made > 0 {
        let (now, mass, _, speed) = state_of(&scene, 1);
        assert_eq!(now, at, "it has not moved in {ticks_held} ticks");
        assert_eq!(mass, mass_at_first, "it has no new weight yet");
        assert_eq!(speed, [0.0, 0.0, 3.0], "the world still has its speed");
        scene.ticks(1);
        ticks_held += 1;
        assert!(ticks_held < 200, "it is made in a few ticks");
    }
    // The first tick made it (4 bricks), the second gave it its speed (8), and the third and fourth made the rest (12, 13).
    assert_eq!(
        ticks_held + 2,
        (bricks as usize).div_ceil(SHIP_BRICKS_PER_TICK),
        "it was held for the ticks its bricks took"
    );
    // It weighs what its blocks weigh and it goes at the speed it was given.
    let (_, mass, centre, _) = state_of(&scene, 1);
    assert!(mass > 0.0);
    assert!((centre[0] - 16.0 * bricks as f64).abs() < 1e-9);
    scene.ticks(20);
    let (later, ..) = state_of(&scene, 1);
    assert!(
        later[2] - at[2] > 1.0,
        "it went along z once it was made: {at:?} to {later:?}"
    );
    assert_eq!(report().ships_being_made, 0);
}

#[test]
fn a_ship_made_over_several_ticks_weighs_what_the_same_blocks_made_whole_weigh() {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    // One brick of the plate, made whole, to say what a brick weighs; and ten of them in a row, made over several ticks.
    make_a_plate(&mut scene, HIGH, 1);
    let (_, one, centre_of_one, _) = state_of(&scene, 1);
    make_ship(
        &mut scene,
        pose_at(0.0, HIGH + 200.0, 0.0),
        false,
        [320, 1, 1],
        |_| true,
    );
    while report().ships_being_made > 0 {
        scene.ticks(1);
    }
    let (_, ten, centre_of_ten, _) = state_of(&scene, 2);
    assert_eq!(ten, 10.0 * one, "ten times as much as one brick of it");
    assert_eq!(centre_of_ten[1..], centre_of_one[1..]);
    assert!((centre_of_ten[0] - 160.0).abs() < 1e-9, "{centre_of_ten:?}");
}

#[test]
fn blocks_put_in_and_taken_out_of_a_ship_while_it_is_being_made_are_in_its_weight_when_it_is_made()
{
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    make_a_plate(&mut scene, HIGH, 1);
    let (_, one, ..) = state_of(&scene, 1);
    let per_voxel = one / 32.0;
    // (Two thick: a block taken out of a plate one thick cuts it in two, and the halves are other ships.)
    make_ship(
        &mut scene,
        pose_at(0.0, HIGH + 200.0, 0.0),
        false,
        [320, 2, 1],
        |_| true,
    );
    let (_, weight_before, ..) = state_of(&scene, 2);
    // Four bricks are made. One block goes from the first (made), one from the last (not yet), and one is put above the first.
    let content = voxunami_cave::content::CaveContent::new();
    scene.run(move |world| {
        let mut plate = world.grid(ship_id(2)).unwrap();
        let channel = content.block_channel();
        plate
            .set(channel, VoxelCoordinate::new(5, 1, 0), content.blocks.air)
            .unwrap();
        plate
            .set(channel, VoxelCoordinate::new(300, 1, 0), content.blocks.air)
            .unwrap();
        plate
            .set(channel, VoxelCoordinate::new(6, 2, 0), content.blocks.stone)
            .unwrap();
    });
    assert!(
        report().ships_being_made > 0,
        "the edit came while it was being made"
    );
    assert_eq!(
        state_of(&scene, 2).1,
        weight_before,
        "an edit does not give a ship that is being made the weight of the bricks made so far"
    );
    // In that tick: the budget (bricks 4 to 7), and the bricks the edit changed that are made or have none to make (the first brick, the one before it in z that has no
    // voxel, and the one before the last in z): three. The last brick is not made yet, and is made once, from what is in it, when its turn comes.
    assert_eq!(
        report().ship_bricks_made_last_tick,
        SHIP_BRICKS_PER_TICK as u64 + 3
    );
    while report().ships_being_made > 0 {
        scene.ticks(1);
    }
    // The one brick of the first plate, and the ten of the second (four in the tick it came, four and the three in the tick of the edit, two in the next).
    assert_eq!(report().ship_bricks_made, 1 + 4 + 7 + 2);
    let (_, mass, centre, _) = state_of(&scene, 2);
    assert_eq!(mass, 639.0 * per_voxel, "640 blocks, two out and one in");
    // Where its middle is, from the blocks: the sum of the places of the blocks over their number.
    let mut sum = [0.0f64; 3];
    let mut put = |x: f64, y: f64| {
        sum[0] += x + 0.5;
        sum[1] += y + 0.5;
        sum[2] += 0.5;
    };
    for x in 0..320 {
        put(f64::from(x), 0.0);
        if x != 5 && x != 300 {
            put(f64::from(x), 1.0);
        }
    }
    put(6.0, 2.0);
    for axis in 0..3 {
        assert!(
            (centre[axis] - sum[axis] / 639.0).abs() < 1e-9,
            "axis {axis} of its centre of mass: {centre:?}"
        );
    }
}

#[test]
fn a_small_ship_that_comes_while_a_big_one_is_being_made_is_made_in_the_tick_it_comes() {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    make_a_plate(&mut scene, HIGH, 5 * SHIP_BRICKS_PER_TICK as i128);
    scene.ticks(1);
    assert_eq!(report().ships_being_made, 1);
    make_ship(
        &mut scene,
        pose_at(0.0, HIGH + 100.0, 0.0),
        false,
        [32, 1, 1],
        |_| true,
    );
    assert!(
        state_of(&scene, 2).1 > 0.0,
        "the small ship weighs something in the tick it came, though the big one is not made"
    );
    assert_eq!(report().ships_being_made, 1, "the big one still is");
}

/// The overworld has ground where a plate at height 0 is in it: the bricks of terrain round the plate are mostly rock with caves in it, which costs 12 ms a brick to make.
fn make_a_plate_in_the_ground(scene: &mut Scene, bricks: i128) {
    make_a_plate(scene, 0.0, bricks);
}

#[test]
fn the_terrain_a_ship_that_is_being_made_needs_is_made_a_few_bricks_a_tick_after_the_ships_own() {
    let (mut scene, report) = Scene::reporting();
    make_a_plate_in_the_ground(&mut scene, 12 * SHIP_BRICKS_PER_TICK as i128);
    assert_eq!(
        report().terrain_bricks_made,
        0,
        "a ship whose own bricks are not made yet asks for no terrain"
    );
    let mut most_while_held = 0;
    let mut ticks = 0;
    while report().ships_being_made > 0 {
        scene.ticks(1);
        most_while_held = most_while_held.max(report().terrain_bricks_made_last_tick);
        ticks += 1;
        assert!(ticks < 2000, "it is made");
    }
    assert!(
        ticks >= 10,
        "it was held for the ticks its own bricks and the terrain took: {ticks}"
    );
    let made = report().terrain_bricks_made;
    assert!(
        made > 100,
        "a plate of {} bricks in the ground needs many bricks of terrain: {made}",
        12 * SHIP_BRICKS_PER_TICK
    );
    // The budget, and one brick that is wanted ahead (which has its own, one a tick). Measured 2026-11-19: 200 bricks in 110 ticks, 2 at the most; made at once they were 200 in one.
    let most = SETTLING_TERRAIN_BRICKS_PER_TICK as u64 + 1;
    assert!(
        most_while_held <= most,
        "{most_while_held} bricks of terrain in a tick of the {ticks} it took"
    );
    assert!(
        report().terrain_bricks_made_most_in_a_tick <= most,
        "{} bricks of terrain in the most of any tick",
        report().terrain_bricks_made_most_in_a_tick
    );
}

#[test]
fn a_ship_of_a_few_bricks_in_the_ground_has_all_the_terrain_it_needs_made_in_the_tick_it_comes() {
    let (mut scene, report) = Scene::reporting();
    make_a_plate_in_the_ground(&mut scene, 3);
    assert_eq!(report().ships_being_made, 0);
    // (A budget for it would be a few bricks. Measured 2026-11-19: the bricks of terrain it needs are 20 or so, made in the tick, as they always were.)
    assert!(
        report().terrain_bricks_made_last_tick > 3 * SETTLING_TERRAIN_BRICKS_PER_TICK as u64,
        "{} bricks of terrain in the tick the ship came",
        report().terrain_bricks_made_last_tick
    );
}

#[test]
fn a_ship_that_is_taken_to_another_frame_while_it_is_being_made_is_made_again_there_and_weighs_what_it_should()
 {
    // (The frames of the solver are 32 km across; a ship taken 45 km leaves its frame, and takes with it only what was made of a ship that is whole.)
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    make_a_plate(&mut scene, HIGH, 1);
    let (_, one, ..) = state_of(&scene, 1);
    make_ship(
        &mut scene,
        pose_at(0.0, HIGH + 200.0, 0.0),
        false,
        [320, 1, 1],
        |_| true,
    );
    scene.run(|world| {
        let pose = world.data().grids().get(ship_id(2)).unwrap().pose;
        world
            .set_grid_pose(
                ship_id(2),
                GridPose::new(
                    pose.position.translated([45_000.0, 0.0, 0.0]),
                    pose.orientation,
                ),
            )
            .unwrap();
    });
    assert_eq!(report().ships_being_made, 1);
    while report().ships_being_made > 0 {
        scene.ticks(1);
    }
    let (_, ten, ..) = state_of(&scene, 2);
    assert_eq!(
        ten,
        10.0 * one,
        "all ten bricks, not the four of the first frame"
    );
    // The one brick, four in the first frame (lost), and all ten in the new one.
    assert_eq!(report().ship_bricks_made, 1 + 4 + 10);
}

#[test]
fn a_ship_that_is_let_go_while_it_is_being_made_falls_when_it_is_made_and_one_that_is_held_does_not()
 {
    // In the overworld, where there is gravity and no planet to be put on a rail round (a ship that is let go in space is put on a rail by the world after a second, and
    // the world moves it, whatever its body is): the ship ids start at 0.
    let (mut scene, report) = Scene::reporting();
    // Two big ships, both held when they came.
    for height in [3000.0, 3100.0] {
        make_ship(
            &mut scene,
            pose_at(0.0, height, 0.0),
            true,
            [32 * 13, 1, 1],
            |_| true,
        );
    }
    // One is let go while they are being made.
    scene.run(|world| world.set_grid_held(ship_id(1), false).unwrap());
    assert_eq!(report().ships_being_made, 2);
    let (held_at, ..) = state_of(&scene, 0);
    let (let_go_at, ..) = state_of(&scene, 1);
    let mut before = let_go_at;
    while report().ships_being_made > 0 {
        before = state_of(&scene, 1).0;
        scene.ticks(1);
    }
    let (in_the_tick_it_was_made, ..) = state_of(&scene, 1);
    assert!(
        in_the_tick_it_was_made[1] < before[1],
        "it began to fall in the tick in which it was made: {before:?} to {in_the_tick_it_was_made:?}"
    );
    scene.ticks(20);
    let (held_later, ..) = state_of(&scene, 0);
    let (let_go_later, ..) = state_of(&scene, 1);
    assert_eq!(held_later, held_at, "the held ship stays where it is");
    assert!(
        let_go_at[1] - let_go_later[1] > 1.0,
        "the ship that was let go fell: {let_go_at:?} to {let_go_later:?}"
    );
}

#[test]
fn a_big_ship_that_comes_with_a_speed_does_not_move_in_the_tick_it_comes() {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let bricks = 3 * SHIP_BRICKS_PER_TICK as i128;
    let content = voxunami_cave::content::CaveContent::new();
    // (Made and given its speed in one tick: the world has the speed from the tick the ship comes, and the bodies must not use it until it is made.)
    scene.run(move |world| {
        let grid = world
            .create_grid(voxel_space::LevelIndex::MAIN, pose_at(0.0, HIGH, 0.0))
            .unwrap();
        let mut view = world.grid(grid).unwrap();
        for x in 0..32 * bricks {
            view.set(
                content.block_channel(),
                VoxelCoordinate::new(x, 0, 0),
                content.blocks.stone,
            )
            .unwrap();
        }
        world
            .set_grid_velocity(grid, [0.0, 0.0, 3.0], [0.0; 3])
            .unwrap();
        // (A ship that was weighed before, as one that comes from a file is.)
        world.set_grid_mass(grid, 1234.0, [1.0, 2.0, 3.0]).unwrap();
    });
    assert_eq!(report().ships_being_made, 1);
    let (at, mass, centre, _) = state_of(&scene, 1);
    assert_eq!(at, [0.0, HIGH, 0.0], "it has not moved in the tick it came");
    assert_eq!(
        (mass, centre),
        (1234.0, [1.0, 2.0, 3.0]),
        "the world keeps the weight it had until the ship is made"
    );
    scene.ticks(1);
    let (later, mass, centre, _) = state_of(&scene, 1);
    assert_eq!(later, at, "it is held until it is made");
    assert_eq!((mass, centre), (1234.0, [1.0, 2.0, 3.0]));
}
