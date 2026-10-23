//! Files written when an orientation was four `f32` (version 1 of the sections that hold poses) are still read, as the same turn, and what is
//! written now is version 2 with four `f64`.

use binary_codec::ByteWriter;
use entities::{ComponentBag, Entity, EntityStore};
use protocol::{EntityId, PoseFormat, write_entity_pose, write_grid_pose};
use voxel_space::{
    BrickCoordinate, EntityPose, GridId, GridPose, LevelIndex, Orientation, WorldPosition,
};
use world_storage::container::{read_container, write_container};
use world_storage::engine_sections::{
    GridRecord, GridsRecord, POSES_VERSION, SUPPORTED_VERSION, decode_entities_with_fallbacks_in,
    decode_grids_in, encode_entities, encode_grids, entities_id, grids_id, pose_format_of,
    reads_version,
};
use world_storage::test_support::{ALL_PLUGINS, Game, id, sample_world};

/// A turn that an `f32` holds exactly (so the older file and this test agree on it) and that is not a multiple of a right angle.
fn a_turn() -> Orientation {
    let turn = Orientation::from_yaw_pitch(0.7, -0.3).components_single();
    Orientation::from_single_components(turn).unwrap()
}

fn a_position() -> WorldPosition {
    WorldPosition::new(BrickCoordinate::new(3, -2, 7), [1.5, 31.5, 0.25])
}

/// The bytes of a pose as version 1 wrote them: the version 2 bytes with the last four `f64` replaced by the four `f32`.
fn single(bytes: Vec<u8>, orientation: Orientation) -> Vec<u8> {
    let mut older = bytes[..bytes.len() - 32].to_vec();
    for component in orientation.components_single() {
        older.extend(component.to_bits().to_le_bytes());
    }
    older
}

fn grid_pose_bytes(pose: &GridPose) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    write_grid_pose(&mut writer, pose);
    writer.into_bytes()
}

fn entity_pose_bytes(pose: &EntityPose) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    write_entity_pose(&mut writer, pose);
    writer.into_bytes()
}

#[test]
fn the_sections_that_hold_poses_are_version_two_and_version_one_is_still_read() {
    assert_eq!(POSES_VERSION, 2);
    assert_eq!(pose_format_of(1), Some(PoseFormat::Single));
    assert_eq!(pose_format_of(2), Some(PoseFormat::Double));
    assert_eq!(pose_format_of(0), None);
    assert_eq!(pose_format_of(3), None);
    for section in [entities_id(), grids_id()] {
        assert!(reads_version(&section, 1) && reads_version(&section, 2));
        assert!(!reads_version(&section, 3) && !reads_version(&section, 0));
    }
    // Sections that hold no pose are still version 1 only.
    assert!(reads_version(
        &world_storage::engine_sections::header_id(),
        SUPPORTED_VERSION
    ));
    assert!(!reads_version(
        &world_storage::engine_sections::header_id(),
        2
    ));
}

#[test]
fn the_entities_of_an_older_file_are_the_same_turn_and_what_is_written_now_is_read_back_exactly() {
    let pose = EntityPose::new(GridId::MAIN, a_position(), a_turn());
    let payload = |pose_bytes: Vec<u8>| {
        let mut writer = ByteWriter::new();
        writer.write_varint(2); // the counter
        writer.write_varint(1); // one entity
        writer.write_varint(1);
        writer.write_identifier(&id("test:pig"));
        let mut bytes = writer.into_bytes();
        bytes.extend(pose_bytes);
        bytes.push(0); // no fallback place
        bytes.extend({
            let mut components = ByteWriter::new();
            ComponentBag::new().write(&mut components);
            components.into_bytes()
        });
        bytes
    };
    let older = payload(single(entity_pose_bytes(&pose), pose.orientation));
    let (store, _) = decode_entities_with_fallbacks_in(&older, PoseFormat::Single).unwrap();
    assert_eq!(store.get(EntityId::new(1)).unwrap().pose, pose);
    // Read as the new layout, the older bytes are damaged and not a different turn.
    assert!(decode_entities_with_fallbacks_in(&older, PoseFormat::Double).is_err());

    // What is written now has a turn that is not a number of `f32` and is read back bit for bit.
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
    let (back, _) =
        decode_entities_with_fallbacks_in(&encode_entities(&store), PoseFormat::Double).unwrap();
    assert_eq!(back.get(EntityId::new(1)).unwrap().pose, exact);
    assert_ne!(
        exact.orientation.components_single().map(f64::from),
        exact.orientation.components()
    );
}

#[test]
fn the_ships_of_an_older_file_are_the_same_turn_and_what_is_written_now_is_read_back_exactly() {
    let record = |orientation| GridsRecord {
        next: GridId::FIRST_ALLOCATED + 3,
        next_serial: 5,
        ships: vec![GridRecord {
            id: GridId::new(GridId::FIRST_ALLOCATED + 1),
            level: id("test:main"),
            pose: GridPose::new(a_position(), orientation),
            bounds: None,
            serial: 4,
            checksum: 0xDEAD_BEEF,
            waits_for_those_aboard: true,
        }],
        tombstones: Vec::new(),
    };
    let wanted = record(a_turn());
    let now = encode_grids(&wanted);
    // The same record in version 1: the pose bytes are the only ones that differ, and sit after the ship's id and level.
    let pose_now = grid_pose_bytes(&wanted.ships[0].pose);
    let at = now
        .windows(pose_now.len())
        .position(|window| window == pose_now)
        .expect("the pose is in the section");
    let mut older = now[..at].to_vec();
    older.extend(single(pose_now.clone(), a_turn()));
    older.extend(&now[at + pose_now.len()..]);
    assert_eq!(decode_grids_in(&older, PoseFormat::Single).unwrap(), wanted);
    assert!(decode_grids_in(&older, PoseFormat::Double).is_err());
    assert_eq!(decode_grids_in(&now, PoseFormat::Double).unwrap(), wanted);

    let exact =
        record(Orientation::from_components([0.1, 0.2, 0.3, (1.0f64 - 0.14).sqrt()]).unwrap());
    assert_eq!(
        decode_grids_in(&encode_grids(&exact), PoseFormat::Double).unwrap(),
        exact
    );
}

#[test]
fn a_world_file_of_this_program_has_its_poses_sections_at_version_two_and_a_file_with_them_at_version_one_loads()
 {
    let mut game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    let ship = game.sample_ship(&world, 0);
    let first = world.grids_mut().create(ship);
    let bytes = game.save_with_ships(&mut world).world;
    let mut sections = read_container(&bytes).unwrap();
    let versions: Vec<(String, u32)> = sections
        .iter()
        .filter(|section| section.identifier == grids_id() || section.identifier == entities_id())
        .map(|section| (section.identifier.to_string(), section.version))
        .collect();
    assert!(
        versions.iter().any(|(name, _)| name == "engine:grids")
            && versions
                .iter()
                .all(|(_, version)| *version == POSES_VERSION),
        "{versions:?}"
    );

    // Rewritten as an older program would have: the ships section in version 1, with the turn it had.
    let section = sections
        .iter_mut()
        .find(|section| section.identifier == grids_id())
        .unwrap();
    let record = decode_grids_in(&section.payload, PoseFormat::Double).unwrap();
    let pose = record.ships[0].pose;
    let pose_now = grid_pose_bytes(&pose);
    let at = section
        .payload
        .windows(pose_now.len())
        .position(|window| window == pose_now)
        .unwrap();
    let mut older = section.payload[..at].to_vec();
    older.extend(single(pose_now.clone(), pose.orientation));
    older.extend(&section.payload[at + pose_now.len()..]);
    section.payload = older;
    section.version = 1;
    let (loaded, _) = game.load(&write_container(&sections)).unwrap();
    // (The ship is in its file, which the world file names; where it is comes from the world file.)
    let (_, found) = loaded.grids().frame_of(first).unwrap();
    assert_eq!(found.position, pose.position);
    assert_eq!(
        found.orientation.components_single(),
        pose.orientation.components_single()
    );

    // A version this program does not know is refused rather than guessed at.
    let section = sections
        .iter_mut()
        .find(|section| section.identifier == grids_id())
        .unwrap();
    section.version = 3;
    assert!(game.load(&write_container(&sections)).is_err());
    let _ = (LevelIndex::MAIN, SUPPORTED_VERSION);
}

#[test]
fn the_flight_of_a_ship_file_written_with_f32_orientations_is_read_as_the_same_turn() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    let mut ship = game.sample_ship(&world, 0);
    ship.pose = GridPose::new(a_position(), a_turn());
    let first = world.grids_mut().create(ship);
    let format = game.ship_format(&world);
    let level = world_storage::test_support::main_level_id();
    let bytes = format
        .encode(world.grids().get(first).unwrap(), &level)
        .unwrap();
    let mut sections = read_container(&bytes).unwrap();
    let flight = sections
        .iter_mut()
        .find(|section| section.identifier.to_string() == "engine:grid_flight")
        .unwrap();
    assert_eq!(flight.version, POSES_VERSION);
    assert_eq!(
        format
            .decode(&bytes)
            .unwrap()
            .flight
            .unwrap()
            .pose
            .orientation,
        a_turn()
    );

    // Written by an older program: the flight in version 1, the other sections as they were.
    let pose = GridPose::new(a_position(), a_turn());
    let length = grid_pose_bytes(&pose).len();
    let mut older = single(flight.payload[..length].to_vec(), a_turn());
    older.extend(&flight.payload[length..]);
    flight.payload = older;
    flight.version = 1;
    let contents = format.decode(&write_container(&sections)).unwrap();
    assert_eq!(contents.flight.unwrap().pose, pose);
}
