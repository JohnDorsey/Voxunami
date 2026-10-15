//! The game, played through its server and sessions, without a window.

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use client::ClientError;
use common::{Player, Rig, SEED, VIEW_RADIUS, above_ground, ground_height, position_of};
use game_shell::GameContent;
use game_shell::files::{open_world, record_connection, save_world};
use game_shell::host::{HostSettings, ServerThread};
use game_shell::session::{Mode, Session};
use game_shell::view::camera_at;
use transport::{Connection, ConnectionClosed};
use voxel_space::VoxelCoordinate;
use voxunami_cave::content::CaveContent;
use voxunami_cave::game::{Cave, break_block, place_block, request_block};
use voxunami_cave::logic::REACH;

/// A player standing 3 voxels above the ground at the column (16, 16), looking straight down.
fn player_above_ground(rig: &mut Rig) -> Player {
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.camera = above_ground(&player, 16, 16, 3.0);
    rig.settle(&mut [&mut player]);
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
    assert_eq!(rig.server.world().overrides.brick_count(), 1);
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
        own.brick().x() * 32 + own.offset()[0] as i64,
        own.brick().y() * 32 + own.offset()[1] as i64 - 3,
        own.brick().z() * 32 + own.offset()[2] as i64,
    );

    // Too far, a block nobody has heard of, and (from a second player who never said where they
    // are) a perfectly reasonable request: none changes the world.
    request_block(
        &mut player,
        VoxelCoordinate::new(near.x() + (REACH as i64) + 5, near.y(), near.z()),
        "base_blocks:stone",
    );
    request_block(&mut player, near, "nonexistent:block");
    let mut stranger = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    request_block(&mut stranger, near, "base_blocks:stone");
    for _ in 0..3 {
        rig.step();
    }
    assert_eq!(rig.server.world().overrides.brick_count(), 0);

    // The same request from the player who is in reach works.
    request_block(&mut player, near, "base_blocks:stone");
    for _ in 0..2 {
        rig.step();
    }
    assert_eq!(rig.server.world().overrides.brick_count(), 1);
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
        let poses: Vec<_> = player.players().values().collect();
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

    second.say_goodbye();
    drop(second);
    rig.step();
    rig.step();
    first.advance().unwrap();
    assert_eq!(first.players().len(), 1);
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
        Mode::Replaying,
        VIEW_RADIUS,
        camera_at([0.0; 3], 0.0, 0.0, VIEW_RADIUS),
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
        Mode::Replaying,
        VIEW_RADIUS,
        player.camera,
        || {
            server.step();
        },
    )
    .unwrap();
    let after_hello = sent.lock().unwrap().len();
    assert_eq!(after_hello, 1, "only the hello");

    // Whatever the watcher's camera is doing, it moves, aims and clicks without telling anyone.
    watcher.camera = player.camera;
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
    let world = content.new_world(SEED);
    let host =
        ServerThread::<Cave>::start(content, world, HostSettings::local(VIEW_RADIUS)).unwrap();
    let connection = host.connect().unwrap();
    let start = camera_at([16.5, 110.0, 16.5], 0.0, 0.0, VIEW_RADIUS);
    let mut player = Session::<_, Cave>::connect(
        Box::new(connection) as Box<dyn transport::Connection>,
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
    let (world, _content) = host.stop();
    assert_eq!(world.world_seed, SEED);
}
