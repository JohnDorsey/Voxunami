//! A test-only renumbering gives a ship a new id and changes nothing else (`ServerWorld::renumber_ship`): it finds what keeps the old id of a ship after the ship has been put under another, which
//! is what a merge or a split does to a ship and a rename would do (`docs/decisions/joints-and-cars.md`, "The planet's unchanged identifier"). These are the things the world itself keeps by ship;
//! the rules of a game and the bodies are run through it in the games' own tests.

use super::coasting_ships::{PULL, TICKS, ship_at, space};
use super::*;
use world_storage::RenumberError;
use world_storage::rail::Centre;

#[test]
fn what_is_aboard_a_renumbered_ship_is_aboard_the_new_id_where_it_was_and_the_ship_is_the_same() {
    let (mut world, ship, _, walkers) = row_with_walkers(&[[1.5, 2.0, 0.5], [7.5, 2.0, 0.5]]);
    let before = world.data().grids().get(ship).unwrap().clone();
    let poses: Vec<EntityPose> = walkers
        .iter()
        .map(|walker| world.entities().get(*walker).unwrap().pose)
        .collect();
    let renumbered = world.renumber_ship(ship).unwrap();
    assert_ne!(renumbered, ship);
    assert!(world.data().grids().get(ship).is_none());
    assert_eq!(world.data().grids().get(renumbered), Some(&before));
    for (walker, pose) in walkers.iter().zip(poses) {
        let found = world.entities().get(*walker).unwrap().pose;
        assert_eq!(
            found,
            EntityPose::new(renumbered, pose.position, pose.orientation)
        );
    }
    assert!(world.entities().in_grid(ship).is_empty());
}

#[test]
fn a_renumbered_ship_is_coupled_to_its_partners_and_a_group_that_is_alone_is_alone_still() {
    let distance = 10_000.0;
    let (mut world, _planet) = space();
    let speed = libm::sqrt(PULL * 1.0e22 / distance);
    let ships = [0.0, 20.0, 40.0]
        .map(|along| ship_at(&mut world, [distance, 0.0, along], [0.0, 0.0, speed]));
    for ship in ships {
        world.set_grid_mass(ship, 1000.0, [0.0; 3]).unwrap();
    }
    world.set_ships_coupled(ships[0], ships[1], true);
    world.set_ships_coupled(ships[1], ships[2], true);
    assert!(world.group_is_alone(ships[0], 1.0));
    let renumbered = world.renumber_ship(ships[1]).unwrap();
    assert_eq!(
        world.data().grids().group_of(ships[0]),
        [ships[0], ships[2], renumbered]
    );
    // (The lowest number is still the anchor, so the group is the group it was.)
    assert!(world.group_is_alone(ships[0], 1.0));
}

#[test]
fn a_ship_carried_by_a_planet_on_a_rail_is_carried_by_it_under_the_new_id() {
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
    let ship = ship_at(
        &mut world,
        [moon_at[0] + 300.0, moon_at[1], moon_at[2]],
        [0.0, 0.0, 100.0],
    );
    world.apply_rails(TICKS);
    assert_eq!(world.carrier_of(ship), Some(moon), "the premise");
    let renumbered = world.renumber_ship(ship).unwrap();
    assert_eq!(world.carrier_of(ship), None);
    assert_eq!(world.carrier_of(renumbered), Some(moon));
    world.apply_rails(TICKS);
    assert_eq!(
        world.carrier_of(renumbered),
        Some(moon),
        "and the next tick keeps it"
    );
}

#[test]
fn a_ship_that_has_been_left_alone_for_a_while_has_been_left_alone_for_as_long_under_the_new_id() {
    // The quiet of a free ship is counted from the tick it began; a renumbered ship did not begin again.
    let (mut world, _planet) = space();
    let ship = ship_at(&mut world, [10_000.0, 0.0, 0.0], [0.0, 0.0, 8_000.0]);
    world.set_grid_mass(ship, 1000.0, [0.0; 3]).unwrap();
    world.data.set_clock(0);
    world.update_coasting(TICKS);
    let before = world.quiet_since.get(&ship).copied();
    assert!(before.is_some(), "the premise: the quiet began");
    let renumbered = world.renumber_ship(ship).unwrap();
    assert_eq!(world.quiet_since.get(&ship).copied(), None);
    assert_eq!(world.quiet_since.get(&renumbered).copied(), before);
}

#[test]
fn a_planet_and_a_ship_that_is_gone_are_not_renumbered() {
    let (mut world, planet) = space();
    assert_eq!(world.renumber_ship(planet), Err(RenumberError::HasTerrain));
    assert_eq!(
        world.renumber_ship(GridId::new(GridId::FIRST_ALLOCATED + 99)),
        Err(RenumberError::NotInMemory)
    );
    assert!(world.data().grids().get(planet).is_some());
}
