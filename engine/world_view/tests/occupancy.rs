//! What the meshers say of which voxels block the sun: for every brick they mesh (the buried and the ones of sky too), once, and again only when it changes.

use std::collections::HashMap;
use std::sync::Arc;

use block_models::BlockTable;
use terrain::TerrainGenerator;
use voxel_grid::Grid;
use voxel_meshing::BrickOccupancy;
use voxel_space::{
    BRICK_VOXEL_COUNT, BrickBox, BrickCoordinate, GridId, LocalVoxelIndex, WorldPosition,
};
use world_storage::test_support::brick;
use world_view::{
    MeshUpdate, OccupancyChange, OccupancyStatistics, ShipTerrainMesher, ShipTerrainSource,
    ViewChannels, ViewSettings, WorldMesher,
};

mod common;

use common::{Scene, scene};

type Told = HashMap<BrickCoordinate, BrickOccupancy>;

/// Applies the changes the way a renderer would.
fn apply(told: &mut Told, changes: Vec<(BrickCoordinate, OccupancyChange)>) {
    for (coordinate, change) in changes {
        match change {
            OccupancyChange::Set(occupancy) => {
                told.insert(coordinate, occupancy);
            }
            OccupancyChange::Clear => {
                told.remove(&coordinate);
            }
        }
    }
}

/// The bricks within `radius` of `center`, as a ball, as the mesher meshes them.
fn ball(center: BrickCoordinate, radius: i128) -> Vec<BrickCoordinate> {
    let mut bricks = Vec::new();
    for x in -radius..=radius {
        for y in -radius..=radius {
            for z in -radius..=radius {
                if x * x + y * y + z * z <= radius * radius {
                    bricks.push(brick(center.x() + x, center.y() + y, center.z() + z));
                }
            }
        }
    }
    bricks.sort();
    bricks
}

fn settled(scene: &Scene, radius: i128, center: BrickCoordinate) -> (WorldMesher, Told) {
    let mut mesher = scene.mesher(radius);
    mesher.enable_occupancy();
    let mut told = Told::new();
    mesher.settle(scene.world.main(), center);
    apply(&mut told, mesher.take_occupancy_updates());
    (mesher, told)
}

#[test]
fn nothing_is_said_of_the_occupancy_unless_it_was_asked_for() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 6);
    let mut mesher = scene.mesher(3);
    mesher.settle(scene.world.main(), brick(0, 0, 0));
    assert!(mesher.take_occupancy_updates().is_empty());
    assert_eq!(
        mesher.occupancy_statistics(),
        OccupancyStatistics::default()
    );
}

#[test]
fn every_brick_that_is_meshed_is_told_of_the_buried_and_the_sky_ones_too_and_a_mixed_one_says_what_the_mesher_knows_of_each_voxel()
 {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 6);
    let mut mesher = scene.mesher(3);
    mesher.enable_occupancy();
    let mut live = HashMap::new();
    let mut told = Told::new();
    for update in mesher.settle(scene.world.main(), brick(0, 0, 0)) {
        if let MeshUpdate::Upload { coordinate, mesh } = update
            && !mesh.is_empty()
        {
            live.insert(coordinate, mesh);
        }
    }
    apply(&mut told, mesher.take_occupancy_updates());

    // Flat ground: the surface bricks (y = 0) have ground to y = 9 and sky over it, those under them are all stone, those over them all sky.
    let expected = ball(brick(0, 0, 0), 3);
    let mut told_of: Vec<BrickCoordinate> = told.keys().copied().collect();
    told_of.sort();
    assert_eq!(told_of, expected);
    assert_eq!(live.len(), 29, "only the surface bricks have a mesh");
    let (mut mixed, mut full, mut empty) = (0, 0, 0);
    for (coordinate, occupancy) in &told {
        match coordinate.y().cmp(&0) {
            std::cmp::Ordering::Equal => {
                assert!(
                    matches!(occupancy, BrickOccupancy::Mixed(_)),
                    "{coordinate}"
                );
                mixed += 1;
            }
            std::cmp::Ordering::Less => {
                assert_eq!(*occupancy, BrickOccupancy::Full, "{coordinate}");
                assert!(!live.contains_key(coordinate), "a buried brick has a mesh");
                full += 1;
            }
            std::cmp::Ordering::Greater => {
                assert_eq!(*occupancy, BrickOccupancy::Empty, "{coordinate}");
                assert!(!live.contains_key(coordinate), "a brick of sky has a mesh");
                empty += 1;
            }
        }
    }
    assert_eq!(
        mesher.occupancy_statistics(),
        OccupancyStatistics {
            empty,
            full,
            mixed,
            shaped: 0,
        }
    );
    assert_eq!((mixed, full + empty), (29, expected.len() - 29));

    // A mixed brick, voxel by voxel, against the mesher's own answer for the same voxel.
    for coordinate in [brick(1, 0, 2), brick(0, 0, 0), brick(-3, 0, 0)] {
        let occupancy = &told[&coordinate];
        let mut blocking = 0;
        for index in 0..BRICK_VOXEL_COUNT {
            let local = LocalVoxelIndex::from_index(index).unwrap();
            let voxel = coordinate.voxel_at(local);
            let expected = mesher.is_opaque_at(voxel).expect("the brick is loaded");
            assert_eq!(occupancy.blocks(local), expected, "{coordinate} {local:?}");
            blocking += usize::from(expected);
        }
        // Ten layers of ground (y = 0 to 9) of 32 by 32.
        assert_eq!(blocking, 10 * 32 * 32, "{coordinate}");
    }
}

#[test]
fn a_brick_is_told_of_again_only_when_its_occupancy_changes() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 6);
    let (mut mesher, mut told) = settled(&scene, 3, brick(0, 0, 0));
    let before = told[&brick(0, 0, 0)].clone();

    // Meshed again, and nothing to say: it is what it was.
    mesher.brick_changed(brick(0, 0, 0));
    mesher.settle(scene.world.main(), brick(0, 0, 0));
    assert!(mesher.take_occupancy_updates().is_empty());

    // Dig out one voxel in the middle of the brick: that brick says so, and the six that were meshed again as its neighbours say nothing.
    let edited = scene.edit([5, 9, 5], "base_blocks:air");
    assert_eq!(edited, brick(0, 0, 0));
    mesher.brick_changed(edited);
    mesher.settle(scene.world.main(), brick(0, 0, 0));
    let changes = mesher.take_occupancy_updates();
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!(changes[0].0, brick(0, 0, 0));
    apply(&mut told, changes);
    let after = &told[&brick(0, 0, 0)];
    let (BrickOccupancy::Mixed(old), BrickOccupancy::Mixed(new)) = (&before, after) else {
        panic!("{before:?} {after:?}");
    };
    let index = LocalVoxelIndex::new(5, 9, 5).index();
    for (word, (&old, &new)) in old.iter().zip(new.iter()).enumerate() {
        if word == index / 32 {
            assert_eq!(old ^ new, 1 << (index % 32), "only the dug voxel's bit");
        } else {
            assert_eq!(old, new, "word {word}");
        }
    }
}

#[test]
fn a_brick_edited_to_sky_is_told_of_as_empty_and_one_filled_with_stone_as_full() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 6);
    let (mut mesher, mut told) = settled(&scene, 2, brick(0, 0, 0));
    for x in 0..32 {
        for z in 0..32 {
            for y in 0..10 {
                scene.edit([x, y, z], "base_blocks:air");
            }
        }
    }
    mesher.brick_changed(brick(0, 0, 0));
    mesher.settle(scene.world.main(), brick(0, 0, 0));
    apply(&mut told, mesher.take_occupancy_updates());
    assert_eq!(told[&brick(0, 0, 0)], BrickOccupancy::Empty);

    for x in 0..32 {
        for z in 0..32 {
            for y in 0..32 {
                scene.edit([x, 32 + y, z], "base_blocks:stone");
            }
        }
    }
    mesher.brick_changed(brick(0, 1, 0));
    mesher.settle(scene.world.main(), brick(0, 0, 0));
    apply(&mut told, mesher.take_occupancy_updates());
    assert_eq!(told[&brick(0, 1, 0)], BrickOccupancy::Full);
}

#[test]
fn moving_away_clears_what_went_out_of_range_and_tells_of_what_came_in() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 20);
    let (mut mesher, mut told) = settled(&scene, 3, brick(0, 0, 0));
    assert_eq!(told.len(), ball(brick(0, 0, 0), 3).len());

    mesher.settle(scene.world.main(), brick(10, 0, 0));
    let changes = mesher.take_occupancy_updates();
    assert!(
        changes
            .iter()
            .any(|(_, change)| *change == OccupancyChange::Clear)
    );
    apply(&mut told, changes);
    let mut told_of: Vec<BrickCoordinate> = told.keys().copied().collect();
    told_of.sort();
    assert_eq!(
        told_of,
        ball(brick(10, 0, 0), 3),
        "only the area round the new centre"
    );
    let statistics = mesher.occupancy_statistics();
    assert_eq!(
        statistics.empty + statistics.full + statistics.mixed,
        told.len()
    );
}

#[test]
fn resetting_clears_every_brick_that_was_told_of_in_order() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 6);
    let (mut mesher, told) = settled(&scene, 2, brick(0, 0, 0));
    assert!(!told.is_empty());
    mesher.reset();
    let changes = mesher.take_occupancy_updates();
    let mut expected: Vec<BrickCoordinate> = told.keys().copied().collect();
    expected.sort();
    assert_eq!(
        changes,
        expected
            .into_iter()
            .map(|coordinate| (coordinate, OccupancyChange::Clear))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        mesher.occupancy_statistics(),
        OccupancyStatistics::default()
    );
}

const PLANET: GridId = GridId::new(GridId::FIRST_ALLOCATED + 2);

fn generator_of(scene: &Scene) -> Arc<dyn TerrainGenerator> {
    let terrain = &scene.world.main().terrain;
    Arc::clone(
        terrain
            .generators()
            .entry(terrain.default_generator())
            .unwrap()
            .content()
            .unwrap(),
    )
}

#[test]
fn a_planet_tells_of_its_bricks_in_its_own_coordinates_as_the_terrain_of_a_level_does() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 6);
    let (_, as_terrain) = settled(&scene, 3, brick(0, 0, 0));

    let voxels = Grid::new(Arc::clone(scene.world.main().overrides.layout()));
    let source = ShipTerrainSource::new(
        &voxels,
        Some(generator_of(&scene)),
        BrickBox::around(BrickCoordinate::ORIGIN, 8),
    );
    let mut opaque = vec![true; 16];
    opaque[scene.air as usize] = false;
    let mut mesher = ShipTerrainMesher::new(
        PLANET,
        ViewSettings {
            view_radius_bricks: 3,
            worker_count: 3,
        },
        ViewChannels::new(scene.material, None, None),
        Arc::new(BlockTable::of_cubes(&opaque)),
    )
    .with_occupancy();
    assert_eq!(mesher.grid(), PLANET);
    mesher.settle(&source, WorldPosition::new(brick(0, 0, 0), [16.0; 3]));
    let mut as_ship = Told::new();
    apply(&mut as_ship, mesher.take_occupancy_updates());
    assert_eq!(as_ship, as_terrain);
    assert_eq!(
        mesher.occupancy_statistics().mixed,
        29,
        "the surface bricks"
    );
}
