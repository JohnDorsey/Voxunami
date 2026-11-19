//! Ships put on a rail by hand: set turning first and the rail last, since moving or speeding a ship takes a coasting rail from it, so that what a ship does on a rail is what it
//! does from its first tick, and ships at different places and speeds can be compared bit for bit.

use super::{Scene, circular_speed, make_ship, ship_id};
use server::ServerWorld;
use voxel_space::{GridId, GridPose, Orientation, WorldPosition};
use world_storage::rail::{Rail, RailTime};

pub const TICKS_PER_SECOND: u32 = 20;

/// As [`a_ship_on_a_rail`], in a scene that is given and for a ship of any shape: the voxels of the box of `size` that `solid` says are stone. (Two ticks, the second of which puts it
/// on its rail.) The scene has no ship before it, so its number is 1.
pub fn put_a_ship_on_a_rail(
    scene: &mut Scene,
    distance: f64,
    speed_factor: f64,
    size: [i128; 3],
    solid: impl Fn([i128; 3]) -> bool + Send + 'static,
    facing: Orientation,
    spin: [f64; 3],
) -> GridId {
    let speed = circular_speed(scene, 0, distance) * speed_factor;
    let middle = size.map(|edge| edge as f64 / 2.0);
    make_ship(
        scene,
        GridPose::new(
            WorldPosition::from_voxels([-middle[0], distance - middle[1], -middle[2]]),
            Orientation::IDENTITY,
        ),
        false,
        size,
        solid,
    );
    let ship = ship_id(1);
    install_a_rail(scene, ship, speed, facing, spin);
    ship
}

/// Faces a ship, sets it going at `speed` along x and turning, and puts it on the rail that is the orbit its middle and speed are on, at the world's clock.
pub fn install_a_rail(
    scene: &mut Scene,
    ship: GridId,
    speed: f64,
    facing: Orientation,
    spin: [f64; 3],
) {
    scene.run(move |world| install_a_rail_in(world, ship, speed, facing, spin));
}

/// What [`install_a_rail`] does in the tick it is given: [`set_going_in`], then [`put_on_its_rail_in`].
pub fn install_a_rail_in(
    world: &mut ServerWorld,
    ship: GridId,
    speed: f64,
    facing: Orientation,
    spin: [f64; 3],
) {
    set_going_in(world, ship, speed, facing, spin);
    put_on_its_rail_in(world, ship);
}

/// Faces a ship and sets it going at `speed` along x and turning. (Moving or speeding a ship takes a coasting rail from it, so this comes before the rail.)
pub fn set_going_in(
    world: &mut ServerWorld,
    ship: GridId,
    speed: f64,
    facing: Orientation,
    spin: [f64; 3],
) {
    let position = world.data().grids().get(ship).unwrap().pose.position;
    world
        .set_grid_pose(ship, GridPose::new(position, facing))
        .unwrap();
    world
        .set_grid_velocity(ship, [speed, 0.0, 0.0], spin)
        .unwrap();
}

/// Puts a ship on the rail that is the orbit its middle and speed are on, at the world's clock. (A ship has a rail only if nothing is near it, by where the others are and how fast they
/// go: a test that sets many going and puts them on rails in one tick sets them all going first.)
pub fn put_on_its_rail_in(world: &mut ServerWorld, ship: GridId) {
    let time = RailTime::of_tick(world.data().clock(), TICKS_PER_SECOND);
    let rail = world
        .coasting_rail(ship, time)
        .expect("the ship is clear of the ground and alone, so a rail may have it");
    world.set_grid_rail(ship, Some(rail)).unwrap();
}

/// Puts a ship on the orbit that it is on, though another ship is near it: the rules of a rail (`coasting_rail`) refuse a ship with a stranger near it, and a ship on a rail has
/// one near it all the same now (the stranger is the contact ghost's), so this is how a test makes the second of two ships near each other ride one. **The orbit is of the time of the
/// ship's state, which is a tick before the world's clock**: a ship that is free is where the last tick left it until the bodies step it, and a rail made for the clock's time would put
/// it a tick of its speed (38 voxels) behind the ship on a rail that it is beside, for good.
pub fn put_on_a_rail_near_another_ship(world: &mut ServerWorld, ship: GridId) {
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
