//! Joint ends that pull on each other when a car that has one rides a rail (step 4c-5-4b of `docs/decisions/joints-and-cars.md`, the joint ends: **a coupling is a touch, as a dock
//! is**, the owner's answer of 2026-11-19): the pull is a force on the rail and a torque on the bodies, so it never takes a car off its rail; only the coupling does, at the moment the
//! two ends are at one place, in line and slow enough to be coupled. Then both cars are taken off their rails (a coupling needs the one body world) and the bodies couple them in the tick
//! after, as they always did.
//!
//! Before this, a car alone on a rail that had another car near it was taken off its rail for any joint end that pulled on its own (`take_railed_groups_off_for_ends_that_pull`), 6 voxels
//! from the other car, because the bodies pull the ships of one frame and a ship on a rail has a frame of its own (or none). Now the pairs are found over the railed ships too (`rail_joints.rs`).
//!
//! Each case is run with the cars riding a rail and, as its oracle, free, which the bodies have always pulled and coupled. **What the pull does is measured as a difference of two runs**
//! (cars with joint ends, and the same cars of plain stone), as for the ports (`ports_on_rails.rs`): what a rail does to a car in a tick that a body does not is in both and is taken out.

mod common;

use common::chains::{Vec3, car, mass_of, origins_of, pivot_gap};
use common::docking::{make, make_going, stones};
use common::rails::{put_on_a_rail_near_another_ship, put_on_its_rail_in};
use common::{Scene, circular_speed, length, pose_at, ship_id};
use voxel_space::{GridId, GridPose};

const DISTANCE: f64 = 70_016.0;

/// How far apart the east pivot of the first car and the west pivot of the second are when the second is made, in voxels, and how fast the second goes toward the first, in voxels a
/// second: it comes into the range of the pull (6) as ships do, from outside it, in the tenth tick or so, when the second car has its rail already (a force that is asked of a free car
/// that is given a rail before the bodies take it is lost: a ship has a rail only with nothing near it, so that does not happen to a car that is pulled).
const GAP: f64 = 7.0;
const CLOSING: f64 = 2.0;

/// How far beyond `GAP` a car that is to be moved in is made, in voxels (out of the room of a stranger that a ship on a rail has a frame for: 761 voxels here), and how far from the
/// first car's pivot it lands, in voxels: within the range of a pull, as a command or an edit moves a ship in.
const FAR: f64 = 3000.0;
const MOVED_GAP: f64 = 5.5;

/// The length of each car, along x: three blocks of stone and a joint block.
const CAR_LENGTH: f64 = 4.0;

/// Which of the two cars ride a rail while they pull.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Riding {
    /// Neither: both are free, as they always were (the oracle).
    Neither,
    /// The first (the one with the joint end on its east face) rode a rail before the second came, and the second is free.
    First,
    /// Both: the second is put on a rail in the tick after it is made, as near the first as it is.
    Both,
}

/// The first car (number 1: three blocks of stone and a joint block at its east end), in orbit at `DISTANCE` going along z at the speed of a circle, and the second (number 2: a joint
/// block at its west end and three of stone) made `GAP` voxels beyond it going as it goes and `CLOSING` voxels a second toward it. `with_joints` false makes both of four blocks of
/// stone, which nothing pulls. The scene, what the bodies report, and the two cars.
fn a_pair_of_cars(
    riding: Riding,
    with_joints: bool,
) -> (
    Scene,
    impl Fn() -> grid_bodies::BodiesReport,
    GridId,
    GridId,
) {
    a_pair_arriving(riding, with_joints, false)
}

/// As [`a_pair_of_cars`], the second made `FAR` voxels farther if `far` (it is moved in by [`move_in`], and is free then).
fn a_pair_arriving(
    riding: Riding,
    with_joints: bool,
    far: bool,
) -> (
    Scene,
    impl Fn() -> grid_bodies::BodiesReport,
    GridId,
    GridId,
) {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    let (first, second) = if with_joints {
        (car(false, 3, true).0, car(true, 3, false).0)
    } else {
        (stones(0, 4), stones(0, 4))
    };
    make(&mut scene, pose_at(0.0, DISTANCE, 0.0), false, first);
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
        pose_at(
            origin[0] + CAR_LENGTH + GAP + if far { FAR } else { 0.0 },
            origin[1],
            origin[2],
        ),
        second,
        [-CLOSING, 0.0, speed],
    );
    // (The same number of ticks for every case: the second car is given its rail in this one, or nothing is done.)
    scene.run(move |world| {
        if riding == Riding::Both {
            put_on_a_rail_near_another_ship(world, b);
        }
    });
    (scene, report, a, b)
}

/// Moves a car made `FAR` voxels beyond the gap that cars are made at to `MOVED_GAP` from the first car's pivot (the same turn and speed).
fn move_in(world: &mut server::ServerWorld, ship: GridId) {
    let pose = world.data().grids().get(ship).expect("the car").pose;
    world
        .set_grid_pose(
            ship,
            GridPose::new(
                pose.position
                    .translated([-(FAR + GAP - MOVED_GAP), 0.0, 0.0]),
                pose.orientation,
            ),
        )
        .unwrap();
}

/// What is read of the two cars after a tick.
#[derive(Clone, Copy, Debug)]
struct Reading {
    speeds: [Vec3; 2],
    on_a_rail: [bool; 2],
    /// How far apart the two pivots are, in voxels.
    gap: f64,
}

fn readings_of(scene: &Scene, ships: [GridId; 2]) -> Reading {
    let grids = scene.rig.server.world().grids();
    let cars = ships.map(|ship| grids.get(ship).expect("the car is there"));
    Reading {
        speeds: cars.map(|car| car.linear_velocity),
        on_a_rail: cars.map(|car| car.rail.is_some()),
        gap: pivot_gap(scene, (ships[0], CAR_LENGTH), ships[1]),
    }
}

/// What is read after each of `ticks` ticks that the cars are followed for; if `moved`, the second car is moved in (`move_in`) in the first of them.
fn followed_for(riding: Riding, with_joints: bool, ticks: usize, moved: bool) -> Vec<Reading> {
    let (mut scene, _, a, b) = a_pair_arriving(riding, with_joints, moved);
    (0..ticks)
        .map(|tick| {
            if moved && tick == 0 {
                scene.run(move |world| move_in(world, b));
            } else {
                scene.ticks(1);
            }
            readings_of(&scene, [a, b])
        })
        .collect()
}

/// How the run went until the cars were coupled.
struct Run {
    /// The tick (from the first after the cars were made) in which the first car was found off its rail, if it was.
    left_at: Option<usize>,
    /// The tick the bodies reported the cars coupled in, if they did.
    coupled_at: Option<usize>,
    /// For each tick before the first car left its rail: the gap between the pivots after it, in voxels.
    gaps: Vec<f64>,
    /// How far apart the pivots were after the tick that the first car left its rail in.
    gap_at_the_touch: f64,
    /// Whether the second car rode a rail in every tick before it was taken off (it did not if it was free).
    second_rode_until_the_touch: bool,
    scene: Scene,
    report: Box<dyn Fn() -> grid_bodies::BodiesReport>,
    ships: [GridId; 2],
}

/// Runs the pair until it is coupled (at most 400 ticks).
fn until_they_are_coupled(riding: Riding) -> Run {
    let (mut scene, report, a, b) = a_pair_of_cars(riding, true);
    let (mut left_at, mut coupled_at) = (None, None);
    let (mut gaps, mut gap_at_the_touch) = (Vec::new(), f64::NAN);
    let mut second_rode_until_the_touch = true;
    for tick in 0..400 {
        scene.ticks(1);
        let reading = readings_of(&scene, [a, b]);
        if left_at.is_none() {
            if reading.on_a_rail[0] {
                gaps.push(reading.gap);
                second_rode_until_the_touch &= reading.on_a_rail[1] == (riding == Riding::Both);
            } else {
                left_at = Some(tick);
                gap_at_the_touch = reading.gap;
            }
        }
        if report().couplings > 0 {
            coupled_at = Some(tick);
            break;
        }
    }
    Run {
        left_at,
        coupled_at,
        gaps,
        gap_at_the_touch,
        second_rode_until_the_touch,
        scene,
        report: Box::new(report),
        ships: [a, b],
    }
}

#[test]
fn a_car_on_a_rail_stays_on_it_while_its_joint_end_pulls_and_comes_off_it_at_the_coupling_and_so_does_the_second_that_is_on_one()
 {
    for riding in [Riding::First, Riding::Both] {
        let run = until_they_are_coupled(riding);
        let left_at = run.left_at.unwrap_or_else(|| {
            panic!(
                "{riding:?}: the first car never left its rail; it was coupled at {:?}",
                run.coupled_at
            )
        });
        // The old rule took the car off its rail in the first tick of the pull, 6 voxels from the other.
        assert!(
            left_at >= 10,
            "{riding:?}: the first car rode its rail until tick {left_at} (the pull went on for a while)"
        );
        // It came off at the coupling: the pivots are within the coupling distance (0.3 metres; a voxel is a metre) and the tick after has them coupled. (The pivots were still
        // far apart while it rode: they were pulled together from 7 voxels, slowly, and a pair is coupled only when it is slow enough, so the car rode until they were nearly one.)
        assert!(
            run.gaps.len() == left_at && run.gaps.iter().all(|gap| *gap > 0.01),
            "{riding:?}: the pivots were still apart while the car was on its rail: {:?}",
            run.gaps
        );
        assert!(
            run.gap_at_the_touch <= 0.3,
            "{riding:?}: the pivots were {} voxels apart when the rail came off",
            run.gap_at_the_touch
        );
        assert!(
            run.second_rode_until_the_touch,
            "{riding:?}: the second car was on a rail exactly when it was meant to be"
        );
        let coupled_at = run
            .coupled_at
            .unwrap_or_else(|| panic!("{riding:?}: the cars were never coupled"));
        // (Measured 2026-11-19: the rail came off in tick 49 with the pivots 0.13 to 0.15 voxels apart, and the bodies coupled the cars in the next, as they do ends that are near enough,
        // in line and slow enough.)
        assert_eq!(
            coupled_at,
            left_at + 1,
            "{riding:?}: the rail came off in tick {left_at} and the cars were coupled in {coupled_at}"
        );
        let report = (run.report)();
        assert_eq!(
            report.rails_left_for_ends,
            if riding == Riding::Both { 2 } else { 1 },
            "{riding:?}: a rail was left for the coupling by each car that rode one, and not before"
        );
        assert_eq!(report.rail_kicks, 0, "{riding:?}: no hit was made");
        // And they are coupled for good: the joint holds.
        let mut scene = run.scene;
        scene.ticks(20);
        let gap = pivot_gap(&scene, (run.ships[0], CAR_LENGTH), run.ships[1]);
        assert!(gap < 0.05, "{riding:?}: the joint holds: {gap} voxels");
        assert_eq!((run.report)().couplings, 1, "{riding:?}: one coupling");
    }
}

/// The impulse (mass times the change of speed) that the pull had given each car by the end of each tick, from the first in which either had any: what the pull did to its speed
/// (the speed less that of the same car in the run of plain stone), times its mass.
fn impulses_given_by_the_pull(riding: Riding, ticks: usize, moved: bool) -> Vec<[Vec3; 2]> {
    let (scene, _, a, b) = a_pair_arriving(riding, true, moved);
    let masses = [a, b].map(|ship| mass_of(&scene, &[ship]));
    drop(scene);
    let with = followed_for(riding, true, ticks, moved);
    let without = followed_for(riding, false, ticks, moved);
    let mut impulses = Vec::new();
    for (with, without) in with.iter().zip(&without) {
        let effect = [0, 1]
            .map(|car| [0, 1, 2].map(|axis| with.speeds[car][axis] - without.speeds[car][axis]));
        let given = [0, 1].map(|car| effect[car].map(|part| part * masses[car]));
        // (From the first tick that a car has a newton second: the two runs are not the same to the last digit before the pull, 0.04 in a tick of the cars falling.)
        if given.iter().flatten().any(|part| part.abs() > 1.0) || !impulses.is_empty() {
            impulses.push(given);
        }
    }
    impulses
}

/// Asserts that what the pull gave the two cars is the same impulse the opposite way, in the run that `riding` and `moved` say.
fn assert_the_impulses_are_opposite(riding: Riding, moved: bool) {
    let given = impulses_given_by_the_pull(riding, 30, moved);
    assert!(
        given.len() >= 15,
        "{riding:?}, moved {moved}: the cars were pulled for a while: {} ticks",
        given.len()
    );
    // **A tick apart, for a car on a rail and one that is free**: the world makes the rail of a car on one whole, with the speed it has and the change added, in the tick that the pull is
    // asked in (the pull is asked after the step), and the bodies give the force to a car that is free in the step of the next. So what the first has by the end of a tick the free
    // second has by the end of the next. (Both on rails: in the same tick.)
    let skew = usize::from(riding == Riding::First);
    for tick in 0..given.len() - skew {
        let (first, second) = (given[tick][0], given[tick + skew][1]);
        assert!(
            length(first) > 1.0,
            "{riding:?}, moved {moved}, tick {tick}: the first car was pulled: {first:?}"
        );
        let sum = [0, 1, 2].map(|axis| first[axis] + second[axis]);
        assert!(
            length(sum) <= 1.0e-3 * length(first),
            "{riding:?}, moved {moved}, tick {tick}: the impulses {first:?} and {second:?} are not opposite"
        );
    }
}

#[test]
fn each_tick_the_pull_of_the_joint_ends_gives_each_car_the_same_impulse_the_opposite_way_whether_it_rides_a_rail_or_is_free()
 {
    for riding in [Riding::First, Riding::Both] {
        assert_the_impulses_are_opposite(riding, false);
    }
}

#[test]
fn a_car_that_is_moved_into_the_range_of_a_car_on_a_rail_pulls_it_and_is_pulled_once() {
    // The car on a rail has no frame of its own in the step that the other comes (a ship is near it only from then), so it is a fixed body in the frame of the free car: the pairs of
    // the ships of a frame would pull the free car a second time. The pull is the rails' (`rail_joints.rs`), and one.
    assert_the_impulses_are_opposite(Riding::First, true);
}

#[test]
fn the_pull_of_the_joint_ends_brings_the_cars_together_over_the_rails_as_it_does_when_they_are_free()
 {
    // The oracle is the free pair, which the bodies have always pulled and coupled. The speeds that they ask of the ends are the same; what differs (a rail answers a push a tick
    // later, and a free car's own fall is the solver's) is a few ticks in the time to the coupling.
    let free = until_they_are_coupled(Riding::Neither)
        .coupled_at
        .expect("the free cars were coupled");
    for riding in [Riding::First, Riding::Both] {
        let on_rails = until_they_are_coupled(riding)
            .coupled_at
            .unwrap_or_else(|| panic!("{riding:?}: the cars were never coupled"));
        assert!(
            on_rails.abs_diff(free) <= 4,
            "{riding:?}: coupled in tick {on_rails}, the free pair in {free}"
        );
    }
}
