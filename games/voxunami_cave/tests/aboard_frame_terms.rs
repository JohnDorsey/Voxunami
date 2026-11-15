//! What a walker aboard a ship feels when the ship accelerates or turns (owner, 2026-11-10): the pull of the world at their place less the acceleration of the ship, less the
//! centrifugal pull of its turn. A ship in free fall is a place to float; a ship that burns weighs more, or less; one that cruises at any speed is as one at rest, whether or
//! not it has a rail. The ship here is a deck, held, whose place and speed the test says tick by tick (as a rail or a burn would), so that what the walker does is the walker's.
//!
//! What is measured is a jump, half a second after the key goes down: it rises `speed * t - pull * t^2 / 2`, so the pull the walker feels is read from it.
//!
//! At the end are the tests of two things that must make no difference to what the walker does: the velocity of the ship through the world, and the id of the ship.

mod common;

use common::{
    CABIN_MIDDLE, FLOOR, Player, PlayerExt, Scene, about_the_middle, eyes_in_the_world,
    eyes_in_their_frame, roll, scene_with_a_deck, ship_id, walker_in_a_cabin, walker_on_the_deck,
};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use game_shell::KeyCode;
use server::{ServerWorld, SplitPart};
use voxel_space::{GravitySettings, GridId, GridPose, Orientation, VoxelCoordinate, WorldPosition};

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

// ---- A ship that turns ---------------------------------------------------------------------------------------------------------------------------------------------------------
//
// The deck is held and its pose does not turn; what the test says of it is its spin (as it said its speed above), so that what is measured is what the walker does with the spin.
// A walker who jumps from the floor of a deck that spins about its z, with the centre of mass of the deck 2.63 below their eyes, is pulled up by the centrifugal pull, pushed
// sideways by the Euler pull while the spin grows, and has the speed of their jump turned by the Coriolis turn. `replica` is the walk the rules say, step by step and written
// apart from the code that does it. (A walker in a pull steers in the air as ever, which is what air does in a ship: it takes a sideways speed that nothing keeps up off them at
// fourteen metres a second a second; the Coriolis turn is seen where there is none, in a ship in free fall.)

use voxunami_cave::frame_terms::{FrameMotion, cross, pull_in_a_frame};

/// The spin the deck is said to have in the tick `tick` of a script (the tick of the jump is 0), about its z.
type Spin = fn(usize) -> f64;

/// Where the walker falls onto the deck, along x in the world: the middle of the deck (15 along it), and 10 farther along.
const MIDDLE: f64 = 20.0;
const FAR_ALONG: f64 = 30.0;

/// A steady spin of 0.8 radians a second.
fn steadily(_: usize) -> f64 {
    0.8
}

/// A steady spin of 0.1: the centrifugal pull of it, at the 2.6 to 4.5 from the centre that a jump takes the walker through, is 0.026 to 0.045, under what anyone could feel
/// (0.05: the walker floats), and the Coriolis turn of a jump is 0.72.
fn slowly(_: usize) -> f64 {
    0.1
}

/// A spin that grows by 0.01 a tick (0.2 radians a second a second) from nothing.
fn growing(tick: usize) -> f64 {
    0.01 * (tick as f64 + 1.0)
}

fn turn_by(vector: [f64; 3], turn: [f64; 3]) -> [f64; 3] {
    let angle = length_of(turn);
    if angle == 0.0 {
        return vector;
    }
    let axis = turn.map(|part| part / angle);
    let (sin, cos) = (libm::sin(angle), libm::cos(angle));
    let along = axis[0] * vector[0] + axis[1] * vector[1] + axis[2] * vector[2];
    let across = cross(axis, vector);
    [0, 1, 2].map(|k| vector[k] * cos + across[k] * sin + axis[k] * along * (1.0 - cos))
}

fn length_of(vector: [f64; 3]) -> f64 {
    (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt()
}

/// Where the eyes of a walker who stood at `eyes` (the centre of mass of the deck at `centre`) and jumps in the first step are after `ticks` ticks of a deck that spins by `spin`
/// (the spin before the first of them being `before`) and, if `falling`, falls freely, by the rules: each tick a pull from the field and the frame at the place it starts in
/// (none, if nobody could feel it); three steps of a sixtieth, in each of which the speed is turned the other way at twice the spin of the deck, a walker in a pull has the air take what is
/// across their way up off them at fourteen a second a second, the jump is made if it is the first, the pull acts along the way up that is against it, and the walker moves.
fn replica(
    eyes: [f64; 3],
    centre: [f64; 3],
    spin: Spin,
    before: f64,
    falling: bool,
    ticks: usize,
) -> [f64; 3] {
    let (mut place, mut velocity) = (eyes, [0.0; 3]);
    let step = 1.0 / 60.0;
    for tick in 0..ticks {
        let now = [0.0, 0.0, spin(tick)];
        let then = [0.0, 0.0, if tick == 0 { before } else { spin(tick - 1) }];
        let motion = FrameMotion {
            acceleration: if falling { [0.0, -PULL, 0.0] } else { [0.0; 3] },
            spin: now,
            spin_change: [0, 1, 2].map(|k| (now[k] - then[k]) / TICK),
        };
        let arm = [0, 1, 2].map(|k| place[k] - centre[k]);
        let mut pull = pull_in_a_frame([0.0, -PULL, 0.0], &motion, arm);
        let mut strength = length_of(pull);
        if strength < 0.05 {
            pull = [0.0; 3];
            strength = 0.0;
        }
        // (With no pull the way up is the way it was: the y of the deck.)
        let up = if strength == 0.0 {
            [0.0, 1.0, 0.0]
        } else {
            pull.map(|part| -part / strength)
        };
        for substep in 0..3 {
            velocity = turn_by(velocity, now.map(|part| -2.0 * part * step));
            let mut along = velocity[0] * up[0] + velocity[1] * up[1] + velocity[2] * up[2];
            let mut across = [0, 1, 2].map(|k| velocity[k] - up[k] * along);
            // The air: in a pull, or on the floor at the first step.
            if strength > 0.0 || (tick == 0 && substep == 0) {
                let braked = length_of(across);
                let by = 14.0 * step;
                across = if braked <= by {
                    [0.0; 3]
                } else {
                    across.map(|part| part * (1.0 - by / braked))
                };
            }
            if tick == 0 && substep == 0 {
                along = JUMP;
            }
            along -= strength * step;
            velocity = [0, 1, 2].map(|k| across[k] + up[k] * along);
            place = [0, 1, 2].map(|k| place[k] + velocity[k] * step);
        }
    }
    place
}

/// The centre of mass of the deck, in its own coordinates.
fn centre_of_the_deck(scene: &Scene) -> [f64; 3] {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(0))
        .expect("the deck is there")
        .centre_of_mass
}

/// A walker standing on the deck, which was already turning by `spin` about its z when they came (the walker does not meet the spin getting going: that is the Euler pull's).
fn standing_on_a_deck_that_spins(spin: f64, at: f64) -> (Scene, Player) {
    let mut scene = scene_with_a_deck();
    scene.run(move |world| {
        world.set_gravity_settings(GravitySettings {
            base: [0.0, -PULL, 0.0],
            ..GravitySettings::default()
        });
        world
            .set_grid_velocity(ship_id(0), [0.0; 3], [0.0, 0.0, spin])
            .unwrap();
    });
    // (They fall onto the deck from 8 above it, at `at` along x in the world; the deck begins at x = 5.)
    let mut player = scene
        .rig
        .join_walking([at, FLOOR + 5.0 + 8.0, 24.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    common::tick_and_follow(&mut scene, &mut player, 60);
    assert_eq!(common::player_pose(&scene, 1).grid, ship_id(0), "aboard");
    // They came down onto a floor that goes at two metres a second under them, and take a while to be still on it, a while that depends on when the terrain was made: until
    // they have not moved for ten ticks.
    let mut last = eyes_in_their_frame(&scene, 1);
    let mut still_for = 0;
    for _ in 0..600 {
        common::tick_and_follow(&mut scene, &mut player, 1);
        let now = eyes_in_their_frame(&scene, 1);
        still_for = if now == last { still_for + 1 } else { 0 };
        last = now;
        if still_for >= 10 {
            break;
        }
    }
    assert!(still_for >= 10, "they did not come to rest on the deck");
    (scene, player)
}

/// Says in each tick that the deck turns by `spin` (the tick of the jump being 0), and falls freely if `falling` (from the tick of the jump).
fn driving(
    spin: Spin,
    falling: bool,
) -> impl Fn(&mut server::ServerWorld, usize) + Clone + Send + 'static {
    move |world, tick| {
        let t = (tick + 1) as f64 * TICK;
        let (origin, velocity) = if falling {
            falling_freely(t)
        } else {
            (START, [0.0; 3])
        };
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
            .set_grid_velocity(ship_id(0), velocity, [0.0, 0.0, spin(tick)])
            .unwrap();
    }
}

/// The walker's eyes after a jump over half a second of a deck that does as `spin` and `falling` say, with what the rules say of it: the eyes before, after, and by the rules.
fn jump_on_a_turning_deck(
    spin: Spin,
    before: f64,
    falling: bool,
    at: f64,
    sideways: f64,
) -> ([f64; 3], [f64; 3], [f64; 3], [f64; 3]) {
    let (mut scene, mut player) = standing_on_a_deck_that_spins(before, at);
    let centre = centre_of_the_deck(&scene);
    let standing = eyes_in_their_frame(&scene, 1);
    player.hold(&[KeyCode::Space]);
    common::each_tick(
        &mut scene,
        &mut player,
        HALF_A_SECOND,
        driving(spin, falling),
    );
    let went = eyes_in_their_frame(&scene, 1);
    let expected = replica(standing, centre, spin, before, falling, HALF_A_SECOND);
    assert_eq!(common::player_pose(&scene, 1).grid, ship_id(0), "aboard");
    // (`sideways` is how near the walk across the way up must be: where the way up leans the body is in the floor, and the clearing of it is not in the rules here.)
    for (axis, tolerance) in [sideways, 0.02, 0.02].into_iter().enumerate() {
        assert!(
            (went[axis] - expected[axis]).abs() < tolerance,
            "the walker went from {standing:?} to {went:?}, and the rules say {expected:?}"
        );
    }
    (standing, went, expected, centre)
}

#[test]
fn a_jump_on_a_deck_that_spins_steadily_is_pulled_up_by_the_centrifugal_pull() {
    let (standing, went, _, _) = jump_on_a_turning_deck(steadily, 0.8, false, MIDDLE, 0.02);
    // The deck is held down less: 5 less 0.64 * 2.63 = 1.7 up, and a jump goes up by more than the 1.15 of an unturning deck.
    assert!(went[1] - standing[1] > 1.3, "{standing:?} {went:?}");
}

#[test]
fn a_jump_while_the_spin_of_the_deck_is_growing_is_pulled_down_by_the_euler_pull_of_a_walker_who_is_off_the_axis()
 {
    // Ten voxels along the deck from its centre of mass, the floor there is being got going upward as the spin grows (toward +y for a spin about +z, at +x): the walker is pulled
    // down by 0.2 * 10 = 2 more, and a jump rises 0.9 and not 1.15. (A little sideways too: the way up leans five degrees.)
    let (standing, went, _, _) = jump_on_a_turning_deck(growing, 0.0, false, FAR_ALONG, 0.2);
    assert!(went[1] - standing[1] < 1.0, "{standing:?} {went:?}");
}

#[test]
fn a_jump_in_a_ship_that_falls_freely_and_turns_slowly_has_its_speed_turned_by_the_coriolis_turn() {
    let (standing, went, _, centre) = jump_on_a_turning_deck(slowly, 0.1, true, MIDDLE, 0.02);
    // Floating (the pull is under what anyone could feel) the walker has nothing to take the sideways off them: a jump at 3.6 in a ship that turns at 0.1 about z is turned
    // toward +x by 2 * 0.1 * 3.6 = 0.72 metres a second a second, nine centimetres or so in half a second.
    assert!(went[0] - standing[0] > 0.06, "{standing:?} {went:?}");
    // It rises as high as the jump goes in nothing.
    assert!(
        (went[1] - standing[1] - 1.8).abs() < 0.1,
        "{standing:?} {went:?}"
    );
    // And it is what is seen from outside: the walker goes in a straight line at the speed they left the floor with, which was the floor's own (the spin crossed with the arm) and
    // the jump; the deck turns under that line, and the walker sees it turned back by the angle the deck has turned. No step of the game, and nothing of the rules above, is in this.
    let arm = [0, 1, 2].map(|k| standing[k] - centre[k]);
    let spin = slowly(0);
    let outside = [-spin * arm[1], JUMP + spin * arm[0], 0.0];
    let seconds = HALF_A_SECOND as f64 * TICK;
    let line = [arm[0] + outside[0] * seconds, arm[1] + outside[1] * seconds];
    let angle = spin * seconds;
    let (sin, cos) = (libm::sin(angle), libm::cos(angle));
    let inside = [
        centre[0] + line[0] * cos + line[1] * sin,
        centre[1] - line[0] * sin + line[1] * cos,
    ];
    assert!(
        (went[0] - inside[0]).abs() < 0.015 && (went[1] - inside[1]).abs() < 0.03,
        "the walker is at {went:?}; seen from outside they are at {inside:?}"
    );
}

#[test]
fn being_let_go_while_the_ship_spins_up_is_no_blow_the_spin_has_been_followed_while_they_were_buckled()
 {
    // Ten voxels from the axis, where a spin getting going is a pull along the way up: a spin that grows to -0.2 in two ticks is -4 radians a second a second, and 40 metres a second a
    // second up for a walker who is let go not knowing that the spin was got going (a kick of two metres a second, which lifts them off the floor).
    let (mut scene, mut player) = standing_on_a_deck_that_spins(0.0, FAR_ALONG);
    let standing = eyes_in_their_frame(&scene, 1);
    let spinning = |world: &mut server::ServerWorld, _: usize| {
        world
            .set_grid_velocity(ship_id(0), [0.0; 3], [0.0, 0.0, -0.2])
            .unwrap();
    };
    common::carried_through(&mut scene, &mut player, 2, spinning);
    // (The kick is up and over in a second, and the walker lands again where they were: the highest they get is what shows it.)
    let mut highest = standing[1];
    for _ in 0..20 {
        common::each_tick(&mut scene, &mut player, 1, spinning);
        highest = highest.max(eyes_in_their_frame(&scene, 1)[1]);
    }
    assert_eq!(common::player_pose(&scene, 1).grid, ship_id(0), "aboard");
    assert!(
        highest - standing[1] < 0.1,
        "they were lifted {} off the floor",
        highest - standing[1]
    );
    let later = eyes_in_their_frame(&scene, 1);
    for (axis, tolerance) in [0.1, 0.05, 0.05].into_iter().enumerate() {
        assert!(
            (later[axis] - standing[axis]).abs() < tolerance,
            "{standing:?} {later:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------
// Two things that must make no difference (docs/motion-in-frames-options.md, option 1). They state, as tests, that a walker's motion aboard a ship is their motion through the
// universe less the ship's, whatever the ship's velocity is and whatever the ship's id is. Neither looks at how the walker's motion is stored; both read only the jump.
// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------

/// The ship the script drives, by the id it has now: a change of id in the middle of a run (a split, a renumbering) writes the new one here.
type ShipHandle = Arc<Mutex<GridId>>;

/// Something done to the ship in the server in the tick it is asked for (before the script poses it), which leaves the handle at the id the walker's ship has after it.
type Change = fn(&mut ServerWorld, &Mutex<GridId>);

fn the_deck() -> ShipHandle {
    Arc::new(Mutex::new(ship_id(0)))
}

/// Runs `ticks` ticks in which the ship at the handle does what `script` says (as `run_script`, the time counted from the first tick of the whole run, of which `done` have
/// been run), and `change`, with the number of the tick it is done in, is done to the ship first.
fn drive(
    scene: &mut Scene,
    player: &mut Player,
    ship: &ShipHandle,
    script: impl Fn(f64) -> ([f64; 3], [f64; 3]) + Clone + Send + 'static,
    done: usize,
    ticks: usize,
    change: Option<(usize, Change)>,
) {
    let ship = Arc::clone(ship);
    common::each_tick(scene, player, ticks, move |world, tick| {
        if let Some(change) = change
            .filter(|(at, _)| *at == tick)
            .map(|(_, change)| change)
        {
            change(world, &ship);
        }
        let id = *ship.lock().unwrap();
        let (origin, velocity) = script((done + tick + 1) as f64 * TICK);
        world
            .set_grid_pose(
                id,
                GridPose::new(
                    WorldPosition::new(voxel_space::BrickCoordinate::ORIGIN, origin),
                    Orientation::IDENTITY,
                ),
            )
            .unwrap();
        world.set_grid_velocity(id, velocity, [0.0; 3]).unwrap();
    });
}

/// What a command is answered while the script goes on for the two ticks the answer takes.
fn say_while(
    scene: &mut Scene,
    player: &mut Player,
    ship: &ShipHandle,
    script: impl Fn(f64) -> ([f64; 3], [f64; 3]) + Clone + Send + 'static,
    done: usize,
    command: &str,
) -> String {
    player.send_command(command);
    drive(scene, player, ship, script, done, 2, None);
    player.take_command_replies().join(" | ")
}

fn plus(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn times(a: [f64; 3], by: f64) -> [f64; 3] {
    a.map(|part| part * by)
}

/// The ticks of the run that gets a ship to a velocity `boost` with a walker buckled in it, and lets the walker go: two ticks at rest (the walker is buckled), forty of a steady
/// acceleration to `boost` (the walker, buckled, is held in the ship's seat through it), six of a cruise (the walker is let go in the first two), after which `script` goes on
/// with `boost` added to what it says: the ship's origin and velocity both, from where the cruise left it.
const AT_REST: usize = 2;
const RAMP: usize = 40;
const CRUISE: usize = 6;
const BEFORE_THE_SCRIPT: usize = AT_REST + RAMP + CRUISE;

fn at_a_boost(script: Script, boost: [f64; 3]) -> impl Fn(f64) -> ([f64; 3], [f64; 3]) + Clone {
    move |t| {
        let tick = (t / TICK + 0.5) as usize;
        let ramp_seconds = RAMP as f64 * TICK;
        if tick <= AT_REST {
            (START, [0.0; 3])
        } else if tick <= AT_REST + RAMP {
            let s = (tick - AT_REST) as f64 * TICK;
            (
                plus(START, times(boost, s * s / (2.0 * ramp_seconds))),
                times(boost, s / ramp_seconds),
            )
        } else if tick <= BEFORE_THE_SCRIPT {
            let s = (tick - AT_REST - RAMP) as f64 * TICK;
            (plus(START, times(boost, ramp_seconds / 2.0 + s)), boost)
        } else {
            let s = (tick - BEFORE_THE_SCRIPT) as f64 * TICK;
            let (origin, velocity) = script(s);
            let carried = ramp_seconds / 2.0 + CRUISE as f64 * TICK + s;
            (plus(origin, times(boost, carried)), plus(velocity, boost))
        }
    }
}

/// A walker on the deck is buckled, the deck is brought to a velocity `boost` (which is no acceleration they feel: they are buckled), they are let go, and `script` goes on in the
/// ship that moves at `boost`: returns the scene, the walker, and the ticks gone.
fn let_go_in_a_ship_that_moves_at(
    script: Script,
    boost: [f64; 3],
) -> (Scene, Player, ShipHandle, usize) {
    let (mut scene, mut player) = standing_on_the_deck();
    let ship = the_deck();
    let whole = at_a_boost(script, boost);
    let reply = say_while(
        &mut scene,
        &mut player,
        &ship,
        whole.clone(),
        0,
        "/ship buckle",
    );
    assert!(reply.starts_with("you are buckled"), "{reply}");
    drive(
        &mut scene,
        &mut player,
        &ship,
        whole.clone(),
        AT_REST,
        RAMP,
        None,
    );
    let reply = say_while(
        &mut scene,
        &mut player,
        &ship,
        whole.clone(),
        AT_REST + RAMP,
        "/ship unbuckle",
    );
    assert!(reply.starts_with("you are let go"), "{reply}");
    drive(
        &mut scene,
        &mut player,
        &ship,
        whole,
        AT_REST + RAMP + 2,
        CRUISE - 2,
        None,
    );
    assert_eq!(common::player_pose(&scene, 1).grid, ship_id(0), "aboard");
    (scene, player, ship, BEFORE_THE_SCRIPT)
}

/// How far the eyes rise in the ship in the half second after the jump key goes down, in a ship that moves at `boost` through the world.
fn rise_of_a_jump_at_a_boost(script: Script, lead_in: usize, boost: [f64; 3]) -> f64 {
    let (mut scene, mut player, ship, done) = let_go_in_a_ship_that_moves_at(script, boost);
    let whole = at_a_boost(script, boost);
    drive(
        &mut scene,
        &mut player,
        &ship,
        whole.clone(),
        done,
        lead_in,
        None,
    );
    let standing = eyes_in_their_frame(&scene, 1)[1];
    player.hold(&[KeyCode::Space]);
    drive(
        &mut scene,
        &mut player,
        &ship,
        whole,
        done + lead_in,
        HALF_A_SECOND,
        None,
    );
    assert_eq!(
        common::player_pose(&scene, 1).grid,
        ship_id(0),
        "still aboard"
    );
    eyes_in_their_frame(&scene, 1)[1] - standing
}

/// The ships' velocities through the world that no jump may tell from rest: along each axis a cruise (100 m/s is the oracle's), one that is very fast, one that is up, and one that is
/// none of the axes.
const BOOSTS: [[f64; 3]; 5] = [
    [30.0, 0.0, 0.0],
    [0.0, 0.0, -60.0],
    [0.0, 60.0, 0.0],
    [250.0, 0.0, 0.0],
    [40.0, 25.0, -30.0],
];

#[test]
fn a_jump_in_a_ship_that_moves_through_the_world_is_the_jump_it_is_in_a_ship_that_does_not() {
    // Every velocity gives the same physics (a Galilean boost): the rise in the ship is the same in a ship at rest, in free fall, and in a burn. (The control is the same run with
    // a boost of nothing: it is checked against the pull first, so that a run that agrees with a wrong control is not taken for right.)
    let mut disagreements = Vec::new();
    for (script, lead_in, pull, what) in [
        (at_rest as Script, 0, PULL, "at rest"),
        (falling_freely, 3, 0.0, "in free fall"),
        (burning_up, 3, 2.0 * PULL, "in a burn"),
    ] {
        let control = rise_of_a_jump_at_a_boost(script, lead_in, [0.0; 3]);
        let expected = expected_rise(pull, HALF_A_SECOND);
        assert!(
            (control - expected).abs() < 0.08,
            "{what}: the control rose {control} and a pull of {pull} would give {expected}"
        );
        for boost in BOOSTS {
            let rise = rise_of_a_jump_at_a_boost(script, lead_in, boost);
            if (rise - control).abs() > 0.02 {
                disagreements.push(format!(
                    "{what}, at {boost:?}: rose {rise}, and {control} at rest"
                ));
            }
        }
    }
    assert!(disagreements.is_empty(), "{disagreements:#?}");
}

/// How far the eyes get in a second of walking forward in a ship that moves at `boost` and does nothing else: in the ship's axes, and in the world's, from where they were.
fn a_second_of_walking_at_a_boost(boost: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let (mut scene, mut player, ship, done) = let_go_in_a_ship_that_moves_at(at_rest, boost);
    let whole = at_a_boost(at_rest, boost);
    drive(&mut scene, &mut player, &ship, whole.clone(), done, 4, None);
    let (in_ship, in_world) = (eyes_in_their_frame(&scene, 1), eyes_in_the_world(&scene, 1));
    player.hold(&[KeyCode::KeyW]);
    drive(&mut scene, &mut player, &ship, whole, done + 4, 20, None);
    let (later_in_ship, later_in_world) =
        (eyes_in_their_frame(&scene, 1), eyes_in_the_world(&scene, 1));
    assert_eq!(common::player_pose(&scene, 1).grid, ship_id(0), "aboard");
    (
        [0, 1, 2].map(|axis| later_in_ship[axis] - in_ship[axis]),
        [0, 1, 2].map(|axis| later_in_world[axis] - in_world[axis]),
    )
}

#[test]
fn a_walker_in_a_ship_that_moves_through_the_world_goes_through_the_world_as_fast_as_the_ship_and_as_fast_as_they_walk_in_it_more()
 {
    // Their velocity through the universe is the ship's plus their velocity in the ship; the second is what it is in a ship at rest.
    let (control_in_ship, _) = a_second_of_walking_at_a_boost([0.0; 3]);
    let walked = control_in_ship
        .iter()
        .map(|part| part * part)
        .sum::<f64>()
        .sqrt();
    assert!(
        walked > 3.0,
        "the premise: they walked, {control_in_ship:?}"
    );
    let mut disagreements = Vec::new();
    for boost in BOOSTS {
        let (in_ship, in_world) = a_second_of_walking_at_a_boost(boost);
        let ship_goes = times(boost, 20.0 * TICK);
        for axis in 0..3 {
            if (in_ship[axis] - control_in_ship[axis]).abs() > 0.05 {
                disagreements.push(format!(
                    "at {boost:?}: walked {in_ship:?} in the ship and {control_in_ship:?} in a ship at rest"
                ));
                break;
            }
        }
        for axis in 0..3 {
            if (in_world[axis] - ship_goes[axis] - in_ship[axis]).abs() > 0.05 {
                disagreements.push(format!(
                    "at {boost:?}: went {in_world:?} through the world, the ship {ship_goes:?} and the walker {in_ship:?} in it"
                ));
                break;
            }
        }
    }
    assert!(disagreements.is_empty(), "{disagreements:#?}");
}

/// The ship is given a new id, and nothing else (`ServerWorld::renumber_ship`).
fn renumbered(world: &mut ServerWorld, ship: &Mutex<GridId>) {
    let mut id = ship.lock().unwrap();
    *id = world.renumber_ship(*id).expect("the ship was renumbered");
}

/// The deck cut in two (it is 30 by 2 by 8 voxels): the walker stands about 15 along it, so they are on the part that is the end of it, from 10 along, which is the first of the two
/// ids that come of it; the handle follows that one.
fn split_in_two(world: &mut ServerWorld, ship: &Mutex<GridId>) {
    let part = |x: std::ops::Range<i128>| -> SplitPart {
        let voxels: BTreeSet<VoxelCoordinate> = x
            .clone()
            .flat_map(|x| {
                (0..2).flat_map(move |y| (0..8).map(move |z| VoxelCoordinate::new(x, y, z)))
            })
            .collect();
        let middle = (x.start + x.end) as f64 / 2.0;
        SplitPart {
            voxels,
            centre_of_mass: [middle, 1.0, 4.0],
        }
    };
    let mut id = ship.lock().unwrap();
    let ids = world
        .split_grid(*id, vec![part(10..30), part(0..10)])
        .expect("the deck was split");
    *id = ids[0];
}

/// The highest the eyes get in the ship in the `ticks` ticks after the jump key goes down, the ship having a `change` done to it `after` ticks after (if it has), and whether
/// they are aboard the ship that the handle names at the end.
fn highest_rise(script: Script, lead_in: usize, change: Option<(usize, Change)>) -> f64 {
    let (mut scene, mut player) = standing_on_the_deck();
    let ship = the_deck();
    drive(&mut scene, &mut player, &ship, script, 0, lead_in, None);
    let standing = eyes_in_their_frame(&scene, 1)[1];
    player.hold(&[KeyCode::Space]);
    let mut highest: f64 = 0.0;
    for tick in 0..12 {
        let change = change.and_then(|(after, change)| (after == tick).then_some((0, change)));
        drive(
            &mut scene,
            &mut player,
            &ship,
            script,
            lead_in + tick,
            1,
            change,
        );
        assert_eq!(
            common::player_pose(&scene, 1).grid,
            *ship.lock().unwrap(),
            "aboard the ship that the handle names, at {tick}"
        );
        highest = highest.max(eyes_in_their_frame(&scene, 1)[1] - standing);
    }
    highest
}

fn a_jump_whose_ship_changes_id_is_the_jump_it_would_have_been(change: Change) {
    let mut disagreements = Vec::new();
    for (script, lead_in, what) in [
        (falling_freely as Script, 3, "in free fall"),
        (burning_up, 3, "in a burn"),
    ] {
        let control = highest_rise(script, lead_in, None);
        for after in [0, 1, 2, 3, 5] {
            let highest = highest_rise(script, lead_in, Some((after, change)));
            if (highest - control).abs() > 0.02 {
                disagreements.push(format!(
                    "{what}, {after} ticks after the key: rose {highest}, and {control} with no change"
                ));
            }
        }
    }
    assert!(disagreements.is_empty(), "{disagreements:#?}");
}

#[test]
fn a_jump_whose_ship_is_given_a_new_id_in_the_air_is_the_jump_it_would_have_been() {
    a_jump_whose_ship_changes_id_is_the_jump_it_would_have_been(renumbered);
}

#[test]
fn a_jump_whose_ship_is_cut_in_two_in_the_air_is_the_jump_it_would_have_been() {
    a_jump_whose_ship_changes_id_is_the_jump_it_would_have_been(split_in_two);
}

/// A pose with the origin of the ship at `origin` and turned as `orientation` says.
fn posed(origin: [f64; 3], orientation: Orientation) -> GridPose {
    GridPose::new(
        WorldPosition::new(voxel_space::BrickCoordinate::ORIGIN, origin),
        orientation,
    )
}

/// Where the base of the merge is from the deck (whole voxels, in the world's axes): beside the deck, with its first block out of the deck's way.
const BASE_BESIDE_THE_DECK: [f64; 3] = [-2.0, 0.0, 0.0];

/// A free fall in which the deck and a larger ship beside it, turned a quarter about z, go as one, `ticks` ticks of it, of which `done` have been run (time from the first tick of the
/// whole run). If `merge_at` is a tick, the deck is made one ship with the other then (`ServerWorld::merge_grids`: the deck is the smaller, so it is carried onto the lattice of the
/// other, turned a quarter), and the handles name the merged ship.
fn drive_a_deck_and_a_turned_base(
    scene: &mut Scene,
    player: &mut Player,
    (deck, base): (&ShipHandle, &ShipHandle),
    done: usize,
    ticks: usize,
    merge_at: Option<usize>,
) {
    let (deck, base) = (Arc::clone(deck), Arc::clone(base));
    common::each_tick(scene, player, ticks, move |world, tick| {
        let (origin, velocity) = falling_freely((done + tick + 1) as f64 * TICK);
        if merge_at == Some(tick) {
            let content = voxunami_cave::content::CaveContent::new();
            let turn =
                |_: &mut Vec<(voxel_grid::ChannelIndex, u32)>, _: voxel_space::LatticeRotation| {};
            let merged = world
                .merge_grids(
                    *deck.lock().unwrap(),
                    *base.lock().unwrap(),
                    &server::MergePlan {
                        block_channel: content.block_channel(),
                        orientation_channel: Some(content.orientation_channel()),
                        tolerance: voxel_space::AlignmentTolerance::default(),
                        turn: &turn,
                        linear_velocity: velocity,
                        angular_velocity: [0.0; 3],
                    },
                )
                .expect("the deck and the other ship were merged");
            *deck.lock().unwrap() = merged;
            *base.lock().unwrap() = merged;
        }
        let (deck_id, base_id) = (*deck.lock().unwrap(), *base.lock().unwrap());
        if deck_id != base_id {
            world
                .set_grid_pose(
                    deck_id,
                    GridPose::new(
                        WorldPosition::new(voxel_space::BrickCoordinate::ORIGIN, origin),
                        Orientation::IDENTITY,
                    ),
                )
                .unwrap();
            world
                .set_grid_velocity(deck_id, velocity, [0.0; 3])
                .unwrap();
        }
        // (The merged ship is in the place and the turn of the larger.)
        world
            .set_grid_pose(
                base_id,
                posed(
                    plus(origin, BASE_BESIDE_THE_DECK),
                    roll(std::f64::consts::FRAC_PI_2),
                ),
            )
            .unwrap();
        world
            .set_grid_velocity(base_id, velocity, [0.0; 3])
            .unwrap();
    });
}

/// How far the eyes go through the world, from the ship's fall, in the `ticks` ticks after the jump key goes down in a ship in free fall, with the deck merged with the base
/// `merge_at` ticks after (if it is).
fn flight_through_the_world_from_the_fall(merge_at: Option<usize>, ticks: usize) -> [f64; 3] {
    let (mut scene, mut player) = standing_on_the_deck();
    common::make_ship(
        &mut scene,
        posed(
            plus(START, BASE_BESIDE_THE_DECK),
            roll(std::f64::consts::FRAC_PI_2),
        ),
        true,
        [40, 2, 8],
        |_| true,
    );
    let (deck, base) = (the_deck(), Arc::new(Mutex::new(ship_id(1))));
    let lead_in = 3;
    drive_a_deck_and_a_turned_base(&mut scene, &mut player, (&deck, &base), 0, lead_in, None);
    let before = eyes_in_the_world(&scene, 1);
    player.hold(&[KeyCode::Space]);
    drive_a_deck_and_a_turned_base(
        &mut scene,
        &mut player,
        (&deck, &base),
        lead_in,
        ticks,
        merge_at,
    );
    let after = eyes_in_the_world(&scene, 1);
    assert_eq!(
        common::player_pose(&scene, 1).grid,
        *deck.lock().unwrap(),
        "aboard the deck, or the ship it was merged into"
    );
    let fell = falling_freely((lead_in + ticks) as f64 * TICK).0;
    let fell_before = falling_freely(lead_in as f64 * TICK).0;
    [0, 1, 2].map(|axis| after[axis] - before[axis] - (fell[axis] - fell_before[axis]))
}

#[test]
fn a_jump_in_a_ship_in_free_fall_goes_on_as_it_was_when_the_ship_is_merged_with_one_that_turns_its_lattice_a_quarter()
 {
    // In free fall the walker goes on through the ship at the speed of the jump, straight up in the world. The merged ship's axes are turned a quarter from the deck's, so the walker's
    // speed in the ship is turned a quarter, or it would go off sideways.
    let control = flight_through_the_world_from_the_fall(None, 12);
    assert!(
        (control[1] - JUMP * 12.0 * TICK).abs() < 0.2
            && control[0].abs() < 0.05
            && control[2].abs() < 0.05,
        "the premise: a jump in free fall goes straight up at the speed of the jump, {control:?}"
    );
    for merge_at in [0, 3, 6] {
        let merged = flight_through_the_world_from_the_fall(Some(merge_at), 12);
        for axis in 0..3 {
            assert!(
                (merged[axis] - control[axis]).abs() < 0.05,
                "merged {merge_at} ticks after the key: went {merged:?} and {control:?} with no merge"
            );
        }
    }
}

fn a_buckled_walker_stays_buckled_when_the_ship_changes_id(change: Change) {
    let (mut scene, mut player) = standing_on_the_deck();
    let ship = the_deck();
    let standing = eyes_in_their_frame(&scene, 1);
    let reply = say_while(
        &mut scene,
        &mut player,
        &ship,
        burning_up,
        0,
        "/ship buckle",
    );
    assert!(reply.starts_with("you are buckled"), "{reply}");
    drive(
        &mut scene,
        &mut player,
        &ship,
        burning_up,
        2,
        10,
        Some((3, change)),
    );
    assert_ne!(
        *ship.lock().unwrap(),
        ship_id(0),
        "the premise: it has a new id"
    );
    assert_eq!(
        common::player_pose(&scene, 1).grid,
        *ship.lock().unwrap(),
        "aboard the new id"
    );
    // Still buckled: this is what they are let go of, not nothing.
    let reply = say_while(
        &mut scene,
        &mut player,
        &ship,
        burning_up,
        12,
        "/ship unbuckle",
    );
    assert!(reply.starts_with("you are let go"), "{reply}");
    drive(
        &mut scene,
        &mut player,
        &ship,
        burning_up,
        14,
        HALF_A_SECOND,
        None,
    );
    let later = eyes_in_their_frame(&scene, 1);
    for axis in 0..3 {
        assert!(
            (later[axis] - standing[axis]).abs() < 0.03,
            "{standing:?} {later:?}"
        );
    }
}

#[test]
fn a_walker_buckled_in_a_ship_that_is_given_a_new_id_is_buckled_in_it_still() {
    a_buckled_walker_stays_buckled_when_the_ship_changes_id(renumbered);
}

#[test]
fn a_walker_buckled_in_a_ship_that_is_cut_in_two_is_buckled_in_their_part_still() {
    a_buckled_walker_stays_buckled_when_the_ship_changes_id(split_in_two);
}

/// How far the eyes of a walker go through the world in the twelve ticks after they have walked off the side of a deck that cruises at 50 metres a second, the deck being given a
/// change (if there is one) two ticks into them. They are in the level then, and go on at the speed the deck gave them: what the air does to them is measured from the speed of
/// the place of the deck they left (`Departure`), and that is the deck's, whatever it is called.
fn how_far_a_walker_who_left_a_cruising_deck_goes(change: Option<Change>) -> f64 {
    let boost = [50.0, 0.0, 0.0];
    let (mut scene, mut player, ship, mut done) = let_go_in_a_ship_that_moves_at(at_rest, boost);
    let whole = at_a_boost(at_rest, boost);
    // Walks off the side (toward -z, from the middle of the deck, which is 4 voxels from the edge).
    player.hold(&[KeyCode::KeyW]);
    let mut left = false;
    for _ in 0..100 {
        drive(&mut scene, &mut player, &ship, whole.clone(), done, 1, None);
        done += 1;
        if common::player_pose(&scene, 1).grid.root_level().is_some() {
            left = true;
            break;
        }
    }
    assert!(left, "the premise: they walked off");
    player.hold(&[]);
    let started = eyes_in_the_world(&scene, 1);
    drive(
        &mut scene,
        &mut player,
        &ship,
        whole,
        done,
        12,
        change.map(|change| (2, change)),
    );
    assert!(
        common::player_pose(&scene, 1).grid.root_level().is_some(),
        "and they are in the level still"
    );
    eyes_in_the_world(&scene, 1)[0] - started[0]
}

fn a_walker_who_left_a_deck_is_not_braked_by_a_new_id_of_the_deck(change: Change) {
    let control = how_far_a_walker_who_left_a_cruising_deck_goes(None);
    // (Twelve ticks at 50 metres a second is 30 voxels; the air takes 2 or 3 of them if it is measured from the level.)
    assert!(
        (control - 30.0).abs() < 1.0,
        "the premise: they go on at the speed of the deck, {control}"
    );
    let changed = how_far_a_walker_who_left_a_cruising_deck_goes(Some(change));
    assert!(
        (changed - control).abs() < 0.3,
        "they went {changed} when the deck was changed and {control} when it was not"
    );
}

#[test]
fn a_walker_who_left_a_fast_deck_goes_on_at_its_speed_when_the_deck_is_given_a_new_id() {
    a_walker_who_left_a_deck_is_not_braked_by_a_new_id_of_the_deck(renumbered);
}

#[test]
fn a_walker_who_left_a_fast_deck_goes_on_at_its_speed_when_the_deck_is_cut_in_two() {
    a_walker_who_left_a_deck_is_not_braked_by_a_new_id_of_the_deck(split_in_two);
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------
// A deck that really turns, cut in two or merged with another ship while it does (what a walker feels at the change of id, 2026-11-12).
// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------

/// A turn about the world's y of `angle` radians, as the pose of a ship.
fn turned_about_y(angle: f64) -> Orientation {
    Orientation::from_components([0.0, libm::sin(angle / 2.0), 0.0, libm::cos(angle / 2.0)])
        .unwrap()
}

/// A vector turned about y by `angle`.
fn about_y(angle: f64, vector: [f64; 3]) -> [f64; 3] {
    let (sin, cos) = (libm::sin(angle), libm::cos(angle));
    [
        cos * vector[0] + sin * vector[2],
        vector[1],
        -sin * vector[0] + cos * vector[2],
    ]
}

/// How far a deck has turned and how fast it turns at the tick `tick` of the timeline of `at_a_boost` (rest, a steady getting up to `spin`, then the spin), about y.
fn turning_at(spin: f64, tick: usize) -> (f64, f64) {
    let ramp = RAMP as f64 * TICK;
    if tick <= AT_REST {
        (0.0, 0.0)
    } else if tick <= AT_REST + RAMP {
        let s = (tick - AT_REST) as f64 * TICK;
        (spin * s * s / (2.0 * ramp), spin * s / ramp)
    } else {
        let s = (tick - AT_REST - RAMP) as f64 * TICK;
        (spin * (ramp / 2.0 + s), spin)
    }
}

/// What else the assembly does, from a tick on, besides turning at a steady rate.
#[derive(Clone, Copy, Debug)]
enum Variation {
    /// Nothing.
    Steady,
    /// A burn straight up at the pull (the pivot accelerates, along the axis of the turn).
    Burning,
    /// The turn gets faster by this many radians a second a second (the Euler pull).
    SpinningUp(f64),
}

/// What a rigid assembly that turns about a vertical is doing at a tick: how far it has turned and how fast, where the point it turns about is, and how fast that goes.
#[derive(Clone, Copy)]
struct Turn {
    angle: f64,
    spin: f64,
    pivot: [f64; 3],
    pivot_velocity: [f64; 3],
}

type Timeline = Arc<dyn Fn(usize) -> Turn + Send + Sync>;

/// The assembly turns as `turning_at` says, and from the tick `from` does what `variation` says too; its pivot is at `pivot` when it has not moved.
fn timeline(spin: f64, variation: Variation, pivot: [f64; 3], from: usize) -> Timeline {
    Arc::new(move |tick| {
        let (mut angle, mut turn) = turning_at(spin, tick);
        let after = tick.saturating_sub(from) as f64 * TICK;
        let (mut place, mut velocity) = (pivot, [0.0; 3]);
        match variation {
            Variation::Steady => {}
            Variation::Burning => {
                place[1] += 0.5 * PULL * after * after;
                velocity[1] = PULL * after;
            }
            Variation::SpinningUp(rate) => {
                angle += 0.5 * rate * after * after;
                turn += rate * after;
            }
        }
        Turn {
            angle,
            spin: turn,
            pivot: place,
            pivot_velocity: velocity,
        }
    })
}

/// A ship that is driven as part of the assembly: its handle (what it is called now), and where its origin was and how it was turned when the assembly had not turned.
#[derive(Clone)]
struct Part {
    handle: ShipHandle,
    origin: [f64; 3],
    orientation: Orientation,
}

/// Something done to the ships of the assembly, which are given as their handles, in the order of the parts.
type Cut = Arc<dyn Fn(&mut ServerWorld, &[ShipHandle]) + Send + Sync>;

/// A change of id of the first ship, as the other tests do it.
fn cut_of(change: Change) -> Cut {
    Arc::new(move |world, handles| change(world, &handles[0]))
}

/// Runs `ticks` ticks (of which `done` have been run) in which the ships of the assembly turn as a rigid body about the vertical through the pivot of `timeline`, which is at
/// `pivot` when it has not moved: each ship is posed from where its origin was and how it was turned, and goes at the speed of its own centre of mass now
/// (`MovingGrid::centre_of_mass`, which a cut or a merge changes) in that turn. `cut` is done in the tick it names, last. A ship that two handles name is driven once, by the first.
#[allow(clippy::too_many_arguments)]
fn drive_assembly(
    scene: &mut Scene,
    player: &mut Player,
    parts: &[Part],
    timeline: &Timeline,
    pivot: [f64; 3],
    done: usize,
    ticks: usize,
    cut: Option<(usize, Cut)>,
) {
    let (parts, timeline) = (parts.to_vec(), Arc::clone(timeline));
    common::each_tick(scene, player, ticks, move |world, tick| {
        let turn = timeline(done + tick + 1);
        let mut driven = BTreeSet::new();
        for part in &parts {
            let id = *part.handle.lock().unwrap();
            if !driven.insert(id) {
                continue;
            }
            let first = [0, 1, 2].map(|axis| part.origin[axis] - pivot[axis]);
            let carried = posed(part.origin, part.orientation);
            let centre = carried.direction_to_world(
                world
                    .data()
                    .grids()
                    .get(id)
                    .expect("the ship")
                    .centre_of_mass,
            );
            // Where the ship's centre of mass is from the pivot when the assembly has not turned, turned.
            let arm = about_y(turn.angle, [0, 1, 2].map(|axis| first[axis] + centre[axis]));
            // (A ship made by a merge has no mass and its centre of mass at its origin until the bodies weigh it in the tick: the speed its plan gave it is the speed of the centre of
            // mass they find, and a speed worked out for the origin would be another point's.)
            let (unweighed, planned) = {
                let ship = world.data().grids().get(id).expect("the ship");
                (
                    ship.mass == 0.0 && ship.centre_of_mass == [0.0; 3],
                    ship.linear_velocity,
                )
            };
            let velocity = [
                turn.pivot_velocity[0] + turn.spin * arm[2],
                turn.pivot_velocity[1],
                turn.pivot_velocity[2] - turn.spin * arm[0],
            ];
            let turned_first = about_y(turn.angle, first);
            let origin = [0, 1, 2].map(|axis| turn.pivot[axis] + turned_first[axis]);
            world
                .set_grid_pose(
                    id,
                    posed(origin, turned_about_y(turn.angle).compose(part.orientation)),
                )
                .unwrap();
            if unweighed {
                world
                    .set_grid_velocity(id, planned, [0.0, turn.spin, 0.0])
                    .unwrap();
            } else {
                world
                    .set_grid_velocity(id, velocity, [0.0, turn.spin, 0.0])
                    .unwrap();
            }
        }
        // (Last, so that the plan of a merge hears of the speeds of this tick, as the docking of the game does when it works them out.)
        if let Some((at, cut)) = &cut
            && *at == tick
        {
            let handles: Vec<ShipHandle> =
                parts.iter().map(|part| Arc::clone(&part.handle)).collect();
            cut(world, &handles);
        }
    });
}

/// The deck cut at x = `CUT` (it is 30 long), and the handle at the part the walker is on.
fn split_at<const CUT: i128>(world: &mut ServerWorld, ship: &Mutex<GridId>) {
    let part = |x: std::ops::Range<i128>| -> SplitPart {
        let voxels: BTreeSet<VoxelCoordinate> = x
            .clone()
            .flat_map(|x| {
                (0..2).flat_map(move |y| (0..8).map(move |z| VoxelCoordinate::new(x, y, z)))
            })
            .collect();
        SplitPart {
            voxels,
            centre_of_mass: [(x.start + x.end) as f64 / 2.0, 1.0, 4.0],
        }
    };
    let mut id = ship.lock().unwrap();
    let ids = world
        .split_grid(*id, vec![part(0..CUT), part(CUT..30)])
        .expect("the deck was split");
    *id = *ids
        .iter()
        .find(|part| !world.entities().in_grid(**part).is_empty())
        .expect("the walker is aboard one of the parts");
}

/// A deck and a larger ship beside it, turned a quarter about z, that are one ship from now on: the deck is the smaller, so it is carried onto the lattice of the other. The
/// speeds are the whole's, as the docking of the game works them out (the speed of the centre of mass of both, which turn alike; the mass of each is what the bodies worked out).
/// The handles of the parts are the base's and then the deck's, and both name the merged ship after.
fn merge_the_deck_into_the_base(world: &mut ServerWorld, handles: &[ShipHandle]) {
    let (base, deck) = (*handles[0].lock().unwrap(), *handles[1].lock().unwrap());
    let (mass, linear) = {
        let grids = world.data().grids();
        let one = grids.get(base).expect("the base");
        let other = grids.get(deck).expect("the deck");
        let mass = one.mass + other.mass;
        let linear = [0, 1, 2].map(|axis| {
            (one.mass * one.linear_velocity[axis] + other.mass * other.linear_velocity[axis]) / mass
        });
        (mass, linear)
    };
    assert!(mass > 0.0, "the premise: the bodies have weighed both");
    let angular = world
        .data()
        .grids()
        .get(base)
        .expect("the base")
        .angular_velocity;
    let content = voxunami_cave::content::CaveContent::new();
    let turn = |_: &mut Vec<(voxel_grid::ChannelIndex, u32)>, _: voxel_space::LatticeRotation| {};
    let merged = world
        .merge_grids(
            deck,
            base,
            &server::MergePlan {
                block_channel: content.block_channel(),
                orientation_channel: Some(content.orientation_channel()),
                tolerance: voxel_space::AlignmentTolerance::default(),
                turn: &turn,
                linear_velocity: linear,
                angular_velocity: angular,
            },
        )
        .expect("the deck and the other ship were merged");
    *handles[0].lock().unwrap() = merged;
    *handles[1].lock().unwrap() = merged;
}

/// The ticks of the settling of a walker on an assembly that turns: buckled while it gets up to its spin (two at rest, forty of getting up), let go, and twenty to be still; the
/// ticks that the window of the measure begins after.
const SETTLED: usize = AT_REST + RAMP + 2 + 20;

/// A walker who stands at `x` along a deck (and, if `with_a_base`, a larger ship beside it, turned a quarter about z, which turns with it) that turns about the vertical through
/// its centre of mass at `spin` and does what `variation` says from ten ticks before the window of the measure; and where their eyes are in the world, tick by tick, for `ticks`
/// ticks from the start of the window, with `cut` done two ticks into it (if there is one). `None` for the tick they are no longer aboard the ship that the first handle names.
fn track_on_a_turning_deck(
    spin: f64,
    variation: Variation,
    x: f64,
    with_a_base: bool,
    cut: Option<Cut>,
    ticks: usize,
) -> (Vec<[f64; 3]>, Option<usize>) {
    let mut scene = scene_with_a_deck();
    scene.run(|world| {
        world.set_gravity_settings(GravitySettings {
            base: [0.0, -PULL, 0.0],
            ..GravitySettings::default()
        });
    });
    if with_a_base {
        common::make_ship(
            &mut scene,
            posed(
                plus(START, BASE_BESIDE_THE_DECK),
                roll(std::f64::consts::FRAC_PI_2),
            ),
            true,
            [40, 2, 8],
            |_| true,
        );
    }
    // (Falls onto the deck from 8 above it at `x` along it, 4 across it, and is still on it.)
    let mut player = scene
        .rig
        .join_walking([5.0 + x, FLOOR + 5.0 + 8.0, 24.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    common::tick_and_follow(&mut scene, &mut player, 60);
    assert_eq!(common::player_pose(&scene, 1).grid, ship_id(0), "aboard");
    let pivot = plus(START, centre_of_the_deck(&scene));
    let deck = Part {
        handle: the_deck(),
        origin: START,
        orientation: Orientation::IDENTITY,
    };
    let mut parts = vec![deck.clone()];
    if with_a_base {
        parts.insert(
            0,
            Part {
                handle: Arc::new(Mutex::new(ship_id(1))),
                origin: plus(START, BASE_BESIDE_THE_DECK),
                orientation: roll(std::f64::consts::FRAC_PI_2),
            },
        );
    }
    let timeline = timeline(spin, variation, pivot, SETTLED - 10);
    let run = |scene: &mut Scene, player: &mut Player, done: usize, ticks: usize, cut| {
        drive_assembly(scene, player, &parts, &timeline, pivot, done, ticks, cut);
    };
    player.send_command("/ship buckle");
    run(&mut scene, &mut player, 0, 2, None);
    let reply = player.take_command_replies().join(" | ");
    assert!(reply.starts_with("you are buckled"), "{reply}");
    run(&mut scene, &mut player, AT_REST, RAMP, None);
    player.send_command("/ship unbuckle");
    run(&mut scene, &mut player, AT_REST + RAMP, 2, None);
    let reply = player.take_command_replies().join(" | ");
    assert!(reply.starts_with("you are let go"), "{reply}");
    run(&mut scene, &mut player, AT_REST + RAMP + 2, 20, None);
    let mut track = Vec::new();
    for tick in 0..ticks {
        let cut = cut.clone().filter(|_| tick == 2).map(|cut| (0, cut));
        run(&mut scene, &mut player, SETTLED + tick, 1, cut);
        if common::player_pose(&scene, 1).grid != *deck.handle.lock().unwrap() {
            return (track, Some(tick));
        }
        track.push(eyes_in_the_world(&scene, 1));
    }
    (track, None)
}

/// How far apart two tracks of the eyes are at the most, in any direction; and the tick the walker was thrown out of the ship in one of them, if they were.
fn how_far_apart(
    control: &(Vec<[f64; 3]>, Option<usize>),
    changed: &(Vec<[f64; 3]>, Option<usize>),
) -> Result<f64, usize> {
    if let Some(tick) = changed.1 {
        return Err(tick);
    }
    Ok(control
        .0
        .iter()
        .zip(&changed.0)
        .map(|(a, b)| length_of([0, 1, 2].map(|axis| a[axis] - b[axis])))
        .fold(0.0, f64::max))
}

/// Cuts a deck that does as `variation` says at each of these spins and in each of these places, and says which ones were thrown out of the ship or moved by more than `allowed`.
fn walkers_on_a_deck_that_is_cut(variation: Variation, spins: &[f64], allowed: f64) -> Vec<String> {
    let mut disagreements = Vec::new();
    for &spin in spins {
        for (name, x, change) in [
            (
                "cut at 10, walker at 20 (their part's centre of mass is 5 voxels on)",
                20.0,
                split_at::<10> as Change,
            ),
            (
                "cut at 10, walker at 5 (10 voxels back)",
                5.0,
                split_at::<10> as Change,
            ),
            (
                "cut at 20, walker at 25 (10 voxels on)",
                25.0,
                split_at::<20> as Change,
            ),
        ] {
            let control = track_on_a_turning_deck(spin, variation, x, false, None, 25);
            let cut = track_on_a_turning_deck(spin, variation, x, false, Some(cut_of(change)), 25);
            match how_far_apart(&control, &cut) {
                Err(tick) => disagreements.push(format!(
                    "{variation:?}, spin {spin}, {name}: thrown out of the ship at tick {tick}"
                )),
                Ok(worst) if worst > allowed => disagreements.push(format!(
                    "{variation:?}, spin {spin}, {name}: the eyes were {worst} voxels from where they are with no cut"
                )),
                Ok(_) => {}
            }
        }
    }
    disagreements
}

/// A walker on a deck that turns steadily is no different for the deck being cut in two: the cut puts the centre of mass of their part elsewhere, so the speed of the centre of
/// mass is another, which is not an acceleration of the ship (it is the same turn, seen from another point). Measured 2026-11-12, before this was made so: a cut that moved the
/// centre of mass by 5 voxels of a deck turning at 0.02 radians a second (a turn in five minutes) moved the walker's eyes 0.27 voxels, at 0.1 a second 0.67, and from 0.15 a second
/// the walker was thrown out of the ship in the tick of the cut. (With no turn the cut changes nothing: 1e-5.) Since the acceleration of the ship that the last step measured is carried
/// over a change of id, with the acceleration the shift of the centre of mass gives (`follow_renamed_ships`), the eyes are within 0.009 voxels of where they are with no cut, at 0.4
/// radians a second.
#[test]
fn a_walker_on_a_deck_that_turns_steadily_is_not_moved_by_the_deck_being_cut_in_two() {
    let disagreements = walkers_on_a_deck_that_is_cut(Variation::Steady, &[0.02, 0.1, 0.4], 0.02);
    assert!(disagreements.is_empty(), "{disagreements:#?}");
}

#[test]
fn a_walker_on_a_deck_that_turns_and_burns_is_not_moved_by_the_deck_being_cut_in_two() {
    let disagreements = walkers_on_a_deck_that_is_cut(Variation::Burning, &[0.1, 0.4], 0.02);
    assert!(disagreements.is_empty(), "{disagreements:#?}");
}

#[test]
fn a_walker_on_a_deck_whose_turn_is_getting_faster_is_not_moved_by_the_deck_being_cut_in_two() {
    let disagreements =
        walkers_on_a_deck_that_is_cut(Variation::SpinningUp(0.2), &[0.0, 0.1], 0.02);
    assert!(disagreements.is_empty(), "{disagreements:#?}");
}

/// The deck merged with a larger ship beside it, turned a quarter (so the walker's speed and place are carried by a turned lattice, and the centre of mass of the merged ship is
/// far from the deck's) while the whole turns.
fn walkers_on_a_deck_that_is_merged(
    variation: Variation,
    spins: &[f64],
    allowed: f64,
) -> Vec<String> {
    let merge: Cut = Arc::new(merge_the_deck_into_the_base);
    let mut disagreements = Vec::new();
    for &spin in spins {
        let control = track_on_a_turning_deck(spin, variation, 8.0, true, None, 25);
        let merged =
            track_on_a_turning_deck(spin, variation, 8.0, true, Some(Arc::clone(&merge)), 25);
        match how_far_apart(&control, &merged) {
            Err(tick) => disagreements.push(format!(
                "{variation:?}, spin {spin}: thrown out of the ship at tick {tick}"
            )),
            Ok(worst) if worst > allowed => disagreements.push(format!(
                "{variation:?}, spin {spin}: the eyes were {worst} voxels from where they are with no merge"
            )),
            Ok(_) => {}
        }
    }
    disagreements
}

#[test]
fn a_walker_on_a_deck_that_turns_is_not_moved_by_the_deck_being_merged_with_a_ship_that_turns_its_lattice_a_quarter()
 {
    let disagreements = walkers_on_a_deck_that_is_merged(Variation::Steady, &[0.0, 0.1, 0.4], 0.02);
    assert!(disagreements.is_empty(), "{disagreements:#?}");
}
