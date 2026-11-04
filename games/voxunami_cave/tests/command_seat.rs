//! Flying from the dev command seat, with the dev rocket engines: a player sits in the seat they look at (`/ship sit`, the `U` key), W A S D Q E turn the ship by the seat's torque,
//! shift and control set the throttle, the engines push the ship with no fuel, and a readout of the flight is on the screen. Checked with a real server and the real client of a player.

mod common;

use common::{
    FLOOR, Player, PlayerExt, Scene, flat_floor, player_pose, say, ship_id, tick_and_follow,
};
use game_shell::screen::{ScreenText, Source};
use game_shell::{GameContent, KeyCode};
use server::GridView;
use voxel_space::{FaceDirection, GravitySettings, LatticeRotation, VoxelCoordinate};
use voxunami_cave::content::{CaveContent, dev_command_seat_id, dev_rocket_engine_id};

/// The ship's origin: its deck is 8 by 1 by 8 of stone from here, 64 tonnes.
const ORIGIN: [f64; 3] = [20.0, FLOOR + 6.0, 22.0];

/// Where the seat is, standing on the deck.
const SEAT: [i128; 3] = [3, 1, 3];

/// The turn that carries a face to another, as the number a voxel keeps.
fn orientation_carrying(from: FaceDirection, to: FaceDirection) -> u32 {
    LatticeRotation::all()
        .into_iter()
        .find(|rotation| rotation.apply_face(from) == to)
        .unwrap()
        .index()
        .into()
}

/// A ship of an 8 by 8 deck with a seat on it (made facing west, as it is made) and four engines under its corners with their exhausts down, held in the air; and a player
/// standing next to the seat on the deck, aboard. Then there is no gravity, and the ship is let go.
fn a_ship_with_a_seat_and_four_engines() -> (Scene, Player) {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    let content = CaveContent::new();
    let registry = content.registries()[0].1;
    let seat = registry.raw_id_of(&dev_command_seat_id()).unwrap();
    let engine = registry.raw_id_of(&dev_rocket_engine_id()).unwrap();
    let exhaust_down = orientation_carrying(FaceDirection::PositiveY, FaceDirection::NegativeY);
    scene.run(move |world| {
        let grid = world
            .create_grid(
                voxel_space::LevelIndex::MAIN,
                common::pose_at(ORIGIN[0], ORIGIN[1], ORIGIN[2]),
            )
            .unwrap();
        world.set_grid_held(grid, true).unwrap();
        let mut view = world.grid(grid).unwrap();
        let mut put = |at: [i128; 3], block: u32, orientation: u32| {
            let at = VoxelCoordinate::new(at[0], at[1], at[2]);
            view.set(content.block_channel(), at, block).unwrap();
            view.set(content.orientation_channel(), at, orientation)
                .unwrap();
        };
        for x in 0..8 {
            for z in 0..8 {
                put([x, 0, z], content.blocks.stone, 0);
            }
        }
        put(SEAT, seat, 0);
        for at in [[1, -1, 1], [6, -1, 1], [1, -1, 6], [6, -1, 6]] {
            put(at, engine, exhaust_down);
        }
    });
    let mut player = scene.rig.join_walking([24.0, FLOOR + 3.0, 34.0], 0.0, 30.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 20);
    // Beside the seat, on the deck (its top is one up from the origin), east of it.
    player.send_command(&format!(
        "/tp {} {} {}",
        ORIGIN[0] + SEAT[0] as f64 + 1.5,
        ORIGIN[1] + 1.5,
        ORIGIN[2] + SEAT[2] as f64 + 0.5
    ));
    tick_and_follow(&mut scene, &mut player, 60);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0), "aboard");
    scene.run(|world| {
        world.set_gravity_settings(GravitySettings {
            base: [0.0; 3],
            ..*world.gravity_settings()
        });
        world.set_grid_held(ship_id(0), false).unwrap();
    });
    player.take_command_replies();
    (scene, player)
}

/// The player looks west and down at the seat from where they stand.
fn look_at_the_seat(player: &mut Player) {
    let (yaw, pitch) = (player.camera.yaw, player.camera.pitch);
    player
        .camera
        .look_by(std::f64::consts::FRAC_PI_2 - yaw, -0.56 - pitch);
    // (The window says where the camera is with every frame; a test says it by holding no keys.)
    player.hold(&[]);
}

fn ship_state(scene: &Scene) -> (voxel_space::GridPose, [f64; 3], [f64; 3]) {
    let ship = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(0))
        .expect("the ship is there");
    (ship.pose, ship.linear_velocity, ship.angular_velocity)
}

/// A direction of the ship's own axes, in the world's.
fn world_direction(scene: &Scene, local: [f64; 3]) -> [f64; 3] {
    ship_state(scene).0.direction_to_world(local)
}

fn sit(scene: &mut Scene, player: &mut Player) -> String {
    look_at_the_seat(player);
    // (The server hears where the camera is a tick before the player's view in the world is what it says.)
    tick_and_follow(scene, player, 3);
    say(scene, player, "/ship sit")
}

#[test]
fn a_player_sits_in_the_seat_they_look_at_and_gets_up_again_and_only_one_sits_in_it() {
    let (mut scene, mut player) = a_ship_with_a_seat_and_four_engines();
    // Not looking at the seat: nothing to sit in.
    let (yaw, pitch) = (player.camera.yaw, player.camera.pitch);
    player.camera.look_by(-yaw, 0.0 - pitch);
    player.hold(&[]);
    tick_and_follow(&mut scene, &mut player, 3);
    let reply = say(&mut scene, &mut player, "/ship sit");
    assert!(reply.contains("not looking at a command seat"), "{reply}");
    let reply = sit(&mut scene, &mut player);
    assert!(
        reply.starts_with("you sit in the command seat of ship 0"),
        "{reply}"
    );
    // The hover controls are not for a seated pilot, and the seat is taken.
    let reply = say(&mut scene, &mut player, "/ship pilot");
    assert!(reply.contains("sitting"), "{reply}");
    let reply = say(&mut scene, &mut player, "/ship sit");
    assert!(reply.contains("you get up"), "{reply}");
    // A pilot of the hover controls cannot sit without letting go of them.
    say(&mut scene, &mut player, "/ship pilot");
    look_at_the_seat(&mut player);
    let reply = say(&mut scene, &mut player, "/ship sit");
    assert!(reply.contains("/ship pilot"), "{reply}");
}

#[test]
fn shift_and_control_set_the_throttle_and_the_engines_push_the_ship_away_from_their_exhaust_and_use_no_fuel()
 {
    let (mut scene, mut player) = a_ship_with_a_seat_and_four_engines();
    sit(&mut scene, &mut player);
    // No throttle: the ship hangs where it is (there is no gravity).
    tick_and_follow(&mut scene, &mut player, 20);
    let (before, speed, _) = ship_state(&scene);
    assert_eq!(speed, [0.0; 3]);
    // Shift for two seconds: full throttle, reached at the end of the second.
    player.hold(&[KeyCode::ShiftLeft]);
    tick_and_follow(&mut scene, &mut player, 40);
    player.hold(&[]);
    let text = player.hud_text();
    assert!(text.contains("throttle 100 %, 4 engines"), "{text}");
    tick_and_follow(&mut scene, &mut player, 40);
    // The exhausts are down, so the ship goes up: four engines of 200 kN on 64 tonnes and the seat and the engines, about 12 voxels a second a second.
    let (after, speed, _) = ship_state(&scene);
    assert!(speed[1] > 20.0, "{speed:?}");
    assert!(speed[0].abs() < 0.5 && speed[2].abs() < 0.5, "{speed:?}");
    assert!(after.position.in_voxels()[1] > before.position.in_voxels()[1] + 10.0);
    // Control lowers it again, to nothing, and then the ship coasts at the speed it has.
    player.hold(&[KeyCode::ControlLeft]);
    tick_and_follow(&mut scene, &mut player, 60);
    player.hold(&[]);
    let text = player.hud_text();
    assert!(text.contains("throttle 0 %"), "{text}");
    let (_, coasting, _) = ship_state(&scene);
    tick_and_follow(&mut scene, &mut player, 20);
    let (_, still_coasting, _) = ship_state(&scene);
    assert!(
        (still_coasting[1] - coasting[1]).abs() < 1.0e-6,
        "{coasting:?} {still_coasting:?}"
    );
}

#[test]
fn w_pitches_the_nose_down_s_up_d_yaws_right_a_left_e_rolls_right_q_left_by_the_seats_torque() {
    // (Nose, right hand and up as the sitter has them: the seat is made facing west (-x) with up up, so the nose is -x, the right hand -z and up +y.)
    let nose = [-1.0, 0.0, 0.0];
    let right = [0.0, 0.0, -1.0];
    let up = [0.0, 1.0, 0.0];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    // Each key, for a second, and the way the nose, the right hand and up have turned: the key gives its turn about its axis, here seen in the world's axes.
    let cases = [
        (KeyCode::KeyW, "nose down", nose, up, -1.0),
        (KeyCode::KeyS, "nose up", nose, up, 1.0),
        (KeyCode::KeyD, "nose to the right", nose, right, 1.0),
        (KeyCode::KeyA, "nose to the left", nose, right, -1.0),
        (KeyCode::KeyE, "right hand down", right, up, -1.0),
        (KeyCode::KeyQ, "right hand up", right, up, 1.0),
    ];
    for (key, what, moved, toward, sign) in cases {
        let (mut scene, mut player) = a_ship_with_a_seat_and_four_engines();
        sit(&mut scene, &mut player);
        player.hold(&[key]);
        tick_and_follow(&mut scene, &mut player, 20);
        let (_, _, turn) = ship_state(&scene);
        // A second at one radian a second a second: about one radian a second (the seat stops adding at one, a tick late, and the inertia it works from is an estimate), and the
        // thing that moves moves toward the way it should.
        let speed = (turn[0] * turn[0] + turn[1] * turn[1] + turn[2] * turn[2]).sqrt();
        assert!((0.8..=1.15).contains(&speed), "{what}: {turn:?}");
        // (The ship began unturned, so its own axes were the world's: the thing that moves is where the turn took it, against the axis it should go toward.)
        let moved_in_the_world = world_direction(&scene, moved);
        assert!(
            sign * dot(moved_in_the_world, toward) > 0.05,
            "{what}: {moved_in_the_world:?} against {toward:?}"
        );
    }
}

#[test]
fn the_seat_stops_the_turn_when_the_keys_are_let_go_so_exactly_that_the_ship_does_not_turn_at_all()
{
    let (mut scene, mut player) = a_ship_with_a_seat_and_four_engines();
    sit(&mut scene, &mut player);
    player.hold(&[KeyCode::KeyW, KeyCode::KeyE]);
    tick_and_follow(&mut scene, &mut player, 30);
    assert_ne!(ship_state(&scene).2, [0.0; 3]);
    // Let go: it brakes at the same torque, in about as long as it took, and is then still to the bit.
    player.hold(&[]);
    tick_and_follow(&mut scene, &mut player, 80);
    assert_eq!(ship_state(&scene).2, [0.0; 3]);
    let (pose, ..) = ship_state(&scene);
    tick_and_follow(&mut scene, &mut player, 20);
    assert_eq!(ship_state(&scene).0.orientation, pose.orientation);
}

#[test]
fn the_engines_work_only_while_someone_sits_and_getting_up_cuts_the_throttle() {
    let (mut scene, mut player) = a_ship_with_a_seat_and_four_engines();
    sit(&mut scene, &mut player);
    player.hold(&[KeyCode::ShiftLeft]);
    tick_and_follow(&mut scene, &mut player, 40);
    player.hold(&[]);
    let reply = say(&mut scene, &mut player, "/ship sit");
    assert!(reply.contains("you get up"), "{reply}");
    let (_, speed, _) = ship_state(&scene);
    tick_and_follow(&mut scene, &mut player, 40);
    let (_, later, _) = ship_state(&scene);
    assert_eq!(later, speed, "no one sits, no engine pushes");
    // Sitting again starts with the throttle at nothing.
    sit(&mut scene, &mut player);
    let text = player.hud_text();
    assert!(text.contains("throttle 0 %"), "{text}");
    // And the screen has no readout once they are up.
    say(&mut scene, &mut player, "/ship sit");
    let text = player.hud_text();
    assert!(!text.contains("throttle"), "{text}");
}

#[test]
fn the_readout_says_the_planet_that_pulls_the_ship_how_far_and_how_fast_and_the_orbit() {
    let (mut scene, mut player) = a_ship_with_a_seat_and_four_engines();
    sit(&mut scene, &mut player);
    tick_and_follow(&mut scene, &mut player, 2);
    let text = player.hud_text();
    assert!(text.contains("throttle 0 %, 4 engines"), "{text}");
    assert!(text.contains("turning 0.00 rad/s"), "{text}");
    // (The flat floor of this scene is the terrain of a level, not a planet that pulls: nothing pulls here, and the readout says so.)
    assert!(text.contains("no planet pulls here"), "{text}");
}

/// How many panels of the client plugins are on the screen (the hotbar, and the inventory if it is open).
fn panels(player: &mut Player) -> usize {
    player
        .screen(&ScreenText::default())
        .sources
        .iter()
        .filter(|source| matches!(source, Source::Overlay(_)))
        .count()
}

#[test]
fn e_rolls_the_ship_of_a_seated_pilot_and_opens_the_inventory_only_when_they_are_up() {
    let (mut scene, mut player) = a_ship_with_a_seat_and_four_engines();
    assert_eq!(panels(&mut player), 1, "the hotbar");
    sit(&mut scene, &mut player);
    tick_and_follow(&mut scene, &mut player, 2);
    player.key_pressed(KeyCode::KeyE);
    assert_eq!(panels(&mut player), 1, "seated, E is not the inventory");
    say(&mut scene, &mut player, "/ship sit");
    tick_and_follow(&mut scene, &mut player, 2);
    player.key_pressed(KeyCode::KeyE);
    assert_eq!(panels(&mut player), 2, "up, it is");
}

#[test]
fn the_u_key_sits_in_the_seat_you_look_at_and_gets_up() {
    let (mut scene, mut player) = a_ship_with_a_seat_and_four_engines();
    look_at_the_seat(&mut player);
    tick_and_follow(&mut scene, &mut player, 3);
    player.key_pressed(KeyCode::KeyU);
    tick_and_follow(&mut scene, &mut player, 3);
    assert!(player.hud_text().contains("throttle 0 %"));
    player.key_pressed(KeyCode::KeyU);
    tick_and_follow(&mut scene, &mut player, 3);
    assert!(!player.hud_text().contains("throttle"));
}
