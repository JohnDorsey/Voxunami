//! The mesher against worlds of flat ground: what it loads, meshes, unloads and re-meshes.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use block_models::BlockTable;
use content_registry::Registry;
use terrain::{TerrainGenerator, WorldTerrain};

use region_octree::BrickCube;
use voxel_grid::{Brick, ChannelIndex, Grid, GridLayout};
use voxel_meshing::{FULL_LIGHT, PackedBrickMesh, Quad};
use voxel_space::{BrickBox, BrickCoordinate, FaceDirection, VoxelCoordinate};
use world_storage::test_support::{Game, Plugin, brick, id};
use world_storage::{LevelData, WorldData};
use world_view::{MeshUpdate, ViewChannels, ViewSettings, WorldMesher};

struct Scene {
    game: Game,
    world: WorldData,
    material: ChannelIndex,
    air: u32,
}

/// Flat ground: stone below, three layers of dirt, grass at height 10 (the top voxel is y = 9).
fn scene() -> Scene {
    let game = Game::new(&[Plugin::BaseBlocks]);
    let world = game.new_world();
    let material = world
        .main()
        .overrides
        .layout()
        .channel_index(&id("core:block"))
        .unwrap();
    let air = game.raw_block("base_blocks:air");
    Scene {
        game,
        world,
        material,
        air,
    }
}

impl Scene {
    /// What the server does when a region is watched: gives its terrain a generator.
    fn assign_around(&mut self, center: BrickCoordinate, radius: u32) {
        let level = self.world.main().terrain.assignment_cube_level();
        for cube in BrickCube::covering(BrickBox::around(center, radius), level) {
            self.world
                .main_mut()
                .terrain
                .assign_unassigned(cube.origin());
        }
    }

    fn mesher(&self, radius: i64) -> WorldMesher {
        // Air is see-through; every other block of the fixtures is an opaque cube.
        let mut opaque = vec![true; 16];
        opaque[self.air as usize] = false;
        WorldMesher::new(
            ViewSettings {
                view_radius_bricks: radius,
                worker_count: 3,
            },
            ViewChannels::new(self.material, None, None),
            Arc::new(BlockTable::of_cubes(&opaque)),
        )
    }

    fn edit(&mut self, at: [i64; 3], block: &str) -> BrickCoordinate {
        let coordinate = VoxelCoordinate::new(at[0], at[1], at[2]);
        let (brick_coordinate, local) = coordinate.split();
        if self
            .world
            .main()
            .overrides
            .brick(brick_coordinate)
            .is_none()
        {
            let world_storage::BrickSource::Generated(generated) =
                self.world.main().effective_brick(brick_coordinate)
            else {
                panic!("the region must be assigned first");
            };
            self.world
                .main_mut()
                .overrides
                .insert_brick(brick_coordinate, std::sync::Arc::new(generated))
                .unwrap();
        }
        self.world
            .main_mut()
            .overrides
            .brick_mut(brick_coordinate)
            .unwrap()
            .set(self.material, local, self.game.raw_block(block));
        brick_coordinate
    }
}

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
        mesher.update(scene.world.level(second).unwrap(), brick(0, 0, 0));
        std::thread::sleep(Duration::from_millis(120));
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
