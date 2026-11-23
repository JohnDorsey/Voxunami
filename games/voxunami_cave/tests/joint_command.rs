//! The command `/joint decouple` (`docs/decisions/joints-and-cars.md`, "The command"): two cars coupled by their joint ends are parted when a player says so, they are not drawn
//! together or coupled again while they are still where they were, and are once they have been apart and are brought back. Which coupling is the one nearest the player's eyes
//! when a car has several.

mod common;

use std::collections::BTreeSet;

use common::chains;
use common::docking::{Placed, make, stones};
use common::joints::{ball_end_toward, joint_block};
use common::{
    FLOOR, Player, Scene, circular_speed, flat_floor, length, say, ship_id, tick_and_follow,
};
use server::ShipSettings;
use voxel_space::{
    BrickCoordinate, FaceDirection, GravitySettings, GridId, GridPose, Orientation, WorldPosition,
};

fn pose(x: f64, y: f64, z: f64) -> GridPose {
    GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z]),
        Orientation::IDENTITY,
    )
}

/// No gravity: cars hang in the air where they are put, and the only forces are the joint ends'.
fn weightless(scene: &mut Scene) {
    scene.run(|world| {
        world.set_gravity_settings(GravitySettings {
            base: [0.0; 3],
            ..GravitySettings::default()
        });
    });
}

/// The height of the cars above the floor.
const LEVEL: f64 = FLOOR + 10.0;

/// A car of `count` stones in a row along x with a joint end after them looking east (its pivot is at x = `count` + 1 in the car).
fn east_car(count: i128) -> Vec<Placed> {
    let mut blocks = stones(0, count);
    blocks.push((
        [count, 0, 0],
        joint_block(),
        ball_end_toward(FaceDirection::PositiveX),
    ));
    blocks
}

/// A car with a joint end looking west before `count` stones (its pivot is at x = 0 in the car).
fn west_car(count: i128) -> Vec<Placed> {
    let mut blocks = vec![(
        [0, 0, 0],
        joint_block(),
        ball_end_toward(FaceDirection::NegativeX),
    )];
    blocks.extend(stones(1, count + 1));
    blocks
}

/// A car with a joint end at each end, looking away from the middle (three stones between them).
fn middle_car() -> Vec<Placed> {
    let mut blocks = west_car(3);
    blocks.push((
        [4, 0, 0],
        joint_block(),
        ball_end_toward(FaceDirection::PositiveX),
    ));
    blocks
}

type Report = Box<dyn Fn() -> grid_bodies::BodiesReport>;

/// A player on the floor and cars in the air above it, with the player's eyes at `eyes_x` along the cars.
fn a_player_and_cars(eyes_x: f64, cars: Vec<(f64, Vec<Placed>)>) -> (Scene, Report, Player) {
    a_player_at_and_cars([eyes_x, FLOOR + 5.0, 30.0], cars)
}

/// A player at a place, flying, and cars in the air over the floor.
fn a_player_at_and_cars(place: [f64; 3], cars: Vec<(f64, Vec<Placed>)>) -> (Scene, Report, Player) {
    let (mut scene, report) = Scene::reporting();
    flat_floor(&mut scene);
    let mut player = scene.rig.join(place, 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    weightless(&mut scene);
    for (x, blocks) in cars {
        make(&mut scene, pose(x, LEVEL, 20.0), false, blocks);
    }
    (scene, Box::new(report), player)
}

/// Two cars with their joint ends three metres apart, and a player beside them: the east car is made first (ship 0).
fn two_cars() -> (Scene, Report, Player) {
    a_player_and_cars(22.0, vec![(20.0, east_car(3)), (27.0, west_car(3))])
}

/// Ticks (with the player following) until `count` couplings are made or `limit` ticks have gone.
fn until_coupled(
    scene: &mut Scene,
    player: &mut Player,
    report: &Report,
    count: u64,
    limit: usize,
) {
    let mut ticks = 0;
    while report().couplings < count && ticks < limit {
        tick_and_follow(scene, player, 1);
        ticks += 1;
    }
    assert_eq!(report().couplings, count, "after {ticks} ticks");
}

fn minus(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Where the middle of a car's voxel is in the world, in voxels.
fn world_of(scene: &Scene, id: GridId, local: [f64; 3]) -> [f64; 3] {
    let ship = scene.rig.server.world().grids().get(id).unwrap();
    let rotation = ship.pose.rotation_matrix();
    let origin = ship
        .pose
        .position
        .relative_to(WorldPosition::from_voxels([0.0; 3]));
    [0, 1, 2].map(|axis| {
        origin[axis]
            + (0..3)
                .map(|column| rotation[axis][column] * local[column])
                .sum::<f64>()
    })
}

fn pairs_the_world_has_as_coupled(scene: &mut Scene) -> BTreeSet<(GridId, GridId)> {
    let found = std::sync::Arc::new(std::sync::Mutex::new(BTreeSet::new()));
    let sink = std::sync::Arc::clone(&found);
    scene.run(move |world| {
        sink.lock()
            .unwrap()
            .extend(world.coupled_ships().iter().copied())
    });
    let found = found.lock().unwrap();
    found.clone()
}

#[test]
fn a_coupled_pair_is_parted_by_the_command_and_the_ends_that_are_still_together_do_not_couple_again()
 {
    let (mut scene, report, mut player) = two_cars();
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    assert_eq!(pairs_the_world_has_as_coupled(&mut scene).len(), 1);
    let made = report().couplings_made;
    let reply = say(&mut scene, &mut player, "/joint decouple");
    assert!(
        reply.contains("decoupled") && reply.contains("ships 0 and 1"),
        "{reply}"
    );
    assert_eq!(report().couplings, 0, "{reply}");
    assert!(
        pairs_the_world_has_as_coupled(&mut scene).is_empty(),
        "the world forgets the pair"
    );
    // The ends are within a centimetre of each other and in line, which is what makes a pair coupled again whatever else: for a long time (a good many seconds) they are not.
    tick_and_follow(&mut scene, &mut player, 200);
    assert_eq!(report().couplings, 0);
    assert_eq!(report().couplings_made, made, "no coupling was made again");
    // Nor are they pulled together or apart: nothing was done to them but let go of.
    let gap = {
        let first = world_of(&scene, ship_id(0), [4.0, 0.5, 0.5]);
        let second = world_of(&scene, ship_id(1), [0.0, 0.5, 0.5]);
        length(minus(first, second))
    };
    assert!(gap < 1.0, "the cars are {gap} m apart");
}

#[test]
fn what_was_parted_couples_again_once_the_cars_have_been_farther_apart_than_the_ends_pull_and_are_brought_back()
 {
    let (mut scene, report, mut player) = two_cars();
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    say(&mut scene, &mut player, "/joint decouple");
    assert_eq!(report().couplings, 0);
    // The second car is taken twenty metres off (the range of a pull is a few), and a little later put back where it was, at rest.
    let second = ship_id(1);
    let second_pose = scene.rig.server.world().grids().get(second).unwrap().pose;
    scene.run(move |world| {
        let mut far = second_pose;
        far.position = far.position.translated([20.0, 0.0, 0.0]);
        world.set_grid_pose(second, far).unwrap();
        world.set_grid_velocity(second, [0.0; 3], [0.0; 3]).unwrap();
    });
    tick_and_follow(&mut scene, &mut player, 5);
    assert_eq!(report().couplings, 0, "far, they are not coupled");
    scene.run(move |world| {
        world.set_grid_pose(second, second_pose).unwrap();
        world.set_grid_velocity(second, [0.0; 3], [0.0; 3]).unwrap();
        world
            .set_grid_velocity(ship_id(0), [0.0; 3], [0.0; 3])
            .unwrap();
    });
    until_coupled(&mut scene, &mut player, &report, 1, 100);
}

#[test]
fn the_command_says_so_when_nothing_is_coupled_near_and_changes_nothing() {
    let (mut scene, report, mut player) = two_cars();
    // (The cars are not yet coupled in the first tick: they are three metres apart, and the command is asked at once.)
    let reply = say(&mut scene, &mut player, "/joint decouple");
    assert!(reply.contains("coupled"), "{reply}");
    assert!(!reply.contains("decoupled"), "{reply}");
    until_coupled(&mut scene, &mut player, &report, 1, 600);
}

#[test]
fn a_coupling_farther_from_the_player_than_the_reach_is_not_parted() {
    let (mut scene, report, mut player) = a_player_at_and_cars(
        [24.0, LEVEL + 60.0, 22.0],
        vec![(20.0, east_car(3)), (27.0, west_car(3))],
    );
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    let reply = say(&mut scene, &mut player, "/joint decouple");
    assert!(reply.contains("nothing is coupled within"), "{reply}");
    tick_and_follow(&mut scene, &mut player, 5);
    assert_eq!(report().couplings, 1);
    assert_eq!(report().couplings_parted, 0);
}

#[test]
fn a_command_with_nothing_coupled_near_says_so() {
    let (mut scene, report) = Scene::reporting();
    flat_floor(&mut scene);
    let mut player = scene.rig.join([22.0, FLOOR + 5.0, 30.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    let reply = say(&mut scene, &mut player, "/joint decouple");
    assert!(reply.contains("nothing is coupled within"), "{reply}");
    assert_eq!(report().couplings, 0);
}

#[test]
fn only_the_coupling_nearest_the_player_is_parted_when_the_car_has_two() {
    // Three cars in a row, the middle one with an end at each end: a player beside the first pivot parts that, and one beside the second parts that.
    for (eyes_x, parted, kept) in [
        (25.0, (ship_id(0), ship_id(1)), (ship_id(1), ship_id(2))),
        (36.0, (ship_id(1), ship_id(2)), (ship_id(0), ship_id(1))),
    ] {
        let (mut scene, report, mut player) = a_player_and_cars(
            eyes_x,
            vec![
                (20.0, east_car(3)),
                (27.0, middle_car()),
                (36.0, west_car(3)),
            ],
        );
        until_coupled(&mut scene, &mut player, &report, 2, 900);
        let reply = say(&mut scene, &mut player, "/joint decouple");
        assert!(reply.contains("decoupled"), "{eyes_x}: {reply}");
        assert_eq!(report().couplings, 1, "{eyes_x}: {reply}");
        let left = pairs_the_world_has_as_coupled(&mut scene);
        assert_eq!(
            left.into_iter().collect::<Vec<_>>(),
            vec![kept],
            "{eyes_x}: parted {parted:?}: {reply}"
        );
    }
}

#[test]
fn a_usage_is_given_for_a_word_the_command_does_not_have() {
    let (mut scene, _report, mut player) = two_cars();
    let reply = say(&mut scene, &mut player, "/joint frobnicate");
    assert!(reply.contains("usage: /joint decouple"), "{reply}");
}

/// A player 10 voxels above a group that goes 38 voxels a tick, sent to where it will be a tick on (the command is heard a tick after, and the reach is 48 voxels).
fn keep_up_with(scene: &mut Scene, player: &mut Player, ships: &[GridId]) {
    let (place, going) = chains::centre_of(scene, ships);
    let ahead = 1.0 / f64::from(chains::TICKS_PER_SECOND);
    player.send_command(&format!(
        "/tp {} {} {}",
        place[0] + going[0] * ahead,
        place[1] + going[1] * ahead + 10.0,
        place[2] + going[2] * ahead
    ));
    tick_and_follow(scene, player, 1);
}

/// Where each car of a chain is and how it goes, how many couplings there are, and which cars have a rail.
type ChainAfter = (Vec<([f64; 3], [f64; 3])>, u64, Vec<bool>);

/// A chain of three cars on a rail round a planet, with a player keeping up with it, which is parted at the first joint if `part` (the command is said, and two ticks go by) and is not
/// otherwise; where each car is and how it goes, as the world has it, two ticks after the point where the command would be said, and the report's couplings and rails.
fn a_chain_on_a_rail_two_ticks_on(part: bool) -> ChainAfter {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let distance = 70_016.0;
    let speed = circular_speed(&scene, 0, distance);
    // (The player is there, and has settled, before the chain is made: how many ticks a join takes to settle is not the same from one run to the next, and a chain that goes 38 voxels a
    // tick is frozen when the player is 110 behind, which made the two runs of this differ by a whole number of ticks.)
    let mut player = scene.rig.join([0.0, distance + 10.0, 0.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    scene
        .rig
        .server
        .set_ship_settings(ShipSettings::new(100.0, 300.0).unwrap());
    let ships = chains::a_chain_in_orbit(&mut scene, chains::a_chain(), distance, speed);
    assert_eq!(report().couplings, 2);
    let rail = chains::put_on_its_rail(&mut scene, &ships, speed, chains::a_swing());
    for _ in 0..20 {
        keep_up_with(&mut scene, &mut player, &ships);
    }
    assert_eq!(
        scene.rig.server.world().grids().get(ships[0]).unwrap().rail,
        Some(rail)
    );
    if part {
        player.send_command("/joint decouple");
    }
    for _ in 0..2 {
        keep_up_with(&mut scene, &mut player, &ships);
    }
    let reply = player.take_command_replies().join(" | ");
    if part {
        assert!(reply.contains("decoupled"), "{reply}");
    }
    let railed = ships
        .iter()
        .map(|id| {
            scene
                .rig
                .server
                .world()
                .grids()
                .get(*id)
                .unwrap()
                .rail
                .is_some()
        })
        .collect();
    let cars = ships
        .iter()
        .map(|id| {
            let ship = scene.rig.server.world().grids().get(*id).unwrap();
            (ship.pose.position.in_voxels(), ship.linear_velocity)
        })
        .collect();
    (cars, report().couplings, railed)
}

#[test]
fn a_chain_on_a_rail_that_is_parted_goes_on_from_where_it_is_with_the_speed_it_had_and_loses_no_tick()
 {
    let (kept, kept_couplings, kept_rails) = a_chain_on_a_rail_two_ticks_on(false);
    let (parted, parted_couplings, parted_rails) = a_chain_on_a_rail_two_ticks_on(true);
    assert_eq!((kept_couplings, kept_rails), (2, vec![true, false, false]));
    assert_eq!(parted_couplings, 1);
    assert_eq!(
        parted_rails,
        vec![false; 3],
        "the group is off the rail it was on: it was for the group as a whole, on its anchor"
    );
    // A car that went a tick short, or was put at the rail's point for a car alone, is 38 voxels or metres off; parting is not a push, so what is left is the difference between a rail and
    // free flight over two ticks (measured: 0.00007 voxels and 0.002 metres a second; the tolerances are a hundred times that).
    for (index, (kept, parted)) in kept.iter().zip(&parted).enumerate() {
        let apart = length(minus(kept.0, parted.0));
        let slower = length(minus(kept.1, parted.1));
        assert!(
            apart < 0.01,
            "car {index} is {apart} voxels from where it was to be"
        );
        assert!(
            slower < 0.01,
            "car {index} goes {slower} m/s from how it was to go"
        );
    }
}
