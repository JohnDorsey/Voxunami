//! Ships coupled by their joint ends ride a rail as a group (step 4c-3 of `docs/decisions/joints-and-cars.md`): one rail, on the ship with the lowest number, the orbit of the centre
//! of mass of the ships; the world puts the group where the rail says without moving the ships against each other; and anything done to any ship of it takes the rail from the
//! anchor. These are the world's rules with no bodies to move anything; what the bodies do with a group on a rail is tested with them (`games/voxunami_cave/tests/group_on_a_rail.rs`).
//!
//! The rails here are made by hand, from the centre of mass and the speed of the group: that a group is put on one by itself is another matter (the rules of `coasting_rail`).

use voxel_space::Orientation;
use world_storage::rail::{Centre, Orbit, Rail, RailTime};
use world_storage::{ParkedGrid, ShipFileName};

use super::coasting_ships::{PULL, TICKS, ship_at, space};
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
    let (mut world, planet) = space();
    let speed = circular_speed();
    let ships = [0.0, 20.0, 40.0]
        .map(|along| ship_at(&mut world, [DISTANCE, 0.0, along], [0.0, 0.0, speed]));
    let weights = [
        (1000.0, [2.0, 0.0, 0.0]),
        (3000.0, [0.0, 1.0, 0.0]),
        (2000.0, [1.0, 1.0, 1.0]),
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
fn the_ships_of_a_group_are_not_in_each_others_way_and_a_ship_near_one_of_them_is_in_the_way_of_the_group()
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
    assert_eq!(world.data().grids().get(ships[0]).unwrap().rail, Some(rail));
    // One near the last ship of the group takes the rail from the anchor, which is not near it.
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
    assert_eq!(world.data().grids().get(ships[0]).unwrap().rail, None);
}

#[test]
fn anything_done_to_a_ship_of_a_group_takes_the_rail_from_its_anchor_and_leaves_the_rail_of_another_ship()
 {
    type Operation = fn(&mut ServerWorld, GridId);
    let operations: [(&str, Operation); 5] = [
        ("a force", |world, ship| {
            world.apply_grid_force(ship, [1.0; 3], None).unwrap()
        }),
        ("a torque", |world, ship| {
            world.apply_grid_torque(ship, [1.0; 3]).unwrap()
        }),
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
