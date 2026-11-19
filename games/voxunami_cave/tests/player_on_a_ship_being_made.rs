//! A player who comes down on a ship that is being made stands on it.
//!
//! A ship too big for a tick is made a few bricks at a time and held meanwhile (`ship_first_build.rs`). The worry was a walker aboard it, standing where its colliders are not made yet:
//! the walker is a body of its own that meets a ship through the ship's blocks (`ShipSource` in `movement.rs`), not through the colliders the bodies plugin makes, so it should not
//! fall through; this is the test that says so, with the walker on the part of the ship that is made last.

mod common;

use common::{
    FLOOR, Scene, eyes_in_their_frame, flat_floor, make_ship, player_pose, pose_at, ship_id,
    tick_and_follow,
};

/// Bricks along x of the deck: 400, which is a hundred ticks at four a tick.
const LENGTH: i128 = 32 * 400;

#[test]
fn a_walker_who_comes_down_on_a_part_of_a_ship_that_is_not_made_yet_stands_on_it() {
    let (mut scene, report) = Scene::reporting();
    flat_floor(&mut scene);
    // A deck 12,800 long, 2 thick and 8 wide, held 3 above the floor, whose last 100 voxels are over the floor: bricks are made in order of coordinate, so the walker, who comes
    // down on them, is on the last of them to be made.
    make_ship(
        &mut scene,
        pose_at(20.0 - (LENGTH - 100) as f64, FLOOR + 3.0, 20.0),
        true,
        [LENGTH, 2, 8],
        |_| true,
    );
    assert_eq!(report().ships_being_made, 1, "the deck is being made");
    let mut player = scene
        .rig
        .join_walking([20.0, FLOOR + 5.0 + 8.0, 24.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 40);
    assert_eq!(
        report().ships_being_made,
        1,
        "the deck is still being made when the walker has come down on it"
    );
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0), "aboard the deck");
    // (The deck is 2 thick; the eyes are 1.62 above the feet.)
    let eyes = eyes_in_their_frame(&scene, 1);
    assert!(
        (eyes[1] - (2.0 + 1.62)).abs() < 0.05,
        "standing on the deck: {eyes:?}"
    );
    // Made, and the walker is where they were.
    while report().ships_being_made > 0 {
        tick_and_follow(&mut scene, &mut player, 1);
    }
    tick_and_follow(&mut scene, &mut player, 20);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
    assert_eq!(eyes_in_their_frame(&scene, 1), eyes);
}
