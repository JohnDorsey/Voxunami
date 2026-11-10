//! A ship that coasts on a rail and is taken off it keeps its speed (the ship audit of 2026-11-09, FRM-1 and PHY-1; the owner's rule that a rail is a function of the clock and
//! goes on being one: going off it must not change a path).
//!
//! A ship on a rail is a fixed body of the solver's, and a fixed body has no speed. The world takes the ship off its rail when another ship comes near
//! (`ServerWorld::update_coasting`, which leaves the ship's pose and velocity as the rail gave them), and the plugin has to give the body that speed when it makes it a free body
//! again. It did not when nothing but the rail's own motion had changed the ship since the plugin last looked: the ship went on from rest, from 761 metres a second to 0.4 in the
//! tick after.

mod common;

use common::{Scene, circular_speed, make_ship, ship_id};
use server::GridView;
use voxel_space::{GridId, GridPose, LevelIndex, Orientation, VoxelCoordinate, WorldPosition};
use voxunami_cave::content::CaveContent;

/// The distance from the middle of the planet at which a ship is clear of its ground and goes on a rail (see `orbit.rs`).
const DISTANCE: f64 = 70_000.0;
/// The edge of the neighbour's cube, in voxels.
const NEIGHBOUR_EDGE: i128 = 3;

fn speed_of(scene: &Scene, ship: GridId) -> f64 {
    let ship = scene.rig.server.world().grids().get(ship).unwrap();
    ship.linear_velocity
        .map(|component| component * component)
        .iter()
        .sum::<f64>()
        .sqrt()
}

fn on_a_rail(scene: &Scene, ship: GridId) -> bool {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship)
        .unwrap()
        .rail
        .is_some()
}

/// A cube of stone going round the planet in a circle at `DISTANCE`, for long enough that it has been put on a rail. Its number is 1.
fn a_coasting_ship() -> (Scene, GridId, f64) {
    let mut scene = Scene::in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    make_ship(
        &mut scene,
        GridPose::new(
            WorldPosition::from_voxels([-1.5, DISTANCE - 1.5, -1.5]),
            Orientation::IDENTITY,
        ),
        false,
        [3, 3, 3],
        |_| true,
    );
    let ship = ship_id(1);
    scene.run(move |world| {
        world
            .set_grid_velocity(ship, [speed, 0.0, 0.0], [0.0; 3])
            .unwrap();
    });
    scene.ticks(30);
    assert!(
        on_a_rail(&scene, ship),
        "the ship coasts clear of the ground, so it is on a rail"
    );
    (scene, ship, speed)
}

/// A cube of stone made `offset` voxels along the y axis from `ship` and going as it goes, in one tick (so that it is not left behind by a ship that goes 38 metres in a tick).
fn make_a_neighbour(scene: &mut Scene, ship: GridId, offset: f64) {
    let content = CaveContent::new();
    scene.run(move |world| {
        let (place, velocity) = {
            let ship = world.data().grids().get(ship).unwrap();
            (ship.pose.position, ship.linear_velocity)
        };
        let neighbour = world
            .create_grid(
                LevelIndex::MAIN,
                GridPose::new(place.translated([0.0, offset, 0.0]), Orientation::IDENTITY),
            )
            .unwrap();
        {
            let mut view = world.grid(neighbour).unwrap();
            for x in 0..NEIGHBOUR_EDGE {
                for y in 0..NEIGHBOUR_EDGE {
                    for z in 0..NEIGHBOUR_EDGE {
                        view.set(
                            content.block_channel(),
                            VoxelCoordinate::new(x, y, z),
                            content.blocks.stone,
                        )
                        .unwrap();
                    }
                }
            }
        }
        world
            .set_grid_velocity(neighbour, velocity, [0.0; 3])
            .unwrap();
    });
}

/// The rail is dropped by a ship that comes near (not by a push): the ship goes on at the speed it had.
#[test]
fn a_ship_taken_off_its_rail_by_a_neighbour_keeps_its_speed() {
    let (mut scene, ship, speed) = a_coasting_ship();
    let on_the_rail = speed_of(&scene, ship);
    assert!(
        (on_the_rail - speed).abs() < 1e-3 * speed,
        "the rail gives the speed the ship went with: {speed} m/s, and {on_the_rail} m/s on the rail"
    );
    make_a_neighbour(&mut scene, ship, 30.0);
    let mut dropped_at = None;
    for tick in 1..=12 {
        scene.ticks(1);
        if dropped_at.is_none() && !on_a_rail(&scene, ship) {
            dropped_at = Some(tick);
        }
        if dropped_at.is_some() {
            let now = speed_of(&scene, ship);
            assert!(
                (now - speed).abs() < 0.01 * speed,
                "tick {tick}: the ship went at {speed} m/s on its rail and goes at {now} m/s off it"
            );
        }
    }
    assert!(
        dropped_at.is_some(),
        "a ship 30 voxels away takes the ship off its rail"
    );
}

/// The control: a push that drops the rail gives the plugin a changed velocity, and the speed was kept before the fix too. The test keeps it so.
#[test]
fn a_ship_taken_off_its_rail_by_a_push_keeps_its_speed() {
    let (mut scene, ship, speed) = a_coasting_ship();
    scene.run(move |world| {
        let velocity = world.data().grids().get(ship).unwrap().linear_velocity;
        world
            .set_grid_velocity(
                ship,
                [velocity[0] + 0.01, velocity[1], velocity[2]],
                [0.0; 3],
            )
            .unwrap();
    });
    for tick in 1..=12 {
        scene.ticks(1);
        assert!(
            !on_a_rail(&scene, ship) || tick == 1,
            "tick {tick}: a push takes the ship off its rail"
        );
        let now = speed_of(&scene, ship);
        assert!(
            (now - speed).abs() < 0.01 * speed,
            "tick {tick}: the ship went at {speed} m/s and goes at {now} m/s"
        );
    }
}
