//! The game, played through its server and sessions, without a window.

mod common;

use game_shell::Identity;
use game_shell::client_plugin::MouseButton;

use protocol::{ClientMessage, Message};

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use client::ClientError;
use common::{
    Player, PlayerExt, Rig, SEED, VIEW_RADIUS, above_ground, ground_height, position_of, voxels_of,
};
use game_shell::files::{open_world, record_connection, save_world};
use game_shell::host::{HostSettings, ServerThread};
use game_shell::session::{Mode, Session};
use game_shell::view::camera_at;
use game_shell::{GameContent, KeyCode};
use transport::{Connection, ConnectionClosed};
use voxel_space::{FaceDirection, VoxelCoordinate};
use voxunami_cave::content::CaveContent;
use voxunami_cave::game::{
    Cave, break_block, place_block, place_block_with_state, request_block, request_block_with_state,
};
use voxunami_cave::logic::REACH;

/// A player standing 3 voxels above the ground at the column (16, 16), looking straight down.
fn player_above_ground(rig: &mut Rig) -> Player {
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.camera = above_ground(&player, 16, 16, 3.0);
    player.teleport_to_camera();
    rig.settle(&mut [&mut player]);
    // (What the setting up was answered with is not what the test asks about.)
    player.take_command_replies();
    player
}

fn air(rig: &Rig) -> u32 {
    rig.content.blocks.air
}

#[test]
fn the_world_around_a_player_is_loaded_and_meshed() {
    let mut rig = Rig::new();
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, -0.3);
    let updates = {
        let server = &mut rig.server;
        player.settle(|| {
            server.step();
        })
    }
    .unwrap();
    assert!(!updates.is_empty());
    assert!(player.mesher().loaded_brick_count() > 100);
    let ground = ground_height(&player, 16, 16);
    assert!((20..200).contains(&ground), "{ground}");
    assert_eq!(
        player
            .mesher()
            .material_at(VoxelCoordinate::new(16, ground, 16)),
        Some(rig.content.blocks.grass)
    );
}

#[test]
fn breaking_a_block_removes_it_for_every_player() {
    let mut rig = Rig::new();
    let mut breaker = player_above_ground(&mut rig);
    let mut onlooker = rig.join([20.5, 110.0, 20.5], 0.0, 0.0);
    rig.settle(&mut [&mut breaker, &mut onlooker]);

    let hit = breaker.target().expect("the ground is within reach");
    assert_eq!(hit.voxel.y(), ground_height(&breaker, 16, 16));
    assert_eq!(
        onlooker.mesher().is_opaque_at(hit.voxel),
        Some(true),
        "the onlooker sees the block before it is broken"
    );

    break_block(&mut breaker);
    rig.settle(&mut [&mut breaker, &mut onlooker]);

    for (name, player) in [("breaker", &breaker), ("onlooker", &onlooker)] {
        assert_eq!(
            player.mesher().material_at(hit.voxel),
            Some(air(&rig)),
            "{name} still sees the block"
        );
    }
    assert_eq!(rig.server.world().main().overrides.brick_count(), 1);
    // The player looks into the hole now: the next block down is the target.
    assert_eq!(breaker.target().unwrap().voxel.y(), hit.voxel.y() - 1);
}

#[test]
fn placing_puts_the_block_in_front_of_the_face_that_was_hit() {
    let mut rig = Rig::new();
    let mut player = player_above_ground(&mut rig);
    let hit = player.target().unwrap();
    assert_eq!(
        hit.previous,
        VoxelCoordinate::new(hit.voxel.x(), hit.voxel.y() + 1, hit.voxel.z())
    );

    place_block(&mut player, "base_blocks:stone");
    rig.settle(&mut [&mut player]);
    assert_eq!(
        player.mesher().material_at(hit.previous),
        Some(rig.content.blocks.stone)
    );
    // The new block is now what the crosshair is on.
    assert_eq!(player.target().unwrap().voxel, hit.previous);
}

#[test]
fn the_server_refuses_what_is_out_of_reach_unknown_or_from_a_player_with_no_position() {
    let mut rig = Rig::new();
    let mut player = player_above_ground(&mut rig);
    let own = position_of(&player.camera);
    let near = VoxelCoordinate::new(
        own.brick().x() * 32 + own.offset()[0] as i128,
        own.brick().y() * 32 + own.offset()[1] as i128 - 3,
        own.brick().z() * 32 + own.offset()[2] as i128,
    );

    // Too far, a block nobody has heard of, and (from a second player who is far away) a perfectly
    // reasonable request: none changes the world.
    request_block(
        &mut player,
        VoxelCoordinate::new(near.x() + (REACH as i128) + 5, near.y(), near.z()),
        "base_blocks:stone",
    );
    request_block(&mut player, near, "nonexistent:block");
    let mut stranger = rig.join([500.5, 110.0, 16.5], 0.0, 0.0);
    request_block(&mut stranger, near, "base_blocks:stone");
    for _ in 0..3 {
        rig.step();
    }
    assert_eq!(rig.server.world().main().overrides.brick_count(), 0);

    // The same request from the player who is in reach works.
    request_block(&mut player, near, "base_blocks:stone");
    for _ in 0..2 {
        rig.step();
    }
    assert_eq!(rig.server.world().main().overrides.brick_count(), 1);
}

#[test]
fn players_see_where_each_other_are_until_they_leave() {
    let mut rig = Rig::new();
    let mut first = rig.join([16.5, 110.0, 16.5], 0.5, -0.2);
    let mut second = rig.join([90.5, 120.0, -40.5], -1.0, 0.1);
    rig.settle(&mut [&mut first, &mut second]);
    first.advance().unwrap();
    second.advance().unwrap();

    for player in [&first, &second] {
        let poses: Vec<_> = player.players().values().map(|state| &state.pose).collect();
        assert_eq!(poses.len(), 2);
        assert!(
            poses
                .iter()
                .any(|pose| pose.position == first.camera.position && pose.yaw == first.camera.yaw)
        );
        assert!(poses.iter().any(
            |pose| pose.position == second.camera.position && pose.pitch == second.camera.pitch
        ));
    }

    // Each draws the other, and never itself.
    let others: Vec<_> = first.other_players().collect();
    assert_eq!(others.len(), 1);
    assert_eq!(others[0].1.position, second.camera.position);
    let others: Vec<_> = second.other_players().collect();
    assert_eq!(others.len(), 1);
    assert_eq!(others[0].1.position, first.camera.position);

    second.say_goodbye();
    drop(second);
    rig.step();
    rig.step();
    first.advance().unwrap();
    assert_eq!(first.players().len(), 1);
    assert_eq!(first.other_players().count(), 0);
}

#[test]
fn edits_survive_saving_and_loading_the_world() {
    let directory = std::env::temp_dir().join(format!("voxunami_cave_test_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("world.voxunami");

    let mut rig = Rig::new();
    let mut player = player_above_ground(&mut rig);
    let hit = player.target().unwrap();
    break_block(&mut player);
    rig.settle(&mut [&mut player]);
    player.say_goodbye();
    rig.step();
    let Rig {
        content, server, ..
    } = rig;
    let mut world = server.into_world();
    save_world(&content, &mut world, &path).unwrap();

    // A new program run: the world file, a fresh server, a fresh player.
    let mut content = CaveContent::new();
    let loaded = open_world(&mut content, Some(&path), 999_999).unwrap();
    assert_eq!(
        loaded.world_seed, SEED,
        "the seed comes from the file, not the command line"
    );
    let mut rig = Rig::with_world(content, loaded);
    let mut later = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut later]);
    assert_eq!(
        later.mesher().material_at(hit.voxel),
        Some(air(&rig)),
        "the broken block is still broken"
    );
    assert_eq!(
        later.mesher().material_at(VoxelCoordinate::new(
            hit.voxel.x() + 1,
            hit.voxel.y(),
            hit.voxel.z()
        )),
        Some(rig.content.blocks.grass),
        "its neighbours were not"
    );
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_recording_replays_the_session_from_the_recorded_players_point_of_view() {
    let directory =
        std::env::temp_dir().join(format!("voxunami_cave_replay_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("session.voxreplay");

    let mut rig = Rig::new();
    let connection = rig.connector.connect().unwrap();
    let recorded = record_connection(connection, &path).unwrap();
    let mut player = rig.join_with(recorded, [16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.camera = above_ground(&player, 16, 16, 3.0);
    player.teleport_to_camera();
    rig.settle(&mut [&mut player]);
    let hit = player.target().unwrap();
    break_block(&mut player);
    rig.settle(&mut [&mut player]);
    player.advance().unwrap();
    let final_summary = player.client().world().summary();
    let final_camera = player.camera;
    drop(player); // closes the recording

    let connection: Box<dyn transport::Connection> =
        Box::new(game_shell::files::open_recording(&path).unwrap());
    let mut replay = Session::<_, Cave>::connect(
        connection,
        &Identity::generate(),
        Mode::Replaying,
        VIEW_RADIUS,
        camera_at([0.0; 3], 0.0, 0.0),
        || {},
    )
    .unwrap();
    let outcome = loop {
        match replay.advance_one_tick() {
            Ok(_) => {}
            Err(error) => break error,
        }
    };
    assert!(matches!(outcome, ClientError::ConnectionClosed));
    replay.settle(|| {}).unwrap();

    assert_eq!(replay.client().world().summary(), final_summary);
    assert_eq!(replay.camera.position, final_camera.position);
    assert_eq!(
        (replay.camera.yaw, replay.camera.pitch),
        (final_camera.yaw, final_camera.pitch)
    );
    assert_eq!(
        replay.mesher().material_at(hit.voxel),
        Some(rig.content.blocks.air)
    );
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_player_who_comes_back_is_where_they_were_and_not_where_they_say_they_start() {
    let mut rig = Rig::new();
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.camera = above_ground(&player, 16, 16, 3.0);
    player.teleport_to_camera();
    rig.settle(&mut [&mut player]);
    let there = voxels_of(&player.camera);
    assert!((there[1] - 110.0).abs() > 1.0, "they moved: {there:?}");
    player.say_goodbye();
    rig.step();

    // The same install joins again, saying it starts far away; the server puts it where it left.
    let connection = rig.connector.connect().unwrap();
    let mut again = rig.join_as(Box::new(connection), 1, [100.5, 110.0, 100.5], 0.0, 0.0);
    rig.settle(&mut [&mut again]);
    let now = voxels_of(&again.camera);
    for axis in 0..3 {
        assert!(
            (now[axis] - there[axis]).abs() < 0.5,
            "{now:?} is not where they left, {there:?}"
        );
    }

    // Another install, saying the same, starts where it says.
    let connection = rig.connector.connect().unwrap();
    let mut other = rig.join_as(Box::new(connection), 2, [100.5, 110.0, 100.5], 0.0, 0.0);
    rig.settle(&mut [&mut other, &mut again]);
    let theirs = voxels_of(&other.camera);
    assert!(
        (theirs[0] - 100.5).abs() < 0.5 && (theirs[2] - 100.5).abs() < 0.5,
        "{theirs:?}"
    );
}

#[test]
fn only_entities_of_the_player_kind_are_players_to_a_session() {
    use player_presence::{PlayerClient, PlayerLook};
    use protocol::ServerMessage;
    use replay::{ReplayConnection, ReplayReader, ReplayWriter};

    let directory =
        std::env::temp_dir().join(format!("voxunami_cave_kinds_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("session.voxreplay");
    let mut rig = Rig::new();
    let recorded = record_connection(rig.connector.connect().unwrap(), &path).unwrap();
    let mut player = rig.join_with(recorded, [16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    let own = player.client().world().server_info().client;
    drop(player);
    let recording = std::fs::read(&path).unwrap();
    std::fs::remove_dir_all(&directory).unwrap();

    // Next to the real player, an entity of another kind that carries the very components a player has.
    let mut reader = ReplayReader::new(std::io::Cursor::new(recording)).unwrap();
    let mut writer = ReplayWriter::new(Vec::new()).unwrap();
    let mut impostor_told = false;
    while let Some(frame) = reader.next_frame().unwrap() {
        if matches!(
            ServerMessage::decode(&frame).unwrap(),
            ServerMessage::TickCompleted { .. }
        ) && !impostor_told
        {
            impostor_told = true;
            let impostor = ServerMessage::EntitiesSpawned(vec![protocol::EntityState {
                id: protocol::EntityId::new(900),
                kind: "test:rock".parse().unwrap(),
                pose: voxel_space::EntityPose::new(
                    voxel_space::GridId::MAIN,
                    camera_at([16.5, 110.0, 16.5], 0.0, 0.0).position,
                    voxel_space::Orientation::IDENTITY,
                ),
                components: vec![
                    (
                        PlayerClient::kind(),
                        PlayerClient(protocol::ClientId::new(77)).encode(),
                    ),
                    (
                        PlayerLook::kind(),
                        PlayerLook {
                            yaw: 0.0,
                            pitch: 0.0,
                        }
                        .encode(),
                    ),
                ],
            }]);
            writer.append(&impostor.encode()).unwrap();
        }
        writer.append(&frame).unwrap();
    }
    let connection: Box<dyn Connection> =
        Box::new(ReplayConnection::new(std::io::Cursor::new(writer.into_inner())).unwrap());
    let mut replay = Session::<_, Cave>::connect(
        connection,
        &Identity::generate(),
        Mode::Replaying,
        VIEW_RADIUS,
        camera_at([0.0; 3], 0.0, 0.0),
        || {},
    )
    .unwrap();
    while replay.advance_one_tick().is_ok() {}
    assert_eq!(
        replay.players().keys().copied().collect::<Vec<_>>(),
        [own],
        "the rock is not a player"
    );
}

/// A connection that remembers what was sent through it.
#[derive(Debug)]
struct Spy {
    inner: Box<dyn Connection>,
    sent: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Connection for Spy {
    fn send(&mut self, frame: Vec<u8>) -> Result<(), ConnectionClosed> {
        self.sent.lock().unwrap().push(frame.clone());
        self.inner.send(frame)
    }

    fn receive(&mut self) -> Result<Option<Vec<u8>>, ConnectionClosed> {
        self.inner.receive()
    }
}

#[test]
fn a_session_that_is_watching_a_replay_asks_the_server_for_nothing() {
    let mut rig = Rig::new();
    let mut player = player_above_ground(&mut rig);
    let sent = Arc::new(Mutex::new(Vec::new()));
    let spy = Spy {
        inner: Box::new(rig.connector.connect().unwrap()),
        sent: Arc::clone(&sent),
    };
    let server = &mut rig.server;
    let mut watcher = Session::<_, Cave>::connect(
        Box::new(spy) as Box<dyn Connection>,
        &Identity::from_secret([50; 32]),
        Mode::Replaying,
        VIEW_RADIUS,
        player.camera,
        || {
            server.step();
        },
    )
    .unwrap();
    let after_hello = sent.lock().unwrap().len();
    let kinds: Vec<_> = sent
        .lock()
        .unwrap()
        .iter()
        .map(|frame| ClientMessage::decode(frame).unwrap())
        .collect();
    assert!(
        matches!(
            &kinds[..],
            [ClientMessage::Hello { .. }, ClientMessage::Prove { .. }]
        ),
        "only the hello and the proof of who it is: {kinds:?}"
    );

    // Whatever the watcher's camera is doing, it moves, aims and clicks without telling anyone.
    watcher.camera = player.camera;
    watcher.teleport_to_camera();
    watcher.send_pose();
    let _ = watcher.update_view();
    break_block(&mut watcher);
    place_block(&mut watcher, "base_blocks:stone");
    request_block(
        &mut watcher,
        VoxelCoordinate::new(16, 60, 16),
        "base_blocks:stone",
    );
    assert_eq!(sent.lock().unwrap().len(), after_hello);
    player.say_goodbye();
}

#[test]
fn the_integrated_server_ticks_by_itself_and_hands_back_the_world() {
    let content = CaveContent::new();
    let world = content.new_overworld_world(SEED);
    let host =
        ServerThread::<Cave>::start(content, world, HostSettings::local(VIEW_RADIUS)).unwrap();
    let connection = host.connect().unwrap();
    let start = camera_at([16.5, 110.0, 16.5], 0.0, 0.0);
    let mut player = Session::<_, Cave>::connect(
        Box::new(connection) as Box<dyn transport::Connection>,
        &Identity::from_secret([60; 32]),
        Mode::Playing,
        VIEW_RADIUS,
        start,
        || {
            std::thread::sleep(Duration::from_millis(1));
        },
    )
    .unwrap();
    player.send_pose();
    let first = player.client().world().tick();
    let deadline = Instant::now() + Duration::from_secs(30);
    while player.client().world().tick().get() < first.get() + 10
        || player.mesher().pending_jobs() > 0
    {
        player.advance().unwrap();
        let _ = player.update_view();
        assert!(Instant::now() < deadline, "the server stopped ticking");
        std::thread::sleep(Duration::from_millis(5));
    }
    player.say_goodbye();
    drop(player);
    let (world, _content) = host.stop().unwrap();
    assert_eq!(world.world_seed, SEED);
}

/// A world of two levels with the same terrain, so that what differs between them is only what
/// players do. (Levels of one world share its seed; they would differ by terrain generator version.)
fn two_level_rig() -> (Rig, voxel_space::LevelIndex) {
    let content = CaveContent::new();
    let mut world = content.new_overworld_world(SEED);
    let second = world
        .add_level(
            "voxunami_cave:cellar".parse().unwrap(),
            content.new_overworld_world(SEED).into_main(),
        )
        .unwrap();
    (Rig::with_world(content, world), second)
}

#[test]
fn a_player_can_go_to_another_level_and_what_they_do_there_stays_there() {
    let (mut rig, second) = two_level_rig();
    let mut mover = player_above_ground(&mut rig);
    let mut stayer = rig.join([20.5, 110.0, 20.5], 0.0, 0.0);
    rig.settle(&mut [&mut mover, &mut stayer]);
    assert_eq!(mover.level(), voxel_space::LevelIndex::MAIN);

    // The player asks to go to the cellar by its short name and is moved there, with the same
    // coordinates, and told so.
    mover.send_command("/level go cellar");
    rig.settle(&mut [&mut mover, &mut stayer]);
    assert_eq!(mover.level(), second);
    assert_eq!(
        mover.take_command_replies(),
        ["you are now in voxunami_cave:cellar"]
    );
    assert_eq!(
        mover.client().world().watched().map(|(level, _)| level),
        Some(second)
    );
    // The view is the cellar's terrain, around the same place.
    assert!(mover.mesher().loaded_brick_count() > 100);
    let ground = ground_height(&mover, 16, 16);
    assert_eq!(
        mover
            .mesher()
            .is_opaque_at(VoxelCoordinate::new(16, ground, 16)),
        Some(true)
    );

    // Players in different levels do not see each other: each knows only of the players in the level
    // they watch, which is only themselves.
    assert_eq!(mover.players().len(), 1);
    assert_eq!(stayer.players().len(), 1);
    assert_eq!(mover.other_players().count(), 0);
    assert_eq!(stayer.other_players().count(), 0);

    // What the player breaks in the cellar is broken only there.
    let hit = mover.target().expect("the ground is within reach");
    break_block(&mut mover);
    rig.settle(&mut [&mut mover, &mut stayer]);
    assert_eq!(
        rig.server
            .world()
            .level(second)
            .unwrap()
            .overrides
            .brick_count(),
        1
    );
    assert_eq!(rig.server.world().main().overrides.brick_count(), 0);
    assert_eq!(mover.mesher().is_opaque_at(hit.voxel), Some(false));
    assert_eq!(stayer.mesher().is_opaque_at(hit.voxel), Some(true));

    // When the other player follows, they see each other and the broken block.
    stayer.send_command("/level go voxunami_cave:cellar");
    rig.settle(&mut [&mut mover, &mut stayer]);
    assert_eq!(stayer.level(), second);
    assert_eq!(stayer.mesher().is_opaque_at(hit.voxel), Some(false));
    assert_eq!(mover.other_players().count(), 1);
    assert_eq!(stayer.other_players().count(), 1);

    // And back: the main level is as it was, whole.
    mover.send_command("/level go overworld");
    rig.settle(&mut [&mut mover, &mut stayer]);
    assert_eq!(mover.level(), voxel_space::LevelIndex::MAIN);
    assert_eq!(mover.mesher().is_opaque_at(hit.voxel), Some(true));
    assert_eq!(mover.other_players().count(), 0);
}

#[test]
fn a_recording_follows_the_recorded_player_into_another_level() {
    let (mut rig, second) = two_level_rig();
    let directory =
        std::env::temp_dir().join(format!("voxunami_cave_levels_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("levels.voxreplay");
    let connection = rig.connector.connect().unwrap();
    let recorded = record_connection(connection, &path).unwrap();
    let mut player = rig.join_with(recorded, [16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.camera = above_ground(&player, 16, 16, 3.0);
    player.teleport_to_camera();
    rig.settle(&mut [&mut player]);
    player.send_command("/level go cellar");
    rig.settle(&mut [&mut player]);
    let hit = player.target().unwrap();
    break_block(&mut player);
    rig.settle(&mut [&mut player]);
    let live_summary = player.client().world().summary();
    drop(player);

    let connection: Box<dyn Connection> =
        Box::new(game_shell::files::open_recording(&path).unwrap());
    let mut replay = Session::<_, Cave>::connect(
        connection,
        &Identity::generate(),
        Mode::Replaying,
        VIEW_RADIUS,
        camera_at([0.0; 3], 0.0, 0.0),
        || {},
    )
    .unwrap();
    while replay.advance_one_tick().is_ok() {}
    replay.settle(|| {}).unwrap();
    assert_eq!(replay.client().world().summary(), live_summary);
    assert_eq!(replay.level(), second);
    assert_eq!(replay.mesher().is_opaque_at(hit.voxel), Some(false));
    let replies = replay.take_command_replies();
    assert_eq!(replies.len(), 3, "{replies:?}");
    assert_eq!(replies[0], "you are flying");
    assert!(replies[1].starts_with("teleported to "), "{replies:?}");
    assert_eq!(replies[2], "you are now in voxunami_cave:cellar");
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn commands_the_game_does_not_know_are_answered() {
    let mut rig = Rig::new();
    let mut player = player_above_ground(&mut rig);
    player.send_command("/dance");
    player.send_command("/level");
    rig.settle(&mut [&mut player]);
    assert_eq!(
        player.take_command_replies(),
        [
            "unknown command; try /fly, /gamemode [MODE...], /help [COMMAND], /level [SUBCOMMAND...], /light [SUBCOMMAND...], /ship ACTION [DETAILS...], /tp [WHERE] [NUMBERS...]",
            "you are in voxunami_cave:overworld"
        ]
    );
}

/// The block and state the server holds at a voxel of the main level.
fn server_block_and_state(rig: &Rig, voxel: VoxelCoordinate) -> (u32, u32) {
    let overrides = &rig.server.world().main().overrides;
    (
        overrides.get(rig.content.block_channel(), voxel),
        overrides.get(rig.content.state_channel(), voxel),
    )
}

#[test]
fn a_block_placed_in_a_state_has_that_shape_for_every_player() {
    let mut rig = Rig::new();
    let mut builder = player_above_ground(&mut rig);
    let mut onlooker = rig.join([20.5, 110.0, 20.5], 0.0, 0.0);
    rig.settle(&mut [&mut builder, &mut onlooker]);
    let hit = builder.target().unwrap();

    place_block_with_state(&mut builder, "base_blocks:stone", 1); // a bottom slab
    rig.settle(&mut [&mut builder, &mut onlooker]);

    let stone = rig.content.blocks.stone;
    assert_eq!(server_block_and_state(&rig, hit.previous), (stone, 1));
    for (name, player) in [("builder", &builder), ("onlooker", &onlooker)] {
        let mesher = player.mesher();
        assert_eq!(mesher.material_at(hit.previous), Some(stone), "{name}");
        assert_eq!(mesher.state_at(hit.previous), Some(1), "{name}");
        // A slab is something to stand on and to aim at, but it does not hide what is behind it.
        assert_eq!(mesher.is_solid_at(hit.previous), Some(true), "{name}");
        assert_eq!(mesher.is_opaque_at(hit.previous), Some(false), "{name}");
    }
    assert_eq!(builder.target().unwrap().voxel, hit.previous);
}

#[test]
fn the_server_refuses_states_a_block_does_not_have_and_air_has_only_the_first() {
    let mut rig = Rig::new();
    let mut player = player_above_ground(&mut rig);
    let own = position_of(&player.camera);
    let near = VoxelCoordinate::new(
        own.brick().x() * 32 + own.offset()[0] as i128,
        own.brick().y() * 32 + own.offset()[1] as i128 - 3,
        own.brick().z() * 32 + own.offset()[2] as i128,
    );
    // Stone has the seven shapes (how they stand is the orientation, which every block has): state 7 is the first it does not have.
    let shapes = rig
        .content
        .block_table()
        .state_count(rig.content.blocks.stone);
    assert_eq!(shapes, 7);

    request_block_with_state(&mut player, near, "base_blocks:stone", shapes);
    request_block_with_state(&mut player, near, "base_blocks:stone", u32::MAX);
    request_block_with_state(&mut player, near, "base_blocks:air", 1);
    for _ in 0..3 {
        rig.step();
    }
    assert_eq!(rig.server.world().main().overrides.brick_count(), 0);

    // The last state that exists is accepted.
    request_block_with_state(&mut player, near, "base_blocks:stone", shapes - 1);
    for _ in 0..2 {
        rig.step();
    }
    assert_eq!(
        server_block_and_state(&rig, near),
        (rig.content.blocks.stone, shapes - 1)
    );
}

#[test]
fn breaking_a_shaped_block_leaves_air_in_the_first_state() {
    let mut rig = Rig::new();
    let mut player = player_above_ground(&mut rig);
    let hit = player.target().unwrap();
    place_block_with_state(&mut player, "base_blocks:dirt", 5);
    rig.settle(&mut [&mut player]);
    assert_eq!(
        server_block_and_state(&rig, hit.previous),
        (rig.content.blocks.dirt, 5)
    );

    break_block(&mut player);
    rig.settle(&mut [&mut player]);

    assert_eq!(server_block_and_state(&rig, hit.previous), (air(&rig), 0));
    assert_eq!(player.mesher().state_at(hit.previous), Some(0));
    assert_eq!(player.mesher().is_solid_at(hit.previous), Some(false));
}

#[test]
fn the_shape_key_cycles_through_every_shape_and_placing_uses_the_one_chosen() {
    let mut rig = Rig::new();
    let mut player = player_above_ground(&mut rig);
    let hit = player.target().unwrap();
    assert_eq!(player.hud_text(), "placing stone (whole)");

    player.key_pressed(KeyCode::KeyR);
    assert_eq!(player.hud_text(), "placing stone (bottom slab)");
    player.mouse_pressed(MouseButton::Secondary);
    rig.settle(&mut [&mut player]);
    assert_eq!(
        server_block_and_state(&rig, hit.previous),
        (rig.content.blocks.stone, 1)
    );

    // Choosing another block keeps the shape; pressing the key enough times comes back to whole.
    player.key_pressed(KeyCode::Digit3);
    assert_eq!(player.hud_text(), "placing grass (bottom slab)");
    // (The lamp has one state, a whole cube; the shape chosen for the blocks that have more is kept in range for it. It used to say "bottom slab"
    // here, and ask the server for a state the lamp does not have, which the server refused.)
    player.key_pressed(KeyCode::Digit4);
    assert_eq!(player.hud_text(), "placing lamp (whole)");
    player.key_pressed(KeyCode::Digit3);
    for _ in 1..7 {
        player.key_pressed(KeyCode::KeyR);
    }
    assert_eq!(player.hud_text(), "placing grass (whole)");
}

#[test]
fn the_dev_docking_port_is_chosen_with_its_number_has_one_state_and_is_placed_facing_the_player() {
    let mut rig = Rig::new();
    let mut player = player_above_ground(&mut rig);
    let hit = player.target().unwrap();
    // The seventh block there is to place.
    player.key_pressed(KeyCode::Digit7);
    assert_eq!(player.hud_text(), "placing dev_docking_port (whole)");
    // It has one state: the shape key leaves it as it is (how it faces is the orientation, which comes from where the player is).
    player.key_pressed(KeyCode::KeyR);
    assert_eq!(player.hud_text(), "placing dev_docking_port (whole)");
    player.mouse_pressed(MouseButton::Secondary);
    rig.settle(&mut [&mut player]);
    let port = {
        rig.content.registries()[0]
            .1
            .raw_id_of(&voxunami_cave::content::dev_docking_port_id())
            .unwrap()
    };
    assert_eq!(server_block_and_state(&rig, hit.previous), (port, 0));
    let orientation = rig
        .server
        .world()
        .main()
        .overrides
        .get(rig.content.orientation_channel(), hit.previous);
    // The player looks straight down from three voxels over the place, close beside it: the front of the port faces up.
    assert_eq!(
        rig.content.block_table().front_face(port, 0, orientation),
        Some(FaceDirection::PositiveY)
    );
    // Choosing a block with more states keeps the number in range for the one chosen: stone has seven.
    player.key_pressed(KeyCode::Digit1);
    assert_eq!(player.hud_text(), "placing stone (whole)");
}

/// Ticks the server `count` times.
fn run_ticks(rig: &mut Rig, count: usize) {
    for _ in 0..count {
        rig.step();
    }
}

/// The block light the server holds at a voxel of the main level.
fn server_light(rig: &Rig, voxel: VoxelCoordinate) -> u32 {
    rig.server
        .world()
        .main()
        .overrides
        .get(rig.content.light_channel(), voxel)
}

/// The block light a player's copy of the world holds at a voxel of the main level.
fn seen_light(rig: &Rig, player: &Player, voxel: VoxelCoordinate) -> u32 {
    player
        .client()
        .world()
        .world()
        .main()
        .overrides
        .get(rig.content.light_channel(), voxel)
}

fn above(voxel: VoxelCoordinate, height: i128) -> VoxelCoordinate {
    VoxelCoordinate::new(voxel.x(), voxel.y() + height, voxel.z())
}

#[test]
fn a_lamp_lights_the_air_around_it_a_voxel_a_tick_and_every_player_sees_it() {
    let mut rig = Rig::new();
    let mut builder = player_above_ground(&mut rig);
    let mut onlooker = rig.join([20.5, 110.0, 20.5], 0.0, 0.0);
    rig.settle(&mut [&mut builder, &mut onlooker]);
    let spot = builder.target().unwrap().previous;
    let emission = u32::from(base_blocks::LAMP_EMISSION);

    place_block(&mut builder, "base_blocks:lamp");
    // The lamp is placed and looked at on the server in the same tick, and the light has gone one
    // voxel by then, and one more with each tick.
    run_ticks(&mut rig, 1);
    assert_eq!(server_light(&rig, above(spot, 1)), emission - 1);
    assert_eq!(server_light(&rig, above(spot, 2)), 0);
    run_ticks(&mut rig, 3);
    assert_eq!(server_light(&rig, above(spot, 4)), emission - 4);
    assert_eq!(server_light(&rig, above(spot, 5)), 0);
    run_ticks(&mut rig, 60);
    rig.settle(&mut [&mut builder, &mut onlooker]);

    // Straight up from a lamp there is only air: a level less for every voxel.
    for height in [1, 5, 20, 63] {
        let expected = emission - height as u32;
        assert_eq!(
            server_light(&rig, above(spot, height)),
            expected,
            "{height}"
        );
        for (name, player) in [("builder", &builder), ("onlooker", &onlooker)] {
            assert_eq!(
                seen_light(&rig, player, above(spot, height)),
                expected,
                "{name} {height}"
            );
        }
    }
    assert_eq!(
        server_light(&rig, above(spot, 64)),
        0,
        "the reach of a lamp is its emission"
    );
    assert_eq!(
        server_light(&rig, spot),
        0,
        "the lamp itself is a whole block and holds none"
    );
    assert_eq!(
        rig.server
            .world()
            .main()
            .overrides
            .get(rig.content.block_channel(), spot),
        rig.content.blocks.lamp
    );
}

#[test]
fn the_faces_next_to_a_lamp_are_drawn_with_its_light() {
    let mut rig = Rig::new();
    let mut builder = player_above_ground(&mut rig);
    let mut updates = Vec::new();
    place_block(&mut builder, "base_blocks:lamp");
    run_ticks(&mut rig, 84);
    {
        let server = &mut rig.server;
        updates.extend(
            builder
                .settle(|| {
                    server.step();
                })
                .unwrap(),
        );
    }
    // The newest mesh of each brick is the one drawn.
    let mut newest = std::collections::HashMap::new();
    for update in updates {
        if let world_view::MeshUpdate::Upload { coordinate, mesh } = update {
            newest.insert(coordinate, mesh);
        }
    }
    let lights: Vec<u8> = newest
        .values()
        .flat_map(|mesh| mesh.quads.iter().map(|quad| quad.unpack().light))
        .collect();
    assert!(
        lights.iter().any(|&light| light >= 60),
        "faces beside the lamp are bright"
    );
    assert!(lights.contains(&0), "and far from it none is lit");
    // A face a few voxels from the lamp is lit by exactly the light there.
    assert!(lights.iter().any(|&light| (40..60).contains(&light)));
}

#[test]
fn taking_the_lamp_away_puts_the_light_out_for_everyone() {
    let mut rig = Rig::new();
    let mut builder = player_above_ground(&mut rig);
    let hit = builder.target().unwrap();
    let spot = hit.previous;
    place_block(&mut builder, "base_blocks:lamp");
    run_ticks(&mut rig, 70);
    rig.settle(&mut [&mut builder]);
    assert_eq!(seen_light(&rig, &builder, above(spot, 3)), 61);

    // Break the lamp: it is what the crosshair is on now.
    break_block(&mut builder);
    run_ticks(&mut rig, 2);
    // The light does not go out at once: it fades from the lamp's side a level a tick or two.
    assert!(server_light(&rig, above(spot, 3)) > 0);
    run_ticks(&mut rig, 300);
    rig.settle(&mut [&mut builder]);
    for height in 0..=70 {
        assert_eq!(server_light(&rig, above(spot, height)), 0, "{height}");
        assert_eq!(
            seen_light(&rig, &builder, above(spot, height)),
            0,
            "{height}"
        );
    }
}

#[test]
fn a_world_saved_while_light_is_still_spreading_carries_on_as_if_it_never_stopped() {
    let place_lamp = |rig: &mut Rig| {
        let mut player = player_above_ground(rig);
        place_block(&mut player, "base_blocks:lamp");
        run_ticks(rig, 1);
        player.say_goodbye();
        run_ticks(rig, 1);
    };
    let mut control = Rig::new();
    place_lamp(&mut control);
    run_ticks(&mut control, 150);

    let mut interrupted = Rig::new();
    place_lamp(&mut interrupted);
    run_ticks(&mut interrupted, 6);
    let Rig {
        content, server, ..
    } = interrupted;
    let world = server.into_world();
    let saved = lattice::ActiveVoxels::from_sections(
        world.opaque_sections(),
        &voxunami_cave::lighting::section_id(),
        |name| world.level_index(name),
    )
    .expect("the game saved its light");
    assert!(!saved.is_empty(), "the light was still spreading");
    let bytes = world.save(&content.registries()).unwrap();

    let mut content = CaveContent::new();
    let parsed = world_storage::ParsedWorld::parse(&bytes).unwrap();
    let seed = parsed.world_seed();
    let (loaded, report) = parsed.load(content.load_context(seed)).unwrap();
    assert!(report.extra_channels.is_empty() && report.missing_content.is_empty());
    let mut resumed = Rig::with_world(content, loaded);
    run_ticks(&mut resumed, 144);
    assert_eq!(
        resumed.server.world().main().overrides.content_digest(),
        control.server.world().main().overrides.content_digest(),
        "the light came out the same"
    );
    let settled = resumed.server.into_world();
    assert!(
        lattice::ActiveVoxels::from_sections(
            settled.opaque_sections(),
            &voxunami_cave::lighting::section_id(),
            |name| settled.level_index(name),
        )
        .unwrap()
        .is_empty(),
        "and it settled"
    );
}

/// How many fires the server's world holds in the bricks changed since generation.
fn fires_in(rig: &Rig) -> usize {
    let channel = rig.content.block_channel();
    rig.server
        .world()
        .main()
        .overrides
        .bricks()
        .map(|(_, brick)| {
            voxel_space::LocalVoxelIndex::all()
                .filter(|&local| brick.get(channel, local) == rig.content.blocks.fire)
                .count()
        })
        .sum()
}

/// The block the server's world has at a voxel of the main level, edited or generated.
fn server_block(rig: &Rig, voxel: VoxelCoordinate) -> u32 {
    let (brick, local) = voxel.split();
    match rig.server.world().main().effective_brick(brick) {
        world_storage::BrickSource::Edited(brick) => brick.get(rig.content.block_channel(), local),
        world_storage::BrickSource::Generated(brick) => {
            brick.get(rig.content.block_channel(), local)
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_fire_a_player_lights_gives_light_burns_the_grass_it_stands_on_and_goes_out_for_everyone() {
    let mut rig = Rig::new();
    let mut builder = player_above_ground(&mut rig);
    let mut onlooker = rig.join([20.5, 110.0, 20.5], 0.0, 0.0);
    rig.settle(&mut [&mut builder, &mut onlooker]);
    let hit = builder.target().unwrap();
    let (grass, spot) = (hit.voxel, hit.previous);
    assert_eq!(server_block(&rig, grass), rig.content.blocks.grass);
    let fire = rig.content.blocks.fire;

    place_block(&mut builder, "base_blocks:fire");
    // Also fires in the grass either side of it, so that the grass in the middle is next to five fires
    // and burns whatever the chance does (one fire might, rarely, go out without lighting it).
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let beside = VoxelCoordinate::new(grass.x() + dx, grass.y(), grass.z() + dz);
        request_block(&mut builder, beside, "base_blocks:fire");
    }
    run_ticks(&mut rig, 3);
    assert_eq!(server_block(&rig, spot), fire);
    assert!(
        server_light(&rig, above(spot, 1)) >= u32::from(base_blocks::FIRE_EMISSION) - 3,
        "flames give light"
    );

    run_ticks(&mut rig, 600);
    rig.settle(&mut [&mut builder, &mut onlooker]);
    // The fire spread to the grass it stood on and the grass around it, and every flame has gone
    // out, leaving air where the grass was.
    assert_eq!(fires_in(&rig), 0);
    assert_eq!(server_block(&rig, spot), rig.content.blocks.air);
    assert_eq!(server_block(&rig, grass), rig.content.blocks.air);
    for (name, player) in [("builder", &builder), ("onlooker", &onlooker)] {
        for voxel in [spot, grass] {
            assert_eq!(
                player.mesher().material_at(voxel),
                Some(rig.content.blocks.air),
                "{name}"
            );
        }
    }
    // And the light of the flames went out with them.
    assert_eq!(server_light(&rig, above(spot, 1)), 0);
}

#[test]
fn fires_burn_only_near_players_and_are_frozen_where_nobody_is() {
    let mut rig = Rig::new();
    let mut builder = player_above_ground(&mut rig);
    let spot = builder.target().unwrap().previous;
    place_block(&mut builder, "base_blocks:fire");
    run_ticks(&mut rig, 2);
    builder.say_goodbye();
    run_ticks(&mut rig, 2);
    let before = server_block(&rig, spot);
    assert_eq!(before, rig.content.blocks.fire);
    let state_of = |rig: &Rig| {
        rig.server
            .world()
            .main()
            .overrides
            .get(rig.content.state_channel(), spot)
    };
    let age = state_of(&rig);
    // (How many fires there are by now depends on chance and on how many ticks the setting up took:
    // the fire may have spread to the grass in the two ticks the builder was there.)
    let fires = fires_in(&rig);
    assert!(fires >= 1);

    // Nobody is near: nothing ages or spreads, however long it is left.
    run_ticks(&mut rig, 300);
    assert_eq!(server_block(&rig, spot), rig.content.blocks.fire);
    assert_eq!(state_of(&rig), age);
    assert_eq!(fires_in(&rig), fires);

    // Someone comes within range, and it burns.
    let position = position_of(&builder.camera);
    let mut visitor = rig.join(
        [
            position.brick().x() as f64 * 32.0 + f64::from(position.offset()[0]),
            position.brick().y() as f64 * 32.0 + f64::from(position.offset()[1]),
            position.brick().z() as f64 * 32.0 + f64::from(position.offset()[2]),
        ],
        0.0,
        0.0,
    );
    rig.settle(&mut [&mut visitor]);
    run_ticks(&mut rig, 600);
    assert_eq!(fires_in(&rig), 0);
}

#[test]
fn a_fire_is_frozen_unless_a_player_in_its_level_is_within_range() {
    let (mut rig, second) = two_level_rig();
    let mut builder = player_above_ground(&mut rig);
    let spot = builder.target().unwrap().previous;
    let here = voxels_of(&builder.camera);
    place_block(&mut builder, "base_blocks:fire");
    run_ticks(&mut rig, 2);
    builder.say_goodbye();
    run_ticks(&mut rig, 2);
    let age = |rig: &Rig| {
        rig.server
            .world()
            .main()
            .overrides
            .get(rig.content.state_channel(), spot)
    };
    let frozen_at = age(&rig);

    // A player well beyond the range of a fire, in the same level, does not wake it.
    let far = voxunami_cave::logic::FIRE_RANGE + 40.0;
    let mut distant = rig.join([here[0] + far, here[1], here[2]], 0.0, 0.0);
    rig.settle(&mut [&mut distant]);
    run_ticks(&mut rig, 200);
    assert_eq!(server_block(&rig, spot), rig.content.blocks.fire);
    assert_eq!(age(&rig), frozen_at, "nothing aged");

    // Nor does a player right on top of it, in another level.
    distant.say_goodbye();
    run_ticks(&mut rig, 2);
    // (They join out of range and move into place only once they are in the cellar.)
    let mut elsewhere = rig.join([here[0] + far, here[1], here[2]], 0.0, 0.0);
    rig.settle(&mut [&mut elsewhere]);
    elsewhere.send_command("/level go cellar");
    rig.settle(&mut [&mut elsewhere]);
    assert_eq!(elsewhere.level(), second);
    elsewhere.camera = camera_at(here, 0.0, 0.0);
    elsewhere.teleport_to_camera();
    rig.settle(&mut [&mut elsewhere]);
    run_ticks(&mut rig, 200);
    assert_eq!(age(&rig), frozen_at, "nothing aged");

    // A player just inside the range, in its level, does.
    elsewhere.send_command("/level go overworld");
    rig.settle(&mut [&mut elsewhere]);
    run_ticks(&mut rig, 200);
    assert!(age(&rig) > frozen_at || server_block(&rig, spot) != rig.content.blocks.fire);
}

#[test]
fn a_world_saved_while_it_burns_keeps_the_fire_and_it_goes_on_burning_when_loaded() {
    let mut rig = Rig::new();
    let mut builder = player_above_ground(&mut rig);
    let spot = builder.target().unwrap().previous;
    place_block(&mut builder, "base_blocks:fire");
    run_ticks(&mut rig, 6);
    let position = position_of(&builder.camera);
    let Rig {
        content, server, ..
    } = rig;
    let world = server.into_world();
    let saved = voxunami_cave::fire::Fire::read_section(world.opaque_sections(), |name| {
        world.level_index(name)
    })
    .expect("the game saved its fires");
    assert!(saved.0 >= 6, "the rule's clock was saved: {}", saved.0);
    assert!(
        saved
            .1
            .contains(voxel_space::GridId::root(world_main()), spot),
        "and the fire that was burning"
    );
    let bytes = world.save(&content.registries()).unwrap();

    let mut content = CaveContent::new();
    let parsed = world_storage::ParsedWorld::parse(&bytes).unwrap();
    let seed = parsed.world_seed();
    let (loaded, report) = parsed.load(content.load_context(seed)).unwrap();
    assert!(report.extra_channels.is_empty() && report.missing_content.is_empty());
    let mut resumed = Rig::with_world(content, loaded);
    assert_eq!(server_block(&resumed, spot), resumed.content.blocks.fire);
    let mut visitor = resumed.join(
        [
            position.brick().x() as f64 * 32.0 + f64::from(position.offset()[0]),
            position.brick().y() as f64 * 32.0 + f64::from(position.offset()[1]),
            position.brick().z() as f64 * 32.0 + f64::from(position.offset()[2]),
        ],
        0.0,
        0.0,
    );
    resumed.settle(&mut [&mut visitor]);
    run_ticks(&mut resumed, 600);
    assert_eq!(fires_in(&resumed), 0, "it burned out after being loaded");
}

fn world_main() -> voxel_space::LevelIndex {
    voxel_space::LevelIndex::MAIN
}

// ----- The server moves the players -----

/// Lets the server tick `count` times, the player taking in what it says after each.
fn tick_and_follow(rig: &mut Rig, player: &mut Player, count: usize) {
    for _ in 0..count {
        rig.step();
        player.advance().unwrap();
    }
}

/// A walking player who has fallen to the ground and stands quietly on it, at the column (16, 16).
fn standing_walker(rig: &mut Rig) -> Player {
    let mut player = rig.join_walking([16.5, 130.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    tick_and_follow(rig, &mut player, 120);
    player
}

#[test]
fn a_walking_player_falls_and_stands_on_the_ground_and_stays_there() {
    let mut rig = Rig::new();
    let mut player = rig.join_walking([16.5, 130.0, 16.5], 0.0, 0.0);
    // (The player's own place comes to them as an entity they watch, so the view is asked for, as every frame does.)
    let _ = player.update_view();
    let start = voxels_of(&player.camera)[1];
    // They fall: after a second they are two and a half metres lower (5 metres a second a second).
    tick_and_follow(&mut rig, &mut player, 20);
    let falling = voxels_of(&player.camera)[1];
    assert!(falling < start - 2.0, "{start} {falling}");
    rig.settle(&mut [&mut player]);
    // (The ground is 30 below: 3.5 seconds, 70 ticks.)
    tick_and_follow(&mut rig, &mut player, 200);
    // Standing on the highest solid voxel of their column, eyes a standing player's height above it.
    let ground = ground_height(&player, 16, 16) as f64;
    let standing = voxels_of(&player.camera);
    assert!(
        (standing[1] - (ground + 1.0 + 1.62)).abs() < 0.05,
        "{standing:?} on {ground}"
    );
    assert!((standing[0] - 16.5).abs() < 0.01 && (standing[2] - 16.5).abs() < 0.01);
    // And they stay there, to the bit.
    tick_and_follow(&mut rig, &mut player, 40);
    assert_eq!(voxels_of(&player.camera), standing);
}

#[test]
fn the_keys_held_walk_the_player_the_way_they_face_and_jumping_takes_them_up_and_back_down() {
    let mut rig = Rig::new();
    let mut player = standing_walker(&mut rig);
    let start = voxels_of(&player.camera);

    // Facing -z (yaw 0), W walks that way; a second of it is several voxels.
    player.hold(&[KeyCode::KeyW]);
    tick_and_follow(&mut rig, &mut player, 20);
    let walked = voxels_of(&player.camera);
    assert!(start[2] - walked[2] > 2.5, "{start:?} {walked:?}");
    assert!((walked[0] - start[0]).abs() < 0.01, "straight ahead");

    // Turned a quarter to the right, W walks toward +x.
    player.camera.yaw = -std::f32::consts::FRAC_PI_2;
    player.hold(&[KeyCode::KeyW]);
    tick_and_follow(&mut rig, &mut player, 20);
    assert!(voxels_of(&player.camera)[0] - walked[0] > 2.5);

    // Standing still again, and jumping: up by about a block and a quarter, then back down. One
    // tap between two ticks is enough.
    player.hold(&[]);
    tick_and_follow(&mut rig, &mut player, 40);
    let before = voxels_of(&player.camera)[1];
    player.hold(&[KeyCode::Space]);
    player.hold(&[]);
    let mut highest = before;
    for _ in 0..30 {
        tick_and_follow(&mut rig, &mut player, 1);
        highest = highest.max(voxels_of(&player.camera)[1]);
    }
    assert!(
        (1.1..1.5).contains(&(highest - before)),
        "{}",
        highest - before
    );
    assert!(
        (voxels_of(&player.camera)[1] - before).abs() < 0.05,
        "back on the ground"
    );
}

#[test]
fn a_wall_built_in_the_way_stops_the_player_against_it() {
    let mut rig = Rig::new();
    let mut player = standing_walker(&mut rig);
    let standing = voxels_of(&player.camera);
    let feet = (standing[1] - 1.62).round() as i128;
    // A wall three blocks high, three voxels ahead (toward -z), wider than the player.
    for height in 0..3 {
        for x in 14..=18 {
            request_block(
                &mut player,
                VoxelCoordinate::new(x, feet + height, 13),
                "base_blocks:stone",
            );
        }
    }
    tick_and_follow(&mut rig, &mut player, 4);
    player.hold(&[KeyCode::KeyW]);
    tick_and_follow(&mut rig, &mut player, 90);
    let at_wall = voxels_of(&player.camera);
    // Stopped with their body (0.3 either side of the middle) against the wall's near face at z = 14.
    assert!((at_wall[2] - 14.3).abs() < 0.05, "{at_wall:?}");
    assert!((at_wall[0] - 16.5).abs() < 0.01);
}

#[test]
fn breaking_the_ground_under_a_walker_lets_them_fall() {
    let mut rig = Rig::new();
    let mut player = standing_walker(&mut rig);
    // They look straight down at what they stand on.
    player.camera.look_by(0.0, -std::f32::consts::FRAC_PI_2);
    player.hold(&[]);
    tick_and_follow(&mut rig, &mut player, 3);
    let before = voxels_of(&player.camera)[1];
    break_block(&mut player);
    tick_and_follow(&mut rig, &mut player, 60);
    assert!(
        before - voxels_of(&player.camera)[1] > 0.9,
        "{before} {:?}",
        voxels_of(&player.camera)
    );
}

#[test]
fn flying_hovers_and_the_commands_for_it_and_for_teleporting_answer() {
    let mut rig = Rig::new();
    let mut player = rig.join_walking([16.5, 130.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.take_command_replies();
    player.send_command("/fly");
    tick_and_follow(&mut rig, &mut player, 2);
    let hovering = voxels_of(&player.camera);
    tick_and_follow(&mut rig, &mut player, 60);
    assert_eq!(voxels_of(&player.camera), hovering);
    player.hold(&[KeyCode::Space]);
    tick_and_follow(&mut rig, &mut player, 20);
    assert!(voxels_of(&player.camera)[1] - hovering[1] > 5.0);
    player.hold(&[]);

    player.send_command("/tp 100.5 200 -50.25");
    tick_and_follow(&mut rig, &mut player, 3);
    let moved = voxels_of(&player.camera);
    assert!(
        (moved[0] - 100.5).abs() < 0.01 && (moved[2] + 50.25).abs() < 0.01,
        "{moved:?}"
    );
    assert!((moved[1] - 200.0).abs() < 0.1);
    player.send_command("/tp 1 2");
    player.send_command("/tp a b c");
    // (Beyond the edge of the world, which is 2^120 voxels, about 1.3e36, from the origin.)
    player.send_command("/tp 1 2 9999999999999999999999999999999999999999");
    player.send_command("/fly");
    tick_and_follow(&mut rig, &mut player, 2);
    assert_eq!(
        player.take_command_replies(),
        [
            "you are flying",
            "teleported to 100.5 200 -50.25",
            "usage: /tp, /tp X Y Z, /tp PLACE [LATITUDE LONGITUDE | X Y Z]",
            "usage: /tp X Y Z; X must be a number",
            "that is outside the world",
            "you are walking",
        ]
    );
    // Walking again, they fall.
    tick_and_follow(&mut rig, &mut player, 30);
    assert!(voxels_of(&player.camera)[1] < moved[1] - 3.0);
}

#[test]
fn teleporting_far_beyond_the_watched_region_takes_the_view_along_to_see_the_ground_there() {
    let mut rig = Rig::new();
    let mut player = rig.join_walking([16.5, 130.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.take_command_replies();
    // Far outside the bricks around the camera (the view reaches `VIEW_RADIUS` bricks of 32 voxels).
    let (far_x, far_z) = (2000.5, -1500.5);
    assert!(far_x > (VIEW_RADIUS * 32 * 10) as f64);
    assert!(
        player
            .mesher()
            .is_opaque_at(VoxelCoordinate::new(2000, 0, -1501))
            .is_none()
    );
    player.send_command(&format!("/tp {far_x} 200 {far_z}"));
    // The view needs the camera to be there, and then the bricks around it.
    rig.settle(&mut [&mut player]);
    let moved = voxels_of(&player.camera);
    assert!(
        (moved[0] - far_x).abs() < 0.01 && (moved[2] - far_z).abs() < 0.01,
        "{moved:?}"
    );
    assert_eq!(
        player.take_command_replies(),
        ["teleported to 2000.5 200 -1500.5"]
    );
    // (They fall to the ground, 100 metres or so, which takes 6 seconds on this planet; the view comes down with them.)
    tick_and_follow(&mut rig, &mut player, 200);
    rig.settle(&mut [&mut player]);
    let ground = ground_height(&player, 2000, -1501);
    assert!((20..200).contains(&ground), "{ground}");
}

#[test]
fn a_player_cannot_move_by_saying_where_they_are() {
    let mut rig = Rig::new();
    let mut player = rig.join([16.5, 130.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    tick_and_follow(&mut rig, &mut player, 3);
    let before = voxels_of(&player.camera);
    // A pose, as the other games' players send; a second spawn; and nonsense for a move.
    let far = player_presence::PlayerPose {
        position: camera_at([500.5, 130.0, 500.5], 0.0, 0.0).position,
        yaw: 0.0,
        pitch: 0.0,
    };
    player.send_game_message(
        player_presence::PoseReport::kind(),
        player_presence::PoseReport(far).encode(),
    );
    player.send_game_message(
        voxunami_cave::messages::Spawn::kind(),
        voxunami_cave::messages::Spawn(far).encode(),
    );
    player.send_game_message(
        voxunami_cave::messages::MoveRequest::kind(),
        vec![9, 9, 9, 9],
    );
    tick_and_follow(&mut rig, &mut player, 5);
    assert_eq!(voxels_of(&player.camera), before);
}

#[test]
fn walkers_pass_through_flames_and_the_cellar_has_the_same_ground() {
    let (mut rig, second) = two_level_rig();
    let mut player = standing_walker(&mut rig);
    let standing = voxels_of(&player.camera);
    // Flames on the ground ahead of them do not stop them.
    let feet = (standing[1] - 1.62).round() as i128;
    for z in 13..16 {
        request_block(
            &mut player,
            VoxelCoordinate::new(16, feet, z),
            "base_blocks:fire",
        );
    }
    tick_and_follow(&mut rig, &mut player, 3);
    player.hold(&[KeyCode::KeyW]);
    tick_and_follow(&mut rig, &mut player, 40);
    // (They walk well into the flames. How far depends on what the fire has burnt, which depends on the tick it started
    // at, which depends on how busy the machine was: a trench of burnt ground can stop them short, and that is not
    // the flames stopping them.)
    assert!(standing[2] - voxels_of(&player.camera)[2] > 2.0);
    player.hold(&[]);
    tick_and_follow(&mut rig, &mut player, 40);
    let before = voxels_of(&player.camera);

    // In the cellar they are at the same place, on that level's ground. (Not at the same height as
    // before: the fire may have burnt the ground away in the main level, and the cellar's is whole.)
    player.send_command("/level go cellar");
    rig.settle(&mut [&mut player]);
    tick_and_follow(&mut rig, &mut player, 60);
    assert_eq!(player.level(), second);
    rig.settle(&mut [&mut player]);
    let in_the_cellar = voxels_of(&player.camera);
    assert!(
        (in_the_cellar[0] - before[0]).abs() < 1.0 && (in_the_cellar[2] - before[2]).abs() < 1.0,
        "{in_the_cellar:?} {before:?}"
    );
    assert_standing(&player, "in the cellar");
}

/// Whether anything solid is where the player's body is (0.6 by 1.8, the feet 1.62 below the eyes), by more than
/// a hair, as the player's picture of the terrain shows it.
fn is_inside_solid(player: &Player) -> Vec<(i128, i128, i128)> {
    let eyes = voxels_of(&player.camera);
    let feet = eyes[1] - 1.62;
    let inside = |centre: f64| (centre - 0.28).floor() as i128..=(centre + 0.28).floor() as i128;
    let mut solid = Vec::new();
    for x in inside(eyes[0]) {
        for z in inside(eyes[2]) {
            for y in (feet + 0.02).floor() as i128..=(feet + 1.78).floor() as i128 {
                if player.mesher().is_opaque_at(VoxelCoordinate::new(x, y, z)) == Some(true) {
                    solid.push((x, y, z));
                }
            }
        }
    }
    solid
}

/// The player is not inside anything solid and stands on ground, as the player's picture of the terrain shows it:
/// the feet are at the top of some column the body overlaps, or is within the controller's skin of (0.3 and a
/// hair each way), and not lower than the top of any column it overlaps by more than the hair the physics
/// lets a body into a block (0.02, so 0.28 each way).
fn assert_standing(player: &Player, label: &str) {
    let eyes = voxels_of(&player.camera);
    assert_eq!(is_inside_solid(player), [], "{label}: eyes at {eyes:?}");
    let tops = |reach: f64| -> Vec<f64> {
        let columns = |centre: f64| {
            [
                (centre - reach).floor() as i128,
                (centre + reach).floor() as i128,
            ]
        };
        columns(eyes[0])
            .into_iter()
            .flat_map(|x| columns(eyes[2]).map(move |z| (x, z)))
            .map(|(x, z)| ground_height(player, x, z) as f64 + 1.0)
            .collect()
    };
    let feet = eyes[1] - 1.62;
    let highest = tops(0.28).into_iter().fold(f64::MIN, f64::max);
    assert!(
        feet > highest - 0.05,
        "{label}: sunk into ground that reaches {highest}: eyes at {eyes:?}"
    );
    assert!(
        tops(0.311).iter().any(|top| (feet - top).abs() < 0.05),
        "{label}: standing on nothing: eyes at {eyes:?}, column tops {:?}",
        tops(0.311)
    );
}

#[test]
fn a_player_teleported_into_the_terrain_is_moved_out_of_it_and_never_left_inside_it() {
    // Positions on the hills where, one voxel inside the tallest column the body overlaps, the physics used to
    // push the body some way sideways and leave it half in a block.
    let mut rig = Rig::new();
    let mut player = standing_walker(&mut rig);
    let columns = |centre: f64| {
        [
            (centre - 0.28).floor() as i128,
            (centre + 0.28).floor() as i128,
        ]
    };
    for (x, z) in [
        (10.20, 6.10),
        (11.68, 6.51),
        (12.79, 6.92),
        (13.53, 8.56),
        (15.38, 8.56),
        (10.94, 9.38),
        (10.57, 11.43),
        (15.38, 11.84),
        (15.75, 12.25),
        (12.79, 12.25),
    ] {
        let tallest = columns(x)
            .into_iter()
            .flat_map(|cx| columns(z).map(move |cz| (cx, cz)))
            .map(|(cx, cz)| ground_height(&player, cx, cz))
            .max()
            .unwrap() as f64;
        // The feet one voxel inside the highest ground the body overlaps.
        player.send_command(&format!("/tp {x} {} {z}", tallest + 1.62));
        tick_and_follow(&mut rig, &mut player, 5);
        rig.settle(&mut [&mut player]);
        tick_and_follow(&mut rig, &mut player, 30);
        let eyes = voxels_of(&player.camera);
        assert_standing(&player, &format!("({x}, {z})"));
        // The shortest way out is sideways, off a taller column, or up onto it: never far.
        assert!(
            (eyes[0] - x).abs() < 1.0
                && (eyes[2] - z).abs() < 1.0
                && eyes[1] < tallest + 1.62 + 1.0,
            "({x}, {z}): moved far: eyes at {eyes:?}"
        );
    }
}

#[test]
fn tp_into_the_ground_says_where_the_player_was_moved_to_and_that_is_where_they_are() {
    let mut rig = Rig::new();
    let mut player = standing_walker(&mut rig);
    let ground = ground_height(&player, 16, 16) as f64;
    // The feet one voxel inside the ground; and then somewhere in the open, which is not moved.
    player.take_command_replies();
    player.send_command(&format!("/tp 16.5 {} 16.5", ground + 1.62));
    tick_and_follow(&mut rig, &mut player, 3);
    let replies = player.take_command_replies();
    let [reply] = &replies[..] else {
        panic!("{replies:?}");
    };
    let numbers: Vec<f64> = reply
        .strip_prefix("teleported to ")
        .unwrap_or_else(|| panic!("{reply}"))
        .split([' ', '(', ','])
        .filter_map(|word| word.parse().ok())
        .collect();
    // Where they are now, then (after "moved from") where they asked to be.
    let [now_x, now_y, now_z, asked_x, asked_y, asked_z] = numbers[..] else {
        panic!("{reply}");
    };
    assert!(
        reply.contains(&format!("moved from 16.5 {} 16.5", ground + 1.62)),
        "{reply}"
    );
    assert_eq!([asked_x, asked_z], [16.5, 16.5]);
    assert!((asked_y - (ground + 1.62)).abs() < 1e-9);
    assert!(
        now_y > asked_y + 0.5,
        "moved out of the ground, up: {reply}"
    );
    // And that is where the player is.
    let eyes = voxels_of(&player.camera);
    assert!(
        (eyes[0] - now_x).abs() < 0.02
            && (eyes[1] - now_y).abs() < 0.02
            && (eyes[2] - now_z).abs() < 0.02,
        "told {reply}, but eyes at {eyes:?}"
    );
    assert_standing(&player, "after the teleport");

    // In the open, the reply is the one it always was.
    player.send_command(&format!("/tp 16.5 {} 16.5", ground + 20.0));
    tick_and_follow(&mut rig, &mut player, 1);
    assert_eq!(
        player.take_command_replies(),
        [format!("teleported to 16.5 {} 16.5", ground + 20.0)]
    );
}

#[test]
fn a_player_in_a_trench_sent_to_a_level_whose_ground_is_whole_ends_up_on_it_not_in_it() {
    let (mut rig, second) = two_level_rig();
    let mut player = standing_walker(&mut rig);
    // Two columns of the ground dug out ahead of them: a trench they walk into and cannot climb out of (as
    // the fire digs one, by chance, in the test of walking through flames).
    // (Four wide: a walker at 4.3 metres a second takes 2.7 metres to fall one on this planet, and would cross a trench of two.)
    for z in [15, 14, 13, 12] {
        let ground = ground_height(&player, 16, z);
        request_block(
            &mut player,
            VoxelCoordinate::new(16, ground, z),
            "base_blocks:air",
        );
    }
    tick_and_follow(&mut rig, &mut player, 3);
    player.hold(&[KeyCode::KeyW]);
    tick_and_follow(&mut rig, &mut player, 40);
    player.hold(&[]);
    tick_and_follow(&mut rig, &mut player, 40);
    let in_the_trench = voxels_of(&player.camera);
    assert!(
        (12.0..12.7).contains(&in_the_trench[2]),
        "the walker stopped at the end of the trench: {in_the_trench:?}"
    );

    // The cellar has its ground whole there, a voxel above their feet: they must end up on it, not in it.
    player.send_command("/level go cellar");
    rig.settle(&mut [&mut player]);
    tick_and_follow(&mut rig, &mut player, 60);
    assert_eq!(player.level(), second);
    rig.settle(&mut [&mut player]);
    assert_standing(&player, "in the cellar");
}

/// A position, bit for bit, as a number.
fn position_hash(camera: &renderer::Camera) -> u64 {
    let position = position_of(camera);
    let mut state: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |value: u64| {
        for byte in value.to_le_bytes() {
            state ^= u64::from(byte);
            state = state.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    feed(position.brick().x() as u64);
    feed(position.brick().y() as u64);
    feed(position.brick().z() as u64);
    for component in position.offset() {
        feed(u64::from(component.to_bits()));
    }
    state
}

/// A scripted walk over the hills of the world: falling, walking in several directions, jumping,
/// running. Returns where the player is after each stage, as numbers.
fn scripted_walk() -> Vec<u64> {
    let mut rig = Rig::new();
    let mut player = rig.join_walking([40.5, 150.0, -20.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    let script: [(&[KeyCode], f32, usize); 7] = [
        (&[], 0.0, 150),
        (&[KeyCode::KeyW], 0.3, 60),
        (&[KeyCode::KeyW, KeyCode::Space], 0.3, 40),
        (&[KeyCode::KeyW, KeyCode::KeyD], -1.2, 80),
        (&[KeyCode::KeyW, KeyCode::ControlLeft], 2.0, 100),
        (&[KeyCode::KeyS, KeyCode::KeyA], 2.0, 60),
        (
            &[KeyCode::KeyW, KeyCode::ControlLeft, KeyCode::Space],
            0.0,
            120,
        ),
    ];
    let mut hashes = Vec::new();
    for (keys, yaw, ticks) in script {
        player.camera.yaw = yaw;
        player.hold(keys);
        tick_and_follow(&mut rig, &mut player, ticks);
        hashes.push(position_hash(&player.camera));
    }
    hashes
}

#[test]
fn a_scripted_walk_over_the_hills_ends_in_the_same_place_every_time_and_on_every_machine() {
    let first = scripted_walk();
    assert_eq!(first, scripted_walk());
    // Taken on Windows; the same numbers come out on Linux, so the two agree to the bit.
    assert_eq!(first, PINNED_WALK, "{first:?}");
}

/// (Moved once, on purpose, 2026-10-22, all but the first and the fifth: the pull of the planet is 5 metres a second a second and a jump 3.6 metres a second, was 32 and 9. The first
/// and the fifth, where the player stands still at the end of a stage on the ground, are the numbers they were.)
const PINNED_WALK: [u64; 7] = [
    468_518_576_783_366_090,
    3_621_015_791_496_737_235,
    15_049_547_940_405_347_413,
    17_907_881_989_766_520_762,
    2_040_764_607_994_219_951,
    3_385_291_536_655_073_884,
    10_975_456_705_509_759_913,
];

#[test]
fn a_walker_standing_in_flames_falls_when_the_fire_burns_the_ground_away() {
    let mut rig = Rig::new();
    let mut player = standing_walker(&mut rig);
    let standing = voxels_of(&player.camera);
    let grass = VoxelCoordinate::new(16, ground_height(&player, 16, 16), 16);
    assert_eq!(server_block(&rig, grass), rig.content.blocks.grass);
    // Fire where they stand and in the grass either side, so that their grass burns whatever the
    // chance does.
    for (dx, dz) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
        let at = VoxelCoordinate::new(grass.x() + dx, grass.y() + 1, grass.z() + dz);
        request_block(&mut player, at, "base_blocks:fire");
    }
    tick_and_follow(&mut rig, &mut player, 2);
    // The fire burns grass by chance, from the tick it is on, and that tick is not the same in every run (the
    // loading before this takes a different number of ticks on a busy machine), so the grass may already be
    // gone. While it is there, the flames do not hold them up or push them; once it has burnt they have begun
    // to fall (one tick of it is 0.05 voxel), and the flames still have not pushed them sideways or up.
    // (The ground is made again for the new blocks, which may settle them by a hair.)
    let now = voxels_of(&player.camera);
    let burnt = server_block(&rig, grass) != rig.content.blocks.grass;
    let sideways = (now[0] - standing[0])
        .abs()
        .max((now[2] - standing[2]).abs());
    if burnt {
        assert!(
            sideways < 0.001 && now[1] <= standing[1] + 0.001,
            "the ground burnt and they fell, not up or sideways: stood at {standing:?}, then at {now:?}"
        );
    } else {
        assert!(
            sideways < 0.001 && (now[1] - standing[1]).abs() < 0.001,
            "flames do not hold them up or push them: stood at {standing:?}, then at {now:?}"
        );
    }
    tick_and_follow(&mut rig, &mut player, 600);
    assert_eq!(server_block(&rig, grass), rig.content.blocks.air);
    let fallen = standing[1] - voxels_of(&player.camera)[1];
    assert!((fallen - 1.0).abs() < 0.05, "{fallen}");
}
