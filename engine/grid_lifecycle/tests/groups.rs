//! Ships that are coupled by their joint ends are kept as one: the lifecycle freezes, thaws, puts in files and brings in a group together, by the distance of its nearest ship.
//! The server world holds which ships are coupled (`ServerWorld::coupled_ships`, written by the bodies plugin); these tests write it themselves, so what is tested is only what the
//! lifecycle does with it. Every test has a control: the same ships with no coupling, which are kept one by one.

mod common;

use common::Rig;
use server::ShipSettings;
use voxel_space::GridId;

/// Simulation distance 100, so a ship that moves stops when no player is within 110; unload distance 300, so a ship is put in a file beyond 330.
fn settings() -> ShipSettings {
    ShipSettings::new(100.0, 300.0).unwrap()
}

/// A rig with ships at these bricks along x (each is 64 voxels long along x), and the first ship of each pair of neighbours in the list coupled to the next, as of the
/// first tick of the rig, when `coupled` is true.
fn rig_of(bricks: &[i128], coupled: bool) -> (Rig, Vec<GridId>) {
    let (mut rig, ships) = Rig::new(settings(), bricks);
    if coupled {
        let ships_to_couple = ships.clone();
        rig.in_a_tick(move |world| {
            for pair in ships_to_couple.windows(2) {
                world.set_ships_coupled(pair[0], pair[1], true);
            }
        });
    }
    (rig, ships)
}

#[test]
fn a_ship_coupled_to_one_that_is_near_a_player_stays_awake_however_far_it_is() {
    // Boxes along x: 0..64 and 640..704. A player 50 before the first is 690 from the second.
    let (mut rig, ships) = rig_of(&[0, 20], true);
    rig.player_at(-50.0);
    rig.tick();
    rig.tick();
    assert!(rig.in_memory(ships[1]) && !rig.frozen(ships[1]));
    assert!(rig.in_memory(ships[0]) && !rig.frozen(ships[0]));

    // The control: with no coupling the second is frozen at once and put in a file after.
    let (mut control, ships) = rig_of(&[0, 20], false);
    control.player_at(-50.0);
    control.tick();
    assert!(control.in_a_file(ships[1]));
    assert!(!control.frozen(ships[0]));
}

#[test]
fn a_chain_of_coupled_ships_is_kept_by_the_nearest_whichever_end_it_is() {
    // Four ships in a chain, at 0..64, 320..384, 640..704 and 960..1024 along x; a player is near the last one.
    let (mut rig, ships) = rig_of(&[0, 10, 20, 30], true);
    // A player at 1024 + 50 is 50 from the last (960..1024) and over 700 from the third.
    rig.player_at(1024.0 + 50.0);
    rig.tick();
    rig.tick();
    for ship in &ships {
        assert!(
            rig.in_memory(*ship) && !rig.frozen(*ship),
            "{ship} is part of a group with a ship that is near"
        );
    }
    let (mut control, ships) = rig_of(&[0, 10, 20, 30], false);
    control.player_at(1024.0 + 50.0);
    control.tick();
    control.tick();
    assert!(control.in_a_file(ships[0]) && control.in_a_file(ships[1]));
}

#[test]
fn a_group_is_frozen_together_only_when_every_ship_of_it_is_beyond_the_distance() {
    let (mut rig, ships) = rig_of(&[0, 20], true);
    // (The ships were frozen before the player had a place, which is when the rig begins: the player comes near, and they are let go.)
    rig.player_at(-50.0);
    assert!(!rig.frozen(ships[0]) && !rig.frozen(ships[1]));
    let before = rig.report();
    // 109 from the first is within the 110 that a moving ship stops beyond.
    rig.player_at(-109.0);
    assert!(!rig.frozen(ships[0]) && !rig.frozen(ships[1]));
    // 111 from the first and 751 from the second: both stop, in one tick.
    rig.player_at(-111.0);
    assert!(rig.frozen(ships[0]) && rig.frozen(ships[1]));
    assert_eq!(rig.report().frozen - before.frozen, 2);
    // Near the second ship instead, which is the nearest now: the group is let go, both, by the distance of the nearest.
    rig.player_at(640.0 + 64.0 + 99.0);
    assert!(!rig.frozen(ships[0]) && !rig.frozen(ships[1]));
    assert_eq!(rig.report().thawed - before.thawed, 2);

    // The control: the first is frozen alone, and is not let go by a player who is near the second (it is put in a file, being 739 away): only the second is awake.
    let (mut control, ships) = rig_of(&[0, 20], false);
    control.player_at(-111.0);
    control.player_at(640.0 + 64.0 + 99.0);
    assert!(control.in_a_file(ships[0]) && !control.frozen(ships[1]));
}

#[test]
fn a_frozen_group_is_not_let_go_between_the_two_distances() {
    // (The hysteresis is the group's: all frozen, a player has to come within the simulation distance of the nearest ship, not within a tenth beyond.)
    let (mut rig, ships) = rig_of(&[0, 20], true);
    rig.player_at(-320.0);
    assert!(rig.frozen(ships[0]) && rig.frozen(ships[1]));
    rig.player_at(-105.0);
    assert!(rig.frozen(ships[0]) && rig.frozen(ships[1]));
    rig.player_at(-99.0);
    assert!(!rig.frozen(ships[0]) && !rig.frozen(ships[1]));
}

#[test]
fn a_ship_of_a_group_that_was_let_go_alone_wakes_the_ones_it_is_coupled_to_when_a_player_is_within_the_distance_it_stops_at()
 {
    // A ship comes awake without its group (for instance it was brought in from its file for the pilot who went away with it): a frozen ship coupled to an awake one
    // would hold it fixed, so the group goes the way of its awake ship, as long as the nearest ship is within the distance a moving ship stops at.
    let (mut rig, ships) = rig_of(&[0, 20], true);
    rig.player_at(-320.0);
    rig.player_at(-105.0);
    assert!(rig.frozen(ships[0]) && rig.frozen(ships[1]));
    let first = ships[0];
    rig.in_a_tick(move |world| {
        world.set_grid_frozen(first, false).unwrap();
    });
    assert!(!rig.frozen(ships[0]));
    assert!(
        !rig.frozen(ships[1]),
        "the ship coupled to an awake ship is awake"
    );

    // The control: with no coupling the second ship is put in a file (740 away) by the time the first is awake, and nothing wakes it.
    let (mut control, ships) = rig_of(&[0, 20], false);
    control.player_at(-320.0);
    control.player_at(-105.0);
    let first = ships[0];
    control.in_a_tick(move |world| {
        world.set_grid_frozen(first, false).unwrap();
    });
    assert!(!control.frozen(ships[0]) && control.in_a_file(ships[1]));
}

#[test]
fn a_group_is_put_in_files_together_and_comes_back_together() {
    let (mut rig, ships) = rig_of(&[0, 20], true);
    // Both beyond the distance a frozen ship is put away at: frozen, then in files, in the next tick.
    rig.player_at(-500.0);
    rig.tick();
    assert!(rig.in_a_file(ships[0]) && rig.in_a_file(ships[1]));
    assert_eq!(rig.report().parked, 2);
    // Within 100 of the first ship's place: both come in, and awake. The second is 600 away.
    rig.player_at(-99.0);
    assert!(rig.in_memory(ships[0]) && rig.in_memory(ships[1]));
    assert!(!rig.frozen(ships[0]) && !rig.frozen(ships[1]));
    assert_eq!(rig.report().brought_in, 2);
    assert!(rig.report().problems.is_empty());

    // The control: only the near one comes back.
    let (mut control, ships) = rig_of(&[0, 20], false);
    control.player_at(-500.0);
    control.tick();
    control.player_at(-99.0);
    assert!(control.in_memory(ships[0]) && control.in_a_file(ships[1]));
}

#[test]
fn a_group_is_not_put_away_while_any_of_its_ships_is_within_the_distance_it_is_kept_at() {
    // Boxes 0..64 and 960..1024. A player 320 before the first: frozen, and between 300 and 330, so kept in memory; the second is 1280 away.
    let (mut rig, ships) = rig_of(&[0, 30], true);
    rig.player_at(-320.0);
    rig.tick();
    rig.tick();
    assert!(rig.in_memory(ships[0]) && rig.frozen(ships[0]));
    assert!(rig.in_memory(ships[1]) && rig.frozen(ships[1]));
    // 331 from the first: the whole group goes.
    rig.player_at(-331.0);
    assert!(rig.in_a_file(ships[0]) && rig.in_a_file(ships[1]));

    let (mut control, ships) = rig_of(&[0, 30], false);
    control.player_at(-320.0);
    control.tick();
    control.tick();
    assert!(control.in_memory(ships[0]) && control.in_a_file(ships[1]));
}

#[test]
fn a_ship_in_a_file_counts_for_the_distance_of_its_group_and_one_that_waits_for_its_pilot_is_not_brought_in()
 {
    // The first ship went away with its pilot (it waits in its file) and the second is coupled to it. A player who comes near where the first is has the second brought in,
    // and the first stays in its file.
    let (mut rig, ships) = rig_of(&[0, 20], true);
    let first = ships[0];
    rig.in_a_tick(move |world| {
        world.park_ship(first, true).unwrap();
    });
    rig.player_at(-500.0);
    rig.tick();
    assert!(rig.in_a_file(ships[0]) && rig.in_a_file(ships[1]));
    rig.player_at(-99.0);
    assert!(rig.in_a_file(ships[0]), "it waits for whoever is aboard");
    assert!(rig.in_memory(ships[1]) && !rig.frozen(ships[1]));
}

/// A rail that only turns: it is a rail, and it places nothing (these ships have no planet to go round).
fn a_rail() -> world_storage::rail::Rail {
    world_storage::rail::Rail {
        spin: Some(world_storage::rail::Spin::new([0.0, 1.0, 0.0], 3600.0, 0.0).unwrap()),
        orbit: None,
    }
}

#[test]
fn a_group_with_a_ship_on_a_rail_is_not_put_in_a_file_though_the_others_have_none() {
    // Two ships coupled, the first on a rail (the anchor of a group on a rail holds the rail of its centre of mass). No player is near: a ship on a rail is not put away, and
    // the ship coupled to it would be a ship in a file whose place the group needs, so it is not put away either.
    let (mut rig, ships) = rig_of(&[0, 20], true);
    let first = ships[0];
    rig.in_a_tick(move |world| world.set_grid_rail(first, Some(a_rail())).unwrap());
    rig.player_at(-500.0);
    rig.tick();
    rig.tick();
    assert!(rig.in_memory(ships[0]) && rig.frozen(ships[0]));
    assert!(rig.in_memory(ships[1]) && rig.frozen(ships[1]));
    assert_eq!(rig.report().parked, 0);

    // The control: with no rail on the first, both are put in files.
    let (mut control, ships) = rig_of(&[0, 20], true);
    control.player_at(-500.0);
    control.tick();
    control.tick();
    assert!(control.in_a_file(ships[0]) && control.in_a_file(ships[1]));

    // The other control: the second has the rail, the first none: the same, the group is on one.
    let (mut other, ships) = rig_of(&[0, 20], true);
    let second = ships[1];
    other.in_a_tick(move |world| world.set_grid_rail(second, Some(a_rail())).unwrap());
    other.player_at(-500.0);
    other.tick();
    other.tick();
    assert!(other.in_memory(ships[0]) && other.in_memory(ships[1]));
}

#[test]
fn the_ships_coupled_to_one_that_was_on_a_rail_when_it_was_put_in_a_file_are_brought_in_with_it_whatever_the_distance()
 {
    // A world that was saved and loaded has all its ships in files; the ones on rails are brought in at once, and the group they hold with them.
    let (mut rig, ships) = rig_of(&[0, 20], true);
    let (first, second) = (ships[0], ships[1]);
    rig.in_a_tick(move |world| {
        world.set_grid_rail(first, Some(a_rail())).unwrap();
        world.park_ship(first, false).unwrap();
        world.park_ship(second, false).unwrap();
    });
    // (The lifecycle looks at the ships in the tick that parks them, and brings them in at once.)
    assert!(rig.in_memory(ships[0]) && rig.in_memory(ships[1]));
    assert_eq!(rig.report().brought_in, 2);
    assert!(rig.report().problems.is_empty());
    // A player 2,000 voxels away changes nothing: they are in memory, frozen as ships no one is near, and are not put away.
    rig.player_at(-2000.0);
    rig.tick();
    assert!(rig.in_memory(ships[0]) && rig.in_memory(ships[1]));
    assert_eq!(rig.report().parked, 0);

    // The control: the same ships with no rail are left in their files.
    let (mut control, ships) = rig_of(&[0, 20], true);
    let (first, second) = (ships[0], ships[1]);
    control.in_a_tick(move |world| {
        world.park_ship(first, false).unwrap();
        world.park_ship(second, false).unwrap();
    });
    control.player_at(-2000.0);
    control.tick();
    assert!(control.in_a_file(ships[0]) && control.in_a_file(ships[1]));
    assert_eq!(control.report().brought_in, 0);
}
