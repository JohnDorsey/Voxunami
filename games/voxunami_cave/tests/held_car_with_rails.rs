//! A held car and a ship or a group on a rail (step 4c-5-4b of `docs/decisions/joints-and-cars.md`). A held ship does not move: gravity, forces and collisions leave it where it is.
//!
//! Two things can happen between a held car and a rail: a ship on a rail comes past the held car's joint end (the two cannot close slowly enough to be coupled: the rail goes round
//! at the speed of the orbit, 761 voxels a second here, and the held car is at rest in the world), and a car of a group on a rail is held (the group cannot ride a rail that its held
//! car would not follow, so it is taken off it, and the rest of the chain swings about the car).

mod common;

use common::chains::{
    a_chain, a_chain_in_orbit, car, centre_of, origins_of, pivot_gap, put_on_its_rail,
};
use common::docking::{make, make_going, stones};
use common::rails::put_on_a_rail_near_another_ship;
use common::{Scene, circular_speed, length, pose_at, ship_id};

const DISTANCE: f64 = 70_016.0;

/// A held car with a joint end on its east face, in orbit height at rest in the world, and a car with one on its west face that comes past it on a rail, going along z at the speed of a
/// circle, from 380 voxels before it, 3 voxels east of the held car's end (the ends face each other across x, and are within the range of a pull, 6 voxels, for a tick). With
/// `with_joints` false the ends are stones. Returns what was read of the held car before and after 60 ticks, whether the passing car rode a rail in every one, and the couplings.
fn a_flyby(with_joints: bool) -> Flyby {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    let held = if with_joints {
        car(false, 3, true).0
    } else {
        stones(0, 4)
    };
    make(&mut scene, pose_at(0.0, DISTANCE, 0.0), true, held);
    scene.ticks(2);
    let passing = if with_joints {
        car(true, 3, false).0
    } else {
        stones(0, 4)
    };
    make_going(
        &mut scene,
        pose_at(7.0, DISTANCE, -380.0),
        passing,
        [0.0, 0.0, speed],
    );
    let (held_ship, passing_ship) = (ship_id(1), ship_id(2));
    scene.run(move |world| put_on_a_rail_near_another_ship(world, passing_ship));
    let read = |scene: &Scene| {
        let ship = scene.rig.server.world().grids().get(held_ship).unwrap();
        (ship.pose, ship.linear_velocity, ship.angular_velocity)
    };
    let before = read(&scene);
    let mut passing_rode_a_rail = true;
    let mut closest = f64::INFINITY;
    let mut passing_velocity = [0.0; 3];
    for _ in 0..60 {
        scene.ticks(1);
        let grids = scene.rig.server.world().grids();
        let (a, b) = (
            grids.get(held_ship).unwrap(),
            grids.get(passing_ship).unwrap(),
        );
        passing_rode_a_rail &= b.rail.is_some();
        passing_velocity = b.linear_velocity;
        closest = closest.min(length([0, 1, 2].map(|axis| {
            b.pose.position.in_voxels()[axis] - a.pose.position.in_voxels()[axis]
        })));
    }
    Flyby {
        held_before: before,
        held_after: read(&scene),
        passing_rode_a_rail,
        passing_velocity,
        closest,
        couplings: report().couplings,
    }
}

struct Flyby {
    held_before: (voxel_space::GridPose, [f64; 3], [f64; 3]),
    held_after: (voxel_space::GridPose, [f64; 3], [f64; 3]),
    passing_rode_a_rail: bool,
    /// How fast the passing car went at the end, in the world's axes.
    passing_velocity: [f64; 3],
    closest: f64,
    couplings: u64,
}

#[test]
fn a_ship_on_a_rail_that_comes_past_a_held_cars_joint_end_does_not_move_it_and_is_not_coupled_to_it_or_taken_off_its_rail()
 {
    // (The pull has no speed in it, and a car that comes past at the speed of its orbit is pulled for a tick: that is the pull's, and it is the passing car that has the force. The
    // held car is the one thing here that must not change, to the bit.)
    let (with, without) = (a_flyby(true), a_flyby(false));
    // The pull did act on the passing car, which is what makes this a test of the held car and not of two cars that never felt each other: it went 0.16 voxels a second slower
    // along its way than the same cars of plain stone did (measured 2026-11-22: the pull at its cap for a tick, 10 kilonewtons).
    let slowed = without.passing_velocity[2] - with.passing_velocity[2];
    assert!(
        slowed > 0.05,
        "the passing car was slowed by {slowed} voxels a second by the joint end it came past"
    );
    for (with_joints, flyby) in [(true, with), (false, without)] {
        assert!(
            flyby.closest < 8.0,
            "joints {with_joints}: the car did come past it: {} voxels at the nearest",
            flyby.closest
        );
        assert_eq!(
            flyby.held_after, flyby.held_before,
            "joints {with_joints}: the held car is where it was, as still, to the bit"
        );
        assert_eq!(
            flyby.couplings, 0,
            "joints {with_joints}: nothing was coupled"
        );
        assert!(
            flyby.passing_rode_a_rail,
            "joints {with_joints}: a pull never takes a ship off its rail, only a touch does"
        );
    }
}

#[test]
fn holding_a_car_of_a_group_on_a_rail_takes_the_group_off_it_at_once_and_the_chain_swings_about_the_car_with_its_joints_whole()
 {
    // Measured 2026-11-22: the chain was off its rail in the tick it was held, both couplings stayed, the held car did not move by a bit, and the pivots were never more than 6.6e-4
    // voxels apart (the chain's speed of 761 voxels a second was stopped by the car it is joined to).
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    let ships = a_chain_in_orbit(&mut scene, a_chain(), DISTANCE, speed);
    put_on_its_rail(&mut scene, &ships, speed, vec![([0.0; 3], [0.0; 3]); 3]);
    scene.ticks(10);
    let rides = |scene: &Scene| {
        scene
            .rig
            .server
            .world()
            .grids()
            .get(ships[0])
            .unwrap()
            .rail
            .is_some()
    };
    assert!(rides(&scene), "the chain rides its rail before it is held");
    let held = ships[1];
    scene.run(move |world| world.set_grid_held(held, true).unwrap());
    let held_at = origins_of(&scene, &[held])[0];
    let held_pose = scene.rig.server.world().grids().get(held).unwrap().pose;
    let (mut left_at, mut worst_gap, mut farthest) = (None, 0.0f64, 0.0f64);
    for tick in 0..100 {
        scene.ticks(1);
        if left_at.is_none() && !rides(&scene) {
            left_at = Some(tick);
        }
        worst_gap = worst_gap
            .max(pivot_gap(&scene, (ships[0], 7.0), ships[1]))
            .max(pivot_gap(&scene, (ships[1], 6.0), ships[2]));
        let centre = centre_of(&scene, &ships).0;
        farthest = farthest.max(length([0, 1, 2].map(|axis| centre[axis] - held_at[axis])));
    }
    assert!(
        left_at.is_some_and(|tick| tick <= 1),
        "the chain left its rail in tick {left_at:?} after the car was held"
    );
    assert_eq!(
        scene.rig.server.world().grids().get(held).unwrap().pose,
        held_pose,
        "the held car did not move"
    );
    assert_eq!(report().couplings, 2, "the chain is coupled still");
    assert!(
        worst_gap < 5.0e-3,
        "two pivots of the chain were {worst_gap} voxels apart"
    );
    // (The chain is 20 voxels long: a chain that went on at the speed of its orbit, 38 voxels a tick, would be thousands of voxels away.)
    assert!(
        farthest < 25.0,
        "the chain's centre was {farthest} voxels from the held car: it went on"
    );
}
