//! The property replay recording exists for: a recording of the stream one client received, played
//! back, rebuilds exactly what that client saw, tick by tick.

use std::io::Cursor;

use client::{ClientError, ClientEvent, Handshake, HandshakeProgress};
use end_to_end_tests::{
    Fixture, Player, SharedBuffer, differences_from_server, game_id, replay_player,
};
use protocol::ServerMessage;
use replay::{RecordingConnection, ReplayConnection, ReplayEnd, ReplayReader};
use transport::loopback::LoopbackConnection;
use voxel_space::{BrickBox, BrickCoordinate};
use world_storage::test_support::{ALL_PLUGINS, Game, Plugin, brick, sample_world};

type RecordedPlayer = Player<RecordingConnection<LoopbackConnection, SharedBuffer>>;

struct Session {
    fixture: Fixture,
    alice: RecordedPlayer,
    recording: SharedBuffer,
    bob: Player<LoopbackConnection>,
}

/// Two players, one recorded, in a busy world: edits by both players and by the game itself, regions
/// that move, messages to everyone and to those nearby. `ticks` ticks are played after both joined.
fn play_session(mut fixture: Fixture, ticks: i64) -> Session {
    let (mut alice, recording) = fixture.join_recorded(&ALL_PLUGINS);
    let mut bob = fixture.join(&ALL_PLUGINS);
    alice.watch(BrickBox::around(brick(0, 0, 0), 2));
    bob.watch(BrickBox::around(brick(1, 0, 0), 1));
    for step in 0..ticks {
        match step % 6 {
            0 => alice.place([step - 20, 2, step % 7], "fancy_blocks:marble"),
            2 => bob.place([step, 3, -step], "base_blocks:dirt"),
            4 => alice.place([step * 2 - 40, 2, 5], "fancy_blocks:glass"),
            _ => {}
        }
        if step == 20 {
            alice.watch(BrickBox::around(brick(2, 0, -1), 2));
        }
        if step == 40 {
            bob.watch(BrickBox::around(brick(-1, 0, 0), 2));
            alice.watch(BrickBox::around(brick(0, 0, 0), 1));
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

/// Plays a replay to its end, which a replay signals as a closed connection.
fn play_to_the_end<R: std::io::Read + Send>(player: &mut Player<ReplayConnection<R>>) {
    match player.catch_up() {
        Err(ClientError::ConnectionClosed) => {}
        other => panic!("a replay ends by closing the connection: {other:?}"),
    }
}

/// A player on an already opened replay.
fn player_from<R: std::io::Read + Send>(
    connection: ReplayConnection<R>,
) -> Player<ReplayConnection<R>> {
    let handshake = Handshake::start(connection, game_id()).unwrap();
    let HandshakeProgress::Welcomed(welcomed) = handshake.poll().unwrap() else {
        panic!("the recording starts with the welcome");
    };
    let mut game = Game::new(&ALL_PLUGINS);
    let (client, _) = welcomed.finish(game.load_context()).unwrap();
    Player {
        client,
        game,
        updates: Vec::new(),
        summaries: Vec::new(),
    }
}

#[test]
fn a_replay_rebuilds_every_view_the_live_client_had_and_every_event_it_was_told() {
    let session = play_session(Fixture::new(), 60);
    let mut replayed = replay_player(session.recording.contents(), &ALL_PLUGINS).unwrap();
    play_to_the_end(&mut replayed);
    assert!(matches!(
        replayed.client.connection().end(),
        ReplayEnd::Complete
    ));

    assert_eq!(replayed.updates.len(), session.alice.updates.len());
    assert_eq!(replayed.updates, session.alice.updates, "events differ");
    assert_eq!(replayed.summaries, session.alice.summaries, "views differ");
    assert_eq!(replayed.tick(), session.alice.tick());

    // And the replay's picture is the server's truth, not merely the live client's.
    assert_eq!(
        differences_from_server(
            &replayed,
            session.fixture.server.world(),
            &session.fixture.game
        ),
        Vec::<String>::new()
    );
}

#[test]
fn the_scenario_exercises_every_kind_of_message() {
    // Guards the test above against passing because nothing happened.
    let session = play_session(Fixture::new(), 60);
    let mut reader = ReplayReader::new(Cursor::new(session.recording.contents())).unwrap();
    let (mut welcomes, mut regions, mut contents, mut edits, mut game_messages, mut ticks) =
        (0, 0, 0, 0, 0, 0);
    while let Some(frame) = reader.next_frame().unwrap() {
        match ServerMessage::decode(&frame).unwrap() {
            ServerMessage::Welcome(_) => welcomes += 1,
            ServerMessage::InterestChanged { .. } => regions += 1,
            ServerMessage::BrickContents { .. } => contents += 1,
            ServerMessage::VoxelEdits { .. } => edits += 1,
            ServerMessage::GameMessage { .. } => game_messages += 1,
            ServerMessage::TickCompleted { .. } => ticks += 1,
            ServerMessage::Disconnected { .. } => panic!("nobody was disconnected"),
        }
    }
    assert_eq!(welcomes, 1);
    assert_eq!(regions, 3, "the initial region and two moves");
    assert!(contents >= 4, "{contents} brick contents");
    assert!(
        edits >= 3,
        "{edits} edit messages: later edits to known bricks"
    );
    assert!(game_messages >= 6, "{game_messages} game messages");
    // One tick for each player joining, then the sixty played.
    assert_eq!(ticks, 62);
    let events: Vec<&ClientEvent> = session
        .alice
        .updates
        .iter()
        .flat_map(|update| &update.events)
        .collect();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ClientEvent::GameMessage { .. }))
    );
    assert_eq!(session.bob.updates.len(), 61);
}

#[test]
fn replaying_one_tick_at_a_time_gives_the_same_ticks_in_order() {
    let session = play_session(Fixture::new(), 30);
    let mut replayed = replay_player(session.recording.contents(), &ALL_PLUGINS).unwrap();
    let mut ticks = Vec::new();
    while let Ok(Some(update)) = replayed.client.poll_tick() {
        ticks.push(update.tick.get());
    }
    let first = ticks[0];
    assert_eq!(
        ticks,
        (first..first + ticks.len() as u64).collect::<Vec<_>>()
    );
    assert_eq!(ticks.len(), session.alice.updates.len());
}

#[test]
fn the_standard_session_makes_the_same_recording_on_every_platform() {
    // Pinned so that Windows and Linux (and any future machine) are held to the same bytes: terrain,
    // edits, message order and encoding all feed into them. A deliberate change to any of those
    // changes the numbers; so does the engine's version number, which the world description records.
    let recording = play_session(Fixture::new(), 40).recording.contents();
    assert_eq!(
        (recording.len(), crc32fast::hash(&recording)),
        (PINNED_RECORDING_LENGTH, PINNED_RECORDING_CHECKSUM)
    );
}

const PINNED_RECORDING_LENGTH: usize = 84_704;
const PINNED_RECORDING_CHECKSUM: u32 = 182_984_571;

#[test]
fn identical_sessions_make_byte_identical_recordings() {
    let first = play_session(Fixture::new(), 40);
    let second = play_session(Fixture::new(), 40);
    assert_eq!(first.recording.contents(), second.recording.contents());
}

#[test]
fn a_replay_with_fewer_plugins_still_rebuilds_what_the_edited_bricks_mean() {
    // The world has terrain from a generator the replaying program lacks, and edited bricks holding
    // content it lacks. The edited bricks must survive (as placeholders), and nothing may crash.
    let game = Game::new(&ALL_PLUGINS);
    let world = sample_world(&game);
    let mut fixture = Fixture::with_world(game, world);
    let (mut alice, recording) = fixture.join_recorded(&ALL_PLUGINS);
    alice.watch(BrickBox::around(brick(0, 0, 0), 2));
    for step in 0..12 {
        if step == 4 {
            // The region whose terrain came from the fancy generator.
            alice.watch(BrickBox::around(brick(100, 0, 0), 1));
        }
        if step == 8 {
            alice.watch(BrickBox::around(brick(1, 0, 0), 1));
        }
        fixture.step();
        alice.catch_up().unwrap();
    }

    let reduced = [Plugin::BaseBlocks, Plugin::FancyBlocks];
    let mut replayed = replay_player(recording.contents(), &reduced).unwrap();
    play_to_the_end(&mut replayed);
    assert_eq!(replayed.updates.len(), alice.updates.len());
    for (live, replay) in alice.summaries.iter().zip(&replayed.summaries) {
        assert_eq!(live.tick, replay.tick);
        assert_eq!(live.watched, replay.watched);
    }
    assert_eq!(
        differences_from_server(&replayed, fixture.server.world(), &fixture.game),
        Vec::<String>::new()
    );
    // The fancy generator's region is there, but its terrain cannot be generated here.
    assert!(matches!(
        replayed
            .client
            .world()
            .world()
            .effective_brick(BrickCoordinate::new(100, 0, 0)),
        world_storage::BrickSource::MissingGenerator(_)
    ));
}

#[test]
fn a_recording_cut_short_replays_its_complete_ticks_and_says_it_was_interrupted() {
    let session = play_session(Fixture::new(), 40);
    let bytes = session.recording.contents();
    for cut in [bytes.len() / 2, bytes.len() * 3 / 4, bytes.len() - 7] {
        let connection = ReplayConnection::new(Cursor::new(bytes[..cut].to_vec())).unwrap();
        let mut replayed = player_from(connection);
        play_to_the_end(&mut replayed);
        assert!(!replayed.updates.is_empty(), "cut at {cut}");
        assert!(
            replayed.updates.len() < session.alice.updates.len(),
            "cut at {cut}"
        );
        // Whatever it replayed is a prefix of what happened live.
        assert_eq!(
            replayed.updates,
            session.alice.updates[..replayed.updates.len()],
            "cut at {cut}"
        );
        assert_eq!(
            replayed.summaries,
            session.alice.summaries[..replayed.summaries.len()],
            "cut at {cut}"
        );
        assert!(
            matches!(
                replayed.client.connection().end(),
                ReplayEnd::Interrupted | ReplayEnd::Complete
            ),
            "cut at {cut}: {:?}",
            replayed.client.connection().end()
        );
    }
    // Seven bytes off the end is certainly inside the last record.
    let connection = ReplayConnection::new(Cursor::new(bytes[..bytes.len() - 7].to_vec())).unwrap();
    let mut replayed = player_from(connection);
    play_to_the_end(&mut replayed);
    assert!(matches!(
        replayed.client.connection().end(),
        ReplayEnd::Interrupted
    ));
}

#[test]
fn a_damaged_recording_replays_up_to_the_damage_and_no_further() {
    let session = play_session(Fixture::new(), 40);
    let mut bytes = session.recording.contents();
    let damage_at = bytes.len() * 3 / 4;
    bytes[damage_at] ^= 0x10;
    let connection = ReplayConnection::new(Cursor::new(bytes)).unwrap();
    let mut replayed = player_from(connection);
    play_to_the_end(&mut replayed);
    assert!(!replayed.updates.is_empty());
    assert!(replayed.updates.len() < session.alice.updates.len());
    assert_eq!(
        replayed.updates,
        session.alice.updates[..replayed.updates.len()]
    );
    assert!(matches!(
        replayed.client.connection().end(),
        ReplayEnd::Failed(_) | ReplayEnd::Interrupted
    ));
}
