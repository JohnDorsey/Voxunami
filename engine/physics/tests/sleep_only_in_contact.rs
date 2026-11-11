//! A body sleeps only while it touches something (`docs/decisions/joints-and-cars.md`, "Sleeping", accepted by the owner on 2026-11-10: "a ship may sleep while it touches another
//! ship or the ground, and never otherwise"). Rapier's own rule puts a body to sleep when it is slow enough for half a second, and zeroes its velocity: the ship audit ran a free
//! ship at 0.1 m/s that stopped dead, and one released at rest in a pull of 0.15 m/s/s that never fell. Sleep is a saving for a stack of bodies at rest on the ground; a body that
//! touches nothing has nothing to save, and a slow one is not a still one.
//!
//! The ground here is a floor of whole cubes with its top at 0, and a body is a cube of whole voxels.

use std::collections::HashMap;

use physics::{
    BodyId, BodyKind, BodyMass, BodySettings, BodyState, BodyWorld, BrickHint, Collision,
    CollisionSource, MassMoments,
};
use voxel_space::{BrickCoordinate, Orientation, VoxelCoordinate, WorldPosition};

const BODY: BodyId = BodyId(1);
const OTHER: BodyId = BodyId(2);

/// Every voxel below the top of the floor is a whole cube.
struct Floor;

impl CollisionSource for Floor {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        if voxel.y() < 0 {
            Collision::Full
        } else {
            Collision::Empty
        }
    }

    fn brick_hint(&mut self, brick: BrickCoordinate) -> BrickHint {
        if brick.y() < 0 {
            BrickHint::Solid
        } else {
            BrickHint::Empty
        }
    }
}

/// The voxels of a cube of `side` voxels with a corner at the origin of the body.
struct Cube(i128);

impl CollisionSource for Cube {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        let inside = |coordinate: i128| (0..self.0).contains(&coordinate);
        if inside(voxel.x()) && inside(voxel.y()) && inside(voxel.z()) {
            Collision::Full
        } else {
            Collision::Empty
        }
    }
}

fn cube_mass(side: i128) -> BodyMass {
    let mut voxels: HashMap<VoxelCoordinate, ()> = HashMap::new();
    for x in 0..side {
        for y in 0..side {
            for z in 0..side {
                voxels.insert(VoxelCoordinate::new(x, y, z), ());
            }
        }
    }
    let mut voxels: Vec<VoxelCoordinate> = voxels.into_keys().collect();
    voxels.sort();
    MassMoments::sum(
        voxels
            .into_iter()
            .map(|v| MassMoments::of_voxel([v.x(), v.y(), v.z()], 1.0)),
    )
    .properties()
    .expect("a cube has a mass")
}

fn world_with(gravity: [f64; 3], floor: bool) -> BodyWorld {
    let mut world = BodyWorld::new(
        BrickCoordinate::ORIGIN,
        BodySettings {
            gravity,
            ..BodySettings::default()
        },
    );
    if floor {
        for x in -1..=1 {
            for z in -1..=1 {
                world.set_terrain_brick(BrickCoordinate::new(x, -1, z), &mut Floor);
            }
        }
    }
    world
}

fn add_cube(world: &mut BodyWorld, id: BodyId, side: i128, state: BodyState) {
    world.add_body(id, state, BodyKind::Dynamic).unwrap();
    world
        .set_brick(id, BrickCoordinate::ORIGIN, &mut Cube(side))
        .unwrap();
    world.set_mass(id, cube_mass(side)).unwrap();
}

fn state_at(height: f64, velocity: [f64; 3], spin: [f64; 3]) -> BodyState {
    BodyState {
        position: WorldPosition::from_voxels([8.0, height, 8.0]),
        orientation: Orientation::IDENTITY,
        linear_velocity: velocity,
        angular_velocity: spin,
    }
}

fn run(world: &mut BodyWorld, ticks: usize) {
    for _ in 0..ticks {
        world.step();
    }
}

fn height_of(world: &BodyWorld, id: BodyId) -> f64 {
    world.state(id).unwrap().position.in_voxels()[1]
}

#[test]
fn a_body_that_touches_nothing_never_sleeps_however_slowly_it_moves() {
    let mut world = world_with([0.0; 3], false);
    add_cube(
        &mut world,
        BODY,
        2,
        state_at(100.0, [0.05, 0.0, 0.0], [0.0; 3]),
    );
    // Ten seconds, twenty times what Rapier waits before it puts a slow body to sleep.
    run(&mut world, 200);
    assert!(!world.is_asleep(BODY));
    let state = world.state(BODY).unwrap();
    assert!(
        (state.linear_velocity[0] - 0.05).abs() < 1e-12,
        "{:?}",
        state.linear_velocity
    );
    assert!((state.position.in_voxels()[0] - 8.5).abs() < 1e-9);
}

#[test]
fn a_body_released_at_rest_in_a_weak_pull_falls_as_it_should() {
    // The audit: at 0.15 m/s/s or less it never fell, because it was put to sleep before it was fast enough to be awake.
    let mut world = world_with([0.0, -0.1, 0.0], false);
    add_cube(&mut world, BODY, 2, state_at(100.0, [0.0; 3], [0.0; 3]));
    run(&mut world, 400);
    // Twenty seconds: half of 0.1 times 20 squared, 20 voxels.
    let fallen = 100.0 - height_of(&world, BODY);
    assert!((fallen - 20.0).abs() < 0.5, "it fell {fallen}");
    assert!(!world.is_asleep(BODY));
}

#[test]
fn a_body_at_rest_on_the_ground_goes_to_sleep() {
    let mut world = world_with([0.0, -5.0, 0.0], true);
    add_cube(&mut world, BODY, 2, state_at(0.0, [0.0; 3], [0.0; 3]));
    run(&mut world, 60);
    assert!(world.is_asleep(BODY));
    assert!(height_of(&world, BODY).abs() < 0.02);
}

#[test]
fn a_body_in_a_weak_pull_comes_to_rest_on_the_ground_and_sleeps() {
    // It is awake all the way down, and sleeps once it has touched and been still for the half second.
    let mut world = world_with([0.0, -0.5, 0.0], true);
    add_cube(&mut world, BODY, 2, state_at(2.0, [0.0; 3], [0.0; 3]));
    run(&mut world, 10);
    assert!(!world.is_asleep(BODY), "a body in the air is awake");
    run(&mut world, 120);
    assert!(height_of(&world, BODY).abs() < 0.02);
    assert!(world.is_asleep(BODY));
}

#[test]
fn a_body_on_another_body_sleeps_with_it() {
    let mut world = world_with([0.0, -5.0, 0.0], true);
    add_cube(&mut world, BODY, 4, state_at(0.0, [0.0; 3], [0.0; 3]));
    add_cube(&mut world, OTHER, 2, state_at(4.01, [0.0; 3], [0.0; 3]));
    run(&mut world, 100);
    assert!(world.is_asleep(BODY));
    assert!(world.is_asleep(OTHER), "it touches the other body");
    assert!((height_of(&world, OTHER) - 4.0).abs() < 0.05);
}

#[test]
fn a_body_that_slept_on_the_ground_and_is_lifted_off_it_wakes_and_does_not_sleep_again() {
    let mut world = world_with([0.0, -0.1, 0.0], true);
    add_cube(&mut world, BODY, 2, state_at(0.0, [0.0; 3], [0.0; 3]));
    // (In a pull this weak it takes three seconds to settle into the ground and be still for the half second.)
    run(&mut world, 120);
    assert!(world.is_asleep(BODY));
    world
        .set_state(BODY, state_at(50.0, [0.0; 3], [0.0; 3]))
        .unwrap();
    // Ten seconds in a pull of 0.1: it is going at 1 m/s and has fallen 5 voxels, and it was awake all the time.
    for tick in 0..200 {
        world.step();
        assert!(!world.is_asleep(BODY), "asleep in the air at tick {tick}");
    }
    let fallen = 50.0 - height_of(&world, BODY);
    assert!((fallen - 5.0).abs() < 0.2, "it fell {fallen}");
}

#[test]
fn a_body_thrown_up_slowly_is_awake_at_the_top_of_its_rise_and_sleeps_again_when_it_has_landed() {
    // In a pull of 0.1, 0.3 m/s up is three seconds of rise to 0.45 voxels, and at the top the body is slower than anything Rapier's rule lets move: it would be asleep there,
    // stopped in the air, and be woken by nothing.
    let mut world = world_with([0.0, -0.1, 0.0], true);
    add_cube(&mut world, BODY, 2, state_at(0.0, [0.0; 3], [0.0; 3]));
    run(&mut world, 120);
    assert!(world.is_asleep(BODY), "it rested on the ground");
    world
        .set_state(BODY, state_at(0.1, [0.0, 0.3, 0.0], [0.0; 3]))
        .unwrap();
    for tick in 0..60 {
        world.step();
        assert!(!world.is_asleep(BODY), "asleep at tick {tick} of the rise");
    }
    let top = height_of(&world, BODY);
    let speed = world.state(BODY).unwrap().linear_velocity[1];
    assert!((top - 0.55).abs() < 0.03, "the top of the rise is {top}");
    assert!(speed.abs() < 0.01, "it is slow at the top: {speed}");
    run(&mut world, 240);
    assert!(height_of(&world, BODY).abs() < 0.02, "it is down again");
    assert!(world.is_asleep(BODY), "and at rest on the ground it sleeps");
}

#[test]
fn the_ground_taken_from_under_a_sleeping_body_lets_it_fall_in_a_weak_pull_too() {
    let mut world = world_with([0.0, -0.3, 0.0], true);
    add_cube(&mut world, BODY, 2, state_at(0.0, [0.0; 3], [0.0; 3]));
    run(&mut world, 100);
    assert!(world.is_asleep(BODY));
    for x in -1..=1 {
        for z in -1..=1 {
            world.remove_terrain_brick(BrickCoordinate::new(x, -1, z));
        }
    }
    // Four seconds: 2.4 voxels, and a body that fell slower than 0.5 m/s would have been put to sleep again by Rapier's rule alone.
    run(&mut world, 80);
    assert!(!world.is_asleep(BODY));
    let fallen = -height_of(&world, BODY);
    assert!((fallen - 2.4).abs() < 0.2, "it fell {fallen}");
}

#[test]
fn a_body_kept_awake_on_the_ground_does_not_sleep_and_one_told_it_may_sleeps() {
    // What the cars of a mechanism ask for: `set_can_sleep(false)` is the stronger of the two rules.
    let mut world = world_with([0.0, -5.0, 0.0], true);
    add_cube(&mut world, BODY, 2, state_at(0.0, [0.0; 3], [0.0; 3]));
    world.set_can_sleep(BODY, false).unwrap();
    run(&mut world, 100);
    assert!(!world.is_asleep(BODY), "it touches the ground and may not");
    world.set_can_sleep(BODY, true).unwrap();
    run(&mut world, 60);
    assert!(world.is_asleep(BODY), "it touches the ground and may");
}

#[test]
fn a_body_that_sleeps_on_the_ground_and_is_then_told_it_may_not_is_woken() {
    let mut world = world_with([0.0, -5.0, 0.0], true);
    add_cube(&mut world, BODY, 2, state_at(0.0, [0.0; 3], [0.0; 3]));
    run(&mut world, 60);
    assert!(world.is_asleep(BODY));
    world.set_can_sleep(BODY, false).unwrap();
    assert!(!world.is_asleep(BODY), "it is woken at once");
    run(&mut world, 100);
    assert!(!world.is_asleep(BODY));
}

#[test]
fn a_body_that_may_sleep_in_the_air_is_still_awake_there() {
    // The other half: allowing sleep does not let a body sleep where it touches nothing.
    let mut world = world_with([0.0; 3], false);
    add_cube(
        &mut world,
        BODY,
        2,
        state_at(100.0, [0.0, 0.01, 0.0], [0.0; 3]),
    );
    world.set_can_sleep(BODY, true).unwrap();
    run(&mut world, 100);
    assert!(!world.is_asleep(BODY));
    assert!((world.state(BODY).unwrap().linear_velocity[1] - 0.01).abs() < 1e-12);
}
