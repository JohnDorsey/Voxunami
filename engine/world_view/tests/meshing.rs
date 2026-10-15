//! The mesher against worlds of flat ground: what it loads, meshes, unloads and re-meshes.

use std::collections::HashMap;

use region_octree::BrickCube;
use voxel_grid::ChannelIndex;
use voxel_meshing::{PackedQuad, Quad};
use voxel_space::{BrickBox, BrickCoordinate, FaceDirection, VoxelCoordinate};
use world_storage::WorldData;
use world_storage::test_support::{Game, Plugin, brick, id};
use world_view::{MeshUpdate, ViewSettings, WorldMesher};

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
        let level = self.world.terrain.assignment_cube_level();
        for cube in BrickCube::covering(BrickBox::around(center, radius), level) {
            self.world.terrain.assign_unassigned(cube.origin());
        }
    }

    fn mesher(&self, radius: i64) -> WorldMesher {
        let air = self.air;
        WorldMesher::new(
            ViewSettings {
                view_radius_bricks: radius,
                worker_count: 3,
            },
            self.material,
            move |value| value != air,
        )
    }

    fn edit(&mut self, at: [i64; 3], block: &str) -> BrickCoordinate {
        let coordinate = VoxelCoordinate::new(at[0], at[1], at[2]);
        let (brick_coordinate, local) = coordinate.split();
        if self.world.overrides.brick(brick_coordinate).is_none() {
            let world_storage::BrickSource::Generated(generated) =
                self.world.effective_brick(brick_coordinate)
            else {
                panic!("the region must be assigned first");
            };
            self.world
                .overrides
                .insert_brick(brick_coordinate, std::sync::Arc::new(generated))
                .unwrap();
        }
        self.world
            .overrides
            .brick_mut(brick_coordinate)
            .unwrap()
            .set(self.material, local, self.game.raw_block(block));
        brick_coordinate
    }
}

/// Applies updates the way a renderer would.
fn apply(live: &mut HashMap<BrickCoordinate, Vec<PackedQuad>>, updates: Vec<MeshUpdate>) {
    for update in updates {
        match update {
            MeshUpdate::Upload { coordinate, quads } if quads.is_empty() => {
                live.remove(&coordinate);
            }
            MeshUpdate::Upload { coordinate, quads } => {
                live.insert(coordinate, quads);
            }
            MeshUpdate::Remove(coordinate) => {
                live.remove(&coordinate);
            }
        }
    }
}

fn unpacked(quads: &[PackedQuad]) -> Vec<Quad> {
    quads.iter().map(|quad| quad.unpack()).collect()
}

#[test]
fn flat_ground_streams_in_exactly_one_top_quad_per_surface_brick() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 6);
    let mut mesher = scene.mesher(3);
    let mut live = HashMap::new();
    apply(&mut live, mesher.settle(&scene.world, brick(0, 0, 0)));

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
                material: grass
            }],
            "{coordinate}"
        );
    }
}

#[test]
fn nothing_is_loaded_until_the_world_says_which_generator_made_the_terrain() {
    let mut scene = scene();
    let mut mesher = scene.mesher(2);
    assert!(mesher.settle(&scene.world, brick(0, 0, 0)).is_empty());
    assert_eq!(mesher.loaded_brick_count(), 0);
    assert_eq!(mesher.material_at(VoxelCoordinate::new(0, 0, 0)), None);

    scene.assign_around(brick(0, 0, 0), 4);
    let updates = mesher.settle(&scene.world, brick(0, 0, 0));
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
    mesher.settle(&scene.world, brick(0, 0, 0));
    assert_eq!(mesher.pending_jobs(), 0);
    assert!(mesher.update(&scene.world, brick(0, 0, 0)).is_empty());
}

#[test]
fn moving_away_unloads_old_bricks_and_loads_new_ones() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 20);
    let mut mesher = scene.mesher(3);
    let mut live = HashMap::new();
    apply(&mut live, mesher.settle(&scene.world, brick(0, 0, 0)));
    let loaded_before = mesher.loaded_brick_count();
    assert_eq!(live.len(), 29);

    apply(&mut live, mesher.settle(&scene.world, brick(10, 0, 0)));

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
    assert!(mesher.settle(&scene.world, brick(0, 40, 0)).is_empty());
}

#[test]
fn dropping_the_mesher_stops_its_workers() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 4);
    let mut mesher = scene.mesher(2);
    mesher.update(&scene.world, brick(0, 0, 0));
    drop(mesher); // must not hang
}

#[test]
fn an_edit_changes_the_mesh_of_its_brick_and_of_the_neighbour_whose_face_it_exposes() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 4);
    let mut mesher = scene.mesher(3);
    let mut live = HashMap::new();
    apply(&mut live, mesher.settle(&scene.world, brick(0, 0, 0)));
    let before_neighbour = live[&brick(1, 0, 0)].clone();

    // Dig out the top voxel at the +x edge of brick (0, 0, 0): its east neighbour now has a wall
    // facing the hole.
    let edited = scene.edit([31, 9, 5], "base_blocks:air");
    assert_eq!(edited, brick(0, 0, 0));
    mesher.brick_changed(edited);
    apply(&mut live, mesher.settle(&scene.world, brick(0, 0, 0)));

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
    apply(&mut live, mesher.settle(&scene.world, brick(0, 0, 0)));
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
    let updates = mesher.settle(&scene.world, brick(0, 0, 0));
    assert!(updates.iter().any(|update| matches!(
        update,
        MeshUpdate::Upload { coordinate, quads } if *coordinate == brick(0, 0, 0) && quads.is_empty()
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
    apply(&mut live, mesher.settle(&scene.world, brick(0, 0, 0)));

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
        apply(&mut live, mesher.update(&scene.world, brick(0, 0, 0)));
    }
    apply(&mut live, mesher.settle(&scene.world, brick(0, 0, 0)));

    let mut fresh = scene.mesher(3);
    let mut expected = HashMap::new();
    apply(&mut expected, fresh.settle(&scene.world, brick(0, 0, 0)));
    assert_eq!(live.len(), expected.len());
    for (coordinate, quads) in &expected {
        assert_eq!(live.get(coordinate), Some(quads), "{coordinate}");
    }
}
