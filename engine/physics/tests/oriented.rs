//! A character whose down is not the world's: in a room that turns (a ship), where gravity points along some other axis of the
//! frame the character is simulated in, and the way it faces is carried along as gravity turns.

mod common;

use common::{FORWARD, STILL, Scene, eye, position, spawn};
use physics::{CharacterBody, CharacterSettings, MoveInput};
use voxel_space::VoxelCoordinate;

/// A turn of the whole scene by a multiple of a quarter turn: where a point goes is the matrix times the point, and the quaternion is
/// the same turn.
#[derive(Clone, Copy)]
struct Turn {
    matrix: [[i128; 3]; 3],
    quaternion: [f32; 4],
}

const HALF_ROOT_TWO: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// x' = y, y' = -x: what was up is now along +x (a quarter turn about z, the way that takes y to x).
const UP_BECOMES_PLUS_X: Turn = Turn {
    matrix: [[0, 1, 0], [-1, 0, 0], [0, 0, 1]],
    quaternion: [0.0, 0.0, -HALF_ROOT_TWO, HALF_ROOT_TWO],
};
/// y' = -z, z' = y: what was up is now along +z (a quarter turn about x).
const UP_BECOMES_PLUS_Z: Turn = Turn {
    matrix: [[1, 0, 0], [0, 0, -1], [0, 1, 0]],
    quaternion: [HALF_ROOT_TWO, 0.0, 0.0, HALF_ROOT_TWO],
};
/// Upside down, a half turn about x.
const UPSIDE_DOWN: Turn = Turn {
    matrix: [[1, 0, 0], [0, -1, 0], [0, 0, -1]],
    quaternion: [1.0, 0.0, 0.0, 0.0],
};
const NO_TURN: Turn = Turn {
    matrix: [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
    quaternion: [0.0, 0.0, 0.0, 1.0],
};

fn turned(turn: Turn, vector: [f64; 3]) -> [f64; 3] {
    turn.matrix
        .map(|row| (0..3).map(|axis| row[axis] as f64 * vector[axis]).sum())
}

fn gravity_after(turn: Turn) -> [f32; 3] {
    // The character's own gravity, turned (the script jumps onto a block with the character's own jump, which is made for it).
    turned(
        turn,
        [0.0, -f64::from(CharacterSettings::default().gravity), 0.0],
    )
    .map(|component| component as f32)
}

/// Whether two quaternions are the same turn (a quaternion and its negative are).
fn same_turn(a: [f32; 4], b: [f32; 4]) -> bool {
    let dot: f32 = (0..4).map(|axis| a[axis] * b[axis]).sum();
    dot.abs() > 1.0 - 1e-6
}

/// The voxel of the unturned scene that the turned scene's voxel `voxel` is the image of: the cube of the voxel, taken back by the
/// turn, is a cube of the unturned scene, and this is its lowest corner.
fn taken_back(turn: Turn, voxel: VoxelCoordinate) -> VoxelCoordinate {
    let voxel = [voxel.x(), voxel.y(), voxel.z()];
    let corner: Vec<i128> = (0..3)
        .map(|axis| {
            (0..3)
                .map(|from| {
                    // The transpose: the turn undone.
                    let entry = turn.matrix[from][axis];
                    entry * (voxel[from] + i128::from(entry < 0))
                })
                .sum()
        })
        .collect();
    VoxelCoordinate::new(corner[0], corner[1], corner[2])
}

/// The unturned scene: a floor whose top is at height 10, a block on it one up, and a wall of three blocks.
fn solid_before_turning(voxel: VoxelCoordinate) -> bool {
    let (x, y, z) = (voxel.x(), voxel.y(), voxel.z());
    y <= 9
        || (x, y, z) == (20, 10, 14)
        || (x == 24 && (10..13).contains(&y) && (8..20).contains(&z))
}

/// That scene turned.
fn scene_turned(turn: Turn) -> Scene {
    Scene::of(move |voxel| solid_before_turning(taken_back(turn, voxel)))
}

/// The turn of the frame of a character put in `turn` and stepped in its gravity.
fn frame_after(turn: Turn) -> [f32; 4] {
    let mut body = spawn(5.0, 5.0, 5.0);
    body.reframe(position(5.0, 5.0, 5.0), [0.0; 3], turn.quaternion);
    let mut scene = Scene::default();
    body.update_terrain(&mut scene);
    body.step_with_gravity(STILL, gravity_after(turn));
    body.frame_rotation()
}

/// The state of the character as plain numbers: its eyes, its speed, whether it is on the ground.
fn state(body: &CharacterBody) -> ([f64; 3], [f64; 3], bool) {
    (
        eye(body),
        body.velocity().map(f64::from),
        body.is_grounded(),
    )
}

/// The same script of keys in a scene turned, with the gravity turned with it. Returns the state after each stage, unturned.
fn script_in(turn: Turn) -> Vec<([f64; 3], [f64; 3], bool)> {
    let start = turned(turn, [20.5, 12.0, 24.0]);
    let mut body = spawn(start[0] as f32, start[1] as f32, start[2] as f32);
    // Put in the frame the turned room has, as a character carried into a ship is: standing in it from the first step.
    body.reframe(
        position(start[0] as f32, start[1] as f32, start[2] as f32),
        [0.0; 3],
        turn.quaternion,
    );
    let mut scene = scene_turned(turn);
    let gravity = gravity_after(turn);
    let unturn = |vector: [f64; 3]| {
        // The turns here are rotations by multiples of a quarter, so the transpose is the inverse.
        let transpose = Turn {
            matrix: [0, 1, 2].map(|row| [0, 1, 2].map(|column| turn.matrix[column][row])),
            ..turn
        };
        turned(transpose, vector)
    };
    let stages = [
        (STILL, 60),
        // Toward the block, far enough from it to clear it at the speed of this planet's jump (a character that meets its side before it is high
        // enough slides along it, and the turned and upright runs slide a few centimetres apart), then onto it, then off the other side.
        (FORWARD, 70),
        // One jump (the key is pressed for a step), then walking on until it is over: a jump of 1.3 metres in a gravity of 5 takes 1.4 seconds,
        // 86 steps. The two runs may take off a step apart (the arithmetic of a turned shape is not that of an upright one), and a stage that
        // ended in the air would compare them at different moments of a jump.
        (
            MoveInput {
                jump: true,
                ..FORWARD
            },
            1,
        ),
        (FORWARD, 110),
        (FORWARD, 40),
        // Diagonally toward the wall and along it.
        (
            MoveInput {
                yaw: 0.5,
                strafe: 1,
                ..FORWARD
            },
            60,
        ),
        (
            MoveInput {
                yaw: 1.2,
                sprint: true,
                ..FORWARD
            },
            80,
        ),
    ];
    let mut seen = Vec::new();
    for (input, count) in stages {
        for _ in 0..count {
            body.update_terrain(&mut scene);
            body.step_with_gravity(input, gravity);
        }
        let (eyes, velocity, grounded) = state(&body);
        seen.push((unturn(eyes), unturn(velocity), grounded));
    }
    seen
}

#[test]
fn gravity_along_minus_y_with_the_step_that_takes_it_is_the_step_without() {
    // The two ways to step agree to the bit when gravity is the character's own, straight down.
    let run_with = |own: bool| {
        let mut scene = scene_turned(NO_TURN);
        let mut body = spawn(20.5, 12.0, 24.0);
        let mut hashes = Vec::new();
        for input in [
            STILL,
            FORWARD,
            MoveInput {
                jump: true,
                yaw: 0.7,
                ..FORWARD
            },
        ] {
            for _ in 0..70 {
                body.update_terrain(&mut scene);
                if own {
                    body.step(input);
                } else {
                    body.step_with_gravity(input, [0.0, -body.settings().gravity, 0.0]);
                }
            }
            hashes.push(body.state_hash());
        }
        hashes
    };
    assert_eq!(run_with(true), run_with(false));
}

#[test]
fn a_turned_room_is_the_same_room_turned_a_quarter_up_becomes_x() {
    assert_turned_like(UP_BECOMES_PLUS_X);
}

#[test]
fn a_turned_room_is_the_same_room_turned_a_quarter_up_becomes_z() {
    assert_turned_like(UP_BECOMES_PLUS_Z);
}

#[test]
fn a_turned_room_is_the_same_room_turned_upside_down() {
    assert_turned_like(UPSIDE_DOWN);
}

/// Everything a character does in a room that is turned, with the gravity turned with it, is what it does in the room unturned, seen
/// turned: the walking, the jump onto the block, the wall it slides along. Nothing in the turned frame is special.
fn assert_turned_like(turn: Turn) {
    let plain = script_in(NO_TURN);
    let turned = script_in(turn);
    assert_eq!(plain.len(), turned.len());
    // The frame the character kept is the one it was put in.
    assert!(same_turn(frame_after(turn), turn.quaternion));
    for (stage, (before, after)) in plain.iter().zip(&turned).enumerate() {
        for axis in 0..3 {
            // A shape that is turned is cast against the terrain by other arithmetic than one that is not, and the two rest a few
            // thousandths of a voxel apart (the largest in this script, 0.008, is half a hundredth of a voxel); what they do is the same.
            assert!(
                (before.0[axis] - after.0[axis]).abs() < 1.5e-2,
                "stage {stage}, eyes on axis {axis}: {:?} against {:?}",
                before.0,
                after.0
            );
            // A character on the ground has no speed up or down, or has the speed one step of gravity gives it before the floor takes it away,
            // which of the two it is at the moment of the look depends on the arithmetic of the step (at a gravity of 5 a step's worth is 0.08 and
            // is seen; at 32 it was taken away at once and was not).
            let one_step_of_gravity = if axis == 1 && before.2 && after.2 {
                f64::from(CharacterSettings::default().gravity) / 60.0 + 1e-3
            } else {
                0.0
            };
            assert!(
                (before.1[axis] - after.1[axis]).abs() < 2e-3f64.max(one_step_of_gravity),
                "stage {stage}, speed on axis {axis}: {:?} against {:?}",
                before.1,
                after.1
            );
        }
        assert_eq!(before.2, after.2, "stage {stage}: on the ground or not");
    }
    // And the script did something: it ended on the floor, away from where it started, having gone over the block.
    let start = [20.5, 12.0, 24.0];
    let end = plain.last().unwrap().0;
    let moved: f64 = (0..3)
        .map(|axis| (end[axis] - start[axis]) * (end[axis] - start[axis]))
        .sum();
    assert!(moved.sqrt() > 8.0, "{end:?}");
    assert!(plain.iter().any(|stage| stage.2));
}

#[test]
fn a_character_falls_along_the_gravity_it_is_given_and_rests_against_the_wall_it_falls_to() {
    // A wall whose face is at x = 10, and gravity pulling toward it: the character stands on the wall with its feet against it and
    // its eyes 1.62 out from it.
    let mut scene = Scene::of(|voxel| voxel.x() <= 9);
    let mut body = spawn(16.5, 14.0, 16.5);
    body.reframe(
        position(16.5, 14.0, 16.5),
        [0.0; 3],
        UP_BECOMES_PLUS_X.quaternion,
    );
    for _ in 0..30 {
        body.update_terrain(&mut scene);
        body.step_with_gravity(STILL, [-32.0, 0.0, 0.0]);
    }
    assert!(body.velocity()[0] < -3.0, "falling toward the wall");
    for _ in 0..150 {
        body.update_terrain(&mut scene);
        body.step_with_gravity(STILL, [-32.0, 0.0, 0.0]);
    }
    assert!(body.is_grounded());
    let resting = eye(&body);
    assert!((resting[0] - (10.0 + 1.62)).abs() < 0.05, "{resting:?}");
    assert!((resting[1] - 14.0).abs() < 1e-3 && (resting[2] - 16.5).abs() < 1e-3);
    assert_eq!(body.up(), [1.0, 0.0, 0.0]);
    // And it stays there without a tremor.
    for _ in 0..200 {
        body.update_terrain(&mut scene);
        body.step_with_gravity(STILL, [-32.0, 0.0, 0.0]);
        assert_eq!(eye(&body), resting);
        assert_eq!(body.velocity(), [0.0; 3]);
    }
    // Its feet are on the wall: along its up, the length of the body below the eyes.
    let feet = body.feet_position();
    let brick = feet.brick();
    let along_x = brick.x() as f64 * 32.0 + f64::from(feet.offset()[0]);
    assert!((along_x - 10.0).abs() < 0.05, "{along_x}");
}

/// A vector turned by the turn of a frame, a quaternion `[x, y, z, w]`.
fn rotated(rotation: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let ([x, y, z], c) = ([rotation[0], rotation[1], rotation[2]], rotation[3]);
    let q = [x, y, z];
    let t = [
        2.0 * (q[1] * v[2] - q[2] * v[1]),
        2.0 * (q[2] * v[0] - q[0] * v[2]),
        2.0 * (q[0] * v[1] - q[1] * v[0]),
    ];
    [
        v[0] + c * t[0] + (q[1] * t[2] - q[2] * t[1]),
        v[1] + c * t[1] + (q[2] * t[0] - q[0] * t[2]),
        v[2] + c * t[2] + (q[0] * t[1] - q[1] * t[0]),
    ]
}

#[test]
fn a_quarter_turn_of_gravity_in_one_step_carries_the_facing_the_same_way_as_a_gradual_one() {
    // The facing is carried by the turn between the two ups and then made to lie across the new up. With small steps that second part
    // hides a turn the wrong way (the facing stays in the plane the turn is in, and the part across the new up is the same either way);
    // in one big step it does not: it would give the opposite facing. Up goes from +y to +z in one step: what faced -z now faces +y.
    let mut body = spawn(5.0, 5.0, 5.0);
    body.step_with_gravity(STILL, [0.0, -32.0, 0.0]);
    body.step_with_gravity(STILL, [0.0, 0.0, -32.0]);
    assert_eq!(body.up(), [0.0, 0.0, 1.0]);
    let ahead = rotated(body.frame_rotation(), [0.0, 0.0, -1.0]);
    assert!(
        ahead[0].abs() < 1e-5 && (ahead[1] - 1.0).abs() < 1e-5 && ahead[2].abs() < 1e-5,
        "{ahead:?}"
    );
    // The other way about the same axis: up goes from +y to -z, and what faced -z now faces -y.
    let mut body = spawn(5.0, 5.0, 5.0);
    body.step_with_gravity(STILL, [0.0, -32.0, 0.0]);
    body.step_with_gravity(STILL, [0.0, 0.0, 32.0]);
    assert_eq!(body.up(), [0.0, 0.0, -1.0]);
    let ahead = rotated(body.frame_rotation(), [0.0, 0.0, -1.0]);
    assert!(
        ahead[0].abs() < 1e-5 && (ahead[1] + 1.0).abs() < 1e-5 && ahead[2].abs() < 1e-5,
        "{ahead:?}"
    );
}

#[test]
fn the_direction_the_character_faces_is_carried_as_gravity_turns_not_reset() {
    // Standing on a floor facing -z; the room tips so that up is along +z (a quarter turn about x): what was ahead is now above, and
    // what faced -z now faces the old up, +y. A tip about z only turns the character about its facing and leaves it facing -z.
    let mut scene = Scene::default();
    let mut body = spawn(5.0, 5.0, 5.0);
    body.step_with_gravity(STILL, [0.0, -32.0, 0.0]);
    assert_eq!(body.up(), [0.0, 1.0, 0.0]);
    let faces = |body: &CharacterBody| {
        // The turn of the frame applied to the direction it faces at yaw 0, (0, 0, -1), and to its right, (1, 0, 0).
        (
            rotated(body.frame_rotation(), [0.0, 0.0, -1.0]),
            rotated(body.frame_rotation(), [1.0, 0.0, 0.0]),
        )
    };
    let (ahead, right) = faces(&body);
    assert_eq!((ahead, right), ([0.0, 0.0, -1.0], [1.0, 0.0, 0.0]));

    // Gravity swings, over 45 steps, from -y to -z: up goes from +y to +z.
    for step in 1..=45 {
        let angle = std::f32::consts::FRAC_PI_2 * step as f32 / 45.0;
        let gravity = [0.0, -32.0 * libm::cosf(angle), -32.0 * libm::sinf(angle)];
        body.update_terrain(&mut scene);
        body.step_with_gravity(STILL, gravity);
    }
    let up = body.up();
    assert!(
        up[0].abs() < 1e-5 && up[1].abs() < 1e-5 && (up[2] - 1.0).abs() < 1e-5,
        "{up:?}"
    );
    let (ahead, right) = faces(&body);
    assert!(
        ahead[0].abs() < 1e-5 && (ahead[1] - 1.0).abs() < 1e-5 && ahead[2].abs() < 1e-5,
        "{ahead:?}"
    );
    assert!(
        (right[0] - 1.0).abs() < 1e-5 && right[1].abs() < 1e-5 && right[2].abs() < 1e-5,
        "{right:?}"
    );

    // A tip about z instead (gravity from -y toward -x) leaves it facing -z, the axis it turned about being its facing.
    let mut body = spawn(5.0, 5.0, 5.0);
    for step in 0..=45 {
        let angle = std::f32::consts::FRAC_PI_2 * step as f32 / 45.0;
        let gravity = [-32.0 * libm::sinf(angle), -32.0 * libm::cosf(angle), 0.0];
        body.update_terrain(&mut scene);
        body.step_with_gravity(STILL, gravity);
    }
    assert!((body.up()[0] - 1.0).abs() < 1e-5);
    let (ahead, right) = faces(&body);
    assert!(ahead[2] < -0.99999 && ahead[0].abs() < 1e-5 && ahead[1].abs() < 1e-5);
    assert!(right[1] < -0.99999, "{right:?}");
}

#[test]
fn turning_gravity_all_the_way_over_turns_about_the_characters_right_and_it_faces_the_other_way() {
    let mut scene = Scene::default();
    let mut body = spawn(5.0, 5.0, 5.0);
    body.step_with_gravity(STILL, [0.0, -32.0, 0.0]);
    body.update_terrain(&mut scene);
    // Gravity reversed in one tick: the room flipped. Up is -y, and the character, turned over about its own right, faces +z.
    body.step_with_gravity(STILL, [0.0, 32.0, 0.0]);
    assert_eq!(body.up(), [0.0, -1.0, 0.0]);
    let [x, y, z, w] = body.frame_rotation();
    let norm = (x * x + y * y + z * z + w * w).sqrt();
    assert!((norm - 1.0).abs() < 1e-5);
    // A half turn about x: (1, 0, 0, 0).
    assert!(
        (x.abs() - 1.0).abs() < 1e-5 && y.abs() < 1e-5 && z.abs() < 1e-5 && w.abs() < 1e-5,
        "{:?}",
        body.frame_rotation()
    );
}

#[test]
fn a_character_whose_gravity_is_nothing_drifts_at_the_speed_it_had_and_keeps_its_frame() {
    let mut scene = Scene::default();
    let mut body = spawn(5.0, 5.0, 5.0);
    // Falling toward -x for a moment, then nothing pulls it.
    for _ in 0..6 {
        body.update_terrain(&mut scene);
        body.step_with_gravity(STILL, [-32.0, 0.0, 0.0]);
    }
    let (up, rotation, speed) = (body.up(), body.frame_rotation(), body.velocity());
    assert!(speed[0] < -2.0, "{speed:?}");
    let mut places = vec![eye(&body)[0]];
    for _ in 0..3 {
        for _ in 0..30 {
            body.update_terrain(&mut scene);
            body.step_with_gravity(STILL, [0.0; 3]);
        }
        places.push(eye(&body)[0]);
    }
    // Neither the up nor the turn of the frame changed, nor the speed; so it went the same distance in each thirty steps.
    assert_eq!((body.up(), body.frame_rotation()), (up, rotation));
    assert_eq!(body.velocity(), speed);
    let legs: Vec<f64> = places.windows(2).map(|pair| pair[1] - pair[0]).collect();
    // Six steps of 32 for a sixtieth of a second each is 3.2 voxels a second, so half a second goes 1.6.
    assert!((legs[0] + 1.6).abs() < 1e-3, "{legs:?}");
    assert!(
        (legs[0] - legs[1]).abs() < 1e-3 && (legs[1] - legs[2]).abs() < 1e-3,
        "{legs:?}"
    );
}

#[test]
fn moving_a_character_to_another_frame_keeps_its_speed_and_state_and_puts_its_eyes_where_asked() {
    let mut scene = Scene::with_floor(9);
    let mut body = spawn(16.5, 14.0, 16.5);
    for _ in 0..90 {
        body.update_terrain(&mut scene);
        body.step(MoveInput {
            yaw: 0.3,
            ..FORWARD
        });
    }
    assert!(body.is_grounded());
    // Into a frame whose up is +x: a quarter turn about z.
    let half = std::f32::consts::FRAC_1_SQRT_2;
    let turn = [0.0, 0.0, -half, half]; // up (0, 1, 0) goes to (1, 0, 0)
    body.reframe(position(50.0, 60.0, 70.0), [1.0, 2.0, 3.0], turn);
    assert_eq!(eye(&body), [50.0, 60.0, 70.0]);
    assert_eq!(body.velocity(), [1.0, 2.0, 3.0]);
    assert!(body.is_grounded(), "standing on a ship is still standing");
    assert!(!body.is_flying());
    let up = body.up();
    assert!(
        (up[0] - 1.0).abs() < 1e-6 && up[1].abs() < 1e-6 && up[2].abs() < 1e-6,
        "{up:?}"
    );
    assert!(
        same_turn(body.frame_rotation(), turn),
        "{:?}",
        body.frame_rotation()
    );
    // Flying is kept across a move too.
    body.set_flying(true);
    body.reframe(position(1.0, 2.0, 3.0), [0.0; 3], [0.0, 0.0, 0.0, 1.0]);
    assert!(body.is_flying());
    assert_eq!(body.up(), [0.0, 1.0, 0.0]);
    assert_eq!(body.frame_rotation(), [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn a_character_put_in_the_ground_of_a_turned_frame_is_lifted_out_along_its_up() {
    // A wall with its face at x = 10, up along +x; the character is put 0.5 into it.
    let mut scene = Scene::of(|voxel| voxel.x() <= 9);
    let mut body = spawn(30.0, 14.0, 16.5);
    let half = std::f32::consts::FRAC_1_SQRT_2;
    // The feet are 1.62 below the eyes along up, so eyes at x = 11.12 put the feet at 9.5, half a voxel in.
    body.reframe(
        position(11.12, 14.0, 16.5),
        [0.0; 3],
        [0.0, 0.0, -half, half],
    );
    body.update_terrain(&mut scene);
    let lifted = eye(&body);
    // Lifted up (along +x) to the face and not sideways: the eyes end 1.62 above it.
    assert!(
        (lifted[0] - (10.0 + 1.62)).abs() < 0.15
            && (lifted[1] - 14.0).abs() < 1e-3
            && (lifted[2] - 16.5).abs() < 1e-3,
        "{lifted:?}"
    );
}

#[test]
fn a_character_put_into_a_block_of_a_turned_frame_leaves_it_by_the_first_of_the_ways_out_in_its_own_order()
 {
    // Up is +x. A single cube from (10, 10, 10) to (11, 11, 11), and the character put with the middle of its body at the middle of the
    // cube. Sideways, across the cube, is the nearest way out (0.5 and the half of the character's width 0.28 over: 0.875 in steps of an
    // eighth); up and down need the length of the body (1.5). The ways are tried in the character's own order: up, down, then the ways
    // across, which for up along +x begin with -y (the turn that takes +y to +x takes +x to -y). In the world's order +y would be first.
    let half = std::f32::consts::FRAC_1_SQRT_2;
    let mut scene = Scene::default();
    scene.cube(10, 10, 10);
    let mut body = spawn(30.0, 14.0, 16.5);
    // The middle of the body is 0.72 under the eyes along up, so the eyes are put at x = 10.5 + 0.72.
    body.reframe(
        position(11.22, 10.5, 10.5),
        [0.0; 3],
        [0.0, 0.0, -half, half],
    );
    body.update_terrain(&mut scene);
    let eyes = eye(&body);
    assert!(
        (eyes[0] - 11.22).abs() < 1e-3
            && (eyes[1] - (10.5 - 0.875)).abs() < 1e-3
            && (eyes[2] - 10.5).abs() < 1e-3,
        "{eyes:?}"
    );
}

#[test]
fn the_state_hash_tells_a_turned_frame_from_a_plain_one_and_a_plain_one_is_what_it_always_was() {
    let mut scene = Scene::with_floor(9);
    let mut plain = spawn(16.5, 14.0, 16.5);
    let mut turned = spawn(16.5, 14.0, 16.5);
    plain.update_terrain(&mut scene);
    turned.update_terrain(&mut scene);
    // Equal states hash equal; a body that has been moved to a frame with another up hashes otherwise even at the same place.
    assert_eq!(plain.state_hash(), turned.state_hash());
    turned.reframe(position(16.5, 15.62, 16.5), [0.0; 3], [1.0, 0.0, 0.0, 0.0]);
    plain.reframe(position(16.5, 15.62, 16.5), [0.0; 3], [0.0, 0.0, 0.0, 1.0]);
    assert_ne!(plain.state_hash(), turned.state_hash());
}

#[test]
fn feet_are_the_length_of_the_body_below_the_eyes_along_the_up() {
    let mut scene = Scene::with_floor(9);
    let mut body = spawn(16.5, 14.0, 16.5);
    for _ in 0..120 {
        body.update_terrain(&mut scene);
        body.step(STILL);
    }
    let feet = body.feet_position();
    let height = f64::from(feet.offset()[1]) + feet.brick().y() as f64 * 32.0;
    assert!((height - 10.0).abs() < 0.05, "{height}");
    // The middle of the body is half its height above them, and the eyes 1.62 above them.
    let middle = body.middle_position();
    let middle_height = f64::from(middle.offset()[1]) + middle.brick().y() as f64 * 32.0;
    assert!(
        (middle_height - height - 0.9).abs() < 1e-5,
        "{middle_height}"
    );
    let eyes = body.eye_position();
    let eye_height = f64::from(eyes.offset()[1]) + eyes.brick().y() as f64 * 32.0;
    assert!((eye_height - height - 1.62).abs() < 1e-5);
}
