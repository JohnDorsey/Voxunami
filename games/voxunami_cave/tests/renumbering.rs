//! A ship given a new id in the middle of a scenario (`ServerWorld::renumber_ship`, a test-only change of id that changes nothing else): what keeps the old id of a ship after a restructuring
//! shows as a difference from the same scenario without it. A merge or a split does give ships new ids; a rename, which `docs/decisions/joints-and-cars.md` keeps open ("The planet's unchanged
//! identifier"), would give a ship a new id and nothing else, and these are the scenarios of the plan: a chain of coupled cars, a ship on a rail, a walker aboard. (The seat and the walker's
//! speed in the ship are not here: they do not survive it, and `docs/history/build-log.md`, "Joints, the change record and the test-only renumber", says how.)

mod common;

use std::sync::{Arc, Mutex};

use common::chains::{
    Vec3, a_chain, a_chain_in_orbit, a_swing, centre_of, orientation_of, origins_of, pivot_gap,
    set_going,
};
use common::docking::{make_all_going, stones};
use common::{
    PlayerExt, Scene, circular_speed, eyes_in_the_world, eyes_in_their_frame, length, minus,
    player_pose, pose_at, ship_id, tick_and_follow, walker_in_a_cabin,
};
use game_shell::KeyCode;
use voxel_space::GridId;

/// Gives a ship a new id in the next tick of the scene.
fn renumber(scene: &mut Scene, ship: GridId) -> GridId {
    let found = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&found);
    scene.run(move |world| {
        *slot.lock().unwrap() = Some(world.renumber_ship(ship));
    });
    let renumbered = found.lock().unwrap().take().expect("the tick ran");
    renumbered.expect("the ship was renumbered")
}

/// What a chain looked like at one tick: where each car's origin was from the centre of mass of the chain, how each was turned, and how far apart the first two cars' pivots were.
struct Snapshot {
    origins: Vec<Vec3>,
    turns: Vec<[f64; 4]>,
    gap: f64,
}

fn snapshot(scene: &Scene, ships: &[GridId]) -> Snapshot {
    let (centre, _) = centre_of(scene, ships);
    Snapshot {
        origins: origins_of(scene, ships)
            .into_iter()
            .map(|origin| minus(origin, centre))
            .collect(),
        turns: ships
            .iter()
            .map(|ship| orientation_of(scene, *ship).components())
            .collect(),
        gap: pivot_gap(scene, (ships[0], 7.0), ships[1]),
    }
}

/// How far apart two runs of a chain were at the worst: of the cars' places from the centre of mass (voxels), of their turns (one less the dot product of the two quaternions), and
/// the greatest gap of the pivots of either (metres).
struct Difference {
    origins: f64,
    turns: f64,
    gap: f64,
}

fn compared(control: &[Snapshot], changed: &[Snapshot]) -> Difference {
    let mut worst = Difference {
        origins: 0.0,
        turns: 0.0,
        gap: 0.0,
    };
    for (control, changed) in control.iter().zip(changed) {
        for (a, b) in control.origins.iter().zip(&changed.origins) {
            worst.origins = worst.origins.max(length(minus(*a, *b)));
        }
        for (a, b) in control.turns.iter().zip(&changed.turns) {
            let dot: f64 = (0..4).map(|part| a[part] * b[part]).sum();
            worst.turns = worst.turns.max(1.0 - dot.abs());
        }
        worst.gap = worst.gap.max(control.gap).max(changed.gap);
    }
    worst
}

/// A chain of three cars in orbit, set swinging, for `ticks` ticks, snapshotted after each. At the tick `at` the car `which` is given a new id if `renumbering`; the control does a
/// tick of nothing there instead, since the renumbering takes a tick of its own. Also the number of couplings the bodies had at the end.
fn swing(at: usize, which: usize, ticks: usize, renumbering: bool) -> (Vec<Snapshot>, u64) {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let distance = 70_016.0;
    let speed = circular_speed(&scene, 0, distance);
    let mut ships = a_chain_in_orbit(&mut scene, a_chain(), distance, speed);
    set_going(&mut scene, &ships, speed, a_swing());
    let mut trace = Vec::new();
    for tick in 0..ticks {
        if tick == at {
            if renumbering {
                ships[which] = renumber(&mut scene, ships[which]);
            } else {
                scene.run(|_| {});
            }
        }
        scene.ticks(1);
        trace.push(snapshot(&scene, &ships));
    }
    (trace, report().couplings)
}

#[test]
fn a_swinging_chain_whose_middle_or_last_car_is_given_a_new_id_swings_as_if_it_had_not_been() {
    // Measured: the two runs agree to 4e-14 voxels and 1e-16 in the turn, which is the rounding of the sums. The renumbering is at tick 10 (the chain has a frame of its own) and at 60
    // (it is on a rail).
    for (at, which) in [(10, 1), (60, 1), (60, 2)] {
        let (control, _) = swing(at, which, 100, false);
        let (changed, couplings) = swing(at, which, 100, true);
        let difference = compared(&control, &changed);
        assert!(
            difference.origins < 1e-9,
            "car {which} at {at}: the cars' places differ by {} voxels",
            difference.origins
        );
        assert!(
            difference.turns < 1e-12,
            "car {which} at {at}: the cars' turns differ by {}",
            difference.turns
        );
        assert!(
            difference.gap < 0.05,
            "car {which} at {at}: the joints were {} m apart",
            difference.gap
        );
        assert_eq!(couplings, 2, "and the chain is coupled still");
    }
}

#[test]
fn a_swinging_chain_whose_first_car_is_given_a_new_id_swings_as_if_it_had_not_been_to_a_small_fraction_of_a_voxel()
 {
    // The first car is the anchor of the group (the lowest id), which holds its rail and names its frame, and another car is the anchor after: a change that is not nothing. Measured:
    // 3.9e-5 voxels at the most when the chain has a frame of its own (tick 10), 4e-7 when it is on a rail (tick 60). (Believed, not looked for: the frame of its own is the first car's.)
    for at in [10, 60] {
        let (control, _) = swing(at, 0, 100, false);
        let (changed, couplings) = swing(at, 0, 100, true);
        let difference = compared(&control, &changed);
        assert!(
            difference.origins < 1e-3,
            "at {at}: the cars' places differ by {} voxels",
            difference.origins
        );
        assert!(
            difference.turns < 1e-9,
            "at {at}: the cars' turns differ by {}",
            difference.turns
        );
        assert!(difference.gap < 0.05);
        assert_eq!(couplings, 2);
    }
}

#[test]
fn a_ship_on_a_rail_that_is_given_a_new_id_goes_on_along_the_same_rail_to_the_bit() {
    let mut runs = Vec::new();
    for renumbering in [false, true] {
        let mut scene = Scene::in_a_new_world();
        let distance = 70_016.0;
        let speed = circular_speed(&scene, 0, distance);
        make_all_going(
            &mut scene,
            vec![(pose_at(0.0, distance, 0.0), stones(0, 1))],
            [0.0, 0.0, speed],
        );
        scene.ticks(40);
        let mut ship = ship_id(1);
        assert!(
            scene
                .rig
                .server
                .world()
                .grids()
                .get(ship)
                .unwrap()
                .rail
                .is_some(),
            "the premise: the ship is on a rail"
        );
        if renumbering {
            ship = renumber(&mut scene, ship);
        } else {
            scene.run(|_| {});
        }
        let mut seen = Vec::new();
        for _ in 0..60 {
            scene.ticks(1);
            let grid = scene.rig.server.world().grids().get(ship).unwrap();
            seen.push((grid.pose.position.in_voxels(), grid.rail.is_some()));
        }
        runs.push(seen);
    }
    assert!(
        runs[1].iter().all(|(_, on_a_rail)| *on_a_rail),
        "it kept its rail"
    );
    for (tick, (control, changed)) in runs[0].iter().zip(&runs[1]).enumerate() {
        assert_eq!(control.0, changed.0, "tick {tick}");
    }
}

#[test]
fn a_walker_aboard_a_ship_that_is_given_a_new_id_is_aboard_the_new_id_where_they_were_and_walks_on()
{
    let mut runs = Vec::new();
    for renumbering in [false, true] {
        let mut scene = Scene::new();
        let mut player = walker_in_a_cabin(&mut scene);
        let ship = player_pose(&scene, 1).grid;
        let now = if renumbering {
            renumber(&mut scene, ship)
        } else {
            scene.run(|_| {});
            ship
        };
        let mut seen = Vec::new();
        for _ in 0..40 {
            tick_and_follow(&mut scene, &mut player, 1);
            seen.push((
                player_pose(&scene, 1).grid,
                eyes_in_their_frame(&scene, 1),
                eyes_in_the_world(&scene, 1),
            ));
        }
        // Walking on, forward, for a second.
        player.hold(&[KeyCode::KeyW]);
        tick_and_follow(&mut scene, &mut player, 20);
        seen.push((
            player_pose(&scene, 1).grid,
            eyes_in_their_frame(&scene, 1),
            eyes_in_the_world(&scene, 1),
        ));
        runs.push((now, seen));
    }
    let (new, changed) = &runs[1];
    assert_ne!(*new, runs[0].0);
    assert!(
        changed.iter().all(|(grid, _, _)| grid == new),
        "aboard the new id the whole time"
    );
    for (control, changed) in runs[0].1.iter().zip(changed) {
        assert_eq!(control.1, changed.1, "the same place in the ship");
        assert_eq!(control.2, changed.2, "and in the world");
    }
}
