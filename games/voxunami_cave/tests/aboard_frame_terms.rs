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
