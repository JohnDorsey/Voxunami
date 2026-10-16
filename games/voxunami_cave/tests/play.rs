//! The game, played through its server and sessions, without a window.

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use client::ClientError;
use common::{Player, Rig, SEED, VIEW_RADIUS, above_ground, ground_height, position_of};
use game_shell::files::{open_world, record_connection, save_world};
use game_shell::host::{HostSettings, ServerThread};
use game_shell::session::{Mode, Session};
use game_shell::view::camera_at;
use game_shell::{GameContent, GameDefinition, KeyCode};
use transport::{Connection, ConnectionClosed};
use voxel_space::VoxelCoordinate;
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

/// A world of two levels with the same terrain, so that what differs between them is only what
/// players do. (Levels of one world share its seed; they would differ by terrain generator version.)
fn two_level_rig() -> (Rig, voxel_space::LevelIndex) {
    let content = CaveContent::new();
    let mut world = content.new_world(SEED);
    let second = world
        .add_level(
            "voxunami_cave:cellar".parse().unwrap(),
            content.new_world(SEED).into_main(),
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

    // Players in different levels do not see each other, though both are known to the server.
    assert_eq!(mover.players().len(), 2);
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
        Mode::Replaying,
        VIEW_RADIUS,
        camera_at([0.0; 3], 0.0, 0.0, VIEW_RADIUS),
        || {},
    )
    .unwrap();
    while replay.advance_one_tick().is_ok() {}
    replay.settle(|| {}).unwrap();
    assert_eq!(replay.client().world().summary(), live_summary);
    assert_eq!(replay.level(), second);
    assert_eq!(replay.mesher().is_opaque_at(hit.voxel), Some(false));
    assert_eq!(
        replay.take_command_replies(),
        ["you are now in voxunami_cave:cellar"]
    );
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn commands_the_game_does_not_know_are_answered() {
    let mut rig = Rig::new();
    let mut player = player_above_ground(&mut rig);
    player.send_command("/fly");
    player.send_command("/level");
    rig.settle(&mut [&mut player]);
    assert_eq!(
        player.take_command_replies(),
        [
            "unknown command; try /level",
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
        own.brick().x() * 32 + own.offset()[0] as i64,
        own.brick().y() * 32 + own.offset()[1] as i64 - 3,
        own.brick().z() * 32 + own.offset()[2] as i64,
    );
    let shapes = base_blocks::SHAPE_NAMES.len() as u32;

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
    assert_eq!(Cave::status_text(&player), "placing stone (whole)");

    Cave::key_pressed(&mut player, KeyCode::KeyR);
    assert_eq!(Cave::status_text(&player), "placing stone (bottom slab)");
    Cave::secondary_action(&mut player);
    rig.settle(&mut [&mut player]);
    assert_eq!(
        server_block_and_state(&rig, hit.previous),
        (rig.content.blocks.stone, 1)
    );

    // Choosing another block keeps the shape; pressing the key enough times comes back to whole.
    Cave::key_pressed(&mut player, KeyCode::Digit3);
    assert_eq!(Cave::status_text(&player), "placing grass (bottom slab)");
    Cave::key_pressed(&mut player, KeyCode::Digit4);
    assert_eq!(Cave::status_text(&player), "placing lamp (bottom slab)");
    Cave::key_pressed(&mut player, KeyCode::Digit3);
    for _ in 1..base_blocks::SHAPE_NAMES.len() {
        Cave::key_pressed(&mut player, KeyCode::KeyR);
    }
    assert_eq!(Cave::status_text(&player), "placing grass (whole)");
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

fn above(voxel: VoxelCoordinate, height: i64) -> VoxelCoordinate {
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

/// A position in voxels, as a player is joined at, taken from a camera.
fn voxels_of(camera: &renderer::Camera) -> [f64; 3] {
    let position = position_of(camera);
    [
        position.brick().x() as f64 * 32.0 + f64::from(position.offset()[0]),
        position.brick().y() as f64 * 32.0 + f64::from(position.offset()[1]),
        position.brick().z() as f64 * 32.0 + f64::from(position.offset()[2]),
    ]
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
    elsewhere.camera = camera_at(here, 0.0, 0.0, VIEW_RADIUS);
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
        saved.1.contains(world_main(), spot),
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
