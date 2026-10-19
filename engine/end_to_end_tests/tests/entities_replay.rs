//! Entities in the stream: what a client is told of them, and that a recording of the stream rebuilds
//! exactly what the client saw, tick by tick.

use std::io::Cursor;

use client::{ClientError, ClientEvent};
use end_to_end_tests::{Fixture, Glow, Player, SharedBuffer, replay_player, wisp_kind};
use protocol::{EntityId, ServerMessage};
use replay::{RecordingConnection, ReplayEnd, ReplayReader, ReplayWriter};
use transport::loopback::LoopbackConnection;
use voxel_space::{BrickBox, BrickCoordinate, EntityPose, GridId};
use world_storage::test_support::{ALL_PLUGINS, brick};

type RecordedPlayer = Player<RecordingConnection<LoopbackConnection, SharedBuffer>>;

struct Session {
    fixture: Fixture,
    alice: RecordedPlayer,
    recording: SharedBuffer,
    bob: Player<LoopbackConnection>,
}

/// Two players in a world with a herd of wisps; alice is recorded and moves her region about so that
/// wisps come into it and leave it. `ticks` are played after both joined.
fn play_session(ticks: i64) -> Session {
    let mut fixture = Fixture::with_herd();
    let (mut alice, recording) = fixture.join_recorded(&ALL_PLUGINS);
    let mut bob = fixture.join(&ALL_PLUGINS);
    alice.watch(BrickBox::around(brick(0, 0, 0), 1));
    bob.watch(BrickBox::around(brick(2, 0, 0), 2));
    for step in 0..ticks {
        if step == 25 {
            alice.watch(BrickBox::around(brick(3, 0, 0), 1));
        }
        if step == 50 {
            alice.watch(BrickBox::around(brick(-1, 0, 0), 2));
        }
        fixture.step();
        alice.catch_up().unwrap();
        bob.catch_up().unwrap();
    }
    Session {
        fixture,
        alice,
        recording,
        bob,
    }
}

type Told = Vec<(EntityId, u32, EntityPose)>;

/// What the server says the entities of a region are: the id, the glow and where each is.
fn expected_entities(session: &Session, region: BrickBox) -> Told {
    let store = session.fixture.server.world().entities();
    store
        .in_bricks(GridId::MAIN, region)
        .into_iter()
        .map(|id| {
            let entity = store.get(id).unwrap();
            (
                id,
                entity.components.get::<Glow>().unwrap().unwrap().0,
                entity.pose,
            )
        })
        .collect()
}

fn known_entities<C: transport::Connection>(player: &Player<C>) -> Told {
    player
        .client
        .world()
        .entities()
        .iter()
        .map(|entity| {
            assert_eq!(entity.kind, wisp_kind());
            (
                entity.id,
                entity.component::<Glow>().unwrap().unwrap().0,
                entity.pose,
            )
        })
        .collect()
}

#[test]
fn a_player_knows_exactly_the_entities_of_the_region_it_watches() {
    let session = play_session(80);
    let alice_region = BrickBox::around(brick(-1, 0, 0), 2);
    let bob_region = BrickBox::around(brick(2, 0, 0), 2);
    assert_eq!(
        known_entities(&session.alice),
        expected_entities(&session, alice_region)
    );
    assert_eq!(
        known_entities(&session.bob),
        expected_entities(&session, bob_region)
    );
}

#[test]
fn the_scenario_exercises_every_kind_of_entity_message() {
    // Guards the tests here against passing because nothing happened.
    let session = play_session(80);
    let mut reader = ReplayReader::new(Cursor::new(session.recording.contents())).unwrap();
    let (mut spawned, mut changed, mut despawned) = (0, 0, 0);
    while let Some(frame) = reader.next_frame().unwrap() {
        match ServerMessage::decode(&frame).unwrap() {
            ServerMessage::EntitiesSpawned(entities) => spawned += entities.len(),
            ServerMessage::EntitiesChanged(changes) => changed += changes.len(),
            ServerMessage::EntitiesDespawned(ids) => despawned += ids.len(),
            _ => {}
        }
    }
    assert!(
        spawned >= 8,
        "{spawned} spawns (made, and came into the region)"
    );
    assert!(changed >= 20, "{changed} changes");
    assert!(
        despawned >= 4,
        "{despawned} despawns (removed, and left the region)"
    );
    assert!(
        session
            .alice
            .updates
            .iter()
            .flat_map(|update| &update.events)
            .any(|event| matches!(event, ClientEvent::EntitiesChanged))
    );
}

#[test]
fn a_replay_rebuilds_every_view_with_the_entities_the_live_client_had() {
    let session = play_session(80);
    let mut replayed = replay_player(session.recording.contents(), &ALL_PLUGINS).unwrap();
    match replayed.catch_up() {
        Err(ClientError::ConnectionClosed) => {}
        other => panic!("a replay ends by closing the connection: {other:?}"),
    }
    assert!(matches!(
        replayed.client.connection().end(),
        ReplayEnd::Complete
    ));
    assert_eq!(replayed.updates, session.alice.updates, "events differ");
    assert_eq!(replayed.summaries, session.alice.summaries, "views differ");
    // The summaries include the entities, and they were not always empty.
    assert!(
        session
            .alice
            .summaries
            .iter()
            .any(|summary| !summary.entities.is_empty())
    );
    assert_eq!(known_entities(&replayed), known_entities(&session.alice));
}

#[test]
fn identical_sessions_make_byte_identical_recordings() {
    assert_eq!(
        play_session(60).recording.contents(),
        play_session(60).recording.contents()
    );
}

/// The recording with the protocol version in its welcome (the first frame) changed.
fn with_welcome_version(recording: &[u8], version: u32) -> Vec<u8> {
    let mut reader = ReplayReader::new(Cursor::new(recording.to_vec())).unwrap();
    let mut frames = Vec::new();
    while let Some(frame) = reader.next_frame().unwrap() {
        frames.push(frame);
    }
    let ServerMessage::Welcome(mut welcome) = ServerMessage::decode(&frames[0]).unwrap() else {
        panic!("a recording starts with the welcome");
    };
    welcome.protocol_version = version;
    frames[0] = ServerMessage::Welcome(welcome).encode();
    let mut writer = ReplayWriter::new(Vec::new()).unwrap();
    for frame in &frames {
        writer.append(frame).unwrap();
    }
    writer.into_inner()
}

#[test]
fn a_recording_made_at_version_2_still_plays_and_other_versions_are_refused() {
    // Version 3 added messages and changed none: an older recording is read as it was made. This one is
    // a recording of a world with no entities whose welcome says it was made at version 2.
    let mut fixture = Fixture::new();
    let (mut alice, recording) = fixture.join_recorded(&ALL_PLUGINS);
    alice.watch(BrickBox::around(brick(0, 0, 0), 1));
    for _ in 0..5 {
        fixture.step();
        alice.catch_up().unwrap();
    }
    let rewritten = with_welcome_version(&recording.contents(), 2);
    let mut replayed = replay_player(rewritten, &ALL_PLUGINS).unwrap();
    let _ = replayed.catch_up();
    assert_eq!(replayed.summaries, alice.summaries);
    // A version nobody has made yet is refused, and so is one older than the oldest this reads.
    for refused in [
        protocol::PROTOCOL_VERSION + 1,
        protocol::OLDEST_READABLE_PROTOCOL_VERSION - 1,
    ] {
        let recording = with_welcome_version(&recording.contents(), refused);
        assert!(
            matches!(
                replay_player(recording, &ALL_PLUGINS),
                Err(ClientError::UnsupportedProtocolVersion(version)) if version == refused
            ),
            "{refused}"
        );
    }
}

#[test]
fn a_region_with_no_entities_in_it_leaves_a_player_knowing_none() {
    let mut session = play_session(30);
    assert!(!known_entities(&session.bob).is_empty(), "there were some");
    session
        .bob
        .watch(BrickBox::around(BrickCoordinate::new(200, 200, 200), 1));
    session.fixture.step();
    session.bob.catch_up().unwrap();
    assert!(known_entities(&session.bob).is_empty());
}
