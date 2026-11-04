//! Ships that leave the sphere of influence of the planet they go round (E16 phase B of `docs/orbits-and-thrust-plan.md`): a ship on an orbit round a planet that goes round a star is not refused
//! for going out of the planet's sphere; at the **instant** it crosses (found in closed form, on a grid of 2^-31 of a second) it goes on the orbit round the star that its state is on, however
//! many ticks that is after the one before, and whatever the tick rate is.
//!
//! Most of these run the clock and put the ships on their rails (`apply_rails`) and nothing else, so that what they say is about the path; one runs the rules of coasting as well (a ship
//! whose neighbour is a planet it is near within a second of its speed comes off its rail), at the scale of a real system (a second is a small part of the time to cross a sphere).

use super::coasting_ships::{PULL, orbit_of, rail_of, refused};
use super::*;
use crate::RailSettings;
use crate::server_world::NotCoasting;

/// The star is a million times as heavy as the planet, and the planet a thousand times as heavy as the moon.
pub(super) const STAR_MASS: f64 = 6.0e27;
pub(super) const PLANET_MASS: f64 = 6.0e21;
pub(super) const MOON_MASS: f64 = 6.0e18;
/// The planet goes round the star 10^7 voxels out; its sphere is 1e7 (1e-6)^(2/5) = 39,810 voxels.
pub(super) const PLANET_DISTANCE: f64 = 1.0e7;
/// The moon goes round the planet 30,000 voxels out; its sphere is 3e4 (1e-3)^(2/5) = 1,893 voxels.
pub(super) const MOON_DISTANCE: f64 = 3.0e4;

/// The time all the orbits here start at: the first tick, of a clock of 20 ticks a second whatever the rate the world is run at (the same moment as the first tick of any other).
pub(super) fn start() -> RailTime {
    RailTime::of_tick(0, 20)
}

pub(super) fn circular_speed(mu: f64, distance: f64) -> f64 {
    libm::sqrt(mu / distance)
}

pub(super) fn gravity_of_these_worlds() -> GravitySettings {
    GravitySettings {
        base: [0.0; 3],
        pull_strength: PULL,
        pull_mass_threshold: Some(1.0e15),
        softening: 1.0,
    }
}

/// A star with a ground of `star_reach` bricks, and a planet of a millionth of its weight that goes round it in a circle; the clock is at 0.
pub(super) fn a_star_with_ground_and_a_planet(star_reach: u32) -> (ServerWorld, GridId, GridId) {
    let mut world = world_for_rails();
    world.set_gravity_settings(gravity_of_these_worlds());
    world.set_level_gravity(LevelIndex::MAIN, [0.0; 3]);
    let star = planet_on(&mut world, star_reach, STAR_MASS, None);
    let mu = PULL * STAR_MASS;
    let orbit = Orbit::from_place(
        Centre::Grid(star),
        [PLANET_DISTANCE, 0.0, 0.0],
        [0.0, circular_speed(mu, PLANET_DISTANCE), 0.0],
        mu,
        start(),
    )
    .unwrap();
    let planet = planet_on(
        &mut world,
        100,
        PLANET_MASS,
        Some(Rail {
            spin: None,
            orbit: Some(orbit),
        }),
    );
    world.data.set_clock(0);
    world.apply_rails(20);
    (world, star, planet)
}

pub(super) fn a_star_and_a_planet() -> (ServerWorld, GridId, GridId) {
    a_star_with_ground_and_a_planet(1000)
}

/// The same, with a moon of a thousandth of the planet's weight that goes round the planet in a circle.
pub(super) fn a_star_a_planet_and_a_moon() -> (ServerWorld, GridId, GridId, GridId) {
    let (mut world, star, planet) = a_star_and_a_planet();
    let mu = PULL * PLANET_MASS;
    let orbit = Orbit::from_place(
        Centre::Grid(planet),
        [MOON_DISTANCE, 0.0, 0.0],
        [0.0, circular_speed(mu, MOON_DISTANCE), 0.0],
        mu,
        start(),
    )
    .unwrap();
    let moon = planet_on(
        &mut world,
        10,
        MOON_MASS,
        Some(Rail {
            spin: None,
            orbit: Some(orbit),
        }),
    );
    world.apply_rails(20);
    (world, star, planet, moon)
}

/// A ship on the orbit round `planet` that this state against it is on at time 0.
pub(super) fn a_ship_round(
    world: &mut ServerWorld,
    planet: GridId,
    mass: f64,
    place: [f64; 3],
    velocity: [f64; 3],
) -> GridId {
    let ship = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    let orbit =
        Orbit::from_place(Centre::Grid(planet), place, velocity, PULL * mass, start()).unwrap();
    world
        .set_grid_rail(
            ship,
            Some(Rail {
                spin: None,
                orbit: Some(orbit),
            }),
        )
        .unwrap();
    world.apply_rails(20);
    ship
}

/// An ellipse round the planet from 9,000 voxels out to 60,000, past the sphere of 39,810: its nearest point is where it starts, going across the radius.
pub(super) fn a_ship_that_goes_out_of_the_sphere(
    world: &mut ServerWorld,
    planet: GridId,
) -> GridId {
    let mu = PULL * PLANET_MASS;
    let speed = libm::sqrt(mu * (2.0 / 9_000.0 - 1.0 / 34_500.0));
    a_ship_round(
        world,
        planet,
        PLANET_MASS,
        [9_000.0, 0.0, 0.0],
        [0.0, 0.0, speed],
    )
}

/// The clock at this tick of a clock of this rate, and the ships put on their rails: all that happens to a ship that is not disturbed.
pub(super) fn tick_at(world: &mut ServerWorld, tick: u64, ticks_per_second: u32) {
    world.data.set_clock(tick);
    world.apply_rails(ticks_per_second);
}

/// A tick of the server: the same, and the rules of coasting between the two puttings.
pub(super) fn tick_with_the_rules_at(world: &mut ServerWorld, tick: u64, ticks_per_second: u32) {
    world.data.set_clock(tick);
    world.apply_rails(ticks_per_second);
    world.update_coasting(ticks_per_second);
    world.apply_rails(ticks_per_second);
}

/// Ticks 1 to the tick that is `seconds` into the world at this rate.
pub(super) fn run_until(world: &mut ServerWorld, seconds: u32, ticks_per_second: u32) {
    run_from_to(
        world,
        1,
        u64::from(seconds) * u64::from(ticks_per_second),
        ticks_per_second,
    );
}

pub(super) fn run_from_to(world: &mut ServerWorld, first: u64, last: u64, ticks_per_second: u32) {
    for tick in first..=last {
        tick_at(world, tick, ticks_per_second);
    }
}

pub(super) fn position_of(world: &ServerWorld, ship: GridId) -> WorldPosition {
    world.data().grids().get(ship).unwrap().pose.position
}

pub(super) fn the_sphere_of(world: &ServerWorld, planet: GridId) -> f64 {
    world.sphere_of_influence(planet).expect("it has one")
}

/// When the ship's orbit leaves the planet's sphere, in seconds into the world.
fn seconds_to_leave(world: &ServerWorld, planet: GridId, orbit: &Orbit) -> f64 {
    let instant = orbit
        .leaves_at(the_sphere_of(world, planet), orbit.since.unwrap())
        .unwrap()
        .expect("it leaves");
    instant.units() as f64 / f64::from(instant.units_per_second())
}

#[test]
fn an_orbit_that_goes_out_of_the_sphere_is_a_rail_where_it_was_refused() {
    let (mut world, _, planet) = a_star_and_a_planet();
    // The ship is put where it is from the planet with the planet's speed and its own, as a free ship is.
    let there = position_of(&world, planet);
    let planet_speed = world.data().grids().get(planet).unwrap().linear_velocity;
    let mu = PULL * PLANET_MASS;
    let speed = libm::sqrt(mu * (2.0 / 9_000.0 - 1.0 / 34_500.0));
    let ship = world
        .create_grid(
            LevelIndex::MAIN,
            GridPose::new(there.translated([9_000.0, 0.0, 0.0]), Orientation::IDENTITY),
        )
        .unwrap();
    world
        .set_grid_velocity(
            ship,
            [planet_speed[0], planet_speed[1], planet_speed[2] + speed],
            [0.0; 3],
        )
        .unwrap();
    let rail = world
        .coasting_rail(ship, RailTime::of_tick(0, 20))
        .expect("a rail");
    let orbit = rail.orbit.unwrap();
    assert_eq!(orbit.around, Centre::Grid(planet));
    assert!((orbit.farthest_distance() / 60_000.0 - 1.0).abs() < 1e-3);
    assert!(orbit.farthest_distance() > the_sphere_of(&world, planet));
    assert_eq!(orbit.since, Some(RailTime::of_tick(0, 20)));
}

#[test]
fn a_ship_that_is_outside_the_sphere_of_its_planet_is_not_put_on_a_rail_round_it() {
    // A star that is not held and is on no rail pulls nothing that a rail could go round, so that the planet is the one that pulls the ship most wherever it is.
    let (mut world, star, planet) = a_star_and_a_planet();
    world.set_grid_held(star, false).unwrap();
    let there = position_of(&world, planet);
    let planet_speed = world.data().grids().get(planet).unwrap().linear_velocity;
    let speed = circular_speed(PULL * PLANET_MASS, 50_000.0);
    let ship = world
        .create_grid(
            LevelIndex::MAIN,
            GridPose::new(
                there.translated([50_000.0, 0.0, 0.0]),
                Orientation::IDENTITY,
            ),
        )
        .unwrap();
    world
        .set_grid_velocity(
            ship,
            [planet_speed[0], planet_speed[1], planet_speed[2] + speed],
            [0.0; 3],
        )
        .unwrap();
    match refused(&world, ship) {
        NotCoasting::PastTheSphere { sphere, .. } => {
            assert!((sphere / 39_810.0 - 1.0).abs() < 1e-2, "{sphere}");
        }
        other => panic!("{other:?}"),
    }
    // Inside the sphere, a circle is a rail round the planet.
    world
        .set_grid_velocity(
            ship,
            [
                planet_speed[0],
                planet_speed[1],
                planet_speed[2] + circular_speed(PULL * PLANET_MASS, 50_000.0),
            ],
            [0.0; 3],
        )
        .unwrap();
    world
        .set_grid_pose(
            ship,
            GridPose::new(there.translated([9_000.0, 0.0, 0.0]), Orientation::IDENTITY),
        )
        .unwrap();
    world
        .set_grid_velocity(
            ship,
            [
                planet_speed[0],
                planet_speed[1],
                planet_speed[2] + circular_speed(PULL * PLANET_MASS, 9_000.0),
            ],
            [0.0; 3],
        )
        .unwrap();
    assert_eq!(
        world
            .coasting_rail(ship, RailTime::of_tick(0, 20))
            .unwrap()
            .orbit
            .unwrap()
            .around,
        Centre::Grid(planet)
    );
}

#[test]
fn a_ship_that_leaves_the_sphere_goes_on_round_the_star_from_the_instant_it_crosses() {
    let (mut world, star, planet) = a_star_and_a_planet();
    let ship = a_ship_that_goes_out_of_the_sphere(&mut world, planet);
    let before = orbit_of(&world, ship);
    let seconds = seconds_to_leave(&world, planet, &before);
    // (About a quarter of the 63 seconds of the ellipse: it is a good part of a second between the ticks it is in either orbit.)
    assert!(seconds > 5.0 && seconds < 30.0, "{seconds}");
    let tick_before = (seconds * 20.0).floor() as u64;
    run_from_to(&mut world, 1, tick_before, 20);
    assert_eq!(orbit_of(&world, ship).around, Centre::Grid(planet));
    assert_eq!(world.rail_work().hand_overs, 0);
    tick_at(&mut world, tick_before + 1, 20);
    let after = orbit_of(&world, ship);
    assert_eq!(after.around, Centre::Grid(star));
    assert_eq!(world.rail_work().hand_overs, 1);
    // The orbit is the one the pure function makes, from the instant (not from the tick), and starts at it.
    let instant = before
        .leaves_at(the_sphere_of(&world, planet), before.since.unwrap())
        .unwrap()
        .unwrap();
    let planet_orbit = orbit_of(&world, planet);
    let expected = before
        .handed_over_to(&planet_orbit, PULL * STAR_MASS, instant, 65_536)
        .unwrap();
    assert_eq!(after, expected);
    assert_eq!(after.since, Some(instant));
    assert_eq!(after.elements.radius.precision().bits(), 160);
    // It goes on round the star, tick after tick.
    run_from_to(&mut world, tick_before + 2, tick_before + 400, 20);
    assert_eq!(orbit_of(&world, ship), expected);
}

#[test]
fn the_path_is_the_same_whatever_the_tick_rate_is() {
    // Forty seconds at three rates, none a multiple of another: the orbit the ship is on and where it is are the same, to the bit.
    let mut results = Vec::new();
    for ticks_per_second in [20, 100, 7] {
        let (mut world, _, planet) = a_star_and_a_planet();
        let ship = a_ship_that_goes_out_of_the_sphere(&mut world, planet);
        run_until(&mut world, 40, ticks_per_second);
        results.push((orbit_of(&world, ship), position_of(&world, ship)));
        assert_eq!(
            world.rail_work().hand_overs,
            1,
            "{ticks_per_second} a second"
        );
    }
    let (orbit, position) = &results[0];
    for (other, other_position) in &results[1..] {
        assert_eq!(other, orbit);
        assert_eq!(other_position, position);
    }
}

#[test]
fn a_ship_that_crosses_two_spheres_between_two_ticks_is_where_it_is_if_it_had_been_seen_in_each() {
    // A moon's ship goes out of the moon's sphere in a hundredth of a second and out of the planet's in another half: with one tick a second both are due at once.
    let mut results = Vec::new();
    for ticks_per_second in [1, 50] {
        let (mut world, star, planet, moon) = a_star_a_planet_and_a_moon();
        let ship = a_ship_round(
            &mut world,
            moon,
            MOON_MASS,
            [0.0, 600.0, 0.0],
            [5.0e4, 0.0, 0.0],
        );
        assert_eq!(orbit_of(&world, ship).around, Centre::Grid(moon));
        // One second in (one tick at the slow rate): both crossings are behind it, in the one tick.
        run_until(&mut world, 1, ticks_per_second);
        assert_eq!(orbit_of(&world, ship).around, Centre::Grid(star));
        assert_eq!(world.rail_work().hand_overs, 2);
        run_from_to(
            &mut world,
            u64::from(ticks_per_second) + 1,
            5 * u64::from(ticks_per_second),
            ticks_per_second,
        );
        let orbit = orbit_of(&world, ship);
        assert_eq!(
            orbit.around,
            Centre::Grid(star),
            "{ticks_per_second} a second"
        );
        assert_eq!(world.rail_work().hand_overs, 2);
        // One hand-over adds 32 bits to the 128 of the moon's ship, and the next 32 more.
        assert_eq!(orbit.elements.radius.precision().bits(), 192);
        // The two crossings were at instants, and the second is after the first.
        let in_the_planets_frame = ship_in_the_planets_frame(&world, planet, moon);
        assert!(
            in_the_planets_frame
                .since
                .unwrap()
                .compare(orbit.since.unwrap())
                .is_lt()
        );
        results.push((orbit, position_of(&world, ship)));
    }
    assert_eq!(results[0].0, results[1].0);
    assert_eq!(results[0].1, results[1].1);
}

/// The orbit round the planet that a ship on the orbit of `a_ship_round(moon, [0, 600, 0], [5e4, 0, 0])` is on after it leaves the moon's sphere: made again, by the pure functions.
fn ship_in_the_planets_frame(world: &ServerWorld, planet: GridId, moon: GridId) -> Orbit {
    let orbit = Orbit::from_place(
        Centre::Grid(moon),
        [0.0, 600.0, 0.0],
        [5.0e4, 0.0, 0.0],
        PULL * MOON_MASS,
        start(),
    )
    .unwrap();
    let sphere = the_sphere_of(world, moon);
    let instant = orbit.leaves_at(sphere, start()).unwrap().unwrap();
    let _ = planet;
    orbit
        .handed_over_to(&orbit_of(world, moon), PULL * PLANET_MASS, instant, 65_536)
        .unwrap()
}

#[test]
fn a_ship_that_was_not_looked_at_for_a_long_time_is_on_the_path_it_would_have_been_on() {
    // A ship parked since time 0 and found at a time long after it crossed (the clock jumps) is where the ship that was looked at every tick is: the crossing is found after the time its
    // orbit starts, and not after the time it is looked at.
    let (mut watched, _, planet) = a_star_and_a_planet();
    let ship = a_ship_that_goes_out_of_the_sphere(&mut watched, planet);
    run_until(&mut watched, 40, 20);

    let (mut parked, _, planet) = a_star_and_a_planet();
    let same_ship = a_ship_that_goes_out_of_the_sphere(&mut parked, planet);
    tick_at(&mut parked, 40 * 20, 20);

    assert_eq!(orbit_of(&parked, same_ship), orbit_of(&watched, ship));
    assert_eq!(position_of(&parked, same_ship), position_of(&watched, ship));
    assert_eq!(parked.rail_work().hand_overs, 1);
}

#[test]
fn the_instant_is_worked_out_once_for_an_orbit_that_can_leave_and_never_for_one_that_cannot() {
    // A ship in a circle round the planet stays inside its sphere: the instant it would leave is never worked out.
    let (mut world, _, planet) = a_star_and_a_planet();
    let circular = circular_speed(PULL * PLANET_MASS, 9_000.0);
    a_ship_round(
        &mut world,
        planet,
        PLANET_MASS,
        [0.0, 9_000.0, 0.0],
        [circular, 0.0, 0.0],
    );
    run_until(&mut world, 5, 20);
    assert_eq!(world.rail_work().exit_searches, 0);
    // One that goes out of it has it worked out once, for all the ticks before it crosses.
    let (mut world, _, planet) = a_star_and_a_planet();
    let ship = a_ship_that_goes_out_of_the_sphere(&mut world, planet);
    let seconds = seconds_to_leave(&world, planet, &orbit_of(&world, ship));
    run_from_to(&mut world, 1, (seconds * 20.0) as u64 - 1, 20);
    assert_eq!(world.rail_work().exit_searches, 1);
    assert_eq!(world.rail_work().hand_overs, 0);
    // The orbit it is on after the crossing cannot leave anything (the star has no sphere): no search for it.
    run_until(&mut world, 40, 20);
    assert_eq!(world.rail_work().hand_overs, 1);
    assert_eq!(world.rail_work().exit_searches, 1);
}

#[test]
fn a_sphere_that_changes_moves_the_instant_a_ship_leaves_it_at() {
    // The star weighs half what it did, so the planet's sphere is 1.32 times as wide: the ship crosses it later, and is handed over then and not at the old instant.
    let (mut world, star, planet) = a_star_and_a_planet();
    let ship = a_ship_that_goes_out_of_the_sphere(&mut world, planet);
    let before = orbit_of(&world, ship);
    let old_seconds = seconds_to_leave(&world, planet, &before);
    run_from_to(&mut world, 1, 5, 20);
    assert_eq!(world.rail_work().exit_searches, 1);
    world
        .set_grid_mass(star, STAR_MASS / 2.0, [0.0; 3])
        .unwrap();
    let new_sphere = the_sphere_of(&world, planet);
    assert!(new_sphere > 1.3 * 39_810.0 && new_sphere < before.farthest_distance());
    let new_instant = before
        .leaves_at(new_sphere, before.since.unwrap())
        .unwrap()
        .unwrap();
    let new_seconds = new_instant.units() as f64 / f64::from(new_instant.units_per_second());
    assert!(
        new_seconds > old_seconds + 1.0,
        "{new_seconds} {old_seconds}"
    );
    // At the old instant it is still round the planet.
    let old_tick = (old_seconds * 20.0).ceil() as u64;
    run_from_to(&mut world, 6, old_tick + 1, 20);
    assert_eq!(orbit_of(&world, ship).around, Centre::Grid(planet));
    assert_eq!(world.rail_work().hand_overs, 0);
    // And at the new it goes round the star, with the mass it has now.
    run_from_to(
        &mut world,
        old_tick + 2,
        (new_seconds * 20.0).ceil() as u64 + 1,
        20,
    );
    let after = orbit_of(&world, ship);
    assert_eq!(after.around, Centre::Grid(star));
    assert_eq!(after.since, Some(new_instant));
    let expected = before
        .handed_over_to(
            &orbit_of(&world, planet),
            PULL * STAR_MASS / 2.0,
            new_instant,
            65_536,
        )
        .unwrap();
    assert_eq!(after, expected);
    assert_eq!(world.rail_work().exit_searches, 2);
}

#[test]
fn a_ship_put_on_another_orbit_is_handed_over_at_the_instant_of_that_one() {
    // The instant kept for an orbit is for that orbit: another, round the same planet and with the same sphere, has its own.
    let (mut world, star, planet) = a_star_and_a_planet();
    let ship = a_ship_that_goes_out_of_the_sphere(&mut world, planet);
    let first = orbit_of(&world, ship);
    run_from_to(&mut world, 1, 5, 20);
    assert_eq!(world.rail_work().exit_searches, 1);
    // A faster one from 9,000 voxels out: it goes out of the sphere sooner. It starts at tick 5.
    let mu = PULL * PLANET_MASS;
    let faster = Orbit::from_place(
        Centre::Grid(planet),
        [9_000.0, 0.0, 0.0],
        [
            0.0,
            0.0,
            1.2 * libm::sqrt(mu * (2.0 / 9_000.0 - 1.0 / 34_500.0)),
        ],
        mu,
        RailTime::of_tick(5, 20),
    )
    .unwrap();
    let instant = faster
        .leaves_at(the_sphere_of(&world, planet), faster.since.unwrap())
        .unwrap()
        .unwrap();
    let first_instant = first
        .leaves_at(the_sphere_of(&world, planet), first.since.unwrap())
        .unwrap()
        .unwrap();
    assert!(instant.compare(first_instant).is_lt());
    world
        .set_grid_rail(
            ship,
            Some(Rail {
                spin: None,
                orbit: Some(faster.clone()),
            }),
        )
        .unwrap();
    let seconds = instant.units() as f64 / f64::from(instant.units_per_second());
    run_from_to(&mut world, 6, (seconds * 20.0).ceil() as u64, 20);
    let after = orbit_of(&world, ship);
    assert_eq!(after.around, Centre::Grid(star));
    assert_eq!(after.since, Some(instant));
    assert_eq!(world.rail_work().exit_searches, 2);
}

#[test]
fn what_is_kept_of_a_ship_that_is_gone_is_let_go() {
    let (mut world, _, planet) = a_star_and_a_planet();
    let ship = a_ship_that_goes_out_of_the_sphere(&mut world, planet);
    run_from_to(&mut world, 1, 3, 20);
    assert_eq!(world.exits.len(), 1);
    world.remove_grid(ship);
    run_from_to(&mut world, 4, 5, 20);
    assert!(world.exits.is_empty());
}

#[test]
fn the_widest_rail_a_host_allows_is_the_most_bits_a_hand_over_makes() {
    for (widest, expected) in [(65_536, 160), (150, 150), (128, 128)] {
        let (mut world, _, planet) = a_star_and_a_planet();
        world.set_rail_settings(
            RailSettings::default()
                .with_widest_rail_bits(widest)
                .unwrap(),
        );
        let ship = a_ship_that_goes_out_of_the_sphere(&mut world, planet);
        run_until(&mut world, 40, 20);
        let orbit = orbit_of(&world, ship);
        assert!(matches!(orbit.around, Centre::Grid(_)));
        assert_eq!(world.rail_work().hand_overs, 1);
        assert_eq!(
            orbit.elements.radius.precision().bits(),
            expected,
            "widest {widest}"
        );
    }
}

#[test]
fn an_orbit_round_the_star_that_comes_lower_than_its_ground_takes_the_ship_off_its_rail_where_it_is()
 {
    // A star whose ground reaches farther than the planet is (an absurd star, to make the star's orbit too low without a flyby of a hundred million voxels a second).
    let (mut world, _, planet) = a_star_with_ground_and_a_planet(400_000);
    let ship = a_ship_that_goes_out_of_the_sphere(&mut world, planet);
    run_until(&mut world, 40, 20);
    assert_eq!(rail_of(&world, ship), None);
    assert_eq!(world.rail_work().hand_overs, 0);
    // It goes on as it was going: the speed the rail gave it last.
    let speed = world.data().grids().get(ship).unwrap().linear_velocity;
    assert!(speed.iter().any(|part| *part != 0.0));
}

#[test]
fn a_ship_that_has_left_a_sphere_stays_on_its_rail_with_the_rules_of_coasting_asked_every_tick() {
    let (mut world, star, planet) = a_star_and_a_planet();
    let ship = a_ship_that_goes_out_of_the_sphere(&mut world, planet);
    for tick in 1..=40 * 20 {
        tick_with_the_rules_at(&mut world, tick, 20);
    }
    assert_eq!(orbit_of(&world, ship).around, Centre::Grid(star));
    assert_eq!(world.rail_work().hand_overs, 1);
}

#[test]
fn a_ship_on_a_rail_is_in_a_sphere_only_when_it_is_inside_it_by_more_than_the_rounding_of_its_place()
 {
    // A ship that has come out of a sphere at the instant it crossed is at its edge to the bit, and the rail round the star must not take it for a ship that went into it.
    let at_a_fraction_of_the_sphere = |fraction: f64| {
        let (mut world, star, planet) = a_star_and_a_planet();
        let sphere = the_sphere_of(&world, planet);
        let planet_speed = world.data().grids().get(planet).unwrap().linear_velocity;
        let ship = world
            .create_grid(
                LevelIndex::MAIN,
                GridPose::new(
                    position_of(&world, planet).translated([sphere * fraction, 0.0, 0.0]),
                    Orientation::IDENTITY,
                ),
            )
            .unwrap();
        // (It goes with the planet.)
        world
            .set_grid_velocity(ship, planet_speed, [0.0; 3])
            .unwrap();
        let body = world.data().grids().get(ship).unwrap();
        (world.near_with(None, ship, body, Some(star), 1.0), planet)
    };
    assert_eq!(at_a_fraction_of_the_sphere(1.0).0, None);
    assert_eq!(at_a_fraction_of_the_sphere(1.0 - 1.0e-12).0, None);
    let (near, planet) = at_a_fraction_of_the_sphere(1.0 - 1.0e-6);
    assert_eq!(near, Some(planet));
    assert_eq!(at_a_fraction_of_the_sphere(0.5).0, Some(planet));
}
