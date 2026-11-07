//! A ship under power does not turn when it goes from one frame of the solver to the next.
//!
//! The solver keeps ships in frames, places with an origin of their own, and a ship that goes twice as far from the origin of its frame as a ship may be in one (32 km) is made a body
//! again in another. Found by the owner flying the dev rocket up from a planet: at about 95 km (the planet's 64 km and the first 32 km of the flight) it suddenly turned, a ninety
//! degrees or so, and then did again every 10 to 20 seconds. The engines pushed the ship in the tick its new body was made, and Rapier turned the push about the origin of the
//! ship and not about its centre of mass (see `engine/physics/tests/forces.rs`): for that tick it spun at 0.6 radians a second, and the seat took nine ticks to stop it.

mod common;

use common::{Player, PlayerExt, Scene, tick_and_follow};
use game_shell::{GameContent, KeyCode};
use server::GridView;
use voxel_space::{FaceDirection, GridId, LatticeRotation, LevelIndex, VoxelCoordinate};
use voxunami_cave::aboard::in_voxels;
use voxunami_cave::content::{CaveContent, dev_command_seat_id, dev_rocket_engine_id};

/// Where the seat is on the deck of the ship.
const SEAT: [i128; 3] = [3, 1, 3];

/// How far out from the planet's centre the ship begins, in voxels: past the planet's ground (63.7 km and hills), where only the space level and the ship are.
const START: f64 = 70_000.0;

fn orientation_carrying(from: FaceDirection, to: FaceDirection) -> u32 {
    LatticeRotation::all()
        .into_iter()
        .find(|rotation| rotation.apply_face(from) == to)
        .unwrap()
        .index()
        .into()
}

/// A world like the one a new game starts in (the planet at the origin of space), and in space over its pole a ship with a deck 8 by 8 of stone, a seat and four engines under
/// the corners with their exhausts down (so they push it straight up, away from the planet, through its centre of mass), held in place; and a player beside the seat on the deck.
fn a_ship_with_a_seat_and_four_engines_over_the_planet() -> (Scene, Player, GridId) {
    let mut scene = Scene::in_a_new_world();
    let content = CaveContent::new();
    let registry = content.registries()[0].1;
    let seat = registry.raw_id_of(&dev_command_seat_id()).unwrap();
    let engine = registry.raw_id_of(&dev_rocket_engine_id()).unwrap();
    let exhaust_down = orientation_carrying(FaceDirection::PositiveY, FaceDirection::NegativeY);
    let made = std::sync::Arc::new(std::sync::Mutex::new(None));
    let slot = std::sync::Arc::clone(&made);
    scene.run(move |world| {
        let grid = world
            .create_grid(LevelIndex::MAIN, common::pose_at(0.0, START, 0.0))
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
        *slot.lock().unwrap() = Some(grid);
    });
    let grid = made.lock().unwrap().expect("the ship was made");
    let mut player = scene.rig.join_walking([0.5, 70_000.0, 0.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 20);
    // Beside the seat, on the deck (its top is one up from the origin), east of it.
    player.send_command(&format!(
        "/tp {} {} {}",
        SEAT[0] as f64 + 1.5,
        START + 1.5,
        SEAT[2] as f64 + 0.5
    ));
    tick_and_follow(&mut scene, &mut player, 60);
    assert_eq!(common::player_pose(&scene, 1).grid, grid, "aboard");
    player.take_command_replies();
    (scene, player, grid)
}

/// The ship's place, its turn as a quaternion and its angular velocity.
fn state_of(scene: &Scene, grid: GridId) -> ([f64; 3], [f64; 4], [f64; 3]) {
    let ship = scene.rig.server.world().grids().get(grid).unwrap();
    (
        in_voxels(ship.pose.position),
        ship.pose.orientation.components(),
        ship.angular_velocity,
    )
}

/// The angle between two turns, in radians.
fn angle_between(first: [f64; 4], second: [f64; 4]) -> f64 {
    let dot: f64 = (0..4).map(|part| first[part] * second[part]).sum();
    2.0 * libm::acos(dot.abs().min(1.0))
}

fn length(vector: [f64; 3]) -> f64 {
    vector.iter().map(|part| part * part).sum::<f64>().sqrt()
}

#[test]
fn a_ship_pushed_by_its_engines_does_not_turn_when_it_goes_from_one_frame_of_the_solver_to_the_next()
 {
    let (mut scene, mut player, grid) = a_ship_with_a_seat_and_four_engines_over_the_planet();
    // The player looks west and down at the seat from where they stand, and sits in it.
    let (yaw, pitch) = (player.camera.yaw, player.camera.pitch);
    player
        .camera
        .look_by(std::f64::consts::FRAC_PI_2 - yaw, -0.56 - pitch);
    player.hold(&[]);
    tick_and_follow(&mut scene, &mut player, 3);
    let reply = common::say(&mut scene, &mut player, "/ship sit");
    assert!(reply.starts_with("you sit in the command seat"), "{reply}");
    // Let go, going up at 1,500 voxels a second, with the throttle up.
    scene.run(move |world| {
        world.set_grid_held(grid, false).unwrap();
        world
            .set_grid_velocity(grid, [0.0, 1500.0, 0.0], [0.0; 3])
            .unwrap();
    });
    player.hold(&[KeyCode::ShiftLeft]);

    let (begun_at, mut before, _) = state_of(&scene, grid);
    let (mut most_turned_in_a_tick, mut most_turning) = (0.0f64, 0.0f64);
    // 40 seconds: 60 km and more, past the first frame's limit of 32 km.
    for _ in 0..800 {
        tick_and_follow(&mut scene, &mut player, 1);
        let (_, turn, turning) = state_of(&scene, grid);
        most_turned_in_a_tick = most_turned_in_a_tick.max(angle_between(before, turn));
        most_turning = most_turning.max(length(turning));
        before = turn;
    }
    let (ended_at, _, _) = state_of(&scene, grid);
    let travelled = length([0, 1, 2].map(|axis| ended_at[axis] - begun_at[axis]));
    assert!(
        travelled > 40_000.0,
        "the ship went {travelled} voxels, which is not past the limit of a frame"
    );
    // Straight through the centre of mass the engines turn it at most by the little that the seat and the engines weigh unevenly: under a thousandth of a radian a second (it
    // measured 0.00086), a twentieth of that in a tick. The jolt was 0.62 radians a second and 0.03 radians in a tick.
    assert!(
        most_turning < 0.01,
        "turned at {most_turning} radians a second"
    );
    assert!(
        most_turned_in_a_tick < 0.001,
        "turned {most_turned_in_a_tick} radians in a tick"
    );
}
