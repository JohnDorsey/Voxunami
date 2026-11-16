//! The contact frame (the rest of step 4c-4 of `docs/decisions/joints-and-cars.md`): promise (a) of the joint plan with something near. **A mechanism does not work differently because
//! a ship is near**, or because of where it is or how fast it goes. These tests are about a chain of three cars (`common/chains.rs`) with a stranger (a ship of one stone) near it.
//!
//! Two kinds. The tests about *nearness* say that a stranger that is near and does not touch changes nothing of what the chain does, to the last digits that the chain alone and
//! the chain on a rail agree to. The tests about *contact* are the controls that any change to that must keep: a stranger that does come in does hit the chain, however fast, and the
//! chain is pushed by the share of the stranger's momentum that physics says and stays coupled.

mod common;

use common::chains::{
    Setting, a_chain, a_chain_in_orbit, a_swing, centre_of, how_the_chain_turns_after, mass_of,
    most_apart, origins_of, pivot_gap, set_going,
};
use common::docking::{PLAIN, Placed, facing, make_all_going, port_block, stone, stones};
use common::{Scene, circular_speed, length, pose_at, ship_id};
use voxel_space::{BrickCoordinate, FaceDirection, GridId, GridPose, Orientation, WorldPosition};

/// Ticks after the chain is set swinging at which two runs are compared: before its swing turns back (about the 130th), where a difference of the rounding of a few ticks before
/// is made large (`how_the_chain_turns_after`).
const TICKS: u32 = 100;

/// How alike the facings and turns of two chains have to be to be the same: what the chain alone at two distances, and alone and on a rail, agree to (`group_off_a_rail.rs`).
const THE_SAME: f64 = 1e-7;

const DISTANCE: f64 = 70_016.0;

/// Where the hit of a stranger on the chain is made: a million voxels from the planet's middle, where its pull on the pair is a few thousandths of what it is at `DISTANCE`. (At 70
/// kilometres the planet's pull along the chain, a few hundredths of a voxel a second a second, gave the pair a momentum of -3806 over the run, the same in every combination of the
/// passes and the cut of step 4c-5-A, and so the planet's and not the hit's: 0.91 percent of what the chain gained before them and 1.1 percent after, with the threshold at 1. Here
/// the momentum the hit does not keep is 2 parts in a million, with or without them.)
const FAR_FROM_THE_PLANET: f64 = 1_000_000.0;

#[test]
fn a_stranger_that_is_near_and_does_not_touch_changes_nothing_of_what_a_chain_does() {
    let alone = how_the_chain_turns_after(DISTANCE, Setting::Alone, TICKS);
    let near = how_the_chain_turns_after(DISTANCE, Setting::WithAStranger, TICKS);
    let apart = most_apart(&alone, &near);
    assert!(
        apart < THE_SAME,
        "with a stranger fifteen voxels off the chain differs from the one alone by {apart}"
    );
    // (They did swing.)
    assert!(alone.iter().any(|(_, spin)| length(*spin) > 0.05));
}

#[test]
fn with_a_stranger_near_what_a_chain_does_does_not_depend_on_where_it_is_or_how_fast_it_goes() {
    let near = how_the_chain_turns_after(70_016.0, Setting::WithAStranger, TICKS);
    let far = how_the_chain_turns_after(140_000.0, Setting::WithAStranger, TICKS);
    let apart = most_apart(&near, &far);
    assert!(
        apart < THE_SAME,
        "with a stranger near, the chain at 70 and at 140 kilometres differs by {apart}"
    );
}

/// A wall of stone, 13 by 13 voxels and one thick, its middle voxel at the origin of its ship: a stranger that a few metres of aim cannot make miss. (A ship at orbital speed in the
/// frame everything is in climbs by the gain of energy that its steps are known to have, `future-goals.md`, "Forces over solver steps": twenty metres in the seven seconds that a
/// stranger closing at 20 metres a second needs to cover a hundred and fifty voxels, which a single stone went over.)
fn a_wall() -> Vec<Placed> {
    (-6..=6)
        .flat_map(|y| (-6..=6).map(move |z| ([0, y, z], stone(), PLAIN)))
        .collect()
}

/// What a stranger (the wall) that closed on the east end of a chain at `closing` metres a second, from a hundred and fifty voxels off along the chain, did in the ticks asked for.
struct Hit {
    /// How much the speed of the chain's centre of mass along the chain changed.
    chain_change: f64,
    /// The same for the wall.
    wall_change: f64,
    /// The momentum that both gained along the chain, together (nothing but a pull a few hundredths of a metre a second a second across the planet's orbit acts along it).
    momentum_gained: f64,
    /// The momentum the chain gained, to say what a part of it is.
    momentum_of_the_chain: f64,
    /// The joints' worst gap, in voxels.
    worst_gap: f64,
    coupled: bool,
    /// How many times a body was given what the ghost had for it, and how many ghosts there were at the end.
    corrections: u64,
    ghosts: u64,
}

fn a_stranger_closing_on_the_chain(closing: f64, ticks: usize) -> Hit {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, FAR_FROM_THE_PLANET);
    let ships = a_chain_in_orbit(&mut scene, a_chain(), FAR_FROM_THE_PLANET, speed);
    set_going(&mut scene, &ships, speed, a_swing());
    scene.ticks(5);
    let end = {
        let origin = origins_of(&scene, &ships)[2];
        [origin[0] + 5.0 + 150.0, origin[1], origin[2]]
    };
    make_all_going(
        &mut scene,
        vec![(pose_at(end[0], end[1], end[2]), a_wall())],
        [-closing, 0.0, speed],
    );
    scene.ticks(2);
    let wall = ship_id(4);
    let before = (centre_of(&scene, &ships).1, centre_of(&scene, &[wall]).1);
    let (chain_mass, wall_mass) = (mass_of(&scene, &ships), mass_of(&scene, &[wall]));
    let mut worst_gap: f64 = 0.0;
    for _ in 0..ticks {
        scene.ticks(1);
        worst_gap = worst_gap
            .max(pivot_gap(&scene, (ships[0], 7.0), ships[1]))
            .max(pivot_gap(&scene, (ships[1], 6.0), ships[2]));
    }
    let after = (centre_of(&scene, &ships).1, centre_of(&scene, &[wall]).1);
    let chain_change = after.0[0] - before.0[0];
    let wall_change = after.1[0] - before.1[0];
    Hit {
        chain_change,
        wall_change,
        momentum_gained: chain_mass * chain_change + wall_mass * wall_change,
        momentum_of_the_chain: chain_mass * chain_change,
        worst_gap,
        coupled: report().couplings == 2,
        corrections: report().ghost_corrections,
        ghosts: report().ghosts,
    }
}

#[test]
fn a_stranger_that_closes_on_the_chain_hits_it_whatever_the_speed_it_closes_at() {
    // Metres a second the stranger closes at, and the ticks to run: it covers a hundred and fifty voxels and hits within them, and the slowest takes longest.
    for (closing, ticks) in [(50.0, 100), (150.0, 50), (300.0, 40)] {
        let hit = a_stranger_closing_on_the_chain(closing, ticks);
        // It hit: the chain went the way the stranger went (along -x), and the stranger lost speed. (A part of the share a plastic hit gives it, which the joints and the turn of the
        // chain take their share of, and the exact amount is the chain's: the control that it is not changed is the conservation below.)
        assert!(
            hit.chain_change < -0.1 * closing && hit.wall_change > 0.0,
            "closing at {closing}: the chain's speed along it changed by {} and the wall's by {}",
            hit.chain_change,
            hit.wall_change
        );
        // Momentum was kept: what one gained the other lost, to a hundredth of what the chain gained.
        assert!(
            hit.momentum_gained.abs() < 0.01 * hit.momentum_of_the_chain.abs(),
            "closing at {closing}: together they gained {} of momentum and the chain {}",
            hit.momentum_gained,
            hit.momentum_of_the_chain
        );
        assert!(
            hit.coupled,
            "closing at {closing}: the chain is coupled still"
        );
        assert!(
            hit.corrections > 0 && hit.ghosts == 1,
            "closing at {closing}: the hit is the ghost's ({} corrections, {} ghosts)",
            hit.corrections,
            hit.ghosts
        );
        assert!(
            hit.worst_gap < 0.5,
            "closing at {closing}: the joints' worst gap was {} voxels",
            hit.worst_gap
        );
    }
}

/// The chain's ships (numbers 1 to 3) in orbit with nothing but a stone (number 4) a few thousandths of a voxel from it, going as the chain goes: they touch, and nothing moves either of
/// them but the planet's pull, which is the same on both (a frame in free fall with one of them has the other fall at the difference only). Where the stone is, from the
/// origin of the chain's first car.
fn a_stone_against_a_chain(at: [f64; 3]) -> (Scene, Vec<GridId>) {
    let mut scene = Scene::in_a_new_world();
    let distance = 70_016.0;
    let speed = circular_speed(&scene, 0, distance);
    // (All made in one tick, so that they are all as old: a stone made later would have fallen less, and slide off the end at the difference.)
    let mut made = Vec::new();
    let mut length_so_far = 0.0;
    for (blocks, length) in a_chain() {
        made.push((pose_at(length_so_far, distance, 0.0), blocks));
        length_so_far += length;
    }
    made.push((pose_at(at[0], distance + at[1], at[2]), stones(0, 1)));
    make_all_going(&mut scene, made, [0.0, 0.0, speed]);
    scene.ticks(2);
    (scene, (1..=3).map(ship_id).collect())
}

/// How far a stone that began at `at` against the chain (see [`a_stone_against_a_chain`]) has moved against the chain's last car, at the most, in three seconds.
fn how_far_a_stone_moves_against_a_chain(at: [f64; 3]) -> f64 {
    let (mut scene, ships) = a_stone_against_a_chain(at);
    let stranger = ship_id(4);
    let apart = |scene: &Scene| {
        let end = origins_of(scene, &ships)[2];
        let stone = origins_of(scene, &[stranger])[0];
        [0, 1, 2].map(|axis| stone[axis] - end[axis])
    };
    let begin = apart(&scene);
    let mut most: f64 = 0.0;
    for _ in 0..60 {
        scene.ticks(1);
        let now = apart(&scene);
        most = most.max(length([0, 1, 2].map(|axis| now[axis] - begin[axis])));
    }
    most
}

#[test]
fn a_stone_that_rests_against_a_chain_in_orbit_stays_there_whatever_the_pull_of_the_planet_is() {
    // Both fall round the planet together. Where each is in a frame of its own the other is pulled by the planet alone in it, and a stone that the frame of the chain did not fall
    // with would be pressed on the chain by the pull's whole size, if it lies on it along the pull (eight metres a second a second at this distance: it fell 0.01 voxels in a tick,
    // and the stone is 0.004 above the last car; ran: 1.9 voxels away in three seconds with the pull of the planet left in what the ghost is told, 0.6 without it, with the step the
    // frame of the group had then).
    //
    // **What is left is not the ghost's:** the frame of a group that is alone falls by velocity Verlet (`Follow::advance`) and the stone, in the frame everything is in, by the
    // bodies' 24 steps of it with the pull held over the tick, and the two are not the same step: they part by 0.012 voxels in the three seconds (ran: 0.0118). The frame of the
    // group went by one semi-implicit step a tick until 2026-11-15 (`v dt + g dt^2`), and then it was 0.61 (the same chain and stone: it was 50 times as much); the step is
    // measured in `docs/decisions/joints-and-cars.md`, 4c-5-2.
    let most = how_far_a_stone_moves_against_a_chain([15.0, 1.004, 0.0]);
    assert!(
        most < 0.05,
        "the stone above the last car moved {most} voxels against the chain in three seconds"
    );
}

/// What two chains of three cars, the second closing on the east end of the first at `closing` metres a second from a hundred and fifty voxels off, did in the ticks asked for.
struct TwoChains {
    /// How fast the second chain goes against the first along the chain, before and after (a hit that keeps no speed of its own, equal masses, ends with the two together).
    closing_before: f64,
    closing_after: f64,
    /// The momentum that both gained along the chain, together, and the momentum that the first gained.
    momentum_gained: f64,
    momentum_of_the_first: f64,
    worst_gap: f64,
    couplings: u64,
    ghosts: u64,
    ghosts_made: u64,
    corrections: u64,
}

fn two_chains_that_meet(closing: f64, ticks: usize) -> TwoChains {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let distance = 70_016.0;
    let speed = circular_speed(&scene, 0, distance);
    let mut made = Vec::new();
    let mut at = 0.0;
    for (blocks, length) in a_chain() {
        made.push((pose_at(at, distance, 0.0), blocks));
        at += length;
    }
    let mut seen = Vec::new();
    for (blocks, length) in a_chain() {
        seen.push((pose_at(at + 150.0, distance, 0.0), blocks));
        at += length;
    }
    // (The second chain goes at the first's speed and `closing` more, all made in one tick so that they are as old.)
    made.extend(seen);
    make_all_going(&mut scene, made, [0.0, 0.0, speed]);
    let (first, second): (Vec<GridId>, Vec<GridId>) = (
        (1..=3).map(ship_id).collect(),
        (4..=6).map(ship_id).collect(),
    );
    scene.run({
        let second = second.clone();
        move |world| {
            for ship in &second {
                world
                    .set_grid_velocity(*ship, [-closing, 0.0, speed], [0.0; 3])
                    .unwrap();
            }
        }
    });
    scene.ticks(2);
    let before = (centre_of(&scene, &first).1, centre_of(&scene, &second).1);
    let masses = (mass_of(&scene, &first), mass_of(&scene, &second));
    let mut worst_gap: f64 = 0.0;
    for _ in 0..ticks {
        scene.ticks(1);
        for (ships, car) in [(&first, 0), (&second, 0)] {
            worst_gap = worst_gap
                .max(pivot_gap(&scene, (ships[car], 7.0), ships[car + 1]))
                .max(pivot_gap(&scene, (ships[car + 1], 6.0), ships[car + 2]));
        }
    }
    let after = (centre_of(&scene, &first).1, centre_of(&scene, &second).1);
    TwoChains {
        closing_before: before.0[0] - before.1[0],
        closing_after: after.0[0] - after.1[0],
        momentum_gained: masses.0 * (after.0[0] - before.0[0])
            + masses.1 * (after.1[0] - before.1[0]),
        momentum_of_the_first: masses.0 * (after.0[0] - before.0[0]),
        worst_gap,
        couplings: report().couplings,
        ghosts: report().ghosts,
        ghosts_made: report().ghosts_made,
        corrections: report().ghost_corrections,
    }
}

#[test]
fn two_chains_that_meet_hit_each_other_once_and_not_twice() {
    // Each is a mechanism with a frame of its own, and one hit is two bodies' share of the impulse: the contact is worked out once for the two, and each has its share of it. A hit
    // that was counted for each chain by itself would give each the whole of it, and they would part at the speed they closed at (they close at a hundred and end together, to a third
    // of a metre a second, which is what the joints and the settling passes keep: ran before step 4c-5-2 was wired, with both in the frame everything is in).
    let closing = 100.0;
    let hit = two_chains_that_meet(closing, 80);
    assert!(
        (hit.closing_before - closing).abs() < 1e-3 * closing,
        "the chains closed at {} and not at {closing}",
        hit.closing_before
    );
    assert!(
        hit.closing_after.abs() < 0.01 * closing,
        "after the hit they go at {} against each other",
        hit.closing_after
    );
    assert!(
        hit.momentum_gained.abs() < 0.01 * hit.momentum_of_the_first.abs(),
        "together they gained {} of momentum and the first chain {}",
        hit.momentum_gained,
        hit.momentum_of_the_first
    );
    assert_eq!(hit.couplings, 4, "both chains are coupled still");
    assert_eq!(
        (hit.ghosts, hit.ghosts_made),
        (1, 1),
        "one ghost for the two chains, which are one mechanism for it"
    );
    assert!(hit.corrections > 0, "and the hit was its");
    assert!(
        hit.worst_gap < 0.5,
        "the joints' worst gap was {} voxels",
        hit.worst_gap
    );
}

#[test]
fn a_stranger_that_is_near_and_does_not_touch_has_a_ghost_that_gives_nothing() {
    // The ghost is there (the stranger is near: fifteen voxels from the end of a swinging chain, in the room of a ghost) and it corrects nothing, in a hundred ticks: "nothing" is
    // what makes the first test above true to the bit, and not to the rounding of a sum of two worlds that differ by nothing.
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let distance = DISTANCE;
    let speed = circular_speed(&scene, 0, distance);
    let mut made = Vec::new();
    let mut at = 0.0;
    for (blocks, length) in a_chain() {
        made.push((pose_at(at, distance, 0.0), blocks));
        at += length;
    }
    made.push((pose_at(at + 15.0, distance, 0.0), stones(0, 1)));
    make_all_going(&mut scene, made, [0.0, 0.0, speed]);
    scene.ticks(2);
    let ships: Vec<GridId> = (1..=3).map(ship_id).collect();
    set_going(&mut scene, &ships, speed, a_swing());
    scene.ticks(TICKS as usize);
    let report = report();
    assert_eq!(
        report.ghosts, 1,
        "one ghost, for the chain and what is near it"
    );
    assert_eq!(report.ghosts_made, 1, "made once and kept");
    assert_eq!(report.ghost_corrections, 0, "and it corrected nothing");
    assert_eq!(
        report.frames, 2,
        "the chain has its own frame, and the stranger the one everything is in"
    );
    assert_eq!(report.couplings, 2);
}

#[test]
fn a_ghost_goes_when_what_was_near_the_chain_goes_and_a_chain_with_nothing_near_has_none() {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    let ships = a_chain_in_orbit(&mut scene, a_chain(), DISTANCE, speed);
    set_going(&mut scene, &ships, speed, a_swing());
    scene.ticks(5);
    assert_eq!(report().ghosts, 0, "alone: no ghost");
    // A stranger far off (5 kilometres): not near enough for a ghost.
    make_all_going(
        &mut scene,
        vec![(pose_at(0.0, DISTANCE + 5_000.0, 0.0), stones(0, 1))],
        [0.0, 0.0, speed],
    );
    scene.ticks(5);
    assert_eq!(
        report().ghosts,
        0,
        "a stranger far away is no reason for a ghost"
    );
    // It comes to ten voxels from the end of the chain: a ghost.
    let stranger = ship_id(4);
    let end = {
        let origin = origins_of(&scene, &ships)[2];
        [origin[0] + 5.0, origin[1], origin[2]]
    };
    scene.run(move |world| {
        world
            .set_grid_pose(stranger, pose_at(end[0] + 10.0, end[1], end[2]))
            .unwrap();
    });
    scene.ticks(5);
    assert_eq!(report().ghosts, 1);
    // And away again.
    let away = {
        let origin = origins_of(&scene, &ships)[2];
        pose_at(origin[0], origin[1] + 5_000.0, origin[2])
    };
    scene.run(move |world| {
        world.set_grid_pose(stranger, away).unwrap();
    });
    scene.ticks(5);
    assert_eq!(report().ghosts, 0, "the ghost goes with what was near");
    assert_eq!(report().ghosts_made, 1);
    assert_eq!(report().ghost_corrections, 0);
}

/// Two cars of the chain coupled to each other, and a third three voxels from the free end of the second, in orbit (made in one tick). The third is the car of `a_chain`, which has a
/// joint end on its west face.
fn two_cars_and_a_third_three_voxels_off() -> (Scene, impl Fn() -> grid_bodies::BodiesReport) {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    let mut made = Vec::new();
    let mut at = 0.0;
    for (index, (blocks, length)) in a_chain().into_iter().enumerate() {
        made.push((pose_at(at, DISTANCE, 0.0), blocks));
        at += length;
        if index == 1 {
            at += 3.0;
        }
    }
    make_all_going(&mut scene, made, [0.0, 0.0, speed]);
    (scene, report)
}

#[test]
fn a_car_whose_joint_end_pulls_on_the_chain_has_it_in_the_frame_it_is_in_until_they_are_coupled() {
    // Joints are made and pulled between ships of one frame: a group that is alone and has a stranger near is in a frame of its own and the stranger is in the other, and a car three
    // voxels from its free end would never come. (Before step 4c-5-2 the stranger took the group into the frame everything is in, which is what is kept for this.)
    let (mut scene, report) = two_cars_and_a_third_three_voxels_off();
    scene.ticks(100);
    assert_eq!(report().couplings, 2, "the third car came and is coupled");
    assert_eq!(
        report().frames,
        1,
        "the three are one group, alone, in a frame of its own"
    );
    assert_eq!(report().ghosts, 0, "and nothing is near it");
}

#[test]
fn a_car_whose_joint_end_faces_away_from_the_chain_is_a_stranger_to_it_and_has_a_ghost_and_no_coupling()
 {
    // The same, with the third car turned the other way (its joint end looks east, away from the chain): it does not pull, so the group keeps its frame, and the car is a
    // stranger three voxels from it, in the ghost's room.
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    let mut made = Vec::new();
    let mut at = 0.0;
    for (index, (blocks, length)) in a_chain().into_iter().enumerate() {
        if index < 2 {
            made.push((pose_at(at, DISTANCE, 0.0), blocks));
        } else {
            // (The car of the chain turned half way about y: its west end is its east end in the world, and its joint end is the one that is not at the chain.)
            made.push((turned_about_y(at + 3.0 + length, DISTANCE, 0.0), blocks));
        }
        at += length;
        if index == 1 {
            at += 3.0;
        }
    }
    make_all_going(&mut scene, made, [0.0, 0.0, speed]);
    scene.ticks(100);
    assert_eq!(report().couplings, 1, "no third coupling");
    assert_eq!(
        report().frames,
        2,
        "the group has its own frame and the third car the other"
    );
    assert_eq!(report().ghosts, 1);
}

/// A pose with the ship turned half way about y, its origin at this place.
fn turned_about_y(x: f64, y: f64, z: f64) -> GridPose {
    GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z]),
        Orientation::from_yaw_pitch(std::f64::consts::PI, 0.0),
    )
}

#[test]
fn a_ship_whose_docking_port_pulls_on_a_port_of_the_group_is_docked_to_it_and_not_left_to_the_ghost()
 {
    // Ports pull and dock ships of one frame, as joint ends do. A car of the pair that has a port on its east face, and a ship with one on its west face three voxels off: the group
    // has the frame the ship is in until they dock (two ships, the pair's first car and the one the other two made).
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    let (first, last) = a_chain().into_iter().enumerate().fold(
        (Vec::new(), Vec::new()),
        |(mut first, mut last), (index, (blocks, length))| {
            if index == 0 {
                first = blocks;
            } else if index == 1 {
                // The second car, with a port in the place of its east joint.
                let mut car = blocks;
                car.retain(|(at, _, _)| at[0] != length as i128 - 1);
                car.push((
                    [length as i128 - 1, 0, 0],
                    port_block(),
                    facing(FaceDirection::PositiveX),
                ));
                last = car;
            }
            (first, last)
        },
    );
    let mut stranger = vec![([0, 0, 0], port_block(), facing(FaceDirection::NegativeX))];
    stranger.extend(stones(1, 3));
    make_all_going(
        &mut scene,
        vec![
            (pose_at(0.0, DISTANCE, 0.0), first),
            (pose_at(7.0, DISTANCE, 0.0), last),
            (pose_at(7.0 + 6.0 + 3.0, DISTANCE, 0.0), stranger),
        ],
        [0.0, 0.0, speed],
    );
    scene.ticks(150);
    let ships = scene
        .rig
        .server
        .world()
        .grids()
        .iter()
        .filter(|(_, ship)| ship.terrain.is_none())
        .count();
    assert_eq!(ships, 2, "the last car and the ship are one ship now");
    assert_eq!(report().couplings, 1, "and the first car is coupled to it");
}
