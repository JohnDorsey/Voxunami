//! A scene built as two ships and merged settles to what it would have been as one, and a scene cut in two settles to what two ships built apart would
//! have been (requirement 14 of the plan): a rule that works aboard a ship works across the join of a merge with nothing done for it, and a
//! split leaves each part as if it had always been alone.
//!
//! The rule checked is the light, which has no chance in it (the fire has, keyed by the grid, so a ship made by a merge cannot burn bit for bit as one built
//! whole would: the owner, 2026-10-22, said that is as it should be).

mod common;

use std::sync::{Arc, Mutex};

use common::{Scene, ship_id, tick_and_follow};
use server::{GridView, MergePlan};
use voxel_space::{
    AlignmentTolerance, GridId, GridPose, LevelIndex, VoxelCoordinate, WorldPosition,
};
use voxunami_cave::content::CaveContent;

/// Whole bricks from the origin of the world (the light works in cells of bricks), and a ship held there.
const ORIGIN: [f32; 3] = [32.0, 224.0, 32.0];

/// The box the light is looked at in: the ships are 12 by 12 on the floor, with room above for the light.
const BOX: ([i128; 3], [i128; 3]) = ([0, 0, 0], [14, 7, 14]);

fn pose() -> GridPose {
    GridPose::new(
        WorldPosition::new(voxel_space::BrickCoordinate::ORIGIN, ORIGIN),
        voxel_space::Orientation::IDENTITY,
    )
}

/// Makes a held ship of stone, with these lamps, at `pose()`: stone at the (x, z) pairs on the floor (y = 0).
fn make(scene: &mut Scene, floor: Vec<(i128, i128)>, lamps: Vec<[i128; 3]>) {
    let content = CaveContent::new();
    scene.run(move |world| {
        let ship = world.create_grid(LevelIndex::MAIN, pose()).unwrap();
        world.set_grid_held(ship, true).unwrap();
        let mut view = world.grid(ship).unwrap();
        for (x, z) in floor {
            view.set(
                content.block_channel(),
                VoxelCoordinate::new(x, 0, z),
                content.blocks.stone,
            )
            .unwrap();
        }
        for [x, y, z] in lamps {
            view.set(
                content.block_channel(),
                VoxelCoordinate::new(x, y, z),
                content.blocks.lamp,
            )
            .unwrap();
        }
    });
}

fn square(x: std::ops::Range<i128>, z: std::ops::Range<i128>) -> Vec<(i128, i128)> {
    x.flat_map(|x| z.clone().map(move |z| (x, z))).collect()
}

/// The light in the box, ship by ship: each ship that has a block at the marker voxel (so that the same ship is looked at in both scenes), in
/// the order of the markers.
fn light_of_the_ships_with(scene: &mut Scene, markers: Vec<[i128; 3]>) -> Vec<Vec<u32>> {
    let content = CaveContent::new();
    let found = Arc::new(Mutex::new(Vec::new()));
    let slot = Arc::clone(&found);
    scene.run(move |world| {
        let ids: Vec<GridId> = world.data().grids().iter().map(|(id, _)| id).collect();
        let mut all = Vec::new();
        for marker in markers {
            let at = VoxelCoordinate::new(marker[0], marker[1], marker[2]);
            let id = ids
                .iter()
                .copied()
                .find(|id| {
                    world
                        .grid(*id)
                        .unwrap()
                        .get(content.block_channel(), at)
                        .unwrap()
                        != content.blocks.air
                })
                .expect("a ship has a block at the marker");
            let mut view = world.grid(id).unwrap();
            let mut light = Vec::new();
            for x in BOX.0[0]..BOX.1[0] {
                for y in BOX.0[1]..BOX.1[1] {
                    for z in BOX.0[2]..BOX.1[2] {
                        light.push(
                            view.get(content.light_channel(), VoxelCoordinate::new(x, y, z))
                                .unwrap(),
                        );
                    }
                }
            }
            all.push(light);
        }
        *slot.lock().unwrap() = all;
    });
    found.lock().unwrap().clone()
}

fn scene_with_a_player() -> (Scene, common::Player) {
    let mut scene = Scene::new();
    let mut player = scene.rig.join([40.0, 232.0, 40.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    (scene, player)
}

#[test]
fn two_ships_merged_settle_to_the_light_one_ship_built_whole_has() {
    // The lamp is on the first floor (x 0 to 6); the second floor (x 6 to 12) is the other ship's, beside it.
    let lamps = vec![[3, 1, 6]];
    // Built whole.
    let (mut whole, mut player) = scene_with_a_player();
    make(&mut whole, square(0..12, 0..12), lamps.clone());
    tick_and_follow(&mut whole, &mut player, 40);
    let light_whole = light_of_the_ships_with(&mut whole, vec![[0, 0, 0]]);
    assert!(
        light_whole[0].iter().any(|level| *level > 0),
        "the lamp lights something"
    );

    // Built as two, which are two for a while and then one.
    let (mut two, mut player) = scene_with_a_player();
    make(&mut two, square(0..6, 0..12), lamps);
    make(&mut two, square(6..12, 0..12), vec![]);
    tick_and_follow(&mut two, &mut player, 40);
    let apart = light_of_the_ships_with(&mut two, vec![[0, 0, 0], [6, 0, 0]]);
    assert_ne!(
        apart[0], light_whole[0],
        "as two, the first ship's air is lit where the second's floor is, and that is not the light of one ship"
    );
    let content = CaveContent::new();
    two.run(move |world| {
        // (The orientations of the blocks are turned by the merge itself; nothing else needs turning.)
        let turn =
            |_: &mut Vec<(voxel_grid::ChannelIndex, u32)>, _: voxel_space::LatticeRotation| {};
        world
            .merge_grids(
                ship_id(0),
                ship_id(1),
                &MergePlan {
                    block_channel: content.block_channel(),
                    orientation_channel: Some(content.orientation_channel()),
                    tolerance: AlignmentTolerance::default(),
                    turn: &turn,
                    linear_velocity: [0.0; 3],
                    angular_velocity: [0.0; 3],
                },
            )
            .unwrap();
    });
    tick_and_follow(&mut two, &mut player, 60);
    let merged = light_of_the_ships_with(&mut two, vec![[0, 0, 0]]);
    assert_eq!(
        merged[0], light_whole[0],
        "merged, the light is what one ship built whole has"
    );
}

#[test]
fn a_ship_cut_in_two_settles_to_the_light_two_ships_built_apart_have() {
    // A floor in two halves (x 0 to 5 and 7 to 12) joined by a bridge of two blocks at x = 5 and 6; the lamp is on the first half (a lamp must be on something: a block joined to neither half would go with whichever part the check leaves as the rest).
    let lamps = vec![[2, 1, 6]];
    let mut floor = square(0..5, 0..12);
    floor.extend(square(7..12, 0..12));
    let mut bridge = floor.clone();
    bridge.extend([(5, 6), (6, 6)]);

    // Built whole, and cut.
    let (mut cut, mut player) = scene_with_a_player();
    make(&mut cut, bridge, lamps.clone());
    tick_and_follow(&mut cut, &mut player, 60);
    let content = CaveContent::new();
    cut.run(move |world| {
        for x in [5, 6] {
            world
                .grid(ship_id(0))
                .unwrap()
                .set(
                    content.block_channel(),
                    VoxelCoordinate::new(x, 0, 6),
                    content.blocks.air,
                )
                .unwrap();
        }
    });
    // The ship is whole until its check says otherwise, and the check takes a tick or two; then the light of each part settles.
    tick_and_follow(&mut cut, &mut player, 200);
    assert_eq!(
        cut.rig.server.world().grids().iter().count(),
        2,
        "the ship was cut in two"
    );
    let after_the_cut = light_of_the_ships_with(&mut cut, vec![[0, 0, 0], [7, 0, 0]]);

    // Built apart.
    let (mut apart, mut player) = scene_with_a_player();
    make(&mut apart, square(0..5, 0..12), lamps);
    make(&mut apart, square(7..12, 0..12), vec![]);
    tick_and_follow(&mut apart, &mut player, 200);
    let built_apart = light_of_the_ships_with(&mut apart, vec![[0, 0, 0], [7, 0, 0]]);

    assert!(
        after_the_cut[0].iter().any(|level| *level > 0),
        "the lamp's half is lit"
    );
    assert_eq!(after_the_cut[0], built_apart[0], "the half with the lamp");
    assert_eq!(after_the_cut[1], built_apart[1], "the half without it");
}
