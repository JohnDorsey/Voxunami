//! `/ship`: a player makes a ship of what they built, holds it, pushes it, and flies it with the keys, with others aboard; checked with a real
//! server and the real command line of a player.

mod common;

use common::{
    FLOOR, Player, PlayerExt, Scene, build_raft, eyes_in_the_world, eyes_in_their_frame,
    flat_floor, player_pose, say, ship_id, ship_pose, tick_and_follow,
};
use game_shell::KeyCode;
use server::GridView;
use voxel_space::GridId;
use voxel_space::VoxelCoordinate;
use voxunami_cave::aboard::in_voxels;

/// A flat floor, a raft of 8 by 8 stone blocks built five above it (from x 20, z 22, one thick, its blocks from y = FLOOR + 6), and a
/// player on the floor south of it looking north and up at its underside.
fn raft_and_a_player_looking_at_it() -> (Scene, Player) {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    build_raft(&mut scene, 20, FLOOR as i64 + 6, 22, 8);
    let mut player = scene
        .rig
        .join_walking([24.0, FLOOR as f64 + 3.0, 34.0], 0.0, 30.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 20);
    (scene, player)
}

fn ship_speed(scene: &Scene, number: u64) -> [f32; 3] {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(number))
        .expect("the ship exists")
        .linear_velocity
}

#[test]
fn a_ship_is_made_of_the_built_blocks_joined_to_the_one_the_player_looks_at_and_falls() {
    let (mut scene, mut player) = raft_and_a_player_looking_at_it();
    let reply = say(&mut scene, &mut player, "/ship create");
    assert_eq!(
        reply,
        "made ship 0 of 64 blocks (it falls unless you /ship hold on)"
    );
    // It is where the blocks were: its corner at the lowest corner of the raft, not turned (it has begun to fall: a sixth of a voxel in the
    // two ticks that the reply took), and the raft is gone from the world.
    let pose = ship_pose(&scene, 0);
    let at = in_voxels(pose.position);
    assert!(
        at[0] == 20.0 && at[2] == 22.0 && (at[1] - (FLOOR as f64 + 6.0)).abs() < 0.3,
        "{pose:?}"
    );
    assert_eq!(pose.orientation, voxel_space::Orientation::IDENTITY);
    // Nobody holds it, so it falls the five voxels and lies on the floor.
    tick_and_follow(&mut scene, &mut player, 120);
    scene.rig.settle(&mut [&mut player]);
    let middle = VoxelCoordinate::new(24, FLOOR as i64 + 6, 26);
    assert_eq!(player.mesher().is_opaque_at(middle), Some(false));
    let at = in_voxels(ship_pose(&scene, 0).position);
    assert!((at[1] - FLOOR as f64).abs() < 0.1, "{at:?}");
    assert!(
        (at[0] - 20.0).abs() < 0.1 && (at[2] - 22.0).abs() < 0.1,
        "{at:?}"
    );
}

#[test]
fn a_raft_built_long_ago_is_still_a_ship_after_the_world_has_forgotten_the_terrain_it_was_built_in()
{
    // (A player's crash: "the brick has no edits and was just assigned a generator", from asking what the generator made in a brick that had
    // been edited, once the world no longer remembered having generated it.)
    let (mut scene, mut player) = raft_and_a_player_looking_at_it();
    let content = voxunami_cave::content::CaveContent::new();
    scene.run(move |world| {
        // Far more bricks than the world remembers, a long way off and high up where they are empty and quick to make.
        for x in 1..=400 {
            world
                .grid(GridId::root(voxel_space::LevelIndex::MAIN))
                .unwrap()
                .get(
                    content.block_channel(),
                    VoxelCoordinate::new(32 * x, 3000, 5),
                )
                .unwrap();
        }
    });
    assert_eq!(
        say(&mut scene, &mut player, "/ship create"),
        "made ship 0 of 64 blocks (it falls unless you /ship hold on)"
    );
}

#[test]
fn what_is_not_a_ship_to_make_is_said_so_and_nothing_changes() {
    let (mut scene, mut player) = raft_and_a_player_looking_at_it();
    // Looking at the sky.
    player.send_command("/tp 24 300 24");
    tick_and_follow(&mut scene, &mut player, 3);
    assert!(say(&mut scene, &mut player, "/ship create").contains("not looking at a block"));
    // The floor was built, as far as the world knows, and is bigger than any ship: refused, and it says what it counted.
    player.send_command(&format!("/tp 40 {} 40", FLOOR as i64 + 3));
    tick_and_follow(&mut scene, &mut player, 20);
    player.camera.look_by(0.0, -3.0);
    player.hold(&[]);
    tick_and_follow(&mut scene, &mut player, 2);
    player.take_command_replies();
    assert_eq!(
        say(&mut scene, &mut player, "/ship create"),
        "more than 4096 blocks are joined to that one (counting only what has been built)"
    );
    assert!(scene.rig.server.world().grids().is_empty());
    // A command that is not one.
    assert!(say(&mut scene, &mut player, "/ship fly").starts_with("usage: /ship"));
    assert!(say(&mut scene, &mut player, "/ship push 1 2").starts_with("usage: /ship"));
    assert!(say(&mut scene, &mut player, "/ship hold maybe").starts_with("usage: /ship"));
}

#[test]
fn ground_nobody_built_is_not_a_ship_unless_all_is_said_and_then_it_is_too_big() {
    // The natural terrain of Cave: a player stands on it and looks straight down.
    let mut scene = Scene::new();
    let mut player = scene.rig.join_walking([16.5, 110.0, 16.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 80);
    player.camera.look_by(0.0, -3.0);
    player.hold(&[]);
    tick_and_follow(&mut scene, &mut player, 2);
    player.take_command_replies();
    assert!(
        say(&mut scene, &mut player, "/ship create")
            .starts_with("that was not built by anyone; /ship create all"),
    );
    assert_eq!(
        say(&mut scene, &mut player, "/ship create all"),
        "more than 4096 blocks are joined to that one"
    );
    assert!(scene.rig.server.world().grids().is_empty());
}

#[test]
fn of_two_ships_the_nearer_is_the_one_meant_and_a_ship_beyond_reach_is_none() {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    // The player is at x 24. Ship 0 is 30 east of them and ship 1, made after it, 10 east; each is a plate of 4 by 4, held.
    for east in [54.0, 34.0] {
        common::make_ship(
            &mut scene,
            common::pose_at(east, FLOOR + 3.0, 30.0),
            true,
            [4, 1, 4],
            |_| true,
        );
    }
    let mut player = scene
        .rig
        .join_walking([24.0, FLOOR as f64 + 3.0, 34.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 20);
    player.take_command_replies();
    assert_eq!(
        say(&mut scene, &mut player, "/ship hold off"),
        "ship 1 is let go"
    );
    let held = |scene: &Scene, number: u64| {
        scene
            .rig
            .server
            .world()
            .grids()
            .get(ship_id(number))
            .unwrap()
            .held
    };
    assert!(held(&scene, 0) && !held(&scene, 1));
    // Out of reach of both (they are 60 and 80 away, in the brick that holds them), they are none.
    player.send_command("/tp 24 220 -60");
    tick_and_follow(&mut scene, &mut player, 5);
    player.take_command_replies();
    assert!(say(&mut scene, &mut player, "/ship hold off").contains("no ship is near"));
    assert!(held(&scene, 0));
}

#[test]
fn a_player_far_from_every_ship_is_told_there_is_none_near() {
    let (mut scene, mut player) = raft_and_a_player_looking_at_it();
    assert!(say(&mut scene, &mut player, "/ship hold on").contains("no ship is near"));
    assert!(say(&mut scene, &mut player, "/ship push 1 0 0").contains("no ship is near"));
    assert!(say(&mut scene, &mut player, "/ship pilot").contains("stand on a ship"));
}

#[test]
fn a_ship_that_is_held_stays_and_one_that_is_let_go_and_pushed_goes_the_speed_it_was_pushed() {
    let (mut scene, mut player) = raft_and_a_player_looking_at_it();
    say(&mut scene, &mut player, "/ship create");
    // The nearest ship is the one meant: this one is a few voxels away.
    assert_eq!(
        say(&mut scene, &mut player, "/ship hold on"),
        "ship 0 is held"
    );
    let held_at = in_voxels(ship_pose(&scene, 0).position);
    tick_and_follow(&mut scene, &mut player, 40);
    assert_eq!(in_voxels(ship_pose(&scene, 0).position), held_at);
    // A held ship does not take a push.
    assert!(say(&mut scene, &mut player, "/ship push 5 0 0").contains("is held"));
    assert_eq!(
        say(&mut scene, &mut player, "/ship hold off"),
        "ship 0 is let go"
    );
    // Pushed by 6 east in a tick: its speed east is 6 (and it falls, which is another matter).
    let before = ship_speed(&scene, 0)[0];
    assert_eq!(
        say(&mut scene, &mut player, "/ship push 6 0 0"),
        "pushed ship 0"
    );
    let gained = ship_speed(&scene, 0)[0] - before;
    assert!((gained - 6.0).abs() < 0.3, "gained {gained}");
}

/// A raft that has been made a ship and held in the air, and a player who stands on it: they were put above it and fell.
fn a_player_on_a_held_ship() -> (Scene, Player) {
    let (mut scene, mut player) = raft_and_a_player_looking_at_it();
    say(&mut scene, &mut player, "/ship create");
    say(&mut scene, &mut player, "/ship hold on");
    player.send_command(&format!("/tp 24 {} 26", FLOOR as i64 + 12));
    tick_and_follow(&mut scene, &mut player, 60);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0), "aboard");
    player.take_command_replies();
    (scene, player)
}

#[test]
fn a_pilot_hovers_a_ship_with_no_keys_flies_where_they_look_and_stops_when_they_let_go() {
    let (mut scene, mut player) = a_player_on_a_held_ship();
    let in_the_ship = eyes_in_their_frame(&scene, 1);
    let reply = say(&mut scene, &mut player, "/ship pilot");
    assert!(reply.starts_with("you pilot ship 0"), "{reply}");
    // The ship is let go; the pilot holds it up against gravity.
    say(&mut scene, &mut player, "/ship hold off");
    let hanging = in_voxels(ship_pose(&scene, 0).position);
    tick_and_follow(&mut scene, &mut player, 60);
    let still = in_voxels(ship_pose(&scene, 0).position);
    for axis in 0..3 {
        assert!(
            (still[axis] - hanging[axis]).abs() < 0.1,
            "it drifted: {hanging:?} then {still:?}"
        );
    }
    // W flies it the way the pilot looks, which is north (-z) when they look level; the pilot rides it, standing where they stood in it.
    let looking_up = player.camera.pitch;
    player.camera.look_by(0.0, -looking_up);
    player.hold(&[KeyCode::KeyW]);
    tick_and_follow(&mut scene, &mut player, 60);
    let flown = in_voxels(ship_pose(&scene, 0).position);
    assert!(still[2] - flown[2] > 30.0, "{still:?} then {flown:?}");
    assert!(
        (flown[0] - still[0]).abs() < 0.5 && (flown[1] - still[1]).abs() < 0.5,
        "it should go straight: {still:?} then {flown:?}"
    );
    let speed = ship_speed(&scene, 0);
    assert!(
        speed[2] < -15.0 && speed[2] > -26.0,
        "after three seconds of a thrust of 24 against a drag of 1 it goes about 23: {speed:?}"
    );
    let riding = eyes_in_their_frame(&scene, 1);
    for axis in 0..3 {
        assert!(
            (riding[axis] - in_the_ship[axis]).abs() < 0.02,
            "{riding:?} against {in_the_ship:?}"
        );
    }
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
    // The keys are let go: it slows to a stop, a tenth a second faster than e to the minus its time, and hangs.
    player.hold(&[]);
    tick_and_follow(&mut scene, &mut player, 200);
    let speed = ship_speed(&scene, 0);
    assert!(
        speed.iter().all(|component| component.abs() < 0.05),
        "{speed:?}"
    );
    // Looking up, W flies up as well as north; the ship goes the way the pilot looks.
    let before = in_voxels(ship_pose(&scene, 0).position);
    player.camera.look_by(0.0, 0.5);
    player.hold(&[KeyCode::KeyW]);
    tick_and_follow(&mut scene, &mut player, 20);
    let after = in_voxels(ship_pose(&scene, 0).position);
    assert!(
        after[1] - before[1] > 3.0 && before[2] - after[2] > 3.0,
        "{before:?} {after:?}"
    );
    player.hold(&[]);
    tick_and_follow(&mut scene, &mut player, 200);
    // Space goes up and shift goes down.
    let before = in_voxels(ship_pose(&scene, 0).position)[1];
    player.hold(&[KeyCode::Space]);
    tick_and_follow(&mut scene, &mut player, 30);
    let up = in_voxels(ship_pose(&scene, 0).position)[1];
    assert!(up - before > 5.0, "{before} then {up}");
    player.hold(&[KeyCode::ShiftLeft]);
    tick_and_follow(&mut scene, &mut player, 60);
    assert!(in_voxels(ship_pose(&scene, 0).position)[1] < up - 5.0);
    // The pilot never walked: they are where they were in the ship.
    let riding = eyes_in_their_frame(&scene, 1);
    for axis in 0..3 {
        assert!((riding[axis] - in_the_ship[axis]).abs() < 0.02);
    }
}

#[test]
fn a_pilot_who_lets_go_walks_again_and_the_ship_they_left_falls() {
    let (mut scene, mut player) = a_player_on_a_held_ship();
    say(&mut scene, &mut player, "/ship pilot");
    say(&mut scene, &mut player, "/ship hold off");
    tick_and_follow(&mut scene, &mut player, 20);
    let letting_go = say(&mut scene, &mut player, "/ship pilot");
    assert_eq!(letting_go, "you let go of the controls of ship 0");
    // Without a pilot the ship falls, with the player who is aboard it, and lies on the floor.
    tick_and_follow(&mut scene, &mut player, 120);
    let at = in_voxels(ship_pose(&scene, 0).position);
    assert!((at[1] - FLOOR as f64).abs() < 0.1, "{at:?}");
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
    // And their keys walk them: W takes them north along the ship, off its edge, and they leave it.
    player.hold(&[KeyCode::KeyW]);
    tick_and_follow(&mut scene, &mut player, 80);
    assert!(player_pose(&scene, 1).grid.root_level().is_some());
    let eyes = eyes_in_the_world(&scene, 1);
    assert!(eyes[2] < 22.0, "{eyes:?}");
}

#[test]
fn only_one_player_pilots_a_ship_and_a_player_who_goes_away_stops_piloting() {
    let (mut scene, mut player) = a_player_on_a_held_ship();
    assert!(say(&mut scene, &mut player, "/ship pilot").starts_with("you pilot ship 0"));
    let mut other = scene
        .rig
        .join_walking_as(2, [24.0, FLOOR as f64 + 12.0, 24.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut other]);
    tick_and_follow(&mut scene, &mut player, 60);
    other.advance().unwrap();
    assert_eq!(
        player_pose(&scene, 2).grid,
        ship_id(0),
        "the second is aboard too"
    );
    other.send_command("/ship pilot");
    for _ in 0..3 {
        scene.rig.step();
        other.advance().unwrap();
    }
    assert_eq!(
        other.take_command_replies(),
        ["someone else is piloting ship 0"]
    );
    // The pilot is teleported away: the ship is nobody's, and the other can pilot it.
    player.send_command("/tp 50 230 50");
    tick_and_follow(&mut scene, &mut player, 3);
    other.send_command("/ship pilot");
    for _ in 0..3 {
        scene.rig.step();
        other.advance().unwrap();
    }
    assert!(
        other.take_command_replies()[0].starts_with("you pilot ship 0"),
        "the ship is free"
    );
}

/// A hash, bit for bit, of the ship and the pilot after every tick of a scripted flight: a raft made and held, a player who falls onto it and
/// takes the controls, flies with every key and a push and a change of the way they look, lets go, and walks off.
fn scripted_flight() -> u64 {
    let mut state: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |value: u64| {
        for byte in value.to_le_bytes() {
            state ^= u64::from(byte);
            state = state.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    let (mut scene, mut player) = raft_and_a_player_looking_at_it();
    for tick in 0..420usize {
        let command = match tick {
            0 => Some("/ship create".to_owned()),
            4 => Some("/ship hold on".to_owned()),
            8 => Some(format!("/tp 24 {} 26", FLOOR as i64 + 12)),
            70 => Some("/ship pilot".to_owned()),
            72 => Some("/ship hold off".to_owned()),
            250 => Some("/ship push 5 0 3".to_owned()),
            330 => Some("/ship pilot".to_owned()),
            332 => Some("/ship hold on".to_owned()),
            _ => None,
        };
        if let Some(command) = command {
            player.send_command(&command);
        }
        let keys: Option<&[KeyCode]> = match tick {
            100 => Some(&[KeyCode::KeyW]),
            130 => Some(&[KeyCode::KeyW, KeyCode::KeyD]),
            160 => Some(&[KeyCode::Space]),
            190 => Some(&[KeyCode::ShiftLeft, KeyCode::KeyA]),
            220 => Some(&[]),
            270 => Some(&[KeyCode::KeyW, KeyCode::ControlLeft]),
            300 => Some(&[]),
            340 => Some(&[KeyCode::KeyA]),
            _ => None,
        };
        if let Some(keys) = keys {
            player.camera.yaw = 0.01 * tick as f32;
            player.camera.pitch = 0.0;
            player.hold(keys);
        }
        scene.rig.step();
        player.advance().unwrap();
        let world = scene.rig.server.world();
        if let Some(ship) = world.grids().get(ship_id(0)) {
            let (brick, offset) = (ship.pose.position.brick(), ship.pose.position.offset());
            for value in [brick.x(), brick.y(), brick.z()] {
                feed(value as u64);
            }
            for value in offset
                .into_iter()
                .chain(ship.pose.orientation.components_single())
                .chain(ship.linear_velocity)
                .chain(ship.angular_velocity)
            {
                feed(u64::from(value.to_bits()));
            }
        }
        let pose = player_pose(&scene, 1);
        feed(pose.grid.get());
        for value in pose
            .position
            .offset()
            .into_iter()
            .chain(pose.orientation.components_single())
        {
            feed(u64::from(value.to_bits()));
        }
    }
    // (So that the number locks something: they flew a long way, and walked off.)
    let ship = in_voxels(ship_pose(&scene, 0).position);
    assert!(
        (ship[0] - 20.0).abs() + (ship[2] - 22.0).abs() > 20.0,
        "{ship:?}"
    );
    assert!(player_pose(&scene, 1).grid.root_level().is_some());
    state
}

#[test]
fn a_scripted_flight_ends_in_the_same_bits_every_time_and_on_every_machine() {
    let first = scripted_flight();
    assert_eq!(first, scripted_flight());
    // Taken on Windows; Linux is checked to agree to the bit.
    // (Moved once, on purpose, 2026-10-22: the pull of the planet is 5 metres a second a second, was 32, and the pilot's thrust holds the ship up against it.)
    // (Moved on purpose, Phase 38a: 1_611_522_163_103_596_451 before. Orientations are f64 (Phase 38a, 2026-10-23): a yaw and a pitch are turned into a quaternion in f64 and rounded once to the f32 the solver takes, which is not always the f32 the old arithmetic in f32 gave, to the last bit. Control: with `from_yaw_pitch` put back to the f32 arithmetic and nothing else changed, the old value came back; so this is the only thing that moved it.)
    assert_eq!(first, 14_857_158_006_750_149_408);
}

#[test]
fn a_pilot_of_a_ship_that_is_cut_apart_has_the_controls_of_the_part_they_are_aboard() {
    use common::{scene_with_a_deck, walker_on_the_deck};
    let mut scene = scene_with_a_deck();
    let mut pilot = walker_on_the_deck(&mut scene);
    assert!(say(&mut scene, &mut pilot, "/ship pilot").starts_with("you pilot ship 0"));
    // The end of the deck, 7 columns of it, from the 23rd along, is taken away from the rest by a cut of one column, away from the pilot.
    let content = voxunami_cave::content::CaveContent::new();
    scene.run(move |world| {
        for z in 0..8 {
            for y in 0..2 {
                use server::GridView;
                world
                    .grid(ship_id(0))
                    .unwrap()
                    .set(
                        content.block_channel(),
                        VoxelCoordinate::new(22, y, z),
                        content.blocks.air,
                    )
                    .unwrap();
            }
        }
    });
    tick_and_follow(&mut scene, &mut pilot, 5);
    let frame = player_pose(&scene, 1).grid;
    assert_ne!(frame, ship_id(0), "the ship they were on is gone");
    assert!(scene.rig.server.world().grids().get(frame).is_some());
    // They are still at the controls, of the part: letting go says so, with the number of the part.
    let reply = say(&mut scene, &mut pilot, "/ship pilot");
    assert_eq!(
        reply,
        format!(
            "you let go of the controls of ship {}",
            frame.get() - voxel_space::GridId::FIRST_ALLOCATED
        )
    );
}

/// The distances the server keeps ships by: where they are simulated, and where they are put in files.
fn distances(scene: &mut Scene) -> (f64, f64) {
    let found = std::sync::Arc::new(std::sync::Mutex::new((0.0, 0.0)));
    let slot = std::sync::Arc::clone(&found);
    scene.run(move |world| {
        let settings = world.ship_settings();
        *slot.lock().unwrap() = (settings.simulation_distance, settings.unload_distance);
    });
    std::mem::take(&mut *found.lock().unwrap())
}

#[test]
fn how_far_from_a_player_ships_are_simulated_is_a_setting_of_the_server_that_a_command_shows_and_changes()
 {
    let (mut scene, mut player) = raft_and_a_player_looking_at_it();
    // The tests keep every ship going; the server was started so.
    assert_eq!(
        say(&mut scene, &mut player, "/ship distance"),
        "ships are simulated everywhere, and put in files nowhere"
    );
    // (What a pilot's logout does is not the command's to change.)
    scene.run(|world| {
        let settings = world
            .ship_settings()
            .with_logout_policy(server::LogoutPolicy::AlwaysStay);
        world.set_ship_settings(settings);
    });
    assert_eq!(
        say(&mut scene, &mut player, "/ship distance 100"),
        "ships are simulated within 100 voxels of a player, and put in files nowhere"
    );
    assert_eq!(distances(&mut scene).0, 100.0);
    assert_eq!(
        say(&mut scene, &mut player, "/ship distance 80 600"),
        "ships are simulated within 80 voxels of a player, and put in files beyond 600"
    );
    // A smaller unload distance than the simulation distance is refused, and so are words that are no distance; nothing changes.
    let refused = say(&mut scene, &mut player, "/ship distance 700 600");
    assert!(refused.contains("put away farther off"), "{refused}");
    assert!(say(&mut scene, &mut player, "/ship distance -5").starts_with("usage: /ship"));
    assert!(say(&mut scene, &mut player, "/ship distance far").starts_with("usage: /ship"));
    assert!(say(&mut scene, &mut player, "/ship distance 1 2 3").starts_with("usage: /ship"));
    assert!(say(&mut scene, &mut player, "/ship distance 5 far").starts_with("usage: /ship"));
    assert_eq!(distances(&mut scene), (80.0, 600.0));
    let policy = std::sync::Arc::new(std::sync::Mutex::new(None));
    let slot = std::sync::Arc::clone(&policy);
    scene.run(move |world| *slot.lock().unwrap() = Some(world.ship_settings().logout_policy));
    assert_eq!(
        *policy.lock().unwrap(),
        Some(server::LogoutPolicy::AlwaysStay)
    );
    // A simulation distance above the unload distance takes it along; `everywhere` is both.
    assert_eq!(
        say(&mut scene, &mut player, "/ship distance 900"),
        "ships are simulated within 900 voxels of a player, and put in files beyond 900"
    );
    assert_eq!(
        say(&mut scene, &mut player, "/ship distance 30 nowhere"),
        "ships are simulated within 30 voxels of a player, and put in files nowhere"
    );
    assert_eq!(
        say(&mut scene, &mut player, "/ship distance everywhere"),
        "ships are simulated everywhere, and put in files nowhere"
    );
}

#[test]
fn a_ship_beyond_the_distance_the_server_simulates_stops_and_goes_on_when_the_distance_grows() {
    let (mut scene, mut player) = raft_and_a_player_looking_at_it();
    assert!(say(&mut scene, &mut player, "/ship create").starts_with("made ship 0"));
    // The player goes 270 voxels away and the server simulates ships within 50: the raft, which was falling, is where it is.
    player.send_command(&format!("/tp 24 {} 300", FLOOR as i64 + 3));
    tick_and_follow(&mut scene, &mut player, 5);
    let said = say(&mut scene, &mut player, "/ship distance 50");
    assert!(said.contains("ships are simulated within 50"), "{said}");
    tick_and_follow(&mut scene, &mut player, 5);
    let ship = |scene: &Scene| {
        scene
            .rig
            .server
            .world()
            .grids()
            .get(ship_id(0))
            .unwrap()
            .clone()
    };
    let stopped = ship(&scene);
    assert!(stopped.frozen);
    tick_and_follow(&mut scene, &mut player, 40);
    assert_eq!(ship(&scene).pose, stopped.pose, "it does not move");
    // Simulated everywhere again, it falls.
    say(&mut scene, &mut player, "/ship distance everywhere");
    tick_and_follow(&mut scene, &mut player, 40);
    let going = ship(&scene);
    assert!(!going.frozen);
    assert_ne!(going.pose, stopped.pose, "it moves");
}
