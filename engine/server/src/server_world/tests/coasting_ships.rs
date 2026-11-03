//! Ships that coast (`server_world/coasting.rs`, step B of `docs/orbits-and-thrust-plan.md`): a ship nothing disturbs goes on a rail round the planet that pulls it most, and a
//! ship on one comes off it when it is pushed, moved, held, or something comes near. These are the rules, with no bodies to move anything: the bodies leave a ship on a rail
//! where it is put (`grid_bodies`), and what it does with one is tested with them.

use super::*;
use crate::server_world::NotCoasting;

/// The `G` of these worlds, in voxels cubed per kilogram per second squared.
const PULL: f64 = 6.674e-11;
/// How many ticks a second.
const TICKS: u32 = 20;
/// The ground of the planets here reaches a hundred bricks, 3,200 voxels.
const REACH: f64 = 3_200.0;

fn time(tick: u64) -> RailTime {
    RailTime::of_tick(tick, TICKS)
}

/// A world with no pull of its own in its level, where anything of a mass of a quadrillion kilograms pulls, and a planet of ten sextillion (6.7e11 is its `G M`).
fn space() -> (ServerWorld, GridId) {
    let mut world = world_for_rails();
    world.set_gravity_settings(GravitySettings {
        base: [0.0; 3],
        pull_strength: PULL,
        pull_mass_threshold: Some(1.0e15),
        softening: 1.0,
    });
    world.set_level_gravity(LevelIndex::MAIN, [0.0; 3]);
    let planet = planet_on(&mut world, 100, 1.0e22, None);
    (world, planet)
}

/// The speed of a circle at this distance from a planet of this mass.
fn circular_speed(mass: f64, distance: f64) -> f64 {
    libm::sqrt(PULL * mass / distance)
}

/// A ship at this place (in voxels from the middle of the planet, which is at the origin) going at this speed.
fn ship_at(world: &mut ServerWorld, place: [f64; 3], velocity: [f64; 3]) -> GridId {
    let ship = world
        .create_grid(
            LevelIndex::MAIN,
            GridPose::new(WorldPosition::from_voxels(place), Orientation::IDENTITY),
        )
        .unwrap();
    world.set_grid_velocity(ship, velocity, [0.0; 3]).unwrap();
    ship
}

/// A ship in a circle round the planet of `space`, at this distance, going the way of +z at the place along +x.
fn ship_in_a_circle(world: &mut ServerWorld, distance: f64) -> GridId {
    ship_at(
        world,
        [distance, 0.0, 0.0],
        [0.0, 0.0, circular_speed(1.0e22, distance)],
    )
}

fn refused(world: &ServerWorld, ship: GridId) -> NotCoasting {
    world.coasting_rail(ship, time(0)).unwrap_err()
}

fn rail_of(world: &ServerWorld, ship: GridId) -> Option<Rail> {
    world.data().grids().get(ship).unwrap().rail.clone()
}

fn distance_between(first: WorldPosition, second: WorldPosition) -> f64 {
    let apart = first.displacement_to(second).in_voxels();
    libm::sqrt(apart[0] * apart[0] + apart[1] * apart[1] + apart[2] * apart[2])
}

#[test]
fn a_ship_in_a_circle_is_given_the_orbit_it_is_on_and_put_there_by_it_it_is_where_it_was() {
    let (mut world, planet) = space();
    let ship = ship_in_a_circle(&mut world, 10_000.0);
    let before = world.data().grids().get(ship).unwrap().clone();
    let rail = world.coasting_rail(ship, time(0)).unwrap();
    assert_eq!(rail.spin, None);
    let orbit = rail.orbit.clone().unwrap();
    assert_eq!(orbit.around, Centre::Grid(planet));
    assert!((orbit.radius() / 10_000.0 - 1.0).abs() < 1e-12, "{orbit:?}");
    assert!(orbit.ratio() > 1.0 - 1e-9, "{orbit:?}");
    let period = std::f64::consts::TAU * libm::sqrt(1.0e12 / (PULL * 1.0e22));
    assert!((orbit.period() / period - 1.0).abs() < 1e-12, "{orbit:?}");
    // On the rail it is where it was, going as it went: the rail is the orbit of its state, to the working of the fit.
    world.set_grid_rail(ship, Some(rail)).unwrap();
    world.apply_rails(TICKS);
    let after = world.data().grids().get(ship).unwrap();
    assert!(
        distance_between(before.pose.position, after.pose.position) < 1e-6,
        "{:?} then {:?}",
        before.pose.position,
        after.pose.position
    );
    for axis in 0..3 {
        assert!(
            (after.linear_velocity[axis] - before.linear_velocity[axis]).abs() < 1e-6,
            "{:?} then {:?}",
            before.linear_velocity,
            after.linear_velocity
        );
    }
}

#[test]
fn it_is_the_centre_of_mass_of_a_ship_that_is_on_its_orbit() {
    let (mut world, planet) = space();
    let middle = [2.0, 1.0, -3.0];
    // A ship whose origin is 10,000 along x, whose centre of mass is a little off it: its centre of mass is the place that goes round.
    let ship = ship_in_a_circle(&mut world, 10_000.0);
    world.set_grid_mass(ship, 5.0e3, middle).unwrap();
    let rail = world.coasting_rail(ship, time(0)).unwrap();
    let orbit = rail.orbit.clone().unwrap();
    let centre = world
        .data()
        .grids()
        .get(ship)
        .unwrap()
        .pose
        .local_to_world(WorldPosition::new(BrickCoordinate::ORIGIN, middle))
        .unwrap();
    let from_the_planet = world
        .data()
        .grids()
        .get(planet)
        .unwrap()
        .pose
        .position
        .displacement_to(centre)
        .in_voxels();
    let distance = libm::sqrt(from_the_planet.iter().map(|part| part * part).sum());
    assert!(
        (orbit.radius() / distance - 1.0).abs() < 1e-3,
        "the radius {} is not the centre of mass's distance {distance}",
        orbit.radius()
    );
    // Put on the rail, the origin is where it was and the centre of mass is on the orbit.
    let origin = world.data().grids().get(ship).unwrap().pose.position;
    world.set_grid_rail(ship, Some(rail)).unwrap();
    world.apply_rails(TICKS);
    let after = world.data().grids().get(ship).unwrap();
    assert!(
        distance_between(origin, after.pose.position) < 1e-6,
        "{origin:?} then {:?}",
        after.pose.position
    );
}

#[test]
fn what_a_ship_cannot_coast_for_is_said() {
    let (mut world, planet) = space();
    let ship = ship_in_a_circle(&mut world, 10_000.0);
    // A planet, and a ship that is not there.
    assert_eq!(refused(&world, planet), NotCoasting::NotAShip);
    let nowhere = GridId::new(GridId::FIRST_ALLOCATED + 99);
    assert_eq!(refused(&world, nowhere), NotCoasting::NotAShip);
    // Held (which stops a ship: it goes on from the speed it had).
    let speed = world.data().grids().get(ship).unwrap().linear_velocity;
    world.set_grid_held(ship, true).unwrap();
    assert_eq!(refused(&world, ship), NotCoasting::Held);
    world.set_grid_held(ship, false).unwrap();
    world.set_grid_velocity(ship, speed, [0.0; 3]).unwrap();
    // Turning.
    world
        .set_grid_velocity(ship, speed, [0.0, 0.001, 0.0])
        .unwrap();
    assert_eq!(refused(&world, ship), NotCoasting::Turning);
    world.set_grid_velocity(ship, speed, [0.0; 3]).unwrap();
    // At rest, and fast enough to leave: no orbit.
    world.set_grid_velocity(ship, [0.0; 3], [0.0; 3]).unwrap();
    assert!(matches!(refused(&world, ship), NotCoasting::NoOrbit(_)));
    world
        .set_grid_velocity(ship, speed.map(|part| part * 2.0), [0.0; 3])
        .unwrap();
    assert!(matches!(refused(&world, ship), NotCoasting::NoOrbit(_)));
    world.set_grid_velocity(ship, speed, [0.0; 3]).unwrap();
    assert!(world.coasting_rail(ship, time(0)).is_ok());
    // A level with a ground of its own.
    world.set_level_gravity(LevelIndex::MAIN, [0.0, -5.0, 0.0]);
    assert_eq!(refused(&world, ship), NotCoasting::LevelPulls);
    world.set_level_gravity(LevelIndex::MAIN, [0.0; 3]);
    // A planet that is too light to pull, and no threshold at all (nothing pulls).
    world.set_gravity_settings(GravitySettings {
        pull_mass_threshold: Some(1.0e23),
        ..*world.gravity_settings()
    });
    assert_eq!(refused(&world, ship), NotCoasting::NoPlanetPulls);
    world.set_gravity_settings(GravitySettings {
        pull_mass_threshold: None,
        ..*world.gravity_settings()
    });
    assert_eq!(refused(&world, ship), NotCoasting::NoPlanetPulls);
}

#[test]
fn a_planet_made_after_a_ship_is_not_what_it_goes_round() {
    // A ship goes round a planet with a lower number than its own, so that the planet's place is worked out first.
    let (mut world, _) = space();
    let early = ship_in_a_circle(&mut world, 10_000.0);
    let heavier = planet_on(&mut world, 100, 1.0e30, None);
    world
        .set_grid_pose(
            heavier,
            GridPose::new(
                WorldPosition::from_voxels([1.0e9, 0.0, 0.0]),
                Orientation::IDENTITY,
            ),
        )
        .unwrap();
    let orbit = world.coasting_rail(early, time(0)).unwrap().orbit.unwrap();
    assert_ne!(orbit.around, Centre::Grid(heavier));
}

#[test]
fn an_orbit_that_comes_lower_than_the_ground_and_a_margin_is_refused() {
    let (mut world, _) = space();
    // The ground reaches 3,200 and the margin is 64: a circle at 3,250 is in the ground's margin, one at 3,300 is not.
    let low = ship_in_a_circle(&mut world, 3_250.0);
    match refused(&world, low) {
        NotCoasting::TooLow { lowest, least } => {
            assert!((lowest / 3_250.0 - 1.0).abs() < 1e-9, "{lowest}");
            assert_eq!(least, REACH + 64.0);
        }
        other => panic!("{other:?}"),
    }
    let (mut fine, _) = space();
    let clear = ship_in_a_circle(&mut fine, 3_300.0);
    assert!(fine.coasting_rail(clear, time(0)).is_ok());
    // A thin ellipse from far out whose nearest point is in the ground is refused though the ship is far from it now.
    let (mut falling, _) = space();
    let speed = circular_speed(1.0e22, 20_000.0) * 0.3;
    let ship = ship_at(&mut falling, [20_000.0, 0.0, 0.0], [0.0, 0.0, speed]);
    assert!(matches!(
        refused(&falling, ship),
        NotCoasting::TooLow { .. }
    ));
}

#[test]
fn an_orbit_that_goes_out_of_the_sphere_of_influence_of_a_planet_that_goes_round_another_is_refused()
 {
    let (mut world, _) = space();
    // A star, and a planet of a millionth of its weight that goes round it 10^7 voxels out: its sphere is 1e7 (1e24 / 1e30)^(2/5) = 39,800 voxels.
    let star = planet_on(&mut world, 1000, 1.0e30, None);
    let rail = orbit_about(Centre::Grid(star), 1.0e7, 1.0e6);
    let planet = planet_on(&mut world, 100, 1.0e24, Some(rail));
    world.apply_rails(TICKS);
    let there = world.data().grids().get(planet).unwrap().pose.position;
    let mu = PULL * 1.0e24;
    // 9,000 voxels from the planet, going fast enough to reach 60,000: well inside the sphere now and well outside it later.
    let start = there.translated([9_000.0, 0.0, 0.0]);
    let speed = libm::sqrt(mu * (2.0 / 9_000.0 - 1.0 / 34_500.0));
    let planet_speed = world.data().grids().get(planet).unwrap().linear_velocity;
    let ship = world
        .create_grid(
            LevelIndex::MAIN,
            GridPose::new(start, Orientation::IDENTITY),
        )
        .unwrap();
    let going = |extra: f64| [planet_speed[0], planet_speed[1], planet_speed[2] + extra];
    world
        .set_grid_velocity(ship, going(speed), [0.0; 3])
        .unwrap();
    match world.coasting_rail(ship, time(0)) {
        Err(NotCoasting::PastTheSphere { farthest, sphere }) => {
            assert!((farthest / 60_000.0 - 1.0).abs() < 1e-3, "{farthest}");
            assert!((sphere / 39_810.0 - 1.0).abs() < 1e-2, "{sphere}");
        }
        other => panic!("{other:?}"),
    }
    // A circle at 9,000 stays inside it: the planet is what it goes round.
    world
        .set_grid_velocity(ship, going(libm::sqrt(mu / 9_000.0)), [0.0; 3])
        .unwrap();
    let rail = world.coasting_rail(ship, time(0)).unwrap();
    assert_eq!(rail.orbit.unwrap().around, Centre::Grid(planet));
}

#[test]
fn another_ship_that_is_near_keeps_a_ship_off_a_rail_and_a_ship_on_one_keeps_it_at_half_the_room() {
    let (mut world, _) = space();
    let ship = ship_in_a_circle(&mut world, 10_000.0);
    // A ship at rest 12,000 voxels from it. The ship goes round at 8,169 voxels a second, so it closes at that, and a margin of 64 and a second of the closing speed is 8,233
    // voxels: it wants twice that to go on a rail and once that to stay on one.
    let other = ship_at(&mut world, [10_000.0, 12_000.0, 0.0], [0.0; 3]);
    assert_eq!(refused(&world, ship), NotCoasting::SomethingNear(other));
    world.remove_grid(other);
    let rail = world.coasting_rail(ship, time(0)).unwrap();
    world.set_grid_rail(ship, Some(rail.clone())).unwrap();
    let other = ship_at(&mut world, [10_000.0, 12_000.0, 0.0], [0.0; 3]);
    world.data.set_clock(0);
    world.update_coasting(TICKS);
    assert_eq!(rail_of(&world, ship), Some(rail), "it stays");
    // Nearer than once that and it comes off.
    world
        .data_mut()
        .grids_mut()
        .get_mut(other)
        .unwrap()
        .pose
        .position = WorldPosition::from_voxels([10_000.0, 6_000.0, 0.0]);
    world.update_coasting(TICKS);
    assert_eq!(rail_of(&world, ship), None);
}

#[test]
fn a_planet_on_a_rail_that_carries_what_is_near_it_keeps_a_ship_off_a_rail() {
    let mut world = world_for_rails();
    world.set_gravity_settings(GravitySettings {
        base: [0.0; 3],
        pull_strength: PULL,
        pull_mass_threshold: Some(1.0e15),
        softening: 1.0,
    });
    world.set_level_gravity(LevelIndex::MAIN, [0.0; 3]);
    // A heavy planet (the ship goes round it) and a lighter one on a rail a million voxels off that carries what is within 4,800 voxels, and a quarter more of that for a ship
    // that is to go on a rail.
    let heavy = planet_on(&mut world, 100, 1.0e30, None);
    let period = std::f64::consts::TAU * libm::sqrt(1.0e18 / (PULL * 1.0e30));
    let rail = orbit_about(Centre::Grid(heavy), 1.0e6, period);
    let light = planet_on(&mut world, 100, 1.0e18, Some(rail));
    world.apply_rails(TICKS);
    let light_at = world.data().grids().get(light).unwrap().pose.position;
    let ship = world
        .create_grid(
            LevelIndex::MAIN,
            GridPose::new(
                light_at.translated([5_500.0, 0.0, 0.0]),
                Orientation::IDENTITY,
            ),
        )
        .unwrap();
    // It goes with the light planet and a little faster, so that it is on an orbit round the heavy one and closes on the light one by 50 voxels a second, not by the speed of
    // the planets round the heavy one (a margin and a second of that is what a ship would need from a planet that goes by at orbital speed).
    let speed = world.data().grids().get(light).unwrap().linear_velocity;
    let size = libm::sqrt(speed.iter().map(|part| part * part).sum());
    world
        .set_grid_velocity(ship, speed.map(|part| part * (1.0 + 50.0 / size)), [0.0; 3])
        .unwrap();
    // 5,500 is outside 4,800 and inside 6,000.
    assert_eq!(world.influence_radius(light), Some(4_800.0));
    assert_eq!(
        world.coasting_rail(ship, time(0)),
        Err(NotCoasting::SomethingNear(light)),
        "(round {heavy:?})"
    );
}

/// A world with a ship in a circle, and the clock at 0.
fn a_circle_at_the_start_of_the_clock() -> (ServerWorld, GridId) {
    let (mut world, _) = space();
    let ship = ship_in_a_circle(&mut world, 10_000.0);
    world.data.set_clock(0);
    (world, ship)
}

/// Runs the ticks from `from` up to and including `to`, as the server does: the clock passes a tick, the ships that coast are put on rails or taken off them, and what is on
/// rails is where the clock says.
fn run_ticks(world: &mut ServerWorld, from: u64, to: u64) {
    for tick in from..=to {
        world.data.set_clock(tick);
        world.update_coasting(TICKS);
        world.apply_rails(TICKS);
    }
}

#[test]
fn a_ship_that_is_left_alone_for_a_second_goes_on_a_rail_and_not_before() {
    let (mut world, ship) = a_circle_at_the_start_of_the_clock();
    run_ticks(&mut world, 1, 20);
    assert_eq!(rail_of(&world, ship), None, "19 ticks after the first");
    run_ticks(&mut world, 21, 21);
    let rail = rail_of(&world, ship).expect("a second of ticks after the first it is on a rail");
    assert_eq!(
        rail.orbit.as_ref().unwrap().around,
        Centre::Grid(GridId::new(GridId::FIRST_ALLOCATED))
    );
    // And it stays on the elements it was given: they are not made again from the place it is at.
    run_ticks(&mut world, 22, 2_000);
    assert_eq!(rail_of(&world, ship), Some(rail));
}

#[test]
fn a_push_or_a_move_starts_the_second_again_and_takes_the_ship_off_a_rail_where_it_is() {
    for push in 0..4 {
        let (mut world, ship) = a_circle_at_the_start_of_the_clock();
        run_ticks(&mut world, 1, 10);
        let disturb = |world: &mut ServerWorld| match push {
            0 => world.apply_grid_force(ship, [1.0, 0.0, 0.0], None).unwrap(),
            1 => world.apply_grid_torque(ship, [0.0, 1.0, 0.0]).unwrap(),
            2 => {
                let velocity = world.data().grids().get(ship).unwrap().linear_velocity;
                world.set_grid_velocity(ship, velocity, [0.0; 3]).unwrap()
            }
            _ => {
                let pose = world.data().grids().get(ship).unwrap().pose;
                world.set_grid_pose(ship, pose).unwrap()
            }
        };
        disturb(&mut world);
        // A second from the disturbance and not from the start.
        run_ticks(&mut world, 11, 30);
        assert_eq!(
            rail_of(&world, ship),
            None,
            "push {push}: 19 ticks after it"
        );
        run_ticks(&mut world, 31, 31);
        let rail = rail_of(&world, ship).expect("on a rail a second after the push");
        // On a rail, a push takes it off, and it is where the rail put it.
        let place = world.data().grids().get(ship).unwrap().pose;
        disturb(&mut world);
        assert_eq!(rail_of(&world, ship), None, "push {push}: {rail:?}");
        assert_eq!(world.data().grids().get(ship).unwrap().pose, place);
    }
}

#[test]
fn what_the_bodies_write_is_not_a_disturbance() {
    let (mut world, ship) = a_circle_at_the_start_of_the_clock();
    run_ticks(&mut world, 1, 10);
    for tick in 11..=40 {
        // (The bodies write the ship's place and speed every tick.)
        let state = world.data().grids().get(ship).unwrap().clone();
        world
            .move_grid_by_bodies(
                ship,
                state.pose,
                state.linear_velocity,
                state.angular_velocity,
            )
            .unwrap();
        world
            .slow_grid_by_bodies(ship, state.linear_velocity, [0.0; 3])
            .unwrap();
        run_ticks(&mut world, tick, tick);
    }
    assert!(rail_of(&world, ship).is_some());
}

#[test]
fn a_ship_on_a_rail_that_is_held_or_that_turns_or_is_in_a_level_with_a_ground_comes_off() {
    for reason in 0..3 {
        let (mut world, ship) = a_circle_at_the_start_of_the_clock();
        run_ticks(&mut world, 1, 25);
        assert!(rail_of(&world, ship).is_some());
        match reason {
            0 => world.set_grid_held(ship, true).unwrap(),
            1 => {
                world
                    .data_mut()
                    .grids_mut()
                    .get_mut(ship)
                    .unwrap()
                    .angular_velocity = [0.0, 0.5, 0.0]
            }
            _ => world.set_level_gravity(LevelIndex::MAIN, [0.0, -5.0, 0.0]),
        }
        world.update_coasting(TICKS);
        assert_eq!(rail_of(&world, ship), None, "reason {reason}");
    }
}

#[test]
fn rails_that_are_not_an_orbit_round_a_planet_are_left_alone_and_so_are_planets() {
    let (mut world, planet) = space();
    let point = orbit_about(Centre::Point([100.0, 0.0, 0.0]), 50.0, 10.0);
    let spinning = Rail {
        spin: Some(Spin::new([0.0, 1.0, 0.0], 3600.0, 0.0).unwrap()),
        orbit: None,
    };
    let on_a_point = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    let turning = world
        .create_grid(LevelIndex::MAIN, GridPose::identity())
        .unwrap();
    world
        .set_grid_rail(on_a_point, Some(point.clone()))
        .unwrap();
    world
        .set_grid_rail(turning, Some(spinning.clone()))
        .unwrap();
    let planet_rail = orbit_about(Centre::Point([0.0; 3]), 1.0e6, 1.0e5);
    world
        .set_grid_rail(planet, Some(planet_rail.clone()))
        .unwrap();
    // Pushed, moved and held: none of it takes these off.
    world.apply_grid_force(on_a_point, [1.0; 3], None).unwrap();
    world
        .set_grid_velocity(turning, [1.0; 3], [0.0; 3])
        .unwrap();
    world.set_grid_held(planet, true).unwrap();
    world.update_coasting(TICKS);
    assert_eq!(rail_of(&world, on_a_point), Some(point));
    assert_eq!(rail_of(&world, turning), Some(spinning));
    assert_eq!(rail_of(&world, planet), Some(planet_rail));
}

#[test]
fn a_frozen_ship_on_a_rail_goes_on_round_and_one_that_is_not_loses_its_speed() {
    let (mut world, ship) = a_circle_at_the_start_of_the_clock();
    // (Too slow for an orbit that does not come to the ground: it is not put on a rail.)
    let drifting = ship_at(&mut world, [0.0, 30_000.0, 0.0], [5.0, 0.0, 0.0]);
    run_ticks(&mut world, 1, 25);
    assert!(rail_of(&world, ship).is_some());
    let speed = world.data().grids().get(ship).unwrap().linear_velocity;
    assert_ne!(speed, [0.0; 3]);
    world.set_grid_frozen(ship, true).unwrap();
    assert_eq!(
        world.data().grids().get(ship).unwrap().linear_velocity,
        speed
    );
    assert_eq!(rail_of(&world, drifting), None);
    world.set_grid_frozen(drifting, true).unwrap();
    assert_eq!(
        world.data().grids().get(drifting).unwrap().linear_velocity,
        [0.0; 3]
    );
}

// ---- a change of the mass of the planet (the owner's rule, 2026-11-02) ----

/// A tick as the server runs it for what is on rails: the clock passes, the rails put ships where they say, the ships that coast are put on rails or taken off, and the rails put
/// them again (where nothing changed, nothing moves).
fn server_tick(world: &mut ServerWorld, tick: u64) {
    world.data.set_clock(tick);
    world.apply_rails(TICKS);
    world.update_coasting(TICKS);
    world.apply_rails(TICKS);
}

/// A ship on an ellipse round the planet of `space`: it starts at the far end, 10,000 voxels along x, the near end is 5,000 (the semi-major axis is 7,500 and the ratio 2/3), and it has
/// gone on a rail at tick 21. Returns the world, the ship and the planet, with the clock at 21.
fn a_ship_on_an_ellipse() -> (ServerWorld, GridId, GridId) {
    let (mut world, planet) = space();
    let mu = PULL * 1.0e22;
    let speed = libm::sqrt(mu * (2.0 / 10_000.0 - 1.0 / 7_500.0));
    let ship = ship_at(&mut world, [10_000.0, 0.0, 0.0], [0.0, 0.0, speed]);
    for tick in 0..=21 {
        server_tick(&mut world, tick);
    }
    assert!(rail_of(&world, ship).is_some(), "on a rail at tick 21");
    (world, ship, planet)
}

fn orbit_of(world: &ServerWorld, ship: GridId) -> Orbit {
    rail_of(world, ship).unwrap().orbit.unwrap()
}

/// Runs the server's ticks after `from` until the rail of the ship is not `old`, or `last` has been run, and says the tick in which it changed.
fn until_the_rail_changes(
    world: &mut ServerWorld,
    ship: GridId,
    old: Option<Rail>,
    from: u64,
    last: u64,
) -> Option<u64> {
    for tick in from + 1..=last {
        server_tick(world, tick);
        if rail_of(world, ship) != old {
            return Some(tick);
        }
    }
    None
}

/// The ticks after `from` up to `last` in which a rail passes an end, as a ship works that out: the stretch of time that a tick stands for, half a tick either side of it.
fn ticks_of_the_end(orbit: Orbit, from: u64, last: u64, periapsis: bool) -> Vec<u64> {
    (from + 1..=last)
        .filter(|&tick| {
            let passed = orbit.apsides_passed(
                RailTime::of_half_ticks(2 * tick - 1, TICKS),
                RailTime::of_half_ticks(2 * tick + 1, TICKS),
            );
            if periapsis {
                passed.periapsis
            } else {
                passed.apoapsis
            }
        })
        .collect()
}

/// How far from the planet a ship is, in voxels.
fn from_the_planet(world: &ServerWorld, ship: GridId, planet: GridId) -> f64 {
    distance_between(
        world.data().grids().get(planet).unwrap().pose.position,
        world.data().grids().get(ship).unwrap().pose.position,
    )
}

fn periapsis_of(orbit: Orbit) -> f64 {
    orbit.radius() * orbit.ratio()
}

fn apoapsis_of(orbit: Orbit) -> f64 {
    orbit.radius() * (2.0 - orbit.ratio())
}

#[test]
fn a_planet_that_weighs_more_is_answered_at_the_nearest_point_and_the_ship_cannot_fall_into_it() {
    let (mut world, ship, planet) = a_ship_on_an_ellipse();
    let old = rail_of(&world, ship);
    let before = orbit_of(&world, ship);
    assert!((periapsis_of(before.clone()) / 5_000.0 - 1.0).abs() < 1e-6);
    assert!((apoapsis_of(before.clone()) / 10_000.0 - 1.0).abs() < 1e-6);
    // (An increase of a fifth: the speed at the nearest point is that of a circle for 1.333 times the mass, so it is still the nearest point with a mass of up to that.)
    let mass = 1.2e22;
    world.set_grid_mass(planet, mass, [0.0; 3]).unwrap();
    // The first end that comes is the nearest, half a period on; a ship does not answer at the far end that comes after it.
    let expected = ticks_of_the_end(before.clone(), 21, 400, true)[0];
    assert!(expected < ticks_of_the_end(before.clone(), 21, 400, false)[0]);
    let tick = until_the_rail_changes(&mut world, ship, old, 21, 400).expect("answered");
    assert_eq!(
        tick, expected,
        "answered in the tick that passes the nearest point"
    );
    let after = orbit_of(&world, ship);
    assert!((after.mass_parameter() / (PULL * mass) - 1.0).abs() < 1e-12);
    // The nearest point stays where it was, to the part of a tick that the ship is from it, and the far end comes in.
    assert!(
        (periapsis_of(after.clone()) / periapsis_of(before.clone()) - 1.0).abs() < 5e-3,
        "{} against {}",
        periapsis_of(after),
        periapsis_of(before)
    );
    assert!(
        apoapsis_of(after.clone()) < 0.8 * apoapsis_of(before.clone()),
        "{after:?}"
    );
    assert!(
        (from_the_planet(&world, ship, planet) / periapsis_of(before) - 1.0).abs() < 5e-3,
        "it was at the nearest point"
    );
    // And it is not answered again: the mass the elements say is the mass now.
    let kept = rail_of(&world, ship);
    for later in tick + 1..tick + 400 {
        server_tick(&mut world, later);
    }
    assert_eq!(rail_of(&world, ship), kept);
}

#[test]
fn a_planet_that_weighs_less_is_answered_at_the_far_point_and_the_ship_cannot_be_thrown_out() {
    let (mut world, ship, planet) = a_ship_on_an_ellipse();
    let old = rail_of(&world, ship);
    let before = orbit_of(&world, ship);
    let mass = 0.7e22;
    world.set_grid_mass(planet, mass, [0.0; 3]).unwrap();
    // The first end that comes is the nearest; the ship lets it pass and answers at the far one.
    let near = ticks_of_the_end(before.clone(), 21, 400, true)[0];
    let far = ticks_of_the_end(before.clone(), 21, 400, false)[0];
    assert!(near < far);
    let tick = until_the_rail_changes(&mut world, ship, old, 21, 400).expect("answered");
    assert_eq!(
        tick, far,
        "answered at the far end, not at the near one at {near}"
    );
    let after = orbit_of(&world, ship);
    assert!((after.mass_parameter() / (PULL * mass) - 1.0).abs() < 1e-12);
    assert!(
        (apoapsis_of(after.clone()) / apoapsis_of(before.clone()) - 1.0).abs() < 5e-3,
        "{} against {}",
        apoapsis_of(after),
        apoapsis_of(before)
    );
    assert!(
        periapsis_of(after.clone()) > 1.2 * periapsis_of(before.clone()),
        "{after:?}"
    );
    assert!(
        (from_the_planet(&world, ship, planet) / apoapsis_of(before) - 1.0).abs() < 5e-3,
        "it was at the far end"
    );
}

#[test]
fn the_answer_is_continuous_in_the_place_and_in_the_speed() {
    // Two worlds that are the same until the tick of the answer: in one the planet weighs what it did, so its ship goes on on the old rail, and in the other it weighs more.
    let (mut changed, ship, planet) = a_ship_on_an_ellipse();
    let (mut unchanged, other, _) = a_ship_on_an_ellipse();
    assert_eq!(ship, other);
    let old = rail_of(&changed, ship);
    changed.set_grid_mass(planet, 1.5e22, [0.0; 3]).unwrap();
    let tick = until_the_rail_changes(&mut changed, ship, old.clone(), 21, 400).expect("answered");
    for later in 22..=tick {
        server_tick(&mut unchanged, later);
    }
    assert_eq!(rail_of(&unchanged, ship), old);
    let (new, was) = (
        changed.data().grids().get(ship).unwrap(),
        unchanged.data().grids().get(ship).unwrap(),
    );
    assert!(
        distance_between(was.pose.position, new.pose.position) < 1e-6,
        "{:?} then {:?}",
        was.pose.position,
        new.pose.position
    );
    for axis in 0..3 {
        assert!(
            (was.linear_velocity[axis] - new.linear_velocity[axis]).abs() < 1e-6,
            "{:?} then {:?}",
            was.linear_velocity,
            new.linear_velocity
        );
    }
}

#[test]
fn a_circle_has_no_ends_and_is_answered_at_once() {
    let (mut world, ship) = a_circle_at_the_start_of_the_clock();
    for tick in 0..=21 {
        server_tick(&mut world, tick);
    }
    let old = rail_of(&world, ship);
    assert!(old.is_some());
    world
        .set_grid_mass(GridId::new(GridId::FIRST_ALLOCATED), 1.2e22, [0.0; 3])
        .unwrap();
    let tick = until_the_rail_changes(&mut world, ship, old, 21, 400).expect("answered");
    assert_eq!(tick, 22);
    assert!((orbit_of(&world, ship).mass_parameter() / (PULL * 1.2e22) - 1.0).abs() < 1e-12);
}

#[test]
fn a_decrease_that_leaves_the_ship_no_ellipse_takes_it_off_its_rail_at_the_far_end() {
    // One that the speed at the far end is too great for (the speed of escape there falls to 5,166 voxels a second, and it goes 6,670), and a planet that pulls nothing.
    for mass in [0.2e22, 1.0e10] {
        let (mut world, ship, planet) = a_ship_on_an_ellipse();
        let old = rail_of(&world, ship);
        let before = orbit_of(&world, ship);
        world.set_grid_mass(planet, mass, [0.0; 3]).unwrap();
        let far = ticks_of_the_end(before, 21, 400, false)[0];
        let tick = until_the_rail_changes(&mut world, ship, old, 21, 400).expect("answered");
        assert_eq!(tick, far, "mass {mass}");
        assert_eq!(rail_of(&world, ship), None, "mass {mass}");
        // It goes on as it was going.
        let speed = world.data().grids().get(ship).unwrap().linear_velocity;
        assert!(speed.iter().any(|part| *part != 0.0));
    }
}

#[test]
fn a_change_of_the_mass_that_is_nothing_is_not_answered() {
    let (mut world, ship, planet) = a_ship_on_an_ellipse();
    let old = rail_of(&world, ship);
    world
        .set_grid_mass(planet, 1.0e22 * (1.0 + 1.0e-12), [0.0; 3])
        .unwrap();
    assert_eq!(until_the_rail_changes(&mut world, ship, old, 21, 600), None);
}

#[test]
fn a_change_of_the_pull_strength_is_a_change_of_the_mass_parameter_and_answered_alike() {
    let (mut world, ship, _) = a_ship_on_an_ellipse();
    let old = rail_of(&world, ship);
    let before = orbit_of(&world, ship);
    let settings = *world.gravity_settings();
    world.set_gravity_settings(GravitySettings {
        pull_strength: settings.pull_strength * 1.3,
        ..settings
    });
    let near = ticks_of_the_end(before, 21, 400, true)[0];
    let tick = until_the_rail_changes(&mut world, ship, old, 21, 400).expect("answered");
    assert_eq!(tick, near);
    assert!((orbit_of(&world, ship).mass_parameter() / (PULL * 1.3 * 1.0e22) - 1.0).abs() < 1e-12);
}

#[test]
fn a_change_that_is_more_than_the_eccentricity_turns_the_end_it_is_answered_at_into_the_other_end_and_moves_the_other_by_more()
 {
    // The owner's rule keeps the distance at the end of the ellipse where the answer is made, but as an end: how much a ship's speed there is from that of a circle for the new mass
    // says which end it is after. At the nearest point the speed is that of a circle for (1 + e) times the mass (e = 1/3 here), so a planet that weighs half as much again
    // (more than 4/3) makes that point the *far* end of the new ellipse, and the near end goes down by what is left: here a fifth. Likewise a ship at the far end, whose speed is that of a
    // circle for (1 - e) = 2/3 of the mass, is at the *near* end of the orbit round a planet of half the mass, and the far end goes out. Both orbits are what a ship of that state
    // has round that mass; nothing here is made safe, and the check of the orbit (above the ground and inside the sphere) is what takes a ship off its rail if it is not.
    let (mut world, ship, planet) = a_ship_on_an_ellipse();
    let old = rail_of(&world, ship);
    let before = orbit_of(&world, ship);
    world.set_grid_mass(planet, 1.5e22, [0.0; 3]).unwrap();
    let near = ticks_of_the_end(before.clone(), 21, 400, true)[0];
    let tick = until_the_rail_changes(&mut world, ship, old, 21, 400).expect("answered");
    assert_eq!(tick, near);
    let after = orbit_of(&world, ship);
    assert!(
        (apoapsis_of(after.clone()) / periapsis_of(before.clone()) - 1.0).abs() < 5e-3,
        "{after:?}"
    );
    assert!(
        periapsis_of(after.clone()) < 0.85 * periapsis_of(before.clone()),
        "{after:?}"
    );

    let (mut world, ship, planet) = a_ship_on_an_ellipse();
    let old = rail_of(&world, ship);
    world.set_grid_mass(planet, 0.5e22, [0.0; 3]).unwrap();
    let far = ticks_of_the_end(before.clone(), 21, 400, false)[0];
    let tick = until_the_rail_changes(&mut world, ship, old, 21, 400).expect("answered");
    assert_eq!(tick, far);
    let after = orbit_of(&world, ship);
    assert!(
        (periapsis_of(after.clone()) / apoapsis_of(before.clone()) - 1.0).abs() < 5e-3,
        "{after:?}"
    );
    assert!(
        apoapsis_of(after.clone()) > 1.2 * apoapsis_of(before),
        "{after:?}"
    );
}
