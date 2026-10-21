//! A character in a level meeting a ship: the ship's blocks are colliders that stand where the ship is, turned as it is, and move
//! when it does.

mod common;

use common::{FORWARD, STILL, Scene, eye, position, spawn};
use physics::{BrickHint, CharacterBody, Collision, CollisionSource, ForeignGrid, MoveInput};
use voxel_space::{BrickCoordinate, GridId, GridPose, Orientation, VoxelCoordinate, WorldPosition};

/// The ship of these tests: a deck two voxels thick, twenty by twenty, with its lowest corner at the ship's origin and its top at 2.
/// What the source says outside the bricks that hold it is air, and it says so by hint, the way a ship's blocks do.
struct Deck {
    scene: Scene,
    /// The one brick the deck is in: the others are air, by hint.
    brick: BrickCoordinate,
    /// Asked about bricks other than the one the deck is in.
    hints: usize,
}

impl Deck {
    fn new() -> Self {
        Self {
            scene: Scene::of(|voxel| {
                voxel.y() <= 1 && (0..20).contains(&voxel.x()) && (0..20).contains(&voxel.z())
            }),
            brick: BrickCoordinate::new(0, 0, 0),
            hints: 0,
        }
    }

    /// A wall along x, twenty long, ten high and one thick, standing from the ship's origin.
    fn wall() -> Self {
        Self {
            scene: Scene::of(|voxel| {
                (0..20).contains(&voxel.x()) && (0..10).contains(&voxel.y()) && voxel.z() == 0
            }),
            brick: BrickCoordinate::new(0, 0, 0),
            hints: 0,
        }
    }

    /// A solid box of `size` voxels standing from the ship's origin.
    fn block(size: [i64; 3]) -> Self {
        Self {
            scene: Scene::of(move |voxel| {
                (0..size[0]).contains(&voxel.x())
                    && (0..size[1]).contains(&voxel.y())
                    && (0..size[2]).contains(&voxel.z())
            }),
            brick: BrickCoordinate::new(0, 0, 0),
            hints: 0,
        }
    }

    /// A deck two thick, twenty by twenty, from voxel (40, 0, 40): in the brick (1, 0, 1), which is not the first the ship has, so that the
    /// place of its blocks depends on where that brick is.
    fn far() -> Self {
        Self {
            scene: Scene::of(|voxel| {
                voxel.y() <= 1 && (40..60).contains(&voxel.x()) && (40..60).contains(&voxel.z())
            }),
            brick: BrickCoordinate::new(1, 0, 1),
            hints: 0,
        }
    }
}

impl CollisionSource for Deck {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        self.scene.collision_at(voxel)
    }

    fn brick_hint(&mut self, brick: BrickCoordinate) -> BrickHint {
        if brick == self.brick {
            BrickHint::Unknown
        } else {
            self.hints += 1;
            BrickHint::Empty
        }
    }
}

fn at(x: f32, y: f32, z: f32) -> WorldPosition {
    position(x, y, z)
}

fn ship_at(x: f32, y: f32, z: f32, yaw: f32) -> GridPose {
    GridPose::new(at(x, y, z), Orientation::from_yaw_pitch(yaw, 0.0))
}

/// Steps with the terrain of an empty level (air) and the ship as it is now, in the order a game does each tick.
fn run(
    body: &mut CharacterBody,
    level: &mut Scene,
    ship: &mut Deck,
    pose: GridPose,
    input: MoveInput,
    count: usize,
) {
    run_pulled(body, level, ship, pose, [0.0, -32.0, 0.0], input, count);
}

/// The same with the character pulled by `gravity`.
fn run_pulled(
    body: &mut CharacterBody,
    level: &mut Scene,
    ship: &mut Deck,
    pose: GridPose,
    gravity: [f32; 3],
    input: MoveInput,
    count: usize,
) {
    for _ in 0..count {
        body.update_terrain(level);
        body.update_foreign(&mut [ForeignGrid {
            id: GridId::new(GridId::FIRST_ALLOCATED),
            pose,
            source: ship,
        }]);
        body.keep_clear(level);
        body.step_with_gravity(input, gravity);
    }
}

/// Where the bottom of the character's box is: the eyes, less their height above the feet.
fn feet(body: &CharacterBody) -> f64 {
    eye(body)[1] - 1.62
}

fn eye_in(pose: &GridPose, body: &CharacterBody) -> [f32; 3] {
    let local = pose.world_to_local(body.eye_position()).unwrap();
    let brick = local.brick();
    let offset = local.offset();
    [
        brick.x() as f32 * 32.0 + offset[0],
        brick.y() as f32 * 32.0 + offset[1],
        brick.z() as f32 * 32.0 + offset[2],
    ]
}

#[test]
fn a_character_falls_onto_a_ship_that_is_turned_and_stands_on_its_deck() {
    let pose = ship_at(5.0, 40.0, 7.0, 0.6);
    let mut level = Scene::default();
    let mut ship = Deck::new();
    // Above the middle of the deck, which is (10, 2, 10) in the ship.
    let middle = pose.local_to_world(at(10.0, 2.0, 10.0)).unwrap();
    let above = middle.translated([0.0, 8.0, 0.0]);
    let mut body = CharacterBody::new(above, physics::CharacterSettings::default());
    run(&mut body, &mut level, &mut ship, pose, STILL, 20);
    assert!(!body.is_grounded() && body.velocity()[1] < -3.0);
    run(&mut body, &mut level, &mut ship, pose, STILL, 120);
    assert!(body.is_grounded());
    // Standing on the top of the deck: its top is 42 in the world, the eyes 1.62 above it, and it has not slid.
    let eyes = eye(&body);
    let wanted = middle.translated([0.0, 1.62, 0.0]);
    let wanted_brick = wanted.brick();
    let wanted_at = [
        wanted_brick.x() as f64 * 32.0 + f64::from(wanted.offset()[0]),
        wanted_brick.y() as f64 * 32.0 + f64::from(wanted.offset()[1]),
        wanted_brick.z() as f64 * 32.0 + f64::from(wanted.offset()[2]),
    ];
    assert!(
        (eyes[1] - wanted_at[1]).abs() < 0.03,
        "{eyes:?} {wanted_at:?}"
    );
    assert!((eyes[0] - wanted_at[0]).abs() < 1e-3 && (eyes[2] - wanted_at[2]).abs() < 1e-3);
    // And it stays, without a tremor.
    let resting = eye(&body);
    for _ in 0..200 {
        run(&mut body, &mut level, &mut ship, pose, STILL, 1);
        assert_eq!(eye(&body), resting);
    }
}

#[test]
fn a_wall_of_a_ship_stops_a_character_who_walks_into_it_and_it_slides_along_it() {
    // A wall twenty long and ten high, rotated 0.6 about the vertical axis, on a floor of the level whose top is at 40.
    let pose = ship_at(40.0, 40.0, 40.0, 0.6);
    let mut level = Scene::with_floor(39);
    let mut ship = Deck::wall();
    // In front of the wall (its thin face is at local z = 0 to 1; "in front" is local z = 3), walking at it across the wall (local -z).
    let start = pose.local_to_world(at(10.0, 0.0, 4.0)).unwrap();
    let mut body = CharacterBody::new(
        start.translated([0.0, 1.62 + 0.05, 0.0]),
        physics::CharacterSettings::default(),
    );
    run(&mut body, &mut level, &mut ship, pose, STILL, 60);
    assert!(body.is_grounded());
    // Local -z is the direction the ship faces at yaw 0.6: the yaw a character needs to walk along it is the same, 0.6.
    let into_the_wall = MoveInput {
        yaw: 0.6,
        ..FORWARD
    };
    let mut deepest = f32::INFINITY;
    for _ in 0..240 {
        run(&mut body, &mut level, &mut ship, pose, into_the_wall, 1);
        deepest = deepest.min(eye_in(&pose, &body)[2]);
    }
    // It never got into the wall, whose face is at local z = 1. Its box is square and stands along the axes of the level, not of the
    // ship, so a corner of it is what touches the face: it reaches 0.3 * (cos + sin of the turn) across it, and the controller leaves
    // a gap of 0.01.
    let reach = 0.3 * (libm::cosf(0.6) + libm::sinf(0.6));
    assert!(
        (deepest - (1.0 + reach + 0.01)).abs() < 0.01,
        "walking into it, it was at {deepest}"
    );
    // Walking at it slantwise, it slides along: it goes along the wall (local x) and still never gets in.
    let before = eye_in(&pose, &body);
    let slantwise = MoveInput {
        yaw: 0.6 + 0.8,
        ..FORWARD
    };
    for step in 0..90 {
        run(&mut body, &mut level, &mut ship, pose, slantwise, 1);
        if step % 5 == 0 {
            println!(
                "step {step}: {:?} velocity {:?}",
                eye_in(&pose, &body),
                body.velocity()
            );
        }
        assert!(eye_in(&pose, &body)[2] > 1.0 + 0.3 * (libm::cosf(0.6) + libm::sinf(0.6)));
    }
    // At 0.8 of a turn from the wall's face it goes along the wall at sin(0.8) of 4.3 a second, 3.08, for a second and a half: 4.6 less
    // the first step or two. (The controller alone loses the whole of the slide where the wall meets the floor, a few steps in, for a
    // wall that is turned this way and not for one that is along the axes: the character stuck to the wall.)
    let after = eye_in(&pose, &body);
    assert!(before[0] - after[0] > 4.4, "{before:?} {after:?}");
}

#[test]
fn a_wall_of_any_turn_is_slid_along_for_as_far_as_the_walker_goes_not_only_one_along_the_axes() {
    // Several turns, both ways along the wall, and a lean toward it that is slight and one that is steep.
    for yaw in [0.0f32, 0.3, 0.6, 1.0, 2.2, -0.9] {
        for lean in [0.3f32, 0.8, -0.3, -0.8] {
            let pose = ship_at(40.0, 40.0, 40.0, yaw);
            let mut level = Scene::with_floor(39);
            let mut ship = Deck::wall();
            let start = pose.local_to_world(at(10.0, 0.0, 4.0)).unwrap();
            let mut body = CharacterBody::new(
                start.translated([0.0, 1.62 + 0.05, 0.0]),
                physics::CharacterSettings::default(),
            );
            run(&mut body, &mut level, &mut ship, pose, STILL, 60);
            run(
                &mut body,
                &mut level,
                &mut ship,
                pose,
                MoveInput { yaw, ..FORWARD },
                120,
            );
            let before = eye_in(&pose, &body)[0];
            run(
                &mut body,
                &mut level,
                &mut ship,
                pose,
                MoveInput {
                    yaw: yaw + lean,
                    ..FORWARD
                },
                90,
            );
            // Positive lean turns toward local -x. The speed along the wall is 4.3 sin(lean); allow the first steps to be spent starting.
            let wanted = -4.3 * libm::sinf(lean) * 1.5;
            let went = eye_in(&pose, &body)[0] - before;
            assert!(
                (went - wanted).abs() < 0.35,
                "yaw {yaw}, lean {lean}: went {went}, should go {wanted}"
            );
        }
    }
}

#[test]
fn a_ship_that_moves_takes_its_colliders_with_it() {
    let mut level = Scene::with_floor(39);
    let mut ship = Deck::wall();
    let first = ship_at(40.0, 40.0, 40.0, 0.0);
    let mut body = CharacterBody::new(at(30.0, 41.7, 40.5), physics::CharacterSettings::default());
    run(&mut body, &mut level, &mut ship, first, STILL, 60);
    // The wall stands along x from 40 to 60 at z 40 to 41. A character at x = 30 walking along +x (yaw -pi/2: positive yaw turns toward
    // -x) is beside the wall's end and walks past it: nothing is there.
    let east = MoveInput {
        yaw: -std::f32::consts::FRAC_PI_2,
        ..FORWARD
    };
    run(&mut body, &mut level, &mut ship, first, east, 60);
    let walked = eye(&body)[0];
    assert!(walked > 33.0, "{walked}");
    // The ship moves, to where its end is one voxel ahead of the character, in a tick; the character walks into it, and its half width
    // (0.3) and the gap the controller leaves (0.01) are what is left of the voxel.
    let second = ship_at(walked as f32 + 1.0, 40.0, 40.0, 0.0);
    run(&mut body, &mut level, &mut ship, second, east, 120);
    let stopped = eye(&body)[0];
    assert!(
        (stopped - (walked + 1.0 - 0.31)).abs() < 0.01,
        "{stopped} from {walked}"
    );
    // The ship goes away: the character walks on through where it was.
    let third = ship_at(200.0, 40.0, 40.0, 0.0);
    run(&mut body, &mut level, &mut ship, third, east, 120);
    assert!(eye(&body)[0] > stopped + 3.0);
}

#[test]
fn colliders_of_a_ship_are_made_again_where_the_ship_is_edited() {
    let pose = ship_at(5.0, 40.0, 7.0, 0.0);
    let mut level = Scene::default();
    let mut ship = Deck::new();
    let mut body = CharacterBody::new(at(15.5, 44.0, 17.5), physics::CharacterSettings::default());
    run(&mut body, &mut level, &mut ship, pose, STILL, 120);
    assert!(body.is_grounded());
    let resting = eye(&body)[1];
    assert!((resting - (42.0 + 1.62)).abs() < 0.03, "{resting}");
    // A hole is made in the deck under it: the voxels (10, 0..2, 10) of the ship are removed (the character is at ship x 10.5, z 10.5).
    let mut hole = Deck {
        scene: Scene::of(|voxel| {
            voxel.y() <= 1
                && (0..20).contains(&voxel.x())
                && (0..20).contains(&voxel.z())
                && !(9..=11).contains(&voxel.x())
        }),
        brick: BrickCoordinate::new(0, 0, 0),
        hints: 0,
    };
    // Not told yet: the character stands on what it was told.
    for _ in 0..10 {
        body.update_terrain(&mut level);
        body.update_foreign(&mut [ForeignGrid {
            id: GridId::new(GridId::FIRST_ALLOCATED),
            pose,
            source: &mut hole,
        }]);
        body.step(STILL);
        assert_eq!(eye(&body)[1], resting);
    }
    // Told: it falls.
    body.foreign_brick_changed(
        &mut ForeignGrid {
            id: GridId::new(GridId::FIRST_ALLOCATED),
            pose,
            source: &mut hole,
        },
        BrickCoordinate::new(0, 0, 0),
    );
    for _ in 0..60 {
        body.update_terrain(&mut level);
        body.update_foreign(&mut [ForeignGrid {
            id: GridId::new(GridId::FIRST_ALLOCATED),
            pose,
            source: &mut hole,
        }]);
        body.step(STILL);
    }
    assert!(eye(&body)[1] < resting - 2.0, "{}", eye(&body)[1]);
}

#[test]
fn a_ship_that_is_no_longer_given_leaves_nothing_behind_and_one_far_away_costs_next_to_nothing() {
    let pose = ship_at(5.0, 40.0, 7.0, 0.0);
    let mut level = Scene::default();
    let mut ship = Deck::new();
    let mut body = CharacterBody::new(at(15.5, 44.0, 17.5), physics::CharacterSettings::default());
    run(&mut body, &mut level, &mut ship, pose, STILL, 120);
    assert!(body.is_grounded());
    let resting = eye(&body)[1];
    // Not given any more: the deck is gone, and the character falls through where it was.
    for _ in 0..60 {
        body.update_terrain(&mut level);
        body.update_foreign(&mut []);
        body.step(STILL);
    }
    assert!(eye(&body)[1] < resting - 2.0);

    // A ship that is a long way off is asked about by brick hint only: the bricks around the character, in its coordinates, are empty.
    let mut far = Deck::new();
    let mut body = spawn(15.5, 44.0, 17.5);
    let far_pose = ship_at(5000.0, 40.0, 7.0, 0.0);
    for _ in 0..10 {
        body.update_terrain(&mut level);
        body.update_foreign(&mut [ForeignGrid {
            id: GridId::new(GridId::FIRST_ALLOCATED),
            pose: far_pose,
            source: &mut far,
        }]);
        body.step(STILL);
    }
    assert_eq!(far.scene.asked, 0, "no block of it was looked at");
    assert!(far.hints > 0);
}

/// A scripted run: a character walks and jumps about a ship that turns and moves under it. The hash of its state must be the same
/// every time and on every machine.
fn scripted_run() -> u64 {
    let mut level = Scene::with_floor(39);
    let mut ship = Deck::new();
    let mut body = CharacterBody::new(at(30.3, 60.0, 31.7), physics::CharacterSettings::default());
    let mut hash = 0u64;
    for (stage, (input, count)) in [
        (STILL, 100),
        (
            MoveInput {
                yaw: 0.3,
                ..FORWARD
            },
            80,
        ),
        (
            MoveInput {
                yaw: 2.0,
                jump: true,
                ..FORWARD
            },
            60,
        ),
        (
            MoveInput {
                yaw: -1.0,
                sprint: true,
                ..FORWARD
            },
            90,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        for step in 0..count {
            // The ship drifts and turns a little each step.
            let moved = (stage * 100 + step) as f32;
            let pose = ship_at(25.0 + moved * 0.01, 40.0, 25.0, 0.4 + moved * 0.002);
            run(&mut body, &mut level, &mut ship, pose, input, 1);
        }
        hash = hash.rotate_left(7) ^ body.state_hash();
    }
    hash
}

#[test]
fn a_run_about_a_ship_that_moves_ends_in_the_same_state_every_time_and_on_every_machine() {
    let first = scripted_run();
    assert_eq!(first, scripted_run());
    assert_eq!(first, PINNED_RUN);
}

const PINNED_RUN: u64 = 8_082_165_827_112_142_602;

#[test]
fn the_blocks_of_a_brick_that_is_not_the_first_of_a_turned_ship_are_where_the_turned_ship_puts_them()
 {
    let pose = ship_at(5.0, 40.0, 7.0, 0.6);
    let mut level = Scene::default();
    let mut ship = Deck::far();
    // Above the middle of the deck, (50, 2, 50) in the ship: a long way from the ship's origin, so a turn about the wrong point, or none, puts
    // the deck somewhere else, and the character falls past it.
    let middle = pose.local_to_world(at(50.0, 2.0, 50.0)).unwrap();
    let mut body = CharacterBody::new(
        middle.translated([0.0, 6.0, 0.0]),
        physics::CharacterSettings::default(),
    );
    run(&mut body, &mut level, &mut ship, pose, STILL, 150);
    assert!(body.is_grounded());
    let wanted = middle.translated([0.0, 1.62, 0.0]);
    let (brick, offset) = (wanted.brick(), wanted.offset());
    let wanted_height = brick.y() as f64 * 32.0 + f64::from(offset[1]);
    assert!(
        (eye(&body)[1] - wanted_height).abs() < 0.03,
        "{:?} against {wanted_height}",
        eye(&body)
    );
}

// What a ship that moves into a character does to it. The controller slides a character out of an overlap along one collider's surface
// without regard to what is behind it, so a ship that comes down on a character presses it into the floor, and the colliders of terrain
// leave out boxes buried in solid terrain, so a character pressed deep enough went on down through the world.

/// A character standing on the floor of `level` (its top is at 40) with the eyes over `(x, z)`, the ship at `pose`.
fn standing_on_the_floor(
    level: &mut Scene,
    ship: &mut Deck,
    pose: GridPose,
    x: f32,
    z: f32,
) -> CharacterBody {
    let mut body = CharacterBody::new(at(x, 41.7, z), physics::CharacterSettings::default());
    run(&mut body, level, ship, pose, STILL, 60);
    assert!(body.is_grounded());
    assert!((feet(&body) - 40.0).abs() < 0.03, "{}", feet(&body));
    body
}

/// A box of 8 by 2 by 8 that falls onto a character who stands on the floor at the middle of its footprint, from twelve above the floor, 1.4
/// a step (as a ship does that has fallen so far), and then lies on the floor for sixty steps. Returns the character, how low its feet got,
/// and a hash of its state after every step.
fn a_ship_falls_onto_a_standing_character() -> (CharacterBody, f64, u64) {
    let mut level = Scene::with_floor(39);
    let mut ship = Deck::block([8, 2, 8]);
    let mut body = standing_on_the_floor(
        &mut level,
        &mut ship,
        ship_at(16.5, 52.0, 16.5, 0.0),
        20.5,
        20.5,
    );
    let (mut lowest, mut hash) = (f64::INFINITY, 0u64);
    for step in 1..=72 {
        let height = (52.0 - 1.4 * step as f32).max(40.0);
        let pose = ship_at(16.5, height, 16.5, 0.0);
        run(&mut body, &mut level, &mut ship, pose, STILL, 1);
        lowest = lowest.min(feet(&body));
        hash = hash.rotate_left(5) ^ body.state_hash();
    }
    (body, lowest, hash)
}

#[test]
fn a_ship_that_falls_onto_a_standing_character_lifts_it_onto_itself_and_does_not_press_it_into_the_floor()
 {
    let (body, lowest, _) = a_ship_falls_onto_a_standing_character();
    assert!(
        lowest > 39.95,
        "pressed into the floor: the feet got to {lowest}"
    );
    // The shortest way out from the middle of the footprint of a box two thick is up, and onto it.
    assert!(body.is_grounded());
    assert!((feet(&body) - 42.0).abs() < 0.05, "{}", feet(&body));
    assert!((eye(&body)[0] - 20.5).abs() < 1e-3 && (eye(&body)[2] - 20.5).abs() < 1e-3);
}

#[test]
fn a_ship_that_falls_onto_a_character_in_a_turned_frame_lifts_it_along_its_own_up() {
    // The frame is turned so that up is +x: the floor is the wall x <= 39, with its face at 40, gravity pulls toward -x, and a box of two
    // along x (and eight across) falls along -x onto a character standing at the middle of its footprint.
    let half = std::f32::consts::FRAC_1_SQRT_2;
    let mut level = Scene::of(|voxel| voxel.x() <= 39);
    let mut ship = Deck::block([2, 8, 8]);
    let pulled = [-32.0, 0.0, 0.0];
    let mut body = CharacterBody::new(at(41.7, 20.5, 20.5), physics::CharacterSettings::default());
    body.reframe(at(41.7, 20.5, 20.5), [0.0; 3], [0.0, 0.0, -half, half]);
    let ship_at_height = |height: f32| ship_at(height, 16.5, 16.5, 0.0);
    let mut lowest = f64::INFINITY;
    run_pulled(
        &mut body,
        &mut level,
        &mut ship,
        ship_at_height(52.0),
        pulled,
        STILL,
        60,
    );
    assert!(body.is_grounded());
    // The feet are 1.62 below the eyes along up, which is +x.
    assert!(
        (eye(&body)[0] - 1.62 - 40.0).abs() < 0.03,
        "{:?}",
        eye(&body)
    );
    for step in 1..=12 {
        let height = (52.0 - 1.4 * step as f32).max(40.0);
        run_pulled(
            &mut body,
            &mut level,
            &mut ship,
            ship_at_height(height),
            pulled,
            STILL,
            1,
        );
        lowest = lowest.min(eye(&body)[0] - 1.62);
    }
    assert!(
        lowest > 39.95,
        "pressed into the wall: the feet got to {lowest}"
    );
    run_pulled(
        &mut body,
        &mut level,
        &mut ship,
        ship_at_height(40.0),
        pulled,
        STILL,
        60,
    );
    // Along its up and onto the box, and nowhere else.
    let [along, across_y, across_z] = eye(&body);
    assert!(body.is_grounded());
    assert!((along - 1.62 - 42.0).abs() < 0.05, "{along}");
    assert!((across_y - 20.5).abs() < 1e-3 && (across_z - 20.5).abs() < 1e-3);
}

#[test]
fn a_ship_that_overlaps_a_character_by_less_than_the_margin_does_not_move_it_and_one_that_overlaps_it_by_more_does()
 {
    // The character stands at x = 20.5, so its box is from 20.2 to 20.8 and the box tested is 0.02 smaller on every side: 20.22 to 20.78.
    // The west face of a box is put at 20.79, which is 0.01 into the character and not into the box tested, and then at 20.77, which is.
    let mut level = Scene::with_floor(39);
    let mut ship = Deck::block([8, 8, 8]);
    let far = ship_at(100.0, 40.0, 28.5, 0.0);
    let mut body = standing_on_the_floor(&mut level, &mut ship, far, 20.5, 32.5);
    let mut keep_clear_with_the_face_at = |face: f32| {
        body.update_foreign(&mut [ForeignGrid {
            id: GridId::new(GridId::FIRST_ALLOCATED),
            pose: ship_at(face, 40.0, 28.5, 0.0),
            source: &mut ship,
        }]);
        let before = body.state_hash();
        body.keep_clear(&mut level);
        (before, body.state_hash())
    };
    let (before, after) = keep_clear_with_the_face_at(20.79);
    assert_eq!(before, after, "moved by a ship that is 0.01 into it");
    let (before, after) = keep_clear_with_the_face_at(20.77);
    assert_ne!(before, after, "not moved by a ship that is 0.03 into it");
}

#[test]
fn a_ship_that_sails_into_a_character_in_a_turned_frame_comes_as_near_as_the_width_of_the_character_not_its_length()
 {
    // Up is +x (the floor is the wall x <= 39), so the character is 1.8 along x and 0.6 across, along y and z. A box of eight sails along
    // -y toward it, 0.2 a step: the nearest it gets is the half width of the character, and not the half of its length (0.9), which is
    // what a box that was not turned with the character would keep it away by.
    let half = std::f32::consts::FRAC_1_SQRT_2;
    let mut level = Scene::of(|voxel| voxel.x() <= 39);
    let mut ship = Deck::block([8, 8, 8]);
    let pulled = [-32.0, 0.0, 0.0];
    let mut body = CharacterBody::new(at(41.7, 20.5, 20.5), physics::CharacterSettings::default());
    body.reframe(at(41.7, 20.5, 20.5), [0.0; 3], [0.0, 0.0, -half, half]);
    // The box stands on the wall (x from 40 to 48) and covers the character in z (16.5 to 24.5); its face toward the character is at
    // y = 33 - 0.2 a step.
    let face_after = |step: u32| 33.0 - 0.2 * step as f32;
    let ship_at_step = |step: u32| ship_at(40.0, face_after(step), 16.5, 0.0);
    run_pulled(
        &mut body,
        &mut level,
        &mut ship,
        ship_at_step(0),
        pulled,
        STILL,
        60,
    );
    assert!(body.is_grounded());
    for step in 1..=100 {
        run_pulled(
            &mut body,
            &mut level,
            &mut ship,
            ship_at_step(step),
            pulled,
            STILL,
            1,
        );
        let (face, across) = (f64::from(face_after(step)), eye(&body)[1]);
        assert!(across <= face - 0.27, "step {step}: inside the box");
        // Once it has reached the character it pushes it along, and what is left between them is the half width and at most an eighth.
        if face < 20.5 {
            assert!(
                face - across < 0.41,
                "step {step}: the character is {} from the face of the box",
                face - across
            );
        }
    }
    assert!(
        eye(&body)[1] < 20.5 - 2.0,
        "it was pushed along: {:?}",
        eye(&body)
    );
}

/// A box of `thickness` along x (eight along y and z) that starts with its west face 12.5 east of a character who stands on the floor and
/// goes west `speed` voxels a step, until it is 12 past the place the character stood. After each step the character is out of the box,
/// and above the floor. Returns whether it was ahead of the box (to the west of it) after every step, and a hash of its state after every step.
fn sail_into_a_standing_character(thickness: i64, speed: f32) -> (bool, u64) {
    let mut level = Scene::with_floor(39);
    let mut ship = Deck::block([thickness, 8, 8]);
    let face_after = move |step: u32| 33.0 - speed * step as f32;
    let at_step = move |step: u32| ship_at(face_after(step), 40.0, 28.5, 0.0);
    let mut body = standing_on_the_floor(&mut level, &mut ship, at_step(0), 20.5, 32.5);
    let steps = ((12.5 + thickness as f32 + 12.0) / speed).ceil() as u32;
    let (mut ahead, mut hash) = (true, 0u64);
    for step in 1..=steps {
        run(&mut body, &mut level, &mut ship, at_step(step), STILL, 1);
        hash = hash.rotate_left(5) ^ body.state_hash();
        let (face, x) = (f64::from(face_after(step)), eye(&body)[0]);
        let wall = thickness as f64;
        // The box is x from `face` to `face + thickness`, and the box of the character is 0.6 wide, less the margin a quarter either way.
        assert!(
            x <= face - 0.27 || x >= face + wall + 0.27,
            "thickness {thickness}, speed {speed}, step {step}: the character is at {x}, the box from {face} to {}",
            face + wall
        );
        assert!(
            feet(&body) > 39.95,
            "thickness {thickness}, speed {speed}, step {step}: pressed into the floor: the feet are at {}",
            feet(&body)
        );
        ahead &= x <= face - 0.27;
    }
    (ahead, hash)
}

#[test]
fn a_ship_that_moves_into_a_standing_character_pushes_it_ahead_of_it_at_any_speed_up_to_a_jump_of_more_than_half_its_thickness()
 {
    // 0.2 a step is 4 a second; 1.2 is 24; 3 is 60, a step of more than a third of the box and less than a half of it.
    for speed in [0.2f32, 1.2, 3.0] {
        assert!(
            sail_into_a_standing_character(8, speed).0,
            "speed {speed}: not ahead of the box all the way"
        );
    }
    // A step of 6 takes the box past the middle of the character (the nearest way out is then the far side): it is never inside, and it
    // is still above the floor, and that is what is asked of it. (Which side it ends on is not.)
    sail_into_a_standing_character(8, 6.0);
}

#[test]
fn a_thin_wall_that_jumps_over_a_character_leaves_it_out_of_the_wall_and_not_in_the_floor() {
    for speed in [0.2f32, 1.0, 3.0, 6.0] {
        sail_into_a_standing_character(1, speed);
    }
}

/// Ships that fall on a character, sail into it and jump over it, one after another: a hash of what became of it.
fn pushed_run() -> u64 {
    let mut hash = a_ship_falls_onto_a_standing_character().2;
    for (thickness, speed) in [
        (8, 0.2f32),
        (8, 1.2),
        (8, 3.0),
        (8, 6.0),
        (1, 0.2),
        (1, 1.0),
        (1, 3.0),
        (1, 6.0),
    ] {
        hash = hash.rotate_left(7) ^ sail_into_a_standing_character(thickness, speed).1;
    }
    hash
}

#[test]
fn ships_that_fall_on_a_character_and_sail_into_it_leave_it_in_the_same_state_every_time_and_on_every_machine()
 {
    let first = pushed_run();
    assert_eq!(first, pushed_run());
    assert_eq!(first, PINNED_PUSHED_RUN);
}

const PINNED_PUSHED_RUN: u64 = 1_210_939_939_087_579_234;

#[test]
fn a_block_built_where_a_character_stands_moves_it_out_the_shortest_way_and_it_stays_on_the_floor()
{
    let mut level = Scene::with_floor(39);
    let mut body = spawn(20.5, 41.7, 20.5);
    common::run(&mut body, &mut level, STILL, 60);
    assert!(body.is_grounded());
    // A cube is built in the voxel the character stands in. The way out that is shortest is sideways, by a bit more than the half
    // of the cube and the half of the character (0.78, in steps of an eighth: 0.875), and not up, which is a bit more than a voxel.
    level.cube(20, 40, 20);
    common::run(&mut body, &mut level, STILL, 1);
    let [x, y, z] = eye(&body);
    assert!(
        (x - 21.375).abs() < 0.01 && (z - 20.5).abs() < 0.01,
        "{x} {z}"
    );
    common::run(&mut body, &mut level, STILL, 60);
    assert!(body.is_grounded());
    assert!((eye(&body)[1] - y).abs() < 0.03 && (eye(&body)[1] - 41.62).abs() < 0.03);
}

#[test]
fn a_block_built_over_a_characters_head_moves_it_out_from_under_it_and_does_not_press_it_into_the_floor()
 {
    let mut level = Scene::with_floor(39);
    let mut body = spawn(20.0, 41.7, 20.0);
    common::run(&mut body, &mut level, STILL, 60);
    assert!(body.is_grounded());
    // A block of two by two is built a voxel above the floor, where the head is (the character stands at the corner the four voxels share).
    // The controller alone slides the character out of it along the surface it finds, which is down, into the floor, and the floor pushes
    // it back up, and it shakes between 39.55 and 40 at every step. The nearest way out is sideways (a voxel and a quarter), not up, which
    // is two, with the block over its head.
    for (x, z) in [(19, 19), (20, 19), (19, 20), (20, 20)] {
        level.cube(x, 41, z);
    }
    let mut lowest = f64::INFINITY;
    for step in 0..30 {
        common::run(&mut body, &mut level, STILL, 1);
        lowest = lowest.min(feet(&body));
        if step == 0 {
            let [x, _, z] = eye(&body);
            assert!(
                (x - 21.375).abs() < 0.01 && (z - 20.0).abs() < 0.01,
                "{x} {z}"
            );
        }
    }
    assert!(
        lowest > 39.95,
        "pressed into the floor: the feet got to {lowest}"
    );
    assert!(body.is_grounded());
    assert!((feet(&body) - 40.0).abs() < 0.03, "{}", feet(&body));
}

#[test]
fn keeping_clear_leaves_alone_a_character_that_only_touches_things() {
    let unchanged_by_keeping_clear = |body: &mut CharacterBody, level: &mut Scene| {
        let before = body.state_hash();
        body.keep_clear(level);
        assert_eq!(body.state_hash(), before);
    };
    // On the floor of the level.
    let mut level = Scene::with_floor(39);
    let mut ship = Deck::new();
    let far = ship_at(500.0, 40.0, 500.0, 0.0);
    let mut body = standing_on_the_floor(&mut level, &mut ship, far, 20.5, 20.5);
    unchanged_by_keeping_clear(&mut body, &mut level);

    // On the deck of a turned ship, and against the wall of one.
    let pose = ship_at(40.0, 40.0, 40.0, 0.6);
    let mut level = Scene::with_floor(39);
    let mut ship = Deck::new();
    let middle = pose.local_to_world(at(10.0, 2.0, 10.0)).unwrap();
    let mut body = CharacterBody::new(
        middle.translated([0.0, 3.0, 0.0]),
        physics::CharacterSettings::default(),
    );
    run(&mut body, &mut level, &mut ship, pose, STILL, 120);
    assert!(body.is_grounded());
    unchanged_by_keeping_clear(&mut body, &mut level);

    let mut ship = Deck::wall();
    let start = pose.local_to_world(at(10.0, 0.0, 4.0)).unwrap();
    let mut body = CharacterBody::new(
        start.translated([0.0, 1.62 + 0.05, 0.0]),
        physics::CharacterSettings::default(),
    );
    run(&mut body, &mut level, &mut ship, pose, STILL, 60);
    let into_the_wall = MoveInput {
        yaw: 0.6,
        ..FORWARD
    };
    run(&mut body, &mut level, &mut ship, pose, into_the_wall, 120);
    unchanged_by_keeping_clear(&mut body, &mut level);
}

#[test]
fn a_character_inside_a_ship_with_no_room_near_stays_where_it_is_and_the_search_is_small() {
    let mut level = Scene::default();
    let mut ship = Deck::block([40, 40, 40]);
    let pose = ship_at(0.0, 0.0, 0.0, 0.0);
    let mut body = CharacterBody::new(at(20.0, 21.0, 20.0), physics::CharacterSettings::default());
    body.update_terrain(&mut level);
    body.update_foreign(&mut [ForeignGrid {
        id: GridId::new(GridId::FIRST_ALLOCATED),
        pose,
        source: &mut ship,
    }]);
    let before = body.state_hash();
    level.asked = 0;
    body.keep_clear(&mut level);
    assert_eq!(
        body.state_hash(),
        before,
        "it was moved though there is no room"
    );
    // Eight voxels in each of six ways by eighths, and a dozen voxels looked at for each place: not the sixty-four of a character that is put.
    assert!(level.asked < 10_000, "{} voxels looked at", level.asked);
}
