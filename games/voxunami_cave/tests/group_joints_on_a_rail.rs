//! A joint end of a car of a chain that rides a rail pulls on the joint end of a car (step 4c-5-4b of `docs/decisions/joints-and-cars.md`, for groups): **the pull is a force on the
//! group's rail and a torque on the bodies, so it never takes the chain off its rail; only the coupling, which is a touch, does** (the owner, 2026-11-19). This is `joints_on_rails.rs`
//! for a group.
//!
//! The chain is three cars with a joint end on the east end of the last, and the car is the one with a joint end on its west face that `joints_on_rails.rs` has, made `GAP` voxels beyond
//! it and closing on it. Each case is run with the chain on a rail and, as its oracle, free (the bodies have always pulled and coupled a group that is free), each with joints and
//! as plain stone, so that what a pull does is a difference of two runs: what a rail does to a car in a tick that a body does not is in both and is taken out.
//!
//! What a pull does to a group is its **centre of mass**: the impulse on the group is its mass times the change of the speed of its centre of mass, which a rail has exact.

mod common;

use common::chains::{
    Vec3, a_chain_in_orbit, car, centre_of, mass_of, origins_of, pivot_gap, put_on_its_rail,
    set_going,
};
use common::docking::{PLAIN, make_going, stone, stones};
use common::joints::{ball_end_toward, joint_block};
use common::rails::put_on_a_rail_near_another_ship;
use common::{Scene, circular_speed, length, pose_at, ship_id};
use voxel_space::{FaceDirection, GridId};

const DISTANCE: f64 = 70_016.0;

/// How far the car's west pivot is from the chain's east pivot when it is made, in voxels, and how fast it closes on it, in voxels a second: it comes into the range of the pull (6) as
/// ships do, from outside it, with its rail already if it is to have one.
const GAP: f64 = 7.0;
const CLOSING: f64 = 2.0;

/// The length of the chain, along x: the cars are 3, 4 and 3 voxels long (the last has the joint end on its east end).
const LENGTH: f64 = 10.0;

/// Whether the chain rides a rail while the joint ends pull.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Chain {
    /// Free, as a group always was (the oracle).
    Free,
    /// On the rail of its centre of mass, put on it before the car came.
    OnARail,
    /// On its rail, and the car is put on one in the tick after it is made, as near the chain as it is.
    AndTheCarOnOne,
}

/// The chain (three cars, numbers 1 to 3, coupled by their joints, standing still against each other) in orbit at `DISTANCE` going along z at the speed of a circle, with a joint end
/// on the east end of the last car (or a stone of the same size, if `with_joints` is false), and the car (number 4: a joint end and three stones, or four stones) made `GAP` voxels
/// beyond the chain's end, going as it goes and `CLOSING` voxels a second toward it. The scene, what the bodies report, the cars and the car.
fn a_chain_and_a_car(
    chain: Chain,
    with_joints: bool,
) -> (
    Scene,
    impl Fn() -> grid_bodies::BodiesReport,
    Vec<GridId>,
    GridId,
) {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    // Three light cars (3, 4 and 3 voxels long): the heavy ones of `a_chain` turn too slowly into line for a coupling before they touch (the free chain does not couple either).
    let mut cars = vec![car(false, 2, true), car(true, 2, true), car(true, 1, false)];
    if with_joints {
        cars[2].0.push((
            [2, 0, 0],
            joint_block(),
            ball_end_toward(FaceDirection::PositiveX),
        ));
    } else {
        cars[2].0.push(([2, 0, 0], stone(), PLAIN));
    }
    cars[2].1 += 1.0;
    let ships = a_chain_in_orbit(&mut scene, cars, DISTANCE, speed);
    let still = vec![([0.0; 3], [0.0; 3]); 3];
    match chain {
        Chain::OnARail | Chain::AndTheCarOnOne => {
            put_on_its_rail(&mut scene, &ships, speed, still);
        }
        Chain::Free => set_going(&mut scene, &ships, speed, still),
    }
    scene.ticks(3);
    let second = if with_joints {
        car(true, 3, false).0
    } else {
        stones(0, 4)
    };
    let origin = origins_of(&scene, &ships[..1])[0];
    make_going(
        &mut scene,
        pose_at(origin[0] + LENGTH + GAP, origin[1], origin[2]),
        second,
        [-CLOSING, 0.0, speed],
    );
    // (The same number of ticks for every case: the car is given its rail in this one, or nothing is done.)
    let ship = ship_id(4);
    scene.run(move |world| {
        if chain == Chain::AndTheCarOnOne {
            put_on_a_rail_near_another_ship(world, ship);
        }
    });
    (scene, report, ships, ship)
}

/// What is read of the chain and the car after a tick.
#[derive(Clone, Copy, Debug)]
struct Reading {
    chain_speed: Vec3,
    car_speed: Vec3,
    /// Whether the chain's anchor and the car ride a rail.
    on_a_rail: [bool; 2],
    /// How far the chain's east pivot and the car's west pivot are apart, in voxels.
    gap: f64,
}

fn read(scene: &Scene, ships: &[GridId], car: GridId) -> Reading {
    let grids = scene.rig.server.world().grids();
    Reading {
        chain_speed: centre_of(scene, ships).1,
        car_speed: grids.get(car).expect("the car").linear_velocity,
        on_a_rail: [
            grids.get(ships[0]).expect("the anchor").rail.is_some(),
            grids.get(car).expect("the car").rail.is_some(),
        ],
        gap: pivot_gap(scene, (ships[2], 3.0), car),
    }
}

/// What is read after each of `ticks` ticks that the cars are followed for.
fn followed_for(chain: Chain, with_joints: bool, ticks: usize) -> Vec<Reading> {
    let (mut scene, _, ships, car) = a_chain_and_a_car(chain, with_joints);
    (0..ticks)
        .map(|_| {
            scene.ticks(1);
            read(&scene, &ships, car)
        })
        .collect()
}

/// The impulse (mass times the change of speed) that the pull had given the chain and the car by the end of each tick, from the first in which either has a newton second: what the pull
/// did to the speed of the chain's centre of mass and to the car's (less the same in the run of plain stone), times the mass.
fn impulses_given_by_the_pull(chain: Chain, ticks: usize) -> Vec<[Vec3; 2]> {
    let (scene, _, ships, car) = a_chain_and_a_car(chain, true);
    let masses = [mass_of(&scene, &ships), mass_of(&scene, &[car])];
    drop(scene);
    let (with, without) = (
        followed_for(chain, true, ticks),
        followed_for(chain, false, ticks),
    );
    let mut impulses = Vec::new();
    for (with, without) in with.iter().zip(&without) {
        let given = [
            [0, 1, 2].map(|axis| (with.chain_speed[axis] - without.chain_speed[axis]) * masses[0]),
            [0, 1, 2].map(|axis| (with.car_speed[axis] - without.car_speed[axis]) * masses[1]),
        ];
        if given.iter().flatten().any(|part| part.abs() > 1.0) || !impulses.is_empty() {
            impulses.push(given);
        }
    }
    impulses
}

#[test]
fn each_tick_the_pull_of_a_joint_end_of_a_chain_on_a_rail_gives_the_chain_and_the_car_the_same_impulse_the_opposite_way()
 {
    for chain in [Chain::OnARail, Chain::AndTheCarOnOne] {
        let given = impulses_given_by_the_pull(chain, 30);
        assert!(
            given.len() >= 15,
            "{chain:?}: the chain and the car were pulled for a while: {} ticks",
            given.len()
        );
        // **A tick apart, for a chain on a rail and a car that is free** (see `joints_on_rails.rs`): the rail is made whole in the tick that the pull is asked in, and the bodies give
        // the force to a car that is free in the next.
        let skew = usize::from(chain == Chain::OnARail);
        for tick in 0..given.len() - skew {
            let (chain_has, car_has) = (given[tick][0], given[tick + skew][1]);
            assert!(
                length(chain_has) > 1.0,
                "{chain:?}, tick {tick}: the chain was pulled: {chain_has:?}"
            );
            let sum = [0, 1, 2].map(|axis| chain_has[axis] + car_has[axis]);
            assert!(
                length(sum) <= 1.0e-3 * length(chain_has),
                "{chain:?}, tick {tick}: the impulses {chain_has:?} and {car_has:?} are not opposite"
            );
        }
    }
}

/// How the run went until the car was coupled to the chain.
struct Run {
    /// The tick (from the first after the car was made) in which the chain was found off its rail, if it was.
    left_at: Option<usize>,
    /// The tick the bodies reported the car coupled in (the chain's two couplings and one more), if they did.
    coupled_at: Option<usize>,
    gap_at_the_touch: f64,
    car_rode_until_the_touch: bool,
    scene: Scene,
    report: Box<dyn Fn() -> grid_bodies::BodiesReport>,
    ships: Vec<GridId>,
    car: GridId,
}

fn until_the_car_is_coupled(chain: Chain) -> Run {
    let (mut scene, report, ships, car) = a_chain_and_a_car(chain, true);
    let (mut left_at, mut coupled_at) = (None, None);
    let mut gap_at_the_touch = f64::NAN;
    let mut car_rode_until_the_touch = true;
    for tick in 0..400 {
        scene.ticks(1);
        let reading = read(&scene, &ships, car);
        if left_at.is_none() {
            if reading.on_a_rail[0] {
                car_rode_until_the_touch &=
                    reading.on_a_rail[1] == (chain == Chain::AndTheCarOnOne);
            } else {
                left_at = Some(tick);
                gap_at_the_touch = reading.gap;
            }
        }
        if report().couplings > 2 {
            coupled_at = Some(tick);
            break;
        }
    }
    Run {
        left_at,
        coupled_at,
        gap_at_the_touch,
        car_rode_until_the_touch,
        scene,
        report: Box::new(report),
        ships,
        car,
    }
}

#[test]
fn a_chain_on_a_rail_stays_on_it_while_its_joint_end_pulls_and_comes_off_it_at_the_coupling_and_so_does_the_car_that_is_on_one()
 {
    for chain in [Chain::OnARail, Chain::AndTheCarOnOne] {
        let run = until_the_car_is_coupled(chain);
        let left_at = run.left_at.unwrap_or_else(|| {
            panic!(
                "{chain:?}: the chain never left its rail; the car was coupled at {:?}",
                run.coupled_at
            )
        });
        // The old rule took the chain off its rail in the first tick of the pull, 6 voxels from the car.
        assert!(
            left_at >= 15,
            "{chain:?}: the chain rode its rail until tick {left_at} (the pull went on for a while)"
        );
        assert!(
            run.gap_at_the_touch <= 0.3,
            "{chain:?}: the pivots were {} voxels apart when the rail came off",
            run.gap_at_the_touch
        );
        assert!(
            run.car_rode_until_the_touch,
            "{chain:?}: the car was on a rail exactly when it was meant to be"
        );
        let coupled_at = run
            .coupled_at
            .unwrap_or_else(|| panic!("{chain:?}: the car was never coupled"));
        // (Measured 2026-11-19: the rail came off in tick 49 with the pivots 0.15 voxels apart, and the bodies coupled the car in the next; the free chain was coupled in 54.)
        assert_eq!(
            coupled_at,
            left_at + 1,
            "{chain:?}: the rail came off in tick {left_at} and the car was coupled in {coupled_at}"
        );
        let report = (run.report)();
        assert_eq!(
            report.rails_left_for_ends,
            if chain == Chain::AndTheCarOnOne { 2 } else { 1 },
            "{chain:?}: a rail was left for the coupling by each of the chain and the car that rode one, and not before"
        );
        assert_eq!(report.rail_kicks, 0, "{chain:?}: no hit was made");
        // And they are coupled for good: the chain and the car are one group, and the joint holds.
        let mut scene = run.scene;
        scene.ticks(20);
        let gap = pivot_gap(&scene, (run.ships[2], 3.0), run.car);
        assert!(gap < 0.05, "{chain:?}: the joint holds: {gap} voxels");
        assert_eq!((run.report)().couplings, 3, "{chain:?}: three couplings");
    }
}

#[test]
fn a_chain_on_a_rail_is_coupled_to_a_car_in_about_the_time_the_free_chain_is() {
    let free = until_the_car_is_coupled(Chain::Free)
        .coupled_at
        .expect("the free chain was coupled to the car");
    for chain in [Chain::OnARail, Chain::AndTheCarOnOne] {
        let on_a_rail = until_the_car_is_coupled(chain)
            .coupled_at
            .unwrap_or_else(|| panic!("{chain:?}: the car was never coupled"));
        assert!(
            on_a_rail.abs_diff(free) <= 4,
            "{chain:?}: coupled in tick {on_a_rail}, the free chain in {free}"
        );
    }
}
