//! A ship that coasts on a rail is found where its rail has taken it, whatever the players did (the ship audit of 2026-11-09, LIFE-1 and JOUR-1; the owner's rule of 2026-11-09
//! that a rail is a function of the clock, so a ship that went away keeps going).
//!
//! The lifecycle looks for a ship in a file by the place the world has of it, which is the place it had when it was put away, and a ship on a rail goes on: 20 seconds later it was 15
//! kilometres from that place, and a player who flew to where it truly was never brought it in, while one at the place it was put away at brought it in only to see its rail
//! take it away again (it was put in a file again within three ticks). **A ship on a rail is not put in a file by distance now** (it is frozen, as a planet is, and its rail goes on
//! putting it where the clock says), and **one that is in a file with a rail is brought in whatever the distance, as a planet is** (a world that was saved and loaded has all its
//! ships in files).

mod common;

use common::{Scene, circular_speed, make_ship, ship_id, tick_and_follow};
use voxel_space::{GridId, GridPose, Orientation, WorldPosition};
use world_storage::rail::{Rail, RailTime};

/// The distance from the middle of the planet at which a ship is clear of its ground and goes on a rail (see `orbit.rs`).
const DISTANCE: f64 = 70_000.0;
/// How near a player has to be to wake a ship, and how far to put it in a file (voxels).
const SIMULATION_DISTANCE: f64 = 100.0;
const UNLOAD_DISTANCE: f64 = 300.0;
/// Twenty seconds, in ticks.
const TWENTY_SECONDS: usize = 400;
/// The most the origin of the cube of three that is the ship may be from the middle of its mass, which is where the rail puts it, with the rest of the error in voxels.
const PLACE_TOLERANCE: f64 = 5.0;

fn length(vector: [f64; 3]) -> f64 {
    vector.iter().map(|part| part * part).sum::<f64>().sqrt()
}

fn apart(first: [f64; 3], second: [f64; 3]) -> f64 {
    length([0, 1, 2].map(|axis| first[axis] - second[axis]))
}

/// Where `rail` says the middle of its ship is at the world's clock now, in voxels (the planet it goes round has not moved: it is held).
fn middle_by_the_rail(scene: &Scene, rail: &Rail) -> [f64; 3] {
    let planet_at = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(0))
        .expect("the planet is in memory")
        .pose
        .position
        .in_voxels();
    rail.state(
        RailTime::of_tick(scene.rig.server.world().clock(), 20),
        planet_at,
        [0.0; 3],
    )
    .position
    .expect("an orbit has a place")
}

/// A ship on a rail that its player left, with the lifecycle's distances short, 20 seconds after: the scene, the player, the ship's number, its rail, and where the rail puts its
/// middle now. The ship is 15 kilometres from where it was when the player left it.
fn a_ship_that_went_on() -> (Scene, common::Player, GridId, Rail, [f64; 3]) {
    let mut scene = Scene::in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    let mut player = scene.rig.join([0.0, DISTANCE + 30.0, 0.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
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
    tick_and_follow(&mut scene, &mut player, 30);
    let (started, rail) = {
        let found = scene.rig.server.world().grids().get(ship).unwrap();
        (
            found.pose.position.in_voxels(),
            found
                .rail
                .clone()
                .expect("the ship coasts clear of the ground, so it is on a rail"),
        )
    };
    scene.rig.server.set_ship_settings(
        server::ShipSettings::new(SIMULATION_DISTANCE, UNLOAD_DISTANCE).unwrap(),
    );
    tick_and_follow(&mut scene, &mut player, TWENTY_SECONDS);
    let middle = middle_by_the_rail(&scene, &rail);
    let gone = apart(middle, started);
    assert!(
        gone > 10_000.0,
        "the ship went far from where the player left it ({gone} voxels), or the test says nothing"
    );
    (scene, player, ship, rail, middle)
}

/// A ship on a rail that every player is far from is not put in a file, where nothing could say where it goes; it is frozen, and where its rail puts it.
#[test]
fn a_ship_on_a_rail_far_from_every_player_is_frozen_and_not_put_in_a_file() {
    let (scene, _player, ship, _, middle) = a_ship_that_went_on();
    let world = scene.rig.server.world();
    assert!(
        world.grids().get_parked(ship).is_none(),
        "the ship was put in a file"
    );
    let found = world.grids().get(ship).expect("the ship is in memory");
    assert!(
        found.frozen,
        "the ship is far from every player, so it is frozen"
    );
    assert!(found.rail.is_some(), "the ship keeps its rail");
    let error = apart(found.pose.position.in_voxels(), middle);
    assert!(
        error < PLACE_TOLERANCE,
        "the ship is {error} voxels from where its rail puts it"
    );
}

/// A player who flies to where the ship truly is wakes it.
#[test]
fn a_player_at_the_place_of_a_ship_on_a_rail_wakes_it() {
    let (mut scene, mut player, ship, _, middle) = a_ship_that_went_on();
    player.send_command(&format!(
        "/tp {} {} {}",
        middle[0],
        middle[1] + 30.0,
        middle[2]
    ));
    let mut woke_at = None;
    for tick in 1..=8 {
        tick_and_follow(&mut scene, &mut player, 1);
        let awake = scene
            .rig
            .server
            .world()
            .grids()
            .get(ship)
            .is_some_and(|found| !found.frozen);
        if woke_at.is_none() && awake {
            woke_at = Some(tick);
        }
    }
    assert!(
        woke_at.is_some(),
        "a player 30 voxels from where the rail puts the ship does not wake it"
    );
}

/// A world that was saved has its ships in files, and a ship on a rail is brought in at once, wherever the players are (there are none): it is where its rail says, and not where
/// it was when the world was saved.
#[test]
fn a_ship_on_a_rail_is_brought_in_by_a_restart_and_is_where_its_rail_says() {
    let (scene, _player, ship, rail, _) = a_ship_that_went_on();
    let mut scene = scene.restarted();
    // (A new server brings in every ship, as a `Rig` has it: the distances of a server that is run are the short ones.)
    scene.rig.server.set_ship_settings(
        server::ShipSettings::new(SIMULATION_DISTANCE, UNLOAD_DISTANCE).unwrap(),
    );
    scene.ticks(3);
    let world = scene.rig.server.world();
    assert!(
        world.grids().get_parked(ship).is_none(),
        "the ship of a rail is still in its file after the restart"
    );
    let found = world.grids().get(ship).expect("the ship is brought in");
    assert!(found.rail.is_some(), "the ship keeps its rail");
    let error = apart(
        found.pose.position.in_voxels(),
        middle_by_the_rail(&scene, &rail),
    );
    assert!(
        error < PLACE_TOLERANCE,
        "after the restart the ship is {error} voxels from where its rail puts it"
    );
}
