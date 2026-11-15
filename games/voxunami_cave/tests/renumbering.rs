//! A ship given a new id in the middle of a scenario (`ServerWorld::renumber_ship`, a test-only change of id that changes nothing else): what keeps the old id of a ship after a restructuring
//! shows as a difference from the same scenario without it. A merge or a split does give ships new ids; a rename, which `docs/decisions/joints-and-cars.md` keeps open ("The planet's unchanged
//! identifier"), would give a ship a new id and nothing else, and these are the scenarios of the plan: a chain of coupled cars, a ship on a rail, a walker aboard. (The seat and the walker's
//! speed in the ship are in `command_seat.rs` and `aboard_frame_terms.rs`: they survive it since `crate::renaming`.)

mod common;

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use common::chains::{
    Vec3, a_chain, a_chain_in_orbit, a_swing, centre_of, orientation_of, origins_of, pivot_gap,
    set_going,
};
use common::docking::{PLAIN, Placed, make_all_going, stone, stones};
use common::rails::put_a_ship_on_a_rail;
use common::{
    PlayerExt, Scene, circular_speed, eyes_in_the_world, eyes_in_their_frame, length, minus,
    player_pose, pose_at, put_aboard, ship_id, tick_and_follow, walker_in_a_cabin,
};
use game_shell::KeyCode;
use server::{ServerWorld, SplitPart};
use voxel_space::{GridId, Orientation, VoxelCoordinate};

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

/// A floor of stone, 8 by 8 voxels and one thick.
fn a_floor() -> Vec<Placed> {
    (0..8)
        .flat_map(|x| (0..8).map(move |z| ([x, 0, z], stone(), PLAIN)))
        .collect()
}

/// How far a walker aboard a floor that coasts on a rail round the home planet gets in the ship in the eight ticks after the tick in which the ship is given a new id, if
/// `renaming` (the walker jumped two ticks before; a ship in orbit is in free fall, so they go on up at the speed of the jump). The distance, and whether the ship was on its rail.
fn drift_of_a_walker_on_a_rail(renaming: bool) -> (f64, bool) {
    let mut scene = Scene::in_a_new_world();
    let distance = 70_016.0;
    let speed = circular_speed(&scene, 0, distance);
    make_all_going(
        &mut scene,
        vec![(pose_at(0.0, distance, 0.0), a_floor())],
        [0.0, 0.0, speed],
    );
    // A walker who is put aboard it, at rest in it, above its middle.
    let mut player = scene
        .rig
        .join_walking([4.0, distance + 3.0, 4.0 + speed * 0.05], 0.0, 0.0);
    scene.ticks(3);
    put_aboard(&mut scene, 1, [4.0, 2.5, 4.0]);
    tick_and_follow(&mut scene, &mut player, 30);
    let mut ship = ship_id(1);
    assert_eq!(player_pose(&scene, 1).grid, ship, "aboard");
    let on_a_rail = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship)
        .unwrap()
        .rail
        .is_some();
    player.hold(&[KeyCode::Space]);
    tick_and_follow(&mut scene, &mut player, 2);
    player.hold(&[]);
    let started = eyes_in_their_frame(&scene, 1);
    if renaming {
        ship = renumber(&mut scene, ship);
    } else {
        scene.run(|_| {});
    }
    tick_and_follow(&mut scene, &mut player, 8);
    assert_eq!(
        player_pose(&scene, 1).grid,
        ship,
        "aboard the ship, whatever it is called"
    );
    let later = eyes_in_their_frame(&scene, 1);
    (length(minus(later, started)), on_a_rail)
}

#[test]
fn a_walker_aboard_a_ship_on_a_rail_that_is_given_a_new_id_goes_on_in_it_as_they_were_going() {
    let (control, on_a_rail) = drift_of_a_walker_on_a_rail(false);
    assert!(on_a_rail, "the premise: the ship is on a rail");
    assert!(
        control > 1.2,
        "the premise: they were going up at the speed of the jump, {control}"
    );
    let (changed, on_a_rail) = drift_of_a_walker_on_a_rail(true);
    assert!(on_a_rail, "and it is on a rail still");
    assert!(
        (changed - control).abs() < 0.05,
        "they went {changed} in the renamed ship and {control} in the other"
    );
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------
// A walker aboard a ship on a rail that turns, and a walker who has stepped off a ship on a rail, when the ship changes id (2026-11-12).
// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------

/// Something done to a ship in the server in one tick that gives it a new id: what the id of the ship is after.
type Change = fn(&mut ServerWorld, GridId) -> GridId;

fn given_a_new_id(world: &mut ServerWorld, ship: GridId) -> GridId {
    world.renumber_ship(ship).expect("the ship was renumbered")
}

/// The voxels of a hollow cabin of 12 on a side (a shell one thick) that are in these columns along x.
fn shell_in(x: std::ops::Range<i128>) -> BTreeSet<VoxelCoordinate> {
    x.flat_map(|x| {
        (0..12).flat_map(move |y| {
            (0..12)
                .filter(move |&z| [x, y, z].iter().any(|&edge| edge == 0 || edge == 11))
                .map(move |z| VoxelCoordinate::new(x, y, z))
        })
    })
    .collect()
}

/// A hollow cabin cut at x = 6 into two halves, each of which has the centre of mass of its own blocks; the id of the half with the walker is the one that comes of it.
fn cut_in_half(world: &mut ServerWorld, ship: GridId) -> GridId {
    let part = |x: std::ops::Range<i128>| {
        let voxels = shell_in(x);
        let sum = voxels.iter().fold([0.0; 3], |sum, voxel| {
            [
                sum[0] + voxel.x() as f64 + 0.5,
                sum[1] + voxel.y() as f64 + 0.5,
                sum[2] + voxel.z() as f64 + 0.5,
            ]
        });
        let count = voxels.len() as f64;
        SplitPart {
            centre_of_mass: sum.map(|part| part / count),
            voxels,
        }
    };
    let ids = world
        .split_grid(ship, vec![part(0..6), part(6..12)])
        .expect("the cabin was cut");
    *ids.iter()
        .find(|part| !world.entities().in_grid(**part).is_empty())
        .expect("the walker is aboard one of the parts")
}

/// Does `change` to the ship in a tick of its own (the same tick, doing nothing, if there is none), and says what the ship is called after.
fn change_in_a_tick(scene: &mut Scene, ship: GridId, change: Option<Change>) -> GridId {
    let found = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&found);
    scene.run(move |world| {
        *slot.lock().unwrap() = Some(change.map_or(ship, |change| change(world, ship)));
    });
    found.lock().unwrap().take().expect("the tick ran")
}

/// A hollow cabin of 12 on a side that coasts on a rail round the home planet, turning at 0.8 radians a second about its vertical, with a walker aboard it who stands on the wall
/// the turn presses them to (the way up is toward the axis), and jumps off it. The ship is changed three ticks into the jump, with the walker in the air (or not); the eyes of the
/// walker in the ship are recorded tick by tick for twenty-five ticks. The track; and whether the ship was on its rail.
fn track_of_a_jump_in_a_spinning_cabin_on_a_rail(
    change: Option<Change>,
    whole_track: bool,
) -> (Vec<[f64; 3]>, [f64; 3], bool) {
    let mut scene = Scene::in_a_new_world();
    let mut ship = put_a_ship_on_a_rail(
        &mut scene,
        70_000.0,
        1.0,
        [12, 12, 12],
        |[x, y, z]| [x, y, z].iter().any(|&edge| edge == 0 || edge == 11),
        Orientation::IDENTITY,
        [0.0, 0.8, 0.0],
    );
    let place = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship)
        .expect("the ship")
        .pose
        .position
        .in_voxels();
    let mut player = scene
        .rig
        .join_walking([place[0], place[1] + 3.0, place[2]], 0.0, 0.0);
    scene.ticks(3);
    // (A little way from the wall they will come to rest on: the turn presses them to it.)
    put_aboard(&mut scene, 1, [9.2, 6.0, 6.0]);
    tick_and_follow(&mut scene, &mut player, 40);
    assert_eq!(player_pose(&scene, 1).grid, ship, "aboard");
    let on_a_rail = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship)
        .expect("the ship")
        .rail
        .is_some();
    player.hold(&[KeyCode::Space]);
    let mut track = Vec::new();
    let mut at_the_change = [0.0; 3];
    for tick in 0..25 {
        if tick == 1 {
            player.hold(&[]);
        }
        if tick == 3 {
            ship = change_in_a_tick(&mut scene, ship, change);
            // (The tick of the change has stepped the walker in the ship it has become.)
            assert_eq!(player_pose(&scene, 1).grid, ship, "aboard at the change");
            at_the_change = eyes_in_their_frame(&scene, 1);
            if !whole_track {
                return (track, at_the_change, on_a_rail);
            }
        }
        tick_and_follow(&mut scene, &mut player, 1);
        assert_eq!(
            player_pose(&scene, 1).grid,
            ship,
            "aboard the ship, whatever it is called, at {tick}"
        );
        track.push(eyes_in_their_frame(&scene, 1));
    }
    (track, at_the_change, on_a_rail)
}

/// A jump in a spinning cabin on a rail, and the cabin given a new id in the air: nothing is done to the walker, so their whole path is the one they would have had.
#[test]
fn a_jump_in_a_cabin_on_a_rail_that_turns_is_not_changed_by_the_cabin_being_given_a_new_id() {
    let (control, _, on_a_rail) = track_of_a_jump_in_a_spinning_cabin_on_a_rail(None, true);
    assert!(on_a_rail, "the premise: the ship is on a rail");
    let moved = control
        .iter()
        .map(|eyes| length(minus(*eyes, control[0])))
        .fold(0.0, f64::max);
    assert!(
        moved > 1.0,
        "the premise: the walker jumped off the wall and is in the air, {moved}"
    );
    let (changed, _, _) = track_of_a_jump_in_a_spinning_cabin_on_a_rail(Some(given_a_new_id), true);
    let worst = control
        .iter()
        .zip(&changed)
        .map(|(a, b)| length(minus(*a, *b)))
        .fold(0.0, f64::max);
    assert!(
        worst < 0.02,
        "the eyes were {worst} voxels from where they are with no change (the walker moves {moved} in all)"
    );
}

/// The same cabin cut in two, in the air. **Only the tick of the cut is the same**: a part that is cut off a spinning cabin is a free body (it has no rail), whose centre of mass goes
/// straight on at the speed it had and which turns about *its own* centre of mass, so the walker, who was 3 voxels from the old axis, is on the other side of the new one and is pulled
/// toward the open end of the half they were in, and falls out of it (measured 2026-11-12: the pull along the cabin was 1.9 and then -0.6). What a cut must not do is spoil the
/// walker's step in the tick it is made in: their eyes after it are where they are with no cut. (With their speed zeroed they would be 0.18 voxels behind.)
#[test]
fn a_jump_in_a_cabin_on_a_rail_that_turns_is_not_disturbed_in_the_tick_the_cabin_is_cut_in_two() {
    let (_, control, _) = track_of_a_jump_in_a_spinning_cabin_on_a_rail(None, false);
    let (_, cut, _) = track_of_a_jump_in_a_spinning_cabin_on_a_rail(Some(cut_in_half), false);
    let apart = length(minus(control, cut));
    assert!(
        apart < 0.005,
        "{apart} voxels apart after the tick of the cut: {control:?} and {cut:?}"
    );
}

/// How far the eyes of a walker go through the world in the twelve ticks after they have walked off the side of a floor that coasts on a rail round the home planet, the floor
/// being changed two ticks into them (or not). They are in the level then, and go on at the speed the floor gave them: what the air does to them is measured from the speed of the
/// place of the floor they left (`Departure`).
fn how_far_a_walker_who_left_a_floor_on_a_rail_goes(change: Option<Change>) -> f64 {
    let mut scene = Scene::in_a_new_world();
    let mut ship = put_a_ship_on_a_rail(
        &mut scene,
        70_000.0,
        1.0,
        [24, 1, 12],
        |_| true,
        Orientation::IDENTITY,
        [0.0; 3],
    );
    let place = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship)
        .expect("the ship")
        .pose
        .position
        .in_voxels();
    let mut player = scene
        .rig
        .join_walking([place[0], place[1] + 3.0, place[2]], 0.0, 0.0);
    scene.ticks(3);
    // A voxel from the side they will walk off (toward -z).
    put_aboard(&mut scene, 1, [12.0, 2.6, 1.5]);
    tick_and_follow(&mut scene, &mut player, 30);
    assert_eq!(player_pose(&scene, 1).grid, ship, "aboard");
    player.hold(&[KeyCode::KeyW]);
    let mut left = false;
    for _ in 0..100 {
        tick_and_follow(&mut scene, &mut player, 1);
        if player_pose(&scene, 1).grid.root_level().is_some() {
            left = true;
            break;
        }
    }
    assert!(left, "the premise: they walked off");
    player.hold(&[]);
    let started = eyes_in_the_world(&scene, 1);
    for tick in 0..12 {
        if tick == 2 {
            ship = change_in_a_tick(&mut scene, ship, change);
        }
        tick_and_follow(&mut scene, &mut player, 1);
    }
    assert!(
        player_pose(&scene, 1).grid.root_level().is_some(),
        "and they are in the level still"
    );
    assert!(
        scene.rig.server.world().grids().get(ship).is_some(),
        "and the ship is there under the name it has"
    );
    length(minus(eyes_in_the_world(&scene, 1), started))
}

fn a_walker_who_left_a_floor_on_a_rail_is_not_braked_by_a_change_of_id_of_the_floor(
    change: Change,
) {
    let control = how_far_a_walker_who_left_a_floor_on_a_rail_goes(None);
    // (Thirteen ticks at the speed of a circle at 70,000 metres: the change takes a tick of its own.)
    let expected = circular_speed(&Scene::in_a_new_world(), 0, 70_000.0) * 13.0 * 0.05;
    assert!(
        (control - expected).abs() < 0.05 * expected,
        "the premise: they go on at the speed of the floor, {control} and not {expected}"
    );
    let changed = how_far_a_walker_who_left_a_floor_on_a_rail_goes(Some(change));
    assert!(
        (changed - control).abs() < 0.3,
        "they went {changed} when the floor was changed and {control} when it was not"
    );
}

#[test]
fn a_walker_who_left_a_floor_on_a_rail_goes_on_at_its_speed_when_the_floor_is_given_a_new_id() {
    a_walker_who_left_a_floor_on_a_rail_is_not_braked_by_a_change_of_id_of_the_floor(
        given_a_new_id,
    );
}
