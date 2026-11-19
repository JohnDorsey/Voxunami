//! Ships coupled by their joint ends ride a rail as a group (step 4c-3 of `docs/decisions/joints-and-cars.md`): one rail, on the ship with the lowest number, the orbit of the centre
//! of mass of the ships; the world puts the group where the rail says without moving the ships against each other; and anything done to any ship of it takes the rail from the
//! anchor. These are the world's rules with no bodies to move anything; what the bodies do with a group on a rail is tested with them (`games/voxunami_cave/tests/group_on_a_rail.rs`).
//!
//! The rails here are made by hand, from the centre of mass and the speed of the group: that a group is put on one by itself is another matter (the rules of `coasting_rail`).

use voxel_space::Orientation;
use world_storage::rail::{Centre, Orbit, Rail, RailTime};
use world_storage::{ParkedGrid, ShipFileName};

use super::coasting_ships::{
    PULL, TICKS, orbit_of, rail_of, server_tick, ship_at, space, ticks_of_the_end,
    until_the_rail_changes,
};
use super::*;

const DISTANCE: f64 = 10_000.0;
/// What the planet of `space` weighs.
const PLANET_MASS: f64 = 1.0e22;

fn time(tick: u64) -> RailTime {
    RailTime::of_tick(tick, TICKS)
}

fn circular_speed() -> f64 {
    libm::sqrt(PULL * PLANET_MASS / DISTANCE)
}

/// The rail of the centre of mass of these ships, made now.
fn rail_of_the_group(world: &ServerWorld, planet: GridId, members: &[GridId]) -> Rail {
    let centre = world.data().grids().group_centre(members).unwrap();
    let place = world
        .data()
        .grids()
        .get(planet)
        .unwrap()
        .pose
        .position
        .displacement_to(centre.position)
        .in_voxels();
    Rail {
        spin: None,
        orbit: Some(
            Orbit::from_place(
                Centre::Grid(planet),
                place,
                centre.velocity,
                PULL * PLANET_MASS,
                time(world.data().clock()),
            )
            .unwrap(),
        ),
    }
}

/// Three ships in a row along z, 20 voxels apart, going round the planet, coupled one to the next, each weighed differently and each moving a little against the others and
/// turning, with the rail of their centre of mass on the first. The planet, the ships and the rail.
fn a_group_on_a_rail() -> (ServerWorld, GridId, [GridId; 3], Rail) {
    a_group_on_a_rail_weighing([1000.0, 3000.0, 2000.0])
}

/// As [`a_group_on_a_rail`], the ships weighing what is said.
fn a_group_on_a_rail_weighing(masses: [f64; 3]) -> (ServerWorld, GridId, [GridId; 3], Rail) {
    let (mut world, planet) = space();
    let speed = circular_speed();
    let ships = [0.0, 20.0, 40.0]
        .map(|along| ship_at(&mut world, [DISTANCE, 0.0, along], [0.0, 0.0, speed]));
    let weights = [
        (masses[0], [2.0, 0.0, 0.0]),
        (masses[1], [0.0, 1.0, 0.0]),
        (masses[2], [1.0, 1.0, 1.0]),
    ];
    for (ship, (mass, middle)) in ships.iter().zip(weights) {
        world.set_grid_mass(*ship, mass, middle).unwrap();
    }
    // Each is moving against the others (the second up, the third back) and turning (the bodies turn them), and the second is faced another way.
    let motion = [
        ([0.0, 0.0, speed], [0.0, 0.1, 0.0]),
        ([0.0, 0.5, speed], [0.2, 0.0, 0.0]),
        ([-0.25, 0.0, speed], [0.0, 0.0, -0.3]),
    ];
    for (ship, (linear, angular)) in ships.iter().zip(motion) {
        world.set_grid_velocity(*ship, linear, angular).unwrap();
    }
    let position = world.data().grids().get(ships[1]).unwrap().pose.position;
    world
        .set_grid_pose(
            ships[1],
            GridPose::new(position, Orientation::from_yaw_pitch(0.7, 0.2)),
        )
        .unwrap();
    world.set_ships_coupled(ships[0], ships[1], true);
    world.set_ships_coupled(ships[1], ships[2], true);
    let rail = rail_of_the_group(&world, planet, &ships);
    world.set_grid_rail(ships[0], Some(rail.clone())).unwrap();
    (world, planet, ships, rail)
}

fn pose_of(world: &ServerWorld, ship: GridId) -> GridPose {
    world.data().grids().get(ship).unwrap().pose
}

fn velocity_of(world: &ServerWorld, ship: GridId) -> [f64; 3] {
    world.data().grids().get(ship).unwrap().linear_velocity
}

fn spin_of(world: &ServerWorld, ship: GridId) -> [f64; 3] {
    world.data().grids().get(ship).unwrap().angular_velocity
}

/// How the second and the third stand against the first, in voxels.
fn standing(world: &ServerWorld, ships: &[GridId; 3]) -> [[f64; 3]; 2] {
    let first = pose_of(world, ships[0]).position;
    [1, 2].map(|index| pose_of(world, ships[index]).position.relative_to(first))
}

/// How fast the second and the third go against the first.
fn going_against(world: &ServerWorld, ships: &[GridId; 3]) -> [[f64; 3]; 2] {
    let first = velocity_of(world, ships[0]);
    [1, 2].map(|index| {
        let other = velocity_of(world, ships[index]);
        [0, 1, 2].map(|axis| other[axis] - first[axis])
    })
}

fn close(found: [f64; 3], expected: [f64; 3], tolerance: f64, what: &str) {
    for axis in 0..3 {
        assert!(
            (found[axis] - expected[axis]).abs() <= tolerance,
            "{what}: {found:?} is not {expected:?}"
        );
    }
}

fn centre_of(world: &ServerWorld, ships: &[GridId; 3]) -> ([f64; 3], [f64; 3]) {
    let centre = world.data().grids().group_centre(ships).unwrap();
    (centre.position.in_voxels(), centre.velocity)
}

#[test]
fn a_group_on_a_rail_is_put_with_its_centre_of_mass_on_the_rail_and_its_ships_where_they_stood_against_each_other()
 {
    let (mut world, planet, ships, rail) = a_group_on_a_rail();
    let before = standing(&world, &ships);
    let moving_before = going_against(&world, &ships);
    let facings: Vec<Orientation> = ships
        .iter()
        .map(|ship| pose_of(&world, *ship).orientation)
        .collect();
    let spins: Vec<[f64; 3]> = ships.iter().map(|ship| spin_of(&world, *ship)).collect();
    let start = centre_of(&world, &ships).0;
    let planet_at = world
        .data()
        .grids()
        .get(planet)
        .unwrap()
        .pose
        .position
        .in_voxels();
    for tick in 1..=200 {
        world.data_mut().advance_clock(1);
        world.apply_rails(TICKS);
        let state = rail.state(time(world.world_tick()), planet_at, [0.0; 3]);
        let (place, speed) = centre_of(&world, &ships);
        close(
            place,
            state.position.unwrap(),
            1e-8,
            &format!("tick {tick}: the centre of mass is the rail's place"),
        );
        close(
            speed,
            state.velocity,
            1e-9,
            &format!("tick {tick}: and goes as the rail does"),
        );
        for (found, expected) in standing(&world, &ships).iter().zip(&before) {
            close(
                *found,
                *expected,
                1e-8,
                &format!("tick {tick}: the ships stand as they did"),
            );
        }
        for (found, expected) in going_against(&world, &ships).iter().zip(&moving_before) {
            close(
                *found,
                *expected,
                1e-9,
                &format!("tick {tick}: and move against each other as they did"),
            );
        }
        for (index, ship) in ships.iter().enumerate() {
            assert_eq!(
                pose_of(&world, *ship).orientation,
                facings[index],
                "tick {tick}: the facing is the bodies'"
            );
            assert_eq!(
                spin_of(&world, *ship),
                spins[index],
                "tick {tick}: and so is the turn"
            );
        }
    }
    // It went round: ten seconds at 8 km/s, and it is not where it began.
    let end = centre_of(&world, &ships).0;
    let apart = [0, 1, 2].map(|axis| end[axis] - start[axis]);
    let travelled = libm::sqrt(apart[0] * apart[0] + apart[1] * apart[1] + apart[2] * apart[2]);
    assert!(travelled > 1000.0, "{travelled}");
}

#[test]
fn a_group_of_which_every_ship_is_frozen_goes_at_the_speed_of_the_rail_and_keeps_its_shape() {
    let (mut world, planet, ships, rail) = a_group_on_a_rail();
    let before = standing(&world, &ships);
    for ship in ships {
        world.set_grid_frozen(ship, true).unwrap();
    }
    let planet_at = world
        .data()
        .grids()
        .get(planet)
        .unwrap()
        .pose
        .position
        .in_voxels();
    for _ in 0..5 {
        world.data_mut().advance_clock(1);
        world.apply_rails(TICKS);
        let state = rail.state(time(world.world_tick()), planet_at, [0.0; 3]);
        for ship in ships {
            // (Not close to it: it. Freezing took the speeds the ships had against each other, and what is left is the rail's.)
            assert_eq!(velocity_of(&world, ship), state.velocity);
            assert_eq!(spin_of(&world, ship), [0.0; 3]);
        }
        for (found, expected) in standing(&world, &ships).iter().zip(&before) {
            close(*found, *expected, 1e-8, "the ships stand as they did");
        }
        close(
            centre_of(&world, &ships).0,
            state.position.unwrap(),
            1e-8,
            "and the group is on the rail",
        );
    }
    // One of them awake, the group is not at rest against itself (it keeps what motion the ships have).
    world.set_grid_frozen(ships[2], false).unwrap();
    world
        .set_grid_velocity(ships[2], [0.0, 0.0, 1.0], [0.0; 3])
        .unwrap();
    assert!(world.data().grids().get(ships[0]).is_some());
}

#[test]
fn a_rail_on_a_ship_of_a_group_that_is_not_its_anchor_is_not_placed_and_is_taken_away() {
    let (mut world, planet, ships, rail) = a_group_on_a_rail();
    // The second ship has a rail of its own, another orbit (made for something else): it is not put by it, and the group is put by the anchor's.
    let other = Rail {
        spin: None,
        orbit: Some(
            Orbit::new(Centre::Grid(planet), [0.0, 1.0, 0.0], 5_000.0, 900.0, 0.3).unwrap(),
        ),
    };
    world.set_grid_rail(ships[1], Some(other)).unwrap();
    let planet_at = world
        .data()
        .grids()
        .get(planet)
        .unwrap()
        .pose
        .position
        .in_voxels();
    let before = standing(&world, &ships);
    world.data_mut().advance_clock(7);
    world.apply_rails(TICKS);
    let state = rail.state(time(world.world_tick()), planet_at, [0.0; 3]);
    close(
        centre_of(&world, &ships).0,
        state.position.unwrap(),
        1e-8,
        "the group is where the anchor's rail puts it",
    );
    for (found, expected) in standing(&world, &ships).iter().zip(&before) {
        close(*found, *expected, 1e-8, "and the ships stand as they did");
    }
    // It is quiet, and the group has nothing near: the anchor keeps its rail, and the other loses the one it has.
    world.update_coasting(TICKS);
    assert_eq!(world.data().grids().get(ships[0]).unwrap().rail, Some(rail));
    assert_eq!(world.data().grids().get(ships[1]).unwrap().rail, None);
}

#[test]
fn the_ships_of_a_group_are_not_in_each_others_way_and_a_ship_near_one_of_them_is_not_either_but_a_planet_is()
 {
    let (mut world, _planet, ships, rail) = a_group_on_a_rail();
    // (The ships are 20 voxels apart: ships that were not coupled would have each other in the way, a margin of 64 voxels and the way they would go in a second.)
    world.update_coasting(TICKS);
    assert_eq!(
        world.data().grids().get(ships[0]).unwrap().rail,
        Some(rail.clone())
    );
    // A stranger far off changes nothing.
    let far = ship_at(
        &mut world,
        [DISTANCE, 5_000.0, 0.0],
        [0.0, 0.0, circular_speed()],
    );
    world.update_coasting(TICKS);
    assert_eq!(
        world.data().grids().get(ships[0]).unwrap().rail,
        Some(rail.clone())
    );
    // One near the last ship of the group does **not** take the rail (the rail's kick, 2026-11-17): what is near a group is the contact ghost's, which the bodies make, and a touch
    // takes the rail (`take_group_off_its_rail`). A ship alone has the room it always had (`coasting_ships.rs`).
    world
        .set_grid_pose(
            far,
            GridPose::new(
                WorldPosition::from_voxels([DISTANCE, 30.0, 60.0]),
                Orientation::IDENTITY,
            ),
        )
        .unwrap();
    world.update_coasting(TICKS);
    assert_eq!(
        world.data().grids().get(ships[0]).unwrap().rail,
        Some(rail.clone()),
        "a stranger 10 voxels from the last ship, going as it goes, leaves the group its rail"
    );
    // Nor does one on a rail of its own, or one that closes on the group fast.
    world
        .set_grid_velocity(far, [0.0, 0.0, 2_000.0], [0.0; 3])
        .unwrap();
    world.update_coasting(TICKS);
    assert_eq!(world.data().grids().get(ships[0]).unwrap().rail, Some(rail));
}

#[test]
fn a_planet_near_a_group_on_a_rail_takes_it_off_as_it_does_a_ship() {
    // The ground of a planet is not the ghost's (a group keeps its own frame over it only until it touches it): a planet that is in the way takes the rail from a group as it
    // always has, and a planet that is not does not.
    let (mut world, _planet, ships, rail) = a_group_on_a_rail();
    let moon = planet_on(&mut world, 20, 1.0e9, None);
    let last = world.data().grids().get(ships[2]).unwrap().pose.position;
    let place = |world: &mut ServerWorld, along: f64| {
        world
            .data_mut()
            .grids_mut()
            .get_mut(moon)
            .unwrap()
            .pose
            .position = last.translated([0.0, 0.0, along]);
    };
    place(&mut world, 50_000.0);
    world.update_coasting(TICKS);
    assert_eq!(
        world.data().grids().get(ships[0]).unwrap().rail,
        Some(rail),
        "a planet 50,000 voxels off is not in the way (the group goes eight thousand voxels a second against it)"
    );
    place(&mut world, 40.0);
    world.update_coasting(TICKS);
    assert_eq!(
        world.data().grids().get(ships[0]).unwrap().rail,
        None,
        "a planet 40 voxels from the last ship is"
    );
}

#[test]
fn anything_done_to_a_ship_of_a_group_takes_the_rail_from_its_anchor_and_leaves_the_rail_of_another_ship()
 {
    // (A force and a torque are not among them: a push on a ship of a group that rides a rail is the rail's for the centre of mass and the bodies' for the turn, step 4c-5-4b, below.)
    type Operation = fn(&mut ServerWorld, GridId);
    let operations: [(&str, Operation); 3] = [
        ("a move", |world, ship| {
            let pose = world.data().grids().get(ship).unwrap().pose;
            world.set_grid_pose(ship, pose).unwrap();
        }),
        ("a hold", |world, ship| {
            world.set_grid_held(ship, true).unwrap()
        }),
        ("a speed", |world, ship| {
            world.set_grid_velocity(ship, [1.0; 3], [0.0; 3]).unwrap()
        }),
    ];
    for target in [1, 2, 0] {
        for (name, operation) in operations {
            let (mut world, planet, ships, rail) = a_group_on_a_rail();
            // A ship on a rail of its own, far away, in no group.
            let alone = ship_at(
                &mut world,
                [-DISTANCE, 0.0, 0.0],
                [0.0, 0.0, circular_speed()],
            );
            let alone_rail = world
                .coasting_rail(alone, time(world.data().clock()))
                .unwrap();
            world
                .set_grid_rail(alone, Some(alone_rail.clone()))
                .unwrap();
            assert_eq!(world.data().grids().get(ships[0]).unwrap().rail, Some(rail));
            operation(&mut world, ships[target]);
            assert_eq!(
                world.data().grids().get(ships[0]).unwrap().rail,
                None,
                "{name} on ship {target}"
            );
            assert_eq!(
                world.data().grids().get(alone).unwrap().rail,
                Some(alone_rail),
                "{name} on ship {target} does nothing to a ship in no group"
            );
            let _ = planet;
        }
    }
}

#[test]
fn a_change_in_who_is_in_the_group_takes_the_rail_from_its_anchor() {
    // A ship gone (destroyed), a ship let go, a ship joined: the rail was of the centre of mass of other ships.
    let (mut world, _planet, ships, _rail) = a_group_on_a_rail();
    world.remove_grid(ships[2]);
    assert_eq!(world.data().grids().get(ships[0]).unwrap().rail, None);

    let (mut world, _planet, ships, _rail) = a_group_on_a_rail();
    world.set_ships_coupled(ships[1], ships[2], false);
    assert_eq!(world.data().grids().get(ships[0]).unwrap().rail, None);

    let (mut world, _planet, ships, _rail) = a_group_on_a_rail();
    let stranger = ship_at(
        &mut world,
        [DISTANCE, 0.0, 400.0],
        [0.0, 0.0, circular_speed()],
    );
    world.set_ships_coupled(ships[2], stranger, true);
    assert_eq!(world.data().grids().get(ships[0]).unwrap().rail, None);

    // A pair of the group that closes a loop changes nobody's group.
    let (mut world, _planet, ships, rail) = a_group_on_a_rail();
    world.set_ships_coupled(ships[0], ships[2], true);
    assert_eq!(world.data().grids().get(ships[0]).unwrap().rail, Some(rail));
}

#[test]
fn a_group_on_a_rail_is_found_from_any_of_its_ships_and_only_when_it_is_whole() {
    let (mut world, _planet, ships, _rail) = a_group_on_a_rail();
    let expected = RailGroup {
        anchor: ships[0],
        members: ships.to_vec(),
    };
    for ship in ships {
        assert_eq!(world.rail_group_of(ship), Some(expected.clone()));
    }
    let (place, speed) = world.rail_group_centre(ships[0]).unwrap();
    let centre = world.data().grids().group_centre(&ships).unwrap();
    assert_eq!((place, speed), (centre.position, centre.velocity));
    assert_eq!(
        world.rail_group_centre(ships[1]),
        None,
        "the rail's point is asked of the anchor"
    );
    // A ship that is alone is in no group, on a rail or not.
    let alone = ship_at(
        &mut world,
        [-DISTANCE, 0.0, 0.0],
        [0.0, 0.0, circular_speed()],
    );
    assert_eq!(world.rail_group_of(alone), None);
    // A group with no rail on its anchor is not on one.
    let (mut other, _planet, others, _rail) = a_group_on_a_rail();
    other.set_grid_rail(others[0], None).unwrap();
    assert_eq!(other.rail_group_of(others[1]), None);
    // A ship of the group in a file: it is not whole, and cannot be put anywhere.
    let before = [0, 1].map(|index| pose_of(&world, ships[index]));
    let parked = ParkedGrid {
        level: LevelIndex::MAIN,
        pose: pose_of(&world, ships[2]),
        bounds: None,
        file: ShipFileName::new(ships[2], 0),
        checksum: 0,
        waits_for_those_aboard: false,
        has_terrain: false,
        on_a_rail: false,
    };
    world.data_mut().grids_mut().park(ships[2], parked).unwrap();
    assert_eq!(world.rail_group_of(ships[0]), None);
    world.data_mut().advance_clock(5);
    // (The world says which ships it could not put: the anchor of the group, whose ships it cannot all find.)
    let now = time(world.world_tick());
    let not_placed = world.data_mut().put_ships_on_their_rails(now);
    assert_eq!(not_placed, [ships[0]]);
    world.apply_rails(TICKS);
    assert_eq!(
        [0, 1].map(|index| pose_of(&world, ships[index])),
        before,
        "nothing is moved"
    );
    // And it takes the rail off the anchor when the world looks at it, since a group that is not whole is not on one.
    world.update_coasting(TICKS);
    assert_eq!(world.data().grids().get(ships[0]).unwrap().rail, None);
}

#[test]
fn the_ships_of_a_group_on_a_rail_are_not_carried_by_a_planet_whose_sphere_they_are_in() {
    // A moon on a rail carries what is within its sphere, ships that are on no rail among it. The ships of a group whose anchor has a rail are put by the group's rail, and the
    // moon does not move them as well.
    let (mut world, planet) = space();
    let moon = planet_on(
        &mut world,
        20,
        1.0e19,
        Some(orbit_about(Centre::Grid(planet), 60_000.0, 5_000.0)),
    );
    world.data.set_clock(0);
    world.apply_rails(TICKS);
    let moon_at = world
        .data()
        .grids()
        .get(moon)
        .unwrap()
        .pose
        .position
        .in_voxels();
    let ships = [0.0, 20.0, 40.0].map(|along| {
        ship_at(
            &mut world,
            [moon_at[0] + 300.0, moon_at[1], moon_at[2] + along],
            [0.0, 0.0, 100.0],
        )
    });
    world.set_ships_coupled(ships[0], ships[1], true);
    world.set_ships_coupled(ships[1], ships[2], true);
    world.apply_rails(TICKS);
    for ship in ships {
        assert_eq!(
            world.carrier_of(ship),
            Some(moon),
            "{ship} is carried by the moon, as any ship is"
        );
    }
    let rail = rail_of_the_group(&world, planet, &ships);
    world.set_grid_rail(ships[0], Some(rail)).unwrap();
    world.apply_rails(TICKS);
    for ship in ships {
        assert_eq!(
            world.carrier_of(ship),
            None,
            "{ship} is in a group on a rail"
        );
    }
}

/// Three ships going round the planet at the same speed, 200 voxels farther out and 30 along from one to the next, weighed 1,000, 3,000 and 2,000, coupled one to the next: the
/// centre of mass of the group is the place of none of them. No rail yet. The planet and the ships.
fn a_group_in_a_circle() -> (ServerWorld, GridId, [GridId; 3]) {
    let (mut world, planet) = space();
    let speed = circular_speed();
    let ships = [(0.0, 0.0), (200.0, 30.0), (400.0, 60.0)]
        .map(|(out, along)| ship_at(&mut world, [DISTANCE + out, 0.0, along], [0.0, 0.0, speed]));
    for (ship, mass) in ships.iter().zip([1000.0, 3000.0, 2000.0]) {
        world.set_grid_mass(*ship, mass, [0.0; 3]).unwrap();
    }
    world.set_ships_coupled(ships[0], ships[1], true);
    world.set_ships_coupled(ships[1], ships[2], true);
    (world, planet, ships)
}

/// Runs `ticks` ticks of the rules of rails (the clock passes one, and ships left alone are put on rails) and says in which of them, counting from 1, the ship was first on one.
fn first_tick_on_a_rail(world: &mut ServerWorld, ship: GridId, ticks: u64) -> Option<u64> {
    (1..=ticks).find(|_| {
        world.data_mut().advance_clock(1);
        world.update_coasting(TICKS);
        world.apply_rails(TICKS);
        world.data().grids().get(ship).unwrap().rail.is_some()
    })
}

#[test]
fn a_group_is_given_the_orbit_of_its_centre_of_mass_and_only_its_anchor_is_asked() {
    let (mut world, planet, ships) = a_group_in_a_circle();
    // (The anchor goes slower than the others, so that the speed of the group is not the speed of any one of the ships: a tenth less for the anchor, a tenth more for the last.)
    world
        .set_grid_velocity(ships[0], [0.0, 0.0, 0.9 * circular_speed()], [0.0; 3])
        .unwrap();
    world
        .set_grid_velocity(ships[2], [0.0, 0.0, 1.1 * circular_speed()], [0.0; 3])
        .unwrap();
    let rail = world.coasting_rail(ships[0], time(0)).unwrap();
    assert!(rail.is_coasting());
    // The orbit has the centre of mass of the group where the group is: at the time the rail was made, its place is the centre of mass' and its speed is the group's. (The
    // centre of mass is 270 voxels farther out than the anchor, and a rail made of the anchor would not be at it.)
    let centre = world.data().grids().group_centre(&ships).unwrap();
    let anchor = world
        .data()
        .grids()
        .get(ships[0])
        .unwrap()
        .middle()
        .unwrap();
    let planet_at = world
        .data()
        .grids()
        .get(planet)
        .unwrap()
        .pose
        .position
        .in_voxels();
    let state = rail.state(time(0), planet_at, [0.0; 3]);
    close(
        state.position.unwrap(),
        centre.position.in_voxels(),
        1e-6,
        "its place is the centre of mass'",
    );
    close(
        state.velocity,
        centre.velocity,
        1e-9,
        "and its speed the group's",
    );
    assert!(centre.position.relative_to(anchor)[0] > 200.0);
    // The other ships of the group are not asked: their rail is the anchor's.
    for ship in &ships[1..] {
        assert_eq!(
            world.coasting_rail(*ship, time(0)).unwrap_err(),
            NotCoasting::InAGroup(ships[0])
        );
    }
}

#[test]
fn a_group_whose_ships_have_not_been_weighed_is_given_the_orbit_of_the_middle_of_their_places() {
    let (mut world, planet, ships) = a_group_in_a_circle();
    for ship in ships {
        world.set_grid_mass(ship, 0.0, [0.0; 3]).unwrap();
    }
    let rail = world.coasting_rail(ships[0], time(0)).unwrap();
    let planet_at = world
        .data()
        .grids()
        .get(planet)
        .unwrap()
        .pose
        .position
        .in_voxels();
    let place = rail.state(time(0), planet_at, [0.0; 3]).position.unwrap();
    // 10,000, 10,200 and 10,400 along x, 0, 30 and 60 along z: the middle of them.
    close(place, [10_200.0, 0.0, 30.0], 1e-6, "the mean of the places");
}

#[test]
fn a_group_is_put_on_a_rail_when_nothing_has_touched_it_for_a_second_and_only_its_anchor_has_one() {
    let (mut world, planet, ships) = a_group_in_a_circle();
    // (The ships were made and set going at the clock's 0; a second is 20 ticks.)
    let first = first_tick_on_a_rail(&mut world, ships[0], 40).expect("it is put on a rail");
    assert!((20..=22).contains(&first), "after {first} ticks");
    for ship in &ships[1..] {
        assert_eq!(
            world.data().grids().get(*ship).unwrap().rail,
            None,
            "{ship}"
        );
    }
    // It is the orbit of the centre of mass: the group stays on it as the world puts it there.
    let rail = world
        .data()
        .grids()
        .get(ships[0])
        .unwrap()
        .rail
        .clone()
        .unwrap();
    let planet_at = world
        .data()
        .grids()
        .get(planet)
        .unwrap()
        .pose
        .position
        .in_voxels();
    let state = rail.state(time(world.world_tick()), planet_at, [0.0; 3]);
    close(
        centre_of(&world, &ships).0,
        state.position.unwrap(),
        1e-6,
        "the centre of mass is on the rail",
    );
}

#[test]
fn a_touch_on_any_ship_of_a_group_makes_it_wait_a_second_again() {
    for target in [0, 1, 2] {
        let (mut world, _planet, ships) = a_group_in_a_circle();
        // Ten ticks of quiet, and a touch (the speed it has, said again, is a touch).
        assert_eq!(first_tick_on_a_rail(&mut world, ships[0], 10), None);
        let speed = world
            .data()
            .grids()
            .get(ships[target])
            .unwrap()
            .linear_velocity;
        world
            .set_grid_velocity(ships[target], speed, [0.0; 3])
            .unwrap();
        let after = first_tick_on_a_rail(&mut world, ships[0], 40).expect("it is put on a rail");
        assert!(
            (20..=22).contains(&after),
            "touched {target}: {after} ticks after the touch"
        );
    }
}

#[test]
fn a_group_is_not_put_on_a_rail_with_something_near_it_and_keeps_one_it_has_whatever_is_near() {
    let (mut world, _planet, ships) = a_group_in_a_circle();
    // A stranger 100 voxels from the last ship, going as the group does: it is nearer than twice the margin (128) that is needed to go on a rail, and farther than the margin
    // (64) that is needed to stay on one.
    let stranger = ship_at(
        &mut world,
        [DISTANCE + 400.0, 0.0, 160.0],
        [0.0, 0.0, circular_speed()],
    );
    assert_eq!(first_tick_on_a_rail(&mut world, ships[0], 60), None);
    assert!(matches!(
        world.coasting_rail(ships[0], time(0)).unwrap_err(),
        NotCoasting::SomethingNear(near) if near == stranger
    ));
    // The stranger goes away and the group, quiet, is put on a rail (and goes round, which is eight kilometres in a second here); the stranger is brought back to 100 voxels from
    // the last ship, going as it goes, and the group keeps its rail.
    world
        .set_grid_pose(
            stranger,
            GridPose::new(
                WorldPosition::from_voxels([DISTANCE, 5_000.0, 0.0]),
                Orientation::IDENTITY,
            ),
        )
        .unwrap();
    assert!(first_tick_on_a_rail(&mut world, ships[0], 40).is_some());
    let last = world.data().grids().get(ships[2]).unwrap();
    let (near, going) = (
        last.pose.position.translated([0.0, 0.0, 100.0]),
        last.linear_velocity,
    );
    world
        .set_grid_pose(stranger, GridPose::new(near, Orientation::IDENTITY))
        .unwrap();
    world.set_grid_velocity(stranger, going, [0.0; 3]).unwrap();
    world.update_coasting(TICKS);
    assert!(
        world.data().grids().get(ships[0]).unwrap().rail.is_some(),
        "100 voxels from a stranger it has the room to stay"
    );
    // And at 50 it keeps it as well: what is near a group on a rail is the contact ghost's and a touch takes the rail (the rail's kick, 2026-11-17). It was taken at half the room
    // it needs to go on one before.
    let nearer = last_position(&world, ships[2]).translated([0.0, 0.0, 50.0]);
    world
        .set_grid_pose(stranger, GridPose::new(nearer, Orientation::IDENTITY))
        .unwrap();
    world.update_coasting(TICKS);
    assert!(
        world.data().grids().get(ships[0]).unwrap().rail.is_some(),
        "50 voxels from a stranger it keeps the rail"
    );
}

#[test]
fn the_ships_near_a_group_on_a_rail_are_measured_with_the_group_a_tick_back() {
    // The rails put a group at its place of the end of the tick when the tick begins, and a ship that is free is where it was at the start until the bodies step it: the two are
    // a tick of the group's speed apart in the world's places (800 voxels in the world of these tests, 38 at orbital speed round Cave's planet). A ghost that the bodies make for the
    // tick that is about to be asked is asked with the length of the tick, and puts the ships that the rails place back by it.
    let (mut world, _planet, ships, _rail) = a_group_on_a_rail();
    let hull =
        HullBox::of_voxels([VoxelCoordinate::new(0, 0, 0), VoxelCoordinate::new(3, 3, 3)]).unwrap();
    for ship in ships {
        world.set_grid_hull(ship, Some(hull)).unwrap();
    }
    let rear = pose_of(&world, ships[0]).position;
    let going = velocity_of(&world, ships[0]);
    let speed = libm::sqrt(going.iter().map(|part| part * part).sum());
    let way = going.map(|part| part / speed);
    let tick = 1.0 / TICKS as f64;
    let lag = speed * tick;
    let radii = 2.0 * hull.radius();
    let room = ContactRoom {
        margin: 10.0,
        seconds: 0.0,
    };
    // A stranger behind the group's rear ship, 5 voxels from where it was at the start of the tick and a tick of its speed more from where the rail has put it.
    let behind = ship_at(
        &mut world,
        rear.translated(way.map(|part| -part * (lag + radii + 5.0)))
            .in_voxels(),
        going,
    );
    world.set_grid_hull(behind, Some(hull)).unwrap();
    assert_eq!(
        world.ships_near_group(ships[0], room),
        [],
        "as the world has them, it is far"
    );
    assert_eq!(
        world.ships_near_group_lagging(ships[0], room, tick),
        [behind],
        "a tick back, it is 5 voxels from the hull"
    );
    // One ahead of the group's front ship, 5 voxels from where the rail has put it, is a tick of its speed farther than that from where it was at the start.
    let front = pose_of(&world, ships[2]).position;
    world
        .set_grid_pose(
            behind,
            GridPose::new(
                front.translated(way.map(|part| part * (radii + 5.0))),
                Orientation::IDENTITY,
            ),
        )
        .unwrap();
    assert_eq!(world.ships_near_group(ships[0], room), [behind]);
    assert_eq!(
        world.ships_near_group_lagging(ships[0], room, tick),
        [],
        "a tick back, the ship ahead is a tick of the speed farther off"
    );
    assert!(
        world.is_placed_by_a_rail(ships[2]),
        "a ship of a group on a rail is"
    );
    assert!(!world.is_placed_by_a_rail(behind), "and a free ship is not");
}

#[test]
fn a_group_on_a_rail_is_taken_off_it_by_the_bodies_for_a_hit_and_is_not_quiet() {
    let (mut world, _planet, ships, rail) = a_group_on_a_rail();
    world.update_coasting(TICKS);
    assert_eq!(world.data().grids().get(ships[0]).unwrap().rail, Some(rail));
    // Any ship of it will do to ask, and the group is not quiet: it waits a second to be put on a rail again.
    world.take_group_off_its_rail(ships[2]);
    assert_eq!(world.data().grids().get(ships[0]).unwrap().rail, None);
    assert_eq!(first_tick_on_a_rail(&mut world, ships[0], 10), None);
}

fn last_position(world: &ServerWorld, ship: GridId) -> WorldPosition {
    pose_of(world, ship).position
}

#[test]
fn a_group_with_a_ship_held_or_in_a_file_is_not_given_a_rail() {
    let (mut world, _planet, ships) = a_group_in_a_circle();
    world.set_grid_held(ships[2], true).unwrap();
    assert_eq!(
        world.coasting_rail(ships[0], time(0)).unwrap_err(),
        NotCoasting::Held
    );
    // (A ship that is held has no speed, and let go goes at none: it is given the speed of the others, or the group would go too low.)
    world.set_grid_held(ships[2], false).unwrap();
    world
        .set_grid_velocity(ships[2], [0.0, 0.0, circular_speed()], [0.0; 3])
        .unwrap();
    assert!(world.coasting_rail(ships[0], time(0)).is_ok());
    let parked = ParkedGrid {
        level: LevelIndex::MAIN,
        pose: pose_of(&world, ships[2]),
        bounds: None,
        file: ShipFileName::new(ships[2], 0),
        checksum: 0,
        waits_for_those_aboard: false,
        has_terrain: false,
        on_a_rail: false,
    };
    world.data_mut().grids_mut().park(ships[2], parked).unwrap();
    assert_eq!(
        world.coasting_rail(ships[0], time(0)).unwrap_err(),
        NotCoasting::GroupNotWhole
    );
}

#[test]
fn a_group_that_is_made_waits_a_second_though_the_ships_were_quiet_before() {
    let (mut world, _planet, ships) = a_group_in_a_circle();
    // They are alone first, for fifteen ticks (each is quiet, and none has a rail yet), and are coupled in the sixteenth.
    world.set_ships_coupled(ships[0], ships[1], false);
    world.set_ships_coupled(ships[1], ships[2], false);
    assert_eq!(first_tick_on_a_rail(&mut world, ships[0], 15), None);
    world.set_ships_coupled(ships[0], ships[1], true);
    world.set_ships_coupled(ships[1], ships[2], true);
    // The group is not asked to have been quiet for the fifteen: it waits its own second.
    let after = first_tick_on_a_rail(&mut world, ships[0], 40).expect("it is put on a rail");
    assert!(
        (20..=22).contains(&after),
        "{after} ticks after the pairs were made"
    );
    // A pair said again (the bodies say it of every pair, every tick) is not a change, and makes no one wait.
    world.set_grid_rail(ships[0], None).unwrap();
    assert_eq!(first_tick_on_a_rail(&mut world, ships[0], 10), None);
    world.set_ships_coupled(ships[0], ships[1], true);
    world.set_ships_coupled(ships[1], ships[2], true);
    assert!(first_tick_on_a_rail(&mut world, ships[0], 40).is_some_and(|tick| tick < 20));
}

#[test]
fn a_group_on_a_rail_answers_a_change_of_the_mass_of_its_planet_from_its_centre_of_mass() {
    // Three ships on an ellipse round the planet, 30 voxels apart along their way, weighed alike: the centre of mass is not the anchor's place. The planet weighs more, and
    // at the nearest point the rail is made again from the state the group is in, which is its centre of mass' and not the anchor's.
    let (mut world, planet) = space();
    let mu = PULL * PLANET_MASS;
    let speed = libm::sqrt(mu * (2.0 / 10_000.0 - 1.0 / 7_500.0));
    let ships = [0.0, 30.0, 60.0]
        .map(|along| ship_at(&mut world, [10_000.0, 0.0, along], [0.0, 0.0, speed]));
    for ship in ships {
        world.set_grid_mass(ship, 1000.0, [0.0; 3]).unwrap();
    }
    world.set_ships_coupled(ships[0], ships[1], true);
    world.set_ships_coupled(ships[1], ships[2], true);
    for tick in 0..=21 {
        server_tick(&mut world, tick);
    }
    let old = rail_of(&world, ships[0]);
    assert!(old.is_some(), "on a rail at tick 21");
    let before = orbit_of(&world, ships[0]);
    world.set_grid_mass(planet, 1.2e22, [0.0; 3]).unwrap();
    let expected = ticks_of_the_end(before, 21, 400, true)[0];
    let tick =
        until_the_rail_changes(&mut world, ships[0], old.clone(), 21, 400).expect("answered");
    assert_eq!(
        tick, expected,
        "answered in the tick that passes the nearest point"
    );
    let after = orbit_of(&world, ships[0]);
    assert!((after.mass_parameter() / (PULL * 1.2e22) - 1.0).abs() < 1e-12);
    // The group did not jump: it is where the old rail has it at that time, and goes as the old rail has it. (A rail made of the anchor's state would have put it 30 voxels away.)
    let planet_here = world
        .data()
        .grids()
        .get(planet)
        .unwrap()
        .pose
        .position
        .in_voxels();
    let was = old
        .unwrap()
        .state(time(world.world_tick()), planet_here, [0.0; 3]);
    close(
        centre_of(&world, &ships).0,
        was.position.unwrap(),
        1e-6,
        "the group did not move when its rail was made again",
    );
    close(
        centre_of(&world, &ships).1,
        was.velocity,
        1e-6,
        "or change its speed",
    );
    // The new rail has the centre of mass of the group where it is, going as it goes. (The anchor is 30 voxels from it, and a rail made of the anchor's state would not.)
    let planet_at = world
        .data()
        .grids()
        .get(planet)
        .unwrap()
        .pose
        .position
        .in_voxels();
    let rail = rail_of(&world, ships[0]).unwrap();
    let state = rail.state(time(world.world_tick()), planet_at, [0.0; 3]);
    close(
        centre_of(&world, &ships).0,
        state.position.unwrap(),
        1e-6,
        "the centre of mass is on the new rail",
    );
    close(
        centre_of(&world, &ships).1,
        state.velocity,
        1e-6,
        "and goes as it does",
    );
    for ship in &ships[1..] {
        assert_eq!(rail_of(&world, *ship), None);
    }
}

// ---- Step 4c-5-4b for a group: a push on a ship of a group that rides a rail does not take the rail. The force on the whole group is the rail's (the centre of mass changes its speed
// by the force over the mass of all the ships); the bodies have the force too, for the turn and the swing of the joints, and hold the centre of mass at the rail's point whatever it did
// to it. A torque is the bodies' alone. ----

/// What the three ships of the fixture weigh together.
const GROUP_MASS: f64 = 1000.0 + 3000.0 + 2000.0;

/// Where the planet is, which the rails of the fixture go round.
fn planet_at(world: &ServerWorld, planet: GridId) -> [f64; 3] {
    world
        .data()
        .grids()
        .get(planet)
        .unwrap()
        .pose
        .position
        .in_voxels()
}

/// The place and the speed that a rail says at a tick, from the planet.
fn rail_says(rail: &Rail, tick: u64, planet: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let state = rail.state(time(tick), planet, [0.0; 3]);
    (state.position.unwrap(), state.velocity)
}

#[test]
fn a_force_on_a_ship_of_a_group_on_a_rail_makes_its_rail_again_with_the_speed_changed_by_the_force_over_the_mass_of_all_the_ships_and_the_bodies_have_it_too()
 {
    for target in [1, 2, 0] {
        let (mut world, planet, ships, rail) = a_group_on_a_rail();
        let at = planet_at(&world, planet);
        let force = [0.0, 3.0e3, 5.0e3];
        world.apply_grid_force(ships[target], force, None).unwrap();
        server_tick(&mut world, 1);
        let after = rail_of(&world, ships[0]).expect("the group is still on a rail");
        assert_ne!(after, rail, "ship {target}: the rail is another");
        assert_eq!(
            after.orbit.as_ref().unwrap().since,
            Some(time(1)),
            "ship {target}: made at the end of the tick of the push"
        );
        let ((place, speed), (place_after, speed_after)) =
            (rail_says(&rail, 1, at), rail_says(&after, 1, at));
        let change = force.map(|part| part / GROUP_MASS / f64::from(TICKS));
        close(
            place_after,
            place,
            1e-9,
            "the rail goes through the place it had",
        );
        close(
            [0, 1, 2].map(|axis| speed_after[axis] - speed[axis]),
            change,
            1e-9,
            &format!(
                "ship {target}: the speed is the force over the mass of all the ships, for a tick"
            ),
        );
        close(
            centre_of(&world, &ships).1,
            speed_after,
            1e-9,
            "and the group goes as the new rail does",
        );
        assert_eq!(
            world.take_grid_forces(),
            [GridForce {
                grid: ships[target],
                force,
                at: None
            }],
            "ship {target}: the bodies are asked for the force too (they hold the centre of mass at the rail's point, and have the turn)"
        );
        for other in ships.iter().skip(1) {
            assert_eq!(
                rail_of(&world, *other),
                None,
                "a rail is the anchor's alone"
            );
        }
    }
}

#[test]
fn the_forces_asked_of_the_ships_of_a_group_on_a_rail_in_a_tick_are_one_push_their_sum() {
    let (mut one_world, _, one, _) = a_group_on_a_rail();
    let (mut two_world, _, two, _) = a_group_on_a_rail();
    one_world
        .apply_grid_force(one[1], [1.0e3, -2.0e3, 5.0e3], None)
        .unwrap();
    two_world
        .apply_grid_force(two[0], [4.0e2, 0.0, 2.0e3], None)
        .unwrap();
    two_world
        .apply_grid_force(two[2], [6.0e2, -2.0e3, 3.0e3], Some([1.0, 2.0, 3.0]))
        .unwrap();
    server_tick(&mut one_world, 1);
    server_tick(&mut two_world, 1);
    let (together, apart) = (
        rail_of(&one_world, one[0]).unwrap(),
        rail_of(&two_world, two[0]).unwrap(),
    );
    let at = [0.0; 3];
    let ((place_together, speed_together), (place_apart, speed_apart)) =
        (rail_says(&together, 1, at), rail_says(&apart, 1, at));
    close(place_together, place_apart, 1e-9, "the same place");
    close(speed_together, speed_apart, 1e-9, "and the same speed");
}

#[test]
fn a_torque_on_a_ship_of_a_group_on_a_rail_leaves_the_rail_as_it_was_and_is_the_bodies() {
    for target in [1, 2, 0] {
        let (mut world, _, ships, rail) = a_group_on_a_rail();
        let torque = [0.0, 30.0, 1.0];
        world.apply_grid_torque(ships[target], torque).unwrap();
        server_tick(&mut world, 1);
        assert_eq!(
            rail_of(&world, ships[0]),
            Some(rail),
            "ship {target}: the rail is what it was"
        );
        assert_eq!(
            world.take_grid_torques(),
            [GridTorque {
                grid: ships[target],
                torque
            }],
            "ship {target}"
        );
        assert_eq!(world.take_grid_forces(), []);
    }
}

#[test]
fn a_group_that_has_lost_its_rail_by_the_end_of_the_tick_is_not_given_the_force_a_second_time() {
    // The bodies were asked for the force when it was asked of the world, and they go on without the rail from there: the end of the tick has nothing to add to what they have.
    let (mut world, _, ships, _) = a_group_on_a_rail();
    let force = [0.0, 3.0e3, 5.0e3];
    world.apply_grid_force(ships[1], force, None).unwrap();
    // (Something else takes the rail in the same tick.)
    world.set_grid_held(ships[2], true).unwrap();
    assert_eq!(rail_of(&world, ships[0]), None);
    server_tick(&mut world, 1);
    assert_eq!(rail_of(&world, ships[0]), None);
    assert_eq!(
        world.take_grid_forces(),
        [GridForce {
            grid: ships[1],
            force,
            at: None
        }],
        "once"
    );
}

#[test]
fn a_force_on_a_group_that_has_a_ship_that_weighs_nothing_takes_its_rail_as_it_did() {
    // (The mass of the whole is not known: the centre of mass is of the ships that weigh something, and what the force does to it is not the force over their mass.)
    let (mut world, _, ships, rail) = a_group_on_a_rail_weighing([1000.0, 3000.0, 0.0]);
    assert_eq!(rail_of(&world, ships[0]), Some(rail));
    world
        .apply_grid_force(ships[1], [0.0, 3.0e3, 5.0e3], None)
        .unwrap();
    assert_eq!(rail_of(&world, ships[0]), None);
    server_tick(&mut world, 1);
    assert_eq!(world.take_grid_forces().len(), 1, "and the bodies have it");
}

#[test]
fn a_force_on_a_group_whose_planet_is_gone_takes_its_rail_as_it_did_and_the_bodies_have_it_once() {
    // (No orbit is made through a planet that is not there: the rail goes and the bodies have the force, as they did before a group's rail could be pushed.)
    let (mut world, planet, ships, rail) = a_group_on_a_rail();
    assert_eq!(rail_of(&world, ships[0]), Some(rail));
    world.remove_grid(planet);
    let force = [0.0, 3.0e3, 5.0e3];
    world.apply_grid_force(ships[1], force, None).unwrap();
    assert_eq!(rail_of(&world, ships[0]), None);
    server_tick(&mut world, 1);
    assert_eq!(
        world.take_grid_forces(),
        [GridForce {
            grid: ships[1],
            force,
            at: None
        }],
        "once"
    );
}

#[test]
fn a_ship_pushed_alone_and_then_coupled_into_a_group_on_a_rail_in_one_tick_is_given_only_the_force_that_the_bodies_have_not_had()
 {
    // The rail forces of one rail are one push, but the bodies were given what was asked of a ship of a group at once and what was asked of a ship alone not at all: when the rail cannot
    // be made, the bodies are owed the second kind only.
    let (mut world, planet) = space();
    let speed = circular_speed();
    let ships = [0.0, 20.0, 40.0]
        .map(|along| ship_at(&mut world, [DISTANCE, 0.0, along], [0.0, 0.0, speed]));
    for ship in &ships {
        world.set_grid_mass(*ship, 1000.0, [0.0; 3]).unwrap();
    }
    let alone = rail_of_the_group(&world, planet, &ships[..1]);
    world.set_grid_rail(ships[0], Some(alone)).unwrap();
    let (first, second) = ([0.0, 1.0e3, 0.0], [0.0, 0.0, 2.0e3]);
    world.apply_grid_force(ships[0], first, None).unwrap();
    // It is coupled to the second ship, which takes its rail; the pair is put on one, and a push on the second is the group's.
    world.set_ships_coupled(ships[0], ships[1], true);
    let together = rail_of_the_group(&world, planet, &ships[..2]);
    world.set_grid_rail(ships[0], Some(together)).unwrap();
    world.apply_grid_force(ships[1], second, None).unwrap();
    assert_eq!(world.rides_a_rail_in_a_group(ships[1]), Some(ships[0]));
    // The planet is gone before the tick ends: no rail is made.
    world.remove_grid(planet);
    server_tick(&mut world, 1);
    assert_eq!(rail_of(&world, ships[0]), None);
    assert_eq!(
        world.take_grid_forces(),
        [
            GridForce {
                grid: ships[1],
                force: second,
                at: None
            },
            GridForce {
                grid: ships[0],
                force: first,
                at: None
            }
        ],
        "the second was given when it was asked, and the first, which was the rail's, is given now: each once"
    );
}
