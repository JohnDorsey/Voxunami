//! The commands `/joint immobilize [SECONDS]` and `/joint mobilize` (`docs/decisions/joints-and-cars.md`, "The command"): a coupled pair is made rigid at the turn it has, after the
//! swing has been braked over a time the player chooses (one second if they say nothing, none if they say 0), and is let go to turn again; a pair that was made rigid stays so while its
//! ships are in files and when the world is saved and loaded. Lock is refused on a joint that is rigid, and mobilize takes back a lock that was asked.

mod common;

use common::docking::ships;
use common::joints::{
    a_chain_on_a_rail, a_player_and_cars, east_car, keep_up_with, two_cars, until_coupled, west_car,
};
use common::{Player, Scene, length, say, ship_id, tick_and_follow};
use server::ShipSettings;
use voxel_space::GridId;

/// The turn of the second ship in the axes of the first, as nine numbers (what a rigid joint keeps and a ball does not).
fn the_second_in_the_first(scene: &Scene, first: GridId, second: GridId) -> [[f64; 3]; 3] {
    let world = scene.rig.server.world();
    let a = world.grids().get(first).unwrap().pose.rotation_matrix();
    let b = world.grids().get(second).unwrap().pose.rotation_matrix();
    [0, 1, 2]
        .map(|row| [0, 1, 2].map(|column| (0..3).map(|k| a[k][row] * b[k][column]).sum::<f64>()))
}

fn most_different(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> f64 {
    (0..3)
        .flat_map(|row| (0..3).map(move |column| (row, column)))
        .map(|(row, column)| (a[row][column] - b[row][column]).abs())
        .fold(0.0, f64::max)
}

/// How fast the second ship turns against the first, in radians a second.
fn spin_between(scene: &Scene, first: GridId, second: GridId) -> f64 {
    let world = scene.rig.server.world();
    let (a, b) = (
        world.grids().get(first).unwrap().angular_velocity,
        world.grids().get(second).unwrap().angular_velocity,
    );
    length([b[0] - a[0], b[1] - a[1], b[2] - a[2]])
}

fn pairs_that_are_rigid(scene: &Scene) -> Vec<(GridId, GridId)> {
    scene
        .rig
        .server
        .world()
        .grids()
        .immobilized()
        .iter()
        .copied()
        .collect()
}

/// Two cars coupled and then set swinging about their ball: the second turns at 0.6 radians a second about y.
fn two_cars_swinging() -> (Scene, common::joints::Report, Player) {
    let (mut scene, report, mut player) = two_cars();
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    tick_and_follow(&mut scene, &mut player, 5);
    scene.run(|world| {
        world
            .set_grid_velocity(ship_id(1), [0.0; 3], [0.0, 0.6, 0.0])
            .unwrap();
    });
    tick_and_follow(&mut scene, &mut player, 5);
    (scene, report, player)
}

/// Ticks (with the player following) until the pair is rigid in the world, or `limit` ticks have gone; how many it took.
fn until_rigid(scene: &mut Scene, player: &mut Player, limit: usize) -> usize {
    let mut ticks = 0;
    while pairs_that_are_rigid(scene).is_empty() && ticks < limit {
        tick_and_follow(scene, player, 1);
        ticks += 1;
    }
    ticks
}

#[test]
fn a_pair_that_is_immobilized_with_no_time_keeps_the_turn_it_has_at_once_and_a_ball_swings_on() {
    let (mut control, _report, mut control_player) = two_cars_swinging();
    let (mut scene, _report, mut player) = two_cars_swinging();
    let at_the_command = the_second_in_the_first(&scene, ship_id(0), ship_id(1));
    let reply = say(&mut scene, &mut player, "/joint immobilize 0");
    assert!(reply.contains("immobilized ships 0 and 1"), "{reply}");
    tick_and_follow(&mut scene, &mut player, 3);
    assert_eq!(pairs_that_are_rigid(&scene), vec![(ship_id(0), ship_id(1))]);
    let held = the_second_in_the_first(&scene, ship_id(0), ship_id(1));
    tick_and_follow(&mut scene, &mut player, 100);
    tick_and_follow(&mut control, &mut control_player, 103);
    // The control swung on (what the test is a test of), and the pair that was made rigid did not.
    assert!(
        most_different(
            the_second_in_the_first(&control, ship_id(0), ship_id(1)),
            at_the_command
        ) > 0.05,
        "the ball did not swing"
    );
    let moved = most_different(
        the_second_in_the_first(&scene, ship_id(0), ship_id(1)),
        held,
    );
    assert!(moved < 1e-3, "the turn between the cars moved by {moved}");
    assert!(spin_between(&scene, ship_id(0), ship_id(1)) < 1e-3);
}

#[test]
fn a_pair_that_is_immobilized_over_a_time_slows_its_swing_over_it_and_holds_the_turn_it_stopped_at()
{
    let (mut scene, _report, mut player) = two_cars_swinging();
    let before = spin_between(&scene, ship_id(0), ship_id(1));
    assert!(before > 0.3, "the pair swings at {before}");
    let reply = say(&mut scene, &mut player, "/joint immobilize 1");
    assert!(
        reply.contains("immobilizing ships 0 and 1 over 1 second"),
        "{reply}"
    );
    // Half a second on (ten ticks): slower, and not rigid yet.
    tick_and_follow(&mut scene, &mut player, 10);
    let halfway = spin_between(&scene, ship_id(0), ship_id(1));
    assert!(
        halfway < before * 0.8 && halfway > 0.0,
        "{before} became {halfway}"
    );
    assert!(pairs_that_are_rigid(&scene).is_empty());
    // It is rigid a second after the command, within a few ticks, and the swing is gone.
    let ticks = until_rigid(&mut scene, &mut player, 30);
    assert!((6..=14).contains(&ticks), "rigid after {ticks} more ticks");
    tick_and_follow(&mut scene, &mut player, 3);
    let held = the_second_in_the_first(&scene, ship_id(0), ship_id(1));
    tick_and_follow(&mut scene, &mut player, 100);
    assert!(
        most_different(
            the_second_in_the_first(&scene, ship_id(0), ship_id(1)),
            held
        ) < 1e-3
    );
    assert!(spin_between(&scene, ship_id(0), ship_id(1)) < 1e-3);
}

#[test]
fn a_pair_that_is_immobilized_with_no_word_of_time_takes_a_second() {
    let (mut scene, _report, mut player) = two_cars_swinging();
    let reply = say(&mut scene, &mut player, "/joint immobilize");
    assert!(reply.contains("over 1 second"), "{reply}");
    tick_and_follow(&mut scene, &mut player, 8);
    assert!(pairs_that_are_rigid(&scene).is_empty(), "not after 8 ticks");
    until_rigid(&mut scene, &mut player, 30);
    assert_eq!(pairs_that_are_rigid(&scene).len(), 1);
}

#[test]
fn a_pair_that_is_not_swinging_is_immobilized_at_once_whatever_time_is_asked() {
    let (mut scene, report, mut player) = two_cars();
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    tick_and_follow(&mut scene, &mut player, 60);
    assert!(spin_between(&scene, ship_id(0), ship_id(1)) < 1e-3);
    say(&mut scene, &mut player, "/joint immobilize 30");
    let ticks = until_rigid(&mut scene, &mut player, 30);
    assert!(
        ticks <= 3,
        "rigid after {ticks} ticks, with nothing to brake"
    );
}

#[test]
fn a_pair_that_is_mobilized_turns_again_as_a_ball_and_a_lock_asked_of_it_is_taken_back() {
    let (mut scene, report, mut player) = two_cars_swinging();
    say(&mut scene, &mut player, "/joint immobilize 0");
    tick_and_follow(&mut scene, &mut player, 3);
    assert_eq!(pairs_that_are_rigid(&scene).len(), 1);
    let reply = say(&mut scene, &mut player, "/joint mobilize");
    assert!(reply.contains("mobilized ships 0 and 1"), "{reply}");
    tick_and_follow(&mut scene, &mut player, 2);
    assert!(pairs_that_are_rigid(&scene).is_empty());
    let held = the_second_in_the_first(&scene, ship_id(0), ship_id(1));
    scene.run(|world| {
        world
            .set_grid_velocity(ship_id(1), [0.0; 3], [0.0, 0.6, 0.0])
            .unwrap();
    });
    tick_and_follow(&mut scene, &mut player, 20);
    assert!(
        most_different(
            the_second_in_the_first(&scene, ship_id(0), ship_id(1)),
            held
        ) > 0.05,
        "it did not swing"
    );
    assert_eq!(report().couplings, 1, "still coupled");
    // Nothing to mobilize now.
    let reply = say(&mut scene, &mut player, "/joint mobilize");
    assert!(reply.contains("nothing is immobilized"), "{reply}");
}

#[test]
fn mobilize_takes_back_a_time_that_is_being_braked_and_leaves_the_pair_a_ball() {
    let (mut scene, _report, mut player) = two_cars_swinging();
    say(&mut scene, &mut player, "/joint immobilize 5");
    tick_and_follow(&mut scene, &mut player, 5);
    let reply = say(&mut scene, &mut player, "/joint mobilize");
    assert!(reply.contains("not to be immobilized any more"), "{reply}");
    tick_and_follow(&mut scene, &mut player, 120);
    assert!(pairs_that_are_rigid(&scene).is_empty(), "never made rigid");
}

#[test]
fn lock_is_refused_on_a_pair_that_is_rigid_or_being_braked_and_is_asked_when_it_is_mobile_again() {
    let (mut scene, _report, mut player) = two_cars_swinging();
    say(&mut scene, &mut player, "/joint immobilize 0");
    tick_and_follow(&mut scene, &mut player, 3);
    let reply = say(&mut scene, &mut player, "/joint lock");
    assert!(reply.contains("mobilize them first"), "{reply}");
    say(&mut scene, &mut player, "/joint mobilize");
    say(&mut scene, &mut player, "/joint immobilize 5");
    let reply = say(&mut scene, &mut player, "/joint lock");
    assert!(reply.contains("mobilize them first"), "{reply}");
    say(&mut scene, &mut player, "/joint mobilize");
    let reply = say(&mut scene, &mut player, "/joint lock");
    assert!(reply.contains("locking ships 0 and 1"), "{reply}");
}

#[test]
fn immobilize_takes_back_a_lock_that_was_asked_and_the_pair_is_not_locked_afterwards() {
    let (mut scene, report, mut player) = two_cars_swinging();
    assert!(say(&mut scene, &mut player, "/joint lock").contains("locking"));
    let reply = say(&mut scene, &mut player, "/joint immobilize 0");
    assert!(
        reply.contains("the lock that was asked of them is taken back"),
        "{reply}"
    );
    tick_and_follow(&mut scene, &mut player, 150);
    assert_eq!(ships(&scene).len(), 2, "not made one ship");
    assert_eq!(report().locks_made, 0);
    assert_eq!(pairs_that_are_rigid(&scene).len(), 1);
}

#[test]
fn decouple_parts_a_pair_that_is_rigid_and_nothing_stiff_is_left_to_come_back_when_it_is_coupled_again()
 {
    let (mut scene, report, mut player) = two_cars_swinging();
    say(&mut scene, &mut player, "/joint immobilize 0");
    tick_and_follow(&mut scene, &mut player, 3);
    assert!(say(&mut scene, &mut player, "/joint decouple").contains("decoupled"));
    tick_and_follow(&mut scene, &mut player, 3);
    assert_eq!(report().couplings, 0);
    assert!(pairs_that_are_rigid(&scene).is_empty());
}

#[test]
fn a_ship_that_is_unlocked_into_two_cars_has_them_coupled_mobile() {
    let mut blocks = east_car(3);
    blocks.extend(
        west_car(3)
            .into_iter()
            .map(|([x, y, z], block, look)| ([x + 4, y, z], block, look)),
    );
    let (mut scene, report, mut player) = a_player_and_cars(24.0, vec![(20.0, blocks)]);
    tick_and_follow(&mut scene, &mut player, 3);
    // A locked ship is rigid already: it says so, and does nothing to it.
    let reply = say(&mut scene, &mut player, "/joint immobilize");
    assert!(reply.contains("is locked there"), "{reply}");
    assert!(say(&mut scene, &mut player, "/joint unlock").contains("unlocked"));
    until_coupled(&mut scene, &mut player, &report, 1, 100);
    assert!(pairs_that_are_rigid(&scene).is_empty(), "coupled (mobile)");
}

#[test]
fn the_commands_say_so_when_nothing_is_coupled_near_and_the_time_must_be_a_number_that_is_not_below_zero()
 {
    let (mut scene, _report, mut player) = two_cars();
    for command in [
        "/joint immobilize",
        "/joint immobilize 0",
        "/joint mobilize",
    ] {
        let reply = say(&mut scene, &mut player, command);
        assert!(
            reply.contains("nothing is coupled within")
                || reply.contains("nothing is immobilized within"),
            "{command}: {reply}"
        );
    }
    let (mut scene, report, mut player) = two_cars();
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    for command in [
        "/joint immobilize -1",
        "/joint immobilize soon",
        "/joint immobilize 1 2",
    ] {
        let reply = say(&mut scene, &mut player, command);
        assert!(reply.contains("usage: /joint"), "{command}: {reply}");
    }
    assert!(pairs_that_are_rigid(&scene).is_empty());
    assert!(
        say(&mut scene, &mut player, "/joint frobnicate")
            .contains("usage: /joint lock|unlock|mobilize|immobilize [SECONDS]|decouple")
    );
}

#[test]
fn a_pair_that_was_made_rigid_is_rigid_again_when_its_ships_come_back_from_files() {
    let (mut scene, report, mut player) = two_cars_swinging();
    scene
        .rig
        .server
        .set_ship_settings(ShipSettings::new(100.0, 300.0).unwrap());
    say(&mut scene, &mut player, "/joint immobilize 0");
    tick_and_follow(&mut scene, &mut player, 3);
    assert_eq!(pairs_that_are_rigid(&scene).len(), 1);
    // The player goes away and the cars are put in files, with the pair remembered, and rigid.
    player.send_command("/tp 22 205 2000");
    let mut ticks = 0;
    while scene
        .rig
        .server
        .world()
        .grids()
        .get_parked(ship_id(0))
        .is_none()
        && ticks < 300
    {
        tick_and_follow(&mut scene, &mut player, 1);
        ticks += 1;
    }
    assert!(
        scene
            .rig
            .server
            .world()
            .grids()
            .get_parked(ship_id(0))
            .is_some(),
        "parked after {ticks} ticks"
    );
    assert_eq!(
        pairs_that_are_rigid(&scene).len(),
        1,
        "still rigid in files"
    );
    player.send_command("/tp 22 205 30");
    let mut ticks = 0;
    while report().couplings != 1 && ticks < 300 {
        tick_and_follow(&mut scene, &mut player, 1);
        ticks += 1;
    }
    assert_eq!(report().couplings, 1, "coupled again after {ticks} ticks");
    // Turned by a torque on one car, a ball would swing and a rigid pair does not.
    tick_and_follow(&mut scene, &mut player, 5);
    let held = the_second_in_the_first(&scene, ship_id(0), ship_id(1));
    for _ in 0..20 {
        scene.run(|world| {
            world
                .apply_grid_torque(ship_id(1), [0.0, 3000.0, 0.0])
                .unwrap();
        });
    }
    let moved = most_different(
        the_second_in_the_first(&scene, ship_id(0), ship_id(1)),
        held,
    );
    assert!(
        moved < 0.01,
        "the cars turned against each other by {moved}"
    );
}

#[test]
fn a_chain_on_a_rail_is_immobilized_at_a_joint_and_the_cars_of_that_joint_keep_their_turn() {
    let mut chain = a_chain_on_a_rail();
    chain.player.send_command("/joint immobilize 0");
    let mut now = chain.ships.clone();
    for _ in 0..3 {
        keep_up_with(&mut chain.scene, &mut chain.player, &now);
        now = chain.ships.clone();
    }
    let reply = chain.player.take_command_replies().join(" | ");
    assert!(reply.contains("immobilized ships"), "{reply}");
    let pairs = pairs_that_are_rigid(&chain.scene);
    assert_eq!(pairs.len(), 1, "{reply}");
    let (first, second) = pairs[0];
    // The group swings (the chain is swinging in its rail); the two cars of the rigid joint do not turn against each other.
    keep_up_with(&mut chain.scene, &mut chain.player, &now);
    let held = the_second_in_the_first(&chain.scene, first, second);
    for _ in 0..40 {
        keep_up_with(&mut chain.scene, &mut chain.player, &now);
    }
    let moved = most_different(the_second_in_the_first(&chain.scene, first, second), held);
    assert!(
        moved < 1e-3,
        "the cars turned against each other by {moved}"
    );
}
