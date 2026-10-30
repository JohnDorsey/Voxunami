use std::sync::atomic::{AtomicUsize, Ordering};

use terrain::{TerrainGenerator, WorldTerrain};
use voxel_grid::{Grid, GridLayout};
use voxel_space::voxel_in_other;

use crate::GridView;
use protocol::ServerMessage;
use voxel_space::BrickBox;
use world_storage::test_support::{Game, Plugin, id, main_level_id};
use world_storage::{GridTerrain, LevelData};

use super::*;

/// A generator that counts how many bricks it is asked for.
#[derive(Debug)]
struct Counting {
    inner: Arc<dyn TerrainGenerator>,
    calls: Arc<AtomicUsize>,
}

impl TerrainGenerator for Counting {
    fn generate_brick(&self, layout: &GridLayout, coordinate: BrickCoordinate) -> Brick {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.inner.generate_brick(layout, coordinate)
    }
}

/// A level of flat ground whose generator counts its calls.
fn counting_level(game: &Game, calls: &Arc<AtomicUsize>) -> LevelData {
    counting_level_with_missing(game, calls, &[])
}

/// The same, with generators in the table that are placeholders for plugins that are not here.
fn counting_level_with_missing(
    game: &Game,
    calls: &Arc<AtomicUsize>,
    missing: &[&NamespacedId],
) -> LevelData {
    let flat = game.generator_registry();
    let inner = Arc::clone(
        flat.entry(flat.runtime_id(&id("base_terrain:flat_v1")).unwrap())
            .unwrap()
            .content()
            .unwrap(),
    );
    let counting: Arc<dyn TerrainGenerator> = Arc::new(Counting {
        inner,
        calls: Arc::clone(calls),
    });
    let mut generators = content_registry::Registry::new();
    let default = generators
        .register(id("base_terrain:flat_v1"), counting)
        .unwrap();
    for identifier in missing {
        generators.register_missing((*identifier).clone());
    }
    let terrain = WorldTerrain::new(generators, default, 3).unwrap();
    let layout = Arc::new(GridLayout::new(game.channel_definitions()).unwrap());
    LevelData::new(terrain, Grid::new(layout))
}

pub(crate) fn block_channel(level: &LevelData) -> ChannelIndex {
    level
        .overrides
        .layout()
        .channel_index(&id("core:block"))
        .unwrap()
}

fn counting_world() -> (ServerWorld, ChannelIndex, Arc<AtomicUsize>) {
    let game = Game::new(&[Plugin::BaseBlocks]);
    let calls = Arc::new(AtomicUsize::new(0));
    let level = counting_level(&game, &calls);
    let channel = block_channel(&level);
    let data = WorldData::new(1, main_level_id(), level);
    (ServerWorld::new(data, Arc::default()), channel, calls)
}

#[test]
fn the_world_counts_the_times_the_generator_ran() {
    let (mut world, channel, calls) = counting_world();
    assert_eq!(world.bricks_generated(), 0);
    for x in 0..32 {
        world
            .grid(GridId::root(LevelIndex::MAIN))
            .unwrap()
            .get(channel, VoxelCoordinate::new(x, 2, 3))
            .unwrap();
    }
    assert_eq!(world.bricks_generated(), 1);
    world
        .grid(GridId::root(LevelIndex::MAIN))
        .unwrap()
        .get(channel, VoxelCoordinate::new(40, 2, 3))
        .unwrap();
    // The generator's own count, and the world's, are the same.
    assert_eq!(world.bricks_generated(), 2);
    assert_eq!(
        calls.load(Ordering::Relaxed) as u64,
        world.bricks_generated()
    );
    // A brick that is read again from the cache is not made again.
    world
        .grid(GridId::root(LevelIndex::MAIN))
        .unwrap()
        .get(channel, VoxelCoordinate::new(41, 2, 3))
        .unwrap();
    assert_eq!(world.bricks_generated(), 2);
}

#[test]
fn reading_terrain_nobody_has_edited_generates_each_brick_once() {
    let (mut world, channel, calls) = counting_world();
    for x in 0..32 {
        world
            .grid(GridId::root(LevelIndex::MAIN))
            .unwrap()
            .get(channel, VoxelCoordinate::new(x, 2, 3))
            .unwrap();
    }
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    world
        .grid(GridId::root(LevelIndex::MAIN))
        .unwrap()
        .get(channel, VoxelCoordinate::new(40, 2, 3))
        .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

#[test]
fn editing_after_reading_uses_the_remembered_brick_and_changes_only_what_was_set() {
    let (mut world, channel, calls) = counting_world();
    let level = LevelIndex::MAIN;
    let at = VoxelCoordinate::new(5, 5, 5);
    let neighbour = VoxelCoordinate::new(6, 5, 5);
    let before = world
        .grid(GridId::root(level))
        .unwrap()
        .get(channel, neighbour)
        .unwrap();
    let original = world
        .grid(GridId::root(level))
        .unwrap()
        .get(channel, at)
        .unwrap();
    assert!(
        world
            .grid(GridId::root(level))
            .unwrap()
            .set(channel, at, original + 1)
            .unwrap()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        world
            .grid(GridId::root(level))
            .unwrap()
            .get(channel, at)
            .unwrap(),
        original + 1
    );
    assert_eq!(
        world
            .grid(GridId::root(level))
            .unwrap()
            .get(channel, neighbour)
            .unwrap(),
        before
    );
    // The remembered brick is not changed by the edit: setting the original value back is
    // a change again only if the edit really happened.
    assert!(
        world
            .grid(GridId::root(level))
            .unwrap()
            .set(channel, at, original)
            .unwrap()
    );
}

#[test]
fn levels_are_separate_worlds_with_their_own_terrain_and_edits_and_unknown_ones_are_errors() {
    let game = Game::new(&[Plugin::BaseBlocks]);
    let first_calls = Arc::new(AtomicUsize::new(0));
    let second_calls = Arc::new(AtomicUsize::new(0));
    let level = counting_level(&game, &first_calls);
    let channel = block_channel(&level);
    let mut data = WorldData::new(1, main_level_id(), level);
    let nether_id = id("test_content:nether");
    let nether = data
        .add_level(nether_id.clone(), counting_level(&game, &second_calls))
        .unwrap();
    let mut world = ServerWorld::new(data, Arc::default());
    assert_eq!(world.level_index(&nether_id), Some(nether));
    assert_eq!(world.level_index(&id("test_content:nowhere")), None);

    let at = VoxelCoordinate::new(1, 1, 1);
    let ground = world
        .grid(GridId::root(LevelIndex::MAIN))
        .unwrap()
        .get(channel, at)
        .unwrap();
    // Editing one level leaves the other as it was, and each remembers its own bricks.
    assert!(
        world
            .grid(GridId::root(nether))
            .unwrap()
            .set(channel, at, ground + 1)
            .unwrap()
    );
    assert_eq!(
        world
            .grid(GridId::root(nether))
            .unwrap()
            .get(channel, at)
            .unwrap(),
        ground + 1
    );
    assert_eq!(
        world
            .grid(GridId::root(LevelIndex::MAIN))
            .unwrap()
            .get(channel, at)
            .unwrap(),
        ground
    );
    assert_eq!(world.data().main().overrides.brick_count(), 0);
    assert_eq!(
        world.data().level(nether).unwrap().overrides.brick_count(),
        1
    );
    assert_eq!(
        world.edits.keys().copied().collect::<Vec<_>>(),
        [(GridId::root(nether), at.split().0)]
    );
    assert_eq!(
        (
            first_calls.load(Ordering::Relaxed),
            second_calls.load(Ordering::Relaxed)
        ),
        (1, 1)
    );

    let nowhere = LevelIndex::new(9);
    // (A level that is not there has no terrain to read or to change.)
    assert_eq!(
        world.grid(GridId::root(nowhere)).err(),
        Some(AccessError::NoSuchLevel(nowhere))
    );
}

#[test]
fn replies_are_cut_to_the_longest_a_message_may_be_without_splitting_a_character() {
    let (mut world, _, _) = counting_world();
    let client = ClientId::new(1);
    world.reply(client, "short");
    let long = "é".repeat(MAXIMUM_COMMAND_LENGTH); // two bytes each
    world.reply(client, &long);
    assert_eq!(world.replies[0], (client, "short".to_owned()));
    assert_eq!(world.replies[1].1.len(), MAXIMUM_COMMAND_LENGTH);
    assert!(world.replies[1].1.chars().all(|character| character == 'é'));
    let odd = format!("{}é", "a".repeat(MAXIMUM_COMMAND_LENGTH - 1));
    world.reply(client, &odd);
    assert_eq!(world.replies[2].1, "a".repeat(MAXIMUM_COMMAND_LENGTH - 1));
}

#[test]
fn a_ship_has_voxels_of_its_own_that_start_at_the_default_and_are_not_the_terrain() {
    let (mut world, block, _) = counting_world();
    let ship = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    assert_eq!(ship.root_level(), None);
    let at = VoxelCoordinate::new(1, 2, 3);
    let default = world.data().main().overrides.layout().default_value(block);
    let terrain = world
        .grid(GridId::root(LevelIndex::MAIN))
        .unwrap()
        .get(block, at)
        .unwrap();
    assert_eq!(world.grid(ship).unwrap().get(block, at), Ok(default));

    assert_eq!(world.grid(ship).unwrap().set(block, at, 7), Ok(true));
    assert_eq!(world.grid(ship).unwrap().get(block, at), Ok(7));
    assert_eq!(world.grid(ship).unwrap().set(block, at, 7), Ok(false));
    // The terrain at the same coordinates is as it was, and nothing was generated for the ship.
    assert_eq!(
        world
            .grid(GridId::root(LevelIndex::MAIN))
            .unwrap()
            .get(block, at)
            .unwrap(),
        terrain
    );
    assert_eq!(world.data().main().overrides.brick_count(), 0);

    // One edit of one brick of the ship, which was made by it.
    let edit = &world.edits[&(ship, at.split().0)];
    assert!(edit.created);
    assert_eq!(edit.changes.len(), 1);
    assert_eq!(world.edits.len(), 1);
}

#[test]
fn the_changes_a_ship_makes_to_a_watched_channel_say_which_grid() {
    let (mut world, block, _) = counting_world();
    let ship = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    world.watch_channels(BTreeSet::from([block]));
    let at = VoxelCoordinate::new(40, -3, 9);
    world.grid(ship).unwrap().set(block, at, 2).unwrap();
    world
        .grid(GridId::root(LevelIndex::MAIN))
        .unwrap()
        .set(block, at, 4)
        .unwrap();
    assert_eq!(
        world.take_changes(),
        [
            ChannelChange {
                grid: ship,
                channel: block,
                voxel: at
            },
            ChannelChange {
                grid: GridId::MAIN,
                channel: block,
                voxel: at
            },
        ]
    );
}

#[test]
fn a_grid_that_does_not_exist_is_an_error_and_so_is_a_ship_in_a_level_that_does_not() {
    let (mut world, block, _) = counting_world();
    let gone = GridId::new(GridId::FIRST_ALLOCATED + 9);
    assert_eq!(world.grid(gone).err(), Some(AccessError::NoSuchGrid(gone)));
    let nowhere = LevelIndex::new(7);
    assert_eq!(
        world.create_grid(nowhere, GridPose::identity()),
        Err(AccessError::NoSuchLevel(nowhere))
    );
    let ship = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    world.data_mut().grids_mut().remove(ship);
    assert!(world.grid(ship).is_err());
    assert_eq!(
        world.set_moving_voxel(ship, block, VoxelCoordinate::ORIGIN, 1),
        Err(AccessError::NoSuchGrid(ship))
    );
}

#[test]
fn detaching_blocks_moves_them_out_of_the_terrain_into_a_ship_exactly_where_they_were() {
    let (mut world, block, _) = counting_world();
    let level = LevelIndex::MAIN;
    let default = world.data().main().overrides.layout().default_value(block);
    // Across a brick boundary on every axis, with negative coordinates; given twice and out of order.
    let voxels = [
        VoxelCoordinate::new(2, 5, 40),
        VoxelCoordinate::new(-1, 5, 31),
        VoxelCoordinate::new(0, 6, 32),
        VoxelCoordinate::new(-1, 5, 31),
    ];
    for (index, voxel) in voxels.iter().enumerate() {
        world
            .grid(GridId::root(level))
            .unwrap()
            .set(block, *voxel, 10 + index as u32)
            .unwrap();
    }
    let bystander = VoxelCoordinate::new(1, 5, 31);
    world
        .grid(GridId::root(level))
        .unwrap()
        .set(block, bystander, 99)
        .unwrap();
    // A second channel travels too.
    let temperature = world
        .data()
        .main()
        .overrides
        .layout()
        .channel_index(&id("core:temperature"))
        .unwrap();
    let normal = world
        .data()
        .main()
        .overrides
        .layout()
        .default_value(temperature);
    world
        .grid(GridId::root(level))
        .unwrap()
        .set(temperature, voxels[2], 33)
        .unwrap();
    world.edits.clear();

    let ship = world.detach_blocks(GridId::root(level), &voxels).unwrap();

    // The terrain there is empty now, the bystander is as it was, and the edits are the ordinary ones of the terrain.
    for voxel in &voxels {
        assert_eq!(
            world
                .grid(GridId::root(level))
                .unwrap()
                .get(block, *voxel)
                .unwrap(),
            default
        );
    }
    assert_eq!(
        world
            .grid(GridId::root(level))
            .unwrap()
            .get(block, bystander)
            .unwrap(),
        99
    );
    assert_eq!(
        world
            .grid(GridId::root(level))
            .unwrap()
            .get(temperature, voxels[2])
            .unwrap(),
        normal
    );
    assert!(world.edits.keys().any(|(grid, _)| *grid == GridId::MAIN));
    // The ship has the blocks where the box of the voxels starts at its own origin ...
    let moving = world.data().grids().get(ship).unwrap();
    assert_eq!(moving.level, level);
    assert_eq!(moving.pose.orientation, Orientation::IDENTITY);
    let (low_brick, low_local) = VoxelCoordinate::new(-1, 5, 31).split();
    assert_eq!(
        moving.pose.position,
        WorldPosition::new(
            low_brick,
            [
                low_local.x() as f32,
                low_local.y() as f32,
                low_local.z() as f32
            ]
        )
    );
    let at = |x, y, z| moving.voxels.get(block, VoxelCoordinate::new(x, y, z));
    assert_eq!(at(3, 0, 9), 10); // (2, 5, 40)
    assert_eq!(at(0, 0, 0), 11 + 2); // (-1, 5, 31) was set last, by its second listing
    assert_eq!(at(1, 1, 1), 12); // (0, 6, 32)
    assert_eq!(
        moving
            .voxels
            .get(temperature, VoxelCoordinate::new(1, 1, 1)),
        33
    );
    assert_eq!(
        moving.voxels.get(block, VoxelCoordinate::new(2, 0, 0)),
        default
    );
    // ... and the ship's voxels are in the world exactly where they were, to the voxel.
    for (voxel, ship_voxel) in [
        (voxels[0], VoxelCoordinate::new(3, 0, 9)),
        (voxels[2], VoxelCoordinate::new(1, 1, 1)),
    ] {
        assert_eq!(
            voxel_in_other(&moving.pose, ship_voxel, &GridPose::identity()),
            Some(voxel)
        );
    }
}

#[test]
fn the_generated_value_of_a_voxel_is_what_the_terrain_made_and_not_what_was_built_there() {
    let (mut world, block, _) = counting_world();
    let level = LevelIndex::MAIN;
    let at = VoxelCoordinate::new(5, 7, 9);
    let made = world
        .grid(GridId::root(level))
        .unwrap()
        .get(block, at)
        .unwrap();
    world
        .grid(GridId::root(level))
        .unwrap()
        .set(block, at, made + 5)
        .unwrap();
    assert_eq!(
        world
            .grid(GridId::root(level))
            .unwrap()
            .get(block, at)
            .unwrap(),
        made + 5
    );
    assert_eq!(
        world
            .grid(GridId::root(level))
            .unwrap()
            .generated(block, at)
            .unwrap(),
        made
    );
    // In a brick nobody edited, and in a brick that was edited somewhere else in it, it is the same as the voxel.
    let beside = VoxelCoordinate::new(6, 7, 9);
    let far = VoxelCoordinate::new(200, 7, 9);
    for voxel in [beside, far] {
        assert_eq!(
            world
                .grid(GridId::root(level))
                .unwrap()
                .generated(block, voxel)
                .unwrap(),
            world
                .grid(GridId::root(level))
                .unwrap()
                .get(block, voxel)
                .unwrap()
        );
    }
    let nowhere = LevelIndex::new(9);
    assert_eq!(
        world.grid(GridId::root(nowhere)).err(),
        Some(AccessError::NoSuchLevel(nowhere))
    );
}

#[test]
fn the_generated_value_of_a_voxel_of_an_edited_brick_is_found_after_the_cache_has_forgotten_the_brick()
 {
    // (This panicked: the cache remembers the bricks generated lately, and a brick that was edited long ago is not in it.)
    let (mut world, block, _) = counting_world();
    let level = LevelIndex::MAIN;
    let at = VoxelCoordinate::new(5, 7, 9);
    let made = world
        .grid(GridId::root(level))
        .unwrap()
        .get(block, at)
        .unwrap();
    world
        .grid(GridId::root(level))
        .unwrap()
        .set(block, at, made + 5)
        .unwrap();
    // Many other bricks are read, enough to push it out of the cache of the most recent.
    for x in 1..=2000 {
        world
            .grid(GridId::root(level))
            .unwrap()
            .get(block, VoxelCoordinate::new(32 * x, 7, 9))
            .unwrap();
    }
    assert_eq!(
        world
            .grid(GridId::root(level))
            .unwrap()
            .get(block, at)
            .unwrap(),
        made + 5
    );
    assert_eq!(
        world
            .grid(GridId::root(level))
            .unwrap()
            .generated(block, at)
            .unwrap(),
        made
    );
}

#[test]
fn detaching_nothing_or_from_a_level_that_is_not_there_is_refused_and_changes_nothing() {
    let (mut world, block, _) = counting_world();
    assert_eq!(
        world.detach_blocks(GridId::MAIN, &[]),
        Err(DetachError::NoVoxels)
    );
    let nowhere = LevelIndex::new(3);
    assert_eq!(
        world.detach_blocks(GridId::root(nowhere), &[VoxelCoordinate::ORIGIN]),
        Err(DetachError::Access(AccessError::NoSuchLevel(nowhere)))
    );
    assert!(world.data().grids().is_empty());
    assert!(world.edits.is_empty());
    let _ = block;
}

#[test]
fn forces_held_ships_and_speeds_are_for_ships_that_exist_and_forces_are_taken_once_in_order() {
    let (mut world, _, _) = counting_world();
    let ship = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    let gone = GridId::new(GridId::FIRST_ALLOCATED + 20);
    assert_eq!(
        world.apply_grid_force(gone, [1.0; 3], None),
        Err(AccessError::NoSuchGrid(gone))
    );
    assert_eq!(
        world.apply_grid_torque(gone, [1.0; 3]),
        Err(AccessError::NoSuchGrid(gone))
    );
    assert_eq!(
        world.set_grid_held(gone, true),
        Err(AccessError::NoSuchGrid(gone))
    );
    assert_eq!(
        world.set_grid_velocity(gone, [1.0; 3], [2.0; 3]),
        Err(AccessError::NoSuchGrid(gone))
    );
    assert_eq!(
        world.set_grid_pose(gone, GridPose::identity()),
        Err(AccessError::NoSuchGrid(gone))
    );
    world.apply_grid_force(ship, [1.0, 2.0, 3.0], None).unwrap();
    world
        .apply_grid_force(ship, [4.0, 5.0, 6.0], Some([7.0, 8.0, 9.0]))
        .unwrap();
    assert_eq!(
        world.take_grid_forces(),
        [
            GridForce {
                grid: ship,
                force: [1.0, 2.0, 3.0],
                at: None
            },
            GridForce {
                grid: ship,
                force: [4.0, 5.0, 6.0],
                at: Some([7.0, 8.0, 9.0])
            },
        ]
    );
    assert!(world.take_grid_forces().is_empty());
    world.apply_grid_torque(ship, [1.0, 2.0, 3.0]).unwrap();
    world.apply_grid_torque(ship, [4.0, 5.0, 6.0]).unwrap();
    assert_eq!(
        world.take_grid_torques(),
        [
            GridTorque {
                grid: ship,
                torque: [1.0, 2.0, 3.0]
            },
            GridTorque {
                grid: ship,
                torque: [4.0, 5.0, 6.0]
            },
        ]
    );
    assert!(world.take_grid_torques().is_empty());
    // A ship that is moving and is then held has stopped: nothing is left on record of how fast it went.
    world
        .set_grid_velocity(ship, [7.0, 8.0, 9.0], [1.0, 1.0, 1.0])
        .unwrap();
    world.set_grid_held(ship, true).unwrap();
    let held = world.data().grids().get(ship).unwrap();
    assert!(held.held);
    assert_eq!(
        (held.linear_velocity, held.angular_velocity),
        ([0.0; 3], [0.0; 3])
    );
    // Whoever then moves a held ship says how fast (it is kinematic: it goes where it is put), and holding it again changes nothing.
    world
        .set_grid_velocity(ship, [1.0, 2.0, 3.0], [4.0, 5.0, 6.0])
        .unwrap();
    world.set_grid_held(ship, true).unwrap();
    let moving = world.data().grids().get(ship).unwrap();
    assert!(moving.held);
    assert_eq!(moving.linear_velocity, [1.0, 2.0, 3.0]);
    assert_eq!(moving.angular_velocity, [4.0, 5.0, 6.0]);
    // Let go, it is a ship like any other, and whatever speed it had is the speed it goes on with.
    world.set_grid_held(ship, false).unwrap();
    let free = world.data().grids().get(ship).unwrap();
    assert!(!free.held);
    assert_eq!(free.linear_velocity, [1.0, 2.0, 3.0]);
}

fn at(x: f32, y: f32, z: f32) -> WorldPosition {
    WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z])
}

// ---- rails ----

use world_storage::rail::{Centre, Orbit, Rail, RailTime, Spin};

/// A world with one level and ships to put on rails.
fn world_for_rails() -> ServerWorld {
    let game = Game::new(&[Plugin::BaseBlocks]);
    let calls = Arc::new(AtomicUsize::new(0));
    let data = WorldData::new(1, main_level_id(), counting_level(&game, &calls));
    ServerWorld::new(data, Arc::default())
}

fn orbit_about(centre: Centre, radius: f64, period: f64) -> Rail {
    Rail {
        spin: None,
        orbit: Some(Orbit::new(centre, [0.0, 1.0, 0.0], radius, period, 0.0).unwrap()),
    }
}

#[test]
fn a_ship_on_a_rail_is_put_where_the_rail_says_at_the_time_of_the_world_every_tick() {
    let mut world = world_for_rails();
    let planet = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    let rail = Rail {
        spin: Some(Spin::new([0.0, 1.0, 0.0], 3600.0, 0.5).unwrap()),
        orbit: Some(
            Orbit::new(
                Centre::Point([1000.0, 0.0, -50.0]),
                [0.0, 1.0, 0.0],
                2.0e6,
                40_000.0,
                1.0,
            )
            .unwrap(),
        ),
    };
    world.set_grid_rail(planet, Some(rail)).unwrap();
    let mut seen = Vec::new();
    for _ in 0..3 {
        // (A tick is a twentieth of a second at this rate.)
        world.data.advance_clock(7);
        world.apply_rails(20);
        let time = RailTime::of_tick(world.world_tick(), 20);
        let state = rail.state(time, [1000.0, 0.0, -50.0], [0.0; 3]);
        let ship = world.data().grids().get(planet).unwrap();
        assert_eq!(
            ship.pose.position,
            WorldPosition::from_voxels(state.position.unwrap())
        );
        assert_eq!(ship.pose.orientation, state.orientation.unwrap());
        assert_eq!(
            ship.linear_velocity,
            state.velocity.map(|component| component as f32)
        );
        assert_eq!(
            ship.angular_velocity,
            state.angular_velocity.map(|component| component as f32)
        );
        seen.push(ship.pose.position);
    }
    // It moved, and does not stand where it began.
    assert!(seen[0] != seen[1] && seen[1] != seen[2]);
}

#[test]
fn the_time_of_a_rail_is_the_clock_over_the_rate_of_the_ticks() {
    let mut world = world_for_rails();
    let planet = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    let rail = orbit_about(Centre::Point([0.0; 3]), 1000.0, 100.0);
    world.set_grid_rail(planet, Some(rail)).unwrap();
    // 30 ticks at 30 a second is one second, and at 20 a second a second and a half.
    world.data.set_clock(30);
    world.apply_rails(30);
    let at_thirty = world.data().grids().get(planet).unwrap().pose.position;
    assert_eq!(
        at_thirty,
        WorldPosition::from_voxels(rail.state(1.0, [0.0; 3], [0.0; 3]).position.unwrap())
    );
    world.apply_rails(20);
    let at_twenty = world.data().grids().get(planet).unwrap().pose.position;
    assert_eq!(
        at_twenty,
        WorldPosition::from_voxels(rail.state(1.5, [0.0; 3], [0.0; 3]).position.unwrap())
    );
}

#[test]
fn a_ship_that_goes_round_another_is_where_the_other_is_and_moves_with_it() {
    let mut world = world_for_rails();
    let star_point = [5000.0, 0.0, 0.0];
    let planet = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    let moon = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    assert!(
        planet < moon,
        "the planet is made first, so its place is worked out first"
    );
    world
        .set_grid_rail(
            planet,
            Some(orbit_about(Centre::Point(star_point), 1.0e6, 40_000.0)),
        )
        .unwrap();
    world
        .set_grid_rail(moon, Some(orbit_about(Centre::Grid(planet), 2.0e4, 500.0)))
        .unwrap();
    for ticks in [0u64, 333, 12_345] {
        world.data.set_clock(ticks);
        world.apply_rails(20);
        let planet_ship = world.data().grids().get(planet).unwrap();
        let moon_ship = world.data().grids().get(moon).unwrap();
        let (planet_at, moon_at) = (
            planet_ship.pose.position.in_voxels(),
            moon_ship.pose.position.in_voxels(),
        );
        // Twenty thousand voxels from the planet, wherever the planet is.
        let apart = [0, 1, 2].map(|axis| moon_at[axis] - planet_at[axis]);
        let distance = (apart[0] * apart[0] + apart[1] * apart[1] + apart[2] * apart[2]).sqrt();
        assert!((distance - 2.0e4).abs() < 1e-2, "{distance} at {ticks}");
        // And its speed is the planet's and its own.
        let own = 2.0 * std::f64::consts::PI * 2.0e4 / 500.0;
        let relative = [0, 1, 2].map(|axis| {
            f64::from(moon_ship.linear_velocity[axis])
                - f64::from(planet_ship.linear_velocity[axis])
        });
        let speed =
            (relative[0] * relative[0] + relative[1] * relative[1] + relative[2] * relative[2])
                .sqrt();
        assert!((speed - own).abs() < 1e-2, "{speed} against {own}");
    }
}

#[test]
fn a_moon_of_a_planet_of_a_star_10_to_the_18_voxels_out_keeps_its_distances_and_is_where_the_exact_turn_puts_it()
 {
    let mut world = world_for_rails();
    // 10^18 voxels from the origin an f64 of voxels is a hundred voxels from the next, so a planet's 1.5 x 10^11 and a moon's 3.84 x 10^8 are lost in a sum of plain
    // numbers; as bricks and an offset they are not.
    let far = WorldPosition::new(BrickCoordinate::new(31_250_000_000_000_000, 0, 0), [0.0; 3]);
    let star = world
        .create_grid(LevelIndex::MAIN, GridPose::new(far, Orientation::IDENTITY))
        .unwrap();
    let planet = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    let moon = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    // A year of 3.156e7 seconds is 631,200,000 ticks at twenty a second.
    world
        .set_grid_rail(
            planet,
            Some(orbit_about(Centre::Grid(star), 1.5e11, 3.156e7)),
        )
        .unwrap();
    world
        .set_grid_rail(
            moon,
            Some(orbit_about(Centre::Grid(planet), 3.84e8, 2.36e6)),
        )
        .unwrap();
    let from = |world: &ServerWorld, centre: GridId, body: GridId| {
        let place = |ship: GridId| world.data().grids().get(ship).unwrap().pose.position;
        place(centre).displacement_to(place(body)).in_voxels()
    };
    let length = |vector: [f64; 3]| {
        (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt()
    };
    for ticks in [0u64, 1, 333, 12_345, 7_600_000_000_037] {
        world.data.set_clock(ticks);
        world.apply_rails(20);
        let planet_from_star = from(&world, star, planet);
        let moon_from_planet = from(&world, planet, moon);
        assert!(
            (length(planet_from_star) - 1.5e11).abs() < 1.0e-3,
            "the planet is {} from the star at tick {ticks}",
            length(planet_from_star)
        );
        assert!(
            (length(moon_from_planet) - 3.84e8).abs() < 1.0e-3,
            "the moon is {} from the planet at tick {ticks}",
            length(moon_from_planet)
        );
        // The planet is where the part of a year gone says: a circle about +y from +x toward -z.
        let angle = std::f64::consts::TAU * ((ticks % 631_200_000) as f64 / 631_200_000.0);
        let expected = [1.5e11 * libm::cos(angle), 0.0, -1.5e11 * libm::sin(angle)];
        for axis in 0..3 {
            assert!(
                (planet_from_star[axis] - expected[axis]).abs() < 1.0e-3,
                "{planet_from_star:?} against {expected:?} at tick {ticks}"
            );
        }
    }
}

#[test]
fn a_ship_5_x_10_to_the_17_voxels_from_what_it_goes_round_is_where_the_turn_worked_out_at_120_digits_puts_it_to_the_last_bit_of_an_f32()
 {
    // The radius of an orbit that was as far as a coordinate went before coordinates were 128 bits (a ninth of 2^60 voxels): an f64 of it is 64 voxels from the next, so an f64
    // orbit is that far out however well the turn is worked out. Round a ship at brick (1000, -2000, 3000) and offset (3.5, 7.25, 30), a year of 9 x 10^8 seconds, a phase of one radian,
    // 123,456,789 ticks at twenty a second: the expected place is `exact_circle_reference.py`'s, at 120 digits.
    let mut world = world_for_rails();
    let centre = world
        .create_grid(
            LevelIndex::MAIN,
            GridPose::new(
                WorldPosition::new(BrickCoordinate::new(1000, -2000, 3000), [3.5, 7.25, 30.0]),
                Orientation::IDENTITY,
            ),
        )
        .unwrap();
    let body = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    let rail = Rail {
        spin: None,
        orbit: Some(Orbit::new(Centre::Grid(centre), [0.6, 0.8, 0.0], 5.0e17, 9.0e8, 1.0).unwrap()),
    };
    world.set_grid_rail(body, Some(rail)).unwrap();
    world.data.set_clock(123_456_789);
    world.apply_rails(20);
    let found = world.data().grids().get(body).unwrap().pose.position;
    assert_eq!(
        [found.brick().x(), found.brick().y(), found.brick().z()],
        [
            6_294_363_568_489_132,
            -4_720_772_676_368_099,
            -13_499_478_419_764_357
        ]
    );
    let expected = [18.927409849581174, 9.655737331008694, 1.4475123486614843];
    for axis in 0..3 {
        // (Half a step of an f32 between 16 and 32 is 0.95 x 10^-6.)
        assert!(
            (f64::from(found.offset()[axis]) - expected[axis]).abs() < 1.0e-6,
            "{:?} against {expected:?}",
            found.offset()
        );
    }
}

#[test]
fn a_ship_6_x_10_to_the_26_voxels_from_what_it_goes_round_is_where_the_turn_worked_out_at_120_digits_puts_it_to_the_last_bit_of_an_f32()
 {
    // The size of the universe in metres, for voxels of a metre: a brick position of 10^25, which an `i64` could not hold (it ends at 9 x 10^18). The same ship and the
    // same turn as the test before, a radius of 6 x 10^26; the expected place is `exact_circle_reference.py`'s, at 120 digits.
    let mut world = world_for_rails();
    let centre = world
        .create_grid(
            LevelIndex::MAIN,
            GridPose::new(
                WorldPosition::new(BrickCoordinate::new(1000, -2000, 3000), [3.5, 7.25, 30.0]),
                Orientation::IDENTITY,
            ),
        )
        .unwrap();
    let body = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    let rail = Rail {
        spin: None,
        orbit: Some(Orbit::new(Centre::Grid(centre), [0.6, 0.8, 0.0], 6.0e26, 9.0e8, 1.0).unwrap()),
    };
    world.set_grid_rail(body, Some(rail)).unwrap();
    world.data.set_clock(123_456_789);
    world.apply_rails(20);
    let found = world.data().grids().get(body).unwrap().pose.position;
    assert_eq!(
        [found.brick().x(), found.brick().y(), found.brick().z()],
        [
            7_553_236_282_185_758_905_874_764,
            -5_664_927_211_639_318_655_296_271,
            -16_199_374_103_720_829_314_894_525
        ]
    );
    let expected = [6.879435126977078, 2.548914014534149, 24.42621732003802];
    for axis in 0..3 {
        assert!(
            (f64::from(found.offset()[axis]) - expected[axis]).abs() < 1.0e-6,
            "{:?} against {expected:?}",
            found.offset()
        );
    }
}

#[test]
fn what_stands_on_a_body_on_a_rail_does_not_feel_what_it_goes_round_since_it_falls_with_it() {
    let mut world = world_for_rails();
    world.set_gravity_settings(GravitySettings {
        base: [0.0; 3],
        pull_strength: 6.674e-11,
        pull_mass_threshold: Some(1.0e19),
        softening: 1.0,
    });
    // A planet that weighs 6 x 10^20 kilograms at the origin, and a moon of 10^19 that goes round it 400,000 voxels out (at the start, along +x).
    let planet = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    let moon = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    world.data.grids_mut().get_mut(planet).unwrap().mass = 6.0e20;
    world.data.grids_mut().get_mut(moon).unwrap().mass = 1.0e19;
    world
        .set_grid_rail(
            moon,
            Some(orbit_about(Centre::Grid(planet), 400_000.0, 200_000.0)),
        )
        .unwrap();
    world.apply_rails(20);
    assert_eq!(
        world
            .data()
            .grids()
            .get(moon)
            .unwrap()
            .pose
            .position
            .in_voxels(),
        [400_000.0, 0.0, 0.0]
    );
    // A hundred voxels over the moon's middle: in the moon's own axes (it does not turn), straight down at the moon's pull, and nothing sideways.
    let local = at(0.0, 100.0, 0.0);
    let own = 6.674e-11 * 1.0e19 * 100.0 / (10_001.0f64 * 10_001.0f64.sqrt());
    let felt = world.gravity_in(moon, local).unwrap();
    assert!(felt[0].abs() < 1e-3 && felt[2].abs() < 1e-3, "{felt:?}");
    assert!(
        (f64::from(-felt[1]) - own).abs() < 1e-2 * own.max(1.0),
        "{felt:?} against {own}"
    );
    // In the level the planet's pull is there: 6.674e-11 * 6e20 / 400,000^2 = 0.25 toward -x, at that place.
    let world_place = world
        .data()
        .grids()
        .get(moon)
        .unwrap()
        .pose
        .local_to_world(local)
        .unwrap();
    let in_the_level = world.gravity_at(LevelIndex::MAIN, world_place);
    assert!((in_the_level[0] + 0.25).abs() < 1e-2, "{in_the_level:?}");
    // The planet it goes round, which goes round nothing, is pulled by the moon and by nothing taken away.
    let over_the_planet = world.gravity_in(planet, at(0.0, 100.0, 0.0)).unwrap();
    assert_eq!(
        over_the_planet,
        world.gravity_at(LevelIndex::MAIN, at(0.0, 100.0, 0.0))
    );
    // A ship that is on no rail feels everything, wherever it is.
    let raft = world
        .create_grid(
            LevelIndex::MAIN,
            GridPose::new(world_place, Orientation::IDENTITY),
        )
        .unwrap();
    assert_eq!(
        world.gravity_in(raft, at(0.0, 0.0, 0.0)).unwrap(),
        world.gravity_at(LevelIndex::MAIN, world_place)
    );
}

#[test]
fn what_stands_on_a_moon_of_a_moon_feels_neither_of_the_bodies_it_goes_round_and_a_rail_in_a_loop_ends()
 {
    let mut world = world_for_rails();
    world.set_gravity_settings(GravitySettings {
        base: [0.0; 3],
        pull_strength: 6.674e-11,
        pull_mass_threshold: Some(1.0e19),
        softening: 1.0,
    });
    let planet = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    let moon = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    let small_moon = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    for (ship, mass) in [(planet, 6.0e20f32), (moon, 6.0e19), (small_moon, 1.0e19)] {
        world.data.grids_mut().get_mut(ship).unwrap().mass = mass;
    }
    world
        .set_grid_rail(
            moon,
            Some(orbit_about(Centre::Grid(planet), 400_000.0, 200_000.0)),
        )
        .unwrap();
    world
        .set_grid_rail(
            small_moon,
            Some(orbit_about(Centre::Grid(moon), 20_000.0, 5_000.0)),
        )
        .unwrap();
    world.apply_rails(20);
    // On the small moon: its own pull alone (straight down, 100 voxels from its middle), not the moon's, which it goes round, and not the planet's.
    let felt = world.gravity_in(small_moon, at(0.0, 100.0, 0.0)).unwrap();
    let own = 6.674e-11 * 1.0e19 * 100.0 / (10_001.0f64 * 10_001.0f64.sqrt());
    assert!(felt[0].abs() < 1e-3 && felt[2].abs() < 1e-3, "{felt:?}");
    assert!(
        (f64::from(-felt[1]) - own).abs() < 1e-2 * own.max(1.0),
        "{felt:?}"
    );
    // On the moon: its own pull, and the small moon's, which goes round the moon and pulls the moon with it (6.674e-11 x 10^19 / 20,000^2 = 1.67 toward +x);
    // and not the planet's 0.25 toward -x, which the moon goes round.
    let on_the_moon = world.gravity_in(moon, at(0.0, 100.0, 0.0)).unwrap();
    assert!((on_the_moon[0] - 1.668).abs() < 0.01, "{on_the_moon:?}");

    // Two ships that go round each other, and one that goes round itself: the chain ends.
    let first = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    let second = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    world
        .set_grid_rail(
            first,
            Some(orbit_about(Centre::Grid(second), 1000.0, 100.0)),
        )
        .unwrap();
    world
        .set_grid_rail(
            second,
            Some(orbit_about(Centre::Grid(first), 1000.0, 100.0)),
        )
        .unwrap();
    assert_eq!(world.what_it_goes_round(first), vec![second]);
    assert_eq!(world.what_it_goes_round(second), vec![first]);
    world
        .set_grid_rail(first, Some(orbit_about(Centre::Grid(first), 1000.0, 100.0)))
        .unwrap();
    assert_eq!(world.what_it_goes_round(first), Vec::<GridId>::new());
    assert!(world.gravity_in(first, at(0.0, 0.0, 0.0)).is_ok());
    // The chain, nearest first.
    assert_eq!(world.what_it_goes_round(small_moon), vec![moon, planet]);
    assert_eq!(world.what_it_goes_round(moon), vec![planet]);
    assert_eq!(world.what_it_goes_round(planet), Vec::<GridId>::new());
}

// ---- the sphere of influence of a planet on a rail ----

/// A planet whose ground reaches `reach` bricks either way, weighing `mass`, in the main level, on this rail.
fn planet_on(world: &mut ServerWorld, reach: u32, mass: f32, rail: Option<Rail>) -> GridId {
    let planet = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    let terrain = GridTerrain {
        generator: world.data().main().terrain.default_generator(),
        extent: BrickBox::around(BrickCoordinate::ORIGIN, reach),
        mass,
    };
    world.set_grid_terrain(planet, terrain).unwrap();
    world.set_grid_rail(planet, rail).unwrap();
    planet
}

fn voxels_from(world: &ServerWorld, planet: GridId, voxels: [f64; 3]) -> WorldPosition {
    let centre = world
        .data()
        .grids()
        .get(planet)
        .unwrap()
        .pose
        .position
        .in_voxels();
    WorldPosition::from_voxels([0, 1, 2].map(|axis| centre[axis] + voxels[axis]))
}

#[test]
fn a_planet_on_a_rail_carries_what_is_within_one_and_a_half_times_the_reach_of_its_ground_and_no_other_ship_carries_anything()
 {
    let (mut world, _) = world_with_a_walker();
    let rail = orbit_about(Centre::Point([0.0; 3]), 1.0e6, 1.0e5);
    // A hundred bricks either way is 3,200 voxels, and one and a half times that 4,800.
    let planet = planet_on(&mut world, 100, 1.0e20, Some(rail));
    assert_eq!(world.influence_radius(planet), Some(4_800.0));
    // Enormous ground is cut to what the numbers of the bodies are exact in (131 km, a little less).
    let vast = planet_on(&mut world, 100_000, 1.0e20, Some(rail));
    assert_eq!(world.influence_radius(vast), Some(120_000.0));
    // Not a planet that stands still, not a ship on a rail with no ground, not a ship.
    let still = planet_on(&mut world, 100, 1.0e20, None);
    assert_eq!(world.influence_radius(still), None);
    let star = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    world.set_grid_rail(star, Some(rail)).unwrap();
    assert_eq!(world.influence_radius(star), None);
    let nothing = GridId::new(GridId::FIRST_ALLOCATED + 99);
    assert_eq!(world.influence_radius(nothing), None);
}

#[test]
fn the_planet_that_carries_a_place_is_the_one_with_the_smallest_sphere_round_it_and_one_that_carries_keeps_what_it_had_a_little_farther()
 {
    let (mut world, _) = world_with_a_walker();
    // A planet with a sphere of 4,800 voxels, and a moon of it with one of 1,200 (reach 25 bricks), 2,000 voxels from its middle.
    let planet = planet_on(
        &mut world,
        100,
        1.0e20,
        Some(orbit_about(Centre::Point([0.0; 3]), 1.0e6, 1.0e5)),
    );
    let moon = planet_on(
        &mut world,
        25,
        1.0e19,
        Some(orbit_about(Centre::Grid(planet), 2_000.0, 500.0)),
    );
    world.apply_rails(20);
    let main = LevelIndex::MAIN;
    // In both spheres: the moon's. In the planet's only: the planet's. In neither: none.
    let near_the_moon = voxels_from(&world, moon, [500.0, 0.0, 0.0]);
    assert_eq!(world.planet_carrying(main, near_the_moon, None), Some(moon));
    let beside = voxels_from(&world, planet, [0.0, 1_000.0, 0.0]);
    assert_eq!(world.planet_carrying(main, beside, None), Some(planet));
    let far = voxels_from(&world, planet, [0.0, 10_000.0, 0.0]);
    assert_eq!(world.planet_carrying(main, far, None), None);
    // Just outside the moon's sphere (1,300 of 1,200): not carried by it, unless it was; and not farther than a quarter more (1,500).
    let outside = voxels_from(&world, moon, [0.0, 1_300.0, 0.0]);
    assert_eq!(world.planet_carrying(main, outside, None), Some(planet));
    assert_eq!(world.planet_carrying(main, outside, Some(moon)), Some(moon));
    let well_outside = voxels_from(&world, moon, [0.0, 1_600.0, 0.0]);
    assert_eq!(
        world.planet_carrying(main, well_outside, Some(moon)),
        Some(planet)
    );
    // Another level has none of these planets.
    assert_eq!(
        world.planet_carrying(LevelIndex::new(9), near_the_moon, None),
        None
    );
}

fn walker_in_the_level(
    world: &mut ServerWorld,
    walker: &NamespacedId,
    at: WorldPosition,
) -> EntityId {
    world
        .spawn_entity(
            walker,
            EntityPose::new(GridId::MAIN, at, Orientation::IDENTITY),
            ComponentBag::new(),
        )
        .unwrap()
}

#[test]
fn what_is_in_the_level_near_a_planet_on_a_rail_goes_with_it_and_what_is_aboard_or_far_is_left_alone()
 {
    let (mut world, walker) = world_with_a_walker();
    let planet = planet_on(
        &mut world,
        100,
        1.0e20,
        Some(orbit_about(Centre::Point([0.0; 3]), 1.0e6, 1.0e5)),
    );
    world.apply_rails(20);
    let near = voxels_from(&world, planet, [100.0, 3_000.0, -50.0]);
    let far = voxels_from(&world, planet, [100.0, 30_000.0, -50.0]);
    let near_walker = walker_in_the_level(&mut world, &walker, near);
    let far_walker = walker_in_the_level(&mut world, &walker, far);
    // One aboard a ship that is near the planet in the level: the ship's, not the planet's, to carry.
    let ship = world
        .create_grid(LevelIndex::MAIN, GridPose::new(near, Orientation::IDENTITY))
        .unwrap();
    let aboard_walker = world
        .spawn_entity(
            &walker,
            aboard(ship, [0, 0, 0], [1.0; 3], Orientation::IDENTITY),
            ComponentBag::new(),
        )
        .unwrap();
    let before = world.entities().get(near_walker).unwrap().pose.position;
    let planet_before = world.data().grids().get(planet).unwrap().pose.position;
    for ticks in 1..=5u64 {
        world.data.set_clock(ticks * 20);
        world.apply_rails(20);
        let planet_now = world.data().grids().get(planet).unwrap().pose.position;
        let moved = planet_before.displacement_to(planet_now).in_voxels();
        assert!(
            moved[0].abs() + moved[1].abs() + moved[2].abs() > 1.0,
            "the planet goes"
        );
        // The walker near it is where it was from the planet: the planet's displacement since the start.
        let now = world.entities().get(near_walker).unwrap().pose.position;
        let carried = before.displacement_to(now).in_voxels();
        for axis in 0..3 {
            assert!(
                (carried[axis] - moved[axis]).abs() < 1e-3,
                "tick {ticks}: carried {carried:?} against the planet's {moved:?}"
            );
        }
    }
    // The far one, and the one aboard a ship, were not moved at all.
    assert_eq!(world.entities().get(far_walker).unwrap().pose.position, far);
    assert_eq!(
        world.entities().get(aboard_walker).unwrap().pose,
        aboard(ship, [0, 0, 0], [1.0; 3], Orientation::IDENTITY)
    );
}

fn a_ship_at(world: &mut ServerWorld, place: WorldPosition) -> GridId {
    world
        .create_grid(
            LevelIndex::MAIN,
            GridPose::new(place, Orientation::IDENTITY),
        )
        .unwrap()
}

fn place_of(world: &ServerWorld, ship: GridId) -> WorldPosition {
    world.data().grids().get(ship).unwrap().pose.position
}

#[test]
fn how_far_the_world_carried_an_entity_this_tick_is_told_to_whoever_keeps_a_state_of_its_own_for_it()
 {
    let (mut world, walker) = world_with_a_walker();
    let planet = planet_on(
        &mut world,
        100,
        1.0e20,
        Some(orbit_about(Centre::Point([0.0; 3]), 1.0e6, 1.0e5)),
    );
    world.apply_rails(20);
    let near = voxels_from(&world, planet, [100.0, 3_000.0, -50.0]);
    let far = voxels_from(&world, planet, [100.0, 30_000.0, -50.0]);
    let near_walker = walker_in_the_level(&mut world, &walker, near);
    let far_walker = walker_in_the_level(&mut world, &walker, far);
    let ship = a_ship_at(&mut world, near);
    let aboard_walker = world
        .spawn_entity(
            &walker,
            aboard(ship, [0, 0, 0], [1.0; 3], Orientation::IDENTITY),
            ComponentBag::new(),
        )
        .unwrap();
    // Nothing has been carried yet: the planet has not moved since it was put on its rail.
    assert_eq!(world.carried_by_rails(near_walker), None);
    let before = place_of(&world, planet);
    world.data.set_clock(20);
    world.apply_rails(20);
    let moved = before.displacement_to(place_of(&world, planet));
    assert_ne!(moved, Displacement::NONE);
    // What was carried is told, with the displacement it was carried by: the planet's. Not what is far, nor what is aboard a ship.
    assert_eq!(world.carried_by_rails(near_walker), Some(moved));
    assert_eq!(world.carried_by_rails(far_walker), None);
    assert_eq!(world.carried_by_rails(aboard_walker), None);
    // A pose put on it afterwards (a teleport, a landing) is not carried: the carry is forgotten, so that no one moves what was put somewhere by it.
    world
        .set_entity_pose(
            near_walker,
            EntityPose::new(GridId::MAIN, far, Orientation::IDENTITY),
        )
        .unwrap();
    assert_eq!(world.carried_by_rails(near_walker), None);
    // The call the server makes again after the rules of the tick moves nothing, and so tells of nothing: the tick's carry is for what runs in the tick.
    let other = walker_in_the_level(&mut world, &walker, near);
    world.data.set_clock(40);
    world.apply_rails(20);
    assert!(world.carried_by_rails(other).is_some());
    world.apply_rails(20);
    assert_eq!(world.carried_by_rails(other), None);
}

#[test]
fn a_ship_in_the_sphere_of_a_planet_on_a_rail_goes_with_it_held_or_free_and_one_beyond_it_or_with_a_rail_of_its_own_does_not()
 {
    let (mut world, _) = world_with_a_walker();
    let planet = planet_on(
        &mut world,
        100,
        1.0e20,
        Some(orbit_about(Centre::Point([0.0; 3]), 1.0e6, 1.0e5)),
    );
    world.apply_rails(20);
    let near = voxels_from(&world, planet, [100.0, 3_000.0, -50.0]);
    let far = voxels_from(&world, planet, [100.0, 30_000.0, -50.0]);
    let free = a_ship_at(&mut world, near);
    let held = a_ship_at(&mut world, near);
    world.set_grid_held(held, true).unwrap();
    let distant = a_ship_at(&mut world, far);
    let on_its_own_rail = a_ship_at(&mut world, near);
    let own_rail = orbit_about(Centre::Point([0.0; 3]), 5.0e5, 1.0e5);
    world
        .set_grid_rail(on_its_own_rail, Some(own_rail))
        .unwrap();
    world
        .set_grid_velocity(free, [1.0, 2.0, 3.0], [0.0, 0.5, 0.0])
        .unwrap();
    world.apply_rails(20);
    // The first call only learns who is where (a ship is carried by the planet whose sphere it is in before the planet moves).
    assert_eq!(world.carrier_of(free), Some(planet));
    assert_eq!(world.carrier_of(held), Some(planet));
    assert_eq!(world.carrier_of(distant), None);
    assert_eq!(
        world.carrier_of(on_its_own_rail),
        None,
        "a ship on a rail is placed by it"
    );
    assert_eq!(
        world.carrier_of(planet),
        None,
        "a planet is carried by no planet"
    );

    let (planet_before, free_before, held_before, distant_before) = (
        place_of(&world, planet),
        place_of(&world, free),
        place_of(&world, held),
        place_of(&world, distant),
    );
    world.data.set_clock(20);
    world.apply_rails(20);
    let moved = planet_before
        .displacement_to(place_of(&world, planet))
        .in_voxels();
    assert!(
        moved[0].abs() + moved[1].abs() + moved[2].abs() > 1.0,
        "the planet goes"
    );
    for (ship, before) in [(free, free_before), (held, held_before)] {
        let carried = before.displacement_to(place_of(&world, ship)).in_voxels();
        for axis in 0..3 {
            assert!(
                (carried[axis] - moved[axis]).abs() < 1e-3,
                "{ship:?} carried {carried:?} against the planet's {moved:?}"
            );
        }
    }
    assert_eq!(
        place_of(&world, distant),
        distant_before,
        "beyond the sphere"
    );
    // What is carried keeps its own velocity: the planet's is the planet's, and the ship's speed against it is the ship's.
    let carried_ship = world.data().grids().get(free).unwrap();
    assert_eq!(carried_ship.linear_velocity, [1.0, 2.0, 3.0]);
    assert_eq!(carried_ship.angular_velocity, [0.0, 0.5, 0.0]);
    // Told again in the same tick (the server does, after the rules), nothing moves it twice.
    let (once_planet, once_free) = (place_of(&world, planet), place_of(&world, free));
    world.apply_rails(20);
    assert_eq!(place_of(&world, planet), once_planet);
    assert_eq!(place_of(&world, free), once_free);
}

#[test]
fn a_ship_that_goes_out_of_the_sphere_is_let_go_a_quarter_farther_out_and_a_world_without_rails_carries_nothing()
 {
    let (mut world, _) = world_with_a_walker();
    let still = a_ship_at(&mut world, at(5.0, 5.0, 5.0));
    world.apply_rails(20);
    assert_eq!(world.carrier_of(still), None);
    assert_eq!(place_of(&world, still), at(5.0, 5.0, 5.0));

    let planet = planet_on(
        &mut world,
        100,
        1.0e20,
        Some(orbit_about(Centre::Point([0.0; 3]), 1.0e6, 1.0e5)),
    );
    world.apply_rails(20);
    // 4,700 of 4,800: inside. It is carried; then it is put at 5,500, beyond the sphere but not a quarter beyond: still carried, as it was. At 6,500 it is let go.
    let inside = voxels_from(&world, planet, [0.0, 4_700.0, 0.0]);
    let ship = a_ship_at(&mut world, inside);
    world.apply_rails(20);
    assert_eq!(world.carrier_of(ship), Some(planet));
    let beyond = voxels_from(&world, planet, [0.0, 5_500.0, 0.0]);
    world
        .set_grid_pose(ship, GridPose::new(beyond, Orientation::IDENTITY))
        .unwrap();
    world.apply_rails(20);
    assert_eq!(world.carrier_of(ship), Some(planet));
    let outside = voxels_from(&world, planet, [0.0, 6_500.0, 0.0]);
    world
        .set_grid_pose(ship, GridPose::new(outside, Orientation::IDENTITY))
        .unwrap();
    world.apply_rails(20);
    assert_eq!(world.carrier_of(ship), None);
    world.data.set_clock(40);
    world.apply_rails(20);
    assert_eq!(place_of(&world, ship), outside, "let go, it stays");
    // And a ship that is not in the world is carried by none.
    assert_eq!(
        world.carrier_of(GridId::new(GridId::FIRST_ALLOCATED + 999)),
        None
    );
}

#[test]
fn an_entity_that_leaves_the_sphere_is_let_go_and_is_not_carried_after_and_a_world_with_no_rails_carries_nothing()
 {
    let (mut world, walker) = world_with_a_walker();
    let still = walker_in_the_level(&mut world, &walker, at(5.0, 5.0, 5.0));
    world.apply_rails(20);
    assert_eq!(
        world.entities().get(still).unwrap().pose.position,
        at(5.0, 5.0, 5.0)
    );

    let planet = planet_on(
        &mut world,
        100,
        1.0e20,
        Some(orbit_about(Centre::Point([0.0; 3]), 1.0e6, 1.0e5)),
    );
    world.apply_rails(20);
    let inside = voxels_from(&world, planet, [0.0, 4_700.0, 0.0]);
    let walker_id = walker_in_the_level(&mut world, &walker, inside);
    world.data.set_clock(20);
    world.apply_rails(20);
    let carried_to = world.entities().get(walker_id).unwrap().pose.position;
    assert_ne!(carried_to, inside, "carried while inside");
    // It walks out beyond the sphere and a quarter more (4,800 and 6,000), and is let go: the planet goes on without it.
    let outside = voxels_from(&world, planet, [0.0, 6_500.0, 0.0]);
    world
        .set_entity_pose(
            walker_id,
            EntityPose::new(GridId::MAIN, outside, Orientation::IDENTITY),
        )
        .unwrap();
    world.data.set_clock(40);
    world.apply_rails(20);
    assert_eq!(
        world.entities().get(walker_id).unwrap().pose.position,
        outside
    );
    world.data.set_clock(60);
    world.apply_rails(20);
    assert_eq!(
        world.entities().get(walker_id).unwrap().pose.position,
        outside
    );
}

#[test]
fn what_is_in_the_level_within_the_sphere_of_a_planet_on_a_rail_falls_with_it_and_feels_nothing_of_what_it_goes_round()
 {
    let (mut world, walker) = world_with_a_walker();
    world.set_gravity_settings(GravitySettings {
        base: [0.0; 3],
        pull_strength: 6.674e-11,
        pull_mass_threshold: Some(1.0e19),
        softening: 1.0,
    });
    let star = planet_on(&mut world, 10, 6.0e20, None);
    let planet = planet_on(
        &mut world,
        100,
        1.0e19,
        Some(orbit_about(Centre::Grid(star), 400_000.0, 200_000.0)),
    );
    world.apply_rails(20);
    let inside = voxels_from(&world, planet, [0.0, 100.0, 0.0]);
    let outside = voxels_from(&world, planet, [0.0, 50_000.0, 0.0]);
    let _ = walker;
    // Inside the sphere: the planet's own pull alone, straight down (it does not turn), and nothing sideways from the star 400,000 voxels away.
    let own = 6.674e-11 * 1.0e19 * 100.0 / (10_001.0f64 * 10_001.0f64.sqrt());
    let felt = world.gravity_in(GridId::MAIN, inside).unwrap();
    assert!(felt[0].abs() < 1e-3 && felt[2].abs() < 1e-3, "{felt:?}");
    assert!(
        (f64::from(-felt[1]) - own).abs() < 1e-2 * own.max(1.0),
        "{felt:?} against {own}"
    );
    // Outside it, the pull is the level's: the star's is felt (0.25 toward it, on the -x side, 6.674e-11 x 6e20 / 400,000^2).
    let there = world.gravity_in(GridId::MAIN, outside).unwrap();
    assert!(there[0] < -0.2, "{there:?}");
    assert_eq!(there, world.gravity_at(LevelIndex::MAIN, outside));
}

#[test]
fn a_ship_whose_rail_puts_it_past_the_end_of_the_world_is_left_where_it_is_and_named() {
    let mut world = world_for_rails();
    let there = GridPose::new(at(1.0, 2.0, 3.0), Orientation::IDENTITY);
    let lost = world.create_grid(LevelIndex::MAIN, there).unwrap();
    let fine = world.create_grid(LevelIndex::MAIN, there).unwrap();
    world
        .set_grid_rail(
            lost,
            // (The edge of the world is 2^120 voxels, 1.3 x 10^36, and no orbit is wider than that, so it takes a centre near the edge to go past it: at tick 0 the ship is a
            // radius along x from the centre, 2.2 x 10^36.)
            Some(orbit_about(
                Centre::Point([1.2e36, 0.0, 0.0]),
                1.0e36,
                100.0,
            )),
        )
        .unwrap();
    world
        .set_grid_rail(
            fine,
            Some(orbit_about(Centre::Point([0.0; 3]), 1000.0, 100.0)),
        )
        .unwrap();
    assert_eq!(world.data.put_ships_on_their_rails(0.0), vec![lost]);
    assert_eq!(world.data().grids().get(lost).unwrap().pose, there);
    // Nor is it given the speed of a place it was not put in.
    assert_eq!(
        world.data().grids().get(lost).unwrap().linear_velocity,
        [0.0; 3]
    );
    // The others were put in their places all the same.
    assert_ne!(world.data().grids().get(fine).unwrap().pose, there);
    // A ship that goes round one that is not there stays where it is, and is not named as lost.
    world
        .set_grid_rail(
            lost,
            Some(orbit_about(
                Centre::Grid(GridId::new(GridId::FIRST_ALLOCATED + 999)),
                10.0,
                10.0,
            )),
        )
        .unwrap();
    assert_eq!(
        world.data.put_ships_on_their_rails(0.0),
        Vec::<GridId>::new()
    );
    assert_eq!(world.data().grids().get(lost).unwrap().pose, there);
}

#[test]
fn a_ship_that_only_turns_stays_where_it_is_and_one_with_no_rail_is_left_alone() {
    let mut world = world_for_rails();
    let there = GridPose::new(at(10.0, 20.0, 30.0), Orientation::from_yaw_pitch(0.3, 0.0));
    let turning = world.create_grid(LevelIndex::MAIN, there).unwrap();
    let plain = world.create_grid(LevelIndex::MAIN, there).unwrap();
    world
        .set_grid_rail(
            turning,
            Some(Rail {
                spin: Some(Spin::new([0.0, 1.0, 0.0], 3600.0, 0.0).unwrap()),
                orbit: None,
            }),
        )
        .unwrap();
    world
        .set_grid_velocity(plain, [1.0, 2.0, 3.0], [0.0; 3])
        .unwrap();
    world.data.set_clock(900 * 20);
    world.apply_rails(20);
    let turned = world.data().grids().get(turning).unwrap();
    assert_eq!(turned.pose.position, there.position);
    assert_ne!(turned.pose.orientation, there.orientation);
    assert_eq!(turned.linear_velocity, [0.0; 3]);
    let alone = world.data().grids().get(plain).unwrap();
    assert_eq!(alone.pose, there);
    assert_eq!(alone.linear_velocity, [1.0, 2.0, 3.0]);
}

#[test]
fn a_rail_can_be_given_and_taken_off_and_a_ship_that_is_not_there_has_none() {
    let mut world = world_for_rails();
    let ship = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    let rail = orbit_about(Centre::Point([0.0; 3]), 100.0, 10.0);
    world.set_grid_rail(ship, Some(rail)).unwrap();
    assert_eq!(world.data().grids().get(ship).unwrap().rail, Some(rail));
    world.set_grid_rail(ship, None).unwrap();
    assert_eq!(world.data().grids().get(ship).unwrap().rail, None);
    let nowhere = GridId::new(GridId::FIRST_ALLOCATED + 999);
    assert_eq!(
        world.set_grid_rail(nowhere, Some(rail)),
        Err(AccessError::NoSuchGrid(nowhere))
    );
}

#[test]
fn a_level_can_have_a_pull_of_its_own_and_a_planet_in_one_in_space_pulls_toward_its_centre_at_the_surface_gravity_it_was_given_its_mass_for()
 {
    let game = Game::new(&[Plugin::BaseBlocks]);
    let calls = Arc::new(AtomicUsize::new(0));
    let level = counting_level(&game, &calls);
    let mut data = WorldData::new(1, main_level_id(), level);
    let space = data
        .add_level(id("test_content:space"), counting_level(&game, &calls))
        .unwrap();
    let mut world = ServerWorld::new(data, Arc::default());
    // A planet of 63,710 metres, whose mass is the one that pulls 10 metres a second a second at that distance (G in metres).
    let radius = 63_710.0f64;
    let mass = (10.0 * radius * radius / 6.674e-11) as f32;
    world.set_gravity_settings(GravitySettings {
        base: [0.0, -5.0, 0.0],
        pull_strength: 6.674e-11,
        pull_mass_threshold: Some(1.0e19),
        softening: 1.0,
    });
    world.set_level_gravity(space, [0.0; 3]);
    let planet = world.create_grid(space, GridPose::identity()).unwrap();
    let terrain = GridTerrain {
        generator: world.data().main().terrain.default_generator(),
        extent: BrickBox::around(BrickCoordinate::ORIGIN, 2000),
        mass,
    };
    world.set_grid_terrain(planet, terrain).unwrap();
    let ship = world.data().grids().get(planet).unwrap();
    assert_eq!(ship.terrain, Some(terrain));
    assert_eq!(ship.mass, mass);
    assert_eq!(ship.centre_of_mass, [0.0; 3]);
    assert!(ship.held);

    // Over the planet's top, in the level in space, the pull is the planet's alone: toward its centre, 10 and a hair under (the softening).
    let over = |x: f32, y: f32, z: f32, brick: [i128; 3]| {
        WorldPosition::new(
            BrickCoordinate::new(brick[0], brick[1], brick[2]),
            [x, y, z],
        )
    };
    let top = world.gravity_at(space, over(0.0, 0.0, 0.0, [0, 1990, 0]));
    let top_voxel_height = 1990.0 * 32.0;
    let expected = 10.0 * (radius / top_voxel_height) * (radius / top_voxel_height);
    assert!(top[0].abs() < 1e-3 && top[2].abs() < 1e-3, "{top:?}");
    assert!(
        (f64::from(-top[1]) - expected).abs() < 1e-3,
        "{top:?} against {expected}"
    );
    // And on the far side, the other way.
    let bottom = world.gravity_at(space, over(0.0, 0.0, 0.0, [0, -1990, 0]));
    assert!(bottom[1] > 0.0 && (f64::from(bottom[1]) - expected).abs() < 1e-3);
    // The main level keeps its own pull, and the planet in the level in space is no part of it.
    assert_eq!(
        world.gravity_at(LevelIndex::MAIN, over(0.0, 0.0, 0.0, [0, 1990, 0])),
        [0.0, -5.0, 0.0]
    );
    // A grid in the level in space falls to the same field (toward the planet: from out on the +x side, along -x).
    let pull = world
        .gravity_in(GridId::root(space), over(0.0, 0.0, 0.0, [1990, 0, 0]))
        .unwrap();
    assert!(
        pull[0] < -1.0 && pull[1].abs() < 1e-3 && pull[2].abs() < 1e-3,
        "{pull:?}"
    );
}

#[test]
fn gravity_in_a_grid_is_the_levels_in_the_axes_of_the_grid_and_turns_as_a_ship_does() {
    let (mut world, _, _) = counting_world();
    let root = GridId::MAIN;
    assert_eq!(
        world.gravity_in(root, at(1.0, 2.0, 3.0)),
        Ok([0.0, -5.0, 0.0])
    );
    let nowhere = GridId::root(LevelIndex::new(9));
    assert_eq!(
        world.gravity_in(nowhere, at(0.0, 0.0, 0.0)),
        Err(AccessError::NoSuchGrid(nowhere))
    );
    // Turned a quarter about the z axis, a ship's down is along its own x axis (its x axis points where the world's y does...
    // whichever way, the length is the same and the world's down is the ship's turned axis).
    let quarter = Orientation::from_components([0.0, 0.0, 0.5f64.sqrt(), 0.5f64.sqrt()]).unwrap();
    let ship = world
        .create_grid(
            LevelIndex::MAIN,
            GridPose::new(at(0.0, 100.0, 0.0), quarter),
        )
        .unwrap();
    let local = world.gravity_in(ship, at(1.0, 1.0, 1.0)).unwrap();
    // The ship's x axis, turned a quarter about z, is the world's y: so a world down is the ship's negative x axis turned back, which is
    // along +x or -x of the ship, and nothing along its y or z.
    assert!(
        (local[0].abs() - 5.0).abs() < 1e-3 && local[1].abs() < 1e-3 && local[2].abs() < 1e-3,
        "{local:?}"
    );
    // The same from the other side: a half turn flips it.
    let flipped = world
        .create_grid(
            LevelIndex::MAIN,
            GridPose::new(
                at(0.0, 0.0, 0.0),
                Orientation::from_components([1.0, 0.0, 0.0, 0.0]).unwrap(),
            ),
        )
        .unwrap();
    let up = world.gravity_in(flipped, at(0.0, 0.0, 0.0)).unwrap();
    assert!(
        (up[1] - 5.0).abs() < 1e-4 && up[0].abs() < 1e-4 && up[2].abs() < 1e-4,
        "{up:?}"
    );
    let gone = GridId::new(GridId::FIRST_ALLOCATED + 70);
    assert_eq!(
        world.gravity_in(gone, at(0.0, 0.0, 0.0)),
        Err(AccessError::NoSuchGrid(gone))
    );
}

#[test]
fn a_ship_that_weighs_enough_pulls_toward_its_centre_of_mass_and_not_itself_and_places_in_it_feel_it()
 {
    let (mut world, _, _) = counting_world();
    world.set_gravity_settings(GravitySettings {
        base: [0.0; 3],
        pull_strength: 1e-3,
        pull_mass_threshold: Some(1e6),
        softening: 1.0,
    });
    let big = world
        .create_grid(
            LevelIndex::MAIN,
            GridPose::new(at(20.0, 0.0, 0.0), Orientation::IDENTITY),
        )
        .unwrap();
    let light = world
        .create_grid(
            LevelIndex::MAIN,
            GridPose::new(at(100.0, 0.0, 0.0), Orientation::IDENTITY),
        )
        .unwrap();
    // Nothing has been weighed: nothing pulls.
    assert_eq!(
        world.gravity_at(LevelIndex::MAIN, at(50.0, 0.0, 0.0)),
        [0.0; 3]
    );
    world.set_grid_mass(big, 4e6, [8.0, 8.0, 8.0]).unwrap();
    world.set_grid_mass(light, 5e5, [1.0, 1.0, 1.0]).unwrap();
    // 4e6 kg at a distance of 50 from the centre, (28, 8, 8) in the world (the ship is 20 along x and its centre is (8, 8, 8) in it): along x toward it, G m / d^2 = 1.6 (plus a hair of softening and the y and z).
    let pull = world.gravity_at(LevelIndex::MAIN, at(78.0, 8.0, 8.0));
    assert!(
        (pull[0] + 1e-3 * 4e6 / 2501.0 * 50.0 / 2501.0f32.sqrt()).abs() < 1e-3,
        "{pull:?}"
    );
    assert!(pull[1].abs() < 1e-5 && pull[2].abs() < 1e-5);
    // The light ship is under the threshold and pulls nothing: a point beside it feels exactly what the big one alone gives there.
    let settings = *world.gravity_settings();
    let big_alone = MassPull {
        position: at(28.0, 8.0, 8.0),
        mass: 4e6,
    };
    let near_light = world.gravity_at(LevelIndex::MAIN, at(101.0, 1.0, 1.0));
    assert_eq!(
        near_light,
        gravity_at(&settings, [big_alone], at(101.0, 1.0, 1.0))
    );
    assert!(near_light[0] < -0.3, "{near_light:?}");
    // A ship's pull is nothing at its own centre of mass, where a ship is pulled: it does not pull itself.
    assert_eq!(
        world.gravity_at(LevelIndex::MAIN, at(28.0, 8.0, 8.0)),
        [0.0; 3]
    );
    // A place in the big ship, in its own axes, feels it too (toward its centre): the voxel above the centre is pulled down.
    let inside = world.gravity_in(big, at(8.0, 12.0, 8.0)).unwrap();
    assert!(inside[1] < 0.0 && inside[0].abs() < 1e-5, "{inside:?}");
    // A ship that is in another level does not pull this one.
    let gone = GridId::new(GridId::FIRST_ALLOCATED + 70);
    assert_eq!(
        world.set_grid_mass(gone, 1.0, [0.0; 3]),
        Err(AccessError::NoSuchGrid(gone))
    );
}

/// A ship that is a row of ten blocks along x, turning about the vertical axis at one radian a second about its middle, with a walker in it
/// at each of these places (in the ship's coordinates). Returns the world, the ship, the block channel and the walkers.
fn row_with_walkers(places: &[[f32; 3]]) -> (ServerWorld, GridId, ChannelIndex, Vec<EntityId>) {
    let (mut world, walker) = world_with_a_walker();
    let block = block_channel(world.data().main());
    let ship = world
        .create_grid(
            LevelIndex::MAIN,
            GridPose::new(
                WorldPosition::new(BrickCoordinate::ORIGIN, [3.0, 40.0, 3.0]),
                Orientation::IDENTITY,
            ),
        )
        .unwrap();
    for x in 0..10 {
        world
            .grid(ship)
            .unwrap()
            .set(block, VoxelCoordinate::new(x, 0, 0), 7)
            .unwrap();
    }
    world.set_grid_mass(ship, 10.0, [5.0, 0.5, 0.5]).unwrap();
    world
        .set_grid_velocity(ship, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
        .unwrap();
    world.set_grid_held(ship, false).unwrap();
    let walkers = places
        .iter()
        .map(|place| {
            world
                .spawn_entity(
                    &walker,
                    aboard(ship, [0, 0, 0], *place, Orientation::IDENTITY),
                    ComponentBag::new(),
                )
                .unwrap()
        })
        .collect();
    (world, ship, block, walkers)
}

fn halves() -> Vec<SplitPart> {
    let part = |range: std::ops::Range<i128>, x: f32| SplitPart {
        voxels: range.map(|x| VoxelCoordinate::new(x, 0, 0)).collect(),
        centre_of_mass: [x, 0.5, 0.5],
    };
    vec![part(0..5, 2.5), part(5..10, 7.5)]
}

#[test]
fn a_ship_split_in_two_is_two_new_ships_with_their_voxels_their_own_speeds_and_a_tombstone() {
    let (mut world, ship, block, _) = row_with_walkers(&[]);
    world.set_grid_held(ship, true).unwrap();
    world
        .set_grid_velocity(ship, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
        .unwrap();
    world
        .set_grid_on_pilot_logout(ship, PilotLogout::Vanish)
        .unwrap();
    let parts = world.split_grid(ship, halves()).unwrap();
    assert_eq!(parts.len(), 2);
    // None has the id of the ship, which is gone, and it names both.
    assert!(parts.iter().all(|part| part.get() > ship.get()));
    assert!(world.data().grids().get(ship).is_none());
    let named: Vec<GridId> = world
        .data()
        .grids()
        .resolve(ship)
        .iter()
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(named, parts);
    for (index, part) in parts.iter().enumerate() {
        let moving = world.data().grids().get(*part).unwrap();
        // Where the ship was, turned as it was, with only its own blocks.
        assert_eq!(
            moving.pose.position,
            WorldPosition::new(BrickCoordinate::ORIGIN, [3.0, 40.0, 3.0])
        );
        let own: Vec<i128> = (0..10)
            .filter(|&x| moving.voxels.get(block, VoxelCoordinate::new(x, 0, 0)) == 7)
            .collect();
        assert_eq!(
            own,
            if index == 0 {
                (0..5).collect::<Vec<_>>()
            } else {
                (5..10).collect::<Vec<_>>()
            }
        );
        // The ship's speed, and what its turn adds at the middle of the part: the ship turns about y, so a place 2.5 voxels to one side of
        // its middle moves 2.5 voxels a second along z, the other way for the other side.
        let along_z = if index == 0 { 2.5 } else { -2.5 };
        assert_eq!(moving.linear_velocity, [1.0, 0.0, along_z]);
        assert_eq!(moving.angular_velocity, [0.0, 1.0, 0.0]);
        assert!(moving.held && moving.on_pilot_logout == PilotLogout::Vanish);
    }
}

#[test]
fn what_is_aboard_a_ship_that_is_split_goes_to_the_part_it_stands_on_or_else_the_nearest() {
    let (mut world, ship, _, walkers) = row_with_walkers(&[
        [1.5, 2.0, 0.5],  // standing on the left half
        [8.5, 2.0, 0.5],  // standing on the right half
        [6.2, 30.0, 0.5], // in the air far above, nearer the right half
        [4.4, 30.0, 0.5], // in the air far above, nearer the left half
    ]);
    let parts = world.split_grid(ship, halves()).unwrap();
    let frames: Vec<GridId> = walkers
        .iter()
        .map(|id| world.entities().get(*id).unwrap().pose.grid)
        .collect();
    assert_eq!(frames, [parts[0], parts[1], parts[1], parts[0]]);
    // Where they are in the ship's coordinates is where they were, which are the parts' too.
    assert_eq!(
        world.entities().get(walkers[1]).unwrap().pose.position,
        WorldPosition::new(BrickCoordinate::ORIGIN, [8.5, 2.0, 0.5])
    );
}

#[test]
fn the_rules_hear_of_the_parts_of_a_split_ship_as_of_ships_that_came_out_of_a_file() {
    let (mut world, ship, block, _) = row_with_walkers(&[]);
    world.watch_channels([block].into_iter().collect());
    world.take_changes();
    let parts = world.split_grid(ship, halves()).unwrap();
    let changes = world.take_changes();
    assert_eq!(changes.len(), 10);
    assert!(changes.iter().all(|change| parts.contains(&change.grid)));
}

#[test]
fn a_split_that_is_not_one_changes_nothing() {
    let (mut world, ship, _, walkers) = row_with_walkers(&[[1.5, 2.0, 0.5]]);
    let missing = GridId::new(GridId::FIRST_ALLOCATED + 99);
    assert_eq!(
        world.split_grid(missing, halves()),
        Err(SplitError::Access(AccessError::NoSuchGrid(missing)))
    );
    assert_eq!(
        world.split_grid(ship, halves()[..1].to_vec()),
        Err(SplitError::NotTwoParts)
    );
    let mut with_nothing = halves();
    with_nothing[1].voxels.clear();
    assert_eq!(
        world.split_grid(ship, with_nothing),
        Err(SplitError::NotTwoParts)
    );
    // A planet is never cut, whatever the parts.
    world.data_mut().grids_mut().get_mut(ship).unwrap().terrain = Some(GridTerrain {
        generator: world.data().main().terrain.default_generator(),
        extent: BrickBox::around(BrickCoordinate::ORIGIN, 1),
        mass: 1.0e20,
    });
    assert_eq!(
        world.split_grid(ship, halves()),
        Err(SplitError::HasTerrain)
    );
    world.data_mut().grids_mut().get_mut(ship).unwrap().terrain = None;
    let mut overlapping = halves();
    overlapping[1].voxels.insert(VoxelCoordinate::new(4, 0, 0));
    assert_eq!(
        world.split_grid(ship, overlapping),
        Err(SplitError::Overlap)
    );
    assert!(world.data().grids().get(ship).is_some());
    assert_eq!(world.data().grids().tombstones().count(), 0);
    assert_eq!(world.entities().get(walkers[0]).unwrap().pose.grid, ship);
}

#[test]
fn blocks_can_be_detached_from_a_ship_into_a_ship_that_is_where_they_were_turned_as_the_ship_is_and_going_as_they_were()
 {
    let (mut world, ship, block, _) = row_with_walkers(&[]);
    // The ship is turned a quarter about the vertical axis and turns at a radian a second about its middle, which is 5 along its x.
    let turned = GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [3.0, 40.0, 3.0]),
        Orientation::from_yaw_pitch(std::f32::consts::FRAC_PI_2, 0.0),
    );
    world.set_grid_pose(ship, turned).unwrap();
    let taken: Vec<VoxelCoordinate> = (6..10).map(|x| VoxelCoordinate::new(x, 0, 0)).collect();
    let part = world.detach_blocks(ship, &taken).unwrap();
    // The blocks are gone from the ship and are in the new one, from its origin, which is the lowest corner of them (voxel 6).
    for x in 0..10 {
        let here = world
            .grid(ship)
            .unwrap()
            .get(block, VoxelCoordinate::new(x, 0, 0))
            .unwrap();
        assert_eq!(here, if x < 6 { 7 } else { 0 }, "x = {x}");
    }
    for x in 0..4 {
        let there = world
            .grid(part)
            .unwrap()
            .get(block, VoxelCoordinate::new(x, 0, 0))
            .unwrap();
        assert_eq!(there, 7);
    }
    let new = world.data().grids().get(part).unwrap();
    // Turned as the ship is; its origin is where voxel 6 of the ship is: 6 along the ship's x, which the quarter turn makes -z in the world.
    assert_eq!(new.pose.orientation, turned.orientation);
    let origin = turned
        .local_to_world(WorldPosition::new(BrickCoordinate::ORIGIN, [6.0, 0.0, 0.0]))
        .unwrap();
    assert_eq!(new.pose.position, origin);
    // Not held, and going as the blocks were: the ship's speed at their middle (voxel 8 of the ship, 3 past its middle) plus its turn.
    assert!(!new.held);
    let expected = world.data().grids().get(ship).unwrap().velocity_at(
        turned
            .local_to_world(WorldPosition::new(BrickCoordinate::ORIGIN, [8.0, 0.5, 0.5]))
            .unwrap(),
    );
    assert_eq!(new.linear_velocity, expected);
    assert_eq!(new.angular_velocity, [0.0, 1.0, 0.0]);
}

#[test]
fn a_ship_has_nothing_generated_and_a_brick_of_it_that_was_never_made_is_every_voxel_at_the_default()
 {
    let (mut world, ship, block, _) = row_with_walkers(&[]);
    let default = world.data().main().overrides.layout().default_value(block);
    let mut view = world.grid(ship).unwrap();
    // What a ship has to begin with is the channel's default, whatever is built there.
    assert_eq!(
        view.generated(block, VoxelCoordinate::new(2, 0, 0))
            .unwrap(),
        default
    );
    // A brick that holds the row is not one value; a brick that was never made is the default throughout.
    assert_eq!(
        view.uniform_value(block, BrickCoordinate::ORIGIN).unwrap(),
        None
    );
    assert_eq!(
        view.uniform_value(block, BrickCoordinate::new(5, 5, 5))
            .unwrap(),
        Some(default)
    );
}

/// A world whose entity catalog has one kind, with no components.
pub(crate) fn world_with_a_walker() -> (ServerWorld, NamespacedId) {
    let (world, _, _) = counting_world();
    let mut kinds = entities::EntityKinds::new();
    kinds
        .register(entities::EntityKindSpec::new("test:walker"))
        .unwrap();
    let catalog = Arc::new(EntityCatalog {
        kinds,
        items: Default::default(),
    });
    (
        ServerWorld::new(world.into_data(), catalog),
        id("test:walker"),
    )
}

pub(crate) fn aboard(
    ship: GridId,
    brick: [i128; 3],
    offset: [f32; 3],
    turn: Orientation,
) -> EntityPose {
    EntityPose::new(
        ship,
        WorldPosition::new(BrickCoordinate::new(brick[0], brick[1], brick[2]), offset),
        turn,
    )
}

#[test]
fn an_entity_can_be_in_the_frame_of_a_ship_that_is_there_and_of_nothing_else() {
    let (mut world, walker) = world_with_a_walker();
    let missing = GridId::new(GridId::FIRST_ALLOCATED + 3);
    let nowhere = aboard(missing, [0, 0, 0], [1.0; 3], Orientation::IDENTITY);
    assert_eq!(
        world.spawn_entity(&walker, nowhere, ComponentBag::new()),
        Err(EntityError::NoSuchGrid(missing))
    );
    assert!(world.entities().is_empty(), "nothing was made");
    let ship = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    let on_deck = aboard(ship, [0, 0, 0], [1.0; 3], Orientation::IDENTITY);
    let entity = world
        .spawn_entity(&walker, on_deck, ComponentBag::new())
        .unwrap();
    assert_eq!(world.entities().get(entity).unwrap().pose.grid, ship);
    // Moving it into a frame that is not there changes nothing; into the level, or another ship that is there, works.
    assert_eq!(
        world.set_entity_pose(entity, nowhere),
        Err(EntityError::NoSuchGrid(missing))
    );
    assert_eq!(world.entities().get(entity).unwrap().pose, on_deck);
    let level = EntityPose::new(
        GridId::MAIN,
        WorldPosition::new(BrickCoordinate::ORIGIN, [3.0; 3]),
        Orientation::IDENTITY,
    );
    world.set_entity_pose(entity, level).unwrap();
    assert_eq!(world.entities().get(entity).unwrap().pose, level);
}

#[test]
fn removing_a_ship_puts_what_was_aboard_in_the_level_where_it_was_and_turned_as_it_was() {
    let (mut world, walker) = world_with_a_walker();
    let quarter = std::f32::consts::FRAC_PI_2;
    let pose = GridPose::new(
        WorldPosition::new(BrickCoordinate::new(2, 0, 0), [1.0, 2.0, 3.0]),
        Orientation::from_yaw_pitch(quarter, 0.0),
    );
    let ship = world.create_grid(LevelIndex::MAIN, pose).unwrap();
    let other_ship = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    let tipped = Orientation::from_yaw_pitch(0.0, quarter);
    let passenger = world
        .spawn_entity(
            &walker,
            aboard(ship, [0, 0, 0], [10.0, 0.0, 0.0], tipped),
            ComponentBag::new(),
        )
        .unwrap();
    // One whose coordinates are far from the ship's origin, and one hidden in the ship: all are aboard.
    let far = world
        .spawn_entity(
            &walker,
            aboard(ship, [-3, 4, 1], [5.0; 3], Orientation::IDENTITY),
            ComponentBag::new(),
        )
        .unwrap();
    let bystander = world
        .spawn_entity(
            &walker,
            aboard(other_ship, [0, 0, 0], [10.0, 0.0, 0.0], tipped),
            ComponentBag::new(),
        )
        .unwrap();
    let on_ground = world
        .spawn_entity(
            &walker,
            EntityPose::new(
                GridId::MAIN,
                WorldPosition::new(BrickCoordinate::ORIGIN, [4.0; 3]),
                tipped,
            ),
            ComponentBag::new(),
        )
        .unwrap();
    let before_bystander = world.entities().get(bystander).unwrap().pose;
    let before_ground = world.entities().get(on_ground).unwrap().pose;
    let expected = |local: EntityPose| {
        let (position, orientation) = pose
            .place_in_world(local.position, local.orientation)
            .unwrap();
        EntityPose::new(GridId::MAIN, position, orientation)
    };
    let expected_passenger = expected(world.entities().get(passenger).unwrap().pose);
    let expected_far = expected(world.entities().get(far).unwrap().pose);
    world.data_mut().entities_mut().take_changed();

    assert!(world.remove_grid(ship).is_some());

    assert_eq!(
        world.entities().get(passenger).unwrap().pose,
        expected_passenger
    );
    assert_eq!(world.entities().get(far).unwrap().pose, expected_far);
    // The numbers, worked out by hand: the ship is turned a quarter about the vertical axis, so 10 along its x is 10 along the
    // world's -z from its origin, which is at (2 bricks, 1, 2, 3).
    let placed = world.entities().get(passenger).unwrap().pose;
    assert_eq!(placed.grid, GridId::MAIN);
    assert_eq!(placed.position.brick(), BrickCoordinate::new(2, 0, -1));
    let offset = placed.position.offset();
    assert!(
        (offset[0] - 1.0).abs() < 1e-4
            && (offset[1] - 2.0).abs() < 1e-4
            && (offset[2] - 25.0).abs() < 1e-4,
        "{offset:?}"
    );
    // Turned as the ship was and then as it was in the ship: a tip about the ship's x is a tip about the world's -z.
    assert_ne!(placed.orientation, tipped);
    // Nothing else moved, and what moved is on the list of what changed.
    assert_eq!(
        world.entities().get(bystander).unwrap().pose,
        before_bystander
    );
    assert_eq!(world.entities().get(on_ground).unwrap().pose, before_ground);
    assert_eq!(
        world.data_mut().entities_mut().take_changed(),
        BTreeSet::from([passenger, far])
    );
    assert!(world.entities().in_grid(ship).is_empty());
    // A ship that is not there takes nothing with it.
    assert!(world.remove_grid(ship).is_none());
}

#[test]
fn a_world_made_without_somewhere_to_keep_ship_files_cannot_put_ships_away_or_bring_them_in() {
    let (mut world, ..) = counting_world();
    let ship = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    assert!(matches!(
        world.park_ship(ship, false),
        Err(ShipStorageError::NoStorage)
    ));
    assert!(matches!(
        world.load_ship(ship),
        Err(ShipStorageError::NoStorage)
    ));
    assert!(world.ship_files_mut().is_none());
    // The ship is where it was.
    assert!(world.data().grids().get(ship).is_some());
}

#[test]
fn a_frozen_ship_is_not_simulated_and_has_no_speed_and_the_terrain_always_is() {
    let (mut world, ..) = counting_world();
    let ship = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    world
        .set_grid_velocity(ship, [1.0, 2.0, 3.0], [0.0, 0.5, 0.0])
        .unwrap();
    assert!(world.grid_is_simulated(ship));
    assert!(world.grid_is_simulated(GridId::MAIN));
    // A grid that is not there is not simulated: not a level the world lacks, and not a ship it never made.
    assert!(!world.grid_is_simulated(GridId::new(5)));
    assert!(!world.grid_is_simulated(GridId::new(GridId::FIRST_ALLOCATED + 40)));

    world.set_grid_frozen(ship, true).unwrap();
    assert!(!world.grid_is_simulated(ship));
    let frozen = world.data().grids().get(ship).unwrap();
    assert!(frozen.frozen);
    assert_eq!(
        (frozen.linear_velocity, frozen.angular_velocity),
        ([0.0; 3], [0.0; 3])
    );
    // Whoever moves it while it is frozen says how fast; freezing again does not take that away.
    world
        .set_grid_velocity(ship, [4.0, 0.0, 0.0], [0.0; 3])
        .unwrap();
    world.set_grid_frozen(ship, true).unwrap();
    assert_eq!(
        world.data().grids().get(ship).unwrap().linear_velocity,
        [4.0, 0.0, 0.0]
    );
    // Let go, it is simulated again, and starts from rest as it is (the bodies start it).
    world.set_grid_frozen(ship, false).unwrap();
    assert!(world.grid_is_simulated(ship));
    assert!(matches!(
        world.set_grid_frozen(GridId::new(GridId::FIRST_ALLOCATED + 40), true),
        Err(AccessError::NoSuchGrid(_))
    ));
}

#[test]
fn a_ship_says_what_its_pilots_logout_does_to_it() {
    let (mut world, ..) = counting_world();
    let ship = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    assert_eq!(
        world.data().grids().get(ship).unwrap().on_pilot_logout,
        PilotLogout::Stay
    );
    world
        .set_grid_on_pilot_logout(ship, PilotLogout::Vanish)
        .unwrap();
    assert_eq!(
        world.data().grids().get(ship).unwrap().on_pilot_logout,
        PilotLogout::Vanish
    );
    world
        .set_grid_on_pilot_logout(ship, PilotLogout::Stay)
        .unwrap();
    assert_eq!(
        world.data().grids().get(ship).unwrap().on_pilot_logout,
        PilotLogout::Stay
    );
    assert!(matches!(
        world.set_grid_on_pilot_logout(
            GridId::new(GridId::FIRST_ALLOCATED + 9),
            PilotLogout::Vanish
        ),
        Err(AccessError::NoSuchGrid(_))
    ));
}

// ---- a ship with terrain (a planet): its bricks are what its generator makes, unless they are stored ----

/// The counting world with a ship whose terrain is the world's generator over the bricks within 3 of its origin.
fn world_with_a_planet() -> (ServerWorld, GridId, ChannelIndex, Arc<AtomicUsize>) {
    let (mut world, block, calls) = counting_world();
    let ship = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    let generator = world.data().main().terrain.default_generator();
    world.data_mut().grids_mut().get_mut(ship).unwrap().terrain = Some(GridTerrain {
        generator,
        extent: BrickBox::around(BrickCoordinate::ORIGIN, 3),
        mass: 1.0e20,
    });
    (world, ship, block, calls)
}

#[test]
fn a_planet_reads_what_its_generator_makes_inside_its_extent_and_the_default_outside() {
    let (mut world, planet, block, _) = world_with_a_planet();
    let default = world.data().main().overrides.layout().default_value(block);
    // The flat ground of the level is the same ground here: stone below height 10 (and dirt and grass on it), the default above, in the bricks it makes.
    for at in [
        VoxelCoordinate::new(5, 2, 3),
        VoxelCoordinate::new(-40, -50, 70),
        VoxelCoordinate::new(5, 9, 3),
        VoxelCoordinate::new(5, 10, 3),
        VoxelCoordinate::new(-96, 0, 95),
    ] {
        let from_the_planet = world.grid(planet).unwrap().get(block, at).unwrap();
        let from_the_level = world
            .grid(GridId::root(LevelIndex::MAIN))
            .unwrap()
            .get(block, at)
            .unwrap();
        assert_eq!(from_the_planet, from_the_level, "{at:?}");
    }
    assert_ne!(
        world
            .grid(planet)
            .unwrap()
            .get(block, VoxelCoordinate::new(5, 2, 3))
            .unwrap(),
        default
    );
    // Outside the extent (the bricks within 3 of the origin are voxels -96 to 127) there is nothing, though the generator would make ground.
    for at in [
        VoxelCoordinate::new(5, 2, 128),
        VoxelCoordinate::new(-97, 2, 3),
        VoxelCoordinate::new(5, -97, 3),
    ] {
        assert_eq!(world.grid(planet).unwrap().get(block, at).unwrap(), default);
    }
}

#[test]
fn bricks_of_a_planet_asked_for_ahead_are_made_by_a_worker_and_a_read_finds_them_made() {
    let (mut world, planet, block, calls) = world_with_a_planet();
    let a_brick = BrickCoordinate::new(0, 0, 0);
    let another = BrickCoordinate::new(1, 0, 0);
    let outside = BrickCoordinate::new(50, 0, 0);
    // Asked twice, and one past the extent: the worker makes two.
    world.prefetch_ship_bricks(planet, [a_brick, another, a_brick, outside]);
    world.prefetch_ship_bricks(planet, [another]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while world.bricks_prefetched() < 2 {
        assert!(
            std::time::Instant::now() < deadline,
            "the worker did not finish"
        );
        world.take_prefetched();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    // Bricks that are remembered are not asked for again.
    world.prefetch_ship_bricks(planet, [a_brick, another]);
    std::thread::sleep(std::time::Duration::from_millis(50));
    world.take_prefetched();
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    // Reading them makes nothing more, and the voxels are what the generator makes: the same as a world that made them where it read them.
    let found: Vec<u32> = [[5, 2, 3], [40, 2, 3]]
        .iter()
        .map(|at| {
            world
                .grid(planet)
                .unwrap()
                .get(block, VoxelCoordinate::new(at[0], at[1], at[2]))
                .unwrap()
        })
        .collect();
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(world.bricks_generated(), 0);
    assert_eq!(world.bricks_prefetched(), 2);
    let (mut plain, plain_planet, plain_block, _) = world_with_a_planet();
    for (at, expected) in [[5, 2, 3], [40, 2, 3]].iter().zip(found) {
        let voxel = VoxelCoordinate::new(at[0], at[1], at[2]);
        assert_eq!(
            plain
                .grid(plain_planet)
                .unwrap()
                .get(plain_block, voxel)
                .unwrap(),
            expected
        );
    }
    assert_eq!(plain.bricks_generated(), 2);
}

#[test]
fn bricks_of_a_level_asked_for_ahead_are_made_by_a_worker_once_and_a_read_finds_them_made() {
    let (mut world, block, calls) = counting_world();
    let level = LevelIndex::MAIN;
    let (a, b) = (BrickCoordinate::new(0, 0, 0), BrickCoordinate::new(1, 0, 0));
    world.prefetch_level_bricks(level, [a, b, a]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    // (The generator has run twice; nothing has been taken into the cache by anything the world was asked.)
    while calls.load(Ordering::Relaxed) < 2 {
        assert!(
            std::time::Instant::now() < deadline,
            "the worker did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    let read = |world: &mut ServerWorld, block| -> Vec<u32> {
        [[5, 2, 3], [40, 2, 3]]
            .iter()
            .map(|at| {
                world
                    .grid(GridId::root(level))
                    .unwrap()
                    .get(block, VoxelCoordinate::new(at[0], at[1], at[2]))
                    .unwrap()
            })
            .collect()
    };
    let found = read(&mut world, block);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(world.bricks_generated(), 0);
    // Remembered, they are not asked for again; a read makes nothing and finds what a world that made them where it read them finds.
    world.prefetch_level_bricks(level, [a, b]);
    std::thread::sleep(std::time::Duration::from_millis(50));
    world.take_prefetched();
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    let (mut plain, plain_block, _) = counting_world();
    assert_eq!(read(&mut plain, plain_block), found);
    assert_eq!(plain.bricks_generated(), 2);
}

#[test]
fn a_planets_bricks_are_made_once_each_and_none_outside_the_extent() {
    let (mut world, planet, block, calls) = world_with_a_planet();
    for x in 0..32 {
        world
            .grid(planet)
            .unwrap()
            .get(block, VoxelCoordinate::new(x, 2, 3))
            .unwrap();
    }
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    world
        .grid(planet)
        .unwrap()
        .get(block, VoxelCoordinate::new(40, 2, 3))
        .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    // The world counts them as it does for terrain; the bricks outside the extent are not asked for.
    assert_eq!(world.bricks_generated(), 2);
    world
        .grid(planet)
        .unwrap()
        .get(block, VoxelCoordinate::new(500, 2, 3))
        .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    // The terrain of the level and the planet make the same brick separately: they do not share what they remember.
    world
        .grid(GridId::root(LevelIndex::MAIN))
        .unwrap()
        .get(block, VoxelCoordinate::new(5, 2, 3))
        .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 3);
}

#[test]
fn an_edit_of_a_planet_is_stored_over_the_generated_brick_and_wins_and_the_rest_of_the_brick_is_as_generated()
 {
    let (mut world, planet, block, _) = world_with_a_planet();
    let at = VoxelCoordinate::new(5, 2, 3);
    let neighbour = VoxelCoordinate::new(6, 2, 3);
    let before = world.grid(planet).unwrap().get(block, at).unwrap();
    let beside = world.grid(planet).unwrap().get(block, neighbour).unwrap();
    let new_value = before + 7;
    assert!(
        world
            .grid(planet)
            .unwrap()
            .set(block, at, new_value)
            .unwrap()
    );
    assert_eq!(
        world.grid(planet).unwrap().get(block, at).unwrap(),
        new_value
    );
    assert_eq!(
        world.grid(planet).unwrap().get(block, neighbour).unwrap(),
        beside
    );
    // The generator's value is still the generator's, and the brick is stored.
    assert_eq!(
        world.grid(planet).unwrap().generated(block, at).unwrap(),
        before
    );
    assert_eq!(
        world
            .data()
            .grids()
            .get(planet)
            .unwrap()
            .voxels
            .brick_count(),
        1
    );
    // The edit is noted for the clients as that of a brick that was not stored before, and the second edit of the brick as one that was.
    let (brick, _) = at.split();
    assert!(world.edits[&(planet, brick)].created);
    assert!(
        world
            .grid(planet)
            .unwrap()
            .set(block, neighbour, beside + 1)
            .unwrap()
    );
    assert!(world.edits[&(planet, brick)].created);
    assert_eq!(world.edits[&(planet, brick)].changes.len(), 2);
    // Setting a voxel to what it has stores nothing and says nothing.
    let elsewhere = VoxelCoordinate::new(5, 2, 40);
    let value = world.grid(planet).unwrap().get(block, elsewhere).unwrap();
    assert!(
        !world
            .grid(planet)
            .unwrap()
            .set(block, elsewhere, value)
            .unwrap()
    );
    assert_eq!(
        world
            .data()
            .grids()
            .get(planet)
            .unwrap()
            .voxels
            .brick_count(),
        1
    );
    assert!(!world.edits.contains_key(&(planet, elsewhere.split().0)));
}

#[test]
fn what_a_brick_of_a_planet_holds_is_known_from_the_stored_brick_or_else_the_generated_one() {
    let (mut world, planet, block, _) = world_with_a_planet();
    let uniform = |world: &mut ServerWorld, grid: GridId, brick: [i128; 3]| {
        world
            .grid(grid)
            .unwrap()
            .uniform_value(block, BrickCoordinate::new(brick[0], brick[1], brick[2]))
            .unwrap()
    };
    let level = GridId::root(LevelIndex::MAIN);
    // Deep ground, the sky, a brick with the surface in it, and a brick beyond the extent: as the level says for the first three, the default for the last.
    for brick in [[0, -2, 0], [1, 2, 1], [0, 0, 0]] {
        let from_the_level = uniform(&mut world, level, brick);
        assert_eq!(
            uniform(&mut world, planet, brick),
            from_the_level,
            "{brick:?}"
        );
    }
    assert_eq!(uniform(&mut world, planet, [0, 0, 0]), None);
    let default = world.data().main().overrides.layout().default_value(block);
    assert_eq!(uniform(&mut world, planet, [9, 0, 0]), Some(default));
    // An edit that makes a brick of the sky not uniform is seen.
    let sky = VoxelCoordinate::new(40, 70, 40);
    assert_eq!(uniform(&mut world, planet, [1, 2, 1]), Some(default));
    world.grid(planet).unwrap().set(block, sky, 9).unwrap();
    assert_eq!(uniform(&mut world, planet, [1, 2, 1]), None);
}

#[test]
fn a_planet_whose_generator_is_missing_cannot_be_read_and_says_which() {
    let game = Game::new(&[Plugin::BaseBlocks]);
    let calls = Arc::new(AtomicUsize::new(0));
    let missing = id("gone_plugin:planet_v1");
    let level = counting_level_with_missing(&game, &calls, &[&missing]);
    let block = block_channel(&level);
    let generator = level.terrain.generators().runtime_id(&missing).unwrap();
    let mut world = ServerWorld::new(WorldData::new(1, main_level_id(), level), Arc::default());
    let planet = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    world
        .data_mut()
        .grids_mut()
        .get_mut(planet)
        .unwrap()
        .terrain = Some(GridTerrain {
        generator,
        extent: BrickBox::around(BrickCoordinate::ORIGIN, 3),
        mass: 1.0e20,
    });
    let at = VoxelCoordinate::new(5, 2, 3);
    assert_eq!(
        world.grid(planet).unwrap().get(block, at),
        Err(AccessError::TerrainUnavailable { generator: missing })
    );
    // Writing needs the brick the generator makes, so it is refused too, and nothing is stored.
    assert!(world.grid(planet).unwrap().set(block, at, 1).is_err());
    assert_eq!(
        world
            .data()
            .grids()
            .get(planet)
            .unwrap()
            .voxels
            .brick_count(),
        0
    );
}

#[test]
fn a_planet_is_told_of_with_its_generator_and_extent_and_sent_only_the_bricks_that_are_stored() {
    // (Its generator is the second of the table, so that its number is not 0 by accident; the placeholder of a plugin that is not here is told of all the same.)
    let game = Game::new(&[Plugin::BaseBlocks]);
    let calls = Arc::new(AtomicUsize::new(0));
    let second = id("gone_plugin:planet_v1");
    let level = counting_level_with_missing(&game, &calls, &[&second]);
    let block = block_channel(&level);
    let generator = level.terrain.generators().runtime_id(&second).unwrap();
    assert_eq!(generator.raw(), 1);
    let mut world = ServerWorld::new(WorldData::new(1, main_level_id(), level), Arc::default());
    let planet = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    world
        .data_mut()
        .grids_mut()
        .get_mut(planet)
        .unwrap()
        .terrain = Some(GridTerrain {
        generator,
        extent: BrickBox::around(BrickCoordinate::ORIGIN, 3),
        mass: 1.0e20,
    });
    let ship = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    world
        .grid(ship)
        .unwrap()
        .set(block, VoxelCoordinate::new(1, 1, 1), 2)
        .unwrap();
    // The planet has one stored brick (its generator is missing, so the brick is put in as it would be stored; the others in its extent are not sent).
    let stored_voxel = VoxelCoordinate::new(40, 1, 1);
    let layout = Arc::clone(world.data().main().overrides.layout());
    let mut stored = Grid::new(layout);
    stored.set(block, stored_voxel, 2);
    world.data_mut().grids_mut().get_mut(planet).unwrap().voxels = stored;
    let views = crate::grid_replication::ship_views(&world);
    assert_eq!(views.len(), 2);
    let mut known = crate::grid_replication::KnownGrids::default();
    let region = BrickBox::around(BrickCoordinate::ORIGIN, 5);
    let messages = crate::grid_replication::grid_messages(
        &world,
        &views,
        &mut known,
        Some((LevelIndex::MAIN, region)),
        1000,
        &[],
    );
    let ServerMessage::GridsSpawned(states) = &messages[0] else {
        panic!("the first message tells of the ships: {:?}", messages[0]);
    };
    let terrain = world.data().grids().get(planet).unwrap().terrain.unwrap();
    assert_eq!(states.len(), 2);
    assert_eq!(states[0].id, planet);
    assert_eq!(
        states[0].terrain,
        Some(protocol::GridTerrainState {
            generator: terrain.generator.raw(),
            extent: terrain.extent
        })
    );
    assert_eq!(states[1].terrain, None);
    // The bricks that follow are the ones that are stored, one for each ship, and not the planet's generated ones.
    let sent: Vec<(GridId, BrickCoordinate)> = messages
        .iter()
        .filter_map(|message| match message {
            ServerMessage::BrickContents { grid, brick, .. } => Some((*grid, *brick)),
            _ => None,
        })
        .collect();
    assert_eq!(
        sent,
        [
            (planet, stored_voxel.split().0),
            (ship, VoxelCoordinate::new(1, 1, 1).split().0)
        ]
    );
}

#[test]
fn a_planet_is_told_of_to_a_client_watching_anywhere_in_its_level_and_an_ordinary_ship_only_near() {
    let (mut world, planet, block, _) = world_with_a_planet();
    let ship = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    world
        .grid(ship)
        .unwrap()
        .set(block, VoxelCoordinate::new(1, 1, 1), 2)
        .unwrap();
    let views = crate::grid_replication::ship_views(&world);
    let told = |watched: Option<(LevelIndex, BrickBox)>| {
        let mut known = crate::grid_replication::KnownGrids::default();
        let messages =
            crate::grid_replication::grid_messages(&world, &views, &mut known, watched, 1000, &[]);
        let ships: Vec<GridId> = match messages.first() {
            Some(ServerMessage::GridsSpawned(states)) => {
                states.iter().map(|state| state.id).collect()
            }
            _ => Vec::new(),
        };
        (ships, known.keys().copied().collect::<Vec<_>>())
    };
    // Near, both. A long way off in the level (a client watching the far side of the planet, or a place in space), only the planet, which a client has to know to draw.
    let near = BrickBox::around(BrickCoordinate::ORIGIN, 5);
    let far = BrickBox::around(BrickCoordinate::new(100_000, -7, 300), 5);
    assert_eq!(told(Some((LevelIndex::MAIN, near))).0, [planet, ship]);
    assert_eq!(
        told(Some((LevelIndex::MAIN, far))),
        (vec![planet], vec![planet])
    );
    // Another level has none of them, and a client that watches nothing is told of nothing.
    assert_eq!(told(Some((LevelIndex::new(1), near))).0, []);
    assert_eq!(told(None).0, []);
}

#[test]
fn a_planet_is_read_through_a_shared_borrow_of_the_world_alongside_other_readers_of_it() {
    let (world, planet, block, calls) = world_with_a_planet();
    // Two readers of the world at once, as a body that meets several ships has: the bricks that the generator makes are remembered all the same.
    let shared: &ServerWorld = &world;
    let ships = shared.data().grids();
    let first = shared
        .moving_voxel(planet, block, VoxelCoordinate::new(5, 2, 3))
        .unwrap();
    let second = shared
        .moving_voxel(planet, block, VoxelCoordinate::new(40, 2, 3))
        .unwrap();
    let again = shared
        .moving_voxel(planet, block, VoxelCoordinate::new(6, 2, 3))
        .unwrap();
    assert!(ships.get(planet).is_some());
    assert_eq!((first, second), (again, again));
    assert_ne!(first, 0);
    // Two bricks made, and the third read came from what was remembered.
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(world.bricks_generated(), 2);
    // What a brick holds is known the same way.
    assert_eq!(
        shared
            .moving_uniform_value(planet, block, BrickCoordinate::new(0, -1, 0))
            .unwrap(),
        Some(first)
    );
}

#[test]
fn a_ship_that_nothing_generates_still_reads_its_default_and_its_stored_bricks_only() {
    let (mut world, block, calls) = counting_world();
    let ship = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    let default = world.data().main().overrides.layout().default_value(block);
    let at = VoxelCoordinate::new(5, 2, 3);
    assert_eq!(world.grid(ship).unwrap().get(block, at).unwrap(), default);
    assert_eq!(
        world.grid(ship).unwrap().generated(block, at).unwrap(),
        default
    );
    world.grid(ship).unwrap().set(block, at, 4).unwrap();
    assert_eq!(world.grid(ship).unwrap().get(block, at).unwrap(), 4);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    let (brick, _) = at.split();
    assert!(world.edits[&(ship, brick)].created);
}
