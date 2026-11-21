//! A dig that reaches the face of a brick changes what stands on the brick beside it: the colliders of terrain leave out a box that nothing open touches, which is found
//! by looking at the voxels across the face of its brick, so a brick that is solid with solid all round it has none, and the one that opens it is not the one that has
//! to be made again. A walker who digs down to the top of such a brick must stand on it.

mod common;

use common::{Scene, make_ship, player_pose, pose_at, ship_voxels, tick_and_follow};
use server::GridView;
use voxel_space::{GridId, LevelIndex, VoxelCoordinate};
use voxunami_cave::content::CaveContent;

/// The brick of the middle of this test, far above the hills of the overworld (which are about 200 up): the bricks are 32 voxels, so this is voxels 320 to 351 up, and
/// the one over it is 352 to 383.
const BURIED_FROM: i128 = 320;

/// Stone in the box of voxels `from` to `to`, both included (this takes a tick).
fn stone(scene: &mut Scene, from: [i128; 3], to: [i128; 3]) {
    let content = CaveContent::new();
    scene.run(move |world| {
        let mut grid = world.grid(GridId::root(LevelIndex::MAIN)).unwrap();
        for x in from[0]..=to[0] {
            for y in from[1]..=to[1] {
                for z in from[2]..=to[2] {
                    grid.set(
                        content.block_channel(),
                        VoxelCoordinate::new(x, y, z),
                        content.blocks.stone,
                    )
                    .unwrap();
                }
            }
        }
    });
}

/// Air in the box of voxels `from` to `to`, both included (this takes a tick).
fn dig(scene: &mut Scene, from: [i128; 3], to: [i128; 3]) {
    let content = CaveContent::new();
    scene.run(move |world| {
        let mut grid = world.grid(GridId::root(LevelIndex::MAIN)).unwrap();
        for x in from[0]..=to[0] {
            for y in from[1]..=to[1] {
                for z in from[2]..=to[2] {
                    grid.set(
                        content.block_channel(),
                        VoxelCoordinate::new(x, y, z),
                        content.blocks.air,
                    )
                    .unwrap();
                }
            }
        }
    });
}

#[test]
fn a_walker_who_digs_down_to_the_top_of_a_solid_brick_stands_on_it() {
    let mut scene = Scene::new();
    // A brick of stone (320 to 351 up, 0 to 31 along x and z) with a layer of stone all round it, so that it is buried; and over it 8 voxels more, in the brick above: the
    // ground the walker stands on, with its top at 360.
    stone(
        &mut scene,
        [-1, BURIED_FROM - 1, -1],
        [32, BURIED_FROM + 32 + 7, 32],
    );
    let mut player = scene.rig.join_walking([16.5, 364.0, 16.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 120);
    let standing = player_pose(&scene, 1).position.offset()[1];
    let on_the_ground = player_pose(&scene, 1).position.brick().y() * 32;

    // A shaft from the top of the ground down to the top of the buried brick, whose floor is the face of the brick above that one.
    dig(
        &mut scene,
        [12, BURIED_FROM + 32, 12],
        [20, BURIED_FROM + 32 + 7, 20],
    );
    tick_and_follow(&mut scene, &mut player, 240);
    let pose = player_pose(&scene, 1);
    let height_now = pose.position.brick().y() * 32;
    let fallen =
        (height_now as f64 + pose.position.offset()[1]) - (on_the_ground as f64 + standing);
    assert!(
        (fallen + 8.0).abs() < 0.05,
        "the walker fell {} down a shaft 8 deep, and stands on the floor of it",
        -fallen
    );
}

#[test]
fn a_ship_over_a_shaft_dug_to_the_top_of_a_solid_brick_comes_to_rest_on_it() {
    let mut scene = Scene::new();
    stone(
        &mut scene,
        [-1, BURIED_FROM - 1, -1],
        [32, BURIED_FROM + 32 + 7, 32],
    );
    // A raft of 5 by 5, free, let down on the ground (whose top is at 360) over where the shaft will be dug, which it fits in.
    let number = scene.rig.server.world().grids().iter().count() as u64;
    make_ship(
        &mut scene,
        pose_at(14.0, 360.5, 14.0),
        false,
        [5, 1, 5],
        |_| true,
    );
    scene.ticks(120);
    let resting = ship_voxels(&scene, number)[1];
    assert!(
        (resting - 360.0).abs() < 0.5,
        "the raft is on the ground: {resting}"
    );

    dig(
        &mut scene,
        [12, BURIED_FROM + 32, 12],
        [20, BURIED_FROM + 32 + 7, 20],
    );
    scene.ticks(240);
    let fallen = resting - ship_voxels(&scene, number)[1];
    assert!(
        (fallen - 8.0).abs() < 0.2,
        "the raft fell {fallen} down a shaft 8 deep, and rests on the floor of it"
    );
}
