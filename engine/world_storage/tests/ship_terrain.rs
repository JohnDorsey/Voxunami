//! A ship with terrain, a planet: its file names the generator version that makes the bricks nobody stored, says which bricks it makes and what it weighs; and that
//! survives being written, read into another world, put away, brought back and saved with a world.

use std::sync::Arc;

use binary_codec::ByteWriter;
use content_registry::IdentifierRegistry;
use world_storage::container::{Section, read_container, write_container};
use world_storage::engine_sections::{decode_grids, grids_id};
use world_storage::test_support::{ALL_PLUGINS, Game, Plugin, id, main_level_id, sample_world};
use world_storage::{
    MemoryShipFiles, MovingGrid, ParsedShip, ShipFileError, ShipFiles, WorldData, terrain_id,
};

const WITHOUT_FANCY_TERRAIN: [Plugin; 4] = [
    Plugin::Newcomer,
    Plugin::BaseBlocks,
    Plugin::FancyBlocks,
    Plugin::Magic,
];

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |state, byte| {
        (state ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn terrain_registry() -> namespaced_id::NamespacedId {
    id("engine:terrain_generator")
}

#[test]
fn a_planet_comes_back_from_its_file_with_its_terrain_and_its_edits_and_the_file_is_the_same_every_time()
 {
    let game = Game::new(&ALL_PLUGINS);
    let world = game.new_world();
    let planet = game.sample_planet(&world, 0);
    let format = game.ship_format(&world);
    let bytes = format.encode(&planet, &main_level_id()).unwrap();
    assert_eq!(format.encode(&planet, &main_level_id()).unwrap(), bytes);

    let read = format.decode(&bytes).unwrap();
    assert_eq!(read.terrain, planet.terrain);
    assert_eq!(read.voxels.content_digest(), planet.voxels.content_digest());
    // Written again from what was read, it is the same file.
    let again = MovingGrid {
        voxels: read.voxels,
        terrain: read.terrain,
        sections: read.sections,
        ..planet.clone()
    };
    assert_eq!(format.encode(&again, &main_level_id()).unwrap(), bytes);
    // The bytes are a function of the planet and the game's content alone, on every machine.
    assert_eq!(fnv(&bytes), PINNED_PLANET_FILE, "{} bytes", bytes.len());
}

/// The hash of the file of the planet above (17,126 bytes: the ship of `ship_files.rs` with the terrain section, 60 bytes, and its stored bricks as edits).
const PINNED_PLANET_FILE: u64 = 9_544_620_368_787_940_474;

#[test]
fn a_ship_that_nothing_generates_has_no_terrain_section() {
    let game = Game::new(&ALL_PLUGINS);
    let world = game.new_world();
    let format = game.ship_format(&world);
    let ship = game.sample_ship(&world, 0);
    let planet = game.sample_planet(&world, 0);
    let has_terrain_section = |ship: &MovingGrid| {
        let bytes = format.encode(ship, &main_level_id()).unwrap();
        read_container(&bytes)
            .unwrap()
            .iter()
            .any(|section| section.identifier == terrain_id())
    };
    assert!(!has_terrain_section(&ship));
    assert!(has_terrain_section(&planet));
    assert_eq!(
        format
            .decode(&format.encode(&ship, &main_level_id()).unwrap())
            .unwrap()
            .terrain,
        None
    );
}

#[test]
fn a_planet_names_its_generator_and_a_world_that_numbers_generators_otherwise_reads_it_as_its_own()
{
    let game = Game::new(&ALL_PLUGINS);
    let world = game.new_world();
    let planet = game.sample_planet(&world, 0);
    let bytes = game
        .ship_format(&world)
        .encode(&planet, &main_level_id())
        .unwrap();
    let parsed = ParsedShip::parse(&bytes).unwrap();
    assert!(
        parsed
            .referenced_content()
            .contains(&(terrain_registry(), id("fancy_terrain:cliffs_v1")))
    );

    // A reader that gives the generators other numbers (every number a hundred more) means the same generator by them.
    let layout = Arc::clone(world.main().overrides.layout());
    let contents = parsed
        .into_contents(&layout, |registry, identifier| {
            if *registry == terrain_registry() {
                Some(100 + game.generator_registry().runtime_id(identifier)?.raw())
            } else if *registry == id("voxel_content:block") {
                game.blocks.raw_id_of(identifier)
            } else {
                game.spells.as_ref()?.raw_id_of(identifier)
            }
        })
        .unwrap();
    let terrain = contents.terrain.unwrap();
    let written = planet.terrain.unwrap();
    assert_eq!(terrain.generator.raw(), 100 + written.generator.raw());
    assert_eq!(terrain.extent, written.extent);
    assert_eq!(terrain.mass, written.mass);
}

#[test]
fn a_world_without_the_generator_refuses_the_planet_and_says_which() {
    let writer = Game::new(&ALL_PLUGINS);
    let world = writer.new_world();
    let planet = writer.sample_planet(&world, 0);
    let bytes = writer
        .ship_format(&world)
        .encode(&planet, &main_level_id())
        .unwrap();

    let reader = Game::new(&WITHOUT_FANCY_TERRAIN);
    let reading_world = reader.new_world();
    let error = reader
        .ship_format(&reading_world)
        .decode(&bytes)
        .unwrap_err();
    match error {
        ShipFileError::UnknownContent {
            registry,
            identifier,
        } => {
            assert_eq!(registry, terrain_registry());
            assert_eq!(identifier, id("fancy_terrain:cliffs_v1"));
        }
        other => panic!("{other}"),
    }
}

#[test]
fn a_generator_the_worlds_table_lacks_cannot_be_written() {
    let game = Game::new(&ALL_PLUGINS);
    let world = game.new_world();
    let mut planet = game.sample_planet(&world, 0);
    let mut terrain = planet.terrain.unwrap();
    terrain.generator = terrain::GeneratorId::from_raw(99);
    planet.terrain = Some(terrain);
    let error = game
        .ship_format(&world)
        .encode(&planet, &main_level_id())
        .unwrap_err();
    assert!(
        matches!(error, ShipFileError::UnknownGenerator(99)),
        "{error}"
    );
}

/// The file of the planet with its terrain section replaced by these bytes (at this version).
fn file_with_terrain_section(
    game: &Game,
    world: &WorldData,
    version: u32,
    payload: Vec<u8>,
) -> Vec<u8> {
    let planet = game.sample_planet(world, 0);
    let bytes = game
        .ship_format(world)
        .encode(&planet, &main_level_id())
        .unwrap();
    let mut sections = read_container(&bytes).unwrap();
    let section = sections
        .iter_mut()
        .find(|section| section.identifier == terrain_id())
        .unwrap();
    *section = Section {
        identifier: terrain_id(),
        version,
        payload,
    };
    write_container(&sections)
}

fn terrain_payload(extent: [[i64; 3]; 2], mass: f32, trailing: bool) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    writer.write_identifier(&id("fancy_terrain:cliffs_v1"));
    for corner in extent {
        for component in corner {
            writer.write_signed_varint(component);
        }
    }
    writer.write_u32(mass.to_bits());
    if trailing {
        writer.write_u8(0);
    }
    writer.into_bytes()
}

#[test]
fn a_damaged_terrain_section_is_found_not_believed() {
    let game = Game::new(&ALL_PLUGINS);
    let world = game.new_world();
    let read = |version, payload| {
        let bytes = file_with_terrain_section(&game, &world, version, payload);
        ParsedShip::parse(&bytes).map(|parsed| parsed.world_seed())
    };
    // The right bytes are read ...
    let good = terrain_payload([[-20; 3], [20; 3]], 6.07e20, false);
    assert!(read(1, good.clone()).is_ok());
    // ... the corners the wrong way round, a weight that is not one, a byte too many, a number too few, and a version from the future are not.
    for (what, version, payload) in [
        (
            "corners the wrong way round",
            1,
            terrain_payload([[20; 3], [-20; 3]], 1.0, false),
        ),
        (
            "a mass that is not a number",
            1,
            terrain_payload([[-20; 3], [20; 3]], f32::NAN, false),
        ),
        (
            "a negative mass",
            1,
            terrain_payload([[-20; 3], [20; 3]], -1.0, false),
        ),
        (
            "an infinite mass",
            1,
            terrain_payload([[-20; 3], [20; 3]], f32::INFINITY, false),
        ),
        (
            "a byte too many",
            1,
            terrain_payload([[-20; 3], [20; 3]], 1.0, true),
        ),
        ("a section cut short", 1, good[..good.len() - 2].to_vec()),
        (
            "a brick out of range",
            1,
            terrain_payload([[-20; 3], [i64::MAX, 0, 0]], 1.0, false),
        ),
    ] {
        let error = read(version, payload).expect_err(what);
        assert!(
            matches!(error, ShipFileError::Section { .. }),
            "{what}: {error}"
        );
    }
    let error = read(7, good).unwrap_err();
    assert!(
        matches!(error, ShipFileError::UnsupportedSection { version: 7, .. }),
        "{error}"
    );
}

#[test]
fn a_planet_put_away_and_brought_back_keeps_its_terrain_its_edits_and_its_weight() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    let planet = game.sample_planet(&world, 0);
    let bounds = planet.world_brick_bounds().unwrap();
    // The box about the origin that it makes, which is much more than the bricks of the edits.
    let ship = game.sample_ship(&world, 0).world_brick_bounds().unwrap();
    assert!(bounds.max().x() - bounds.min().x() > 3 * (ship.max().x() - ship.min().x()));
    let id_of_planet = world.grids_mut().create(planet.clone());
    let format = game.ship_format(&world);
    let mut files = MemoryShipFiles::new();
    world
        .park_ship(id_of_planet, false, &mut files, &format)
        .unwrap();
    assert_eq!(
        world.grids().get_parked(id_of_planet).unwrap().bounds,
        Some(bounds)
    );
    world.load_ship(id_of_planet, &files, &format).unwrap();
    let back = world.grids().get(id_of_planet).unwrap();
    assert_eq!(back.terrain, planet.terrain);
    assert_eq!(back.mass, planet.terrain.unwrap().mass);
    assert_eq!(back.voxels.content_digest(), planet.voxels.content_digest());
    let _: &dyn ShipFiles = &files;
}

#[test]
fn a_world_with_a_planet_saves_and_loads_it_in_another_process() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    let planet = game.sample_planet(&world, 0);
    let planet_id = world.grids_mut().create(planet.clone());
    let other_ship = game.sample_ship(&world, 100);
    let ship_id = world.grids_mut().create(other_ship);
    let save = game.save_with_ships(&mut world);
    let mut files = MemoryShipFiles::new();
    for (file, bytes) in &save.new_ship_files {
        files.write(file, bytes).unwrap();
    }
    let mut other = Game::new(&ALL_PLUGINS);
    let (mut loaded, report) = other.load(&save.world).unwrap();
    assert!(report.missing_content.is_empty());
    other.load_all_ships(&mut loaded, &files);
    assert_eq!(
        loaded.grids().get(planet_id).unwrap().terrain,
        planet.terrain
    );
    assert_eq!(loaded.grids().get(ship_id).unwrap().terrain, None);
}

#[test]
fn a_generator_whose_plugin_is_missing_is_kept_by_the_planet_that_names_it() {
    // The world file lists the generators of the game that saved it; a game without the plugin loads the name as a placeholder, and a planet that is
    // made by it is still a planet when the plugin comes back.
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    let planet = game.sample_planet(&world, 0);
    let planet_id = world.grids_mut().create(planet.clone());
    let save = game.save_with_ships(&mut world);
    let mut files = MemoryShipFiles::new();
    for (file, bytes) in &save.new_ship_files {
        files.write(file, bytes).unwrap();
    }

    let mut without = Game::new(&WITHOUT_FANCY_TERRAIN);
    let (mut middle, report) = without.load(&save.world).unwrap();
    assert!(
        report
            .missing_content
            .contains(&(terrain_registry(), id("fancy_terrain:cliffs_v1"))),
        "{:?}",
        report.missing_content
    );
    without.load_all_ships(&mut middle, &files);
    let kept = middle.grids().get(planet_id).unwrap();
    assert_eq!(kept.terrain, planet.terrain);
    assert!(
        middle
            .main()
            .terrain
            .generators()
            .entry(kept.terrain.unwrap().generator)
            .unwrap()
            .is_missing()
    );

    // Saved again by the game that lacks the plugin, and read by the game that has it, the planet is as it was.
    let save_again = without.save_with_ships(&mut middle);
    let mut files_again = MemoryShipFiles::new();
    for (file, bytes) in &save_again.new_ship_files {
        files_again.write(file, bytes).unwrap();
    }
    let mut restored = Game::new(&ALL_PLUGINS);
    let (mut last, _) = restored.load(&save_again.world).unwrap();
    restored.load_all_ships(&mut last, &files_again);
    assert_eq!(last.grids().get(planet_id).unwrap().terrain, planet.terrain);
}

#[test]
fn the_world_file_says_which_of_its_ships_are_planets_whether_they_were_in_memory_or_in_files() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    let (made_planet, made_ship) = (game.sample_planet(&world, 0), game.sample_ship(&world, 100));
    let planet = world.grids_mut().create(made_planet);
    let ship = world.grids_mut().create(made_ship);
    let listed = |world_file: &[u8]| {
        let section = read_container(world_file)
            .unwrap()
            .into_iter()
            .find(|section| section.identifier == grids_id())
            .expect("the file has ships");
        decode_grids(&section.payload)
            .unwrap()
            .ships
            .into_iter()
            .map(|record| (record.id, record.has_terrain))
            .collect::<Vec<_>>()
    };

    // Saved from memory.
    let save = game.save_with_ships(&mut world);
    assert_eq!(listed(&save.world), [(planet, true), (ship, false)]);
    let mut files = MemoryShipFiles::new();
    for (file, bytes) in &save.new_ship_files {
        files.write(file, bytes).unwrap();
    }

    // Read back, both are in files, and the world knows which is a planet without reading them.
    let mut other = Game::new(&ALL_PLUGINS);
    let (mut loaded, _) = other.load(&save.world).unwrap();
    assert!(loaded.grids().get_parked(planet).unwrap().has_terrain);
    assert!(!loaded.grids().get_parked(ship).unwrap().has_terrain);

    // And put away from memory, one by one, the same.
    other.load_all_ships(&mut loaded, &files);
    let format = other.ship_format(&loaded);
    loaded
        .park_ship(planet, false, &mut files, &format)
        .unwrap();
    loaded.park_ship(ship, false, &mut files, &format).unwrap();
    assert!(loaded.grids().get_parked(planet).unwrap().has_terrain);
    assert!(!loaded.grids().get_parked(ship).unwrap().has_terrain);
    let again = other.save_with_ships(&mut loaded);
    assert_eq!(listed(&again.world), [(planet, true), (ship, false)]);
}
