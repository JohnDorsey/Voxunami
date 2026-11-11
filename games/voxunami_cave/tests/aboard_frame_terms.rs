//! What a walker aboard a ship feels when the ship accelerates or turns (owner, 2026-11-10): the pull of the world at their place less the acceleration of the ship, less the
//! centrifugal pull of its turn. A ship in free fall is a place to float; a ship that burns weighs more, or less; one that cruises at any speed is as one at rest, whether or
//! not it has a rail. The ship here is a deck, held, whose place and speed the test says tick by tick (as a rail or a burn would), so that what the walker does is the walker's.
//!
//! What is measured is a jump, half a second after the key goes down: it rises `speed * t - pull * t^2 / 2`, so the pull the walker feels is read from it.

mod common;

use common::{
    CABIN_MIDDLE, FLOOR, Player, PlayerExt, Scene, about_the_middle, eyes_in_their_frame, roll,
    scene_with_a_deck, ship_id, walker_in_a_cabin, walker_on_the_deck,
};
use game_shell::KeyCode;
use voxel_space::{GravitySettings, GridPose, Orientation, WorldPosition};

/// The pull of the world in these scenes, and the seconds a tick lasts.
const PULL: f64 = 5.0;
const TICK: f64 = 0.05;
/// A jump's take-off speed (`CharacterSettings::jump_speed`).
const JUMP: f64 = 3.6;
/// Where the origin of the deck is when the script begins.
const START: [f64; 3] = [5.0, FLOOR + 3.0, 20.0];
const HALF_A_SECOND: usize = 10;

/// What the script says the deck does at the time `t` (seconds from the first tick of the script): its origin, and its velocity.
type Script = fn(f64) -> ([f64; 3], [f64; 3]);

fn at_rest(_: f64) -> ([f64; 3], [f64; 3]) {
    (START, [0.0; 3])
}

fn falling_freely(t: f64) -> ([f64; 3], [f64; 3]) {
    (
        [START[0], START[1] - 0.5 * PULL * t * t, START[2]],
        [0.0, -PULL * t, 0.0],
    )
}

fn falling_at_half_the_pull(t: f64) -> ([f64; 3], [f64; 3]) {
    let a = PULL / 2.0;
    (
        [START[0], START[1] - 0.5 * a * t * t, START[2]],
        [0.0, -a * t, 0.0],
    )
}

/// A burn straight up that gives the deck 5 metres a second a second.
fn burning_up(t: f64) -> ([f64; 3], [f64; 3]) {
    (
        [START[0], START[1] + 0.5 * PULL * t * t, START[2]],
        [0.0, PULL * t, 0.0],
    )
}

/// A burn straight up at 20 metres a second a second for five seconds, which presses the walker onto the deck, and then a cruise at 100 metres a second, which is nothing the
/// walker feels. (Made to go at 100 in a tick it would be a blow, and the walker would be thrown: a ship is made to cruise by a burn.)
fn cruising(t: f64) -> ([f64; 3], [f64; 3]) {
    let (burn, speed) = (5.0, 100.0);
    if t < burn {
        (
            [START[0], START[1] + 10.0 * t * t, START[2]],
            [0.0, 20.0 * t, 0.0],
        )
    } else {
        (
            [
                START[0],
                START[1] + 0.5 * speed * burn + speed * (t - burn),
                START[2],
            ],
            [0.0, speed, 0.0],
        )
    }
}

/// The scene, with a pull of 5 down, and a walker standing on the deck.
fn standing_on_the_deck() -> (Scene, Player) {
    let mut scene = scene_with_a_deck();
    scene.run(|world| {
        world.set_gravity_settings(GravitySettings {
            base: [0.0, -PULL, 0.0],
            ..GravitySettings::default()
        });
    });
    let player = walker_on_the_deck(&mut scene);
    assert_eq!(common::player_pose(&scene, 1).grid, ship_id(0), "aboard");
    (scene, player)
}

/// Runs `ticks` ticks in which the deck does what `script` says, the time counted from the first tick of the script, of which `done` have been run already.
fn run_script(scene: &mut Scene, player: &mut Player, script: Script, done: usize, ticks: usize) {
    common::each_tick(scene, player, ticks, move |world, tick| {
        let (origin, velocity) = script((done + tick + 1) as f64 * TICK);
        world
            .set_grid_pose(
                ship_id(0),
                GridPose::new(
                    WorldPosition::new(voxel_space::BrickCoordinate::ORIGIN, origin),
                    Orientation::IDENTITY,
                ),
            )
            .unwrap();
        world
            .set_grid_velocity(ship_id(0), velocity, [0.0; 3])
            .unwrap();
    });
}

/// How far the walker's eyes rise in the ship in `ticks` ticks of the script after the jump key goes down, the ship having gone on by the script for `lead_in` ticks before (so that
/// one that goes at a speed is going at it when the key goes down, and was not made to in a tick: a blow).
fn rise_of_a_jump(script: Script, lead_in: usize, ticks: usize) -> f64 {
    let (mut scene, mut player) = standing_on_the_deck();
    run_script(&mut scene, &mut player, script, 0, lead_in);
    let standing = eyes_in_their_frame(&scene, 1)[1];
    player.hold(&[KeyCode::Space]);
    run_script(&mut scene, &mut player, script, lead_in, ticks);
    assert_eq!(
        common::player_pose(&scene, 1).grid,
        ship_id(0),
        "still aboard"
    );
    eyes_in_their_frame(&scene, 1)[1] - standing
}

/// What a jump rises in `t` seconds in a pull `pull`, with the steps the walker takes (60 a second, the pull taken off the speed before the move).
fn expected_rise(pull: f64, ticks: usize) -> f64 {
    let (mut speed, mut height) = (JUMP, 0.0);
    for _ in 0..ticks * 3 {
        speed -= pull / 60.0;
        height += speed / 60.0;
    }
    height
}

fn assert_rise(script: Script, lead_in: usize, pull: f64, what: &str) {
    let rise = rise_of_a_jump(script, lead_in, HALF_A_SECOND);
    let expected = expected_rise(pull, HALF_A_SECOND);
    assert!(
        (rise - expected).abs() < 0.08,
        "{what}: the jump rose {rise} and a pull of {pull} would give {expected}"
    );
}

#[test]
fn a_jump_on_a_deck_at_rest_is_a_jump_in_the_pull_of_the_world() {
    assert_rise(at_rest, 0, PULL, "at rest");
}

#[test]
fn a_jump_on_a_deck_that_cruises_at_a_hundred_metres_a_second_is_the_same() {
    // (After the burn, which is a hundred ticks, and a few ticks of the cruise.)
    assert_rise(cruising, 105, PULL, "cruising");
}

#[test]
fn a_walker_aboard_a_ship_in_free_fall_is_weightless() {
    assert_rise(falling_freely, 3, 0.0, "free fall");
}

#[test]
fn a_walker_aboard_a_ship_that_falls_at_half_the_pull_weighs_half() {
    assert_rise(falling_at_half_the_pull, 3, PULL / 2.0, "falling at half");
}

#[test]
fn a_walker_aboard_a_ship_that_burns_up_at_the_pull_weighs_twice_as_much() {
    assert_rise(burning_up, 3, 2.0 * PULL, "burning");
}

#[test]
fn standing_still_in_a_ship_that_falls_freely_the_walker_stays_where_they_are_in_it() {
    let (mut scene, mut player) = standing_on_the_deck();
    let standing = eyes_in_their_frame(&scene, 1);
    run_script(
        &mut scene,
        &mut player,
        falling_freely,
        0,
        3 * HALF_A_SECOND,
    );
    let later = eyes_in_their_frame(&scene, 1);
    for axis in 0..3 {
        assert!(
            (later[axis] - standing[axis]).abs() < 0.02,
            "{standing:?} {later:?}"
        );
    }
    assert_eq!(common::player_pose(&scene, 1).grid, ship_id(0), "aboard");
}

#[test]
fn a_walker_aboard_a_ship_that_stops_burning_is_pressed_to_the_deck_no_more() {
    // The burn ends: the walker weighed twice as much for a second and now weighs what the world gives them. (The speed the burn gave the ship is not an acceleration.)
    let (mut scene, mut player) = standing_on_the_deck();
    run_script(&mut scene, &mut player, burning_up, 0, 2 * HALF_A_SECOND);
    fn coasting_on(t: f64) -> ([f64; 3], [f64; 3]) {
        // From the end of the burn, at the speed it had, in free fall under the pull of the world: the walker is weightless, as in any ship that falls.
        let end = 2.0 * 0.5;
        let (origin, velocity) = burning_up(end);
        let after = t - end;
        (
            [
                origin[0],
                origin[1] + velocity[1] * after - 0.5 * PULL * after * after,
                origin[2],
            ],
            [0.0, velocity[1] - PULL * after, 0.0],
        )
    }
    // (Time counts from the first tick of the first script, so the second script starts at the end of the first.)
    let standing = eyes_in_their_frame(&scene, 1)[1];
    common::each_tick(
        &mut scene,
        &mut player,
        HALF_A_SECOND,
        move |world, tick| {
            let (origin, velocity) = coasting_on(1.0 + (tick + 1) as f64 * TICK);
            world
                .set_grid_pose(
                    ship_id(0),
                    GridPose::new(
                        WorldPosition::new(voxel_space::BrickCoordinate::ORIGIN, origin),
                        Orientation::IDENTITY,
                    ),
                )
                .unwrap();
            world
                .set_grid_velocity(ship_id(0), velocity, [0.0; 3])
                .unwrap();
        },
    );
    let later = eyes_in_their_frame(&scene, 1)[1];
    assert!(
        (later - standing).abs() < 0.05,
        "still on the deck: {standing} {later}"
    );
    assert_eq!(common::player_pose(&scene, 1).grid, ship_id(0), "aboard");
}

/// A ship that falls freely, with a residual that is not zero: the pull of a ship in a rail is the pull of the world at the walker less the ship's own acceleration, and what is
/// left is the tide and the rounding of the speeds, a ten-thousandth of a metre a second a second, whose direction is nothing. It must not be taken for a down.
fn falling_with_a_residual(t: f64) -> ([f64; 3], [f64; 3]) {
    let a = 0.001;
    (
        [
            START[0] + 0.5 * a * t * t,
            START[1] - 0.5 * PULL * t * t,
            START[2],
        ],
        [a * t, -PULL * t, 0.0],
    )
}

#[test]
fn a_residual_pull_of_a_thousandth_in_a_ship_in_free_fall_is_no_down_and_the_walker_is_not_turned_or_put_out()
 {
    let (mut scene, mut player) = standing_on_the_deck();
    let standing = eyes_in_their_frame(&scene, 1);
    // (Four seconds, in which a pull taken for a down would have turned the walker's floor to the side and put them out of the ship.)
    run_script(&mut scene, &mut player, falling_with_a_residual, 0, 80);
    let later = eyes_in_their_frame(&scene, 1);
    assert_eq!(common::player_pose(&scene, 1).grid, ship_id(0), "aboard");
    for axis in 0..3 {
        assert!(
            (later[axis] - standing[axis]).abs() < 0.02,
            "{standing:?} {later:?}"
        );
    }
    // And a jump from there is a jump in nothing: the up they had.
    player.hold(&[KeyCode::Space]);
    run_script(
        &mut scene,
        &mut player,
        falling_with_a_residual,
        80,
        HALF_A_SECOND,
    );
    let risen = eyes_in_their_frame(&scene, 1)[1] - standing[1];
    assert!(
        (risen - expected_rise(0.0, HALF_A_SECOND)).abs() < 0.08,
        "{risen}"
    );
}

#[test]
fn being_let_go_in_the_middle_of_a_burn_is_no_blow_the_ship_has_been_followed_while_they_were_buckled()
 {
    let (mut scene, mut player) = standing_on_the_deck();
    let standing = eyes_in_their_frame(&scene, 1);
    let reply = common::say(&mut scene, &mut player, "/ship buckle");
    assert!(reply.starts_with("you are buckled"), "{reply}");
    // A second of a burn that gives the ship five metres a second: it is 100 metres a second a second to a walker whose last measure of it was the ship at rest.
    run_script(&mut scene, &mut player, burning_up, 0, 2 * HALF_A_SECOND);
    let reply = common::say(&mut scene, &mut player, "/ship unbuckle");
    assert!(reply.starts_with("you are let go"), "{reply}");
    run_script(
        &mut scene,
        &mut player,
        burning_up,
        2 * HALF_A_SECOND,
        HALF_A_SECOND,
    );
    let later = eyes_in_their_frame(&scene, 1);
    assert_eq!(common::player_pose(&scene, 1).grid, ship_id(0), "aboard");
    for axis in 0..3 {
        assert!(
            (later[axis] - standing[axis]).abs() < 0.03,
            "{standing:?} {later:?}"
        );
    }
}

#[test]
fn a_cabin_on_its_side_that_falls_freely_is_weightless_whichever_way_it_is_turned() {
    // The acceleration of the ship is in the world's axes and the walker's pull is in the ship's: the cabin is turned a quarter, so that the world's down is the ship's -x (the walker
    // stands on the wall that is the floor now), and it falls. A ship's acceleration that was not turned into its axes would be a pull of seven across the walker.
    let mut scene = Scene::new();
    scene.run(|world| {
        world.set_gravity_settings(GravitySettings {
            base: [0.0, -PULL, 0.0],
            ..GravitySettings::default()
        });
    });
    let mut player = walker_in_a_cabin(&mut scene);
    let quarter = std::f64::consts::FRAC_PI_2;
    let on_its_side = move |centre: [f64; 3]| about_the_middle(centre, [4.0; 3], roll(quarter));
    common::each_tick(&mut scene, &mut player, 90, move |world, tick| {
        let angle = quarter * (tick as f64 + 1.0) / 90.0;
        world
            .set_grid_pose(
                ship_id(0),
                about_the_middle(CABIN_MIDDLE, [4.0; 3], roll(angle)),
            )
            .unwrap();
    });
    common::tick_and_follow(&mut scene, &mut player, 80);
    let standing = eyes_in_their_frame(&scene, 1);
    assert_eq!(common::player_pose(&scene, 1).grid, ship_id(0), "aboard");
    // It falls for two seconds, the walker standing on the wall as before.
    common::each_tick(&mut scene, &mut player, 40, move |world, tick| {
        let t = (tick + 1) as f64 * TICK;
        let centre = [
            CABIN_MIDDLE[0],
            CABIN_MIDDLE[1] - 0.5 * PULL * t * t,
            CABIN_MIDDLE[2],
        ];
        world
            .set_grid_pose(ship_id(0), on_its_side(centre))
            .unwrap();
        world
            .set_grid_velocity(ship_id(0), [0.0, -PULL * t, 0.0], [0.0; 3])
            .unwrap();
    });
    let later = eyes_in_their_frame(&scene, 1);
    assert_eq!(
        common::player_pose(&scene, 1).grid,
        ship_id(0),
        "still aboard"
    );
    for axis in 0..3 {
        assert!(
            (later[axis] - standing[axis]).abs() < 0.03,
            "{standing:?} {later:?}"
        );
    }
    // And a jump from the wall goes along the ship's x, up from the wall, at the speed of a jump and with no pull: it rises 1.8 in half a second.
    player.hold(&[KeyCode::Space]);
    common::each_tick(
        &mut scene,
        &mut player,
        HALF_A_SECOND,
        move |world, tick| {
            let t = (40 + tick + 1) as f64 * TICK;
            let centre = [
                CABIN_MIDDLE[0],
                CABIN_MIDDLE[1] - 0.5 * PULL * t * t,
                CABIN_MIDDLE[2],
            ];
            world
                .set_grid_pose(ship_id(0), on_its_side(centre))
                .unwrap();
            world
                .set_grid_velocity(ship_id(0), [0.0, -PULL * t, 0.0], [0.0; 3])
                .unwrap();
        },
    );
    let risen = eyes_in_their_frame(&scene, 1)[0] - standing[0];
    assert!(
        (risen - expected_rise(0.0, HALF_A_SECOND)).abs() < 0.1,
        "{risen}"
    );
}
