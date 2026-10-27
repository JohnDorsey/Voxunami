//! The frame-invariance test (plan requirement 6): a rule given one grid and its local coordinates does the same in the terrain of a level
//! and in a ship, wherever the ship is and however it is turned. The rule here is Cave's light; a scene is built in each place by
//! plugin code running inside the server, and the light that settles is compared voxel by voxel in the grid's own coordinates.

mod common;

use std::sync::{Arc, Mutex};

use common::{Rig, Scene};
use server::{GridView, ServerWorld};
use voxel_space::{
    BrickCoordinate, GridId, GridPose, LatticeRotation, LevelIndex, VoxelCoordinate, WorldPosition,
};
use voxunami_cave::content::CaveContent;

/// How big the room is: voxels 0 to 6 on each axis, with walls on the outside layer.
const SIZE: i128 = 7;

/// A closed stone room with a lamp in the middle, built in the grid at this offset from the grid's origin.
fn build_room(world: &mut ServerWorld, grid: GridId, content: &CaveContent, offset: [i128; 3]) {
    let (block, stone, lamp) = (
        content.block_channel(),
        content.blocks.stone,
        content.blocks.lamp,
    );
    let mut view = world.grid(grid).unwrap();
    for x in 0..SIZE {
        for y in 0..SIZE {
            for z in 0..SIZE {
                let wall = [x, y, z].iter().any(|c| *c == 0 || *c == SIZE - 1);
                let value = if wall { stone } else { 0 };
                let at = VoxelCoordinate::new(x + offset[0], y + offset[1], z + offset[2]);
                if value != 0 {
                    view.set(block, at, value).unwrap();
                }
            }
        }
    }
    let middle = SIZE / 2;
    let at = VoxelCoordinate::new(middle + offset[0], middle + offset[1], middle + offset[2]);
    view.set(block, at, lamp).unwrap();
}

/// The light in and around the room, voxel by voxel in the grid's own coordinates.
fn light_of(rig: &Rig, grid: GridId, offset: [i128; 3]) -> Vec<u32> {
    let light = rig.content.light_channel();
    let world = rig.server.world();
    let mut values = Vec::new();
    for x in -1..=SIZE {
        for y in -1..=SIZE {
            for z in -1..=SIZE {
                let at = VoxelCoordinate::new(x + offset[0], y + offset[1], z + offset[2]);
                values.push(match grid.root_level() {
                    Some(level) => world.level(level).unwrap().overrides.get(light, at),
                    None => world.grids().get(grid).unwrap().voxels.get(light, at),
                });
            }
        }
    }
    values
}

/// Builds the room in the terrain of the main level, high above the ground, and returns its light.
fn light_in_the_terrain() -> Vec<u32> {
    let mut scene = Scene::new();
    let content = CaveContent::new();
    let offset = [100, 300, 100];
    scene.run(move |world| build_room(world, GridId::MAIN, &content, offset));
    scene.ticks(60);
    light_of(&scene.rig, GridId::MAIN, offset)
}

/// Builds the room in a new ship with this pose, and returns its light.
fn light_in_a_ship(pose: GridPose) -> Vec<u32> {
    let mut scene = Scene::new();
    let content = CaveContent::new();
    let ship = Arc::new(Mutex::new(None));
    let made = Arc::clone(&ship);
    scene.run(move |world| {
        let grid = world.create_grid(LevelIndex::MAIN, pose).unwrap();
        *made.lock().unwrap() = Some(grid);
        build_room(world, grid, &content, [0, 0, 0]);
    });
    scene.ticks(60);
    let grid = ship.lock().unwrap().unwrap();
    light_of(&scene.rig, grid, [0, 0, 0])
}

fn pose(brick: [i128; 3], offset: [f32; 3], rotation: LatticeRotation) -> GridPose {
    GridPose::new(
        WorldPosition::new(BrickCoordinate::new(brick[0], brick[1], brick[2]), offset),
        rotation.orientation(),
    )
}

#[test]
fn light_settles_the_same_in_the_terrain_and_in_a_ship_wherever_it_is_and_however_it_is_turned() {
    let in_the_terrain = light_in_the_terrain();
    // The scene is a real one: the lamp lights its room, dimmer farther away, and the walls keep it in.
    let middle = (SIZE / 2 + 1) as usize;
    let side = (SIZE + 2) as usize;
    let index = |x: usize, y: usize, z: usize| (x * side + y) * side + z;
    let beside_it = in_the_terrain[index(middle + 1, middle, middle)];
    let in_a_corner = in_the_terrain[index(2, 2, 2)];
    let outside = in_the_terrain[index(0, 0, 0)];
    // (The lamp is a block, and what it lights is the air beside it.)
    assert_eq!(Some(&beside_it), in_the_terrain.iter().max());
    assert!(beside_it > in_a_corner && in_a_corner > 0);
    assert_eq!(outside, 0);

    let rotations = LatticeRotation::all();
    let poses = [
        pose([0, 0, 0], [0.0; 3], LatticeRotation::IDENTITY),
        pose([-7, 11, 3], [5.5, 1.25, 30.0], rotations[1]),
        pose([2_000_000, 40, -9], [0.0; 3], rotations[6]),
        pose([0, 9, 0], [31.0, 0.0, 0.5], rotations[17]),
        pose([1, 1, 1], [1.0; 3], rotations[23]),
    ];
    for pose in poses {
        assert_eq!(light_in_a_ship(pose), in_the_terrain, "{pose:?}");
    }
}
