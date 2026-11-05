//! A character in the air, against what it left: the speed it asks for with its keys, and the fastest it falls, are measured from the speed of the thing it stepped off, not from
//! the speed of the world it is simulated in. A person who steps off a train that goes at 500 voxels a second does not slow to a walk at once because the keys ask for one.

mod common;

use common::{FORWARD, STILL, Scene, eye, position, run, spawn};
use physics::{CharacterBody, CharacterSettings, MoveInput};

/// A script of keys over a floor: walk, jump, turn, sprint, stand. After every ten steps the eye and the speed, as bits.
fn trace(mut step: impl FnMut(&mut CharacterBody, &mut Scene, MoveInput)) -> Vec<[u64; 6]> {
    let mut scene = Scene::with_floor(9);
    let mut body = spawn(5.0, 14.0, 5.0);
    let mut looks = Vec::new();
    for count in 0..150 {
        let input = match count {
            0..60 => MoveInput {
                jump: count == 30,
                ..FORWARD
            },
            60..90 => MoveInput {
                yaw: 0.4,
                ..FORWARD
            },
            90..120 => MoveInput {
                yaw: 0.4,
                sprint: true,
                ..FORWARD
            },
            _ => STILL,
        };
        step(&mut body, &mut scene, input);
        if count % 10 == 9 {
            let [x, y, z] = eye(&body);
            let [vx, vy, vz] = body.velocity();
            looks.push([x, y, z, vx, vy, vz].map(f64::to_bits));
        }
    }
    looks
}

/// What the script above gave before there was a reference to step against (taken from the controller as it was, with its signs of zero: the last speed is -0.0).
const BEFORE: [[u64; 6]; 15] = [
    [
        4617315517961601024,
        4624027914281214862,
        4617074700481526351,
        0,
        13829053255779003051,
        13835808655220058795,
    ],
    [
        4617315517961601024,
        4623906723666242218,
        4616405415536903237,
        0,
        13833556855406373544,
        13839899424881586995,
    ],
    [
        4617315517961601024,
        4623707345557738837,
        4615007423152573646,
        0,
        13836183955189006336,
        13839899424881586995,
    ],
    [
        4617315517961601024,
        4623429779955704718,
        4613393633286099216,
        0,
        13838060455033744046,
        13839899424881586995,
    ],
    [
        4617315517961601024,
        4623074026860139860,
        4611779843419624786,
        0,
        13839749304894007981,
        13839899424881586995,
    ],
    [
        4617315517961601218,
        4622736726013029979,
        4608646088678912422,
        0,
        0,
        13839899424881586995,
    ],
    [
        4617008198979265889,
        4622736726013040108,
        4604152972226483188,
        13833592128523066361,
        0,
        13839472849045953533,
    ],
    [
        4616693979291947400,
        4622736726013048259,
        4570378636829259520,
        13833592128523066361,
        0,
        13839472849045953533,
    ],
    [
        4616379759604740979,
        4622736726012575037,
        13827460694907112960,
        13833592128523066361,
        0,
        13839472849045953533,
    ],
    [
        4615751468480927921,
        4622736726013014841,
        13832879125900371520,
        13835465051698468145,
        0,
        13840865381211291360,
    ],
    [
        4614933035806982101,
        4622736726012856166,
        13835904365900976832,
        13835465051698468145,
        0,
        13840865381211291360,
    ],
    [
        4614114603133036281,
        4622736726012857170,
        13837840141210686048,
        13835465051698468145,
        0,
        13840865381211291360,
    ],
    [
        4613982094985899676,
        4622736726012857329,
        13838153552451305640,
        0,
        0,
        9223372036854775808,
    ],
    [
        4613982094985899676,
        4622736726012857329,
        13838153552451305640,
        0,
        0,
        9223372036854775808,
    ],
    [
        4613982094985899676,
        4622736726012857329,
        13838153552451305640,
        0,
        0,
        9223372036854775808,
    ],
];

#[test]
fn a_walk_over_a_floor_is_what_it_was_before_there_was_a_reference_to_step_against() {
    let looks = trace(|body, scene, input| run(body, scene, input, 1));
    assert_eq!(looks, BEFORE, "to the bit");
}

#[test]
fn stepping_against_nothing_is_the_same_step_to_the_bit() {
    let gravity = [0.0, -CharacterSettings::default().gravity, 0.0];
    let looks = trace(|body, scene, input| {
        body.update_terrain(scene);
        body.keep_clear(scene);
        body.step_with_gravity_against(input, gravity, [0.0; 3]);
    });
    assert_eq!(looks, BEFORE, "to the bit");
}

/// A character in the air with nothing near it, whose eyes are at `height` and that goes at `velocity`, in a world that pulls with `gravity`.
fn in_the_air(height: f64, velocity: [f64; 3]) -> (CharacterBody, Scene) {
    let mut body = spawn(5.0, height, 5.0);
    body.reframe(position(5.0, height, 5.0), velocity, [0.0, 0.0, 0.0, 1.0]);
    (body, Scene::default())
}

const PULL: [f64; 3] = [0.0, -5.0, 0.0];

fn fall(
    body: &mut CharacterBody,
    scene: &mut Scene,
    input: MoveInput,
    against: [f64; 3],
    steps: usize,
) {
    for _ in 0..steps {
        body.update_terrain(scene);
        body.step_with_gravity_against(input, PULL, against);
    }
}

#[test]
fn a_character_in_the_air_with_no_keys_held_keeps_the_speed_of_what_it_left_and_the_air_does_not_brake_it()
 {
    // It stepped off something that goes at 500 across: two seconds later it still goes at 500 across, and has fallen as anything does (5 a second a second).
    let (mut body, mut scene) = in_the_air(100.0, [500.0, 0.0, 0.0]);
    fall(&mut body, &mut scene, STILL, [500.0, 0.0, 0.0], 120);
    let [x, y, z] = body.velocity();
    assert!((x - 500.0).abs() < 1e-9, "{x}");
    assert!(z.abs() < 1e-9, "{z}");
    assert!((y + 10.0).abs() < 1e-9, "{y}");

    // The same character measured from the world's speed, as every character was: the air takes 14 a second a second off the 500 until it is at what the keys ask (nothing).
    let (mut body, mut scene) = in_the_air(100.0, [500.0, 0.0, 0.0]);
    fall(&mut body, &mut scene, STILL, [0.0; 3], 120);
    assert!(
        (body.velocity()[0] - (500.0 - 14.0 * 2.0)).abs() < 1e-9,
        "{:?}",
        body.velocity()
    );
}

#[test]
fn a_character_in_the_air_with_a_key_held_walks_at_walking_speed_against_what_it_left() {
    // The keys ask for the walk (4.3 toward -z, which is where yaw 0 looks) from what it left; it is at 500 across in the world, and that stays.
    let (mut body, mut scene) = in_the_air(100.0, [500.0, 0.0, 0.0]);
    fall(&mut body, &mut scene, FORWARD, [500.0, 0.0, 0.0], 60);
    let [x, _, z] = body.velocity();
    assert!((x - 500.0).abs() < 1e-9, "{x}");
    assert!(
        (z + CharacterSettings::default().walk_speed).abs() < 1e-9,
        "{z}"
    );
}

#[test]
fn a_character_that_steps_off_something_going_down_fast_falls_with_it_and_is_not_held_to_the_fastest_a_falling_character_goes()
 {
    let ship_down = [0.0, -300.0, 0.0];
    let (mut body, mut scene) = in_the_air(100_000.0, ship_down);
    fall(&mut body, &mut scene, STILL, ship_down, 30);
    // Falling with it, pulled by the world: 300 and what half a second adds.
    assert!(
        (body.velocity()[1] + 302.5).abs() < 1e-9,
        "{:?}",
        body.velocity()
    );

    // Measured from the world, the fastest a falling character goes is 78.
    let (mut body, mut scene) = in_the_air(100_000.0, ship_down);
    fall(&mut body, &mut scene, STILL, [0.0; 3], 30);
    assert_eq!(
        body.velocity()[1],
        -CharacterSettings::default().terminal_speed
    );
}
