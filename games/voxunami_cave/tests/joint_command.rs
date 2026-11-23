//! The command `/joint decouple` (`docs/decisions/joints-and-cars.md`, "The command"): two cars coupled by their joint ends are parted when a player says so, they are not drawn
//! together or coupled again while they are still where they were, and are once they have been apart and are brought back. Which coupling is the one nearest the player's eyes
//! when a car has several.

mod common;

use common::joints::{
    LEVEL, a_chain_on_a_rail, a_player_and_cars, a_player_at_and_cars, east_car, keep_up_with,
    middle_car, minus, pairs_the_world_has_as_coupled, two_cars, until_coupled, west_car, world_of,
};
use common::{FLOOR, Scene, flat_floor, length, say, ship_id, tick_and_follow};

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
    assert!(
        reply.contains("usage: /joint lock|unlock|decouple"),
        "{reply}"
    );
}

/// Where each car of a chain is and how it goes, how many couplings there are, and which cars have a rail.
type ChainAfter = (Vec<([f64; 3], [f64; 3])>, u64, Vec<bool>);

/// A chain of three cars on a rail round a planet, with a player keeping up with it, which is parted at the first joint if `part` (the command is said, and two ticks go by) and is not
/// otherwise; where each car is and how it goes, as the world has it, two ticks after the point where the command would be said, and the report's couplings and rails.
fn a_chain_on_a_rail_two_ticks_on(part: bool) -> ChainAfter {
    let mut chain = a_chain_on_a_rail();
    if part {
        chain.player.send_command("/joint decouple");
    }
    for _ in 0..2 {
        keep_up_with(&mut chain.scene, &mut chain.player, &chain.ships);
    }
    let reply = chain.player.take_command_replies().join(" | ");
    if part {
        assert!(reply.contains("decoupled"), "{reply}");
    }
    let railed = chain
        .ships
        .iter()
        .map(|id| {
            chain
                .scene
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
    let cars = chain
        .ships
        .iter()
        .map(|id| {
            let ship = chain.scene.rig.server.world().grids().get(*id).unwrap();
            (ship.pose.position.in_voxels(), ship.linear_velocity)
        })
        .collect();
    (cars, (chain.report)().couplings, railed)
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
