//! A docking port of a car of a chain that rides a rail pulls on the port of a ship (step 4c-5-4b of `docs/decisions/joints-and-cars.md`, for groups): **the pull is a force on the
//! group's rail and a torque on the bodies, so it never takes the group off its rail; only the dock, which is a touch, does.** This is `ports_on_rails.rs` for a group.
//!
//! The chain is the three cars of `a_chain` with a port on the east end of the last, and the ship is the one with a port on its west face that `a_pair` has, made `GAP` voxels beyond
//! it and closing on it. Each case is run with the chain on a rail and, as its oracle, free (the bodies have always pulled and docked a group that is free), each as ports and as plain
//! stone, so that what a pull does is a difference of two runs: what a rail does to a ship in a tick that a body does not is in both and is taken out.
//!
//! What a pull does to a group is its **centre of mass**: the impulse on the group is its mass times the change of the speed of its centre of mass, which a rail has exact.

mod common;

use common::chains::{
    Vec3, a_chain_in_orbit, car, centre_of, mass_of, minus, origins_of, put_on_its_rail, set_going,
};
use common::docking::{PLAIN, a_pair, facing, make_going, port_block, stone, stones};
use common::rails::put_on_a_rail_near_another_ship;
use common::{Scene, circular_speed, length, pose_at, ship_id};
use voxel_space::{FaceDirection, GridId};

const DISTANCE: f64 = 70_016.0;

/// How far the ship's port is from the chain's end when it is made, in voxels (the range of a pull is 6), and how fast it closes on it, in voxels a second: it comes into the range
/// as ships do, from outside it.
const GAP: f64 = 7.0;
const CLOSING: f64 = 2.0;

/// The length of the chain, along x: the cars are 3, 4 and 3 voxels long (the last has the port on its east end).
const LENGTH: f64 = 10.0;

/// How many ticks are followed for, read after each: the pull begins in the tenth, and the ships dock after the eighteenth.
const TICKS_FOLLOWED: usize = 18;

/// Whether the chain rides a rail while the ports pull.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Chain {
    /// Free, as a group always was (the oracle).
    Free,
    /// On the rail of its centre of mass, put on it before the ship came.
    OnARail,
    /// On its rail, and the ship is put on one in the tick after it comes, as near the chain as it is.
    AndTheShipOnOne,
}

/// The chain (three cars, numbers 1 to 3, coupled by their joints, standing still against each other) in orbit at `DISTANCE` going along z at the speed of a circle, with a port on
/// the east end of the last car (or a stone of the same weight, if `with_ports` is false), and the ship (number 4, 3 voxels long: a port and two stones, or three stones) made `GAP`
/// voxels beyond the chain's end, going as it goes and `CLOSING` voxels a second toward it. The scene, what the bodies report, the cars and the ship.
fn a_chain_and_a_ship(
    chain: Chain,
    with_ports: bool,
) -> (
    Scene,
    impl Fn() -> grid_bodies::BodiesReport,
    Vec<GridId>,
    GridId,
) {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    // Three light cars (3, 4 and 3 voxels long): a heavy car on a ball joint turns too slowly into line to dock before it touches (the free chain does not dock either).
    let mut cars = vec![car(false, 2, true), car(true, 2, true), car(true, 1, false)];
    let (block, look) = if with_ports {
        (port_block(), facing(FaceDirection::PositiveX))
    } else {
        (stone(), PLAIN)
    };
    cars[2].0.push(([2, 0, 0], block, look));
    cars[2].1 += 1.0;
    let ships = a_chain_in_orbit(&mut scene, cars, DISTANCE, speed);
    let still = vec![([0.0; 3], [0.0; 3]); 3];
    match chain {
        Chain::OnARail | Chain::AndTheShipOnOne => {
            put_on_its_rail(&mut scene, &ships, speed, still);
        }
        Chain::Free => set_going(&mut scene, &ships, speed, still),
    }
    scene.ticks(3);
    let second = if with_ports { a_pair().1 } else { stones(0, 3) };
    let origin = origins_of(&scene, &ships[..1])[0];
    make_going(
        &mut scene,
        pose_at(origin[0] + LENGTH + GAP, origin[1], origin[2]),
        second,
        [-CLOSING, 0.0, speed],
    );
    // (The same number of ticks for every case: the ship is given its rail in this one, or nothing is done.)
    let ship = ship_id(4);
    scene.run(move |world| {
        if chain == Chain::AndTheShipOnOne {
            put_on_a_rail_near_another_ship(world, ship);
        }
    });
    (scene, report, ships, ship)
}

/// What is read of the chain and the ship after a tick: how fast the centre of mass of the chain goes and how fast the ship goes, and whether the chain's anchor rides a rail.
#[derive(Clone, Copy, Debug)]
struct Reading {
    chain_speed: Vec3,
    ship_speed: Vec3,
    on_a_rail: bool,
}

fn read(scene: &Scene, ships: &[GridId], ship: GridId) -> Reading {
    let grids = scene.rig.server.world().grids();
    Reading {
        chain_speed: centre_of(scene, ships).1,
        ship_speed: grids.get(ship).expect("the ship").linear_velocity,
        on_a_rail: grids.get(ships[0]).expect("the anchor").rail.is_some(),
    }
}

/// What is read after each of the ticks that the ships are followed for; they do not dock in them.
fn followed_for_a_while(chain: Chain, with_ports: bool) -> Vec<Reading> {
    let (mut scene, report, ships, ship) = a_chain_and_a_ship(chain, with_ports);
    let mut seen = Vec::new();
    for _ in 0..TICKS_FOLLOWED {
        scene.ticks(1);
        assert_eq!(report().dockings_made, 0, "the ships did not dock yet");
        seen.push(read(&scene, &ships, ship));
    }
    seen
}

/// The impulse (mass times the change of speed) that the pull gave the chain and the ship in each tick that it pulled: the change of what it did to the speed from the tick before,
/// times the mass; from the first tick in which either had any.
fn impulses_of_the_pull(chain: Chain) -> Vec<[Vec3; 2]> {
    let (scene, _, ships, ship) = a_chain_and_a_ship(chain, true);
    let masses = [mass_of(&scene, &ships), mass_of(&scene, &[ship])];
    drop(scene);
    let (with, without) = (
        followed_for_a_while(chain, true),
        followed_for_a_while(chain, false),
    );
    let mut impulses = Vec::new();
    let mut before = [[0.0; 3]; 2];
    for (with, without) in with.iter().zip(&without) {
        let effect = [
            minus(with.chain_speed, without.chain_speed),
            minus(with.ship_speed, without.ship_speed),
        ];
        let change = [0, 1].map(|index| minus(effect[index], before[index]));
        if change.iter().flatten().any(|part| *part != 0.0) || !impulses.is_empty() {
            impulses.push([0, 1].map(|index| change[index].map(|part| part * masses[index])));
        }
        before = effect;
    }
    impulses
}

/// What a pull of a tick is, in newton seconds: the force of a port (10,000 newtons toward the other) for a twentieth of a second.
fn impulse_of_a_tick() -> f64 {
    grid_bodies::DockingSettings::default().attraction_force / 20.0
}

#[test]
fn each_tick_the_pull_gives_the_chain_and_the_ship_the_same_impulse_the_opposite_way_whether_the_chain_rides_a_rail_or_is_free()
 {
    for chain in [Chain::Free, Chain::OnARail, Chain::AndTheShipOnOne] {
        let impulses = impulses_of_the_pull(chain);
        assert!(
            impulses.len() >= 6,
            "{chain:?}: the ships were pulled for a while: {} ticks",
            impulses.len()
        );
        let wanted = impulse_of_a_tick();
        // (Measured, 2026-11-19: on a rail the impulse is 500 to 3.4e-3 and the two are opposite to 6e-6, as for a ship alone. Free, the size is as good (5.1e-3) and the two are opposite
        // to 0.76 only: a free chain whose port pulls is taken out of the frame of its own into the one that the ship is in, which integrates the pull of the planet on the chain a little
        // otherwise, and that is in the difference of the runs. It is the oracle's error and not the rail's.)
        let opposite = match chain {
            Chain::Free => 2.0,
            Chain::OnARail | Chain::AndTheShipOnOne => 1.0e-4,
        };
        for (tick, [on_the_chain, on_the_ship]) in impulses.iter().enumerate() {
            assert!(
                (length(*on_the_chain) - wanted).abs() <= 0.02
                    && (length(*on_the_ship) - wanted).abs() <= 0.02,
                "{chain:?}, tick {tick}: the impulses are {} and {}, not {wanted}",
                length(*on_the_chain),
                length(*on_the_ship)
            );
            let sum = [0, 1, 2].map(|axis| on_the_chain[axis] + on_the_ship[axis]);
            assert!(
                length(sum) <= opposite,
                "{chain:?}, tick {tick}: the impulses {on_the_chain:?} and {on_the_ship:?} are not opposite"
            );
        }
    }
}

/// Runs the chain and the ship, with ports, until they dock (at most 200 ticks), and the tick they did in, if they did; in each tick before it the chain's anchor is on its rail if
/// the chain rides one.
fn until_they_dock(chain: Chain) -> (Scene, impl Fn() -> grid_bodies::BodiesReport, Option<usize>) {
    let (mut scene, report, ships, ship) = a_chain_and_a_ship(chain, true);
    let mut docked_at = None;
    for tick in 0..200 {
        scene.ticks(1);
        if report().dockings_made > 0 {
            docked_at = Some(tick);
            break;
        }
        let reading = read(&scene, &ships, ship);
        assert_eq!(
            reading.on_a_rail,
            chain != Chain::Free,
            "{chain:?}: the chain's rail in tick {tick}, while its port pulls"
        );
        assert_eq!(
            scene
                .rig
                .server
                .world()
                .grids()
                .get(ship)
                .unwrap()
                .rail
                .is_some(),
            chain == Chain::AndTheShipOnOne,
            "{chain:?}: the ship's rail in tick {tick}"
        );
        assert_eq!(report().couplings, 2, "{chain:?}: and it is a chain");
    }
    (scene, report, docked_at)
}

#[test]
fn a_chain_on_a_rail_whose_port_pulls_stays_on_it_until_the_dock_and_the_ship_they_make_goes_on_in_the_orbit()
 {
    for chain in [Chain::OnARail, Chain::AndTheShipOnOne] {
        let speed = {
            let scene = Scene::in_a_new_world();
            circular_speed(&scene, 0, DISTANCE)
        };
        let (mut scene, report, docked_at) = until_they_dock(chain);
        assert!(
            docked_at.is_some(),
            "{chain:?}: the chain and the ship docked"
        );
        assert_eq!(
            report().rails_left_for_ends,
            0,
            "{chain:?}: no rail was left for the ends that pull"
        );
        assert_eq!(report().rail_kicks, 0, "{chain:?}: and no hit was made");
        // The ship that the last car and the ship make has the speed of the orbit (the docking reads both ships' speeds; where it cannot it gives the ship none, and the ship falls).
        scene.ticks(5);
        let merged = scene
            .rig
            .server
            .world()
            .grids()
            .iter()
            .map(|(id, grid)| (id, grid.linear_velocity))
            .max_by_key(|(id, _)| *id)
            .expect("a ship");
        assert!(
            (merged.1[2] - speed).abs() < 5.0,
            "the docked ship goes at the speed of its orbit, {speed}: {merged:?}"
        );
    }
}
