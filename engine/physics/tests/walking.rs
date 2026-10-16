//! A character walking, jumping, flying and falling through terrain made of plain maps.

use std::collections::HashMap;
use std::sync::Arc;

use block_models::{Shape, ShapeBox};
use physics::{BrickHint, CharacterBody, CharacterSettings, Collision, CollisionSource, MoveInput};
use voxel_space::{BrickCoordinate, FaceDirection, VoxelCoordinate, WorldPosition};

/// A world of air above a floor, with whatever else is put in it. Looking at what the character asks
/// about is counted, to see that the work stays small.
#[derive(Default)]
struct Scene {
    /// Everything at or below this height is a whole cube. `None` for no floor.
    floor: Option<i64>,
    cubes: HashMap<VoxelCoordinate, Collision>,
    /// Voxels beyond this distance along +x are not available: whole cubes, as the contract says.
    unavailable_beyond_x: Option<i64>,
    /// Voxels below this height are not available either.
    unavailable_below_y: Option<i64>,
    asked: usize,
    /// Edited since the character was told.
    dirty: bool,
}

impl Scene {
    fn with_floor(top_voxel: i64) -> Self {
        Self {
            floor: Some(top_voxel),
            ..Self::default()
        }
    }

    fn cube(&mut self, x: i64, y: i64, z: i64) {
        self.dirty = true;
        self.cubes.insert(at(x, y, z), Collision::Full);
    }

    fn remove(&mut self, x: i64, y: i64, z: i64) {
        self.dirty = true;
        self.cubes.insert(at(x, y, z), Collision::Empty);
    }

    fn shape(&mut self, x: i64, y: i64, z: i64, shape: &Shape) {
        let boxes: Arc<[ShapeBox]> = shape.boxes().to_vec().into();
        self.dirty = true;
        self.cubes.insert(at(x, y, z), Collision::Partial(boxes));
    }
}

impl CollisionSource for Scene {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        self.asked += 1;
        if self
            .unavailable_beyond_x
            .is_some_and(|limit| voxel.x() > limit)
            || self
                .unavailable_below_y
                .is_some_and(|limit| voxel.y() < limit)
        {
            return Collision::Full;
        }
        if let Some(found) = self.cubes.get(&voxel) {
            return found.clone();
        }
        if self.floor.is_some_and(|floor| voxel.y() <= floor) {
            Collision::Full
        } else {
            Collision::Empty
        }
    }

    fn brick_hint(&mut self, brick: BrickCoordinate) -> BrickHint {
        // High above everything is known to be empty without asking about its voxels.
        let low = brick.y() * 32;
        if self.cubes.is_empty() && self.floor.is_some_and(|floor| low > floor) {
            BrickHint::Empty
        } else if self.cubes.is_empty()
            && self.floor.is_some_and(|floor| low + 31 <= floor)
            && self.unavailable_beyond_x.is_none()
        {
            // Everything in it is under the floor.
            BrickHint::Solid
        } else {
            BrickHint::Unknown
        }
    }
}

fn at(x: i64, y: i64, z: i64) -> VoxelCoordinate {
    VoxelCoordinate::new(x, y, z)
}

fn position(x: f32, y: f32, z: f32) -> WorldPosition {
    WorldPosition::new(BrickCoordinate::new(0, 0, 0), [x, y, z])
}

/// The eye position as plain numbers, in voxels.
fn eye(body: &CharacterBody) -> [f64; 3] {
    let eye = body.eye_position();
    let brick = eye.brick();
    let offset = eye.offset();
    [
        brick.x() as f64 * 32.0 + f64::from(offset[0]),
        brick.y() as f64 * 32.0 + f64::from(offset[1]),
        brick.z() as f64 * 32.0 + f64::from(offset[2]),
    ]
}

fn spawn(x: f32, y: f32, z: f32) -> CharacterBody {
    CharacterBody::new(position(x, y, z), CharacterSettings::default())
}

/// Steps `count` times with `input`, keeping the terrain around the character.
fn run(body: &mut CharacterBody, scene: &mut Scene, input: MoveInput, count: usize) {
    if scene.dirty {
        edited(body, scene);
    }
    for _ in 0..count {
        body.update_terrain(scene);
        body.step(input);
    }
}

/// Tells the character that the scene was edited, as the game does when a brick changes: every
/// brick around the origin, where these scenes are, has its colliders made again.
fn edited(body: &mut CharacterBody, scene: &mut Scene) {
    scene.dirty = false;
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                body.brick_changed(scene, BrickCoordinate::new(x, y, z));
            }
        }
    }
}

const STILL: MoveInput = MoveInput {
    forward: 0,
    strafe: 0,
    vertical: 0,
    jump: false,
    sprint: false,
    yaw: 0.0,
};

const FORWARD: MoveInput = MoveInput {
    forward: 1,
    ..STILL
};

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
        #![proptest_config(ProptestConfig::with_cases(48))]

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
    }
}
