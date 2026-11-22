//! Two cars with two pairs of joint ends between them, one or both riding a rail (step 4c-5-4b of `docs/decisions/joints-and-cars.md`). `joint_coupling.rs` has the free case: **the
//! two cars are coupled at one pair, the closest, and nothing is made of the other** (several balls on one pair of cars are for the integer rule of the decision file, which is not
//! built). Over the rails the pairs of ends are found by `rail_joints.rs`, which asks for the closest pair of ends of two ships and not for every pair, and the coupling is the bodies'
//! in the tick after both are taken off their rails; this is the same scene as the free one with the cars riding rails in the same ways as in `joints_on_rails.rs`.

mod common;

use common::chains::origins_of;
use common::docking::{PLAIN, Placed, make, make_going, stone};
use common::joints::{ball_end_toward, joint_block};
use common::rails::{put_on_a_rail_near_another_ship, put_on_its_rail_in};
use common::{Scene, circular_speed, pose_at, ship_id};
use voxel_space::{FaceDirection, GridId};

const DISTANCE: f64 = 70_016.0;

/// How far apart the east pivots of the first car and the west pivots of the second are when the second is made, in voxels, and how fast the second goes toward the first.
const GAP: f64 = 7.0;
const CLOSING: f64 = 2.0;

/// The length of each car along x: three blocks of stone and a joint block.
const CAR_LENGTH: f64 = 4.0;

/// How far the second car is made along z (the way the cars go) from the first: 0.4 puts the pair of row 0 nearer (0.4 apart, the other 1.4), and -0.9 the pair of the upper rows (0.1
/// apart, the other 0.9: the nearer pair is the later in order of the ends).
const OFFSET_FOR_THE_FIRST_PAIR: f64 = 0.4;
const OFFSET_FOR_THE_SECOND_PAIR: f64 = -0.9;

/// Which of the two cars ride a rail while they pull.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Riding {
    Neither,
    First,
    Both,
}

/// The first car: three stones in two rows (z = 0 and 1) and a joint end on the east face of each row. With `with_joints` false the ends are stones.
fn first_car(with_joints: bool) -> Vec<Placed> {
    let mut blocks: Vec<Placed> = (0..3)
        .flat_map(|x| (0..2).map(move |z| ([x, 0, z], stone(), PLAIN)))
        .collect();
    for z in 0..2 {
        blocks.push(if with_joints {
            (
                [3, 0, z],
                joint_block(),
                ball_end_toward(FaceDirection::PositiveX),
            )
        } else {
            ([3, 0, z], stone(), PLAIN)
        });
    }
    blocks
}

/// The second car: a joint end on the west face of row 0 and of `upper_row` (2: one more voxel from the first's second end than the first's two are from each other, so that the
/// pair of row 0 is the nearer; 1: the two pairs are alike), stones between them, and three stones behind each row.
fn second_car(with_joints: bool, upper_row: i128) -> Vec<Placed> {
    let mut blocks: Vec<Placed> = Vec::new();
    for z in [0, upper_row] {
        blocks.push(if with_joints {
            (
                [0, 0, z],
                joint_block(),
                ball_end_toward(FaceDirection::NegativeX),
            )
        } else {
            ([0, 0, z], stone(), PLAIN)
        });
    }
    blocks.extend((1..upper_row).map(|z| ([0, 0, z], stone(), PLAIN)));
    blocks.extend((1..4).flat_map(|x| (0..=upper_row).map(move |z| ([x, 0, z], stone(), PLAIN))));
    blocks
}

fn two_cars(
    riding: Riding,
    with_joints: bool,
    upper_row: i128,
    offset: f64,
) -> (Scene, impl Fn() -> grid_bodies::BodiesReport, [GridId; 2]) {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    make(
        &mut scene,
        pose_at(0.0, DISTANCE, 0.0),
        false,
        first_car(with_joints),
    );
    scene.ticks(2);
    let (a, b) = (ship_id(1), ship_id(2));
    scene.run(move |world| {
        world
            .set_grid_velocity(a, [0.0, 0.0, speed], [0.0; 3])
            .unwrap();
        if riding != Riding::Neither {
            put_on_its_rail_in(world, a);
        }
    });
    scene.ticks(3);
    let origin = origins_of(&scene, &[a])[0];
    make_going(
        &mut scene,
        pose_at(origin[0] + CAR_LENGTH + GAP, origin[1], origin[2] + offset),
        second_car(with_joints, upper_row),
        [-CLOSING, 0.0, speed],
    );
    scene.run(move |world| {
        if riding == Riding::Both {
            put_on_a_rail_near_another_ship(world, b);
        }
    });
    (scene, report, [a, b])
}

/// How far apart the ends of each pair are: the east pivot of the first car's row 0 and the west pivot of the second's row 0 (the nearer pair when `upper_row` is 2), and the first's
/// row 1 and the second's `upper_row`.
fn gaps(scene: &Scene, [a, b]: [GridId; 2], upper_row: i128) -> [f64; 2] {
    [(0.0, 0.0), (1.0, upper_row as f64)].map(|(row_of_first, row_of_second)| {
        let first = common::chains::world_of(scene, a, [CAR_LENGTH, 0.5, row_of_first + 0.5]);
        let second = common::chains::world_of(scene, b, [0.0, 0.5, row_of_second + 0.5]);
        common::length([0, 1, 2].map(|axis| first[axis] - second[axis]))
    })
}

/// What a run of the two cars gave.
struct Run {
    near_at: Option<usize>,
    /// The tick in which the first car was found off its rail (never for cars that are free).
    left_at: Option<usize>,
    coupled_at: Option<usize>,
    couplings: u64,
    couplings_made: u64,
    /// How far apart the ends of each pair were at the end.
    gaps_at_the_end: [f64; 2],
    /// Whether each car rode a rail at the end.
    on_rails_at_the_end: [bool; 2],
}

fn run(riding: Riding, upper_row: i128, offset: f64) -> Run {
    let (mut scene, report, ships) = two_cars(riding, true, upper_row, offset);
    let (mut near_at, mut left_at, mut coupled_at) = (None, None, None);
    for tick in 0..300 {
        scene.ticks(1);
        let gaps = gaps(&scene, ships, upper_row);
        if near_at.is_none() && gaps[0].min(gaps[1]) < 0.3 {
            near_at = Some(tick);
        }
        let rides = |ship: GridId| {
            scene
                .rig
                .server
                .world()
                .grids()
                .get(ship)
                .unwrap()
                .rail
                .is_some()
        };
        if left_at.is_none() && riding != Riding::Neither && !rides(ships[0]) {
            left_at = Some(tick);
        }
        if coupled_at.is_none() && report().couplings > 0 {
            coupled_at = Some(tick);
        }
    }
    Run {
        near_at,
        left_at,
        coupled_at,
        couplings: report().couplings,
        couplings_made: report().couplings_made,
        gaps_at_the_end: gaps(&scene, ships, upper_row),
        on_rails_at_the_end: ships.map(|ship| {
            scene
                .rig
                .server
                .world()
                .grids()
                .get(ship)
                .unwrap()
                .rail
                .is_some()
        }),
    }
}

#[test]
fn two_cars_with_two_pairs_of_ends_between_them_are_coupled_at_the_nearer_pair_and_only_one_whether_they_ride_rails_or_are_free()
 {
    // Measured 2026-11-22: free, the pair of row 0 is coupled in tick 55 (the ends within 0.3 voxels in 52), on a rail or two in 54 (50), with a gap of 3e-6 voxels at that pair and
    // 2.8 at the other, and one coupling made in each. (A tick's difference is the rail's: a ship on a rail is a tick ahead of a free one.)
    for (offset, nearer) in [
        (OFFSET_FOR_THE_FIRST_PAIR, 0),
        (OFFSET_FOR_THE_SECOND_PAIR, 1),
    ] {
        let free = run(Riding::Neither, 2, offset);
        assert_eq!(
            (free.couplings, free.couplings_made),
            (1, 1),
            "free: one coupling"
        );
        assert!(
            free.gaps_at_the_end[nearer] < 1.0e-3,
            "free: at the nearer pair: {:?}",
            free.gaps_at_the_end
        );
        assert!(
            free.gaps_at_the_end[1 - nearer] > 0.5,
            "free: and not at the other: {:?}",
            free.gaps_at_the_end
        );
        for riding in [Riding::First, Riding::Both] {
            let on_rails = run(riding, 2, offset);
            assert_eq!(
                (on_rails.couplings, on_rails.couplings_made),
                (1, 1),
                "{riding:?}: one coupling, not two and not none"
            );
            assert!(
                on_rails.gaps_at_the_end[nearer] < 1.0e-3,
                "{riding:?}: coupled at the nearer pair: {:?}",
                on_rails.gaps_at_the_end
            );
            assert!(
                on_rails.gaps_at_the_end[1 - nearer] > 0.5,
                "{riding:?}: and not at the other: {:?}",
                on_rails.gaps_at_the_end
            );
            let (coupled, free_coupled) = (
                on_rails.coupled_at.expect("coupled"),
                free.coupled_at.expect("free: coupled"),
            );
            assert!(
                coupled.abs_diff(free_coupled) <= 2,
                "{riding:?}: coupled in tick {coupled}, free in {free_coupled}"
            );
            // (The car on a rail left it at the touch and not before: the ends were within 0.3 voxels in tick `near`, and it was off its rail in the tick before the coupling. A coupled
            // pair is put on a rail again when it has been quiet for a while, and a group's rail is its anchor's, the first car's: the second has none of its own.)
            let (near, left) = (
                on_rails.near_at.expect("near"),
                on_rails.left_at.expect("left its rail"),
            );
            assert!(
                near <= left + 1 && left + 3 >= coupled && left <= coupled,
                "{riding:?}: near in tick {near}, left its rail in tick {left}, coupled in {coupled}"
            );
            assert!(
                !on_rails.on_rails_at_the_end[1],
                "{riding:?}: the second car has no rail of its own"
            );
        }
    }
}

#[test]
fn two_cars_with_two_pairs_of_ends_alike_are_coupled_at_one_of_them_and_only_one_whether_they_ride_rails_or_are_free()
 {
    // (A tie: which pair is decided by the order of the ends, and a test cannot say which, but it is one, and the same ones come together: the two cars do not strain at the other.)
    for riding in [Riding::Neither, Riding::First, Riding::Both] {
        let run = run(riding, 1, OFFSET_FOR_THE_FIRST_PAIR);
        assert_eq!(
            (run.couplings, run.couplings_made),
            (1, 1),
            "{riding:?}: one coupling"
        );
        let [first, second] = run.gaps_at_the_end;
        assert!(
            first.min(second) < 1.0e-3,
            "{riding:?}: one of the pairs is together: {first:e} and {second:e}"
        );
    }
}
