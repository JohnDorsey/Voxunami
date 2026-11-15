//! The hull of a ship: the box that its blocks fill, worked out where the bodies weigh the ship and kept on it (step 4c-5-1 of `docs/decisions/joints-and-cars.md`). Nothing reads it yet:
//! this is the measure that a room for a hit to come is made of, and what it must be is the box that a look at every voxel of the ship finds.

mod common;

use std::sync::{Arc, Mutex};

use common::{Scene, make_ship, pose_at, ship_id};
use server::GridView;
use voxel_space::{GridPose, VoxelCoordinate};
use voxunami_cave::content::CaveContent;
use world_storage::HullBox;

/// The number of the ship that a test makes: a new world has its home planet as ship 0.
const SHIP: u64 = 1;

/// What the server has as the hull of ship `number` (counting from 0 in a world with no ships before). This takes a tick.
fn hull_of(scene: &mut Scene, number: u64) -> Option<HullBox> {
    let found = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&found);
    scene.run(move |world| {
        let ship = world
            .data()
            .grids()
            .get(ship_id(number))
            .expect("the ship is there");
        *slot.lock().unwrap() = Some(ship.hull);
    });
    found.lock().unwrap().expect("the tick ran the action")
}

/// The box of the voxels of `size` that `solid` says are stone, by a look at every one of them.
fn box_of(size: [i128; 3], solid: impl Fn([i128; 3]) -> bool) -> Option<HullBox> {
    let mut found: Option<HullBox> = None;
    for x in 0..size[0] {
        for y in 0..size[1] {
            for z in 0..size[2] {
                if solid([x, y, z]) {
                    let voxel = VoxelCoordinate::new(x, y, z);
                    found =
                        Some(found.map_or(HullBox::of_voxel(voxel), |hull| hull.including(voxel)));
                }
            }
        }
    }
    found
}

/// A ship in space far from anything, held, made of the voxels of `size` that `solid` says are stone.
fn a_ship(solid: impl Fn([i128; 3]) -> bool + Send + 'static, size: [i128; 3]) -> Scene {
    let mut scene = Scene::in_a_new_world();
    make_ship(&mut scene, pose_at(0.0, 500_000.0, 0.0), true, size, solid);
    scene.ticks(3);
    scene
}

#[test]
fn a_ship_has_the_box_of_its_blocks_as_its_hull_and_not_the_box_of_its_bricks() {
    // An L that starts at (2, 1, 3): the box is not from the origin of the ship, and not the 32 voxels of its brick.
    let solid = |at: [i128; 3]| at[0] >= 2 && at[1] >= 1 && at[2] >= 3 && (at[0] < 15 || at[1] < 4);
    let size = [20, 10, 6];
    let mut scene = a_ship(solid, size);
    let hull = hull_of(&mut scene, SHIP).expect("a ship with blocks has a hull once it is weighed");
    assert_eq!(Some(hull), box_of(size, solid));
    assert_eq!(hull.lowest(), VoxelCoordinate::new(2, 1, 3));
    assert_eq!(hull.highest(), VoxelCoordinate::new(19, 9, 5));
}

#[test]
fn the_hull_of_blocks_in_bricks_far_apart_goes_from_one_to_the_other_and_not_to_the_far_corners_of_their_bricks()
 {
    let solid = |at: [i128; 3]| at == [5, 5, 5] || at == [105, 40, 3];
    let size = [110, 45, 8];
    let mut scene = a_ship(solid, size);
    let hull = hull_of(&mut scene, SHIP).expect("a hull");
    assert_eq!(Some(hull), box_of(size, solid));
    assert_eq!(hull.lowest(), VoxelCoordinate::new(5, 5, 3));
    assert_eq!(hull.highest(), VoxelCoordinate::new(105, 40, 5));
}

#[test]
fn a_ship_with_no_blocks_has_no_hull() {
    let mut scene = a_ship(|_| false, [4, 4, 4]);
    assert_eq!(hull_of(&mut scene, SHIP), None);
}

#[test]
fn a_block_put_outside_the_hull_makes_it_larger_and_one_taken_from_its_edge_makes_it_smaller() {
    let solid = |at: [i128; 3]| at[0] < 6 && at[1] < 4 && at[2] < 5;
    let mut scene = a_ship(solid, [6, 4, 5]);
    let before = hull_of(&mut scene, SHIP).expect("a hull");
    assert_eq!(before.highest(), VoxelCoordinate::new(5, 3, 4));
    let content = CaveContent::new();
    // A block a brick away, in another brick: the box goes to it.
    scene.run(move |world| {
        let mut view = world.grid(ship_id(SHIP)).unwrap();
        view.set(
            content.block_channel(),
            VoxelCoordinate::new(40, 2, 2),
            content.blocks.stone,
        )
        .unwrap();
    });
    scene.ticks(3);
    let larger = hull_of(&mut scene, SHIP).expect("a hull");
    assert_eq!(larger.lowest(), VoxelCoordinate::new(0, 0, 0));
    assert_eq!(larger.highest(), VoxelCoordinate::new(40, 3, 4));
    // That block taken out again, and the whole row of blocks at the highest x of the first brick: the box is what is left.
    let content = CaveContent::new();
    scene.run(move |world| {
        let mut view = world.grid(ship_id(SHIP)).unwrap();
        view.set(
            content.block_channel(),
            VoxelCoordinate::new(40, 2, 2),
            content.blocks.air,
        )
        .unwrap();
        for y in 0..4 {
            for z in 0..5 {
                view.set(
                    content.block_channel(),
                    VoxelCoordinate::new(5, y, z),
                    content.blocks.air,
                )
                .unwrap();
            }
        }
    });
    scene.ticks(3);
    let smaller = hull_of(&mut scene, SHIP).expect("a hull");
    assert_eq!(smaller.lowest(), VoxelCoordinate::new(0, 0, 0));
    assert_eq!(smaller.highest(), VoxelCoordinate::new(4, 3, 4));
}

#[test]
fn a_block_too_far_from_the_origin_of_the_ship_for_a_body_is_no_part_of_its_hull() {
    // A brick more than 2^31 voxels from the origin of a ship has no collider and no mass (`BodyWorld::set_brick`), so nothing can hit it and the ship does not weigh it: the hull is the box
    // of what the ship has a body of, the same bricks as its mass.
    let solid = |at: [i128; 3]| at[0] < 5 && at[1] < 3 && at[2] < 4;
    let mut scene = a_ship(solid, [5, 3, 4]);
    let content = CaveContent::new();
    scene.run(move |world| {
        let mut view = world.grid(ship_id(SHIP)).unwrap();
        view.set(
            content.block_channel(),
            VoxelCoordinate::new(3_000_000_000, 0, 0),
            content.blocks.stone,
        )
        .unwrap();
    });
    scene.ticks(3);
    let hull = hull_of(&mut scene, SHIP).expect("a hull");
    assert_eq!(Some(hull), box_of([5, 3, 4], solid));
}

#[test]
fn a_ship_that_goes_to_another_frame_of_the_solver_has_the_same_hull() {
    // Past the 32 km at which a ship leaves its frame it is made a body again in another, and takes its bricks' scans with it (`ship_frame_carry.rs`): the hull is one of them.
    let solid = |at: [i128; 3]| at[0] >= 1 && at[1] >= 2 && at[2] >= 1 && at[0] < 40;
    let size = [45, 4, 4];
    let mut scene = a_ship(solid, size);
    let before = hull_of(&mut scene, SHIP).expect("a hull");
    assert_eq!(Some(before), box_of(size, solid));
    scene.run(move |world| {
        let pose = world.data().grids().get(ship_id(SHIP)).unwrap().pose;
        world
            .set_grid_pose(
                ship_id(SHIP),
                GridPose::new(
                    pose.position.translated([45_000.0, 0.0, 0.0]),
                    pose.orientation,
                ),
            )
            .unwrap();
    });
    scene.ticks(5);
    assert_eq!(hull_of(&mut scene, SHIP), Some(before));
}

#[test]
fn the_hull_is_not_saved_and_is_worked_out_again_when_the_ship_is_next_weighed() {
    let solid = |at: [i128; 3]| at[0] >= 3 && at[1] >= 1 && at[2] >= 2 && at[0] < 11;
    let size = [12, 6, 6];
    let mut scene = Scene::in_a_new_world();
    // A ship is put in a file when it is far from every player or frozen; here the server is saved and made again from what was written.
    make_ship(&mut scene, pose_at(0.0, 500_000.0, 0.0), true, size, solid);
    scene.ticks(3);
    let before = hull_of(&mut scene, SHIP).expect("a hull");
    let mut scene = scene.restarted();
    scene.ticks(3);
    assert_eq!(hull_of(&mut scene, SHIP), Some(before));
    assert_eq!(Some(before), box_of(size, solid));
}

#[test]
fn a_planet_has_no_hull_for_its_blocks_are_mostly_not_stored() {
    // The bodies leave a planet alone (`set_grid_terrain`), so nothing weighs it, and a hull made of its few edits would be a lie about how far it reaches.
    let mut scene = common::moving_planet::a_barely_moving_scene();
    scene.ticks(3);
    assert_eq!(hull_of(&mut scene, common::moving_planet::WANDERER), None);
    assert_eq!(hull_of(&mut scene, 0), None, "the home planet");
}
