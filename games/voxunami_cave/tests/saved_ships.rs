//! Ships in saved worlds, as Cave plays them: a ship in flight is where it was and moving as it was when the program is started again, and a
//! player who logged out aboard it logs in aboard it, at the same place in it, wherever it has gone.

mod common;

use common::{
    FLOOR, Scene, eyes_in_the_world, eyes_in_their_frame, flat_floor, make_ship, player_pose,
    pose_at, scene_with_a_deck, ship_id, tick_and_follow, walker_on_the_deck,
};
use voxel_space::{BrickCoordinate, GridPose, Orientation, WorldPosition};

/// What the world has of a ship: where it is, how it moves, and whether it is held.
fn state_of(scene: &Scene, number: u64) -> (GridPose, [f32; 3], [f32; 3], bool) {
    let ship = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(number))
        .expect("the ship is in memory");
    (
        ship.pose,
        ship.linear_velocity,
        ship.angular_velocity,
        ship.held,
    )
}

#[test]
fn a_ship_in_flight_is_saved_and_a_restarted_server_has_it_where_and_as_it_was() {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    make_ship(
        &mut scene,
        pose_at(14.0, FLOOR + 30.0, 14.0),
        false,
        [6, 2, 6],
        |_| true,
    );
    scene.run(|world| {
        world
            .set_grid_velocity(ship_id(0), [3.0, 0.0, 1.0], [0.0, 0.4, 0.0])
            .unwrap();
    });
    scene.ticks(8);
    let before = state_of(&scene, 0);
    // It is falling and drifting and turning: this is not a ship at rest.
    assert!(before.1[1] < -1.0 && before.1[0].abs() > 0.5 && before.2[1].abs() > 0.1);

    let mut scene = scene.restarted();
    // The new server has the ship in its file. Brought in before a tick, it is in memory as it was saved, to the bit.
    assert!(scene.rig.server.world().grids().get(ship_id(0)).is_none());
    assert!(scene.rig.server.load_parked_ships().is_empty());
    assert_eq!(state_of(&scene, 0), before);
    assert_eq!(scene.rig.files.len(), 1);

    // And it goes on: it comes down on the floor and rests there, having drifted on the way.
    scene.ticks(400);
    let (pose, velocity, ..) = state_of(&scene, 0);
    let height = pose.position.brick().y() as f32 * 32.0 + pose.position.offset()[1];
    assert!(
        (height - FLOOR).abs() < 0.1,
        "it rests on the floor: {height}"
    );
    assert!(
        velocity.iter().all(|speed| speed.abs() < 0.1),
        "{velocity:?}"
    );
    assert!(pose.position.brick() != BrickCoordinate::ORIGIN || pose.position.offset()[0] > 14.5);
}

#[test]
fn a_held_ship_stays_held_and_where_it_was_through_a_restart() {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    make_ship(
        &mut scene,
        pose_at(14.0, FLOOR + 30.0, 14.0),
        true,
        [4, 4, 4],
        |_| true,
    );
    scene.ticks(10);
    let before = state_of(&scene, 0);
    assert!(before.3);
    let mut scene = scene.restarted();
    scene.ticks(40);
    // (Held: it did not fall.)
    assert_eq!(state_of(&scene, 0), before);
}

#[test]
fn a_player_who_logged_out_aboard_a_ship_logs_in_aboard_it_at_the_same_place_after_the_program_is_started_again()
 {
    let mut scene = scene_with_a_deck();
    let passenger = walker_on_the_deck(&mut scene);
    // The ship moves and turns, so that where the passenger is in the world is not where their coordinates in it say.
    let moved = GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [14.0, FLOOR + 3.0, 33.0]),
        Orientation::from_yaw_pitch(0.9, 0.0),
    );
    scene.run(move |world| world.set_grid_pose(ship_id(0), moved).unwrap());
    tick_and_follow(&mut scene, &mut { passenger }, 3);
    // (the passenger session is dropped here: they log out aboard)
    scene.ticks(3);
    let place_in_the_ship = eyes_in_their_frame(&scene, 1);
    let place_in_the_world = eyes_in_the_world(&scene, 1);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));

    let mut scene = scene.restarted();
    // The ship is back and the player is in its frame, hidden, where they were in it.
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
    assert_eq!(eyes_in_their_frame(&scene, 1), place_in_the_ship);

    // They log in again (the same player: the same key), somewhere else in the world: they are aboard, where they left.
    let mut back = scene
        .rig
        .join_walking_as(1, [200.0, f64::from(FLOOR) + 30.0, 200.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut back]);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
    assert_eq!(eyes_in_their_frame(&scene, 1), place_in_the_ship);
    let now = eyes_in_the_world(&scene, 1);
    for (found, wanted) in now.into_iter().zip(place_in_the_world) {
        assert!(
            (found - wanted).abs() < 0.2,
            "{now:?} against {place_in_the_world:?}"
        );
    }
}
