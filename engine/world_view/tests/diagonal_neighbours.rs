//! A brick is drawn from the voxels of the layer round it, which includes the bricks that touch it only along an edge or at a corner: the shade of the corners of a face reads
//! the cells beside the one it looks at (`voxel_meshing::corner_occlusion`). So the mesher waits for all twenty-six, and a change in a brick is a change in all twenty-six.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use terrain::TerrainGenerator;
use voxel_grid::{Brick, GridLayout};
use voxel_space::BrickCoordinate;
use world_storage::LevelData;
use world_storage::test_support::brick;
use world_view::{MeshUpdate, TerrainSource, WorldMesher};

mod common;

use common::scene;

/// The terrain of a level, with bricks that nothing here can make.
struct Source<'a> {
    level: &'a LevelData,
    missing: Vec<BrickCoordinate>,
}

impl TerrainSource for Source<'_> {
    fn known(&self, coordinate: BrickCoordinate) -> Option<Arc<Brick>> {
        self.level.known(coordinate)
    }

    fn generator_for(&self, coordinate: BrickCoordinate) -> Option<Arc<dyn TerrainGenerator>> {
        if self.missing.contains(&coordinate) {
            return None;
        }
        self.level.generator_for(coordinate)
    }

    fn layout(&self) -> &Arc<GridLayout> {
        self.level.layout()
    }
}

/// What a renderer would hold after the updates: the bricks that have a picture.
fn pictures(updates: Vec<MeshUpdate>) -> BTreeSet<BrickCoordinate> {
    let mut live = HashMap::new();
    for update in updates {
        match update {
            MeshUpdate::Upload { coordinate, mesh } if !mesh.is_empty() => {
                live.insert(coordinate, ());
            }
            MeshUpdate::Upload { coordinate, .. } | MeshUpdate::Remove(coordinate) => {
                live.remove(&coordinate);
            }
            other => panic!("the mesher of the terrain made {other:?}"),
        }
    }
    live.into_keys().collect()
}

/// The bricks of the ground (y = 0) that have a picture when everything is there, and when `missing` cannot be made.
fn drawn_without(missing: Vec<BrickCoordinate>) -> BTreeSet<BrickCoordinate> {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 8);
    let source = Source {
        level: scene.world.main(),
        missing,
    };
    let mut mesher = scene.mesher(3);
    pictures(mesher.settle(&source, brick(0, 0, 0)))
}

#[test]
fn a_brick_waits_for_a_brick_that_touches_it_only_at_a_corner() {
    let everything = drawn_without(Vec::new());
    assert!(everything.contains(&brick(0, 0, 0)));
    // Nothing makes the brick above and beyond the ground's brick (0, 0, 0) at its high corner. The ground bricks that touch it, at their own corners or along their own
    // edges, are the nine of x and z from 0 to 2: they cannot be drawn against a hole.
    let waiting: BTreeSet<BrickCoordinate> = (0..=2)
        .flat_map(|x| (0..=2).map(move |z| brick(x, 0, z)))
        .collect();
    let drawn = drawn_without(vec![brick(1, 1, 1)]);
    let expected: BTreeSet<BrickCoordinate> = everything.difference(&waiting).copied().collect();
    assert_eq!(drawn, expected);
    // Before the mesher looked at corners, only the bricks that the hole touched across a face would have waited.
    assert!(waiting.len() > 1);
}

#[test]
fn a_brick_waits_for_a_brick_that_touches_it_only_along_an_edge() {
    let everything = drawn_without(Vec::new());
    // The brick at (1, 1, 0) touches the ground's bricks (0, 0, 0) to (2, 0, 1) along edges or faces; of those the ground ones are x from 0 to 2 and z from -1 to 1.
    let waiting: BTreeSet<BrickCoordinate> = (0..=2)
        .flat_map(|x| (-1..=1).map(move |z| brick(x, 0, z)))
        .collect();
    let drawn = drawn_without(vec![brick(1, 1, 0)]);
    let expected: BTreeSet<BrickCoordinate> = everything.difference(&waiting).copied().collect();
    assert_eq!(drawn, expected);
}

#[test]
fn a_change_in_a_brick_draws_it_and_every_brick_that_touches_it_again() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 8);
    let mut mesher: WorldMesher = scene.mesher(3);
    let before = pictures(mesher.settle(scene.world.main(), brick(0, 0, 0)));
    assert!(before.contains(&brick(0, 0, 0)));

    mesher.brick_changed(brick(0, 0, 0));
    let again: BTreeSet<BrickCoordinate> = mesher
        .settle(scene.world.main(), brick(0, 0, 0))
        .into_iter()
        .filter_map(|update| match update {
            MeshUpdate::Upload { coordinate, mesh } if !mesh.is_empty() => Some(coordinate),
            _ => None,
        })
        .collect();
    // The ground bricks among the twenty-seven: the brick and the eight round it in its own layer, four of them only at a corner.
    let expected: BTreeSet<BrickCoordinate> = (-1..=1)
        .flat_map(|x| (-1..=1).map(move |z| brick(x, 0, z)))
        .collect();
    assert_eq!(again, expected);
}
