//! The mesher against worlds of flat ground: what it loads, meshes, unloads and re-meshes.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use content_registry::Registry;
use terrain::{TerrainGenerator, WorldTerrain};

use region_octree::BrickCube;
use voxel_grid::{Brick, ChannelStorage, Grid, GridLayout};
use voxel_meshing::{FULL_LIGHT, PackedBrickMesh, Quad};
use voxel_space::{BrickBox, BrickCoordinate, FaceDirection, VoxelCoordinate};
use world_storage::test_support::{Game, Plugin, brick, id};
use world_storage::{LevelData, WorldData};
use world_view::{BrickLight, MeshUpdate};

mod common;

use common::{Scene, scene};

/// Applies updates the way a renderer would.
fn apply(live: &mut HashMap<BrickCoordinate, PackedBrickMesh>, updates: Vec<MeshUpdate>) {
    for update in updates {
        match update {
            MeshUpdate::Upload { coordinate, mesh } if mesh.is_empty() => {
                live.remove(&coordinate);
            }
            MeshUpdate::Upload { coordinate, mesh } => {
                live.insert(coordinate, mesh);
            }
            MeshUpdate::Remove(coordinate) => {
                live.remove(&coordinate);
            }
            // The terrain mesher makes nothing for ships.
            MeshUpdate::UploadGridBrick { .. }
            | MeshUpdate::RemoveGridBrick { .. }
            | MeshUpdate::GridPose { .. }
            | MeshUpdate::RemoveGrid(_) => panic!("the terrain mesher made an update for a ship"),
            MeshUpdate::UploadCoarse { .. } | MeshUpdate::RemoveCoarse(_) => {
                panic!("the mesher of bricks made an update for a coarse node")
            }
        }
    }
}

/// The whole-voxel quads of a mesh; the fixtures' blocks are all whole cubes, so it has no others.
fn unpacked(mesh: &PackedBrickMesh) -> Vec<Quad> {
    assert!(mesh.detailed.is_empty());
    mesh.quads.iter().map(|quad| quad.unpack()).collect()
}

#[test]
fn flat_ground_streams_in_exactly_one_top_quad_per_surface_brick() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 6);
    let mut mesher = scene.mesher(3);
    let mut live = HashMap::new();
    apply(&mut live, mesher.settle(scene.world.main(), brick(0, 0, 0)));

    // Surface bricks (y = 0) within 3 bricks horizontally: integer points with x^2 + z^2 <= 9.
    let mut expected: Vec<BrickCoordinate> = (-3..=3)
        .flat_map(|x| (-3..=3).map(move |z| (x, z)))
        .filter(|(x, z)| x * x + z * z <= 9)
        .map(|(x, z)| brick(x, 0, z))
        .collect();
    expected.sort();
    assert_eq!(expected.len(), 29);
    let mut shown: Vec<BrickCoordinate> = live.keys().copied().collect();
    shown.sort();
    assert_eq!(shown, expected);

    // One 32 x 32 grass quad on top and nothing else: any border faces would reveal a mesh built
    // before its neighbours existed.
    let grass = scene.game.raw_block("base_blocks:grass");
    for coordinate in expected {
        assert_eq!(
            unpacked(&live[&coordinate]),
            [Quad {
                origin: [0, 9, 0],
                width: 32,
                height: 32,
                face: FaceDirection::PositiveY,
                material: grass,
                light: FULL_LIGHT,
            }],
            "{coordinate}"
        );
    }
}

#[test]
fn nothing_is_loaded_until_the_world_says_which_generator_made_the_terrain() {
    let mut scene = scene();
    let mut mesher = scene.mesher(2);
    assert!(mesher.settle(scene.world.main(), brick(0, 0, 0)).is_empty());
    assert_eq!(mesher.loaded_brick_count(), 0);
    assert_eq!(mesher.material_at(VoxelCoordinate::new(0, 0, 0)), None);

    scene.assign_around(brick(0, 0, 0), 4);
    let updates = mesher.settle(scene.world.main(), brick(0, 0, 0));
    assert!(!updates.is_empty());
    assert!(mesher.loaded_brick_count() > 30);
    assert_eq!(
        mesher.material_at(VoxelCoordinate::new(5, 3, 5)),
        Some(scene.game.raw_block("base_blocks:stone"))
    );
    assert_eq!(
        mesher.is_opaque_at(VoxelCoordinate::new(5, 20, 5)),
        Some(false)
    );
    assert_eq!(
        mesher.is_solid_at(VoxelCoordinate::new(5, 3, 5)),
        Some(true)
    );
    assert_eq!(
        mesher.is_solid_at(VoxelCoordinate::new(5, 20, 5)),
        Some(false)
    );
    assert_eq!(
        mesher.is_opaque_at(VoxelCoordinate::new(5, 3, 5)),
        Some(true)
    );
    assert_eq!(mesher.is_opaque_at(VoxelCoordinate::new(5000, 3, 5)), None);
}

#[test]
fn a_settled_mesher_has_nothing_more_to_do() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 4);
    let mut mesher = scene.mesher(2);
    mesher.settle(scene.world.main(), brick(0, 0, 0));
    assert_eq!(mesher.pending_jobs(), 0);
    assert!(mesher.update(scene.world.main(), brick(0, 0, 0)).is_empty());
}

#[test]
fn moving_away_unloads_old_bricks_and_loads_new_ones() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 20);
    let mut mesher = scene.mesher(3);
    let mut live = HashMap::new();
    apply(&mut live, mesher.settle(scene.world.main(), brick(0, 0, 0)));
    let loaded_before = mesher.loaded_brick_count();
    assert_eq!(live.len(), 29);

    apply(
        &mut live,
        mesher.settle(scene.world.main(), brick(10, 0, 0)),
    );

    assert_eq!(
        live.len(),
        29,
        "only the area around the new centre remains"
    );
    assert!(
        live.keys()
            .all(|coordinate| (coordinate.x() - 10).pow(2) + coordinate.z().pow(2) <= 9)
    );
    assert!(
        mesher.loaded_brick_count() <= loaded_before + 40,
        "old bricks should have been unloaded"
    );
}

#[test]
fn nothing_is_drawn_where_the_world_is_all_sky() {
    let mut scene = scene();
    scene.assign_around(brick(0, 40, 0), 4);
    let mut mesher = scene.mesher(2);
    assert!(
        mesher
            .settle(scene.world.main(), brick(0, 40, 0))
            .is_empty()
    );
}

#[test]
fn dropping_the_mesher_stops_its_workers() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 4);
    let mut mesher = scene.mesher(2);
    mesher.update(scene.world.main(), brick(0, 0, 0));
    drop(mesher); // must not hang
}

#[test]
fn an_edit_changes_the_mesh_of_its_brick_and_of_the_neighbour_whose_face_it_exposes() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 4);
    let mut mesher = scene.mesher(3);
    let mut live = HashMap::new();
    apply(&mut live, mesher.settle(scene.world.main(), brick(0, 0, 0)));
    let before_neighbour = live[&brick(1, 0, 0)].clone();

    // Dig out the top voxel at the +x edge of brick (0, 0, 0): its east neighbour now has a wall
    // facing the hole.
    let edited = scene.edit([31, 9, 5], "base_blocks:air");
    assert_eq!(edited, brick(0, 0, 0));
    mesher.brick_changed(edited);
    apply(&mut live, mesher.settle(scene.world.main(), brick(0, 0, 0)));

    let neighbour = unpacked(&live[&brick(1, 0, 0)]);
    assert_ne!(live[&brick(1, 0, 0)], before_neighbour);
    assert!(
        neighbour
            .iter()
            .any(|quad| quad.face == FaceDirection::NegativeX && quad.origin == [0, 9, 5]),
        "{neighbour:?}"
    );
    let own = unpacked(&live[&brick(0, 0, 0)]);
    assert!(own.len() > 1, "the hole adds walls and a floor: {own:?}");
}

#[test]
fn editing_everything_away_removes_the_mesh() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 4);
    let mut mesher = scene.mesher(2);
    let mut live = HashMap::new();
    apply(&mut live, mesher.settle(scene.world.main(), brick(0, 0, 0)));
    assert!(live.contains_key(&brick(0, 0, 0)));

    // Replace the whole brick (and the three beside it that also have ground) with sky.
    for x in 0..32 {
        for z in 0..32 {
            for y in 0..10 {
                scene.edit([x, y, z], "base_blocks:air");
            }
        }
    }
    mesher.brick_changed(brick(0, 0, 0));
    let updates = mesher.settle(scene.world.main(), brick(0, 0, 0));
    assert!(updates.iter().any(|update| matches!(
        update,
        MeshUpdate::Upload { coordinate, mesh } if *coordinate == brick(0, 0, 0) && mesh.is_empty()
    )));
    apply(&mut live, updates);
    assert!(!live.contains_key(&brick(0, 0, 0)));
}

#[test]
fn meshes_updated_by_edits_are_exactly_what_a_fresh_mesher_makes_of_the_edited_world() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 4);
    let mut mesher = scene.mesher(3);
    let mut live = HashMap::new();
    apply(&mut live, mesher.settle(scene.world.main(), brick(0, 0, 0)));

    // Several edits, some while earlier work is still running, in different bricks including ones
    // that touch each other.
    let edits = [
        ([5, 9, 5], "base_blocks:air"),
        ([31, 9, 7], "base_blocks:dirt"),
        ([32, 9, 7], "base_blocks:air"),
        ([40, 10, 40], "base_blocks:stone"),
        ([-1, 3, -1], "base_blocks:air"),
    ];
    for (at, block) in edits {
        let changed = scene.edit(at, block);
        mesher.brick_changed(changed);
        apply(&mut live, mesher.update(scene.world.main(), brick(0, 0, 0)));
    }
    apply(&mut live, mesher.settle(scene.world.main(), brick(0, 0, 0)));

    let mut fresh = scene.mesher(3);
    let mut expected = HashMap::new();
    apply(
        &mut expected,
        fresh.settle(scene.world.main(), brick(0, 0, 0)),
    );
    assert_eq!(live.len(), expected.len());
    for (coordinate, quads) in &expected {
        assert_eq!(live.get(coordinate), Some(quads), "{coordinate}");
    }
}

#[test]
fn resetting_forgets_everything_and_meshes_another_levels_bricks_from_scratch() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 3);
    // A second level of the same ground, but with its first brick holding only a floating stone.
    let second = scene
        .world
        .add_level(
            id("test_content:second"),
            scene.game.new_level("base_terrain:flat_v1"),
        )
        .unwrap();
    let level_cube = scene
        .world
        .level(second)
        .unwrap()
        .terrain
        .assignment_cube_level();
    for cube in BrickCube::covering(BrickBox::around(brick(0, 0, 0), 3), level_cube) {
        scene
            .world
            .level_mut(second)
            .unwrap()
            .terrain
            .assign_unassigned(cube.origin());
    }
    let stone = scene.game.raw_block("base_blocks:stone");
    scene
        .game
        .set_in_level(&mut scene.world, second, [5, 20, 5], "core:block", stone);

    let mut mesher = scene.mesher(2);
    let mut live = HashMap::new();
    apply(&mut live, mesher.settle(scene.world.main(), brick(0, 0, 0)));
    let first_level_picture = live.clone();
    assert!(!live.is_empty());

    // Everything drawn is taken away, and nothing is held.
    let removals = mesher.reset();
    let mut removed: Vec<BrickCoordinate> = removals
        .iter()
        .map(|update| match update {
            MeshUpdate::Remove(coordinate) => *coordinate,
            other => panic!("{other:?}"),
        })
        .collect();
    let mut drawn: Vec<BrickCoordinate> = live.keys().copied().collect();
    removed.sort();
    drawn.sort();
    assert_eq!(removed, drawn);
    apply(&mut live, removals);
    assert!(live.is_empty());
    assert_eq!(mesher.loaded_brick_count(), 0);
    assert_eq!(mesher.pending_jobs(), 0);
    assert!(mesher.reset().is_empty(), "nothing is left to take away");

    // The same mesher then pictures the other level, as a fresh one does.
    let level = scene.world.level(second).unwrap();
    apply(&mut live, mesher.settle(level, brick(0, 0, 0)));
    let mut expected = HashMap::new();
    apply(&mut expected, scene.mesher(2).settle(level, brick(0, 0, 0)));
    assert_eq!(live, expected);
    assert_ne!(live, first_level_picture);
    assert_eq!(
        mesher.material_at(VoxelCoordinate::new(5, 20, 5)),
        Some(stone)
    );
}

#[test]
fn work_that_was_running_when_the_mesher_was_reset_does_not_leak_into_the_new_picture() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 3);
    let mut mesher = scene.mesher(3);
    // Start jobs and reset straight away, before they can have finished, then picture the same
    // level again: the answer must be the complete and ordinary picture, however the stale results
    // arrive.
    for _ in 0..20 {
        mesher.update(scene.world.main(), brick(0, 0, 0));
        mesher.reset();
    }
    let mut live = HashMap::new();
    apply(&mut live, mesher.settle(scene.world.main(), brick(0, 0, 0)));
    let mut expected = HashMap::new();
    apply(
        &mut expected,
        scene.mesher(3).settle(scene.world.main(), brick(0, 0, 0)),
    );
    assert_eq!(live, expected);
}

/// Terrain that takes its time, so work for it is still running when a test moves on.
#[derive(Debug)]
struct Slow(Arc<dyn TerrainGenerator>);

impl TerrainGenerator for Slow {
    fn generate_brick(&self, layout: &GridLayout, coordinate: BrickCoordinate) -> Brick {
        std::thread::sleep(Duration::from_millis(10));
        self.0.generate_brick(layout, coordinate)
    }
}

#[test]
fn bricks_generated_for_a_level_before_a_reset_never_reach_the_next_levels_picture() {
    // Two levels whose terrain differs (flat ground that is slow to generate, and cliffs), so work
    // for the first is still running after the viewer has moved to the second, and a brick of one
    // in the picture of the other is visible.
    let game = Game::new(&[Plugin::BaseBlocks, Plugin::FancyTerrain]);
    let generators = game.generator_registry();
    let mut slow_generators = Registry::new();
    for (_, entry) in generators.iter() {
        let inner = Arc::clone(entry.content().expect("both are available"));
        let generator: Arc<dyn TerrainGenerator> =
            if *entry.identifier() == id("base_terrain:flat_v1") {
                Arc::new(Slow(inner))
            } else {
                inner
            };
        slow_generators
            .register(entry.identifier().clone(), generator)
            .unwrap();
    }
    let default = slow_generators
        .runtime_id(&id("base_terrain:flat_v1"))
        .unwrap();
    let terrain = WorldTerrain::new(slow_generators, default, 3).unwrap();
    let layout = game.new_world().main().overrides.layout().clone();
    let mut world = WorldData::new(
        1,
        id("test_content:main"),
        LevelData::new(terrain, Grid::new(layout)),
    );
    let second = world
        .add_level(
            id("test_content:second"),
            game.new_level("fancy_terrain:cliffs_v1"),
        )
        .unwrap();
    let material = world
        .main()
        .overrides
        .layout()
        .channel_index(&id("core:block"))
        .unwrap();
    let air = game.raw_block("base_blocks:air");
    let mut scene = Scene {
        game,
        world,
        material,
        air,
    };
    scene.assign_around(brick(0, 0, 0), 5);
    let cube_level = scene.world.main().terrain.assignment_cube_level();
    for cube in BrickCube::covering(BrickBox::around(brick(0, 0, 0), 5), cube_level) {
        scene
            .world
            .level_mut(second)
            .unwrap()
            .terrain
            .assign_unassigned(cube.origin());
    }

    let mut mesher = scene.mesher(3);
    for _ in 0..3 {
        // The first level's slow work is running when the viewer moves to the second, whose work
        // is queued behind it; the first level's results then arrive while the second's are awaited.
        mesher.update(scene.world.main(), brick(0, 0, 0));
        mesher.reset();
        // Wait for the second level's brick to be drawn (however slow the machine), rather than a fixed time.
        let waited = Instant::now();
        while mesher.material_at(VoxelCoordinate::new(3, 20, 3)).is_none()
            && waited.elapsed() < Duration::from_secs(10)
        {
            mesher.update(scene.world.level(second).unwrap(), brick(0, 0, 0));
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(60));
        mesher.update(scene.world.level(second).unwrap(), brick(0, 0, 0));
        // High above the flat ground but deep inside the cliffs: whose terrain the brick holds.
        assert_eq!(
            mesher.material_at(VoxelCoordinate::new(3, 20, 3)),
            Some(scene.game.raw_block("base_blocks:stone")),
            "the second level's terrain, not the first level's"
        );
    }
    let level = scene.world.level(second).unwrap();
    let mut live = HashMap::new();
    apply(&mut live, mesher.settle(level, brick(0, 0, 0)));
    let mut expected = HashMap::new();
    apply(&mut expected, scene.mesher(3).settle(level, brick(0, 0, 0)));
    assert_eq!(live, expected);
}

/// A hook that lights one brick as bright as `brightness` and leaves the rest dark.
#[derive(Debug)]
struct LitBrick {
    lit: BrickCoordinate,
    brightness: u32,
}

impl BrickLight for LitBrick {
    fn brightness(&self, _brick: &Brick, coordinate: BrickCoordinate) -> Option<ChannelStorage> {
        Some(ChannelStorage::uniform(if coordinate == self.lit {
            self.brightness
        } else {
            0
        }))
    }
}

#[test]
fn the_brightness_a_hook_gives_is_what_a_face_looking_into_another_brick_is_drawn_with() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 4);
    let hook = Arc::new(LitBrick {
        lit: brick(0, 0, 0),
        brightness: 100,
    });
    let mut mesher = scene.mesher_seeing(3, scene.channels_lit_by(&hook));
    // Dig out the top voxel at the +x edge of brick (0, 0, 0): the wall that brick (1, 0, 0) now has
    // faces the hole, so it is lit by the air in brick (0, 0, 0), a neighbour's, not by its own.
    scene.edit([31, 9, 5], "base_blocks:air");
    let mut live = HashMap::new();
    apply(&mut live, mesher.settle(scene.world.main(), brick(0, 0, 0)));

    let wall = |coordinate: BrickCoordinate| {
        unpacked(&live[&coordinate])
            .into_iter()
            .find(|quad| quad.face == FaceDirection::NegativeX && quad.origin == [0, 9, 5])
            .map(|quad| quad.light)
    };
    assert_eq!(wall(brick(1, 0, 0)), Some(100), "the neighbour's air");
    // And a top face is lit by the air above it, in its own brick: dark in brick (1, 0, 0), lit in (0, 0, 0).
    let top_light = |coordinate: BrickCoordinate| {
        unpacked(&live[&coordinate])
            .into_iter()
            .find(|quad| quad.face == FaceDirection::PositiveY)
            .map(|quad| quad.light)
    };
    assert_eq!(top_light(brick(0, 0, 0)), Some(100));
    assert_eq!(top_light(brick(1, 0, 0)), Some(0));
}

#[test]
fn the_view_is_meshed_again_when_the_hook_is_another_one_and_not_when_it_is_the_same() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 4);
    let hook = Arc::new(LitBrick {
        lit: brick(0, 0, 0),
        brightness: 100,
    });
    let mut mesher = scene.mesher_seeing(2, scene.channels_lit_by(&hook));
    let mut live = HashMap::new();
    apply(&mut live, mesher.settle(scene.world.main(), brick(0, 0, 0)));
    let light_of_origin = |live: &HashMap<BrickCoordinate, PackedBrickMesh>| {
        unpacked(&live[&brick(0, 0, 0)])
            .iter()
            .map(|quad| quad.light)
            .max()
    };
    assert_eq!(light_of_origin(&live), Some(100));

    // The same hook again (a game asks for the view's channels whenever it may have changed): nothing to do.
    mesher.set_channels(scene.channels_lit_by(&hook));
    assert!(
        mesher.settle(scene.world.main(), brick(0, 0, 0)).is_empty(),
        "the same hook is not a change"
    );

    // Another hook that says something else: every brick is drawn again with it.
    let brighter = Arc::new(LitBrick {
        lit: brick(0, 0, 0),
        brightness: 200,
    });
    mesher.set_channels(scene.channels_lit_by(&brighter));
    apply(&mut live, mesher.settle(scene.world.main(), brick(0, 0, 0)));
    assert_eq!(light_of_origin(&live), Some(200));
}
