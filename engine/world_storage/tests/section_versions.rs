//! The versions of the sections that hold coordinates: what this program writes (2 for the plain sections, 3 for those that hold poses and for the generators,
//! with a coordinate of up to 128 bits) and that anything older is refused, never misread. Nothing was released and nothing older is kept readable. And that
//! the point of the change holds: a coordinate beyond 64 bits survives a save.

use binary_codec::ByteWriter;
use entities::{ComponentBag, Entity, EntityStore};
use protocol::{EntityId, write_entity_pose};
use voxel_space::{
    BRICK_COORDINATE_MAXIMUM, BRICK_COORDINATE_MINIMUM, BrickBox, BrickCoordinate, EntityPose,
    GridId, GridPose, LatticeRotation, LatticeTransform, Orientation, WorldPosition,
};
use world_storage::container::{read_container, write_container};
use world_storage::engine_sections::{
    GENERATORS_VERSION, GridRecord, GridsRecord, POSES_VERSION, SUPPORTED_VERSION, channels_id,
    clock_id, decode_entities_with_fallbacks, decode_grids, encode_entities, encode_grids,
    entities_id, generator_recipes_id, generators_id, grids_id, header_id, levels_id,
    override_bricks_id, reads_version, registries_id,
};
use world_storage::test_support::{ALL_PLUGINS, Game, id, main_level_id, sample_world};

fn a_position() -> WorldPosition {
    WorldPosition::new(BrickCoordinate::new(3, -2, 7), [1.5, 31.5, 0.25])
}

fn entity_pose_bytes(pose: &EntityPose) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    write_entity_pose(&mut writer, pose);
    writer.into_bytes()
}

#[test]
fn each_section_is_read_at_the_one_version_this_program_writes_and_at_no_other() {
    assert_eq!(
        (SUPPORTED_VERSION, POSES_VERSION, GENERATORS_VERSION),
        (2, 3, 3)
    );
    let table = [
        (entities_id(), POSES_VERSION),
        (grids_id(), POSES_VERSION),
        (generators_id(), GENERATORS_VERSION),
        (header_id(), SUPPORTED_VERSION),
        (registries_id(), SUPPORTED_VERSION),
        (channels_id(), SUPPORTED_VERSION),
        (levels_id(), SUPPORTED_VERSION),
        (override_bricks_id(), SUPPORTED_VERSION),
        (clock_id(), SUPPORTED_VERSION),
        (generator_recipes_id(), SUPPORTED_VERSION),
    ];
    for (section, current) in table {
        for version in 0..=5 {
            assert_eq!(
                reads_version(&section, version),
                version == current,
                "{section} at version {version}"
            );
        }
    }
}

#[test]
fn a_turn_that_is_not_a_number_of_f32_is_read_back_bit_for_bit() {
    let exact = EntityPose::new(
        GridId::MAIN,
        a_position(),
        Orientation::from_components([
            0.123_456_789_012_345_67,
            -0.5,
            0.25,
            (1.0f64 - 0.123_456_789_012_345_67f64 * 0.123_456_789_012_345_67f64 - 0.25 - 0.0625)
                .sqrt(),
        ])
        .unwrap(),
    );
    let store = EntityStore::restore(
        2,
        vec![Entity {
            id: EntityId::new(1),
            kind: id("test:pig"),
            pose: exact,
            components: ComponentBag::new(),
        }],
    )
    .unwrap();
    let (back, _) = decode_entities_with_fallbacks(&encode_entities(&store)).unwrap();
    assert_eq!(back.get(EntityId::new(1)).unwrap().pose, exact);
    assert_ne!(
        exact.orientation.components_single().map(f64::from),
        exact.orientation.components()
    );

    let record = GridsRecord {
        next: GridId::FIRST_ALLOCATED + 3,
        next_serial: 5,
        ships: vec![GridRecord {
            id: GridId::new(GridId::FIRST_ALLOCATED + 1),
            level: id("test:main"),
            pose: GridPose::new(
                a_position(),
                Orientation::from_components([0.1, 0.2, 0.3, (1.0f64 - 0.14).sqrt()]).unwrap(),
            ),
            bounds: None,
            serial: 4,
            checksum: 0xDEAD_BEEF,
            waits_for_those_aboard: true,
            has_terrain: true,
        }],
        tombstones: Vec::new(),
    };
    assert_eq!(decode_grids(&encode_grids(&record)).unwrap(), record);
}

#[test]
fn a_coordinate_at_the_ends_of_the_range_survives_the_section_of_entities_and_the_section_of_ships()
{
    let far = WorldPosition::new(
        BrickCoordinate::new(
            BRICK_COORDINATE_MINIMUM,
            BRICK_COORDINATE_MAXIMUM,
            (1 << 100) + 12_345,
        ),
        [1.5, 31.5, 0.25],
    );
    let pose = EntityPose::new(GridId::MAIN, far, Orientation::IDENTITY);
    let store = EntityStore::restore(
        2,
        vec![Entity {
            id: EntityId::new(1),
            kind: id("test:pig"),
            pose,
            components: ComponentBag::new(),
        }],
    )
    .unwrap();
    let (back, _) = decode_entities_with_fallbacks(&encode_entities(&store)).unwrap();
    assert_eq!(back.get(EntityId::new(1)).unwrap().pose, pose);

    let successor = GridId::new(GridId::FIRST_ALLOCATED + 2);
    let record = GridsRecord {
        next: GridId::FIRST_ALLOCATED + 3,
        next_serial: 5,
        ships: vec![GridRecord {
            id: GridId::new(GridId::FIRST_ALLOCATED + 1),
            level: id("test:main"),
            pose: GridPose::new(far, Orientation::IDENTITY),
            bounds: Some(
                BrickBox::new(
                    BrickCoordinate::new(BRICK_COORDINATE_MINIMUM, -(1 << 100), 0),
                    BrickCoordinate::new(BRICK_COORDINATE_MAXIMUM, 1 << 100, 1 << 90),
                )
                .unwrap(),
            ),
            serial: 4,
            checksum: 1,
            waits_for_those_aboard: false,
            has_terrain: false,
        }],
        tombstones: vec![(
            GridId::new(GridId::FIRST_ALLOCATED),
            vec![(
                successor,
                LatticeTransform {
                    rotation: LatticeRotation::all()[5],
                    offset: [1 << 100, -(1 << 110), 7],
                },
            )],
        )],
    };
    assert_eq!(decode_grids(&encode_grids(&record)).unwrap(), record);
}

#[test]
fn a_world_file_has_its_sections_at_the_current_versions_and_a_section_of_any_other_version_refuses_to_load()
 {
    let mut game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    let ship = game.sample_ship(&world, 0);
    world.grids_mut().create(ship);
    let bytes = game.save_with_ships(&mut world).world;
    let sections = read_container(&bytes).unwrap();
    for section in &sections {
        if section.identifier == grids_id() || section.identifier == entities_id() {
            assert_eq!(section.version, POSES_VERSION, "{}", section.identifier);
        }
        if section.identifier == header_id() || section.identifier == levels_id() {
            assert_eq!(section.version, SUPPORTED_VERSION, "{}", section.identifier);
        }
    }
    assert!(
        sections
            .iter()
            .any(|section| section.identifier == grids_id())
    );
    assert!(game.load(&bytes).is_ok());

    // The same file with one section as an earlier program numbered it, or as a later one might: refused each time, not guessed at.
    for identifier in [grids_id(), header_id(), levels_id()] {
        for version in [0, 1, 2, 3, 4] {
            let mut changed = sections.clone();
            let section = changed
                .iter_mut()
                .find(|section| section.identifier == identifier)
                .unwrap();
            if section.version == version {
                continue;
            }
            section.version = version;
            assert!(
                game.load(&write_container(&changed)).is_err(),
                "{identifier} at version {version}"
            );
        }
    }
}

#[test]
fn a_ship_file_has_its_flight_at_the_poses_version_and_a_flight_of_another_version_is_refused() {
    let game = Game::new(&ALL_PLUGINS);
    let world = sample_world(&game);
    // Far out, past what a 64-bit coordinate could hold: the pose and the bricks are exact in the file.
    let shift = 1i128 << 100;
    let ship = game.sample_ship(&world, shift);
    let format = game.ship_format(&world);
    let bytes = format.encode(&ship, &main_level_id()).unwrap();
    let contents = format.decode(&bytes).unwrap();
    assert_eq!(contents.flight.unwrap().pose, ship.pose);
    let stored = contents.voxels.sorted_brick_coordinates();
    assert_eq!(stored, ship.voxels.sorted_brick_coordinates());
    assert!(stored.iter().all(|at| at.x() > 1 << 90));

    let sections = read_container(&bytes).unwrap();
    let flight = sections
        .iter()
        .find(|section| section.identifier.to_string() == "engine:grid_flight")
        .unwrap();
    assert_eq!(flight.version, POSES_VERSION);
    for version in [0, 1, 2, 4] {
        let mut changed = sections.clone();
        changed
            .iter_mut()
            .find(|section| section.identifier.to_string() == "engine:grid_flight")
            .unwrap()
            .version = version;
        assert!(
            format.decode(&write_container(&changed)).is_err(),
            "flight at version {version}"
        );
    }
}

#[test]
fn the_bytes_of_a_pose_in_a_section_are_the_protocols() {
    // The section holds the pose as the wire does, so one layout is pinned in one place: a brick of (3, -2, 7) is three length-prefixed integers.
    let pose = EntityPose::new(GridId::MAIN, a_position(), Orientation::IDENTITY);
    let bytes = entity_pose_bytes(&pose);
    assert_eq!(&bytes[..7], [0, 1, 6, 1, 3, 1, 14]);
}
