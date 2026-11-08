//! Buckling: `/ship buckle` holds a player where they stand in the ship they are aboard, as they are turned, so that they cannot fall off it; `/ship unbuckle` lets them go.

mod common;

use common::{
    PlayerExt, Scene, each_tick, eyes_in_the_world, player_pose, say, scene_with_a_deck, ship_id,
    ship_pose, tick_and_follow, walker_on_the_deck,
};
use game_shell::KeyCode;
use voxel_space::{GridPose, Orientation};
use voxunami_cave::aboard::in_voxels;

/// A turn of `angle` radians about the z axis.
fn roll(angle: f64) -> Orientation {
    Orientation::from_components([0.0, 0.0, libm::sin(angle / 2.0), libm::cos(angle / 2.0)])
        .unwrap()
}

#[test]
fn a_player_who_is_not_aboard_a_ship_is_told_to_stand_on_one() {
    let mut scene = Scene::new();
    common::flat_floor(&mut scene);
    let mut player = scene
        .rig
        .join_walking([20.0, common::FLOOR + 3.0, 24.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 20);
    assert_eq!(
        say(&mut scene, &mut player, "/ship buckle"),
        "stand on a ship to buckle to it"
    );
    assert_eq!(
        say(&mut scene, &mut player, "/ship unbuckle"),
        "you are not buckled"
    );
}

#[test]
fn a_buckled_player_does_not_move_in_the_ship_whatever_their_keys_ask_and_an_unbuckled_one_does() {
    let mut scene = scene_with_a_deck();
    let mut player = walker_on_the_deck(&mut scene);
    let standing = player_pose(&scene, 1);
    assert_eq!(standing.grid, ship_id(0));
    let reply = say(&mut scene, &mut player, "/ship buckle");
    assert!(reply.starts_with("you are buckled to ship 0"), "{reply}");
    assert_eq!(
        say(&mut scene, &mut player, "/ship buckle"),
        "you are buckled to ship 0 already"
    );
    // Walking, running, sidestepping and jumping at once, for two seconds.
    player.hold(&[
        KeyCode::KeyW,
        KeyCode::KeyD,
        KeyCode::ControlLeft,
        KeyCode::Space,
    ]);
    tick_and_follow(&mut scene, &mut player, 40);
    assert_eq!(
        player_pose(&scene, 1).position,
        standing.position,
        "held in place, to the bit"
    );
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
    // Let go, the same keys walk them (the control: the keys did something before).
    assert_eq!(
        say(&mut scene, &mut player, "/ship unbuckle"),
        "you are let go of ship 0"
    );
    tick_and_follow(&mut scene, &mut player, 20);
    let walked = in_voxels(player_pose(&scene, 1).position);
    let was = in_voxels(standing.position);
    assert!(
        (walked[0] - was[0]).abs() + (walked[2] - was[2]).abs() > 3.0,
        "{was:?} {walked:?}"
    );
}

#[test]
fn a_buckled_player_is_carried_exactly_by_a_ship_that_turns_over_and_an_unbuckled_one_falls_off() {
    // Buckled: the ship is turned onto its side and then upside down, and they are where they were in it, turned with it.
    let mut scene = scene_with_a_deck();
    let mut player = walker_on_the_deck(&mut scene);
    let standing = player_pose(&scene, 1);
    say(&mut scene, &mut player, "/ship buckle");
    let origin = ship_pose(&scene, 0).position;
    each_tick(&mut scene, &mut player, 120, move |world, tick| {
        let turned = GridPose::new(origin, roll(0.03 * (tick as f64 + 1.0)));
        world.set_grid_pose(ship_id(0), turned).unwrap();
    });
    // (A turn of 3.6 radians: past upside down.)
    assert_eq!(
        player_pose(&scene, 1),
        standing,
        "the same place and view in the ship, to the bit"
    );
    let expected = in_voxels(
        ship_pose(&scene, 0)
            .local_to_world(standing.position)
            .unwrap(),
    );
    let eyes = eyes_in_the_world(&scene, 1);
    for axis in 0..3 {
        assert!(
            (expected[axis] - eyes[axis]).abs() < 1e-3,
            "{expected:?} {eyes:?}"
        );
    }

    // The same without buckling: past on its side they fall to what is now the floor, and are not where they were in the ship.
    let mut scene = scene_with_a_deck();
    let mut player = walker_on_the_deck(&mut scene);
    let standing = player_pose(&scene, 1);
    let origin = ship_pose(&scene, 0).position;
    each_tick(&mut scene, &mut player, 120, move |world, tick| {
        let turned = GridPose::new(origin, roll(0.03 * (tick as f64 + 1.0)));
        world.set_grid_pose(ship_id(0), turned).unwrap();
    });
    assert_ne!(player_pose(&scene, 1).position, standing.position);
}

#[test]
fn a_player_buckled_to_a_ship_that_goes_away_is_let_go_and_falls() {
    let mut scene = scene_with_a_deck();
    let mut player = walker_on_the_deck(&mut scene);
    say(&mut scene, &mut player, "/ship buckle");
    scene.run(|world| {
        world.remove_grid(ship_id(0)).unwrap();
    });
    tick_and_follow(&mut scene, &mut player, 60);
    assert!(player_pose(&scene, 1).grid.root_level().is_some());
    assert_eq!(
        say(&mut scene, &mut player, "/ship unbuckle"),
        "you are not buckled"
    );
    // They stand on the floor again, and walk.
    let before = in_voxels(player_pose(&scene, 1).position);
    player.hold(&[KeyCode::KeyW]);
    tick_and_follow(&mut scene, &mut player, 20);
    let after = in_voxels(player_pose(&scene, 1).position);
    assert!((after[2] - before[2]).abs() > 3.0, "{before:?} {after:?}");
}
