//! Two joint ends of one group that face each other, near, while the group rides a rail or is free (step 4c-5-4b of `docs/decisions/joints-and-cars.md`). **Two ships of one group are not
//! a pair for a pull or a coupling** (`couplings.rs`: "two ships of one group are not coupled again, a loop is for later"; `rail_joints.rs` and `rail_ports.rs`: `are_of_one_group`), so
//! such ends must be left alone: no force on the group, no second coupling, and a group on a rail stays on its rail, exactly. Without that rule the ends would pull on each other
//! (the pull is sized for two ships, and one group's would be a force on its own rail), and a coupling of a group to itself would be a loop.
//!
//! The group is two cars or three, coupled by balls as a chain's are. The first has an arm over the others (two stones up, then along) that ends in a joint end facing the last, **in
//! the next voxel to a joint end on top of the last car that faces it (the arm at height 2: their pivots are together, in line and still, which is a coupling to be made if they were
//! two ships), **or one voxel higher (height 3: 2 voxels apart, in the range of a pull and not of a coupling)**: the rule for a coupling and the rule for a pull are two lines of code. **With two cars the ends are of a pair that is coupled already, with three they are of two cars that are in one
//! group and are not coupled to each other**: the two rules are not the same line of code (`joint_pairs` leaves a pair that is coupled, `groups.same` a group).

mod common;

use common::chains::{a_chain_in_orbit, car, put_on_its_rail, set_going, world_of};
use common::docking::{PLAIN, Placed, stone};
use common::joints::{ball_end_toward, joint_block};
use common::{Scene, circular_speed, length};
use voxel_space::{FaceDirection, GridId};
use world_storage::rail::Rail;

const DISTANCE: f64 = 70_016.0;

/// The groups: two cars and three, with the arm's joint end at height 2 (touching the other's) and at 3 (2 voxels from it).
const CASES: [(usize, i128); 4] = [(2, 2), (2, 3), (3, 2), (3, 3)];

/// Where the arm's joint end is along x in the first car's own coordinates, for a group of two cars and of three: over the last car's second stone (the cars are 4, 5 and 4 voxels
/// long, so the last of two begins at x = 4 and of three at x = 9, and its stone at local x = 1 is at 5 and at 10).
fn arm_end(cars: usize) -> i128 {
    if cars == 2 { 5 } else { 10 }
}

/// The first car: three stones and a joint end on the east (the one the ball joins to the next car), and the arm: stones along z at x = 0, from 1 to `arm_z`, then along x at that
/// height from 1 to just before its end, and a joint end there that faces the way of -z (toward the last car, which is below it along z).
fn first_car(cars: usize, arm_z: i128, with_joints: bool) -> (Vec<Placed>, f64) {
    let (mut blocks, length) = car(false, 3, true);
    blocks.extend((1..=arm_z).map(|z| ([0, 0, z], stone(), PLAIN)));
    blocks.extend((1..arm_end(cars)).map(|x| ([x, 0, arm_z], stone(), PLAIN)));
    blocks.push(if with_joints {
        (
            [arm_end(cars), 0, arm_z],
            joint_block(),
            ball_end_toward(FaceDirection::NegativeZ),
        )
    } else {
        ([arm_end(cars), 0, arm_z], stone(), PLAIN)
    });
    (blocks, length)
}

/// The last car: a joint end on the west (the ball's), three stones, and a joint end on top of the stone at x = 1 that faces +z, toward the arm's.
fn last_car(with_joints: bool) -> (Vec<Placed>, f64) {
    let (mut blocks, length) = car(true, 3, false);
    blocks.push(if with_joints {
        (
            [1, 0, 1],
            joint_block(),
            ball_end_toward(FaceDirection::PositiveZ),
        )
    } else {
        ([1, 0, 1], stone(), PLAIN)
    });
    (blocks, length)
}

/// The group, free or on its rail, in orbit at `DISTANCE`, after the ticks that the bodies take to couple the cars.
fn the_group(
    cars: usize,
    arm_z: i128,
    on_a_rail: bool,
    with_joints: bool,
) -> (
    Scene,
    impl Fn() -> grid_bodies::BodiesReport,
    Vec<GridId>,
    Option<Rail>,
) {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    let mut made = vec![first_car(cars, arm_z, with_joints)];
    if cars == 3 {
        made.push(car(true, 3, true));
    }
    made.push(last_car(with_joints));
    let ships = a_chain_in_orbit(&mut scene, made, DISTANCE, speed);
    let still = vec![([0.0; 3], [0.0; 3]); cars];
    let rail = if on_a_rail {
        Some(put_on_its_rail(&mut scene, &ships, speed, still))
    } else {
        set_going(&mut scene, &ships, speed, still);
        None
    };
    scene.ticks(3);
    (scene, report, ships, rail)
}

/// How far apart the two joint ends that face each other are, in voxels: the arm's, at the end of the arm on the first car with its centre at z = `arm_z` + 0.5, and the last car's at
/// x = 1 on it with its centre at z = 1.5 (at height 2, one voxel below it, their pivots on the face between them, together).
fn the_ends_apart(scene: &Scene, ships: &[GridId], cars: usize, arm_z: i128) -> f64 {
    let arm = world_of(
        scene,
        ships[0],
        [arm_end(cars) as f64 + 0.5, 0.5, arm_z as f64 + 0.5],
    );
    let other = world_of(scene, *ships.last().expect("cars"), [1.5, 0.5, 1.5]);
    length([0, 1, 2].map(|axis| arm[axis] - other[axis]))
}

/// What a run of the group for 100 ticks gave.
struct Run {
    /// The most the ends' distance was from what it was at the start, in voxels.
    most_moved: f64,
    /// The first tick in which the group's rail was not the one it was given (for a free group: in which it had one).
    rail_changed_at: Option<usize>,
    couplings: u64,
    couplings_made: u64,
    rails_left_for_ends: u64,
    rail_kicks: u64,
}

fn run(cars: usize, arm_z: i128, on_a_rail: bool) -> Run {
    let (mut scene, report, ships, rail) = the_group(cars, arm_z, on_a_rail, true);
    let start = the_ends_apart(&scene, &ships, cars, arm_z);
    let (mut most_moved, mut rail_changed_at) = (0.0f64, None);
    for tick in 0..100 {
        scene.ticks(1);
        most_moved = most_moved.max((the_ends_apart(&scene, &ships, cars, arm_z) - start).abs());
        let now = scene
            .rig
            .server
            .world()
            .grids()
            .get(ships[0])
            .unwrap()
            .rail
            .clone();
        if rail_changed_at.is_none() && now != rail {
            rail_changed_at = Some(tick);
        }
    }
    assert!(
        (start - (arm_z - 1) as f64).abs() < 1.0e-3,
        "{cars} cars, arm at {arm_z}: the ends are {} voxels apart to begin with: {start}",
        arm_z - 1
    );
    Run {
        most_moved,
        rail_changed_at,
        couplings: report().couplings,
        couplings_made: report().couplings_made,
        rails_left_for_ends: report().rails_left_for_ends,
        rail_kicks: report().rail_kicks,
    }
}

#[test]
fn two_ends_of_a_group_that_face_each_other_are_left_alone_by_a_group_on_a_rail_which_keeps_its_rail_exactly()
 {
    for (cars, arm_z) in CASES {
        let run = run(cars, arm_z, true);
        assert_eq!(
            (run.couplings, run.couplings_made),
            (cars as u64 - 1, cars as u64 - 1),
            "{cars} cars, arm at {arm_z}: the balls between the cars, and no coupling of the group to itself"
        );
        assert_eq!(
            run.rail_changed_at, None,
            "{cars} cars, arm at {arm_z}: the group's own ends did not push its rail or take it off"
        );
        assert_eq!(
            run.rails_left_for_ends, 0,
            "{cars} cars, arm at {arm_z}: no rail was left for an end"
        );
        assert_eq!(
            run.rail_kicks, 0,
            "{cars} cars, arm at {arm_z}: and nothing hit it"
        );
        assert!(
            run.most_moved < 1.0e-3,
            "{cars} cars, arm at {arm_z}: the ends moved {} voxels: they pulled on each other",
            run.most_moved
        );
    }
}

#[test]
fn two_ends_of_a_free_group_that_face_each_other_are_not_pulled_together_or_coupled() {
    for (cars, arm_z) in CASES {
        let run = run(cars, arm_z, false);
        assert_eq!(
            (run.couplings, run.couplings_made),
            (cars as u64 - 1, cars as u64 - 1),
            "{cars} cars, arm at {arm_z}: the balls between the cars, and no coupling of the group to itself"
        );
        assert!(
            run.most_moved < 1.0e-3,
            "{cars} cars, arm at {arm_z}: the ends moved {} voxels: they pulled on each other",
            run.most_moved
        );
    }
}
