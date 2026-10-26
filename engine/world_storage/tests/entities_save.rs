//! The entities of a world in its file: that they come back as they were, that the counter keeps ids from
//! being reused, that a world without entities saves as it always did, and that what a game cannot interpret
//! is kept.

use std::sync::Arc;

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use entities::{ComponentBag, Entity, EntityKindSpec, EntityKinds, EntityStore};
use namespaced_id::NamespacedId;
use proptest::prelude::*;
use protocol::{EntityId, Message};
use voxel_grid::Grid;
use voxel_space::test_support::entity_pose;
use voxel_space::{
    BrickCoordinate, EntityPose, GridId, GridPose, LevelIndex, Orientation, WorldPosition,
};
use world_storage::container::{read_container, write_container};
use world_storage::engine_sections::{
    POSES_VERSION, decode_entities, decode_entities_with_fallbacks, encode_entities,
    encode_entities_with, entities_id,
};
use world_storage::test_support::{ALL_PLUGINS, Game, id, sample_world};
use world_storage::{LoadError, MovingGrid, PilotLogout, Section, WorldData};

/// A component that is one number.
#[derive(Debug, PartialEq)]
struct Health(u32);

impl Message for Health {
    fn kind() -> NamespacedId {
        id("test:health")
    }
    fn encode(&self) -> Vec<u8> {
        self.0.to_le_bytes().to_vec()
    }
    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let value = reader.read_u32()?;
        reader.finish()?;
        Ok(Self(value))
    }
    fn samples() -> Vec<Self> {
        vec![Self(0)]
    }
}

fn kinds() -> EntityKinds {
    let mut kinds = EntityKinds::new();
    kinds
        .register(EntityKindSpec::new("test:cow").component(Health(10), true))
        .unwrap();
    kinds.register(EntityKindSpec::new("test:pig")).unwrap();
    kinds
}

fn pose(level: u32, brick: [i64; 3], offset: [f32; 3]) -> EntityPose {
    EntityPose::new(
        GridId::root(LevelIndex::new(level)),
        WorldPosition::new(BrickCoordinate::new(brick[0], brick[1], brick[2]), offset),
        Orientation::from_yaw_pitch(0.5, -0.25),
    )
}

fn spawn(world: &mut WorldData, kind: &str, at: EntityPose) -> EntityId {
    world
        .entities_mut()
        .spawn(&kinds(), &id(kind), at, ComponentBag::new())
        .unwrap()
}

/// A world with a few entities of two kinds, one of them hurt.
fn world_with_entities(game: &Game) -> WorldData {
    let mut world = sample_world(game);
    let cow = spawn(
        &mut world,
        "test:cow",
        pose(0, [3, -2, 7], [1.5, 31.5, 0.0]),
    );
    spawn(&mut world, "test:pig", pose(0, [-40, 0, 0], [0.0; 3]));
    let gone = spawn(&mut world, "test:pig", pose(1, [0, 0, 0], [5.0; 3]));
    world.entities_mut().despawn(gone);
    world
        .entities_mut()
        .set_component(&kinds(), cow, &Health(3))
        .unwrap();
    world
}

fn entities_of(world: &WorldData) -> Vec<Entity> {
    world.entities().iter().cloned().collect()
}

fn sections_of(bytes: &[u8]) -> Vec<Section> {
    read_container(bytes).unwrap()
}

#[test]
fn entities_come_back_as_they_were_and_the_file_saves_to_the_same_bytes() {
    let game = Game::new(&ALL_PLUGINS);
    let original = world_with_entities(&game);
    let bytes = game.save(&original);
    let mut again = Game::new(&ALL_PLUGINS);
    let (loaded, _) = again.load(&bytes).unwrap();
    assert_eq!(entities_of(&loaded), entities_of(&original));
    assert_eq!(loaded.entities().next_id(), original.entities().next_id());
    assert_eq!(again.save(&loaded), bytes);
    assert_eq!(
        loaded
            .entities()
            .component::<Health>(EntityId::new(1))
            .unwrap(),
        Some(Health(3))
    );
}

#[test]
fn the_counter_is_saved_so_an_id_is_not_given_again_after_a_reload() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    let ids: Vec<EntityId> = (0..3)
        .map(|_| spawn(&mut world, "test:pig", pose(0, [0; 3], [1.0; 3])))
        .collect();
    for id in ids {
        world.entities_mut().despawn(id);
    }
    assert!(world.entities().is_empty());
    let bytes = game.save(&world);
    let mut again = Game::new(&ALL_PLUGINS);
    let (mut loaded, _) = again.load(&bytes).unwrap();
    assert!(loaded.entities().is_empty());
    assert_eq!(loaded.entities().next_id(), 4);
    assert_eq!(
        spawn(&mut loaded, "test:pig", pose(0, [0; 3], [1.0; 3])).get(),
        4
    );
}

#[test]
fn a_world_that_never_had_entities_saves_as_it_always_did_and_a_description_has_none() {
    let game = Game::new(&ALL_PLUGINS);
    let plain = sample_world(&game);
    assert!(
        !sections_of(&game.save(&plain))
            .iter()
            .any(|section| section.identifier == entities_id())
    );
    let with = world_with_entities(&game);
    assert!(
        sections_of(&game.save(&with))
            .iter()
            .any(|section| section.identifier == entities_id())
    );
    // What a server tells a joining client is the shape of the world: its entities are told another way.
    let description = with.describe(&game.registries()).unwrap();
    assert!(
        !sections_of(&description)
            .iter()
            .any(|section| section.identifier == entities_id())
    );
}

#[test]
fn entities_of_kinds_and_components_nobody_has_are_kept_byte_for_byte() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    // A ghost of a mod this game does not have, holding a component of that mod and one this game knows.
    let mut components = ComponentBag::new();
    components.set(&Health(8));
    components.set_raw(id("gone_mod:mood"), vec![9, 9, 9]);
    let ghost = Entity {
        id: EntityId::new(5),
        kind: id("gone_mod:ghost"),
        pose: pose(0, [1, 2, 3], [4.0; 3]),
        components,
    };
    *world.entities_mut() = EntityStore::restore(6, vec![ghost.clone()]).unwrap();
    let bytes = game.save(&world);
    let mut again = Game::new(&ALL_PLUGINS);
    let (loaded, _) = again.load(&bytes).unwrap();
    assert_eq!(entities_of(&loaded), [ghost]);
    assert_eq!(again.save(&loaded), bytes);
}

/// The world file with its entities section's payload and version replaced.
fn with_entities_section(bytes: &[u8], version: u32, payload: Vec<u8>) -> Vec<u8> {
    let mut sections = sections_of(bytes);
    let section = sections
        .iter_mut()
        .find(|section| section.identifier == entities_id())
        .expect("the file has entities");
    section.version = version;
    section.payload = payload;
    write_container(&sections)
}

fn payload_of(bytes: &[u8]) -> Vec<u8> {
    sections_of(bytes)
        .into_iter()
        .find(|section| section.identifier == entities_id())
        .expect("the file has entities")
        .payload
}

#[test]
fn a_damaged_entities_section_is_refused_and_never_panics() {
    let game = Game::new(&ALL_PLUGINS);
    let bytes = game.save(&world_with_entities(&game));
    let good = payload_of(&bytes);
    let load = |payload: Vec<u8>| {
        Game::new(&ALL_PLUGINS).load(&with_entities_section(&bytes, POSES_VERSION, payload))
    };
    assert!(load(good.clone()).is_ok());
    for length in 0..good.len() {
        assert!(load(good[..length].to_vec()).is_err(), "cut at {length}");
    }
    let mut trailing = good.clone();
    trailing.push(0);
    assert!(load(trailing).is_err());
    // A newer section version is refused, not read as if it were this one.
    assert!(matches!(
        Game::new(&ALL_PLUGINS).load(&with_entities_section(&bytes, POSES_VERSION + 1, good)),
        Err(LoadError::UnsupportedEngineSection { .. })
    ));
}

/// The entities section payload for these entities (by id and kind), with this counter.
fn payload_for(next_id: u64, ids: &[u64]) -> Vec<u8> {
    let entities: Vec<Entity> = ids
        .iter()
        .map(|&raw| Entity {
            id: EntityId::new(raw),
            kind: id("test:pig"),
            pose: pose(0, [0; 3], [1.0; 3]),
            components: ComponentBag::new(),
        })
        .collect();
    // `restore` refuses a bad list, so the bytes are written by hand.
    let mut writer = ByteWriter::new();
    writer.write_varint(next_id);
    writer.write_varint(entities.len() as u64);
    for entity in &entities {
        writer.write_varint(entity.id.get());
        writer.write_identifier(&entity.kind);
        protocol::write_entity_pose(&mut writer, &entity.pose);
        writer.write_u8(0);
        entity.components.write(&mut writer);
    }
    writer.into_bytes()
}

#[test]
fn ids_out_of_order_repeated_or_beyond_the_counter_are_refused() {
    assert!(decode_entities(&payload_for(10, &[1, 4, 9])).is_ok());
    for (counter, ids, why) in [
        (10, vec![4, 1], "out of order"),
        (10, vec![4, 4], "repeated"),
        (10, vec![1, 10], "at the counter"),
        (10, vec![1, 11], "beyond the counter"),
    ] {
        assert!(
            decode_entities(&payload_for(counter, &ids)).is_err(),
            "{why}"
        );
    }
}

/// The payload for one pig, in a level, with its fallback flag set and this place after it.
fn pig_in_a_level_with_a_fallback(place: EntityPose) -> Vec<u8> {
    let mut payload = payload_for(10, &[1]);
    // The flag is followed only by the count of components, which is 0.
    let at = payload.len() - 2;
    assert_eq!(payload[at], 0, "this is the flag");
    payload.truncate(at);
    let mut writer = ByteWriter::new();
    writer.write_u8(1);
    protocol::write_entity_pose(&mut writer, &place);
    writer.write_u8(0);
    payload.extend(writer.into_bytes());
    payload
}

#[test]
fn a_place_to_fall_back_to_on_an_entity_that_is_in_a_level_is_refused_rather_than_dropped() {
    let place = pose(0, [3, 0, 0], [1.0; 3]);
    assert!(decode_entities(&pig_in_a_level_with_a_fallback(place)).is_err());
    // Nor is any other flag.
    let mut payload = payload_for(10, &[1]);
    let at = payload.len() - 2;
    payload[at] = 2;
    assert!(decode_entities(&payload).is_err());
}

#[test]
fn a_place_to_fall_back_to_is_in_a_level_not_in_the_frame_of_another_ship() {
    let ship = GridId::new(GridId::FIRST_ALLOCATED);
    let aboard = EntityPose::new(
        ship,
        WorldPosition::new(BrickCoordinate::ORIGIN, [1.0; 3]),
        Orientation::IDENTITY,
    );
    let mut store = EntityStore::new();
    let pig = store
        .spawn(&kinds(), &id("test:pig"), aboard, ComponentBag::new())
        .unwrap();
    // The writer does not write a place that is not in a level.
    let elsewhere = EntityPose::new(
        GridId::new(GridId::FIRST_ALLOCATED + 1),
        WorldPosition::new(BrickCoordinate::ORIGIN, [2.0; 3]),
        Orientation::IDENTITY,
    );
    let (read, fallbacks) =
        decode_entities_with_fallbacks(&encode_entities_with(&store, |_| Some(elsewhere))).unwrap();
    assert_eq!(read.get(pig).unwrap().pose, aboard);
    assert!(fallbacks.is_empty());
    // And a reader does not take one: the same bytes with the place put in by hand are refused.
    let mut writer = ByteWriter::new();
    writer.write_varint(store.next_id());
    writer.write_varint(1);
    writer.write_varint(pig.get());
    writer.write_identifier(&id("test:pig"));
    protocol::write_entity_pose(&mut writer, &aboard);
    writer.write_u8(1);
    protocol::write_entity_pose(&mut writer, &elsewhere);
    writer.write_u8(0);
    assert!(decode_entities_with_fallbacks(&writer.into_bytes()).is_err());
}

proptest! {
    #[test]
    fn any_entities_survive_encoding(
        poses in proptest::collection::vec(entity_pose(), 0..20),
        despawn in proptest::collection::vec(0usize..20, 0..8),
    ) {
        let mut store = EntityStore::new();
        let mut live = Vec::new();
        for at in poses {
            live.push(
                store
                    .spawn(&kinds(), &id("test:cow"), at, ComponentBag::new())
                    .unwrap(),
            );
        }
        for pick in despawn {
            if !live.is_empty() {
                store.despawn(live.remove(pick % live.len()));
            }
        }
        let decoded = decode_entities(&encode_entities(&store)).unwrap();
        prop_assert_eq!(decoded.next_id(), store.next_id());
        prop_assert_eq!(
            decoded.iter().cloned().collect::<Vec<_>>(),
            store.iter().cloned().collect::<Vec<_>>()
        );
        prop_assert_eq!(encode_entities(&decoded), encode_entities(&store));
    }

    #[test]
    fn decoding_arbitrary_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..120)) {
        let _ = decode_entities(&bytes);
    }
}

/// A ship in the main level at `pose`, with no voxels.
fn make_ship(world: &mut WorldData, pose: GridPose) -> GridId {
    let layout = Arc::clone(world.main().overrides.layout());
    world.grids_mut().create(MovingGrid {
        level: LevelIndex::MAIN,
        pose,
        linear_velocity: [0.0; 3],
        angular_velocity: [0.0; 3],
        mass: 0.0,
        centre_of_mass: [0.0; 3],
        held: false,
        frozen: false,
        on_pilot_logout: PilotLogout::Stay,
        voxels: Grid::new(layout),
        terrain: None,
        rail: None,
        sections: Vec::new(),
    })
}

#[test]
fn someone_aboard_a_ship_is_saved_in_the_frame_of_the_ship_with_the_place_it_has_in_the_world() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = world_with_entities(&game);
    let quarter = std::f32::consts::FRAC_PI_2;
    let ship_pose = GridPose::new(
        WorldPosition::new(BrickCoordinate::new(2, 0, 0), [1.0, 2.0, 3.0]),
        Orientation::from_yaw_pitch(quarter, 0.0),
    );
    let ship = make_ship(&mut world, ship_pose);
    let aboard = EntityPose::new(
        ship,
        WorldPosition::new(BrickCoordinate::new(0, 0, 0), [10.0, 0.0, 0.0]),
        Orientation::from_yaw_pitch(0.0, quarter),
    );
    let passenger = spawn(&mut world, "test:cow", aboard);
    let before = entities_of(&world);

    let save = game.save_with_ships(&mut world);

    // Saving does not move them in the running world.
    assert_eq!(entities_of(&world), before);
    // The file has them in the frame of the ship, and where that is in the world.
    let (in_the_file, fallbacks) =
        decode_entities_with_fallbacks(&payload_of(&save.world)).unwrap();
    assert_eq!(in_the_file.get(passenger).unwrap().pose, aboard);
    let (position, orientation) = ship_pose
        .place_in_world(aboard.position, aboard.orientation)
        .unwrap();
    assert_eq!(
        fallbacks.into_iter().collect::<Vec<_>>(),
        [(
            passenger,
            EntityPose::new(GridId::MAIN, position, orientation)
        )],
        "only the one who is aboard has a place to fall back to"
    );

    // Loaded, they are the same entity in the same frame, and the ship is in its file, where the world file says it is.
    let mut again = Game::new(&ALL_PLUGINS);
    let (loaded, _) = again.load(&save.world).unwrap();
    assert_eq!(entities_of(&loaded), before);
    assert_eq!(
        loaded.entities().component::<Health>(passenger).unwrap(),
        Some(Health(10))
    );
    assert!(loaded.grids().get(ship).is_none());
    assert_eq!(
        loaded.grids().frame_of(ship),
        Some((LevelIndex::MAIN, ship_pose))
    );
    // Saved again with the ship still in its file, the world file is the same bytes: it names the file, and does not rewrite it.
    assert_eq!(loaded.save(&again.registries()).unwrap(), save.world);
}

#[test]
fn an_entity_in_the_frame_of_a_ship_the_file_does_not_list_is_put_where_it_was_in_the_world() {
    let game = Game::new(&ALL_PLUGINS);
    let (lost, forgotten) = (
        GridId::new(GridId::FIRST_ALLOCATED),
        GridId::new(GridId::FIRST_ALLOCATED + 1),
    );
    let at = |grid, x| {
        EntityPose::new(
            grid,
            WorldPosition::new(BrickCoordinate::ORIGIN, [x, 2.0, 3.0]),
            Orientation::IDENTITY,
        )
    };
    let mut store = EntityStore::new();
    let with_place = store
        .spawn(
            &kinds(),
            &id("test:pig"),
            at(lost, 1.0),
            ComponentBag::new(),
        )
        .unwrap();
    let without_place = store
        .spawn(
            &kinds(),
            &id("test:pig"),
            at(forgotten, 5.0),
            ComponentBag::new(),
        )
        .unwrap();
    let place = EntityPose::new(
        GridId::root(LevelIndex::MAIN),
        WorldPosition::new(BrickCoordinate::new(7, 0, 0), [9.0, 8.0, 7.0]),
        Orientation::from_yaw_pitch(0.3, 0.0),
    );
    // A file that has these in the frames of two ships it does not list: one saved with the place it was in the world, the other not.
    let payload = encode_entities_with(&store, |entity| (entity.id == with_place).then_some(place));
    let bytes = game.save(&world_with_entities(&game));
    let (loaded, _) = Game::new(&ALL_PLUGINS)
        .load(&with_entities_section(&bytes, POSES_VERSION, payload))
        .unwrap();
    assert_eq!(loaded.entities().get(with_place).unwrap().pose, place);
    // Nothing better is known of the other than the coordinates it had: in the main level.
    assert_eq!(
        loaded.entities().get(without_place).unwrap().pose,
        at(GridId::MAIN, 5.0)
    );
}
