//! Ships in a saved world: a file for each, named by the world file; the counter and the retired ids; what is aboard; and a save that, however
//! it ends, leaves a world that can be read.

use std::collections::BTreeSet;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};

use proptest::prelude::*;
use voxel_space::{BrickCoordinate, GridId, LatticeRotation, LatticeTransform, LevelIndex};
use world_storage::container::{read_container, write_container};
use world_storage::engine_sections::{
    GridRecord, GridsRecord, decode_grids, encode_grids, grids_id,
};
use world_storage::test_support::{ALL_PLUGINS, Game, id, main_level_id, sample_world};
use world_storage::{
    LoadError, MemoryShipFiles, MovingGrid, ParkedGrid, SaveError, ShipFileName, ShipFiles,
    ShipLoadError, ShipParkError, WorldData,
};

const FIRST: u64 = GridId::FIRST_ALLOCATED;

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |state, byte| {
        (state ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The sample world with two ships in memory.
fn world_with_ships(game: &Game) -> (WorldData, GridId, GridId) {
    let mut world = sample_world(game);
    let (one, two) = (game.sample_ship(&world, 0), game.sample_ship(&world, 100));
    let first = world.grids_mut().create(one);
    let second = world.grids_mut().create(two);
    (world, first, second)
}

fn name(grid: GridId, serial: u64) -> ShipFileName {
    ShipFileName::new(grid, serial)
}

/// What a ship is, to compare ships before and after a save. (How much it weighs is the bodies' to say again, and is not saved.)
fn essence(ship: &MovingGrid) -> String {
    format!(
        "{:?}",
        (
            ship.level,
            ship.pose,
            ship.linear_velocity,
            ship.angular_velocity,
            ship.held,
            ship.on_pilot_logout,
            ship.voxels.content_digest(),
            &ship.sections,
        )
    )
}

fn grids_of(world_file: &[u8]) -> GridsRecord {
    let section = read_container(world_file)
        .unwrap()
        .into_iter()
        .find(|section| section.identifier == grids_id())
        .expect("the file has ships");
    decode_grids(&section.payload).unwrap()
}

#[test]
fn a_world_with_ships_is_a_world_file_and_a_file_for_each_ship_that_the_world_file_names() {
    let game = Game::new(&ALL_PLUGINS);
    let (mut world, first, second) = world_with_ships(&game);
    let save = game.save_with_ships(&mut world);

    // The files are numbered from the world's counter, in order of the ships.
    assert_eq!(
        save.new_ship_files
            .iter()
            .map(|(file, _)| *file)
            .collect::<Vec<_>>(),
        [name(first, 0), name(second, 1)]
    );
    assert_eq!(
        save.named_ship_files,
        BTreeSet::from([name(first, 0), name(second, 1)])
    );
    // The world file lists them in order, with where they are, and which file and what checksum.
    let record = grids_of(&save.world);
    assert_eq!(record.next, FIRST + 2);
    assert_eq!(record.next_serial, 2);
    assert!(record.tombstones.is_empty());
    assert_eq!(
        record.ships.iter().map(|ship| ship.id).collect::<Vec<_>>(),
        [first, second]
    );
    for (listed, (file, bytes)) in record.ships.iter().zip(&save.new_ship_files) {
        let ship = world.grids().get(listed.id).unwrap();
        assert_eq!(listed.level, main_level_id());
        assert_eq!(listed.pose, ship.pose);
        assert_eq!(listed.bounds, ship.world_brick_bounds());
        assert_eq!(*file, name(listed.id, listed.serial));
        assert_eq!(listed.checksum, crc32fast::hash(bytes));
        assert!(!listed.waits_for_those_aboard);
    }
    // The pose and the bricks it touches are not the same for the two (they were made a hundred voxels apart).
    assert_ne!(record.ships[0].bounds, record.ships[1].bounds);
    // It is the same on every machine (the hash was taken on Windows and Linux).
    assert_eq!(fnv(&save.world), PINNED_WORLD, "{} bytes", save.world.len());
}

/// The hash of the world file of the scenario above. (Moved on purpose, Phase 38a: 3_028_195_094_562_407_983 before. The ships section is version 2 and each of the two
/// poses has four `f64` for its turn where it had four `f32`: 16 bytes each, 34,105 to 34,137 (the length at the old commit measured, not assumed); the checksums of the ship files
/// in the section changed with the files. Taken on Windows; Linux is checked to agree.)
const PINNED_WORLD: u64 = 13_693_694_158_651_394_378;

#[test]
fn a_world_loaded_has_its_ships_in_their_files_and_asks_for_them_one_by_one_as_they_were() {
    let game = Game::new(&ALL_PLUGINS);
    let (mut world, first, second) = world_with_ships(&game);
    let save = game.save_with_ships(&mut world);
    let mut files = MemoryShipFiles::new();
    for (file, bytes) in &save.new_ship_files {
        files.write(file, bytes).unwrap();
    }

    let mut other = Game::new(&ALL_PLUGINS);
    let (mut loaded, report) = other.load(&save.world).unwrap();
    assert!(report.missing_content.is_empty());
    // No ship is in memory, and the world knows where each is without reading its file.
    assert_eq!(loaded.grids().len(), 0);
    assert_eq!(loaded.grids().parked().len(), 2);
    for (id, serial) in [(first, 0), (second, 1)] {
        let ship = world.grids().get(id).unwrap();
        let parked = loaded.grids().get_parked(id).unwrap();
        assert_eq!(parked.level, ship.level);
        assert_eq!(parked.pose, ship.pose);
        assert_eq!(parked.bounds, ship.world_brick_bounds());
        assert_eq!(parked.file, name(id, serial));
        assert!(loaded.grids().knows(id));
        assert_eq!(loaded.grids().frame_of(id), Some((ship.level, ship.pose)));
    }
    assert!(!loaded.grids().knows(GridId::new(FIRST + 2)));

    // Asked for, a ship comes into memory as it was, and the other stays where it is.
    let format = other.ship_format(&loaded);
    loaded.load_ship(second, &files, &format).unwrap();
    assert_eq!(loaded.grids().len(), 1);
    assert!(loaded.grids().get_parked(second).is_none());
    assert!(loaded.grids().get_parked(first).is_some());
    assert_eq!(
        essence(loaded.grids().get(second).unwrap()),
        essence(world.grids().get(second).unwrap())
    );
    other.load_all_ships(&mut loaded, &files);
    assert_eq!(loaded.grids().len(), 2);
    assert_eq!(
        essence(loaded.grids().get(first).unwrap()),
        essence(world.grids().get(first).unwrap())
    );
    // A ship that is in memory cannot be asked for again, and one that never was cannot be asked for at all.
    assert!(matches!(
        loaded.load_ship(first, &files, &format),
        Err(ShipLoadError::NotParked(_))
    ));
    assert!(matches!(
        loaded.load_ship(GridId::new(FIRST + 9), &files, &format),
        Err(ShipLoadError::NotParked(_))
    ));
    // Ids go on from where they were.
    let next = other.sample_ship(&loaded, 5);
    assert_eq!(loaded.grids_mut().create(next).get(), FIRST + 2);
}

#[test]
fn a_world_loaded_by_a_game_with_other_numbers_for_content_reads_the_ships_all_the_same() {
    let writer = Game::new(&[
        world_storage::test_support::Plugin::BaseBlocks,
        world_storage::test_support::Plugin::FancyBlocks,
        world_storage::test_support::Plugin::Magic,
        world_storage::test_support::Plugin::FancyTerrain,
    ]);
    let (mut world, first, _) = world_with_ships(&writer);
    let save = writer.save_with_ships(&mut world);
    let mut files = MemoryShipFiles::new();
    for (file, bytes) in &save.new_ship_files {
        files.write(file, bytes).unwrap();
    }
    // The reader has two blocks more, registered first, so that all the numbers of the blocks are others.
    let mut reader = Game::new(&ALL_PLUGINS);
    assert_ne!(
        reader.raw_block("base_blocks:stone"),
        writer.raw_block("base_blocks:stone")
    );
    let (mut loaded, _) = reader.load(&save.world).unwrap();
    reader.load_all_ships(&mut loaded, &files);
    let meaning = |game: &Game, world: &WorldData| {
        let ship = world.grids().get(first).unwrap();
        ship.voxels
            .sorted_brick_coordinates()
            .into_iter()
            .map(|brick| {
                game.brick_meaning(ship.voxels.layout(), ship.voxels.brick(brick).unwrap())
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(meaning(&reader, &loaded), meaning(&writer, &world));
}

#[test]
fn the_counter_of_ship_ids_survives_when_every_ship_is_gone_and_a_world_that_never_had_one_writes_nothing_of_them()
 {
    let game = Game::new(&ALL_PLUGINS);
    let plain = sample_world(&game);
    let sections: Vec<_> = read_container(&game.save(&plain))
        .unwrap()
        .into_iter()
        .map(|section| section.identifier)
        .collect();
    assert!(!sections.contains(&grids_id()));
    assert!(
        !read_container(&plain.describe(&game.registries()).unwrap())
            .unwrap()
            .iter()
            .any(|section| section.identifier == grids_id())
    );

    let (mut world, first, second) = world_with_ships(&game);
    world.grids_mut().remove(first).unwrap();
    world.grids_mut().remove(second).unwrap();
    assert!(world.grids().is_empty());
    let bytes = game.save(&world);
    let record = grids_of(&bytes);
    assert_eq!(record.next, FIRST + 2);
    assert!(record.ships.is_empty());
    let (mut loaded, _) = Game::new(&ALL_PLUGINS).load(&bytes).unwrap();
    let again = game.sample_ship(&loaded, 0);
    assert_eq!(loaded.grids_mut().create(again).get(), FIRST + 2);
    // A description for clients is of the world's shape, and has no ships in it.
    assert!(
        !read_container(&world.describe(&game.registries()).unwrap())
            .unwrap()
            .iter()
            .any(|section| section.identifier == grids_id())
    );
}

#[test]
fn a_world_file_alone_cannot_hold_a_ship_that_is_in_memory_and_says_so() {
    let game = Game::new(&ALL_PLUGINS);
    let (world, ..) = world_with_ships(&game);
    assert!(matches!(
        world.save(&game.registries()),
        Err(SaveError::ShipsInMemory(2))
    ));
}

#[test]
fn the_ids_that_were_retired_and_what_took_their_places_are_saved_with_the_turns_between() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = game.new_world();
    let ship = game.sample_ship(&world, 0);
    let old = world.grids_mut().create(ship.clone());
    let turn = LatticeRotation::all()
        .into_iter()
        .find(|rotation| {
            rotation.apply([1, 0, 0]) == [0, 0, -1] && rotation.apply([0, 1, 0]) == [0, 1, 0]
        })
        .unwrap();
    let transform = LatticeTransform {
        rotation: turn,
        offset: [-40, 3, 1_000_000_000_000],
    };
    let (_, parts) = world
        .grids_mut()
        .retire(
            old,
            vec![
                (ship.clone(), transform),
                (ship.clone(), LatticeTransform::IDENTITY),
            ],
        )
        .unwrap();
    let merged = world
        .grids_mut()
        .merge(&[(parts[1], LatticeTransform::IDENTITY)], ship)
        .unwrap()
        .1;
    // Two ships made, one retired into two parts, one of those merged into a third.
    assert_eq!(merged.get(), FIRST + 3);
    let save = game.save_with_ships(&mut world);

    let mut other = Game::new(&ALL_PLUGINS);
    let (loaded, _) = other.load(&save.world).unwrap();
    assert_eq!(loaded.grids().next_id(), FIRST + 4);
    for id in [old, parts[0], parts[1], merged, GridId::new(FIRST + 20)] {
        assert_eq!(
            loaded.grids().resolve(id),
            world.grids().resolve(id),
            "{id}"
        );
    }
    assert_eq!(loaded.grids().resolve(old).len(), 2);
    assert_eq!(loaded.grids().resolve(old)[0], (parts[0], transform));
    assert_eq!(
        loaded.grids().tombstones().collect::<Vec<_>>(),
        world.grids().tombstones().collect::<Vec<_>>()
    );
    // The retired ids are not ships, and nothing is a file for them.
    assert!(!loaded.grids().knows(old));
    assert_eq!(save.new_ship_files.len(), 2);
}

#[test]
fn a_ship_in_another_level_is_in_it_again_and_one_in_a_level_the_world_lacks_is_refused() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = game.new_world();
    let nether = world
        .add_level(
            id("test_content:nether"),
            game.new_level("base_terrain:flat_v1"),
        )
        .unwrap();
    let mut ship = game.sample_ship(&world, 0);
    ship.level = nether;
    let in_the_nether = world.grids_mut().create(ship);
    let save = game.save_with_ships(&mut world);

    let mut other = Game::new(&ALL_PLUGINS);
    let (loaded, _) = other.load(&save.world).unwrap();
    assert_eq!(
        loaded.grids().get_parked(in_the_nether).unwrap().level,
        nether
    );
    assert_eq!(loaded.level_id(nether), Some(&id("test_content:nether")));

    // A world file that puts a ship in a level it does not have is refused, not guessed at.
    let mut record = grids_of(&save.world);
    record.ships[0].level = id("test_content:the_void");
    let mut sections = read_container(&save.world).unwrap();
    sections
        .iter_mut()
        .find(|section| section.identifier == grids_id())
        .unwrap()
        .payload = encode_grids(&record);
    let error = Game::new(&ALL_PLUGINS)
        .load(&write_container(&sections))
        .unwrap_err();
    assert!(
        matches!(&error, LoadError::ShipInUnknownLevel { ship, .. } if *ship == in_the_nether),
        "{error}"
    );
}

#[test]
fn a_ship_put_in_a_file_and_asked_for_again_is_the_same_and_one_that_cannot_be_written_stays() {
    let game = Game::new(&ALL_PLUGINS);
    let (mut world, first, second) = world_with_ships(&game);
    let format = game.ship_format(&world);
    let before = essence(world.grids().get(first).unwrap());
    let mut files = MemoryShipFiles::new();

    world.park_ship(first, true, &mut files, &format).unwrap();
    assert!(world.grids().get(first).is_none());
    let parked = world.grids().get_parked(first).unwrap().clone();
    assert_eq!(parked.file, name(first, 0));
    assert!(parked.waits_for_those_aboard);
    assert_eq!(files.names().unwrap(), [name(first, 0)]);
    assert_eq!(
        parked.checksum,
        crc32fast::hash(&files.read(&name(first, 0)).unwrap())
    );
    // The world still knows where it is.
    assert_eq!(world.grids().frame_of(first).unwrap().0, LevelIndex::MAIN);

    world.load_ship(first, &files, &format).unwrap();
    assert_eq!(essence(world.grids().get(first).unwrap()), before);
    // (It is asked for, so it is no longer waiting for anyone: that was a note on the record, which is gone.)
    assert!(world.grids().get_parked(first).is_none());

    // A ship that is not in memory cannot be put away.
    world.park_ship(second, false, &mut files, &format).unwrap();
    assert!(matches!(
        world.park_ship(second, false, &mut files, &format),
        Err(ShipParkError::NotLoaded(_))
    ));
    // A file that cannot be written leaves the ship in memory and the world as it was.
    let mut failing = Flaky::new(MemoryShipFiles::new(), Some(0));
    let was = world.grids().get(first).unwrap().voxels.content_digest();
    assert!(matches!(
        world.park_ship(first, false, &mut failing, &format),
        Err(ShipParkError::Writing { .. })
    ));
    assert_eq!(
        world.grids().get(first).unwrap().voxels.content_digest(),
        was
    );
    assert!(world.grids().get_parked(first).is_none());
    assert!(failing.inner.is_empty());
}

#[test]
fn a_ship_can_be_read_from_its_file_without_being_brought_in_and_brought_in_as_it_was_changed() {
    let game = Game::new(&ALL_PLUGINS);
    let (mut world, first, _) = world_with_ships(&game);
    let format = game.ship_format(&world);
    let mut files = MemoryShipFiles::new();
    world.park_ship(first, false, &mut files, &format).unwrap();
    let original = game.sample_ship(&world, 0);

    // Read, it is the ship that was put away, and the world has not changed.
    let mut ship = world.read_ship(first, &files, &format).unwrap();
    assert_eq!(essence(&ship), essence(&original));
    assert!(world.grids().get(first).is_none() && world.grids().get_parked(first).is_some());
    // Read again, it is read again.
    assert_eq!(
        essence(&world.read_ship(first, &files, &format).unwrap()),
        essence(&ship)
    );

    // Changed (moved, and a block taken out), it is the ship that is brought in.
    let block = ship
        .voxels
        .layout()
        .channel_index(&id("core:block"))
        .unwrap();
    ship.voxels.set(
        block,
        voxel_space::VoxelCoordinate::new(0, 1, 0),
        ship.voxels.layout().default_value(block),
    );
    ship.pose = voxel_space::GridPose::identity();
    let changed = essence(&ship);
    world.admit_ship(first, ship).unwrap();
    assert!(world.grids().get_parked(first).is_none());
    assert_eq!(essence(world.grids().get(first).unwrap()), changed);
    assert_ne!(changed, essence(&original));
    // It cannot be brought in twice, or as a ship that was never put away.
    let again = world.grids().get(first).unwrap().clone();
    assert!(matches!(
        world.admit_ship(first, again.clone()),
        Err(ShipLoadError::NotParked(_))
    ));
    assert!(matches!(
        world.admit_ship(GridId::new(FIRST + 70), again),
        Err(ShipLoadError::NotParked(_))
    ));
    assert!(matches!(
        world.read_ship(first, &files, &format),
        Err(ShipLoadError::NotParked(_))
    ));
}

#[test]
fn a_file_that_is_missing_or_not_the_one_saved_or_damaged_is_refused_and_the_ship_stays_in_its_file()
 {
    let game = Game::new(&ALL_PLUGINS);
    let (mut world, first, second) = world_with_ships(&game);
    let save = game.save_with_ships(&mut world);
    let mut files = MemoryShipFiles::new();
    for (file, bytes) in &save.new_ship_files {
        files.write(file, bytes).unwrap();
    }
    let mut other = Game::new(&ALL_PLUGINS);
    let (mut loaded, _) = other.load(&save.world).unwrap();
    let format = other.ship_format(&loaded);

    // A file that is not there.
    let mut gone = files.duplicate();
    gone.remove(&name(first, 0)).unwrap();
    let error = loaded.load_ship(first, &gone, &format).unwrap_err();
    assert!(
        matches!(&error, ShipLoadError::Unreadable { source, .. } if source.kind() == io::ErrorKind::NotFound),
        "{error}"
    );

    // A byte of it changed, anywhere.
    let original = files.read(&name(first, 0)).unwrap();
    for at in (0..original.len()).step_by(97).chain([original.len() - 1]) {
        let mut damaged = files.duplicate();
        let mut bytes = original.clone();
        bytes[at] ^= 0x04;
        damaged.write(&name(first, 0), &bytes).unwrap();
        let error = loaded.load_ship(first, &damaged, &format).unwrap_err();
        assert!(
            matches!(error, ShipLoadError::WrongFile { .. }),
            "byte {at}: {error}"
        );
    }

    // Another ship's file under its name.
    let mut swapped = files.duplicate();
    swapped
        .write(&name(first, 0), &files.read(&name(second, 1)).unwrap())
        .unwrap();
    assert!(matches!(
        loaded.load_ship(first, &swapped, &format),
        Err(ShipLoadError::WrongFile { .. })
    ));

    // A file the world saved that is not a ship file at all (the world has the checksum of what it was told).
    let mut nonsense = files.duplicate();
    let rubbish = b"this is not a ship".to_vec();
    nonsense.write(&name(first, 0), &rubbish).unwrap();
    let record = loaded.grids().get_parked(first).unwrap().clone();
    loaded.grids_mut().get_parked_mut(first).unwrap().checksum = crc32fast::hash(&rubbish);
    assert!(matches!(
        loaded.load_ship(first, &nonsense, &format),
        Err(ShipLoadError::Damaged { .. })
    ));
    *loaded.grids_mut().get_parked_mut(first).unwrap() = record;

    // Nothing changed: both ships are still in their files, and the intact file still loads.
    assert_eq!(loaded.grids().len(), 0);
    assert_eq!(loaded.grids().parked().len(), 2);
    loaded.load_ship(first, &files, &format).unwrap();
    assert_eq!(
        essence(loaded.grids().get(first).unwrap()),
        essence(world.grids().get(first).unwrap())
    );
}

/// Files that fail on one of the calls made to them.
#[derive(Debug)]
struct Flaky {
    inner: MemoryShipFiles,
    calls: AtomicUsize,
    fail_at: Option<usize>,
}

impl Flaky {
    fn new(inner: MemoryShipFiles, fail_at: Option<usize>) -> Self {
        Self {
            inner,
            calls: AtomicUsize::new(0),
            fail_at,
        }
    }

    fn call(&self) -> io::Result<()> {
        if self.fail_at == Some(self.calls.fetch_add(1, Ordering::SeqCst)) {
            Err(io::Error::other("the disk failed"))
        } else {
            Ok(())
        }
    }
}

impl ShipFiles for Flaky {
    fn read(&self, file: &ShipFileName) -> io::Result<Vec<u8>> {
        self.call()?;
        self.inner.read(file)
    }

    fn write(&mut self, file: &ShipFileName, bytes: &[u8]) -> io::Result<()> {
        self.call()?;
        self.inner.write(file, bytes)
    }

    fn remove(&mut self, file: &ShipFileName) -> io::Result<()> {
        self.call()?;
        self.inner.remove(file)
    }

    fn names(&self) -> io::Result<Vec<ShipFileName>> {
        self.call()?;
        self.inner.names()
    }
}

/// Every ship the world file names can be read, from these files, and the world is whole.
fn assert_a_whole_world(game: &Game, world_file: &[u8], files: &dyn ShipFiles) -> WorldData {
    let mut other = Game::new(&game.plugins);
    let (mut world, _) = other.load(world_file).expect("the world file loads");
    other.load_all_ships(&mut world, files);
    world
}

#[test]
fn a_save_that_goes_wrong_anywhere_leaves_a_world_that_can_be_read_and_the_next_save_makes_it_tidy()
{
    let game = Game::new(&ALL_PLUGINS);
    let (mut world, first, second) = world_with_ships(&game);

    // The first save, to a disk that has nothing.
    let mut files = MemoryShipFiles::new();
    let mut on_disk: Vec<u8> = Vec::new();
    let one = game.save_with_ships(&mut world);
    one.write_with(&mut files, |bytes| {
        on_disk = bytes.to_vec();
        Ok(())
    })
    .unwrap();
    assert_eq!(on_disk, one.world);
    assert_eq!(files.names().unwrap(), [name(first, 0), name(second, 1)]);

    // Then the world moves on: one ship is moved, one is put away, a third is made.
    let format = game.ship_format(&world);
    world.grids_mut().get_mut(first).unwrap().pose.position =
        voxel_space::WorldPosition::new(BrickCoordinate::new(9, 9, 9), [1.0; 3]);
    world.park_ship(second, false, &mut files, &format).unwrap();
    let third_ship = game.sample_ship(&world, 7);
    let third = world.grids_mut().create(third_ship);
    let two = game.save_with_ships(&mut world);
    // (the one that was put away is named as it is, and not written again)
    assert_eq!(
        two.new_ship_files
            .iter()
            .map(|(file, _)| *file)
            .collect::<Vec<_>>(),
        [name(first, 3), name(third, 4)]
    );
    assert!(two.named_ship_files.contains(&name(second, 2)));

    // How many calls the whole of the second save makes on the disk.
    let calls = {
        let counting = Flaky::new(files.duplicate(), None);
        let mut counting = counting;
        two.write_with(&mut counting, |_| Ok(())).unwrap();
        counting.calls.load(Ordering::SeqCst)
    };
    assert!(calls >= 5, "{calls}");

    for fail_at in 0..calls {
        let mut disk = Flaky::new(files.duplicate(), Some(fail_at));
        let mut world_file = on_disk.clone();
        let result = two.write_with(&mut disk, |bytes| {
            world_file = bytes.to_vec();
            Ok(())
        });
        // Whichever call failed, the world file there is the first save's or the second's, and every ship it names can be read.
        let read = assert_a_whole_world(&game, &world_file, &disk.inner);
        if world_file == one.world {
            assert_eq!(read.grids().len(), 2, "call {fail_at}");
            assert_eq!(
                essence(read.grids().get(second).unwrap()),
                essence(&game.sample_ship(&read, 100))
            );
        } else {
            assert_eq!(world_file, two.world, "call {fail_at}");
            assert_eq!(read.grids().len(), 3, "call {fail_at}");
        }
        // A save that failed before the world file was written has not touched the world file; one that failed after has still
        // written all of it.
        assert_eq!(result.is_ok(), world_file == two.world, "call {fail_at}");
        // The next save, when the disk works, finishes the job and leaves nothing but what it names.
        two.write_with(&mut disk.inner, |bytes| {
            world_file = bytes.to_vec();
            Ok(())
        })
        .unwrap();
        assert_eq!(world_file, two.world);
        assert_eq!(
            disk.inner
                .names()
                .unwrap()
                .into_iter()
                .collect::<BTreeSet<_>>(),
            two.named_ship_files
        );
        assert_a_whole_world(&game, &world_file, &disk.inner);
    }

    // A world file that cannot be written leaves the old one and the files it names, and the new ship files that were written are
    // nobody's until a save succeeds.
    let mut disk = files.duplicate();
    let error = two
        .write_with(&mut disk, |_| Err(io::Error::other("the disk is full")))
        .unwrap_err();
    assert_eq!(error.to_string(), "the disk is full");
    assert_a_whole_world(&game, &on_disk, &disk);
    assert!(disk.names().unwrap().contains(&name(first, 3)));
    two.write_with(&mut disk, |_| Ok(())).unwrap();
    assert_eq!(
        disk.names().unwrap().into_iter().collect::<BTreeSet<_>>(),
        two.named_ship_files
    );

    // A disk that cannot say what is on it, or cannot remove, does not fail a save that is done.
    for fail_at in [calls - 1, calls - 2] {
        let mut disk = Flaky::new(files.duplicate(), Some(fail_at));
        let mut world_file = on_disk.clone();
        let _ = two.write_with(&mut disk, |bytes| {
            world_file = bytes.to_vec();
            Ok(())
        });
        assert_eq!(world_file, two.world, "call {fail_at}");
    }
}

#[test]
fn a_ship_that_was_put_away_is_not_written_again_by_a_save_and_its_file_stays() {
    let game = Game::new(&ALL_PLUGINS);
    let (mut world, first, _) = world_with_ships(&game);
    let format = game.ship_format(&world);
    let mut files = MemoryShipFiles::new();
    world.park_ship(first, false, &mut files, &format).unwrap();
    let save = game.save_with_ships(&mut world);
    assert!(
        save.new_ship_files
            .iter()
            .all(|(file, _)| file.grid != first)
    );
    assert!(save.named_ship_files.contains(&name(first, 0)));
    save.write_with(&mut files, |_| Ok(())).unwrap();
    assert!(files.read(&name(first, 0)).is_ok());
    // A world with only ships in files saves without a ship in memory, to the same bytes as the world saved with them.
    let (mut empty, ..) = world_with_ships(&game);
    for id in [first, GridId::new(FIRST + 1)] {
        empty
            .park_ship(id, false, &mut MemoryShipFiles::new(), &format)
            .unwrap();
    }
    assert!(empty.save(&game.registries()).is_ok());
}

fn sample_record() -> GridsRecord {
    let ship = |raw: u64, bounds| GridRecord {
        id: GridId::new(raw),
        level: main_level_id(),
        pose: world_storage::test_support::Game::new(&ALL_PLUGINS)
            .sample_ship(&Game::new(&ALL_PLUGINS).new_world(), 0)
            .pose,
        bounds,
        serial: 17,
        checksum: 0xdead_beef,
        waits_for_those_aboard: raw.is_multiple_of(2),
    };
    GridsRecord {
        next: FIRST + 9,
        next_serial: 40,
        ships: vec![
            ship(FIRST + 1, None),
            ship(
                FIRST + 4,
                voxel_space::BrickBox::new(
                    BrickCoordinate::new(-2, 0, 1),
                    BrickCoordinate::new(3, 4, 5),
                ),
            ),
        ],
        tombstones: vec![(
            GridId::new(FIRST),
            vec![(
                GridId::new(FIRST + 1),
                LatticeTransform {
                    rotation: LatticeRotation::all()[17],
                    offset: [1, -2, 3],
                },
            )],
        )],
    }
}

#[test]
fn the_ships_section_round_trips_and_refuses_what_cannot_be_right() {
    let good = sample_record();
    let bytes = encode_grids(&good);
    assert_eq!(decode_grids(&bytes).unwrap(), good);
    for length in 0..bytes.len() {
        assert!(decode_grids(&bytes[..length]).is_err(), "cut at {length}");
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(decode_grids(&trailing).is_err());

    let broken = |change: &dyn Fn(&mut GridsRecord)| {
        let mut record = sample_record();
        change(&mut record);
        decode_grids(&encode_grids(&record))
    };
    let first = GridId::new(FIRST);
    assert!(
        broken(&|record| record.next = FIRST - 1).is_err(),
        "a counter below the first id"
    );
    assert!(
        broken(&|record| record.ships[1].id = GridId::new(FIRST + 9)).is_err(),
        "a ship the world has not given out"
    );
    assert!(
        broken(&|record| record.ships[0].id = GridId::new(5)).is_err(),
        "a ship that is a level"
    );
    assert!(
        broken(&|record| record.ships[0].serial = record.next_serial).is_err(),
        "a file with a number that has not been given out"
    );
    assert!(
        broken(&|record| record.ships[1].id = record.ships[0].id).is_err(),
        "a ship twice"
    );
    assert!(
        broken(&|record| record.ships.swap(0, 1)).is_err(),
        "ships out of order"
    );
    assert!(
        broken(&|record| record.tombstones[0].0 = GridId::new(FIRST + 4)).is_err(),
        "a ship that is also retired"
    );
    assert!(
        broken(&|record| record.tombstones[0].0 = GridId::new(FIRST + 20)).is_err(),
        "a retired id not given out"
    );
    assert!(
        broken(&|record| record.tombstones[0].1.clear()).is_err(),
        "nothing in the place of a retired id"
    );
    assert!(
        broken(&|record| record.tombstones[0].1[0].0 = GridId::new(FIRST + 30)).is_err(),
        "taken by a ship not given out"
    );
    assert!(
        broken(&|record| record.tombstones[0].1[0].0 = first).is_err(),
        "retired into itself"
    );
    assert!(
        broken(&|record| record
            .tombstones
            .push((first, record.tombstones[0].1.clone())))
        .is_err(),
        "a retired id twice"
    );
    // A turn that is not one of the 24: the byte after the successor's id.
    let mut bad_turn = bytes.clone();
    let turn_at = bad_turn.len() - 1 - 3;
    assert!(LatticeRotation::all().len() == 24 && bad_turn[turn_at] == 17);
    bad_turn[turn_at] = 24;
    assert!(decode_grids(&bad_turn).is_err());
}

proptest! {
    #[test]
    fn arbitrary_bytes_are_never_a_ships_section_and_never_a_panic(bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
        let _ = decode_grids(&bytes);
    }

    #[test]
    fn any_ships_section_a_world_could_write_reads_back_the_same(
        count in 0usize..5,
        retired in 0usize..4,
        serial in 0u64..1000,
        checksum in any::<u32>(),
        offset in any::<[i64; 3]>(),
        turn in 0usize..24,
        waits in any::<bool>(),
    ) {
        let pose = Game::new(&ALL_PLUGINS)
            .sample_ship(&Game::new(&ALL_PLUGINS).new_world(), 0)
            .pose;
        let record = GridsRecord {
            next: FIRST + 100,
            next_serial: 1000,
            ships: (0..count as u64).map(|number| GridRecord {
                id: GridId::new(FIRST + 10 + number),
                level: main_level_id(),
                pose,
                bounds: None,
                serial,
                checksum,
                waits_for_those_aboard: waits,
            }).collect(),
            tombstones: (0..retired as u64).map(|number| (
                GridId::new(FIRST + number),
                vec![(GridId::new(FIRST + 50 + number), LatticeTransform { rotation: LatticeRotation::all()[turn], offset })],
            )).collect(),
        };
        prop_assert_eq!(decode_grids(&encode_grids(&record)).unwrap(), record);
    }
}

#[test]
fn a_parked_record_is_what_a_world_needs_to_find_a_ship_without_its_file() {
    // The record is plain data: a program can make one for a file it has (as importing a ship will).
    let game = Game::new(&ALL_PLUGINS);
    let (mut world, first, _) = world_with_ships(&game);
    let ship = world.grids().get(first).unwrap().clone();
    let record = ParkedGrid {
        level: ship.level,
        pose: ship.pose,
        bounds: ship.world_brick_bounds(),
        file: name(first, 1),
        checksum: 0,
        waits_for_those_aboard: false,
    };
    let taken = world.grids_mut().park(first, record.clone()).unwrap();
    assert_eq!(taken, ship);
    assert_eq!(world.grids().get_parked(first), Some(&record));
    assert!(world.grids_mut().park(first, record).is_none());
    assert!(world.grids_mut().unpark(first, taken.clone()).is_some());
    assert!(world.grids_mut().unpark(first, taken).is_none());
}
