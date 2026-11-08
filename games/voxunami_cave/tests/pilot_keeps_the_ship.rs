//! A pilot who flies faster than their view is aimed keeps the ship they are aboard.
//!
//! Found by the owner, who left the dev rocket at full throttle for an hour and came back 41 Mm out with no rocket in sight, the HUD still telling the rocket's state and the speed still
//! rising. A client watches a box of bricks round its camera and asks for a new one when the camera's brick changes, which the server answers a tick later; at about 3.9 kilometres a second
//! (6 bricks a tick) the ship was out of the box by the time the server looked, so the server told the client to forget it. A player aboard a ship the client does not know is placed nowhere
//! the client knows, so the camera stopped, so the box never moved again, and the ship never came back: measured, from the tick it was lost to the end of the flight.

mod common;

use common::rocket::{
    a_ship_with_a_seat_and_four_engines_over_the_planet, sit_and_let_go_going_up, state_of,
};
use common::{Scene, eyes_in_the_world, length, minus, tick_and_follow, voxels_of};

#[test]
fn a_pilot_flying_faster_than_the_box_of_bricks_they_watch_is_moved_keeps_their_ship_and_their_camera_goes_with_it()
 {
    let (mut scene, mut player, grid) =
        a_ship_with_a_seat_and_four_engines_over_the_planet(Scene::in_a_new_world());
    // 4,500 voxels a second is 225 a tick, 7 bricks, and the box is 5 bricks and a margin of 5 round the camera's brick.
    sit_and_let_go_going_up(&mut scene, &mut player, grid, 4500.0);
    let (begun_at, _, _) = state_of(&scene, grid);
    for tick in 0..120 {
        tick_and_follow(&mut scene, &mut player, 1);
        // (A window does this every frame; the tests' players do it when they are asked to.)
        let _ = player.update_view();
        assert!(
            player.client().world().grids().get(grid).is_some(),
            "the client had forgotten the ship by tick {tick}"
        );
        assert!(
            player.own_server_position().is_some(),
            "the client could not place its own player by tick {tick}"
        );
    }
    let (ended_at, _, _) = state_of(&scene, grid);
    let travelled = length(minus(ended_at, begun_at));
    assert!(travelled > 25_000.0, "the ship went {travelled} voxels");
    // The camera is where the server has the player, give or take the tick or two it is behind by (225 voxels a tick).
    let behind = length(minus(
        eyes_in_the_world(&scene, 1),
        voxels_of(&player.camera),
    ));
    assert!(
        behind < 1_000.0,
        "the camera is {behind} voxels behind the player"
    );
}
