//! The suit thrusters of a character (owner, 2026-11-10 and 2026-11-11): a toggle that, in the air, pushes the character with a fixed force along the keys, and along up for the
//! vertical keys. There is no brake: with no key held the character keeps its velocity. Where there is no pull the keys do nothing without the thrusters (a floating character
//! does not stop itself). Not strong enough to hover over a planet whose pull is 5 metres a second a second, but strong enough to add to a jump there.

mod common;

use common::{FORWARD, STILL, Scene, eye, position, spawn};
use physics::{CharacterBody, CharacterSettings, MoveInput};

const WEIGHTLESS: [f64; 3] = [0.0; 3];
const PLANET: [f64; 3] = [0.0, -5.0, 0.0];
const TICKS_PER_SECOND: usize = 60;

/// A character floating in nothing at `velocity`, the thrusters as asked.
fn floating(velocity: [f64; 3], thrusters: bool) -> (CharacterBody, Scene) {
    let mut body = spawn(5.0, 100.0, 5.0);
    body.reframe(position(5.0, 100.0, 5.0), velocity, [0.0, 0.0, 0.0, 1.0]);
    body.set_thrusters(thrusters);
    (body, Scene::default())
}

fn step(
    body: &mut CharacterBody,
    scene: &mut Scene,
    input: MoveInput,
    gravity: [f64; 3],
    count: usize,
) {
    for _ in 0..count {
        body.update_terrain(scene);
        body.step_with_gravity(input, gravity);
    }
}

fn thrust() -> f64 {
    CharacterSettings::default().thruster_acceleration()
}

fn close(actual: [f64; 3], expected: [f64; 3], tolerance: f64, what: &str) {
    for axis in 0..3 {
        assert!(
            (actual[axis] - expected[axis]).abs() <= tolerance,
            "{what}: {actual:?} against {expected:?}"
        );
    }
}

#[test]
fn the_thrust_is_a_round_number_of_newtons_that_is_half_the_weight_of_an_adult_on_a_planet_of_9_81_and_is_less_than_a_pull_of_5()
 {
    let settings = CharacterSettings::default();
    // A sensible adult, to the nearest 5 kilograms, and a round force.
    assert_eq!(settings.mass, 75.0);
    assert_eq!(settings.thruster_force, 350.0);
    let weight = settings.mass * 9.81;
    let share = settings.thruster_force / weight;
    assert!(
        (0.45..0.55).contains(&share),
        "about half the weight: {share}"
    );
    assert_eq!(settings.thruster_acceleration(), 350.0 / 75.0);
    // Not quite enough to hover over the planet whose pull is 5 (the owner), so a jump there is lengthened and not made into a hover.
    assert!(settings.thruster_acceleration() < 5.0);
    assert!(settings.thruster_acceleration() > 4.0);
}

#[test]
fn the_thrusters_start_off_and_are_switched_with_a_call() {
    let mut body = spawn(5.0, 100.0, 5.0);
    assert!(!body.thrusters_on());
    body.set_thrusters(true);
    assert!(body.thrusters_on());
    body.set_thrusters(false);
    assert!(!body.thrusters_on());
}

#[test]
fn a_floating_character_with_the_thrusters_off_keeps_its_velocity_whatever_keys_are_held_and_nothing_brakes_it()
 {
    let (mut body, mut scene) = floating([3.0, 0.5, -2.0], false);
    step(
        &mut body,
        &mut scene,
        FORWARD,
        WEIGHTLESS,
        5 * TICKS_PER_SECOND,
    );
    close(
        body.velocity(),
        [3.0, 0.5, -2.0],
        0.0,
        "keys without thrusters",
    );
    step(
        &mut body,
        &mut scene,
        STILL,
        WEIGHTLESS,
        5 * TICKS_PER_SECOND,
    );
    close(body.velocity(), [3.0, 0.5, -2.0], 0.0, "no keys");
}

#[test]
fn a_floating_character_with_the_thrusters_on_speeds_up_along_the_keys_at_the_thrust() {
    let (mut body, mut scene) = floating([0.0; 3], true);
    step(&mut body, &mut scene, FORWARD, WEIGHTLESS, TICKS_PER_SECOND);
    // Forward at yaw 0 is toward -z; a second of thrust.
    close(
        body.velocity(),
        [0.0, 0.0, -thrust()],
        1e-9,
        "a second forward",
    );
    step(
        &mut body,
        &mut scene,
        FORWARD,
        WEIGHTLESS,
        2 * TICKS_PER_SECOND,
    );
    close(
        body.velocity(),
        [0.0, 0.0, -3.0 * thrust()],
        1e-9,
        "three seconds forward",
    );
    // Past the walking speed: there is no speed the keys ask for.
    assert!(3.0 * thrust() > CharacterSettings::default().sprint_speed);
}

#[test]
fn a_floating_character_with_the_thrusters_on_and_no_key_held_keeps_its_velocity_for_ever() {
    let (mut body, mut scene) = floating([0.0; 3], true);
    step(&mut body, &mut scene, FORWARD, WEIGHTLESS, TICKS_PER_SECOND);
    let speed = body.velocity();
    step(
        &mut body,
        &mut scene,
        STILL,
        WEIGHTLESS,
        60 * TICKS_PER_SECOND,
    );
    assert_eq!(body.velocity(), speed, "no dampeners");
}

#[test]
fn a_thrust_against_the_velocity_slows_it_and_the_key_for_the_other_way_turns_it_round() {
    let (mut body, mut scene) = floating([0.0, 0.0, -10.0], true);
    let backwards = MoveInput {
        forward: -1,
        ..STILL
    };
    step(
        &mut body,
        &mut scene,
        backwards,
        WEIGHTLESS,
        TICKS_PER_SECOND,
    );
    close(
        body.velocity(),
        [0.0, 0.0, -10.0 + thrust()],
        1e-9,
        "braking by hand",
    );
    step(
        &mut body,
        &mut scene,
        backwards,
        WEIGHTLESS,
        3 * TICKS_PER_SECOND,
    );
    assert!(
        body.velocity()[2] > 0.0,
        "turned round: {:?}",
        body.velocity()
    );
}

#[test]
fn the_strafe_keys_thrust_to_the_side() {
    let (mut body, mut scene) = floating([0.0; 3], true);
    let right = MoveInput { strafe: 1, ..STILL };
    step(&mut body, &mut scene, right, WEIGHTLESS, TICKS_PER_SECOND);
    close(
        body.velocity(),
        [thrust(), 0.0, 0.0],
        1e-9,
        "a second to the right",
    );
}

#[test]
fn the_vertical_keys_thrust_along_up_and_down_and_up_is_against_the_pull() {
    // On a planet's axes (pull down) there is no ground here, so a character with the thrusters on goes up for the key up.
    let (mut body, mut scene) = floating([0.0; 3], true);
    let up = MoveInput {
        vertical: 1,
        ..STILL
    };
    // (In a pull of 5 a second up is the thrust less the pull: it still falls, more slowly.)
    step(&mut body, &mut scene, up, PLANET, TICKS_PER_SECOND);
    close(
        body.velocity(),
        [0.0, thrust() - 5.0, 0.0],
        1e-9,
        "up in a pull",
    );

    // Up is against the pull: when the pull points along +x, up is toward -x.
    let (mut body, mut scene) = floating([0.0; 3], true);
    step(&mut body, &mut scene, up, [5.0, 0.0, 0.0], TICKS_PER_SECOND);
    close(
        body.velocity(),
        [-(thrust() - 5.0), 0.0, 0.0],
        1e-9,
        "up against a sideways pull",
    );

    // Down is with it.
    let (mut body, mut scene) = floating([0.0; 3], true);
    let down = MoveInput {
        vertical: -1,
        ..STILL
    };
    step(&mut body, &mut scene, down, WEIGHTLESS, TICKS_PER_SECOND);
    let [x, y, z] = body.velocity();
    assert_eq!([x, z], [0.0, 0.0]);
    assert!(
        (y + thrust()).abs() < 1e-9,
        "down with no pull goes along the way up the character had: {y}"
    );
}

#[test]
fn keys_together_do_not_add_up_to_more_than_the_thrust() {
    let (mut body, mut scene) = floating([0.0; 3], true);
    let all = MoveInput {
        forward: 1,
        strafe: 1,
        vertical: 1,
        ..STILL
    };
    step(&mut body, &mut scene, all, WEIGHTLESS, TICKS_PER_SECOND);
    let [x, y, z] = body.velocity();
    let speed = (x * x + y * y + z * z).sqrt();
    assert!((speed - thrust()).abs() < 1e-9, "{speed}");
    // Each way equally, so the three parts are equal.
    assert!((x.abs() - y.abs()).abs() < 1e-9 && (y.abs() - z.abs()).abs() < 1e-9);
}

#[test]
fn a_character_in_the_air_over_a_planet_with_the_thrusters_off_steers_as_it_always_did() {
    // The air control of a walker that has jumped: toward the walking speed, not beyond it.
    let (mut body, mut scene) = floating([0.0; 3], false);
    step(&mut body, &mut scene, FORWARD, PLANET, 2 * TICKS_PER_SECOND);
    let [x, _, z] = body.velocity();
    assert_eq!(x, 0.0);
    assert!(
        (z + CharacterSettings::default().walk_speed).abs() < 1e-9,
        "{z}"
    );
}

#[test]
fn the_thrusters_replace_the_steering_in_the_air_and_have_no_walking_speed_to_stop_at() {
    let (mut body, mut scene) = floating([0.0; 3], true);
    step(&mut body, &mut scene, FORWARD, PLANET, 4 * TICKS_PER_SECOND);
    let [_, _, z] = body.velocity();
    assert!(
        (z + 4.0 * thrust()).abs() < 1e-9,
        "four seconds at the thrust, past the walk of 4.3: {z}"
    );
    // And it falls at the same time, as anything does.
    assert!(
        (body.velocity()[1] + 4.0 * 5.0).abs() < 1e-9,
        "{:?}",
        body.velocity()
    );
}

#[test]
fn on_the_ground_the_thrusters_change_nothing_a_character_walks_as_it_did() {
    let walk = |thrusters: bool| {
        let mut scene = Scene::with_floor(9);
        let mut body = spawn(5.0, 12.0, 5.0);
        body.set_thrusters(thrusters);
        // (It has fallen onto the floor first, which is the air, and the thrusters are for that.)
        for _ in 0..30 {
            body.update_terrain(&mut scene);
            body.keep_clear(&mut scene);
            body.step_with_gravity(STILL, PLANET);
        }
        assert!(body.is_grounded());
        let mut looks = Vec::new();
        for count in 0..120 {
            body.update_terrain(&mut scene);
            body.keep_clear(&mut scene);
            body.step_with_gravity(if count < 90 { FORWARD } else { STILL }, PLANET);
            looks.push((
                eye(&body).map(f64::to_bits),
                body.velocity().map(f64::to_bits),
            ));
        }
        assert!(body.is_grounded());
        looks
    };
    assert_eq!(walk(false), walk(true), "to the bit");
}

#[test]
fn a_jump_with_the_vertical_key_held_and_the_thrusters_on_goes_far_higher_but_is_no_hover() {
    let apex = |thrusters: bool| {
        let mut scene = Scene::with_floor(9);
        let mut body = spawn(5.0, 12.0, 5.0);
        body.set_thrusters(thrusters);
        for _ in 0..30 {
            body.update_terrain(&mut scene);
            body.keep_clear(&mut scene);
            body.step_with_gravity(STILL, PLANET);
        }
        assert!(body.is_grounded());
        let start = eye(&body)[1];
        let hold = MoveInput {
            jump: true,
            vertical: 1,
            ..STILL
        };
        // Up and down once: to the first landing.
        let mut highest: f64 = 0.0;
        let mut left_the_ground = false;
        for count in 0..(60 * TICKS_PER_SECOND) {
            body.update_terrain(&mut scene);
            body.keep_clear(&mut scene);
            body.step_with_gravity(hold, PLANET);
            highest = highest.max(eye(&body)[1] - start);
            if !body.is_grounded() {
                left_the_ground = true;
            } else if left_the_ground {
                return (highest, count);
            }
        }
        panic!("it did not come down in a minute: a hover");
    };
    let (plain, _) = apex(false);
    // A jump of 3.6 in a pull of 5: 1.296 high.
    assert!((plain - 1.296).abs() < 0.05, "{plain}");
    let (boosted, landed) = apex(true);
    // Net deceleration of 5 - 4.667 = 0.333 from 3.6 less the pull of the take-off tick (the thrusters are for the air): v^2 / (2 * 0.333) = 18.5 high; up for 10.7 s and down
    // for as long.
    let net = 5.0 - thrust();
    let speed = 3.6 - 5.0 / 60.0;
    let expected = speed * speed / (2.0 * net);
    assert!(
        (boosted - expected).abs() < 0.2,
        "{boosted} against {expected}"
    );
    assert!(boosted > 10.0 * plain);
    // It comes down, in about 2 * speed / net = 21.1 s: no hover.
    assert!(
        (landed as f64 / 60.0 - 2.0 * speed / net).abs() < 0.5,
        "{landed}"
    );
}

#[test]
fn flying_ignores_the_thrusters() {
    let run = |thrusters: bool| {
        let mut scene = Scene::default();
        let mut body = spawn(5.0, 100.0, 5.0);
        body.set_flying(true);
        body.set_thrusters(thrusters);
        for _ in 0..90 {
            body.update_terrain(&mut scene);
            body.step_with_gravity(FORWARD, PLANET);
        }
        body.velocity().map(f64::to_bits)
    };
    assert_eq!(run(false), run(true));
}

#[test]
fn a_floating_character_is_not_held_to_the_fastest_a_falling_one_goes() {
    // The 78 a second is a fall in a pull; floating at 100 along the way down it was (is) not braked to it.
    let (mut body, mut scene) = floating([0.0, -100.0, 0.0], false);
    step(&mut body, &mut scene, STILL, WEIGHTLESS, TICKS_PER_SECOND);
    assert_eq!(body.velocity(), [0.0, -100.0, 0.0]);
    // In a pull it is.
    let (mut body, mut scene) = floating([0.0, -100.0, 0.0], false);
    step(&mut body, &mut scene, STILL, PLANET, 1);
    assert_eq!(
        body.velocity()[1],
        -CharacterSettings::default().terminal_speed
    );
}
