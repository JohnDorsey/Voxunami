//! A solar system in data (Phase 40f): the example pack `examples/packs/solar_system` puts a star, a planet that goes round it and a moon that goes round the planet in
//! Cave's space, and they go where the clock says, the periods of the pack are the ones the game's pull would keep, and what rests on the moon goes with it.

mod common;

use std::path::PathBuf;

use common::{SEED, Scene, length, minus, raft_over, ship_id, ship_voxels};
use game_shell::GameContent;
use voxunami_cave::content::CaveContent;
use voxunami_cave::game::Cave;

/// The ships of a world made with the pack: the planet of Cave is the first, then the pack's bodies in the order of their file names.
const STAR: u64 = 1;
const PLANET: u64 = 2;
const MOON: u64 = 3;

const TICKS_A_SECOND: f64 = 20.0;
/// The pull of the game (the `G` of the masses), as an `f32` is the one the game uses.
const PULL_STRENGTH: f64 = 6.674e-11_f32 as f64;

fn the_packs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/packs")
}

fn content() -> CaveContent {
    game_shell::content_with_packs::<Cave>(Some(&the_packs())).expect("the example pack loads")
}

fn scene() -> Scene {
    let content = content();
    let world = content.new_world(SEED);
    Scene::in_this_world(content, world)
}

fn mass_of(scene: &Scene, number: u64) -> f64 {
    f64::from(
        scene
            .rig
            .server
            .world()
            .grids()
            .get(ship_id(number))
            .expect("the body is there")
            .mass,
    )
}

fn the_tick(scene: &Scene) -> u64 {
    scene.rig.server.world().clock()
}

/// What the file of a body says of its orbit: the radius and the period.
fn orbit_in_the_file(name: &str) -> (f64, f64) {
    let path = the_packs().join(format!("solar_system/bodies/{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap();
    let file: serde_json::Value = serde_json::from_str(&text).unwrap();
    let orbit = &file["rail"]["orbit"];
    (
        orbit["radius"].as_f64().unwrap(),
        orbit["period"].as_f64().unwrap(),
    )
}

/// The angle, in radians, between two vectors.
fn angle_between(first: [f64; 3], second: [f64; 3]) -> f64 {
    let dot: f64 = (0..3).map(|axis| first[axis] * second[axis]).sum();
    libm::acos((dot / (length(first) * length(second))).clamp(-1.0, 1.0))
}

#[test]
fn the_pack_puts_a_star_a_planet_and_a_moon_in_the_world_each_pulling_as_its_file_says() {
    let scene = scene();
    assert_eq!(scene.rig.server.world().grids().iter().count(), 4);
    // The mass a body is given is what pulls `surface_gravity` at its radius: g r r / G.
    for (number, radius, gravity) in [
        (STAR, 40_000.0, 30.0),
        (PLANET, 16_000.0, 12.0),
        (MOON, 9_000.0, 10.0),
    ] {
        let expected = gravity * radius * radius / PULL_STRENGTH;
        let mass = mass_of(&scene, number);
        assert!(
            (mass / expected - 1.0).abs() < 1.0e-6,
            "ship {number} weighs {mass}, not {expected}"
        );
        // (A body pulls at all only above this: the moon is a moon and not a rock.)
        assert!(mass > 1.0e19, "ship {number} weighs {mass}");
    }
    // The star stands where its file puts it; the planet and the moon are where their orbits put them at time 0.
    let star = ship_voxels(&scene, STAR);
    assert_eq!(star, [-3_000_000.0, 0.0, 0.0]);
    let planet = ship_voxels(&scene, PLANET);
    let moon = ship_voxels(&scene, MOON);
    assert!((length(minus(planet, star)) - 1_500_000.0).abs() < 0.01);
    assert!((length(minus(moon, planet)) - 80_000.0).abs() < 0.01);
}

#[test]
fn the_periods_of_the_pack_are_the_ones_the_pull_of_the_game_would_keep() {
    // A rail does not feel any pull (the orbit is a function of the clock), so the pack's periods are the owner's to choose; these are the ones of Kepler's third law,
    // T = 2 pi sqrt(r^3 / (G M)), rounded, so that what a body does is what its pull says it should.
    let scene = scene();
    for (name, centre, number) in [
        ("star_planet", STAR, PLANET),
        ("star_planet_moon", PLANET, MOON),
    ] {
        let (radius, period) = orbit_in_the_file(name);
        let kepler = 2.0
            * std::f64::consts::PI
            * (radius * radius * radius / (PULL_STRENGTH * mass_of(&scene, centre))).sqrt();
        assert!(
            (period / kepler - 1.0).abs() < 0.01,
            "{name} (ship {number}) goes round in {period} seconds, the pull keeps {kepler}"
        );
    }
}

#[test]
fn the_planet_and_the_moon_go_round_at_the_rates_the_clock_gives_and_keep_their_distances() {
    let mut scene = scene();
    scene.ticks(1);
    let (planet_radius, planet_period) = orbit_in_the_file("star_planet");
    let (moon_radius, moon_period) = orbit_in_the_file("star_planet_moon");
    let from_the_star = |scene: &Scene| minus(ship_voxels(scene, PLANET), ship_voxels(scene, STAR));
    let from_the_planet =
        |scene: &Scene| minus(ship_voxels(scene, MOON), ship_voxels(scene, PLANET));
    let (planet_before, moon_before, tick_before) = (
        from_the_star(&scene),
        from_the_planet(&scene),
        the_tick(&scene),
    );
    // Fifty seconds, then a hundred and fifty more.
    for seconds in [50, 150] {
        scene.ticks(seconds * TICKS_A_SECOND as usize);
        let (planet, moon) = (from_the_star(&scene), from_the_planet(&scene));
        let elapsed = (the_tick(&scene) - tick_before) as f64 / TICKS_A_SECOND;
        // However long it has gone, it is as far from what it goes round as it was...
        assert!(
            (length(planet) - planet_radius).abs() < 0.01,
            "{}",
            length(planet)
        );
        assert!(
            (length(moon) - moon_radius).abs() < 0.01,
            "{}",
            length(moon)
        );
        // ...and has gone the angle the clock says (a tenth of a voxel at the planet's distance is 7e-8 of a radian).
        let turn = |period: f64| 2.0 * std::f64::consts::PI * elapsed / period;
        assert!(
            (angle_between(planet_before, planet) - turn(planet_period)).abs() < 1.0e-7,
            "the planet went {} radians, the clock says {}",
            angle_between(planet_before, planet),
            turn(planet_period)
        );
        // (The moon goes round in 2,565 seconds: 200 seconds is no full turn.)
        assert!(
            (angle_between(moon_before, moon) - turn(moon_period)).abs() < 1.0e-6,
            "the moon went {} radians, the clock says {}",
            angle_between(moon_before, moon),
            turn(moon_period)
        );
    }
}

#[test]
fn a_raft_on_the_moon_goes_with_it_round_the_planet_and_the_star_and_stays_on_its_ground() {
    let mut scene = scene();
    scene.ticks(1);
    // Fifty voxels over the moon's pole (its ground there is 9,000 and some hills high); it falls in about three seconds and the moon, at the end of the half
    // minute, has gone 11 kilometres in the world.
    let raft = raft_over(&mut scene, MOON, [0.0, 9_080.0, 0.0], false, [0.0; 3]);
    let moon_before = ship_voxels(&scene, MOON);
    scene.ticks(600);
    let travelled = length(minus(ship_voxels(&scene, MOON), moon_before));
    assert!(travelled > 10_000.0, "the moon went {travelled} voxels");
    let offset = minus(ship_voxels(&scene, raft), ship_voxels(&scene, MOON));
    assert!(
        offset[0].abs() < 15.0 && offset[2].abs() < 15.0 && (8_960.0..9_045.0).contains(&offset[1]),
        "the raft is {offset:?} from the middle of the moon"
    );
}

#[test]
fn the_system_goes_on_from_where_it_was_when_the_world_is_saved_and_loaded() {
    let mut scene = scene();
    scene.ticks(400);
    let (tick, moon) = (
        the_tick(&scene),
        minus(ship_voxels(&scene, MOON), ship_voxels(&scene, PLANET)),
    );
    let mut scene = scene.restarted();
    scene.ticks(1);
    assert_eq!(the_tick(&scene), tick + 1);
    let planet = minus(ship_voxels(&scene, PLANET), ship_voxels(&scene, STAR));
    let after = minus(ship_voxels(&scene, MOON), ship_voxels(&scene, PLANET));
    assert!((length(planet) - 1_500_000.0).abs() < 0.01);
    assert!((length(after) - 80_000.0).abs() < 0.01);
    // The moon is where the clock says a tick after it was, not where it began (a world that began its clock again would have put it 400 ticks back).
    let (_, period) = orbit_in_the_file("star_planet_moon");
    let step = angle_between(moon, after);
    let expected = 2.0 * std::f64::consts::PI / (period * TICKS_A_SECOND);
    assert!(
        (step - expected).abs() < 1.0e-7,
        "the moon went {step} radians in a tick, the clock says {expected}"
    );
}
