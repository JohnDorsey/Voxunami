//! Voxunami Eels, played through its server and sessions, without a window.

use game_shell::files::record_connection;
use game_shell::host::build_server;
use game_shell::session::{Mode, Session};
use game_shell::view::camera_at;
use game_shell::{GameContent, GameDefinition, KeyCode};
use player_presence::{PlayerPose, PoseReport};
use protocol::Message;
use server::Server;
use transport::Connection;
use transport::loopback::LoopbackConnector;
use voxel_space::VoxelCoordinate;
use voxunami_eels::arena::ARENA_EDGE;
use voxunami_eels::content::{ContentEels, blocks};
use voxunami_eels::eel::Steer;
use voxunami_eels::game::{Eels, steer};
use voxunami_eels::round::{
    COUNTDOWN_TICKS, OVER_TICKS, Phase, STEP_TICKS, TICKS_PER_SECOND, start_of,
};

const VIEW_RADIUS: i64 = 3;

type Player = Session<Box<dyn Connection>, Eels>;

struct Rig {
    content: ContentEels,
    server: Server,
    connector: LoopbackConnector,
}

impl Rig {
    fn new() -> Self {
        let content = ContentEels::new();
        let world = content.new_world(1);
        let (server, connector) = build_server::<Eels>(&content, world, VIEW_RADIUS).unwrap();
        Self {
            content,
            server,
            connector,
        }
    }

    fn join(&mut self) -> Player {
        let connection: Box<dyn Connection> = Box::new(self.connector.connect().unwrap());
        self.join_with(connection)
    }

    fn join_with(&mut self, connection: Box<dyn Connection>) -> Player {
        let start = camera_at([32.5, 40.0, 32.5], 0.0, 0.0, VIEW_RADIUS);
        let server = &mut self.server;
        Session::<_, Eels>::connect(connection, Mode::Playing, VIEW_RADIUS, start, || {
            server.step();
        })
        .unwrap()
    }

    /// Ticks the server `count` times, every player taking in what it says after each.
    fn tick(&mut self, players: &mut [&mut Player], count: u32) {
        for _ in 0..count {
            self.server.step();
            for player in players.iter_mut() {
                player.advance().unwrap();
                let _ = player.update_view();
            }
        }
    }

    /// Ticks until the round runs.
    fn start_round(&mut self, players: &mut [&mut Player]) {
        self.tick(players, COUNTDOWN_TICKS + 3);
        for player in players.iter() {
            assert_eq!(player.state.standings.phase, Phase::Running);
        }
    }

    fn steps(&mut self, players: &mut [&mut Player], count: u32) {
        self.tick(players, count * STEP_TICKS);
    }

    /// Gives the server a few ticks to send the world around the players, so that they can see it.
    /// (The round's countdown is far longer.)
    fn load(&mut self, players: &mut [&mut Player]) {
        self.tick(players, 6);
        self.settle(players);
    }

    /// Lets every player mesh what they have been sent, without ticking the server: what happens
    /// in the match is only what the test ticks.
    fn settle(&mut self, players: &mut [&mut Player]) {
        for player in players.iter_mut() {
            player.settle(|| {}).unwrap();
        }
    }
}

fn voxels_of(camera: &renderer::Camera) -> [f64; 3] {
    let position = camera.position;
    std::array::from_fn(|axis| {
        [
            position.brick().x(),
            position.brick().y(),
            position.brick().z(),
        ][axis] as f64
            * 32.0
            + f64::from(position.offset()[axis])
    })
}

fn at(cell: [i64; 3]) -> VoxelCoordinate {
    VoxelCoordinate::new(cell[0], cell[1], cell[2])
}

fn block_and_colour(player: &Player, cell: [i64; 3]) -> (Option<u32>, Option<u32>) {
    (
        player.mesher().material_at(at(cell)),
        player.mesher().state_at(at(cell)),
    )
}

#[test]
fn the_server_runs_at_the_games_tick_rate() {
    let mut rig = Rig::new();
    let player = rig.join();
    assert_eq!(
        player.client().world().server_info().ticks_per_second,
        TICKS_PER_SECOND
    );
    assert_eq!(<Eels as GameDefinition>::ticks_per_second(), 30);
}

#[test]
fn a_player_joining_gets_a_colour_an_eel_and_a_countdown_then_swims_with_the_camera_behind_it() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    rig.load(&mut [&mut player]);
    let standings = &player.state.standings;
    assert_eq!(standings.players.len(), 1);
    assert_eq!(
        (standings.players[0].slot, standings.players[0].alive),
        (0, true)
    );
    assert!(matches!(standings.phase, Phase::Countdown { .. }));

    // The camera is behind and above the eel (which starts heading +z), looking along it.
    let (head, _) = start_of(0);
    let camera = voxels_of(&player.camera);
    assert!(
        (camera[0] - (head[0] as f64 + 0.5)).abs() < 1e-3,
        "{camera:?}"
    );
    assert!(
        (camera[1] - (head[1] as f64 + 3.0)).abs() < 1e-3,
        "{camera:?}"
    );
    assert!(
        (camera[2] - (head[2] as f64 - 5.5)).abs() < 1e-3,
        "{camera:?}"
    );
    assert!((player.camera.yaw - std::f32::consts::PI).abs() < 1e-4);
    assert!(player.camera.pitch < 0.0);

    // The eel's head is in the world, in its colour, and the wall is round the arena.
    for x in head[0] - 1..=head[0] + 1 {
        assert_eq!(
            block_and_colour(&player, [x, head[1], head[2]]),
            (Some(blocks::HEAD), Some(0))
        );
    }
    let last = ARENA_EDGE;
    for outside in [
        [-1, 10, 10],
        [last, 10, 10],
        [10, -1, 10],
        [10, last, 10],
        [10, 10, -1],
        [10, 10, last],
    ] {
        let (block, _) = block_and_colour(&player, outside);
        assert!(
            matches!(block, Some(blocks::WALL | blocks::WALL_LINE)),
            "{outside:?}: {block:?}"
        );
    }
    assert_eq!(block_and_colour(&player, [10, 10, 10]).0, Some(blocks::AIR));

    // Once the round runs, the eel moves a cell every few ticks and the camera moves with it.
    rig.start_round(&mut [&mut player]);
    // The camera glides between steps, a share of a cell on every tick, and not in jumps.
    let mut last = voxels_of(&player.camera)[2];
    let mut moved = Vec::new();
    for _ in 0..6 {
        rig.tick(&mut [&mut player], 1);
        let now = voxels_of(&player.camera)[2];
        moved.push(now - last);
        last = now;
    }
    assert!(
        moved
            .iter()
            .all(|&distance| (distance - 1.0 / 3.0).abs() < 1e-3),
        "{moved:?}"
    );
    let before = voxels_of(&player.camera);
    rig.steps(&mut [&mut player], 10);
    let after = voxels_of(&player.camera);
    assert!(
        (after[2] - before[2] - 10.0).abs() < 0.5,
        "{before:?} {after:?}"
    );
    assert!((after[0] - before[0]).abs() < 1e-3);
    // Everyone sees the body it has laid, in its colour, a head at the front.
    rig.settle(&mut [&mut player]);
    let tail = [head[0], head[1], head[2] + 4];
    assert_eq!(
        block_and_colour(&player, tail),
        (Some(blocks::TRAIL), Some(0))
    );
}

#[test]
fn steering_turns_the_eel_and_the_view_and_a_roll_changes_the_wall_it_leaves() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    rig.start_round(&mut [&mut player]);
    let (head, _) = start_of(0);
    rig.steps(&mut [&mut player], 4);
    // Turn left (the eel heads +z, so left is +x) and roll.
    steer(&mut player, Steer::Left);
    steer(&mut player, Steer::Roll);
    rig.steps(&mut [&mut player], 10);
    // The view has turned to look along +x: yaw -90 degrees, whichever way round.
    let turned =
        (player.camera.yaw + std::f32::consts::FRAC_PI_2).rem_euclid(std::f32::consts::TAU);
    assert!(
        turned < 0.05 || turned > std::f32::consts::TAU - 0.05,
        "{}",
        player.camera.yaw
    );
    rig.settle(&mut [&mut player]);
    // After the turn the body runs along +x, standing on its edge: three cells tall.
    let along = [head[0] + 8, head[1], head[2] + 4];
    for dy in -1..=1 {
        assert_eq!(
            block_and_colour(&player, [along[0], along[1] + dy, along[2]]).0,
            Some(blocks::TRAIL),
            "dy {dy}"
        );
    }
    assert_eq!(
        block_and_colour(&player, [along[0], along[1] + 2, along[2]]).0,
        Some(blocks::AIR)
    );
    // The key presses are the controls: A turns left, and a key that is not one does nothing.
    <Eels as GameDefinition>::key_pressed(&mut player, KeyCode::KeyX);
    <Eels as GameDefinition>::key_pressed(&mut player, KeyCode::KeyA);
    rig.steps(&mut [&mut player], 2);
    let status = <Eels as GameDefinition>::status_text(&player);
    assert!(
        status.starts_with("red eel | wins 0 | swimming"),
        "{status}"
    );
}

#[test]
fn a_lone_eel_dies_on_the_wall_and_a_new_round_clears_the_arena_for_everyone() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    rig.start_round(&mut [&mut player]);
    let (head, _) = start_of(0);
    rig.steps(&mut [&mut player], (ARENA_EDGE - head[2]) as u32 + 2);
    assert!(matches!(
        player.state.standings.phase,
        Phase::Over { winner: None, .. }
    ));
    assert!(!player.state.standings.players[0].alive);
    let status = <Eels as GameDefinition>::status_text(&player);
    assert!(
        status.contains("(out)") && status.contains("nobody wins"),
        "{status}"
    );
    // Its body is still there while the round is shown.
    rig.settle(&mut [&mut player]);
    let body = [head[0], head[1], head[2] + 10];
    assert_eq!(block_and_colour(&player, body).0, Some(blocks::TRAIL));
    // Some seconds on, the arena is clear and the eel is back where it started.
    rig.tick(&mut [&mut player], OVER_TICKS + 2);
    rig.settle(&mut [&mut player]);
    println!("DBG {:?}", player.state.standings);
    assert!(matches!(
        player.state.standings.phase,
        Phase::Countdown { .. }
    ));
    assert_eq!(block_and_colour(&player, body).0, Some(blocks::AIR));
    assert_eq!(
        block_and_colour(&player, [head[0], head[1], head[2]]).0,
        Some(blocks::HEAD)
    );
    assert!(player.state.standings.players[0].alive);
}

#[test]
fn two_players_have_two_colours_see_each_others_bodies_and_the_survivor_wins() {
    let mut rig = Rig::new();
    let mut first = rig.join();
    let mut second = rig.join();
    rig.tick(&mut [&mut first, &mut second], 3);
    let slots: Vec<u8> = first
        .state
        .standings
        .players
        .iter()
        .map(|p| p.slot)
        .collect();
    assert_eq!(slots, [0, 1]);
    assert_eq!(
        first.state.standings.players,
        second.state.standings.players
    );
    // (A player is told about themselves by their own client id.)
    assert!(
        <Eels as GameDefinition>::status_text(&first).starts_with("red eel"),
        "{}",
        <Eels as GameDefinition>::status_text(&first)
    );
    assert!(<Eels as GameDefinition>::status_text(&second).starts_with("orange eel"));

    rig.start_round(&mut [&mut first, &mut second]);
    // The first eel heads for the nearest wall and dies there; the second swims on.
    steer(&mut first, Steer::Left);
    steer(&mut first, Steer::Left);
    rig.steps(&mut [&mut first, &mut second], 15);
    assert!(matches!(
        first.state.standings.phase,
        Phase::Over {
            winner: Some(1),
            ..
        }
    ));
    assert_eq!(first.state.standings.players[1].wins, 1);
    assert!(<Eels as GameDefinition>::status_text(&second).contains("orange eel wins the round"));
    // Each sees the other's body, in the other's colour.
    rig.settle(&mut [&mut first, &mut second]);
    let (second_head, _) = start_of(1);
    let (first_head, _) = start_of(0);
    for player in [&first, &second] {
        assert_eq!(
            block_and_colour(player, [second_head[0] - 3, second_head[1], second_head[2]]),
            (Some(blocks::TRAIL), Some(1))
        );
        assert_eq!(
            block_and_colour(player, [first_head[0], first_head[1], first_head[2] + 1]),
            (Some(blocks::TRAIL), Some(0))
        );
    }
}

#[test]
fn a_player_who_leaves_loses_their_eel_and_the_others_carry_on() {
    let mut rig = Rig::new();
    let mut first = rig.join();
    let mut second = rig.join();
    rig.start_round(&mut [&mut first, &mut second]);
    rig.steps(&mut [&mut first, &mut second], 3);
    first.say_goodbye();
    drop(first);
    rig.tick(&mut [&mut second], 2);
    assert_eq!(second.state.standings.players.len(), 1);
    assert_eq!(second.state.standings.players[0].slot, 1);
    assert!(matches!(
        second.state.standings.phase,
        Phase::Over {
            winner: Some(1),
            ..
        }
    ));
    // A newcomer takes the free colour, and the next round has both.
    let mut third = rig.join();
    rig.tick(&mut [&mut second, &mut third], OVER_TICKS + 3);
    let mut slots: Vec<u8> = second
        .state
        .standings
        .players
        .iter()
        .map(|p| p.slot)
        .collect();
    slots.sort();
    assert_eq!(slots, [0, 1], "the newcomer took the free colour");
    assert!(matches!(
        second.state.standings.phase,
        Phase::Countdown { .. }
    ));
    assert!(
        second
            .state
            .standings
            .players
            .iter()
            .all(|player| player.alive)
    );
}

#[test]
fn a_player_cannot_move_their_camera_by_saying_where_it_is() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    rig.tick(&mut [&mut player], 3);
    let before = voxels_of(&player.camera);
    let far = PlayerPose {
        position: camera_at([500.5, 40.0, 500.5], 0.0, 0.0, VIEW_RADIUS).position,
        yaw: 1.0,
        pitch: 0.3,
    };
    player.send_game_message(PoseReport::kind(), PoseReport(far).encode());
    rig.tick(&mut [&mut player], 3);
    // Nothing but the eel's own movement (the countdown has the eel still).
    assert_eq!(voxels_of(&player.camera), before);
}

#[test]
fn a_world_saved_with_bodies_in_it_loads_and_the_first_round_clears_them() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    rig.start_round(&mut [&mut player]);
    rig.steps(&mut [&mut player], 8);
    rig.settle(&mut [&mut player]);
    let (head, _) = start_of(0);
    let body = [head[0], head[1], head[2] + 3];
    assert_eq!(block_and_colour(&player, body).0, Some(blocks::TRAIL));
    // Save and load, as a world file would.
    let bytes = rig.server.world().save(&rig.content.registries()).unwrap();
    let mut content = ContentEels::new();
    let parsed = world_storage::ParsedWorld::parse(&bytes).unwrap();
    let seed = parsed.world_seed();
    let (world, report) = parsed.load(content.load_context(seed)).unwrap();
    assert!(report.missing_content.is_empty());
    let (server, connector) = build_server::<Eels>(&content, world, VIEW_RADIUS).unwrap();
    let mut later = Rig {
        content,
        server,
        connector,
    };
    let mut again = later.join();
    later.tick(&mut [&mut again], 3);
    later.settle(&mut [&mut again]);
    // The body from before is gone: the round that began when the player joined cleared it, and
    // the new eel is at its start.
    assert_eq!(block_and_colour(&again, body).0, Some(blocks::AIR));
    assert_eq!(block_and_colour(&again, head).0, Some(blocks::HEAD));
}

#[test]
fn a_recording_replays_the_match_from_the_recorded_players_eel() {
    let directory =
        std::env::temp_dir().join(format!("voxunami_eels_replay_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("session.voxreplay");

    let mut rig = Rig::new();
    let connection = rig.connector.connect().unwrap();
    let recorded = record_connection(connection, &path).unwrap();
    let mut player = rig.join_with(recorded);
    rig.start_round(&mut [&mut player]);
    steer(&mut player, Steer::Left);
    rig.steps(&mut [&mut player], 12);
    rig.settle(&mut [&mut player]);
    player.advance().unwrap();
    let final_summary = player.client().world().summary();
    let final_camera = player.camera;
    drop(player); // closes the recording

    let connection: Box<dyn transport::Connection> =
        Box::new(game_shell::files::open_recording(&path).unwrap());
    let mut replay = Session::<_, Eels>::connect(
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
    assert!(matches!(outcome, client::ClientError::ConnectionClosed));
    replay.settle(|| {}).unwrap();

    assert_eq!(replay.client().world().summary(), final_summary);
    assert_eq!(replay.camera.position, final_camera.position);
    assert_eq!(
        (replay.camera.yaw, replay.camera.pitch),
        (final_camera.yaw, final_camera.pitch)
    );
    // The replay shows the match: the status line is the recorded player's.
    let status = <Eels as GameDefinition>::status_text(&replay);
    assert!(status.starts_with("red eel"), "{status}");
    std::fs::remove_dir_all(&directory).unwrap();
}
