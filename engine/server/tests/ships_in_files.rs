//! Ships in files, as a server sees them: a save writes a file for each ship that is in memory and the world file names them; a server made
//! from that world with those files brings the ships back, and the rules hear of what is in them as if it had just been made.

use server::GridView;
use std::collections::BTreeSet;
use std::io;
use std::sync::{Arc, Mutex};

use namespaced_id::NamespacedId;
use server::test_support::ScriptedLogic;
use server::{Server, ServerConfig, ServerWorld};
use transport::loopback::loopback_listener;
use voxel_space::{GridId, LevelIndex, VoxelCoordinate};
use world_storage::test_support::{ALL_PLUGINS, Game, id, sample_world};
use world_storage::{MemoryShipFiles, ShipFileName, ShipFiles, ShipLoadError, WorldData};

fn game_id() -> NamespacedId {
    id("test_game:game")
}

fn server_for(
    game: &Game,
    world: WorldData,
    files: &MemoryShipFiles,
    logic: ScriptedLogic,
) -> Server {
    let (listener, _connector) = loopback_listener();
    Server::new(
        ServerConfig::new(game_id()).with_counted_challenges(1),
        world,
        &game.registries(),
        Box::new(logic),
        Box::new(listener),
    )
    .unwrap()
    .with_ship_files(Box::new(files.clone()))
}

/// What a server's save becomes on a disk with no earlier save: the files and the world file.
fn written(server: &mut Server, game: &Game, files: &MemoryShipFiles) -> Vec<u8> {
    let save = server.save(&game.registries()).unwrap();
    let mut world_file = Vec::new();
    save.write_with(&mut files.clone(), |bytes| {
        world_file = bytes.to_vec();
        Ok(())
    })
    .unwrap();
    world_file
}

#[test]
fn a_server_saves_its_ships_and_a_server_made_from_the_save_brings_them_back() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    let (one, two) = (game.sample_ship(&world, 0), game.sample_ship(&world, 100));
    let (first, second) = (world.grids_mut().create(one), world.grids_mut().create(two));
    let before: Vec<_> = [first, second]
        .map(|id| {
            let ship = world.grids().get(id).unwrap();
            (ship.pose, ship.voxels.content_digest(), ship.held)
        })
        .to_vec();

    let files = MemoryShipFiles::new();
    let mut server = server_for(&game, world, &files, ScriptedLogic::new());
    server.step();
    let world_file = written(&mut server, &game, &files);
    assert_eq!(files.len(), 2);

    // Another server, from the world file and the same files.
    let mut other_game = Game::new(&ALL_PLUGINS);
    let (reloaded, _) = other_game.load(&world_file).unwrap();
    assert_eq!(reloaded.grids().len(), 0);
    let mut again = server_for(&other_game, reloaded, &files, ScriptedLogic::new());
    assert!(again.load_parked_ships().is_empty());
    let after: Vec<_> = [first, second]
        .map(|id| {
            let ship = again.world().grids().get(id).unwrap();
            (ship.pose, ship.voxels.content_digest(), ship.held)
        })
        .to_vec();
    assert_eq!(after, before);

    // Saved again, a ship that has not been touched is a new file all the same (it was in memory), and the old files are cleaned away
    // once the new world file is in place.
    let before_names = files.names().unwrap();
    let world_file = written(&mut again, &other_game, &files);
    let names = files.names().unwrap();
    assert_eq!(names.len(), 2);
    assert!(names.iter().all(|name| !before_names.contains(name)));
    let (final_world, _) = Game::new(&ALL_PLUGINS).load(&world_file).unwrap();
    assert_eq!(
        final_world
            .grids()
            .parked()
            .map(|(_, parked)| parked.file)
            .collect::<BTreeSet<_>>(),
        names.into_iter().collect()
    );
}

#[test]
fn a_ship_whose_file_is_gone_is_reported_stays_in_its_file_and_is_still_named_by_the_next_save() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    let (one, two) = (game.sample_ship(&world, 0), game.sample_ship(&world, 100));
    let (first, second) = (world.grids_mut().create(one), world.grids_mut().create(two));
    let files = MemoryShipFiles::new();
    let mut server = server_for(&game, world, &files, ScriptedLogic::new());
    let world_file = written(&mut server, &game, &files);

    let mut broken = files.duplicate();
    let lost = ShipFileName::new(first, 0);
    broken.remove(&lost).unwrap();
    let mut other_game = Game::new(&ALL_PLUGINS);
    let (reloaded, _) = other_game.load(&world_file).unwrap();
    let mut again = server_for(&other_game, reloaded, &broken, ScriptedLogic::new());
    let problems = again.load_parked_ships();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].0, first);
    assert!(
        matches!(&problems[0].1, ShipLoadError::Unreadable { source, .. } if source.kind() == io::ErrorKind::NotFound)
    );
    // The other ship is back; the lost one is where it was, in a file that is not there, and the world file goes on naming it.
    assert!(again.world().grids().get(second).is_some());
    assert!(again.world().grids().get_parked(first).is_some());
    let save = again.save(&other_game.registries()).unwrap();
    assert!(save.named_ship_files.contains(&lost));
    assert_eq!(save.new_ship_files.len(), 1);
}

#[test]
fn a_ship_is_put_away_and_asked_for_again_from_inside_a_tick_and_cannot_be_asked_for_twice() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    let ship = game.sample_ship(&world, 0);
    let id = world.grids_mut().create(ship);
    let before = world.grids().get(id).unwrap().voxels.content_digest();

    let outcome = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&outcome);
    let logic = ScriptedLogic::new().on_tick(move |tick, world: &mut ServerWorld| {
        let mut seen = seen.lock().unwrap();
        match tick.get() {
            1 => seen.push(world.park_ship(id, false).is_ok()),
            2 => seen.push(world.load_ship(id).is_ok()),
            3 => seen.push(world.load_ship(id).is_ok()),
            _ => {}
        }
    });
    let files = MemoryShipFiles::new();
    let mut server = server_for(&game, world, &files, logic);
    for _ in 0..3 {
        server.step();
    }
    // Put away, brought back, and not brought back when it is already here.
    assert_eq!(*outcome.lock().unwrap(), [true, true, false]);
    assert_eq!(
        server
            .world()
            .grids()
            .get(id)
            .unwrap()
            .voxels
            .content_digest(),
        before
    );
    assert_eq!(files.len(), 1);
}

#[test]
fn a_ship_brought_in_is_announced_to_the_rules_as_voxels_that_were_just_made_in_the_channels_they_watch()
 {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    let ship = game.sample_ship(&world, 0);
    let id = world.grids_mut().create(ship);
    let layout = Arc::clone(world.main().overrides.layout());
    let block = layout.channel_index(&id_of("core:block")).unwrap();
    let dirt = game.raw_block("base_blocks:dirt");

    let heard = Arc::new(Mutex::new(Vec::new()));
    let hearing = Arc::clone(&heard);
    let logic =
        ScriptedLogic::new().on_tick(move |tick, world: &mut ServerWorld| match tick.get() {
            1 => {
                world.watch_channels(BTreeSet::from([block]));
                world.park_ship(id, false).unwrap();
            }
            2 => {
                // An edit of the terrain is heard of first, and the ship after it.
                world
                    .grid(GridId::root(LevelIndex::MAIN))
                    .unwrap()
                    .set(block, VoxelCoordinate::new(1, 2, 3), dirt)
                    .unwrap();
                world.load_ship(id).unwrap();
                hearing.lock().unwrap().push(world.take_changes());
                // Said once.
                hearing.lock().unwrap().push(world.take_changes());
            }
            _ => {}
        });
    let files = MemoryShipFiles::new();
    let mut server = server_for(&game, world, &files, logic);
    server.step();
    server.step();

    let heard = heard.lock().unwrap();
    // The edit of the terrain, and then the 12 blocks of the deck and the marble one: every voxel of the ship that is not at the default
    // of the block channel, and nothing of the temperature of the marble block, which nobody watches.
    assert_eq!(heard[0].len(), 14);
    assert_eq!(heard[0][0].grid, GridId::MAIN);
    assert_eq!(heard[0][0].voxel, VoxelCoordinate::new(1, 2, 3));
    let changes = &heard[0][1..];
    assert_eq!(changes.len(), 13);
    assert!(
        changes
            .iter()
            .all(|change| change.grid == id && change.channel == block)
    );
    let places: Vec<_> = changes
        .iter()
        .map(|change| {
            let (brick, local) = change.voxel.split();
            (brick, local.index())
        })
        .collect();
    let mut sorted = places.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        places, sorted,
        "bricks in order, and the voxels of a brick in order"
    );
    assert!(
        changes
            .iter()
            .any(|change| change.voxel == VoxelCoordinate::new(40, 33, 2))
    );
    assert!(
        changes
            .iter()
            .any(|change| change.voxel == VoxelCoordinate::new(3, 1, 2))
    );
    assert!(heard[1].is_empty());
}

fn id_of(text: &str) -> NamespacedId {
    id(text)
}

#[test]
fn a_ship_read_from_its_file_and_changed_is_the_ship_brought_in_and_the_rules_hear_of_what_it_holds()
 {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    let ship = game.sample_ship(&world, 0);
    let id = world.grids_mut().create(ship);
    let layout = Arc::clone(world.main().overrides.layout());
    let block = layout.channel_index(&id_of("core:block")).unwrap();

    let heard = Arc::new(Mutex::new(Vec::new()));
    let hearing = Arc::clone(&heard);
    let logic =
        ScriptedLogic::new().on_tick(move |tick, world: &mut ServerWorld| match tick.get() {
            1 => {
                world.watch_channels(BTreeSet::from([block]));
                world.park_ship(id, false).unwrap();
            }
            2 => {
                // Read, not brought in; then changed (one of the 13 blocks taken out) and brought in.
                let mut ship = world.read_parked_ship(id).unwrap();
                assert!(world.data().grids().get(id).is_none());
                ship.voxels.set(
                    block,
                    VoxelCoordinate::new(3, 1, 2),
                    layout.default_value(block),
                );
                world.admit_ship(id, ship).unwrap();
                hearing.lock().unwrap().push(world.take_changes());
            }
            _ => {}
        });
    let files = MemoryShipFiles::new();
    let mut server = server_for(&game, world, &files, logic);
    server.step();
    server.step();
    // 12 voxels are heard of, not 13: the ship that was brought in is the changed one.
    let heard = heard.lock().unwrap();
    assert_eq!(heard[0].len(), 12);
    assert!(heard[0].iter().all(|change| change.grid == id));
    assert!(
        !heard[0]
            .iter()
            .any(|change| change.voxel == VoxelCoordinate::new(3, 1, 2))
    );
}
