//! A rocket over the planet of a new world, with its pilot: for the tests of what a ship does far from the planet, at the speeds of a flight.

use game_shell::{GameContent, KeyCode};
use server::GridView;
use voxel_space::{FaceDirection, GridId, LevelIndex, VoxelCoordinate};
use voxunami_cave::aboard::in_voxels;
use voxunami_cave::content::{CaveContent, dev_command_seat_id, dev_rocket_engine_id};
use voxunami_cave::placement::turn_carrying;

use super::{Player, PlayerExt, Scene, pose_at, say, tick_and_follow};

/// Where the seat is on the deck of the ship.
pub const SEAT: [i128; 3] = [3, 1, 3];

/// How far out from the planet's centre the ship begins, in voxels: past the planet's ground (63.7 km and hills), where only the space level and the ship are.
pub const START: f64 = 70_000.0;

/// In the world of `scene` (a new game's: the planet at the origin of space), over the pole of the planet, a ship with a deck 8 by 8 of stone, a seat and four engines under the corners
/// with their exhausts down (so they push it straight up, away from the planet, through its centre of mass), held in place; and a player beside the seat on the deck, aboard.
pub fn a_ship_with_a_seat_and_four_engines_over_the_planet(
    mut scene: Scene,
) -> (Scene, Player, GridId) {
    let content = CaveContent::new();
    let registry = content.registries()[0].1;
    let seat = registry.raw_id_of(&dev_command_seat_id()).unwrap();
    let engine = registry.raw_id_of(&dev_rocket_engine_id()).unwrap();
    let exhaust_down: u32 = turn_carrying(FaceDirection::PositiveY, FaceDirection::NegativeY)
        .index()
        .into();
    let made = std::sync::Arc::new(std::sync::Mutex::new(None));
    let slot = std::sync::Arc::clone(&made);
    scene.run(move |world| {
        let grid = world
            .create_grid(LevelIndex::MAIN, pose_at(0.0, START, 0.0))
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
    let mut player = scene.rig.join_walking([0.5, START, 0.5], 0.0, 0.0);
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
    assert_eq!(super::player_pose(&scene, 1).grid, grid, "aboard");
    player.take_command_replies();
    (scene, player, grid)
}

/// The player looks west and down at the seat from where they stand, sits in it, and lets the ship go going `speed` voxels a second up, with the throttle up.
pub fn sit_and_let_go_going_up(scene: &mut Scene, player: &mut Player, grid: GridId, speed: f64) {
    let (yaw, pitch) = (player.camera.yaw, player.camera.pitch);
    player
        .camera
        .look_by(std::f64::consts::FRAC_PI_2 - yaw, -0.56 - pitch);
    player.hold(&[]);
    tick_and_follow(scene, player, 3);
    let reply = say(scene, player, "/ship sit");
    assert!(reply.starts_with("you sit in the command seat"), "{reply}");
    scene.run(move |world| {
        world.set_grid_held(grid, false).unwrap();
        world
            .set_grid_velocity(grid, [0.0, speed, 0.0], [0.0; 3])
            .unwrap();
    });
    player.hold(&[KeyCode::ShiftLeft]);
}

/// The ship's place, its turn as a quaternion and its angular velocity.
pub fn state_of(scene: &Scene, grid: GridId) -> ([f64; 3], [f64; 4], [f64; 3]) {
    let ship = scene.rig.server.world().grids().get(grid).unwrap();
    (
        in_voxels(ship.pose.position),
        ship.pose.orientation.components(),
        ship.angular_velocity,
    )
}

/// The angle between two turns, in radians.
pub fn angle_between(first: [f64; 4], second: [f64; 4]) -> f64 {
    let dot: f64 = (0..4).map(|part| first[part] * second[part]).sum();
    2.0 * libm::acos(dot.abs().min(1.0))
}
