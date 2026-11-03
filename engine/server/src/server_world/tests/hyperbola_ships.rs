//! Ships on hyperbolas (E16 of `docs/orbits-and-thrust-plan.md`): a free ship that goes faster than it would to escape, round a planet whose sphere is not asked (a planet that goes round
//! nothing), goes on a rail that is a hyperbola, as one that goes slower goes on an ellipse; the rules that were made for an ellipse are those of a hyperbola where they mean the same, and
//! are said again where they do not: a periapsis that is behind the ship does not take it off its rail, and a change of the mass has no apoapsis to wait for.

use super::coasting_ships::{
    PULL, orbit_of, rail_of, refused, server_tick, ship_at, space, ticks_of_the_end,
    until_the_rail_changes,
};
use super::*;
use crate::server_world::NotCoasting;

/// `G M` of the planet of `space`.
const MU: f64 = PULL * 1.0e22;

/// A ship at this place from the planet (which is at the origin) going at this speed, left alone until it is on its rail at tick 21.
fn a_ship_on_a_hyperbola(place: [f64; 3], velocity: [f64; 3]) -> (ServerWorld, GridId, GridId) {
    let (mut world, planet) = space();
    let ship = ship_at(&mut world, place, velocity);
    for tick in 0..=21 {
        server_tick(&mut world, tick);
    }
    assert!(rail_of(&world, ship).is_some(), "on a rail at tick 21");
    (world, ship, planet)
}

/// The energy and the angular momentum a ship has, against the planet, from the world.
fn energy_and_momentum(world: &ServerWorld, ship: GridId, planet: GridId) -> (f64, f64, f64) {
    let grid = world.data().grids().get(ship).unwrap();
    let r = world
        .data()
        .grids()
        .get(planet)
        .unwrap()
        .pose
        .position
        .displacement_to(grid.pose.position)
        .in_voxels();
    let v = grid.linear_velocity;
    let distance = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
    let h = [
        r[1] * v[2] - r[2] * v[1],
        r[2] * v[0] - r[0] * v[2],
        r[0] * v[1] - r[1] * v[0],
    ];
    (
        0.5 * (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) - MU / distance,
        (h[0] * h[0] + h[1] * h[1] + h[2] * h[2]).sqrt(),
        distance,
    )
}

#[test]
fn a_ship_going_faster_than_it_would_to_escape_goes_on_a_hyperbola_and_keeps_its_energy_and_angular_momentum()
 {
    // 10,000 voxels out and across the radius at 12,000 a second; the speed of escape there is 11,555.
    let (mut world, ship, planet) =
        a_ship_on_a_hyperbola([10_000.0, 0.0, 0.0], [0.0, 0.0, 12_000.0]);
    let orbit = orbit_of(&world, ship);
    assert!(orbit.is_a_hyperbola());
    assert!((orbit.mass_parameter() / MU - 1.0).abs() < 1e-12);
    let (energy, momentum, distance) = energy_and_momentum(&world, ship, planet);
    assert!(energy > 0.0, "energy {energy}");
    for tick in [100, 2_000, 40_000, 3_000_000] {
        server_tick(&mut world, tick);
        let (later_energy, later_momentum, later_distance) =
            energy_and_momentum(&world, ship, planet);
        assert!(
            (later_energy - energy).abs() <= 1e-9 * energy,
            "tick {tick}: {later_energy} against {energy}"
        );
        assert!(
            (later_momentum - momentum).abs() <= 1e-9 * momentum,
            "tick {tick}"
        );
        assert!(later_distance > distance, "it goes away: tick {tick}");
    }
}

#[test]
fn a_ship_whose_periapsis_is_behind_it_is_not_too_low_and_one_whose_is_ahead_is() {
    // Both go at 12,000 along the radius and 1,000 across it, 10,000 voxels out: the hyperbola comes within 75 voxels of the middle (the ground reaches 3,200). Going away, that point
    // was passed; coming in, it is still to come.
    // (Two worlds: two ships that close on each other this fast would be "something near" each other.)
    let (mut away_world, _) = space();
    let going_away = ship_at(
        &mut away_world,
        [10_000.0, 0.0, 0.0],
        [12_000.0, 0.0, 1_000.0],
    );
    let (mut in_world, _) = space();
    let coming_in = ship_at(
        &mut in_world,
        [0.0, 10_000.0, 0.0],
        [0.0, -12_000.0, 1_000.0],
    );
    assert!(
        away_world
            .coasting_rail(going_away, RailTime::of_tick(0, 20))
            .is_ok()
    );
    assert!(matches!(
        refused(&in_world, coming_in),
        NotCoasting::TooLow { .. }
    ));
    for tick in 0..=21 {
        server_tick(&mut away_world, tick);
        server_tick(&mut in_world, tick);
    }
    assert!(rail_of(&away_world, going_away).is_some());
    assert!(rail_of(&in_world, coming_in).is_none());
    // The one going away is on a hyperbola that goes through the planet's ground, behind it.
    let orbit = orbit_of(&away_world, going_away);
    assert!(orbit.is_a_hyperbola() && orbit.nearest_distance() < 3_200.0);
}

#[test]
fn a_planet_that_weighs_less_is_answered_at_once_by_a_ship_on_a_hyperbola() {
    // Coming in, 9,478 voxels at its nearest: no apoapsis to wait for, and a hyperbola made less bound cannot be thrown out of anything.
    let (mut world, ship, planet) =
        a_ship_on_a_hyperbola([10_000.0, 0.0, 0.0], [-3_000.0, 0.0, 12_000.0]);
    let old = rail_of(&world, ship);
    assert!(orbit_of(&world, ship).is_a_hyperbola());
    world.set_grid_mass(planet, 0.8e22, [0.0; 3]).unwrap();
    let tick = until_the_rail_changes(&mut world, ship, old, 21, 400).expect("answered");
    assert_eq!(tick, 22);
    let after = orbit_of(&world, ship);
    assert!(after.is_a_hyperbola());
    assert!((after.mass_parameter() / (PULL * 0.8e22) - 1.0).abs() < 1e-12);
}

#[test]
fn a_planet_that_weighs_more_is_answered_at_the_periapsis_if_it_is_ahead_and_at_once_if_it_is_behind()
 {
    let (mut ahead, ship, planet) =
        a_ship_on_a_hyperbola([10_000.0, 0.0, 0.0], [-3_000.0, 0.0, 12_000.0]);
    let old = rail_of(&ahead, ship);
    let before = orbit_of(&ahead, ship);
    let periapsis = ticks_of_the_end(before.clone(), 21, 400, true);
    assert_eq!(periapsis.len(), 1, "a hyperbola has one periapsis");
    assert!(
        periapsis[0] > 22,
        "still ahead at the tick the mass changes: {periapsis:?}"
    );
    ahead.set_grid_mass(planet, 1.5e22, [0.0; 3]).unwrap();
    let tick = until_the_rail_changes(&mut ahead, ship, old, 21, 400).expect("answered");
    assert_eq!(tick, periapsis[0], "answered at the periapsis");
    assert!((orbit_of(&ahead, ship).mass_parameter() / (PULL * 1.5e22) - 1.0).abs() < 1e-12);
    // The same ship once the periapsis is behind it: nothing to wait for.
    let (mut behind, ship, planet) =
        a_ship_on_a_hyperbola([10_000.0, 0.0, 0.0], [-3_000.0, 0.0, 12_000.0]);
    for tick in 22..periapsis[0] + 5 {
        server_tick(&mut behind, tick);
    }
    let old = rail_of(&behind, ship);
    assert!(!orbit_of(&behind, ship).periapsis_ahead(RailTime::of_tick(periapsis[0] + 5, 20)));
    behind.set_grid_mass(planet, 1.5e22, [0.0; 3]).unwrap();
    let tick = until_the_rail_changes(&mut behind, ship, old, periapsis[0] + 4, periapsis[0] + 40)
        .expect("answered");
    assert_eq!(tick, periapsis[0] + 5, "answered at the next tick");
}
