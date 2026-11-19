//! Ports that pull on each other when a ship that has one rides a rail (step 4c-5-4b of `docs/decisions/joints-and-cars.md`): **the pull is a force on the rail and a torque on the
//! bodies, so it never takes a ship off its rail; only the dock, which is a touch, does** (the owner's promise of 2026-11-09).
//!
//! Before this, a ship alone on a rail that had another ship near it was taken off its rail for any port that pulled on its own (`take_railed_groups_off_for_ends_that_pull`), because
//! the bodies pull ships of one frame and a ship on a rail has a frame of its own (or none). Now the pairs are found over the railed ships too, both ends are measured at one instant
//! (a ship on a rail is a tick ahead of a free one while the bodies are about to step), each end gets its force (a rail's, by the world, or a body's) and the dock is made after the
//! step, when every ship is at the place of the end of the tick.
//!
//! Each case is run with the ships riding a rail and, as its oracle, with the same two ships free, which the bodies have always pulled and docked. **What a pull does is measured as a
//! difference of two runs** (the ships with ports, and the same ships of plain stone): what a rail does to a ship in a tick that a body does not (a tick of the orbit's turn in the
//! speed stored) is in both and is taken out.

mod common;

use common::chains::{mass_of, origins_of};
use common::docking::{a_pair, make, make_going, stones};
use common::rails::{TICKS_PER_SECOND, put_on_its_rail_in};
use common::{Scene, circular_speed, length, pose_at, ship_id};
use server::ServerWorld;
use voxel_space::{GridId, GridPose};
use world_storage::rail::{Rail, RailTime};

const DISTANCE: f64 = 70_016.0;

/// Which of the two ships ride a rail while they pull.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Riding {
    /// Neither: both are free, as they always were (the oracle).
    Neither,
    /// The first (the one with the port on its east face) rode a rail before the second came, and the second is free.
    First,
    /// Both: the second is put on a rail in the tick after it comes, as near the first as it is.
    Both,
}

/// How the second ship comes into the range of the pull of the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arrival {
    /// It is made `GAP` voxels from the first and closes on it.
    Made,
    /// It is made `FAR` voxels beyond that, and moved to `MOVED_GAP` voxels from the first (inside the range of the pull; as a command or an edit moves a ship) in the first of the ticks
    /// that the ships are followed for, so that it is pulled in the very step that it comes: the ship on a rail has a frame of its own only when a stranger was near it in the tick
    /// before (`has_a_stranger_near_on_its_rail`), and has none then.
    Moved,
}

/// How far beyond `GAP` a ship that is to be moved in is made, in voxels: out of the room of a stranger that a ship on a rail has a frame for (a margin of 64 voxels and a second of the
/// ships' speed, 761 voxels here: a ship 300 voxels off gave it its frame).
const FAR: f64 = 3000.0;

/// How far a ship that is moved in lands from the first ship's port, in voxels: within the range of a pull (6), a little less than a pair that was made comes in from.
const MOVED_GAP: f64 = 5.5;

type Vec3 = [f64; 3];

/// How far the second ship's port is from the first's when it is made, in voxels (the range of a pull is 6), and how fast the second ship goes toward the first, in voxels a second: it
/// comes into the range of the pull as ships do, from outside it. (A ship that is made within the range of a pull is not pulled in the step that makes it, because the bodies have not
/// seen it yet; one that comes in from outside is pulled as soon as it is within, whichever the ships ride.)
const GAP: f64 = 7.0;
const CLOSING: f64 = 2.0;

/// The first ship, 4 voxels of stone and port along x, in orbit at `DISTANCE` going along z at the speed of a circle, and the second, 3 voxels, made `GAP` voxels from its port going as
/// it goes and `CLOSING` voxels a second toward it; `with_ports` false makes both of plain stone of the same size, which nothing pulls. The scene, what the bodies report, and the two
/// ships.
fn a_pair_in_orbit(
    riding: Riding,
    with_ports: bool,
) -> (
    Scene,
    impl Fn() -> grid_bodies::BodiesReport,
    GridId,
    GridId,
) {
    a_pair_arriving(riding, with_ports, Arrival::Made)
}

/// As [`a_pair_in_orbit`], the second ship coming as `arrival` says (`Moved` has to be moved in by [`followed_for_a_while`]).
fn a_pair_arriving(
    riding: Riding,
    with_ports: bool,
    arrival: Arrival,
) -> (
    Scene,
    impl Fn() -> grid_bodies::BodiesReport,
    GridId,
    GridId,
) {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    let (first, second) = if with_ports {
        a_pair()
    } else {
        (stones(0, 4), stones(0, 3))
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
    let far = if arrival == Arrival::Moved { FAR } else { 0.0 };
    make_going(
        &mut scene,
        pose_at(origin[0] + 4.0 + GAP + far, origin[1], origin[2]),
        second,
        [-CLOSING, 0.0, speed],
    );
    if arrival == Arrival::Made {
        // (The same number of ticks for every case: the second ship is given its rail in this one, or nothing is done.)
        scene.run(move |world| {
            if riding == Riding::Both {
                put_on_a_rail_near_another_ship(world, b);
            }
        });
    } else {
        assert_ne!(riding, Riding::Both, "a ship that is moved in is free");
    }
    (scene, report, a, b)
}

/// Puts a ship on the orbit that it is on, though another ship is near it: the rules of a rail (`coasting_rail`) refuse a ship with a stranger near it, and a ship on a rail has
/// one near it all the same now (the stranger is the contact ghost's), so this is how a test makes the second of two ships near each other ride one. **The orbit is of the time of the
/// ship's state, which is a tick before the world's clock**: a ship that is free is where the last tick left it until the bodies step it, and a rail made for the clock's time would put
/// it a tick of its speed (38 voxels) behind the ship on a rail that it is beside, for good.
fn put_on_a_rail_near_another_ship(world: &mut ServerWorld, ship: GridId) {
    let time = RailTime::of_tick(world.data().clock() - 1, TICKS_PER_SECOND);
    let orbit = world
        .flight_state(ship, time)
        .and_then(|state| state.orbit.ok())
        .expect("the ship is in orbit");
    world
        .set_grid_rail(
            ship,
            Some(Rail {
                spin: None,
                orbit: Some(orbit),
            }),
        )
        .unwrap();
}

fn on_a_rail(scene: &Scene, ship: GridId) -> bool {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship)
        .unwrap()
        .rail
        .is_some()
}

fn minus(first: Vec3, second: Vec3) -> Vec3 {
    [0, 1, 2].map(|axis| first[axis] - second[axis])
}

/// How many ticks the ships are followed for, their places and speeds read after each: the pull begins in the tenth, and the ships dock after the eighteenth.
const TICKS_FOLLOWED: usize = 18;

/// What is read of the two ships after a tick.
#[derive(Clone, Copy, Debug)]
struct Reading {
    places: [Vec3; 2],
    speeds: [Vec3; 2],
    spins: [Vec3; 2],
    on_a_rail: [bool; 2],
}

fn readings_of(scene: &Scene, ships: [GridId; 2]) -> Reading {
    let grids = scene.rig.server.world().grids();
    let ships = ships.map(|ship| grids.get(ship).expect("the ship is there"));
    Reading {
        places: ships.map(|ship| ship.pose.position.in_voxels()),
        speeds: ships.map(|ship| ship.linear_velocity),
        spins: ships.map(|ship| ship.angular_velocity),
        on_a_rail: ships.map(|ship| ship.rail.is_some()),
    }
}

/// What is read of the two ships after each of the ticks that they are followed for; they do not dock in them.
fn followed_for_a_while(riding: Riding, with_ports: bool, arrival: Arrival) -> Vec<Reading> {
    let (mut scene, report, a, b) = a_pair_arriving(riding, with_ports, arrival);
    let mut seen = Vec::new();
    for tick in 0..TICKS_FOLLOWED {
        if arrival == Arrival::Moved && tick == 0 {
            scene.run(move |world| move_in(world, b));
        } else {
            scene.ticks(1);
        }
        assert_eq!(report().dockings_made, 0, "the ships did not dock yet");
        seen.push(readings_of(&scene, [a, b]));
    }
    seen
}

/// Moves a ship made `FAR` voxels beyond the gap that ships are made at to `MOVED_GAP` from the first ship's port (the same turn and speed).
fn move_in(world: &mut ServerWorld, ship: GridId) {
    let pose = world.data().grids().get(ship).expect("the ship").pose;
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

/// What the pull did to each ship in each tick: where it is and how fast it goes less where the same ship is, and how fast, in the run of plain stone: (places, speeds) of both.
fn what_the_pull_did(riding: Riding, arrival: Arrival) -> Vec<([Vec3; 2], [Vec3; 2])> {
    let with = followed_for_a_while(riding, true, arrival);
    let without = followed_for_a_while(riding, false, arrival);
    with.iter()
        .zip(&without)
        .map(|(with, without)| {
            (
                [0, 1].map(|ship| minus(with.places[ship], without.places[ship])),
                [0, 1].map(|ship| minus(with.speeds[ship], without.speeds[ship])),
            )
        })
        .collect()
}

/// The impulse (mass times the change of speed) that the pull gave each ship in each tick that it pulled: the change in the effect of the pull on the speed from the tick before, times
/// the ship's mass. The first of the ticks, in which the pull begins, is the first that has any.
fn impulses_of_the_pull(riding: Riding, arrival: Arrival) -> Vec<[Vec3; 2]> {
    let (scene, _, a, b) = a_pair_arriving(riding, true, arrival);
    let masses = [a, b].map(|ship| mass_of(&scene, &[ship]));
    drop(scene);
    let effect = what_the_pull_did(riding, arrival);
    let mut impulses = Vec::new();
    let mut before = [[0.0; 3]; 2];
    for (_, speeds) in &effect {
        let change = [0, 1].map(|ship| minus(speeds[ship], before[ship]));
        if change.iter().flatten().any(|part| *part != 0.0) || !impulses.is_empty() {
            impulses.push([0, 1].map(|ship| change[ship].map(|part| part * masses[ship])));
        }
        before = *speeds;
    }
    impulses
}

/// What a pull of a tick is, in newton seconds: the force of a port (10,000 newtons toward the other) for a twentieth of a second.
fn impulse_of_a_tick() -> f64 {
    grid_bodies::DockingSettings::default().attraction_force / f64::from(TICKS_PER_SECOND)
}

/// Asserts that in each tick that the ports pulled, in the run that `riding` and `arrival` say, each ship was given the impulse of one tick of the pull, the opposite way to the other.
fn assert_each_tick_of_the_pull_is_the_same_impulse_the_opposite_way(
    riding: Riding,
    arrival: Arrival,
) {
    let impulses = impulses_of_the_pull(riding, arrival);
    assert!(
        impulses.len() >= 6,
        "{riding:?}, {arrival:?}: the ships were pulled for a while: {} ticks",
        impulses.len()
    );
    let wanted = impulse_of_a_tick();
    for (tick, [first, second]) in impulses.iter().enumerate() {
        let size = |vector: &Vec3| length(*vector);
        // (Measured, 2026-11-19: the size is 500 to 4.3e-3 in every case, and the two are opposite to 7e-6; what is left is the rounding of taking the run of plain stone from the run
        // of ports. The change of the speed of a ship on a rail is exact: the orbit is made again through the speed with the change added.)
        assert!(
            (size(first) - wanted).abs() <= 0.02 && (size(second) - wanted).abs() <= 0.02,
            "{riding:?}, {arrival:?}, tick {tick}: the impulses are {} and {}, not {wanted}",
            size(first),
            size(second)
        );
        let sum = [0, 1, 2].map(|axis| first[axis] + second[axis]);
        assert!(
            length(sum) <= 1.0e-4,
            "{riding:?}, {arrival:?}, tick {tick}: the impulses {first:?} and {second:?} are not opposite"
        );
    }
}

#[test]
fn each_tick_the_pull_of_the_ports_gives_each_ship_the_same_impulse_the_opposite_way_whether_it_rides_a_rail_or_is_free()
 {
    for riding in [Riding::Neither, Riding::First, Riding::Both] {
        assert_each_tick_of_the_pull_is_the_same_impulse_the_opposite_way(riding, Arrival::Made);
    }
}

#[test]
fn a_ship_that_is_moved_into_the_range_of_a_ship_on_a_rail_pulls_it_and_is_pulled_from_the_step_it_comes_in_once()
 {
    // The ship on a rail has no frame of its own in that step (a ship is near it only from then), so it is a fixed body to the bodies: the pull of its port is the rail's all the same,
    // and the ship that came is pulled by the one pair of ports (the pairs of the ships of a frame, which would have it pulled again, leave a ship that rides a rail alone to these).
    for riding in [Riding::Neither, Riding::First] {
        assert_each_tick_of_the_pull_is_the_same_impulse_the_opposite_way(riding, Arrival::Moved);
    }
}

#[test]
fn a_ship_on_a_rail_whose_port_pulls_stays_on_the_rail_while_it_pulls_and_so_does_the_second_that_is_on_one()
 {
    for riding in [Riding::First, Riding::Both] {
        let seen = followed_for_a_while(riding, true, Arrival::Made);
        for (tick, reading) in seen.iter().enumerate() {
            assert!(
                reading.on_a_rail[0],
                "{riding:?}: the first ship is on its rail in tick {tick}"
            );
            assert_eq!(
                reading.on_a_rail[1],
                riding == Riding::Both,
                "{riding:?}: the second ship's rail in tick {tick}"
            );
        }
    }
}

/// Runs the pair with ports until it docks (at most 200 ticks), and the tick it did in, if it did; in each tick before it each ship that rode a rail is on it.
fn until_they_dock(
    riding: Riding,
) -> (Scene, impl Fn() -> grid_bodies::BodiesReport, Option<usize>) {
    let (mut scene, report, a, b) = a_pair_in_orbit(riding, true);
    let mut docked_at = None;
    for tick in 0..200 {
        scene.ticks(1);
        if report().dockings_made > 0 {
            docked_at = Some(tick);
            break;
        }
        assert!(
            on_a_rail(&scene, a),
            "{riding:?}: the first ship is on its rail in tick {tick}, while its port pulls"
        );
        assert_eq!(
            on_a_rail(&scene, b),
            riding == Riding::Both,
            "{riding:?}: the second ship's rail in tick {tick}"
        );
    }
    (scene, report, docked_at)
}

#[test]
fn ships_on_rails_whose_ports_pull_dock_and_the_ship_they_make_goes_on_in_the_orbit() {
    for riding in [Riding::First, Riding::Both] {
        let speed = {
            let (scene, _, _, _) = a_pair_in_orbit(riding, true);
            circular_speed(&scene, 0, DISTANCE)
        };
        let (mut scene, report, docked_at) = until_they_dock(riding);
        assert!(docked_at.is_some(), "{riding:?}: the two ships docked");
        assert_eq!(
            report().rails_left_for_ends,
            0,
            "{riding:?}: no rail was left for the ends that pull"
        );
        assert_eq!(report().rail_kicks, 0, "{riding:?}: and no hit was made");
        // The ship they make has the speed of the orbit (the docking reads both ships' speeds; where it cannot it gives the ship none, and the ship falls).
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
        let along = merged.1[2];
        assert!(
            (along - speed).abs() < 5.0,
            "{riding:?}: the docked ship goes at the speed of its orbit, {speed}: {merged:?}"
        );
    }
}

/// The turn about z that the pull gave each ship (the ports are on x, a little apart in y: the twist and the lever turn the ships about z) in the first four ticks of the pull, in the
/// run with ports (the run of plain stone has none): from the first tick that either ship turns in.
fn turns_given_by_the_pull(riding: Riding) -> Vec<[f64; 2]> {
    let (mut scene, _, a, b) = a_pair_in_orbit(riding, true);
    let mut turns = Vec::new();
    for _ in 0..TICKS_FOLLOWED {
        scene.ticks(1);
        let turn = readings_of(&scene, [a, b]).spins.map(|spin| spin[2]);
        if turn != [0.0; 2] || !turns.is_empty() {
            turns.push(turn);
        }
    }
    turns.truncate(4);
    turns
}

#[test]
fn the_lever_and_the_twist_of_the_pull_turn_a_ship_on_a_rail_about_as_they_turn_a_free_one() {
    // **About**, because the places the ships begin the pull from are not the same: a free ship falls by the solver's steps, a ship on a rail by its orbit, and in the ticks before
    // the pull they are tenths of a voxel apart in height, which is what the lever and the twist depend on (the spins of the ships on rails are 0.7 to 0.75 of the free ones, measured
    // 2026-11-19). What this is for is a torque that does not reach the bodies of a ship on a rail, or the wrong way: that is none, or the opposite spin.
    let free = turns_given_by_the_pull(Riding::Neither);
    assert!(
        free.len() == 4 && free.iter().all(|turns| turns[0] > 1e-3 && turns[1] > 1e-3),
        "the free ships turn: {free:?}"
    );
    for riding in [Riding::First, Riding::Both] {
        let turns = turns_given_by_the_pull(riding);
        assert_eq!(
            turns.len(),
            4,
            "{riding:?}: the ships turned from the first tick of the pull: {turns:?}"
        );
        for (tick, (turn, free)) in turns.iter().zip(&free).enumerate() {
            for ship in 0..2 {
                let share = turn[ship] / free[ship];
                assert!(
                    (0.5..=1.5).contains(&share),
                    "{riding:?}, tick {tick}: ship {ship} turned {} about z, {share} of what the free one did ({})",
                    turn[ship],
                    free[ship]
                );
            }
        }
    }
}
