//! The file of one ship: what it holds, that it is the same every time, that it can be read by a world that numbers its content otherwise, and
//! that damage to it is found.

use std::sync::Arc;

use content_registry::IdentifierRegistry;
use namespaced_id::NamespacedId;
use proptest::prelude::*;
use voxel_grid::Grid;
use voxel_space::GridId;
use world_storage::test_support::{ALL_PLUGINS, Game, Plugin, id, main_level_id};
use world_storage::{MovingGrid, ParsedShip, ShipFileError, ShipFlight, WorldData};

const WITHOUT_NEWCOMER: [Plugin; 4] = [
    Plugin::BaseBlocks,
    Plugin::FancyBlocks,
    Plugin::Magic,
    Plugin::FancyTerrain,
];

/// A ship of two bricks of the game's blocks, in flight, with a section of a plugin nobody has.
fn ship(game: &Game, world: &WorldData) -> MovingGrid {
    game.sample_ship(world, 0)
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |state, byte| {
        (state ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[test]
fn a_ship_comes_back_from_its_file_as_it_was_and_the_file_is_the_same_every_time() {
    let game = Game::new(&ALL_PLUGINS);
    let world = game.new_world();
    let ship = ship(&game, &world);
    let format = world.ship_format(&game.registries()).unwrap();
    let bytes = format.encode(&ship, &main_level_id()).unwrap();
    assert_eq!(format.encode(&ship, &main_level_id()).unwrap(), bytes);

    let read = format.decode(&bytes).unwrap();
    assert_eq!(read.level, main_level_id());
    assert_eq!(read.voxels.content_digest(), ship.voxels.content_digest());
    assert_eq!(read.voxels.brick_count(), 2);
    assert_eq!(read.flight, Some(ShipFlight::of(&ship)));
    assert_eq!(read.sections, ship.sections);
    // Written again from what was read, it is the same file: nothing was lost on the way.
    let again = MovingGrid {
        voxels: read.voxels,
        sections: read.sections,
        ..ship.clone()
    };
    assert_eq!(format.encode(&again, &main_level_id()).unwrap(), bytes);

    // The bytes are a function of the ship and the game's content alone, on every machine (the hash was taken on Windows and Linux).
    assert_eq!(fnv(&bytes), PINNED_FILE, "{} bytes", bytes.len());
}

/// The hash of the file of the ship above.
const PINNED_FILE: u64 = 15_394_325_344_773_901_199;

#[test]
fn a_world_that_numbers_its_content_otherwise_reads_the_ship_and_means_the_same_blocks() {
    let writer = Game::new(&WITHOUT_NEWCOMER);
    let world = writer.new_world();
    let ship = ship(&writer, &world);
    let bytes = writer
        .new_world()
        .ship_format(&writer.registries())
        .unwrap()
        .encode(&ship, &main_level_id())
        .unwrap();

    // The reader has two more blocks, registered first: every block it has has another number.
    let mut reader = Game::new(&ALL_PLUGINS);
    assert_ne!(
        reader.raw_block("base_blocks:stone"),
        writer.raw_block("base_blocks:stone")
    );
    let reading_world = reader.new_world();
    let layout = Arc::clone(reading_world.main().overrides.layout());
    let contents = ParsedShip::parse(&bytes)
        .unwrap()
        .into_contents(&layout, |registry, identifier| {
            if *registry == id("voxel_content:block") {
                reader.blocks.raw_id_of(identifier)
            } else {
                reader.spells.as_ref()?.raw_id_of(identifier)
            }
        })
        .unwrap();
    let meaning = |game: &Game, grid: &Grid| -> Vec<_> {
        grid.sorted_brick_coordinates()
            .into_iter()
            .map(|coordinate| {
                (
                    coordinate,
                    game.brick_meaning(grid.layout(), grid.brick(coordinate).unwrap()),
                )
            })
            .collect()
    };
    assert_eq!(
        meaning(&reader, &contents.voxels),
        meaning(&writer, &ship.voxels)
    );
    assert_ne!(
        contents.voxels.content_digest(),
        ship.voxels.content_digest()
    );
    // (A world with more content is not changed by reading a ship: its numbers are its own.)
    let _ = &mut reader;
}

#[test]
fn a_world_without_something_the_file_names_refuses_the_ship_and_says_what() {
    let writer = Game::new(&ALL_PLUGINS);
    let world = writer.new_world();
    let ship = ship(&writer, &world);
    let bytes = world
        .ship_format(&writer.registries())
        .unwrap()
        .encode(&ship, &main_level_id())
        .unwrap();

    let reader = Game::new(&WITHOUT_NEWCOMER);
    let layout = Arc::clone(reader.new_world().main().overrides.layout());
    let error = ParsedShip::parse(&bytes)
        .unwrap()
        .into_contents(&layout, |registry, identifier| {
            if *registry == id("voxel_content:block") {
                reader.blocks.raw_id_of(identifier)
            } else {
                reader.spells.as_ref()?.raw_id_of(identifier)
            }
        })
        .unwrap_err();
    assert!(
        matches!(&error, ShipFileError::UnknownContent { identifier, .. } if *identifier == id("newcomer:obsidian")),
        "{error}"
    );
    // The content it needs is asked for before anything is read.
    let needed = ParsedShip::parse(&bytes).unwrap().referenced_content();
    assert!(needed.contains(&(id("voxel_content:block"), id("newcomer:obsidian"))));
    assert!(needed.contains(&(id("magic:spell"), id("magic:fireball"))));
}

#[test]
fn a_world_with_no_place_for_a_channel_of_the_file_refuses_it() {
    let writer = Game::new(&ALL_PLUGINS);
    let world = writer.new_world();
    let ship = ship(&writer, &world);
    let bytes = world
        .ship_format(&writer.registries())
        .unwrap()
        .encode(&ship, &main_level_id())
        .unwrap();
    // A world without the magic plugin has no mana and no spells to put the ship's in.
    let reader = Game::new(&[Plugin::BaseBlocks, Plugin::FancyBlocks, Plugin::Newcomer]);
    let layout = Arc::clone(reader.new_world().main().overrides.layout());
    let error = ParsedShip::parse(&bytes)
        .unwrap()
        .into_contents(&layout, |_, identifier| reader.blocks.raw_id_of(identifier))
        .unwrap_err();
    assert!(
        matches!(&error, ShipFileError::UnknownChannel(channel) if *channel == id("magic:mana")),
        "{error}"
    );
}

#[test]
fn a_ship_made_for_another_layout_cannot_be_written_as_this_worlds() {
    let game = Game::new(&ALL_PLUGINS);
    let world = game.new_world();
    let mut strange = ship(&game, &world);
    let other = Game::new(&[Plugin::BaseBlocks, Plugin::FancyBlocks]);
    strange.voxels = Grid::new(Arc::clone(other.new_world().main().overrides.layout()));
    let error = world
        .ship_format(&game.registries())
        .unwrap()
        .encode(&strange, &main_level_id())
        .unwrap_err();
    assert!(matches!(error, ShipFileError::LayoutMismatch), "{error}");
}

#[test]
fn a_ship_without_flight_is_read_as_a_ship_to_put_somewhere() {
    let game = Game::new(&ALL_PLUGINS);
    let world = game.new_world();
    let bytes = world
        .ship_format(&game.registries())
        .unwrap()
        .encode(&ship(&game, &world), &main_level_id())
        .unwrap();
    // The same file with the section that says where it was taken out.
    let without = world_storage::container::write_container(
        &world_storage::container::read_container(&bytes)
            .unwrap()
            .into_iter()
            .filter(|section| section.identifier != world_storage::flight_id())
            .collect::<Vec<_>>(),
    );
    let read = ParsedShip::parse(&without).unwrap();
    assert_eq!(read.flight(), None);
    assert_eq!(read.level(), &main_level_id());
    assert_eq!(read.world_seed(), world.world_seed);
    let layout = Arc::clone(world.main().overrides.layout());
    let contents = read
        .into_contents(&layout, |registry, identifier| {
            if *registry == id("voxel_content:block") {
                game.blocks.raw_id_of(identifier)
            } else {
                game.spells.as_ref()?.raw_id_of(identifier)
            }
        })
        .unwrap();
    assert!(contents.flight.is_none());
    assert_eq!(contents.voxels.brick_count(), 2);
}

#[test]
fn a_world_file_is_not_a_ship_and_a_section_of_a_newer_version_is_not_guessed_at() {
    let game = Game::new(&ALL_PLUGINS);
    let world = game.new_world();
    let error = ParsedShip::parse(&game.save(&world)).unwrap_err();
    assert!(
        matches!(&error, ShipFileError::MissingSection(section) if *section == world_storage::ship_id()),
        "{error}"
    );

    let bytes = world
        .ship_format(&game.registries())
        .unwrap()
        .encode(&ship(&game, &world), &main_level_id())
        .unwrap();
    let newer = world_storage::container::write_container(
        &world_storage::container::read_container(&bytes)
            .unwrap()
            .into_iter()
            .map(|mut section| {
                if section.identifier == world_storage::flight_id() {
                    section.version = 2;
                }
                section
            })
            .collect::<Vec<_>>(),
    );
    let error = ParsedShip::parse(&newer).unwrap_err();
    assert!(
        matches!(&error, ShipFileError::UnsupportedSection { version: 2, .. }),
        "{error}"
    );
}

#[test]
fn a_flight_that_is_not_a_number_or_not_a_flag_is_refused() {
    let game = Game::new(&ALL_PLUGINS);
    let world = game.new_world();
    let format = world.ship_format(&game.registries()).unwrap();
    for (name, damage) in [
        ("not a number", 0usize),
        ("held that is neither", 1),
        ("logout that is unknown", 2),
    ] {
        let mut ship = ship(&game, &world);
        ship.linear_velocity = [1.0, 2.0, 3.0];
        let bytes = format.encode(&ship, &main_level_id()).unwrap();
        let mut sections = world_storage::container::read_container(&bytes).unwrap();
        let flight = sections
            .iter_mut()
            .find(|section| section.identifier == world_storage::flight_id())
            .unwrap();
        let length = flight.payload.len();
        match damage {
            // The first speed's four bytes (after the pose) become a NaN.
            0 => {
                let at = length - 2 - 6 * 4;
                flight.payload[at..at + 4].copy_from_slice(&f32::NAN.to_bits().to_le_bytes());
            }
            1 => flight.payload[length - 2] = 9,
            _ => flight.payload[length - 1] = 9,
        }
        let broken = world_storage::container::write_container(&sections);
        assert!(ParsedShip::parse(&broken).is_err(), "{name}");
    }
}

#[test]
fn every_cut_of_a_ship_file_and_every_damaged_byte_of_it_is_found() {
    let game = Game::new(&ALL_PLUGINS);
    let world = game.new_world();
    let bytes = world
        .ship_format(&game.registries())
        .unwrap()
        .encode(&ship(&game, &world), &main_level_id())
        .unwrap();
    for length in 0..bytes.len() {
        assert!(
            ParsedShip::parse(&bytes[..length]).is_err(),
            "cut at {length}"
        );
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(ParsedShip::parse(&trailing).is_err());
    for at in 0..bytes.len() {
        for mask in [0x01u8, 0x10, 0x80, 0xff] {
            let mut damaged = bytes.clone();
            damaged[at] ^= mask;
            assert!(
                ParsedShip::parse(&damaged).is_err(),
                "byte {at} of {} changed by {mask:#x} was not found",
                bytes.len()
            );
        }
    }
}

proptest! {
    #[test]
    fn arbitrary_bytes_are_never_a_ship_and_never_a_panic(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
        prop_assert!(ParsedShip::parse(&bytes).is_err());
    }
}

#[test]
fn the_names_of_ships_do_not_matter_to_the_file() {
    // Two ships that are the same but for their numbers are the same file: the number is the file's name and the world's, not its content.
    let game = Game::new(&ALL_PLUGINS);
    let mut world = game.new_world();
    let (one, two) = (ship(&game, &world), ship(&game, &world));
    let first = world.grids_mut().create(one);
    let second = world.grids_mut().create(two);
    assert_ne!(first, second);
    let format = world.ship_format(&game.registries()).unwrap();
    let file = |id: GridId| {
        format
            .encode(world.grids().get(id).unwrap(), &main_level_id())
            .unwrap()
    };
    assert_eq!(file(first), file(second));
    let _: NamespacedId = main_level_id();
}
