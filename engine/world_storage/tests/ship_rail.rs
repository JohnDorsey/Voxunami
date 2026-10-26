//! A ship on a rail (a planet that goes round something and turns): its file has the rail, as a section of its own, and the rail survives being written,
//! put away, brought back and saved with a world. A ship with no rail has no such section, so its file is what it was before rails.

use world_storage::container::{Section, read_container, write_container};
use world_storage::rail::{Centre, Orbit, Rail, Spin, encode_rail};
use world_storage::test_support::{ALL_PLUGINS, Game, main_level_id, sample_world};
use world_storage::{MemoryShipFiles, MovingGrid, ParsedShip, ShipFileError, ShipFiles, rail_id};

fn a_rail() -> Rail {
    Rail {
        spin: Some(Spin::new([0.0, 1.0, 0.0], 86_400.0, 0.25).unwrap()),
        orbit: Some(
            Orbit::new(
                Centre::Point([1.0e6, 0.0, -2.0e6]),
                [0.0, 1.0, 0.0],
                1.5e9,
                31_557_600.0,
                1.0,
            )
            .unwrap(),
        ),
    }
}

fn has_rail_section(bytes: &[u8]) -> bool {
    read_container(bytes)
        .unwrap()
        .iter()
        .any(|section| section.identifier == rail_id())
}

#[test]
fn a_ship_with_no_rail_has_no_rail_section_and_one_with_a_rail_comes_back_with_it() {
    let game = Game::new(&ALL_PLUGINS);
    let world = game.new_world();
    let format = game.ship_format(&world);
    let planet = game.sample_planet(&world, 0);
    assert_eq!(planet.rail, None);
    let plain = format.encode(&planet, &main_level_id()).unwrap();
    assert!(!has_rail_section(&plain));
    assert_eq!(format.decode(&plain).unwrap().rail, None);

    let on_rails = MovingGrid {
        rail: Some(a_rail()),
        ..planet.clone()
    };
    let bytes = format.encode(&on_rails, &main_level_id()).unwrap();
    assert!(has_rail_section(&bytes));
    assert_eq!(format.encode(&on_rails, &main_level_id()).unwrap(), bytes);
    let read = format.decode(&bytes).unwrap();
    assert_eq!(read.rail, Some(a_rail()));
    // Written again from what was read, it is the same file.
    let again = MovingGrid {
        voxels: read.voxels,
        terrain: read.terrain,
        rail: read.rail,
        sections: read.sections,
        ..planet
    };
    assert_eq!(format.encode(&again, &main_level_id()).unwrap(), bytes);
}

/// The file of a planet on a rail with its rail section replaced by these bytes (at this version).
fn file_with_rail_section(game: &Game, version: u32, payload: Vec<u8>) -> Vec<u8> {
    let world = game.new_world();
    let planet = MovingGrid {
        rail: Some(a_rail()),
        ..game.sample_planet(&world, 0)
    };
    let bytes = game
        .ship_format(&world)
        .encode(&planet, &main_level_id())
        .unwrap();
    let mut sections = read_container(&bytes).unwrap();
    let section = sections
        .iter_mut()
        .find(|section| section.identifier == rail_id())
        .unwrap();
    *section = Section {
        identifier: rail_id(),
        version,
        payload,
    };
    write_container(&sections)
}

#[test]
fn a_damaged_rail_section_is_found_not_believed_and_one_of_a_newer_version_is_refused() {
    let game = Game::new(&ALL_PLUGINS);
    let good = encode_rail(&a_rail());
    assert!(ParsedShip::parse(&file_with_rail_section(&game, 1, good.clone())).is_ok());
    let mut longer = good.clone();
    longer.push(0);
    for (what, payload) in [
        ("cut short", good[..good.len() - 1].to_vec()),
        ("a byte too many", longer),
        ("nothing", Vec::new()),
        ("a flag nobody knows", vec![0b1000]),
    ] {
        let error = ParsedShip::parse(&file_with_rail_section(&game, 1, payload)).expect_err(what);
        assert!(
            matches!(&error, ShipFileError::Section { section, .. } if *section == rail_id()),
            "{what}: {error}"
        );
    }
    let error = ParsedShip::parse(&file_with_rail_section(&game, 2, good)).unwrap_err();
    assert!(
        matches!(error, ShipFileError::UnsupportedSection { version: 2, .. }),
        "{error}"
    );
}

#[test]
fn a_ship_on_a_rail_put_away_and_brought_back_still_has_the_rail() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    let planet = MovingGrid {
        rail: Some(a_rail()),
        ..game.sample_planet(&world, 0)
    };
    let id_of_planet = world.grids_mut().create(planet);
    let format = game.ship_format(&world);
    let mut files = MemoryShipFiles::new();
    world
        .park_ship(id_of_planet, false, &mut files, &format)
        .unwrap();
    world.load_ship(id_of_planet, &files, &format).unwrap();
    assert_eq!(
        world.grids().get(id_of_planet).unwrap().rail,
        Some(a_rail())
    );
    let _: &dyn ShipFiles = &files;
}

#[test]
fn a_world_with_a_planet_on_a_rail_saves_and_loads_it_with_the_rail() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    let planet = MovingGrid {
        rail: Some(a_rail()),
        ..game.sample_planet(&world, 0)
    };
    let planet_id = world.grids_mut().create(planet);
    let plain = game.sample_ship(&world, 100);
    let plain_id = world.grids_mut().create(plain);
    let save = game.save_with_ships(&mut world);
    let mut files = MemoryShipFiles::new();
    for (file, bytes) in &save.new_ship_files {
        files.write(file, bytes).unwrap();
    }
    let mut other = Game::new(&ALL_PLUGINS);
    let (mut loaded, _) = other.load(&save.world).unwrap();
    other.load_all_ships(&mut loaded, &files);
    assert_eq!(loaded.grids().get(planet_id).unwrap().rail, Some(a_rail()));
    assert_eq!(loaded.grids().get(plain_id).unwrap().rail, None);
}
