//! Stepping off a ship: a walker who leaves one keeps the speed it gave them. The air does not take it from them at 14 voxels a second a second because their keys ask for a walk
//! (measured from the world, a ship in orbit goes at 500 to 700 voxels a second, and a walker who stepped off one was left behind at once: found by the owner, 2026-11-05), until they
//! stand on something again.

mod common;

use common::{
    PlayerExt, Scene, eyes_in_the_world, make_ship, player_pose, scene_with_a_deck, ship_id,
    tick_and_follow, walker_on_the_deck,
};
use game_shell::{GameDefinition, KeyCode};
use voxel_space::{GridPose, Orientation, WorldPosition};
use voxunami_cave::Cave;
use voxunami_cave::aboard::in_voxels;

/// The speed of a circle round the planet at this distance from its middle, as the world's pull has it.
fn circular_speed(scene: &Scene, distance: f64) -> f64 {
    let settings = Cave::gravity();
    let planet = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(0))
        .expect("the planet is the first ship of a new world");
    let mass = planet.terrain.expect("it has terrain").mass;
    let gravitational_parameter = settings.pull_strength * mass;
    (gravitational_parameter * distance
        / (distance * distance + settings.softening * settings.softening))
        .sqrt()
}

/// Where the middle of ship `number` is, in voxels.
fn ship_middle(scene: &Scene, number: u64) -> [f64; 3] {
    let ship = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(number))
        .expect("the ship is there");
    let origin = in_voxels(ship.pose.position);
    [0, 1, 2].map(|axis| origin[axis] + ship.centre_of_mass[axis])
}

fn distance_between(first: [f64; 3], second: [f64; 3]) -> f64 {
    common::length(common::minus(first, second))
}

/// A deck of 30 by 2 by 8 that goes round the planet in a circle `distance` from its middle, on the rail it has been put on after a second without a push (the planet's pull
/// and the speed of the circle: the owner's ship), with a walker standing on it. The scene, the walker and the ship's number.
fn walker_on_a_deck_in_orbit(distance: f64) -> (Scene, common::Player, u64) {
    let mut scene = Scene::in_a_new_world();
    let speed = circular_speed(&scene, distance);
    // Held while the walker falls onto it, straight over the middle of the planet, so that the deck's top is the way up.
    make_ship(
        &mut scene,
        GridPose::new(
            WorldPosition::from_voxels([-15.0, distance, -4.0]),
            Orientation::IDENTITY,
        ),
        true,
        [30, 2, 8],
        |_| true,
    );
    let number = 1;
    let mut player = scene
        .rig
        .join_walking([0.0, distance + 12.0, 0.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 80);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(number), "aboard it");
    scene.run(move |world| {
        world.set_grid_held(ship_id(number), false).unwrap();
        world
            .set_grid_velocity(ship_id(number), [speed, 0.0, 0.0], [0.0; 3])
            .unwrap();
    });
    tick_and_follow(&mut scene, &mut player, 120);
    let ship = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(number))
        .unwrap();
    assert!(ship.rail.is_some(), "after six seconds it is on its rail");
    assert_eq!(player_pose(&scene, 1).grid, ship_id(number), "still aboard");
    (scene, player, number)
}

#[test]
fn a_walker_who_steps_off_a_ship_in_orbit_stays_beside_it_and_is_not_left_behind() {
    for distance in [80_000.0, 140_000.0] {
        let (mut scene, mut player, number) = walker_on_a_deck_in_orbit(distance);
        // Walk off the side (yaw 0 is toward -z; the deck is 4 from its middle to the side), and on for two seconds after the first tick in the level.
        player.hold(&[KeyCode::KeyW]);
        let mut ticks_in_the_level = 0;
        for _ in 0..200 {
            tick_and_follow(&mut scene, &mut player, 1);
            if player_pose(&scene, 1).grid.root_level().is_some() {
                ticks_in_the_level += 1;
            }
            if ticks_in_the_level == 40 {
                break;
            }
        }
        assert_eq!(ticks_in_the_level, 40, "they walked off the deck");
        // Let go of the keys, and ten seconds.
        player.hold(&[]);
        tick_and_follow(&mut scene, &mut player, 200);
        let eyes = eyes_in_the_world(&scene, 1);
        let apart = distance_between(eyes, ship_middle(&scene, number));
        // They walked about 4 meters over the side and 9 more in the two seconds, and then stood in the air beside it. (Held to the world's standstill the air took 14 voxels a second a
        // second off the ship's speed, and 12 seconds later they were 1,000 voxels behind it.)
        assert!(
            apart < 40.0,
            "at {distance}: {apart} voxels from the ship, who is at {:?}",
            ship_middle(&scene, number)
        );
        // And they do not drift off: a second later they are as far (to a few voxels).
        tick_and_follow(&mut scene, &mut player, 20);
        let later = distance_between(eyes_in_the_world(&scene, 1), ship_middle(&scene, number));
        assert!(
            (later - apart).abs() < 6.0,
            "at {distance}: {apart} then {later}"
        );
    }
}

/// A flat floor, a deck held 3 over it that is said to go at 6 voxels a second along x (it is held, so it does not move: only what it gives a walker who leaves it is tested), and a
/// walker on the deck.
fn walker_on_a_deck_that_goes_at_six() -> (Scene, common::Player) {
    let mut scene = scene_with_a_deck();
    let player = walker_on_the_deck(&mut scene);
    scene.run(|world| {
        world
            .set_grid_velocity(ship_id(0), [6.0, 0.0, 0.0], [0.0; 3])
            .unwrap();
    });
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0), "aboard it");
    (scene, player)
}

/// Walks the walker toward -z off the side of the deck, and returns the tick on which they are in the level.
fn walk_off(scene: &mut Scene, player: &mut common::Player) {
    player.hold(&[KeyCode::KeyW]);
    for _ in 0..200 {
        tick_and_follow(scene, player, 1);
        if player_pose(scene, 1).grid.root_level().is_some() {
            player.hold(&[]);
            return;
        }
    }
    panic!("they did not walk off");
}

#[test]
fn a_walker_who_steps_off_a_ship_keeps_its_speed_in_the_air_and_stops_on_the_ground() {
    let (mut scene, mut player) = walker_on_a_deck_that_goes_at_six();
    walk_off(&mut scene, &mut player);
    let left_at = eyes_in_the_world(&scene, 1);
    // In the air, over the next second, they go on at the speed of the deck (6) and not at the 6 less 14 a second a second of the air of the world.
    tick_and_follow(&mut scene, &mut player, 20);
    let a_second_on = eyes_in_the_world(&scene, 1);
    assert!(
        a_second_on[0] - left_at[0] > 4.5,
        "the deck's speed is kept in the air: {left_at:?} then {a_second_on:?}"
    );
    // On the floor they are held by it (the ground takes the speed of the deck from them): two seconds later they are not still going.
    tick_and_follow(&mut scene, &mut player, 40);
    let on_the_floor = eyes_in_the_world(&scene, 1);
    tick_and_follow(&mut scene, &mut player, 20);
    let a_second_later = eyes_in_the_world(&scene, 1);
    assert!(
        on_the_floor[1] < common::FLOOR + 3.0,
        "they are on the floor: {on_the_floor:?}"
    );
    assert!(
        (a_second_later[0] - on_the_floor[0]).abs() < 0.5,
        "they stand: {on_the_floor:?} then {a_second_later:?}"
    );
}

#[test]
fn a_walker_who_stepped_off_a_ship_that_is_taken_away_is_braked_by_the_air_as_every_walker_was() {
    let (mut scene, mut player) = walker_on_a_deck_that_goes_at_six();
    walk_off(&mut scene, &mut player);
    scene.run(|world| {
        world.remove_grid(ship_id(0)).unwrap();
    });
    let before = eyes_in_the_world(&scene, 1);
    tick_and_follow(&mut scene, &mut player, 12);
    let after = eyes_in_the_world(&scene, 1);
    // With nothing to measure from, the speed they left with is the world's, and the air takes it from them: in 0.6 seconds they go less than the 3.6 voxels the deck's speed would
    // have taken them (1.3 or so; with the deck's speed kept they would go all 3.6).
    assert!(
        after[0] - before[0] < 2.5,
        "braked by the air: {before:?} then {after:?}"
    );
}
