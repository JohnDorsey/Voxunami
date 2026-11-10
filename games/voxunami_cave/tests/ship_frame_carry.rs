//! A ship that goes from one frame of the solver to the next takes its colliders and its mass with it, and does not have every brick of it looked at again.
//!
//! The solver keeps ships in frames, and a ship that has gone twice as far from the origin of its frame as a ship may be in one (32 km) is made a body again in another. Making it
//! again looked at the 32,768 voxels of each of its bricks twice (once for the mass and the ports, once for the colliders): 3 ms for a ship of 900 blocks, 48 ms for one of 100,000, in
//! the tick it crossed (`docs/history/build-log.md`, "Joints, step 4c-0"). What is carried must not change what the ship does, so the first test is a landing after the crossing, whose bits
//! were recorded with the colliders made again and are held now that they are carried; the others count bricks.

mod common;

use common::{Scene, make_ship, pose_at, ship_id};
use voxel_space::{GridId, GridPose};

/// How far the ships are taken, in voxels along x: past the 32 km (1,000 bricks, 32,000 voxels) at which a ship leaves its frame.
const FAR: f64 = 45_000.0;

/// A floor held in space and a block that is let fall on it, 4 voxels above, going down at 3 voxels a second, across a little and turning a little, in a place with no planet
/// near (500,000 voxels up). Their numbers are 1 and 2.
fn a_block_over_a_floor() -> (
    Scene,
    impl Fn() -> grid_bodies::BodiesReport,
    GridId,
    GridId,
) {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    make_ship(
        &mut scene,
        pose_at(0.0, 500_000.0, 0.0),
        true,
        [12, 1, 12],
        |_| true,
    );
    make_ship(
        &mut scene,
        pose_at(4.0, 500_004.0, 4.0),
        false,
        [3, 3, 3],
        |_| true,
    );
    let (floor, block) = (ship_id(1), ship_id(2));
    scene.run(move |world| {
        world
            .set_grid_velocity(block, [0.5, -3.0, 0.2], [0.1, 0.05, 0.02])
            .unwrap();
    });
    (scene, report, floor, block)
}

/// Takes both ships `FAR` along x in one tick, so that they leave their frame together and are made bodies again in the same new one, with the same distance between them.
fn take_both_far(scene: &mut Scene, floor: GridId, block: GridId) {
    scene.run(move |world| {
        for ship in [floor, block] {
            let pose = world.data().grids().get(ship).unwrap().pose;
            world
                .set_grid_pose(
                    ship,
                    GridPose::new(pose.position.translated([FAR, 0.0, 0.0]), pose.orientation),
                )
                .unwrap();
        }
    });
}

/// The bits of what the block did, as the world has it: where it is (the brick and the offset in it), how it faces, and how fast it moves and turns.
fn bits_of(scene: &Scene, ship: GridId) -> Vec<u64> {
    let ship = scene.rig.server.world().grids().get(ship).unwrap();
    let brick = ship.pose.position.brick();
    let offset = ship.pose.position.offset();
    let facing = ship.pose.orientation.components();
    let mut bits = vec![brick.x() as u64, brick.y() as u64, brick.z() as u64];
    bits.extend(offset.map(f64::to_bits));
    bits.extend(facing.map(f64::to_bits));
    bits.extend(ship.linear_velocity.map(f64::to_bits));
    bits.extend(ship.angular_velocity.map(f64::to_bits));
    bits
}

#[test]
fn a_block_let_fall_on_a_floor_lands_as_it_always_did_when_both_go_to_another_frame_in_the_air() {
    let (mut scene, _, floor, block) = a_block_over_a_floor();
    scene.ticks(3);
    take_both_far(&mut scene, floor, block);
    scene.ticks(120);
    // It came down on the floor and rests on it: the floor is one voxel thick from 500,000, so the block's underside is at 500,001.
    let landed = scene.rig.server.world().grids().get(block).unwrap();
    let height = landed.pose.position.offset()[1] + landed.pose.position.brick().y() as f64 * 32.0
        - 500_000.0;
    assert!(
        (height - 1.0).abs() < 0.05,
        "the block rests on the floor, at {height} over the floor's underside"
    );
    // The numbers of the block, recorded on 2026-11-10 with the colliders of both ships made again from their voxels in the new frame (the brick, the offset in it, the way it faces,
    // its speed and its turn), and held since the colliders are carried: the brick is 1,406 along x and 15,625 up (500,000 voxels), and the block lies on the floor, a hair over a voxel up.
    let expected_block: [u64; 16] = [
        0x57e,
        0x3d09,
        0x0,
        0x402b52a2b40fd5d6,
        0x3ff0000832c96302,
        0x4000d909980d043e,
        0xbfe08c273f1f3619,
        0x3fb9851fc290956c,
        0x3fb25ceaeb61fa3b,
        0x3feb1b2411de9de4,
        0x3fbd62088e729b80,
        0xbfd15a2372cebd57,
        0xbfea725f92198106,
        0xbfce41c3d38cc7ce,
        0x3fa7a01db3e06551,
        0xbf9e8cef3cbc011c,
    ];
    assert_eq!(bits_of(&scene, block), expected_block);
    // The floor was held and did not move.
    let floor_at_rest: [u64; 16] = [
        0x57e,
        0x3d09,
        0x0,
        0x4020000000000000,
        0,
        0,
        0,
        0,
        0,
        0x3ff0000000000000,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    assert_eq!(bits_of(&scene, floor), floor_at_rest);
}

#[test]
fn ships_that_go_to_another_frame_take_their_bricks_with_them_and_have_none_made_again() {
    let (mut scene, report, floor, block) = a_block_over_a_floor();
    scene.ticks(3);
    let before = report();
    assert_eq!(
        before.ship_bricks_made, 2,
        "a brick each, made when the bodies were made"
    );
    assert_eq!(before.ship_bricks_carried, 0);
    take_both_far(&mut scene, floor, block);
    scene.ticks(3);
    let after = report();
    assert_eq!(
        after.ship_bricks_made, before.ship_bricks_made,
        "no brick was looked at again"
    );
    assert_eq!(
        after.ship_bricks_carried, 2,
        "the two bricks went to the new frame"
    );
}

/// A block put on a ship in the tick it crosses: the brick it is in is made again from the voxels, the others are carried, and the mass and the centre of mass are those of a ship
/// that was built with the block and never moved.
#[test]
fn a_block_put_on_a_ship_in_the_tick_it_crosses_is_seen_and_the_other_bricks_are_carried() {
    use server::GridView;
    use voxel_space::VoxelCoordinate;
    use voxunami_cave::content::CaveContent;

    let (mut scene, report) = Scene::reporting_in_a_new_world();
    // A ship of two bricks (40 along x), and a ship built with the extra block that stays where it is.
    make_ship(
        &mut scene,
        pose_at(0.0, 500_000.0, 0.0),
        false,
        [40, 1, 3],
        |_| true,
    );
    make_ship(
        &mut scene,
        pose_at(0.0, 600_000.0, 0.0),
        false,
        [40, 2, 3],
        |[x, y, z]| y == 0 || (x == 35 && y == 1 && z == 1),
    );
    let (moved, control) = (ship_id(1), ship_id(2));
    scene.ticks(3);
    let before = report();
    assert_eq!(before.ship_bricks_made, 4, "two bricks each");
    let content = CaveContent::new();
    scene.run(move |world| {
        let pose = world.data().grids().get(moved).unwrap().pose;
        world
            .set_grid_pose(
                moved,
                GridPose::new(pose.position.translated([FAR, 0.0, 0.0]), pose.orientation),
            )
            .unwrap();
        world
            .grid(moved)
            .unwrap()
            .set(
                content.block_channel(),
                VoxelCoordinate::new(35, 1, 1),
                content.blocks.stone,
            )
            .unwrap();
    });
    scene.ticks(3);
    let after = report();
    assert_eq!(
        after.ship_bricks_made - before.ship_bricks_made,
        1,
        "only the brick the block was put in was looked at again"
    );
    assert_eq!(
        after.ship_bricks_carried, 2,
        "both its bricks went to the new frame"
    );
    let world = scene.rig.server.world();
    let (moved, control) = (
        world.grids().get(moved).unwrap(),
        world.grids().get(control).unwrap(),
    );
    assert!(moved.mass > 0.0, "the moved ship has a mass");
    assert_eq!(moved.mass.to_bits(), control.mass.to_bits());
    assert_eq!(
        moved.centre_of_mass.map(f64::to_bits),
        control.centre_of_mass.map(f64::to_bits),
        "and its centre of mass is the one of a ship built with the block"
    );
}
