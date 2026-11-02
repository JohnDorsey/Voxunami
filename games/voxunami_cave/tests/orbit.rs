//! A free ship in orbit round Cave's planet (M1 step 1, `architecture-review.md` 7.1): before anything is built on flying, does a ship that goes round at orbital speed keep its
//! orbit? The solver is Rapier's and a tick is a twentieth of a second, so a ship that is only pulled by a planet could gain or lose energy a little every tick.
//!
//! **It does, and these tests say how much** (found 2026-10-30; `build-log.md`). In a circle 2 km over the planet's hills a cube of stone gains about 0.35 thousandths of its
//! energy and 0.18 thousandths of its angular momentum in every tenth of a lap (1,050 ticks, 52 seconds). A pull toward a point cannot change angular momentum, so this is the
//! integration: the pull is worked out once for the tick and held for every solver step of it (a ship at this speed is cut into 24 steps, `physics::BodyWorld::step`) while the ship
//! goes 39 metres. A model that does just that (`x' = x + v dt + a dt^2 / 2`, `v' = v + a dt`, `a` from `x`) gives 0.38 and 0.19 thousandths, within 7 percent of what is seen (and the radius within 11).
//!
//! The first two tests say what holds and what is wrong for a ship that is a body of the solver's. **A ship that coasts clear of the planet's ground is not** (step B of
//! `orbits-and-thrust-plan.md`, 2026-11-02): after a second without a push it is put on a rail, the orbit its place and its speed are on, and goes round on it exactly. The third
//! test is the criterion the review asked for, and it holds for such a ship: a circle clear of what the planet's extent says its ground reaches (2,100 bricks, 67,200 voxels,
//! and a margin) neither gains nor loses energy or angular momentum, however long it goes. The last tests say that it is on a rail and stays on the same elements.

mod common;

use std::sync::OnceLock;

use common::{Scene, make_ship, ship_id};
use game_shell::GameDefinition;
use voxel_space::{GridPose, Orientation, WorldPosition};
use voxunami_cave::Cave;

/// The edge of the cube that is the ship, in voxels.
const EDGE: i128 = 3;
/// The distance of the ship's middle from the planet's: 2 km over the top of the planet's radius (63,710), clear of hills of a few hundred metres, and inside what the planet's
/// extent says its ground reaches (67,200 voxels), so it is a body of the solver's.
const DISTANCE: f64 = 65_710.0;
/// The distance of a ship that is clear of the planet's extent and its margin (67,264): 6.3 km over the radius, where a ship goes on a rail.
const HIGH_DISTANCE: f64 = 70_000.0;
/// How often the orbit is looked at, in ticks, and how many times: together a tenth of a lap of about 10,506 ticks (11,550 at the high distance, where the sample is 9 percent
/// of a lap).
const SAMPLE_EVERY: usize = 105;
const SAMPLES: usize = 10;

/// What an orbit is made of, taken from the world as it is and not written down again here.
struct Planet {
    /// `G M` of the planet, in voxels cubed per second squared.
    gravitational_parameter: f64,
    /// How far the pull stops growing as the distance shrinks.
    softening: f64,
}

fn the_planet(scene: &Scene) -> Planet {
    // The game's own settings: what the server of a game is configured with.
    let settings = Cave::gravity();
    let planet = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(0))
        .expect("the planet is the first ship of a new world");
    let mass = planet.terrain.expect("it has terrain").mass;
    Planet {
        gravitational_parameter: settings.pull_strength * mass,
        softening: settings.softening,
    }
}

/// The speed of a circle at this distance from the middle of the planet, given the pull the world has (`a = G M / (d^2 + softening^2)`).
fn circular_speed(planet: &Planet, distance: f64) -> f64 {
    (planet.gravitational_parameter * distance
        / (distance * distance + planet.softening * planet.softening))
        .sqrt()
}

/// Makes a free ship whose middle is `distance` from the middle of the planet, straight over it, going round at `speed` (along +x). Returns its number.
fn put_in_orbit(scene: &mut Scene, distance: f64, speed: f64) -> u64 {
    let middle = EDGE as f64 / 2.0;
    make_ship(
        scene,
        GridPose::new(
            WorldPosition::from_voxels([-middle, distance - middle, -middle]),
            Orientation::IDENTITY,
        ),
        false,
        [EDGE; 3],
        |_| true,
    );
    let number = 1;
    scene.run(move |world| {
        world
            .set_grid_velocity(ship_id(number), [speed, 0.0, 0.0], [0.0; 3])
            .unwrap();
    });
    number
}

/// What a circle keeps, per kilogram, and where the ship is in it: the energy and the angular momentum about the planet's middle (in the plane of the orbit, about the z axis),
/// how far out it is, and the angle of its place from straight up, in degrees, turned the way it goes.
#[derive(Clone, Copy, Debug)]
struct Orbit {
    distance: f64,
    energy: f64,
    angular_momentum: f64,
    degrees_round: f64,
}

fn orbit_of(scene: &Scene, number: u64, planet: &Planet) -> Orbit {
    let ship = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(number))
        .expect("the ship is there");
    let origin = voxunami_cave::aboard::in_voxels(ship.pose.position);
    let middle = [0, 1, 2].map(|axis| origin[axis] + ship.centre_of_mass[axis]);
    let velocity = ship.linear_velocity.map(f64::from);
    let distance = (middle[0] * middle[0] + middle[1] * middle[1] + middle[2] * middle[2]).sqrt();
    let speed_squared = velocity.iter().map(|part| part * part).sum::<f64>();
    Orbit {
        distance,
        energy: speed_squared / 2.0 - planet.gravitational_parameter / distance,
        angular_momentum: middle[0] * velocity[1] - middle[1] * velocity[0],
        degrees_round: libm::atan2(middle[0], middle[1]).to_degrees(),
    }
}

/// The orbit of a ship put in a circle 2 km over the hills, at the start and then every [`SAMPLE_EVERY`] ticks, [`SAMPLES`] times. Worked out once for all the tests that read it
/// (about ten seconds: a ship at orbital speed costs about 11 milliseconds a tick, nearly all of it the terrain colliders asked for round a fast ship, `grid_bodies::keep_terrain`, `build-log.md`).
fn a_tenth_of_a_lap() -> &'static [Orbit] {
    static MEASURED: OnceLock<Vec<Orbit>> = OnceLock::new();
    MEASURED.get_or_init(|| measured_at(DISTANCE).orbits)
}

/// What was measured of a circle: how it looked at the start and every [`SAMPLE_EVERY`] ticks, and the rail the ship had after the first sample and at the end (none for a body of
/// the solver's).
struct Measured {
    orbits: Vec<Orbit>,
    rail_after_the_first_sample: Option<world_storage::rail::Rail>,
    rail_at_the_end: Option<world_storage::rail::Rail>,
}

fn rail_of_the_ship(scene: &Scene, number: u64) -> Option<world_storage::rail::Rail> {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(number))
        .expect("the ship is there")
        .rail
}

fn measured_at(distance: f64) -> Measured {
    let mut scene = Scene::in_a_new_world();
    let planet = the_planet(&scene);
    let number = put_in_orbit(&mut scene, distance, circular_speed(&planet, distance));
    let mut orbits = vec![orbit_of(&scene, number, &planet)];
    let mut rail_after_the_first_sample = None;
    for sample in 0..SAMPLES {
        scene.ticks(SAMPLE_EVERY);
        orbits.push(orbit_of(&scene, number, &planet));
        if sample == 0 {
            // (105 ticks: more than the second of being left alone, and the tick that puts the ship on its rail.)
            rail_after_the_first_sample = rail_of_the_ship(&scene, number);
        }
    }
    let rail_at_the_end = rail_of_the_ship(&scene, number);
    Measured {
        orbits,
        rail_after_the_first_sample,
        rail_at_the_end,
    }
}

/// The same for a ship in a circle clear of the planet's ground, worked out once for all the tests that read it.
fn a_tenth_of_a_lap_up_high() -> &'static Measured {
    static MEASURED: OnceLock<Measured> = OnceLock::new();
    MEASURED.get_or_init(|| measured_at(HIGH_DISTANCE))
}

/// How much the orbit has changed by this sample, as a part of the size of the start's.
fn change(of: impl Fn(&Orbit) -> f64, sample: usize) -> f64 {
    let orbits = a_tenth_of_a_lap();
    (of(&orbits[sample]) - of(&orbits[0])) / of(&orbits[0]).abs()
}

/// The same for the circle that is on a rail.
fn change_up_high(of: impl Fn(&Orbit) -> f64, sample: usize) -> f64 {
    let orbits = &a_tenth_of_a_lap_up_high().orbits;
    (of(&orbits[sample]) - of(&orbits[0])) / of(&orbits[0]).abs()
}

#[test]
fn a_free_ship_at_the_speed_of_a_circle_goes_round_the_planet_near_that_circle() {
    let orbits = a_tenth_of_a_lap();
    assert!(
        (orbits[0].distance - DISTANCE).abs() < 0.1,
        "it starts where it was put: {:?}",
        orbits[0]
    );
    for (index, orbit) in orbits.iter().enumerate() {
        assert!(
            (orbit.distance - DISTANCE).abs() < 5.0,
            "sample {index} strays from the circle: {orbit:?}"
        );
    }
    // A tenth of a lap is 36 degrees (the speed of the circle in the first tick, 785.9 metres a second, is the speed the pull asks for there).
    let last = orbits[SAMPLES].degrees_round;
    assert!(
        (35.0..37.0).contains(&last),
        "the ship is {last} degrees round after a tenth of a lap"
    );
}

#[test]
fn today_a_ship_in_a_circle_gains_energy_and_angular_momentum_at_every_look_because_the_pull_is_held_for_the_whole_tick()
 {
    // What is wrong for a body of the solver's (this ship is inside the planet's extent, so it never goes on a rail), said so that the day it is put right is a change of this test
    // and not a surprise: after a tenth of a lap the energy is up by about 0.35 thousandths of its
    // size and the angular momentum, which a pull toward a point cannot change, by about 0.18 thousandths; the energy gains twice what the momentum does, which is what an error
    // of the first order in the step does (a textbook explicit step gives 2.0 too); and every look finds more than the one before.
    let energy = change(|orbit| orbit.energy, SAMPLES);
    let momentum = change(|orbit| orbit.angular_momentum.abs(), SAMPLES);
    assert!(
        (3.0e-4..4.2e-4).contains(&energy),
        "energy changed by {energy:e}"
    );
    assert!(
        (1.5e-4..2.1e-4).contains(&momentum),
        "angular momentum changed by {momentum:e}"
    );
    assert!(
        (1.9..2.1).contains(&(energy / momentum)),
        "energy {energy:e} against momentum {momentum:e}"
    );
    let orbits = a_tenth_of_a_lap();
    for sample in 1..=SAMPLES {
        assert!(
            orbits[sample].energy > orbits[sample - 1].energy
                && orbits[sample].angular_momentum.abs()
                    > orbits[sample - 1].angular_momentum.abs(),
            "sample {sample} gained nothing: {:?} then {:?}",
            orbits[sample - 1],
            orbits[sample]
        );
    }
}

/// The criterion of the review (7.1): a ship at orbital speed should neither decay nor gain energy over a hundred laps. A hundred laps is a thousand times what is measured here
/// (three hours of running), so this asks the same of a tenth of a lap, tighter than a hundred laps could be allowed: the energy and the angular momentum are to change by less
/// than a hundred thousandth of their size. **It fails for a body of the solver's** by a factor of about thirty, and holds for a ship that coasts on a rail by far more than the
/// criterion asks (the elements come from the state of the ship, an `f64`: about 1e-12 is what is lost in the first second, and nothing after).
#[test]
fn a_circle_on_a_rail_does_not_gain_or_lose_energy_or_angular_momentum() {
    for sample in 1..=SAMPLES {
        let energy = change_up_high(|orbit| orbit.energy, sample);
        let momentum = change_up_high(|orbit| orbit.angular_momentum.abs(), sample);
        assert!(
            energy.abs() < 1.0e-5,
            "sample {sample}: energy changed by {energy:e}"
        );
        assert!(
            momentum.abs() < 1.0e-5,
            "sample {sample}: angular momentum changed by {momentum:e}"
        );
    }
}

#[test]
fn a_ship_in_a_circle_clear_of_the_ground_is_put_on_a_rail_after_a_second_and_keeps_it() {
    let measured = a_tenth_of_a_lap_up_high();
    let rail = measured
        .rail_after_the_first_sample
        .expect("a ship that is left alone for a second is on a rail");
    assert_eq!(rail.spin, None);
    assert_eq!(
        rail.orbit.expect("an orbit").around,
        world_storage::rail::Centre::Grid(ship_id(0)),
        "round the planet, which is the first ship of the world"
    );
    // The same elements at the end: nothing made them again from where the ship was.
    assert_eq!(measured.rail_at_the_end, Some(rail));
    for (index, orbit) in measured.orbits.iter().enumerate() {
        assert!(
            // (The ship was put 70,000 out and flew for a second as a body of the solver's before it was put on a rail: a straight tick is 0.011 voxel off the circle, the
            // second of the solver's adds a little, and the rail is the orbit of the state it had.)
            (orbit.distance - HIGH_DISTANCE).abs() < 0.1,
            "sample {index} strays from the circle: {orbit:?}"
        );
    }
    // On the rail nothing is gained or lost at all: from the first look on, the energy and the angular momentum are the same to the rounding of a number.
    let first = measured.orbits[1];
    for (index, orbit) in measured.orbits.iter().enumerate().skip(1) {
        assert!(
            ((orbit.energy - first.energy) / first.energy).abs() < 1.0e-12
                && ((orbit.angular_momentum - first.angular_momentum) / first.angular_momentum)
                    .abs()
                    < 1.0e-12,
            "sample {index}: {orbit:?} against {first:?}"
        );
    }
}

#[test]
fn a_ship_that_is_inside_what_the_extent_of_the_planet_says_is_ground_stays_a_body_of_the_solver() {
    let measured = measured_at(DISTANCE);
    assert_eq!(measured.rail_after_the_first_sample, None);
    assert_eq!(measured.rail_at_the_end, None);
}
