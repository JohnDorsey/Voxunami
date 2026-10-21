//! A character walking, jumping, flying and falling through terrain made of plain maps.

mod common;

use block_models::{Shape, ShapeBox};
use common::{FORWARD, STILL, Scene, at, edited, eye, position, run, spawn};
use physics::{CharacterBody, CharacterSettings, Collision, CollisionSource, MoveInput};
use voxel_space::{BrickCoordinate, FaceDirection, VoxelCoordinate, WorldPosition};

/// A flat floor whose top is at height 10 (voxels up to 9 are solid) and a character standing on it
/// at (16.5, z = 16.5), with its eyes where a standing character's are.
fn standing_on_a_floor() -> (CharacterBody, Scene) {
    let mut scene = Scene::with_floor(9);
    let mut body = spawn(16.5, 14.0, 16.5);
    run(&mut body, &mut scene, STILL, 120);
    assert!(body.is_grounded());
    (body, scene)
}

const STANDING_EYE_HEIGHT: f64 = 10.0 + 1.62;

#[test]
fn a_character_falls_and_comes_to_rest_on_the_floor_and_stays_there_without_a_tremor() {
    let mut scene = Scene::with_floor(9);
    let mut body = spawn(16.5, 20.0, 16.5);
    // It is in the air at first, and falls.
    run(&mut body, &mut scene, STILL, 10);
    assert!(!body.is_grounded());
    assert!(body.velocity()[1] < -3.0);
    run(&mut body, &mut scene, STILL, 120);
    assert!(body.is_grounded());
    let resting = eye(&body);
    // The eyes are at standing height above the floor, give or take the small gap collision leaves.
    assert!(
        (resting[1] - STANDING_EYE_HEIGHT).abs() < 0.05,
        "{}",
        resting[1]
    );
    assert!((resting[0] - 16.5).abs() < 1e-4 && (resting[2] - 16.5).abs() < 1e-4);
    // And it does not move, not even a little, however long it stands there.
    for _ in 0..600 {
        run(&mut body, &mut scene, STILL, 1);
        assert_eq!(eye(&body), resting);
        assert!(body.is_grounded());
        assert_eq!(body.velocity(), [0.0; 3]);
    }
}

#[test]
fn walking_goes_the_way_the_player_faces_at_walking_speed_and_sprinting_goes_faster() {
    // Yaw 0 faces -z and a quarter turn to the left faces -x.
    for (yaw_degrees, direction) in [
        (0.0f32, [0.0, -1.0]),
        (90.0, [-1.0, 0.0]),
        (180.0, [0.0, 1.0]),
        (-90.0, [1.0, 0.0]),
    ] {
        let (mut body, mut scene) = standing_on_a_floor();
        let start = eye(&body);
        let input = MoveInput {
            yaw: yaw_degrees.to_radians(),
            ..FORWARD
        };
        run(&mut body, &mut scene, input, 60);
        let end = eye(&body);
        let moved = [end[0] - start[0], end[2] - start[2]];
        let along = moved[0] * direction[0] + moved[1] * direction[1];
        let across = moved[0] * direction[1] - moved[1] * direction[0];
        assert!((4.0..4.35).contains(&along), "yaw {yaw_degrees}: {along}");
        assert!(across.abs() < 0.01, "yaw {yaw_degrees}: {across}");
        assert!((end[1] - start[1]).abs() < 0.01, "it stays on the floor");
    }
    let (mut walker, mut scene) = standing_on_a_floor();
    let (mut sprinter, mut scene_2) = standing_on_a_floor();
    let start = eye(&walker)[2];
    run(&mut walker, &mut scene, FORWARD, 60);
    run(
        &mut sprinter,
        &mut scene_2,
        MoveInput {
            sprint: true,
            ..FORWARD
        },
        60,
    );
    let (walked, sprinted) = (start - eye(&walker)[2], start - eye(&sprinter)[2]);
    assert!(sprinted > walked * 1.2, "{walked} {sprinted}");
}

#[test]
fn walking_diagonally_is_no_faster_than_walking_straight() {
    let (mut straight, mut scene) = standing_on_a_floor();
    let (mut diagonal, mut scene_2) = standing_on_a_floor();
    let start = eye(&straight);
    run(&mut straight, &mut scene, FORWARD, 60);
    run(
        &mut diagonal,
        &mut scene_2,
        MoveInput {
            strafe: 1,
            ..FORWARD
        },
        60,
    );
    let length = |end: [f64; 3]| {
        (((end[0] - start[0]) * (end[0] - start[0])) + ((end[2] - start[2]) * (end[2] - start[2])))
            .sqrt()
    };
    assert!((length(eye(&straight)) - length(eye(&diagonal))).abs() < 0.05);
}

#[test]
fn a_wall_stops_the_character_and_it_slides_along_it() {
    let (mut body, mut scene) = standing_on_a_floor();
    // A wall of whole cubes across z = 10 (its near face is at z = 11), four high.
    for x in 0..40 {
        for y in 10..14 {
            scene.cube(x, y, 10);
        }
    }
    edited(&mut body, &mut scene);
    run(&mut body, &mut scene, FORWARD, 120);
    let at_wall = eye(&body);
    // The centre is a radius (0.3) from the wall's face at 11, plus the gap collision keeps.
    assert!((at_wall[2] - 11.31).abs() < 0.03, "{}", at_wall[2]);
    assert!(
        (at_wall[0] - 16.5).abs() < 1e-3,
        "straight on, it does not drift"
    );
    // Pushing against it on a slant slides along it.
    run(
        &mut body,
        &mut scene,
        MoveInput {
            strafe: 1,
            ..FORWARD
        },
        60,
    );
    let slid = eye(&body);
    assert!(slid[0] > at_wall[0] + 2.0, "{} {}", slid[0], at_wall[0]);
    assert!(
        (slid[2] - at_wall[2]).abs() < 0.02,
        "it stays against the wall"
    );
}

/// A floor topped at 7 with a wall along z of the blocks on it, one high (more than a character steps up), whose face is at x = 15 and
/// reaches from z = 0 to 41. A character on the floor at (`x`, `z`) walks with the yaw `yaw` and its keys held, which is toward -z and
/// leaning into the wall for a yaw above 0: `steps` steps. Returns where it ended and the most steps in a row that it did not move.
fn walk_into_a_step(yaw: f32, x: f32, z: f32, steps: usize) -> ([f64; 3], usize) {
    let mut scene = Scene::with_floor(6);
    for wall_x in 0..=14 {
        for wall_z in 0..=40 {
            scene.cube(wall_x, 7, wall_z);
        }
    }
    let mut body = spawn(x, 7.0 + 1.62 + 0.01, z);
    run(&mut body, &mut scene, STILL, 30);
    let input = MoveInput { yaw, ..FORWARD };
    let mut last = eye(&body);
    let (mut still, mut longest_still) = (0, 0);
    for _ in 0..steps {
        run(&mut body, &mut scene, input, 1);
        let now = eye(&body);
        if (now[0] - last[0]).abs() + (now[2] - last[2]).abs() < 1e-6 {
            still += 1;
            longest_still = longest_still.max(still);
        } else {
            still = 0;
        }
        last = now;
    }
    (last, longest_still)
}

#[test]
fn a_walker_pressed_into_a_wall_on_a_slant_slides_along_it_where_the_controller_alone_stops_dead() {
    // Found in Cave's golden session, where a player walking with the yaw 0.4 along a hillside stood still for a hundred steps: the
    // shape cast against the wall gives a normal with a hair of downward slope (about 1e-5), the slide along the wall has a hair of
    // downward speed, and the controller then takes the floor, which it meets at once, to be what the walker slides down, and
    // gives up with nothing moved. This is one such walk: it reaches the wall and sticks at z = 25.8 after 67 steps if nothing is done.
    let (end, longest_still) = walk_into_a_step(0.3, 15.6, 30.0, 150);
    assert_eq!(longest_still, 0, "it stood still, at {end:?}");
    // The walk along the wall is 4.3 cos(0.3) a second: more than 10 voxels in the 150 steps, less the few spent starting and arriving.
    assert!(end[2] < 30.0 - 9.0, "it only got to z = {}", end[2]);
    assert!(
        end[0] > 15.2 && end[0] < 15.5,
        "it stays at the wall: {end:?}"
    );
}

#[test]
fn a_walker_never_stops_dead_against_a_wall_that_is_free_to_slide_along_for_any_slant_and_place() {
    // About a fifth of these used to stick. (Every walk is along a wall that goes on for 20 voxels or more, so a slide is free.)
    let mut stuck = Vec::new();
    for yaw_hundredths in (10..130).step_by(5) {
        for start in 0..6 {
            let yaw = yaw_hundredths as f32 / 100.0;
            let x = 15.6 + start as f32 * 0.23;
            let (end, longest_still) = walk_into_a_step(yaw, x, 30.0, 150);
            if longest_still > 0 {
                stuck.push((yaw, x, end));
            }
        }
    }
    assert!(stuck.is_empty(), "stood still: {stuck:?}");
}

#[test]
fn a_jump_clears_a_block_and_a_quarter_and_not_two_blocks() {
    let (mut body, mut scene) = standing_on_a_floor();
    let start = eye(&body)[1];
    run(
        &mut body,
        &mut scene,
        MoveInput {
            jump: true,
            ..STILL
        },
        1,
    );
    let mut highest = start;
    for _ in 0..120 {
        run(&mut body, &mut scene, STILL, 1);
        highest = highest.max(eye(&body)[1]);
    }
    assert!(
        (1.15..1.35).contains(&(highest - start)),
        "{}",
        highest - start
    );
    assert!(body.is_grounded(), "and it came back down");
    assert!((eye(&body)[1] - start).abs() < 0.02);

    // A step one block high ahead: jumping while walking gets onto it.
    let (mut body, mut scene) = standing_on_a_floor();
    for x in 0..40 {
        scene.cube(x, 10, 12);
        scene.cube(x, 10, 11);
        scene.cube(x, 10, 10);
        scene.cube(x, 10, 9);
        scene.cube(x, 10, 8);
    }
    // (The block is on the floor everywhere from z = 8 to 12, and the character is at z = 16.5.)
    edited(&mut body, &mut scene);
    run(&mut body, &mut scene, FORWARD, 40);
    let blocked = eye(&body);
    assert!(
        blocked[1] < STANDING_EYE_HEIGHT + 0.05,
        "walking alone does not climb a block"
    );
    assert!(blocked[2] > 13.2, "{}", blocked[2]);
    // One jump, then walking on.
    run(
        &mut body,
        &mut scene,
        MoveInput {
            jump: true,
            ..FORWARD
        },
        1,
    );
    run(&mut body, &mut scene, FORWARD, 50);
    let on_top = eye(&body);
    assert!(body.is_grounded());
    assert!(
        (on_top[1] - (STANDING_EYE_HEIGHT + 1.0)).abs() < 0.06,
        "{}",
        on_top[1]
    );
    assert!(on_top[2] < 12.0, "it is on the block now");

    // A wall two blocks high cannot be jumped.
    let (mut body, mut scene) = standing_on_a_floor();
    for x in 0..40 {
        scene.cube(x, 10, 12);
        scene.cube(x, 11, 12);
    }
    // Hopping at it for a long time (the jump key held) never gets over it: the character is never
    // higher than a jump takes it and never gets past the wall.
    let mut highest = 0.0f64;
    for _ in 0..200 {
        run(
            &mut body,
            &mut scene,
            MoveInput {
                jump: true,
                ..FORWARD
            },
            1,
        );
        highest = highest.max(eye(&body)[1]);
        assert!(eye(&body)[2] > 13.0, "{}", eye(&body)[2]);
    }
    assert!(highest < STANDING_EYE_HEIGHT + 1.4, "{highest}");
}

#[test]
fn slabs_and_the_steps_of_stairs_are_walked_up_without_jumping_and_a_block_is_not() {
    // A bottom slab (half a block high) in the way.
    let (mut body, mut scene) = standing_on_a_floor();
    for x in 0..40 {
        for z in 5..13 {
            scene.shape(x, 10, z, &Shape::half(FaceDirection::NegativeY));
        }
    }
    edited(&mut body, &mut scene);
    run(&mut body, &mut scene, FORWARD, 90);
    let top = eye(&body);
    assert!(
        (top[1] - (STANDING_EYE_HEIGHT + 0.5)).abs() < 0.06,
        "{}",
        top[1]
    );
    assert!(top[2] < 12.5, "it walked onto the slabs: {}", top[2]);

    // And down again off the far edge, back to the floor.
    run(&mut body, &mut scene, FORWARD, 200);
    assert!(
        (eye(&body)[1] - STANDING_EYE_HEIGHT).abs() < 0.06,
        "{}",
        eye(&body)[1]
    );

    // A stair rising toward -x: each step of it is half a block, which a walking character climbs.
    let (mut body, mut scene) = standing_on_a_floor();
    let stair = Shape::stair(FaceDirection::NegativeX);
    for z in 0..40 {
        for x in 8..14 {
            scene.shape(x, 10, z, &stair);
        }
    }
    let yaw = 90.0f32.to_radians(); // toward -x
    edited(&mut body, &mut scene);
    run(&mut body, &mut scene, MoveInput { yaw, ..FORWARD }, 60);
    assert!(
        eye(&body)[1] > STANDING_EYE_HEIGHT + 0.4,
        "{}",
        eye(&body)[1]
    );
}

#[test]
fn a_ceiling_stops_a_jump_short_and_the_character_falls_back() {
    let (mut body, mut scene) = standing_on_a_floor();
    // A ceiling whose underside is at height 12: the head (1.8 up from the feet at 10) reaches 11.8.
    for x in 10..24 {
        for z in 10..24 {
            scene.cube(x, 12, z);
        }
    }
    edited(&mut body, &mut scene);
    let start = eye(&body)[1];
    run(
        &mut body,
        &mut scene,
        MoveInput {
            jump: true,
            ..STILL
        },
        1,
    );
    let mut highest = start;
    for _ in 0..30 {
        run(&mut body, &mut scene, STILL, 1);
        highest = highest.max(eye(&body)[1]);
    }
    assert!(highest - start < 0.25, "{}", highest - start);
    // Hitting the ceiling took away the speed upward at once, instead of leaving it to be used up
    // by gravity while the head pushes against the ceiling.
    let (mut probe, mut probe_scene) = standing_on_a_floor();
    for x in 10..24 {
        for z in 10..24 {
            probe_scene.cube(x, 12, z);
        }
    }
    run(
        &mut probe,
        &mut probe_scene,
        MoveInput {
            jump: true,
            ..STILL
        },
        1,
    );
    run(&mut probe, &mut probe_scene, STILL, 5);
    assert!(probe.velocity()[1] <= 0.0, "{}", probe.velocity()[1]);
    run(&mut body, &mut scene, STILL, 90);
    assert!(body.is_grounded());
    assert!((eye(&body)[1] - start).abs() < 0.02);
}

#[test]
fn a_fall_gets_no_faster_than_the_terminal_speed() {
    let mut scene = Scene::with_floor(9);
    let mut body = spawn(16.5, 2000.0, 16.5);
    run(&mut body, &mut scene, STILL, 400);
    assert_eq!(
        body.velocity()[1],
        -CharacterSettings::default().terminal_speed
    );
    assert!(eye(&body)[1] < 2000.0 - 200.0);
}

#[test]
fn a_flying_character_hovers_goes_up_and_down_and_is_still_stopped_by_walls() {
    let (mut body, mut scene) = standing_on_a_floor();
    body.set_flying(true);
    assert!(body.is_flying());
    let start = eye(&body);
    run(&mut body, &mut scene, STILL, 120);
    assert_eq!(eye(&body), start, "no gravity");
    run(
        &mut body,
        &mut scene,
        MoveInput {
            vertical: 1,
            ..STILL
        },
        60,
    );
    let climbed = eye(&body)[1] - start[1];
    assert!((8.0..11.5).contains(&climbed), "{climbed}");
    run(
        &mut body,
        &mut scene,
        MoveInput {
            vertical: -1,
            ..STILL
        },
        200,
    );
    // Down again, and stopped by the floor, not through it.
    assert!(
        (eye(&body)[1] - STANDING_EYE_HEIGHT).abs() < 0.06,
        "{}",
        eye(&body)[1]
    );
    // A wall in the way of a fast flier.
    for y in 8..40 {
        for x in 0..40 {
            scene.cube(x, y, 4);
        }
    }
    run(
        &mut body,
        &mut scene,
        MoveInput {
            forward: 1,
            sprint: true,
            ..STILL
        },
        120,
    );
    assert!(eye(&body)[2] > 5.2, "{}", eye(&body)[2]);
    // Stopping flying makes it fall again.
    body.set_flying(false);
    run(
        &mut body,
        &mut scene,
        MoveInput {
            vertical: 1,
            ..STILL
        },
        30,
    );
    assert!(body.velocity()[1] <= 0.0);
}

#[test]
fn removing_the_ground_under_the_character_lets_it_fall_and_building_it_back_catches_it() {
    let (mut body, mut scene) = standing_on_a_floor();
    for x in 14..19 {
        for z in 14..19 {
            for y in 6..10 {
                scene.remove(x, y, z);
            }
        }
    }
    // The terrain the character stands on changed: its brick is made again.
    edited(&mut body, &mut scene);
    run(&mut body, &mut scene, STILL, 15);
    assert!(!body.is_grounded());
    assert!(
        eye(&body)[1] < STANDING_EYE_HEIGHT - 1.0,
        "{}",
        eye(&body)[1]
    );
    // It lands on what is under the hole, five blocks down.
    run(&mut body, &mut scene, STILL, 120);
    assert!(body.is_grounded());
    assert!(
        (eye(&body)[1] - (6.0 + 1.62)).abs() < 0.06,
        "{}",
        eye(&body)[1]
    );
    // Edits to a brick far from the character do nothing.
    body.brick_changed(&mut scene, BrickCoordinate::new(50, 50, 50));
}

#[test]
fn terrain_that_is_not_available_is_a_wall_not_a_void() {
    let mut scene = Scene::with_floor(9);
    scene.unavailable_beyond_x = Some(20);
    let mut body = spawn(16.5, 11.7, 16.5);
    run(
        &mut body,
        &mut scene,
        MoveInput {
            yaw: (-90.0f32).to_radians(),
            ..FORWARD
        },
        120,
    );
    assert!(eye(&body)[0] < 21.0, "{}", eye(&body)[0]);
    assert!(body.is_grounded());
    // And a character over ground that is not there at all stops where the world does, instead of
    // falling forever: nothing below height 0 is available, so that is the floor.
    let mut scene = Scene {
        unavailable_below_y: Some(0),
        ..Scene::default()
    };
    let mut body = spawn(16.5, 11.0, 16.5);
    run(&mut body, &mut scene, STILL, 600);
    assert!(body.is_grounded());
    assert!((eye(&body)[1] - 1.62).abs() < 0.06, "{}", eye(&body)[1]);
}

#[test]
fn a_long_walk_across_many_bricks_is_smooth_and_the_frame_is_rebuilt_without_a_jolt() {
    let mut scene = Scene::with_floor(9);
    let mut body = spawn(16.5, 11.7, 16.5);
    run(&mut body, &mut scene, STILL, 60);
    let mut last = eye(&body);
    let input = MoveInput {
        sprint: true,
        yaw: (-90.0f32).to_radians(),
        ..FORWARD
    };
    let first_origin = body.frame_origin();
    let mut moves = Vec::new();
    for step in 0..2700 {
        run(&mut body, &mut scene, input, 1);
        let now = eye(&body);
        let moved = now[0] - last[0];
        if step > 30 {
            moves.push(moved);
        }
        assert!((now[1] - last[1]).abs() < 0.02, "step {step}: it bobbed");
        assert!((now[2] - last[2]).abs() < 0.001, "step {step}: it drifted");
        last = now;
    }
    // 2700 steps at the sprinting speed of 5.6 a second, 45 seconds: about 250 voxels, farther than a
    // frame reaches (6 bricks), so it is rebuilt around the character. The speed
    // hardly changes, however many brick boundaries and frame rebuilds it crosses: no step is more
    // than a third short, and almost none is short at all.
    assert!(last[0] > 16.5 + 230.0, "{}", last[0]);
    // The frame was rebuilt around the character, and is near it.
    let origin = body.frame_origin();
    assert_ne!(origin, first_origin);
    assert!(origin.x() >= 5, "{origin:?}");
    let fastest = moves.iter().copied().fold(0.0f64, f64::max);
    assert!(
        moves.iter().all(|&moved| moved > fastest * 0.7),
        "{:?}",
        moves
            .iter()
            .enumerate()
            .filter(|&(_, &moved)| moved <= fastest * 0.7)
            .collect::<Vec<_>>()
    );
    let short = moves
        .iter()
        .filter(|&&moved| moved < fastest * 0.99)
        .count();
    assert!(short * 100 < moves.len(), "{short} of {}", moves.len());
}

#[test]
fn bricks_known_to_be_empty_are_not_asked_about_voxel_by_voxel() {
    let mut scene = Scene::with_floor(9);
    let mut body = spawn(16.5, 90.0, 16.5);
    body.update_terrain(&mut scene);
    // 27 bricks, and the ones up in the sky are never asked about.
    assert!(scene.asked < 27 * 32 * 32 * 32 / 2, "{}", scene.asked);
}

/// A scripted run: falling, walking, jumping onto a block, climbing a slab, flying and walking into a
/// wall. The hash of the state at the end must be the same on every machine.
fn scripted_run() -> u64 {
    let mut scene = Scene::with_floor(9);
    for x in 0..80 {
        scene.cube(x, 10, 5);
        for z in 20..24 {
            scene.shape(x, 10, z, &Shape::half(FaceDirection::NegativeY));
        }
    }
    let mut body = spawn(31.7, 25.3, 40.1);
    let mut hash = 0u64;
    let script = [
        (STILL, 100),
        (
            MoveInput {
                yaw: 0.4,
                ..FORWARD
            },
            90,
        ),
        (
            MoveInput {
                yaw: 0.4,
                jump: true,
                ..FORWARD
            },
            5,
        ),
        (
            MoveInput {
                yaw: -2.0,
                strafe: 1,
                ..FORWARD
            },
            120,
        ),
        (
            MoveInput {
                yaw: 3.0,
                sprint: true,
                ..FORWARD
            },
            80,
        ),
        (
            MoveInput {
                yaw: 0.0,
                jump: true,
                sprint: true,
                ..FORWARD
            },
            200,
        ),
    ];
    for (input, count) in script {
        run(&mut body, &mut scene, input, count);
        hash = hash.rotate_left(7) ^ body.state_hash();
    }
    body.set_flying(true);
    run(
        &mut body,
        &mut scene,
        MoveInput {
            vertical: 1,
            forward: 1,
            yaw: 1.0,
            ..STILL
        },
        100,
    );
    hash = hash.rotate_left(7) ^ body.state_hash();
    hash
}

#[test]
fn a_scripted_run_ends_in_the_same_state_every_time_and_on_every_machine() {
    let first = scripted_run();
    assert_eq!(first, scripted_run());
    // The value was taken on Windows and the test passes on Linux too, so the two agree to the bit.
    assert_eq!(first, PINNED_SCRIPTED_RUN);
}

/// Puts the character with its eyes where a character whose feet are at `feet` has them, at (x, z).
fn teleport_feet(body: &mut CharacterBody, x: f32, feet: f32, z: f32) {
    body.teleport(position(x, feet + 1.62, z));
}

/// Lets a character that was just put somewhere take its first steps, and settle.
fn settle(body: &mut CharacterBody, scene: &mut Scene) {
    run(body, scene, STILL, 90);
}

fn feet_of(body: &CharacterBody) -> f64 {
    eye(body)[1] - 1.62
}

#[test]
fn a_character_teleported_into_the_ground_is_lifted_onto_it_not_left_in_it() {
    // Terrain whose surface is at 10; the feet are put 1, 2 and 7 voxels below it, and a few hundredths.
    for depth in [0.05_f32, 1.0, 2.0, 7.0] {
        let (mut body, mut scene) = standing_on_a_floor();
        teleport_feet(&mut body, 16.5, 10.0 - depth, 16.5);
        settle(&mut body, &mut scene);
        assert!(body.is_grounded(), "depth {depth}");
        assert!(
            (feet_of(&body) - 10.0).abs() < 0.03,
            "depth {depth}: feet at {}",
            feet_of(&body)
        );
        assert!(!is_inside_something(&body, &mut scene), "depth {depth}");
    }
}

#[test]
fn a_character_spawned_inside_the_ground_is_lifted_onto_it_too() {
    // Deep in the ground, where nothing could push the character out by itself.
    let mut scene = Scene::with_floor(9);
    let mut body = spawn(16.5, 10.0 - 7.0 + 1.62, 16.5);
    settle(&mut body, &mut scene);
    assert!((feet_of(&body) - 10.0).abs() < 0.03, "{}", feet_of(&body));
    assert!(!is_inside_something(&body, &mut scene));
}

#[test]
fn lifting_goes_across_the_edge_of_a_brick_into_terrain_that_was_not_loaded() {
    // Solid up to height 70: from the brick at 0 the surface is two bricks up, past what is made around it.
    let mut scene = Scene::with_floor(69);
    let mut body = spawn(16.5, 80.0, 16.5);
    settle(&mut body, &mut scene);
    teleport_feet(&mut body, 16.5, 20.0, 16.5);
    settle(&mut body, &mut scene);
    assert!((feet_of(&body) - 70.0).abs() < 0.03, "{}", feet_of(&body));
    assert!(!is_inside_something(&body, &mut scene));
}

/// After settling, a character that was put somewhere is clear of everything, on the ground, and moved no
/// more than `at_most` (in x and z) from where it was put.
fn assert_cleared(
    body: &CharacterBody,
    scene: &mut Scene,
    put_at: [f64; 2],
    at_most: f64,
    what: &str,
) {
    let eye = eye(body);
    assert!(
        !is_inside_something(body, scene),
        "{what}: still inside something, eye {eye:?}"
    );
    assert!(body.is_grounded(), "{what}: not on the ground, eye {eye:?}");
    assert!(
        (eye[0] - put_at[0]).abs() <= at_most && (eye[2] - put_at[1]).abs() <= at_most,
        "{what}: moved from {put_at:?} to {eye:?}"
    );
}

#[test]
fn a_character_put_beside_a_taller_step_is_moved_the_shortest_way_out_of_it_for_any_overlap() {
    // A floor with a one-voxel step at x = 17, and the east edge of the character `overlap` into the step.
    // The shortest way out is sideways, back off the step, not up onto it.
    for overlap in [0.03_f32, 0.08, 0.15, 0.29] {
        let (mut body, mut scene) = standing_on_a_floor();
        for z in 14..=19 {
            scene.cube(17, 10, z);
        }
        let x = 17.0 + overlap - 0.3;
        teleport_feet(&mut body, x, 10.0, 16.5);
        settle(&mut body, &mut scene);
        let what = format!("overlap {overlap}");
        assert_cleared(
            &body,
            &mut scene,
            [f64::from(x), 16.5],
            f64::from(overlap) + 0.15,
            &what,
        );
        assert!(
            (feet_of(&body) - 10.0).abs() < 0.03,
            "{what}: feet {}",
            feet_of(&body)
        );
        assert!(
            eye(&body)[0] < f64::from(x),
            "{what}: moved away from the step"
        );
    }
}

#[test]
fn terrain_is_made_where_a_lifted_character_ends_up_before_its_first_step() {
    // Solid up to height 70, the character lifted two bricks up, past what was made around it. The game steps
    // a character three times for each update of its terrain: it must not sink into the ground in those steps
    // for want of colliders where it now is.
    let mut scene = Scene::with_floor(69);
    let mut body = spawn(16.5, 80.0, 16.5);
    settle(&mut body, &mut scene);
    teleport_feet(&mut body, 16.5, 20.0, 16.5);
    body.update_terrain(&mut scene);
    for step in 0..3 {
        body.step(STILL);
        assert!(
            feet_of(&body) > 70.0 - 0.02,
            "step {step}: feet at {}, sunk into the ground",
            feet_of(&body)
        );
    }
}

#[test]
fn a_character_put_deep_in_a_mass_of_blocks_that_are_not_whole_is_moved_out_of_it() {
    // A thick patch of blocks seven eighths high, stacked three high: there is no ground to fall back on
    // inside it, and its parts are in each other's way.
    let tall = Shape::from_boxes(&[ShapeBox {
        min: [0, 0, 0],
        max: [8, 7, 8],
    }]);
    for feet in [10.3_f32, 11.2, 12.5] {
        let (mut body, mut scene) = standing_on_a_floor();
        for x in 12..=21 {
            for z in 12..=21 {
                for y in 10..13 {
                    scene.shape(x, y, z, &tall);
                }
            }
        }
        edited(&mut body, &mut scene);
        teleport_feet(&mut body, 16.5, feet, 16.5);
        settle(&mut body, &mut scene);
        assert!(
            !is_inside_something(&body, &mut scene),
            "feet put at {feet}: eye {:?}",
            eye(&body)
        );
        assert!(
            feet_of(&body) > 9.9,
            "feet put at {feet}: {}",
            feet_of(&body)
        );
    }
}

#[test]
fn a_character_put_in_the_air_or_on_the_ground_is_not_lifted() {
    let (mut body, mut scene) = standing_on_a_floor();
    // Standing: put where it already is.
    teleport_feet(&mut body, 16.5, 10.0, 16.5);
    settle(&mut body, &mut scene);
    assert!((feet_of(&body) - 10.0).abs() < 0.03);
    // In the air: it falls from where it was put, not from higher.
    teleport_feet(&mut body, 16.5, 13.0, 16.5);
    run(&mut body, &mut scene, STILL, 1);
    assert!(feet_of(&body) <= 13.0, "{}", feet_of(&body));
    assert!(!body.is_grounded());
    settle(&mut body, &mut scene);
    assert!((feet_of(&body) - 10.0).abs() < 0.03);
}

#[test]
fn a_character_put_inside_a_slab_is_lifted_onto_the_top_of_the_slab() {
    // A floor with a bottom slab (half a block) on it; the feet are put in the slab, a few hundredths and
    // a quarter of a block up from the floor.
    for height in [0.02_f32, 0.25] {
        let (mut body, mut scene) = standing_on_a_floor();
        scene.shape(16, 10, 16, &Shape::half(FaceDirection::NegativeY));
        edited(&mut body, &mut scene);
        teleport_feet(&mut body, 16.5, 10.0 + height, 16.5);
        settle(&mut body, &mut scene);
        assert!(
            (feet_of(&body) - 10.5).abs() < 0.03,
            "put {height} up in the slab: feet at {}",
            feet_of(&body)
        );
        assert!(!is_inside_something(&body, &mut scene));
    }
}

#[test]
fn a_character_put_beside_a_step_that_is_not_a_whole_block_is_moved_out_of_it_too() {
    // A line of blocks seven eighths high, more than a character steps up.
    let tall = Shape::from_boxes(&[ShapeBox {
        min: [0, 0, 0],
        max: [8, 7, 8],
    }]);
    for overlap in [0.03_f32, 0.15, 0.29] {
        let (mut body, mut scene) = standing_on_a_floor();
        for z in 14..=19 {
            scene.shape(17, 10, z, &tall);
        }
        edited(&mut body, &mut scene);
        let x = 17.0 + overlap - 0.3;
        teleport_feet(&mut body, x, 10.0, 16.5);
        settle(&mut body, &mut scene);
        let what = format!("overlap {overlap}");
        assert_cleared(
            &body,
            &mut scene,
            [f64::from(x), 16.5],
            f64::from(overlap) + 0.15,
            &what,
        );
        assert!(
            (feet_of(&body) - 10.0).abs() < 0.03,
            "{what}: feet {}",
            feet_of(&body)
        );
    }
}

/// A floor topped at 10 with a room over part of it: a ceiling from 12 up to `roof_top` over `10..=22` in
/// x and z (a tunnel two blocks high under it).
fn tunnel_under(roof_top: i64) -> (CharacterBody, Scene) {
    let (mut body, mut scene) = standing_on_a_floor();
    for x in 10..=22 {
        for z in 10..=22 {
            for y in 12..=roof_top {
                scene.cube(x, y, z);
            }
        }
    }
    edited(&mut body, &mut scene);
    (body, scene)
}

#[test]
fn a_character_put_with_its_head_in_a_ceiling_goes_down_into_the_room_not_up_through_the_roof() {
    // Feet a little up, so that the head (1.8 above them) is in the roof; a thin roof and a very thick one.
    for roof_top in [12_i64, 200] {
        for feet in [10.3_f32, 10.5] {
            let (mut body, mut scene) = tunnel_under(roof_top);
            teleport_feet(&mut body, 16.5, feet, 16.5);
            settle(&mut body, &mut scene);
            let what = format!("roof to {roof_top}, feet {feet}");
            assert_cleared(&body, &mut scene, [16.5, 16.5], 0.01, &what);
            assert!(
                (feet_of(&body) - 10.0).abs() < 0.03,
                "{what}: feet {}",
                feet_of(&body)
            );
        }
    }
}

#[test]
fn a_character_put_in_a_thin_wall_goes_out_sideways_not_up_over_it() {
    // A wall one block thick and a hundred and fifty high, the character in the middle of it.
    let (mut body, mut scene) = standing_on_a_floor();
    for y in 10..=160 {
        for z in 0..=40 {
            scene.cube(16, y, z);
        }
    }
    edited(&mut body, &mut scene);
    teleport_feet(&mut body, 16.5, 10.0, 16.5);
    settle(&mut body, &mut scene);
    assert_cleared(&body, &mut scene, [16.5, 16.5], 1.0, "in a wall");
    assert!(
        (feet_of(&body) - 10.0).abs() < 0.03,
        "feet {}",
        feet_of(&body)
    );
}

#[test]
fn clearing_works_for_negative_coordinates_and_a_frame_that_is_not_at_the_origin() {
    // A step across the edge of a brick, and a character put in it, in bricks with negative coordinates and
    // far from the origin; the frame of its terrain is made around where it starts, not at the origin.
    for (brick_x, brick_z) in [(-1_i64, -1_i64), (-3, -4), (4, 5)] {
        let mut scene = Scene::with_floor(9);
        let base_x = brick_x * 32;
        let base_z = brick_z * 32;
        for z in base_z + 12..=base_z + 20 {
            scene.cube(base_x + 30, 10, z);
            scene.cube(base_x + 31, 10, z);
            scene.cube(base_x + 32, 10, z);
        }
        let put = |offset_x: f32| {
            WorldPosition::new(
                BrickCoordinate::new(brick_x, 0, brick_z),
                [offset_x, 10.0 + 1.62, 16.5],
            )
        };
        let put_z = base_z as f64 + 16.5;
        let what = format!("bricks ({brick_x}, {brick_z})");
        // In the middle of the step, and then across the edge between two bricks.
        let mut body = CharacterBody::new(put(31.5), CharacterSettings::default());
        settle(&mut body, &mut scene);
        assert_cleared(&body, &mut scene, [base_x as f64 + 31.5, put_z], 2.0, &what);
        body.teleport(put(31.95));
        settle(&mut body, &mut scene);
        assert_cleared(
            &body,
            &mut scene,
            [base_x as f64 + 31.95, put_z],
            2.0,
            &what,
        );
    }
}

#[test]
fn a_character_left_alone_where_it_stands_is_not_moved_at_all() {
    // Put where it already is, after standing, a jump, a walk or flying, it must not move by a hair:
    // clearing is for characters inside terrain, not a nudge.
    let (mut body, mut scene) = standing_on_a_floor();
    for _ in 0..4 {
        let before = eye(&body);
        body.teleport(body.eye_position());
        body.update_terrain(&mut scene);
        assert_eq!(eye(&body), before);
        run(
            &mut body,
            &mut scene,
            MoveInput {
                jump: true,
                ..FORWARD
            },
            40,
        );
        run(&mut body, &mut scene, FORWARD, 30);
    }
    body.set_flying(true);
    for _ in 0..3 {
        let before = eye(&body);
        body.teleport(body.eye_position());
        body.update_terrain(&mut scene);
        assert_eq!(eye(&body), before);
        run(
            &mut body,
            &mut scene,
            MoveInput {
                vertical: 1,
                ..STILL
            },
            20,
        );
    }
}

#[test]
fn a_flying_character_put_inside_the_ground_is_moved_out_of_it_too() {
    let (mut body, mut scene) = standing_on_a_floor();
    body.set_flying(true);
    teleport_feet(&mut body, 16.5, 7.0, 16.5);
    run(&mut body, &mut scene, STILL, 10);
    assert!(!is_inside_something(&body, &mut scene), "{:?}", eye(&body));
    assert!((feet_of(&body) - 10.0).abs() < 0.2, "{}", feet_of(&body));
}

#[test]
fn the_edge_of_the_world_that_the_search_runs_into_counts_as_solid_and_does_not_panic() {
    use voxel_space::BRICK_COORDINATE_MAXIMUM;
    /// Everything solid, everywhere.
    struct Solid;
    impl CollisionSource for Solid {
        fn collision_at(&mut self, _: VoxelCoordinate) -> Collision {
            Collision::Full
        }
    }
    // One brick below the top of the world: the search for room, 64 voxels each way, crosses the top.
    let top = WorldPosition::new(
        BrickCoordinate::new(0, BRICK_COORDINATE_MAXIMUM - 1, 0),
        [16.0, 20.0, 16.0],
    );
    let mut body = CharacterBody::new(top, CharacterSettings::default());
    body.update_terrain(&mut Solid);
    assert_eq!(
        body.eye_position(),
        top,
        "no room anywhere: left where it was put"
    );
}

#[test]
fn a_character_put_where_there_is_no_clear_space_above_stays_where_it_was_put_and_nothing_hangs() {
    // Solid for hundreds of voxels up: lifting finds no room, and the character is left where it was put,
    // not carried up to the limit of the search.
    let mut scene = Scene::with_floor(5000);
    let mut body = spawn(16.5, 20.0, 16.5);
    run(&mut body, &mut scene, STILL, 5);
    assert!(
        (feet_of(&body) - (20.0 - 1.62)).abs() < 2.0,
        "{}",
        feet_of(&body)
    );
}

const PINNED_SCRIPTED_RUN: u64 = 7_763_444_616_874_213_092;

/// Whether the character's box reaches into any solid thing of the scene by more than a hair.
fn is_inside_something(body: &CharacterBody, scene: &mut Scene) -> bool {
    let settings = CharacterSettings::default();
    let eye = eye(body);
    let center = [
        eye[0],
        eye[1] - f64::from(settings.eye_height - settings.height / 2.0),
        eye[2],
    ];
    let half = [
        f64::from(settings.radius),
        f64::from(settings.height / 2.0),
        f64::from(settings.radius),
    ];
    let tolerance = 0.06;
    let low = [0, 1, 2].map(|axis| (center[axis] - half[axis]).floor() as i64);
    let high = [0, 1, 2].map(|axis| (center[axis] + half[axis]).floor() as i64);
    for x in low[0]..=high[0] {
        for y in low[1]..=high[1] {
            for z in low[2]..=high[2] {
                let boxes: Vec<([f64; 3], [f64; 3])> = match scene.collision_at(at(x, y, z)) {
                    Collision::Empty => Vec::new(),
                    Collision::Full => vec![([0.0; 3], [1.0; 3])],
                    Collision::Partial(parts) => parts
                        .iter()
                        .map(|part| {
                            (
                                part.min.map(|value| f64::from(value) / 8.0),
                                part.max.map(|value| f64::from(value) / 8.0),
                            )
                        })
                        .collect(),
                };
                for (min, max) in boxes {
                    let cell = [x as f64, y as f64, z as f64];
                    let overlaps = (0..3).all(|axis| {
                        let overlap = (center[axis] + half[axis]).min(cell[axis] + max[axis])
                            - (center[axis] - half[axis]).max(cell[axis] + min[axis]);
                        overlap > tolerance
                    });
                    if overlaps {
                        return true;
                    }
                }
            }
        }
    }
    false
}

mod properties {
    use proptest::prelude::*;

    use super::*;

    /// A random scene on a floor of height 10: whole cubes and slabs in a patch near the character.
    fn scene() -> impl Strategy<Value = Vec<(i64, i64, i64, u8)>> {
        proptest::collection::vec((10i64..24, 10i64..14, 10i64..24, 0u8..3), 0..60)
    }

    /// The same, with a fourth kind of block: seven eighths of a cube, more than a character steps up.
    fn scene_with_tall_blocks() -> impl Strategy<Value = Vec<(i64, i64, i64, u8)>> {
        proptest::collection::vec((10i64..24, 10i64..16, 10i64..24, 0u8..4), 0..80)
    }

    fn input() -> impl Strategy<Value = MoveInput> {
        (
            -1i8..=1,
            -1i8..=1,
            -1i8..=1,
            any::<bool>(),
            any::<bool>(),
            -3.2f32..3.2,
        )
            .prop_map(|(forward, strafe, vertical, jump, sprint, yaw)| MoveInput {
                forward,
                strafe,
                vertical,
                jump,
                sprint,
                yaw,
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(96))]

        /// However the character is steered through a scene of cubes and slabs, walking or flying,
        /// it never ends a step inside anything solid, and it never falls through the floor.
        #[test]
        fn a_character_is_never_inside_anything_solid(
            blocks in scene(),
            script in proptest::collection::vec((input(), 1usize..25), 1..14),
            flying in any::<bool>(),
        ) {
            let mut scene = Scene::with_floor(9);
            for (x, y, z, kind) in blocks {
                match kind {
                    0 => scene.cube(x, y, z),
                    1 => scene.shape(x, y, z, &Shape::half(FaceDirection::NegativeY)),
                    _ => scene.shape(x, y, z, &Shape::stair(FaceDirection::PositiveX)),
                }
            }
            let mut body = spawn(8.5, 12.0, 8.5);
            body.set_flying(flying);
            let mut scene_for_check = Scene::with_floor(9);
            scene_for_check.cubes = scene.cubes.clone();
            for (input, count) in script {
                for _ in 0..count {
                    run(&mut body, &mut scene, input, 1);
                    prop_assert!(!is_inside_something(&body, &mut scene_for_check), "{:?}", eye(&body));
                    prop_assert!(eye(&body)[1] > 10.0 + 1.0, "fell through the floor: {:?}", eye(&body));
                }
            }
        }

        /// However a character is put into a scene of cubes, slabs, stairs and blocks that are not whole,
        /// walking or flying, at any depth in the floor or in the blocks, it ends clear of everything, above
        /// the floor, and not carried off far.
        #[test]
        fn a_character_put_anywhere_is_moved_out_of_the_terrain_and_not_left_in_it(
            blocks in scene_with_tall_blocks(),
            put in (12.0f32..22.0, 2.0f32..16.0, 12.0f32..22.0),
            flying in any::<bool>(),
            spawned in any::<bool>(),
        ) {
            let mut scene = Scene::with_floor(9);
            let tall = Shape::from_boxes(&[ShapeBox { min: [0, 0, 0], max: [8, 7, 8] }]);
            for (x, y, z, kind) in blocks {
                match kind {
                    0 => scene.cube(x, y, z),
                    1 => scene.shape(x, y, z, &Shape::half(FaceDirection::NegativeY)),
                    2 => scene.shape(x, y, z, &tall),
                    _ => scene.shape(x, y, z, &Shape::stair(FaceDirection::PositiveX)),
                }
            }
            let (x, feet, z) = put;
            let mut body = if spawned {
                spawn(x, feet + 1.62, z)
            } else {
                let mut body = spawn(8.5, 12.0, 8.5);
                body.set_flying(flying);
                run(&mut body, &mut scene, STILL, 5);
                teleport_feet(&mut body, x, feet, z);
                body
            };
            body.set_flying(flying);
            run(&mut body, &mut scene, STILL, 120);
            prop_assert!(!is_inside_something(&body, &mut scene), "put at {:?}, now {:?}", put, eye(&body));
            prop_assert!(feet_of(&body) > 9.9, "below the floor: put at {:?}, now {:?}", put, eye(&body));
            prop_assert!(feet_of(&body) < 30.0, "carried off: put at {:?}, now {:?}", put, eye(&body));
        }

        /// Hills: columns of whole cubes of random heights, the character put with its feet inside the top of
        /// the tallest column its body overlaps (by a little, a whole block or more), at any place, as when a
        /// player is sent to a level whose ground is higher than where they stood. It ends clear of
        /// everything and is not carried further than the way straight out of where it was put.
        #[test]
        fn a_character_put_into_the_top_of_hills_is_moved_out_of_them(
            heights in proptest::collection::vec(0i64..4, 15 * 15),
            at in (14.0f32..20.0, 14.0f32..20.0),
            depth in prop_oneof![Just(0.3f32), Just(1.0f32), Just(1.7f32)],
        ) {
            let mut scene = Scene::with_floor(9);
            let height_of = |x: i64, z: i64| heights[((z - 10) * 15 + (x - 10)) as usize];
            for x in 10..25 {
                for z in 10..25 {
                    for y in 10..10 + height_of(x, z) {
                        scene.cube(x, y, z);
                    }
                }
            }
            let (x, z) = at;
            let columns = |centre: f32| [(centre - 0.28).floor() as i64, (centre + 0.28).floor() as i64];
            let tallest = columns(x)
                .into_iter()
                .flat_map(|cx| columns(z).map(move |cz| height_of(cx, cz)))
                .max()
                .unwrap();
            let mut body = spawn(8.5, 12.0, 8.5);
            run(&mut body, &mut scene, STILL, 5);
            teleport_feet(&mut body, x, (10 + tallest) as f32 - depth, z);
            run(&mut body, &mut scene, STILL, 120);
            prop_assert!(!is_inside_something(&body, &mut scene), "put at {:?} depth {}, now {:?}", at, depth, eye(&body));
            // (How exactly it rests is the controller's: put within a hair of two walls at once, a body can be
            // left standing a little above the ground it falls beside. What clearing promises is that it is
            // not inside anything and not carried off.)
            // The shortest way out is never longer than straight up out of the depth it was put at, plus the
            // step of the search and the hair of the margin, so it is moved no further than that.
            let at_most = f64::from(depth) + 0.25;
            prop_assert!(
                (eye(&body)[0] - f64::from(x)).abs() <= at_most
                    && (eye(&body)[2] - f64::from(z)).abs() <= at_most
                    && feet_of(&body) <= (10 + tallest) as f64 + 0.25,
                "carried off: put at {:?} depth {}, now {:?}",
                at,
                depth,
                eye(&body)
            );
        }
    }
}
