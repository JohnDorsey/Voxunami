//! Ships by how near the players are, run by the plugin host on a real server with the pretend game of `world_storage::test_support`.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{Flaky, Rig, second};
use player_presence::PlayerPose;
use player_presence::lock;
use protocol::{ClientId, ClientMessage};
use server::ShipSettings;
use server::test_support::log_in;
use transport::Connection;
use voxel_space::{BrickBox, BrickCoordinate, LevelIndex, Orientation, WorldPosition};
use world_storage::test_support::id;
use world_storage::{GridTerrain, MemoryShipFiles};

fn settings() -> ShipSettings {
    ShipSettings::new(100.0, 300.0).unwrap()
}

#[test]
fn a_ship_is_frozen_when_no_player_is_within_the_simulation_distance_and_let_go_when_one_comes_to_it()
 {
    let (mut rig, ships) = Rig::new(settings(), &[0]);
    let ship = ships[0];
    // The box of the ship is from 0 to 64 along x. A player 50 voxels from its low face is near.
    rig.player_at(-50.0);
    assert!(!rig.frozen(ship));
    // 109 away is not within 100, but a ship that moves does not stop until a player is more than a tenth farther than that.
    rig.player_at(-109.0);
    assert!(!rig.frozen(ship));
    rig.player_at(-111.0);
    assert!(rig.frozen(ship));
    // Frozen, it does not move again until a player is within 100, not within a tenth beyond.
    rig.player_at(-105.0);
    assert!(rig.frozen(ship));
    rig.player_at(-100.5);
    assert!(rig.frozen(ship));
    rig.player_at(-99.0);
    assert!(!rig.frozen(ship));
    // From the other side the distance is to the high face (64).
    rig.player_at(64.0 + 99.0);
    assert!(!rig.frozen(ship));
    rig.player_at(64.0 + 111.0);
    assert!(rig.frozen(ship));
    assert_eq!(rig.report().frozen, 2);
    assert_eq!(rig.report().thawed, 1);
}

#[test]
fn a_ship_that_is_far_from_every_player_is_put_in_a_file_and_comes_back_when_one_is_near_the_place_it_was()
 {
    let (mut rig, ships) = Rig::new(settings(), &[0]);
    let ship = ships[0];
    let pose_before = rig.server.world().grids().get(ship).unwrap().pose;
    let digest = rig
        .server
        .world()
        .grids()
        .get(ship)
        .unwrap()
        .voxels
        .content_digest();

    // 400 away: it is frozen in one tick and put away in the next.
    rig.player_at(-400.0);
    assert!(rig.frozen(ship));
    rig.tick();
    assert!(!rig.in_memory(ship) && rig.in_a_file(ship));
    assert_eq!(rig.files.len(), 1);
    assert_eq!(rig.report().parked, 1);
    // The world knows where it is, so a player can be placed in it.
    assert_eq!(
        rig.server.world().grids().frame_of(ship),
        Some((LevelIndex::MAIN, pose_before))
    );

    // Between the distance it stops moving at and the one it is put away at, a ship in a file stays where it is.
    rig.player_at(-250.0);
    rig.player_at(-110.0);
    assert!(rig.in_a_file(ship));
    // Within the simulation distance of where it is, it comes back, awake, as it was.
    rig.player_at(-99.0);
    assert!(rig.in_memory(ship) && !rig.in_a_file(ship) && !rig.frozen(ship));
    let ship_now = rig.server.world().grids().get(ship).unwrap();
    assert_eq!(ship_now.pose, pose_before);
    assert_eq!(ship_now.voxels.content_digest(), digest);
    assert_eq!(rig.report().brought_in, 1);
    assert!(rig.report().problems.is_empty());
}

#[test]
fn a_ship_that_is_frozen_between_the_two_distances_is_not_put_away_and_one_a_tenth_beyond_the_unload_distance_is()
 {
    let (mut rig, ships) = Rig::new(settings(), &[0]);
    let ship = ships[0];
    rig.player_at(-320.0);
    rig.tick();
    rig.tick();
    // 320 is beyond 300 but not beyond 330.
    assert!(rig.frozen(ship) && rig.in_memory(ship));
    rig.player_at(-331.0);
    assert!(rig.in_a_file(ship));
}

#[test]
fn the_distance_to_a_ship_in_a_file_is_to_its_nearest_brick_from_whichever_side() {
    let (mut rig, ships) = Rig::new(settings(), &[0]);
    let ship = ships[0];
    rig.player_at(-400.0);
    rig.tick();
    assert!(rig.in_a_file(ship));
    // The box is from 0 to 64 along x. A player 90 voxels past its high face is near it, though 154 from the place its pose is.
    rig.player_at(64.0 + 90.0);
    assert!(
        rig.in_memory(ship),
        "the ship was brought in by a player on its far side"
    );
    // And one that is 120 past it is not near.
    rig.player_at(-400.0);
    rig.tick();
    assert!(rig.in_a_file(ship));
    rig.player_at(64.0 + 120.0);
    assert!(rig.in_a_file(ship));
}

#[test]
fn only_the_players_in_the_level_of_a_ship_count() {
    let (mut rig, ships) = Rig::new(settings(), &[0]);
    let ship = ships[0];
    rig.player_at(-10.0);
    assert!(!rig.frozen(ship));
    // The player goes to the other level, at the same place: that is another world, and the ship is far from everyone in its own.
    let presence = rig.presence.clone();
    let client: ClientId = rig.server.connected_clients().next().unwrap();
    rig.in_a_tick(move |world| lock(&presence).move_to_level(client, second(), world));
    assert!(rig.frozen(ship));
    rig.tick();
    assert!(rig.in_a_file(ship));
    // And back.
    let presence = rig.presence.clone();
    rig.in_a_tick(move |world| lock(&presence).move_to_level(client, LevelIndex::MAIN, world));
    rig.tick();
    assert!(rig.in_memory(ship) && !rig.frozen(ship));
}

#[test]
fn a_ship_that_went_away_with_whoever_was_aboard_does_not_come_back_for_a_player_who_comes_near() {
    let (mut rig, ships) = Rig::new(settings(), &[0]);
    let ship = ships[0];
    rig.player_at(-20.0);
    rig.in_a_tick(move |world| world.park_ship(ship, true).unwrap());
    rig.player_at(-20.0);
    rig.player_at(-1.0);
    assert!(rig.in_a_file(ship) && !rig.in_memory(ship));
    assert!(
        rig.server
            .world()
            .grids()
            .get_parked(ship)
            .unwrap()
            .waits_for_those_aboard
    );
}

#[test]
fn a_player_who_joins_aboard_a_ship_in_a_file_has_it_brought_in_before_anything_places_them_in_it()
{
    let (mut rig, ships) = Rig::new(settings(), &[0]);
    let ship = ships[0];
    // The player is aboard, then logs out, and the ship is put away.
    let presence = rig.presence.clone();
    let client: ClientId = rig.server.connected_clients().next().unwrap();
    rig.player_at(10.0);
    // (Aboard in the sense that their record says so: a long way along the ship's x from its bricks, so that the distance of the ship from them
    // is not what brings it in. It is the join that does.)
    let aboard_at = WorldPosition::new(BrickCoordinate::new(40, 0, 0), [3.0, 3.0, 3.0]);
    rig.in_a_tick(move |world| {
        lock(&presence)
            .set_pose_in(
                world,
                client,
                ship,
                PlayerPose {
                    position: aboard_at,
                    yaw: 0.0,
                    pitch: 0.0,
                },
                Orientation::IDENTITY,
            )
            .unwrap();
    });
    rig.connection
        .send(ClientMessage::Goodbye.encode())
        .unwrap();
    rig.tick();
    assert_eq!(rig.server.connected_clients().count(), 0);
    // Nobody is near, and the ship is frozen and put in a file.
    rig.tick();
    rig.tick();
    assert!(rig.in_a_file(ship));
    rig.tick();
    assert!(rig.in_a_file(ship));

    // The same player joins again: the ship is in memory in the tick they join, and they are aboard it.
    let mut connection = rig.connector.connect().unwrap();
    log_in(&mut rig.server, &mut connection, &id("test_game:game"), 1);
    assert!(rig.in_memory(ship));
    let client = rig.server.connected_clients().next().unwrap();
    let presence = lock(&rig.presence);
    assert_eq!(presence.frame_of(rig.server.world(), client), Some(ship));
    assert_eq!(
        presence
            .local_pose_of(rig.server.world(), client)
            .unwrap()
            .1
            .position,
        aboard_at
    );
    assert_eq!(rig.report().brought_in, 1);
}

#[test]
fn a_ship_that_cannot_be_put_away_stays_and_the_disk_is_not_asked_again_every_tick() {
    let flaky = Flaky {
        inner: MemoryShipFiles::new(),
        failing: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        writes: Arc::new(AtomicUsize::new(0)),
    };
    let (mut rig, ships) = Rig::with_files(
        settings(),
        &[0],
        MemoryShipFiles::new(),
        Some(flaky.clone()),
    );
    let ship = ships[0];
    rig.player_at(-400.0);
    rig.tick();
    // It could not be written: it is in memory, frozen, and the report says why.
    assert!(rig.in_memory(ship) && rig.frozen(ship));
    assert_eq!(flaky.writes.load(Ordering::SeqCst), 1);
    assert_eq!(rig.report().problems.len(), 1);
    assert!(
        rig.report().problems[0].contains("the disk is full"),
        "{:?}",
        rig.report().problems
    );
    // Not asked again soon.
    for _ in 0..150 {
        rig.tick();
    }
    assert_eq!(flaky.writes.load(Ordering::SeqCst), 1);
    // Asked again later, and this time the disk works.
    flaky.failing.store(false, Ordering::SeqCst);
    for _ in 0..60 {
        rig.tick();
    }
    assert!(rig.in_a_file(ship));
    assert_eq!(flaky.inner.len(), 1);
    assert_eq!(rig.report().parked, 1);
}

#[test]
fn with_ships_everywhere_nothing_is_frozen_or_put_away_and_every_ship_in_a_file_comes_in() {
    let (mut rig, ships) = Rig::new(ShipSettings::everywhere(), &[0, 100]);
    let (first, second_ship) = (ships[0], ships[1]);
    // The player is a thousand bricks away from both.
    rig.player_at(32_000.0);
    rig.player_at(32_000.0);
    assert!(!rig.frozen(first) && !rig.frozen(second_ship));
    // A ship that is in a file (put there by a save, say) is brought in by the tick that finds it, wherever the players are.
    rig.in_a_tick(move |world| world.park_ship(first, false).unwrap());
    assert!(rig.in_memory(first) && !rig.frozen(first));
    assert_eq!(rig.report().brought_in, 1);
    assert_eq!(
        rig.report().parked,
        0,
        "the lifecycle put none away: the test did"
    );
    assert_eq!(rig.report().frozen, 0);
    let _ = &rig.game;
}

#[test]
fn each_ship_does_what_its_own_distance_asks_whatever_the_others_are_doing() {
    // Three ships at different distances from one player: each does what its own distance asks, whatever the others are.
    let (mut rig, ships) = Rig::new(settings(), &[10, -20, 30]);
    // Boxes: 320..384, -640..-576, 960..1024 along x. A player at 400 is 16 from the first, 976 from the second and 560 from the third.
    rig.player_at(400.0);
    rig.tick();
    assert!(!rig.frozen(ships[0]));
    assert!(rig.in_a_file(ships[1]) || rig.frozen(ships[1]));
    assert!(rig.frozen(ships[2]) || rig.in_a_file(ships[2]));
    rig.tick();
    assert!(rig.in_a_file(ships[1]));
    assert!(rig.in_a_file(ships[2]));
    assert!(rig.in_memory(ships[0]));
}

#[test]
fn a_planet_is_frozen_when_no_player_is_near_but_is_never_put_away_so_that_it_can_be_drawn_from_anywhere()
 {
    // Two ships side by side, as the others are; the first is made a planet (its extent is the brick on each side of its origin), and the second is the control.
    let (mut rig, ships) = Rig::new(settings(), &[0, 10]);
    let (planet, ordinary) = (ships[0], ships[1]);
    rig.in_a_tick(move |world| {
        let generator = world.data().main().terrain.default_generator();
        world
            .set_grid_terrain(
                planet,
                GridTerrain {
                    generator,
                    extent: BrickBox::around(BrickCoordinate::ORIGIN, 1),
                    mass: 1.0e20,
                },
            )
            .unwrap();
    });
    // Far from both: both are frozen, and the ordinary one is put away.
    rig.player_at(-2000.0);
    rig.tick();
    rig.tick();
    assert!(rig.frozen(planet) && rig.in_memory(planet));
    assert!(rig.in_a_file(ordinary));
    // However long it goes on, and however far the player goes.
    rig.player_at(-50_000.0);
    for _ in 0..5 {
        rig.tick();
    }
    assert!(rig.in_memory(planet) && !rig.in_a_file(planet));
    assert_eq!(rig.report().parked, 1, "only the ordinary ship");
    // And a player who comes near wakes it as any ship.
    rig.player_at(-50.0);
    assert!(rig.in_memory(planet) && !rig.frozen(planet));
}

#[test]
fn a_planet_that_is_in_a_file_is_brought_back_whatever_the_distance_and_an_ordinary_ship_is_not() {
    let (mut rig, ships) = Rig::new(settings(), &[0, 10]);
    let (planet, ordinary) = (ships[0], ships[1]);
    rig.in_a_tick(move |world| {
        let generator = world.data().main().terrain.default_generator();
        world
            .set_grid_terrain(
                planet,
                GridTerrain {
                    generator,
                    extent: BrickBox::around(BrickCoordinate::ORIGIN, 1),
                    mass: 1.0e20,
                },
            )
            .unwrap();
    });
    // Far from both. The ordinary ship is put away by the lifecycle; the planet is put away by something else (a world that was saved and loaded has all its ships in
    // files, and a rule may put one away).
    rig.player_at(-5000.0);
    rig.tick();
    rig.tick();
    assert!(rig.in_a_file(ordinary));
    rig.in_a_tick(move |world| world.park_ship(planet, false).unwrap());
    // The look at the ships comes later in the same tick: it brings the planet back, and the report says so; the ordinary ship stays where it is.
    assert!(rig.in_memory(planet) && !rig.in_a_file(planet));
    assert!(rig.in_a_file(ordinary));
    assert_eq!(rig.report().brought_in, 1);
    assert!(rig.report().problems.is_empty());
}
