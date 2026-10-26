//! A player who logs in aboard a ship that is in a file: the ship is brought in, and if it went away with them and something is where it was
//! left they are asked what to do; if the file cannot be found they are asked that. Run by the plugin host on a real server with the pretend
//! game of `world_storage::test_support`, the way a game's rules would put a ship away with its pilot (`park_ship` with `true`).

mod common;

use common::{Rig, answer, hear, second, second_spawn, spawn};
use player_presence::{PlayerPose, lock};
use plugin_host::{Prompt, PromptClosed};
use protocol::{ClientId, ClientMessage};
use server::{GridView, ShipSettings};
use transport::Connection;
use transport::loopback::LoopbackConnection;
use voxel_space::{
    BrickBox, BrickCoordinate, GridId, GridPose, LevelIndex, Orientation, VoxelCoordinate,
    WorldPosition,
};
use world_storage::test_support::{Game, id};
use world_storage::{GridTerrain, PilotLogout, ShipFiles, WorldData};

/// Where the ship of these tests is when it goes away and where it comes back to: above the ground (which is solid below 10), with the
/// cells of its deck (one layer) at height 13.
fn landing() -> GridPose {
    GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [8.0, 12.0, 8.0]),
        Orientation::IDENTITY,
    )
}

/// Where a passenger stands in the ship, in its own coordinates: above the deck.
fn deck_place() -> WorldPosition {
    place_of_passenger(0)
}

/// Where the passenger numbered `number` (from 0) stands when there are several: a voxel along from the one before.
fn place_of_passenger(number: usize) -> WorldPosition {
    WorldPosition::new(BrickCoordinate::ORIGIN, [1.5 + number as f32, 2.0, 1.5])
}

/// The voxel of the world that the cell (1, 1, 1) of the deck is in when the ship is at `landing()`: the one stone that is put in its way.
const IN_THE_WAY: [i64; 3] = [9, 13, 9];

const QUESTION: &str = "Something is where your ship was left.";
const OPTIONS: [&str; 5] = [
    "Move the ship to the nearest free place",
    "Break the terrain that is in the way",
    "Break the blocks of the ship that are in the way",
    "Put the ship there anyway (it may be glitchy)",
    "Go to the world spawn and leave the ship behind",
];
const LOST: &str = "The ship you were aboard cannot be found.";
const LOST_OPTIONS: [&str; 3] = [
    "Go to the world spawn",
    "Appear in the air where you were",
    "Quit to the title",
];

const MOVE_THE_SHIP: u32 = 0;
const BREAK_THE_TERRAIN: u32 = 1;
const BREAK_THE_SHIP: u32 = 2;
const PUT_IT_THERE: u32 = 3;
const WARP_TO_THE_SPAWN: u32 = 4;
const GO_TO_THE_SPAWN: u32 = 0;
const APPEAR_IN_THE_AIR: u32 = 1;
const QUIT: u32 = 2;

/// A world with one ship (held, as `sample_ship` has it) at `landing()`, with stone at these voxels of the main level.
fn world_with_a_ship(stone: Vec<[i64; 3]>) -> impl FnOnce(&Game, &mut WorldData) -> Vec<GridId> {
    world_with_a_ship_at(landing(), stone)
}

/// The same, with the ship at this pose.
fn world_with_a_ship_at(
    pose: GridPose,
    stone: Vec<[i64; 3]>,
) -> impl FnOnce(&Game, &mut WorldData) -> Vec<GridId> {
    move |game, world| {
        for at in stone {
            game.set_block(world, at, "base_blocks:stone");
        }
        let mut ship = game.sample_ship(world, 0);
        ship.pose = pose;
        vec![world.grids_mut().create(ship)]
    }
}

/// A world with one ship in the second level, with stone at these voxels of that level (and none anywhere in the main one).
fn world_with_a_ship_in_the_second_level(
    stone: Vec<[i64; 3]>,
) -> impl FnOnce(&Game, &mut WorldData) -> Vec<GridId> {
    move |game, world| {
        let block = game.raw_block("base_blocks:stone");
        for at in stone {
            game.set_in_level(world, second(), at, "core:block", block);
        }
        let mut ship = game.sample_ship(world, 0);
        ship.pose = landing();
        ship.level = second();
        vec![world.grids_mut().create(ship)]
    }
}

/// The player (the only one logged in) is aboard the ship, at `deck_place()`.
fn put_aboard(rig: &mut Rig, ship: GridId) {
    let (presence, client) = (rig.presence.clone(), rig.client());
    rig.in_a_tick(move |world| {
        lock(&presence)
            .set_pose_in(
                world,
                client,
                ship,
                PlayerPose {
                    position: deck_place(),
                    yaw: 0.0,
                    pitch: 0.0,
                },
                Orientation::IDENTITY,
            )
            .unwrap();
    });
}

/// The player logs out aboard the ship, and the ship goes away with them: it is put in a file that waits for them.
fn go_away_aboard(rig: &mut Rig, ship: GridId) {
    put_aboard(rig, ship);
    rig.log_out();
    rig.in_a_tick(move |world| world.park_ship(ship, true).unwrap());
    assert!(rig.in_a_file(ship) && !rig.in_memory(ship));
    assert_eq!(rig.files.len(), 1);
}

fn waits(rig: &Rig, ship: GridId) -> bool {
    rig.server
        .world()
        .grids()
        .get_parked(ship)
        .expect("the ship is in a file")
        .waits_for_those_aboard
}

fn ship_pose(rig: &Rig, ship: GridId) -> GridPose {
    rig.server
        .world()
        .grids()
        .get(ship)
        .expect("the ship is in memory")
        .pose
}

/// The frame the only player is in, and where they are in it.
fn place_of_the_player(rig: &Rig) -> (GridId, WorldPosition) {
    let presence = lock(&rig.presence);
    let (frame, pose, _) = presence
        .local_pose_of(rig.server.world(), rig.client())
        .expect("the player has a place");
    (frame, pose.position)
}

/// Where the only player is in the world.
fn place_in_the_world(rig: &Rig) -> WorldPosition {
    lock(&rig.presence)
        .pose_of(rig.server.world(), rig.client())
        .expect("the player has a place")
        .position
}

/// The block in a voxel of the main level, as the world has it written.
fn block_at(rig: &Rig, at: [i64; 3]) -> u32 {
    block_in(rig, LevelIndex::MAIN, at)
}

/// The block in a voxel of a level, as the world has it written.
fn block_in(rig: &Rig, level: LevelIndex, at: [i64; 3]) -> u32 {
    let level = rig.server.world().level(level).unwrap();
    let channel = level
        .overrides
        .layout()
        .channel_index(&id("core:block"))
        .unwrap();
    level
        .overrides
        .get(channel, VoxelCoordinate::new(at[0], at[1], at[2]))
}

fn block_of_the_ship(rig: &Rig, ship: GridId, at: [i64; 3]) -> u32 {
    let ship = rig.server.world().grids().get(ship).unwrap();
    let channel = ship
        .voxels
        .layout()
        .channel_index(&id("core:block"))
        .unwrap();
    ship.voxels
        .get(channel, VoxelCoordinate::new(at[0], at[1], at[2]))
}

fn stone(rig: &Rig) -> u32 {
    rig.game.raw_block("base_blocks:stone")
}

fn air(rig: &Rig) -> u32 {
    rig.game.raw_block("base_blocks:air")
}

fn digest_of(rig: &Rig, ship: GridId) -> u64 {
    rig.server
        .world()
        .grids()
        .get(ship)
        .unwrap()
        .voxels
        .content_digest()
}

/// A rig whose ship is in the way of nothing, and one whose ship has stone where it will land.
fn free_rig() -> (Rig, GridId) {
    let (rig, ships) = Rig::for_arrivals(ShipSettings::everywhere(), world_with_a_ship(vec![]));
    (rig, ships[0])
}

fn blocked_rig() -> (Rig, GridId) {
    let (rig, ships) = Rig::for_arrivals(
        ShipSettings::everywhere(),
        world_with_a_ship(vec![IN_THE_WAY]),
    );
    (rig, ships[0])
}

/// The player has gone away with the ship and come back, and has been asked what to do: returns the id of the question.
fn asked_after_coming_back(rig: &mut Rig, ship: GridId) -> Prompt {
    go_away_aboard(rig, ship);
    rig.log_in_again(1);
    let mut questions = hear(&mut rig.connection).questions();
    assert_eq!(questions.len(), 1, "{questions:?}");
    questions.remove(0)
}

#[test]
fn a_ship_that_went_away_with_its_pilot_is_back_where_it_was_when_they_come_back_if_nothing_is_there()
 {
    let (mut rig, ship) = free_rig();
    let digest = digest_of(&rig, ship);
    go_away_aboard(&mut rig, ship);
    rig.log_in_again(1);
    // It came in in the tick they joined, asking nobody anything, and they are in it where they were.
    assert!(rig.in_memory(ship) && !rig.in_a_file(ship));
    assert_eq!(ship_pose(&rig, ship), landing());
    assert_eq!(digest_of(&rig, ship), digest);
    assert_eq!(hear(&mut rig.connection).questions(), []);
    assert_eq!(place_of_the_player(&rig), (ship, deck_place()));
    assert_eq!(rig.report().brought_in, 1);
    assert_eq!(rig.report().waiting, 0);
    assert!(rig.report().problems.is_empty());
}

#[test]
fn only_the_blocks_of_a_ship_are_looked_at_and_not_the_empty_space_of_its_bricks() {
    // The ground is solid up to height 10. The ship's lowest row of voxels is in the ground, but the deck is a row above it, on top of it:
    // nothing of the ship is in the ground, so nothing is in the way.
    let low = GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [8.0, 9.0, 8.0]),
        Orientation::IDENTITY,
    );
    let (mut rig, ships) = Rig::for_arrivals(
        ShipSettings::everywhere(),
        world_with_a_ship_at(low, vec![]),
    );
    let ship = ships[0];
    go_away_aboard(&mut rig, ship);
    rig.log_in_again(1);
    assert!(rig.in_memory(ship));
    assert_eq!(ship_pose(&rig, ship), low);
    assert_eq!(hear(&mut rig.connection).questions(), []);
    assert_eq!(rig.report().waiting, 0);
}

#[test]
fn when_something_is_where_the_ship_was_left_the_player_is_asked_and_the_ship_waits_in_its_file() {
    let (mut rig, ship) = blocked_rig();
    let question = asked_after_coming_back(&mut rig, ship);
    assert_eq!(question.title, QUESTION);
    assert_eq!(question.options, OPTIONS);
    // Nothing happens until they say: the ship is in its file, still waiting for them, however long it takes.
    for _ in 0..30 {
        rig.tick();
    }
    assert!(rig.in_a_file(ship) && !rig.in_memory(ship));
    assert!(waits(&rig, ship));
    assert_eq!(rig.report().waiting, 1);
    assert_eq!(rig.report().brought_in, 0);
    // They are still aboard it, where they were.
    assert_eq!(place_of_the_player(&rig), (ship, deck_place()));
    assert_eq!(block_at(&rig, IN_THE_WAY), stone(&rig));
}

#[test]
fn the_ship_can_be_moved_to_the_nearest_free_place_and_that_is_one_voxel_up() {
    let (mut rig, ship) = blocked_rig();
    let digest = digest_of(&rig, ship);
    let question = asked_after_coming_back(&mut rig, ship);
    answer(&mut rig.connection, question.id, MOVE_THE_SHIP);
    rig.tick();
    assert!(rig.in_memory(ship) && !rig.in_a_file(ship));
    // One voxel up is the nearest free place there is (the stone is only under its deck), and it is the same ship, turned the same.
    let moved = GridPose::new(
        landing().position.translated([0.0, 1.0, 0.0]),
        Orientation::IDENTITY,
    );
    assert_eq!(ship_pose(&rig, ship), moved);
    assert_eq!(digest_of(&rig, ship), digest);
    assert_eq!(block_at(&rig, IN_THE_WAY), stone(&rig));
    // The player is where they were in it, so a voxel higher in the world; the question is over.
    assert_eq!(place_of_the_player(&rig), (ship, deck_place()));
    assert_eq!(
        place_in_the_world(&rig),
        WorldPosition::new(BrickCoordinate::ORIGIN, [9.5, 15.0, 9.5])
    );
    assert_eq!(
        hear(&mut rig.connection).closed(),
        [PromptClosed(question.id)]
    );
    assert_eq!(rig.report().waiting, 0);
    assert_eq!(rig.report().brought_in, 1);
}

#[test]
fn the_terrain_in_the_way_can_be_broken_and_nothing_else_is() {
    let (mut rig, ship) = blocked_rig();
    let digest = digest_of(&rig, ship);
    let question = asked_after_coming_back(&mut rig, ship);
    answer(&mut rig.connection, question.id, BREAK_THE_TERRAIN);
    rig.tick();
    assert!(rig.in_memory(ship));
    assert_eq!(ship_pose(&rig, ship), landing());
    assert_eq!(digest_of(&rig, ship), digest, "the ship is whole");
    assert_eq!(block_at(&rig, IN_THE_WAY), air(&rig));
    assert_eq!(rig.report().waiting, 0);
}

#[test]
fn only_the_terrain_that_is_in_the_way_is_broken() {
    // Stone beside the one that is in the way, where no cell of the ship is.
    let beside = [20, 13, 20];
    let (mut rig, ships) = Rig::for_arrivals(
        ShipSettings::everywhere(),
        world_with_a_ship(vec![IN_THE_WAY, beside]),
    );
    let ship = ships[0];
    let question = asked_after_coming_back(&mut rig, ship);
    answer(&mut rig.connection, question.id, BREAK_THE_TERRAIN);
    rig.tick();
    assert_eq!(block_at(&rig, IN_THE_WAY), air(&rig));
    assert_eq!(block_at(&rig, beside), stone(&rig));
}

#[test]
fn the_blocks_of_the_ship_that_are_in_the_way_can_be_broken_and_nothing_else_is() {
    let (mut rig, ship) = blocked_rig();
    let question = asked_after_coming_back(&mut rig, ship);
    answer(&mut rig.connection, question.id, BREAK_THE_SHIP);
    rig.tick();
    assert!(rig.in_memory(ship));
    assert_eq!(ship_pose(&rig, ship), landing());
    // The cell of the deck that was in the stone is gone, and the rest of the deck is there.
    assert_eq!(block_of_the_ship(&rig, ship, [1, 1, 1]), air(&rig));
    for whole in [[0, 1, 0], [2, 1, 1], [3, 1, 2], [1, 1, 0], [1, 1, 2]] {
        assert_eq!(
            block_of_the_ship(&rig, ship, whole),
            stone(&rig),
            "{whole:?}"
        );
    }
    assert_eq!(
        block_of_the_ship(&rig, ship, [40, 33, 2]),
        rig.game.raw_block("fancy_blocks:marble")
    );
    assert_eq!(
        block_at(&rig, IN_THE_WAY),
        stone(&rig),
        "the terrain is whole"
    );
}

#[test]
fn the_ship_can_be_put_where_it_was_whatever_is_there() {
    let (mut rig, ship) = blocked_rig();
    let digest = digest_of(&rig, ship);
    let question = asked_after_coming_back(&mut rig, ship);
    answer(&mut rig.connection, question.id, PUT_IT_THERE);
    rig.tick();
    assert!(rig.in_memory(ship));
    assert_eq!(ship_pose(&rig, ship), landing());
    assert_eq!(digest_of(&rig, ship), digest);
    assert_eq!(block_at(&rig, IN_THE_WAY), stone(&rig));
    assert_eq!(place_of_the_player(&rig), (ship, deck_place()));
    assert_eq!(rig.report().waiting, 0);
}

#[test]
fn the_player_can_leave_the_ship_behind_and_go_to_the_spawn_and_the_ship_is_then_anyones_to_find() {
    let (mut rig, ships) = Rig::for_arrivals(
        ShipSettings::everywhere(),
        world_with_a_ship(vec![IN_THE_WAY]),
    );
    let ship = ships[0];
    go_away_aboard(&mut rig, ship);
    // Ships are now kept by how near the players are: the spawn is 40 voxels above the bricks of this one, which is not near.
    rig.server
        .set_ship_settings(ShipSettings::new(30.0, 600.0).unwrap());
    rig.log_in_again(1);
    let question = hear(&mut rig.connection).questions().remove(0);
    answer(&mut rig.connection, question.id, WARP_TO_THE_SPAWN);
    rig.tick();
    // They are at the spawn, in the world, and were told so.
    assert_eq!(
        place_of_the_player(&rig),
        (GridId::root(LevelIndex::MAIN), spawn())
    );
    let heard = hear(&mut rig.connection);
    assert_eq!(heard.placed(), [spawn()]);
    assert_eq!(heard.closed(), [PromptClosed(question.id)]);
    // The ship is where it was, in its file, and waits for nobody.
    for _ in 0..10 {
        rig.tick();
    }
    assert!(rig.in_a_file(ship) && !rig.in_memory(ship));
    assert!(!waits(&rig, ship));
    assert_eq!(rig.report().waiting, 0);
    assert_eq!(rig.report().brought_in, 0);
    // A player who comes near it finds it.
    rig.player_at(10.0);
    assert!(rig.in_memory(ship));
}

/// A world with the ship at `landing()` in a block of stone: the box from (-30, -25, -30) to (50, `top`, 50). The ship is in it, and so
/// is every place it could be moved to within 32 voxels, except the ones that are higher than `top` (the lowest voxel of its deck is
/// at height 13, so one that is moved up by `n` has its deck at 13 + n).
fn world_with_a_ship_in_stone_up_to(top: i64) -> impl FnOnce(&Game, &mut WorldData) -> Vec<GridId> {
    move |game, world| {
        let stone = game.raw_block("base_blocks:stone");
        let level = world.level_mut(LevelIndex::MAIN).unwrap();
        let channel = level
            .overrides
            .layout()
            .channel_index(&id("core:block"))
            .unwrap();
        for x in -30..=50 {
            for y in -25..=top {
                for z in -30..=50 {
                    level
                        .overrides
                        .set(channel, VoxelCoordinate::new(x, y, z), stone);
                }
            }
        }
        let mut ship = game.sample_ship(world, 0);
        ship.pose = landing();
        vec![world.grids_mut().create(ship)]
    }
}

#[test]
fn the_ship_is_moved_as_far_as_32_voxels_to_find_a_place_for_it() {
    // The only place within reach is straight up by 32.
    let (mut rig, ships) = Rig::for_arrivals(
        ShipSettings::everywhere(),
        world_with_a_ship_in_stone_up_to(44),
    );
    let ship = ships[0];
    let question = asked_after_coming_back(&mut rig, ship);
    answer(&mut rig.connection, question.id, MOVE_THE_SHIP);
    rig.tick();
    assert!(rig.in_memory(ship));
    assert_eq!(
        ship_pose(&rig, ship).position,
        landing().position.translated([0.0, 32.0, 0.0])
    );
    assert_eq!(rig.report().waiting, 0);
}

#[test]
fn when_there_is_no_free_place_within_reach_the_player_is_told_and_asked_again() {
    // The only place that is free is straight up by 33, which is out of reach.
    let (mut rig, ships) = Rig::for_arrivals(
        ShipSettings::everywhere(),
        world_with_a_ship_in_stone_up_to(45),
    );
    let ship = ships[0];
    let first = asked_after_coming_back(&mut rig, ship);
    answer(&mut rig.connection, first.id, MOVE_THE_SHIP);
    rig.tick();
    // It stays in its file, and they are told why, and asked again, with what was found said.
    assert!(rig.in_a_file(ship) && !rig.in_memory(ship));
    let heard = hear(&mut rig.connection);
    assert_eq!(
        heard.replies(),
        ["there is no free place for the ship within 32 voxels"]
    );
    assert_eq!(heard.closed(), [PromptClosed(first.id)]);
    let questions = heard.questions();
    assert_eq!(questions.len(), 1);
    assert_eq!(
        questions[0].title,
        "Something is where your ship was left. There is no free place for it within 32 voxels."
    );
    assert_eq!(questions[0].options, OPTIONS);
    assert_ne!(questions[0].id, first.id);
    assert_eq!(rig.report().waiting, 1);
    // And they can still say what to do: the ship is put there anyway.
    answer(&mut rig.connection, questions[0].id, PUT_IT_THERE);
    rig.tick();
    assert!(rig.in_memory(ship));
    assert_eq!(ship_pose(&rig, ship), landing());
    assert_eq!(rig.report().waiting, 0);
}

/// The file of every ship is lost.
fn lose_the_files(rig: &Rig) {
    let mut files = rig.files.clone();
    for name in files.names().unwrap() {
        files.remove(&name).unwrap();
    }
    assert_eq!(rig.files.len(), 0);
}

#[test]
fn a_ship_whose_file_is_lost_asks_the_player_where_to_go_and_the_ship_stays_in_the_world_file() {
    let (mut rig, ship) = free_rig();
    go_away_aboard(&mut rig, ship);
    lose_the_files(&rig);
    rig.log_in_again(1);
    let question = hear(&mut rig.connection).questions().remove(0);
    assert_eq!(question.title, LOST);
    assert_eq!(question.options, LOST_OPTIONS);
    assert_eq!(rig.report().waiting, 1);
    assert_eq!(rig.report().problems.len(), 1);
    assert!(
        rig.report().problems[0].contains("could not be brought in"),
        "{:?}",
        rig.report().problems
    );
    // (The world still knows the ship: if the file is found again, it can be brought in.)
    assert!(rig.in_a_file(ship));
    // They are still aboard it, however it is that they do not see it, until they answer.
    assert_eq!(place_of_the_player(&rig).0, ship);
}

#[test]
fn a_player_whose_ship_is_lost_can_go_to_the_world_spawn() {
    let (mut rig, ship) = free_rig();
    go_away_aboard(&mut rig, ship);
    lose_the_files(&rig);
    rig.log_in_again(1);
    let question = hear(&mut rig.connection).questions().remove(0);
    answer(&mut rig.connection, question.id, GO_TO_THE_SPAWN);
    rig.tick();
    assert_eq!(
        place_of_the_player(&rig),
        (GridId::root(LevelIndex::MAIN), spawn())
    );
    let heard = hear(&mut rig.connection);
    assert_eq!(heard.placed(), [spawn()]);
    assert_eq!(heard.closed(), [PromptClosed(question.id)]);
    assert_eq!(rig.report().waiting, 0);
}

#[test]
fn a_player_whose_ship_is_lost_can_appear_in_the_air_where_they_were_in_the_world() {
    let (mut rig, ship) = free_rig();
    go_away_aboard(&mut rig, ship);
    lose_the_files(&rig);
    rig.log_in_again(1);
    let question = hear(&mut rig.connection).questions().remove(0);
    // Where they were: the ship was at its pose and they were at the deck in it.
    let there = WorldPosition::new(BrickCoordinate::ORIGIN, [9.5, 14.0, 9.5]);
    assert_eq!(place_in_the_world(&rig), there);
    answer(&mut rig.connection, question.id, APPEAR_IN_THE_AIR);
    rig.tick();
    assert_eq!(
        place_of_the_player(&rig),
        (GridId::root(LevelIndex::MAIN), there)
    );
    assert_eq!(hear(&mut rig.connection).placed(), [there]);
}

#[test]
fn a_player_whose_ship_is_lost_can_quit_and_is_disconnected_with_the_reason() {
    let (mut rig, ship) = free_rig();
    go_away_aboard(&mut rig, ship);
    lose_the_files(&rig);
    rig.log_in_again(1);
    let question = hear(&mut rig.connection).questions().remove(0);
    answer(&mut rig.connection, question.id, QUIT);
    rig.tick();
    assert_eq!(
        hear(&mut rig.connection).disconnected().as_deref(),
        Some("you chose to quit")
    );
    rig.tick();
    assert_eq!(rig.server.connected_clients().count(), 0);
    assert_eq!(rig.report().waiting, 0);
}

#[test]
fn a_ship_that_was_put_away_by_distance_and_whose_file_is_lost_asks_too() {
    // Put away because nobody was near, not because it went away with them: it waits for nobody.
    let (mut rig, ships) = Rig::for_arrivals(
        ShipSettings::new(30.0, 100.0).unwrap(),
        world_with_a_ship(vec![]),
    );
    let ship = ships[0];
    put_aboard(&mut rig, ship);
    rig.log_out();
    for _ in 0..3 {
        rig.tick();
    }
    assert!(rig.in_a_file(ship) && !waits(&rig, ship));
    lose_the_files(&rig);
    rig.log_in_again(1);
    let questions = hear(&mut rig.connection).questions();
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0].title, LOST);
}

#[test]
fn a_ship_that_was_put_away_by_distance_is_brought_in_for_a_player_who_logs_in_aboard_without_asking_what_is_in_the_way()
 {
    // The same stone, but this ship did not go away with them: it was only far from everyone, and is where it was, as it would be if they
    // had never gone.
    let (mut rig, ships) = Rig::for_arrivals(
        ShipSettings::new(30.0, 100.0).unwrap(),
        world_with_a_ship(vec![IN_THE_WAY]),
    );
    let ship = ships[0];
    put_aboard(&mut rig, ship);
    rig.log_out();
    for _ in 0..3 {
        rig.tick();
    }
    assert!(rig.in_a_file(ship) && !waits(&rig, ship));
    rig.log_in_again(1);
    assert!(rig.in_memory(ship));
    assert_eq!(hear(&mut rig.connection).questions(), []);
    assert_eq!(rig.report().waiting, 0);
}

#[test]
fn without_a_look_at_where_ships_land_a_ship_that_went_away_comes_in_where_it_was_whatever_is_there()
 {
    let (mut rig, ships) = Rig::without_arrivals(
        ShipSettings::everywhere(),
        world_with_a_ship(vec![IN_THE_WAY]),
    );
    let ship = ships[0];
    go_away_aboard(&mut rig, ship);
    rig.log_in_again(1);
    assert!(rig.in_memory(ship));
    assert_eq!(ship_pose(&rig, ship), landing());
    assert_eq!(hear(&mut rig.connection).questions(), []);
    assert_eq!(rig.report().brought_in, 1);
}

/// Players 1, 2, ... (the install numbered so) are aboard the ships in turn, at their own places in them, and all go away: the ships are put
/// in their files. They return and are asked. Returns the question each was asked, and the connections of all but the first (his is the rig's).
fn go_away_together(rig: &mut Rig, ships: &[GridId]) -> (Vec<Prompt>, Vec<LoopbackConnection>) {
    let mut others: Vec<LoopbackConnection> = (2..=ships.len() as u8)
        .map(|number| rig.log_in_as(number))
        .collect();
    let mut clients: Vec<ClientId> = rig.server.connected_clients().collect();
    clients.sort();
    let (presence, aboard) = (rig.presence.clone(), ships.to_vec());
    rig.in_a_tick(move |world| {
        for (number, (client, ship)) in clients.iter().zip(aboard).enumerate() {
            lock(&presence)
                .set_pose_in(
                    world,
                    *client,
                    ship,
                    PlayerPose {
                        position: place_of_passenger(number),
                        yaw: 0.0,
                        pitch: 0.0,
                    },
                    Orientation::IDENTITY,
                )
                .unwrap();
        }
    });
    rig.connection
        .send(ClientMessage::Goodbye.encode())
        .unwrap();
    for other in &mut others {
        other.send(ClientMessage::Goodbye.encode()).unwrap();
    }
    rig.tick();
    assert_eq!(rig.server.connected_clients().count(), 0);
    let parked = ships.to_vec();
    rig.in_a_tick(move |world| {
        for ship in parked {
            // (Two players on one ship put it away once.)
            let _ = world.park_ship(ship, true);
        }
    });
    for ship in ships {
        assert!(rig.in_a_file(*ship) && !rig.in_memory(*ship));
    }
    // They come back in turn, and each is asked.
    rig.log_in_again(1);
    let mut others: Vec<LoopbackConnection> = (2..=ships.len() as u8)
        .map(|number| rig.log_in_as(number))
        .collect();
    let mut questions = vec![hear(&mut rig.connection).questions().remove(0)];
    for other in &mut others {
        questions.push(hear(other).questions().remove(0));
    }
    (questions, others)
}

#[test]
fn when_one_of_two_players_aboard_answers_the_other_is_not_asked_any_more() {
    let (mut rig, ship) = blocked_rig();
    let (questions, mut others) = go_away_together(&mut rig, &[ship, ship]);
    assert_ne!(questions[0].id, questions[1].id);
    assert_eq!(rig.report().waiting, 2);
    answer(&mut rig.connection, questions[0].id, PUT_IT_THERE);
    rig.tick();
    assert!(rig.in_memory(ship));
    // The other one's question is withdrawn, and nobody is waited for.
    assert_eq!(
        hear(&mut others[0]).closed(),
        [PromptClosed(questions[1].id)]
    );
    assert_eq!(rig.report().waiting, 0);
    // Both are aboard it where they were.
    let presence = lock(&rig.presence);
    let mut clients: Vec<ClientId> = rig.server.connected_clients().collect();
    clients.sort();
    for (number, client) in clients.iter().enumerate() {
        let (frame, pose, _) = presence.local_pose_of(rig.server.world(), *client).unwrap();
        assert_eq!(frame, ship);
        assert_eq!(pose.position, place_of_passenger(number));
    }
}

#[test]
fn an_answer_about_one_ship_withdraws_the_questions_about_that_ship_and_no_other() {
    // Two ships, a hundred voxels apart, each with something in the way of its own, and a player aboard each.
    const APART: i64 = 100;
    let (mut rig, ships) = Rig::for_arrivals(ShipSettings::everywhere(), |game, world| {
        game.set_block(world, IN_THE_WAY, "base_blocks:stone");
        game.set_block(
            world,
            [IN_THE_WAY[0] + APART, IN_THE_WAY[1], IN_THE_WAY[2]],
            "base_blocks:stone",
        );
        let mut first = game.sample_ship(world, 0);
        first.pose = landing();
        let mut apart = game.sample_ship(world, 0);
        apart.pose = GridPose::new(
            landing().position.translated([APART as f32, 0.0, 0.0]),
            Orientation::IDENTITY,
        );
        vec![
            world.grids_mut().create(first),
            world.grids_mut().create(apart),
        ]
    });
    let (questions, mut others) = go_away_together(&mut rig, &ships);
    assert_eq!(rig.report().waiting, 2);
    answer(&mut rig.connection, questions[0].id, PUT_IT_THERE);
    rig.tick();
    assert!(rig.in_memory(ships[0]) && rig.in_a_file(ships[1]));
    // The other player is still asked about the other ship.
    assert_eq!(hear(&mut others[0]).closed(), []);
    assert_eq!(rig.report().waiting, 1);
    answer(&mut others[0], questions[1].id, PUT_IT_THERE);
    rig.tick();
    assert!(rig.in_memory(ships[1]));
    assert_eq!(
        hear(&mut others[0]).closed(),
        [PromptClosed(questions[1].id)]
    );
    assert_eq!(rig.report().waiting, 0);
}

#[test]
fn an_answer_to_a_question_of_another_plugin_is_not_an_answer_about_the_ship() {
    let (mut rig, ship) = blocked_rig();
    let about_the_ship = asked_after_coming_back(&mut rig, ship);
    // Another plugin asks the player something while the ship waits for what they say.
    rig.say("/ask");
    let other = hear(&mut rig.connection).questions();
    assert_eq!(other.len(), 1);
    assert_eq!(other[0].title, "Another question?");
    assert_ne!(other[0].id, about_the_ship.id);
    // They answer that one. It means nothing to the ship, which goes on waiting.
    answer(&mut rig.connection, other[0].id, 1);
    rig.tick();
    assert!(rig.in_a_file(ship) && !rig.in_memory(ship));
    assert_eq!(rig.report().waiting, 1);
    let heard = rig.answers.lock().unwrap().clone();
    assert_eq!(heard.len(), 1);
    assert_eq!((heard[0].prompt, heard[0].choice), (other[0].id, 1));
    // And what they say about the ship still counts.
    answer(&mut rig.connection, about_the_ship.id, PUT_IT_THERE);
    rig.tick();
    assert!(rig.in_memory(ship));
    assert_eq!(rig.report().waiting, 0);
}

#[test]
fn what_is_in_the_way_is_what_is_in_the_level_of_the_ship() {
    // Stone where the ship will land in the second level, and none in the main one.
    let (mut rig, ships) = Rig::for_arrivals(
        ShipSettings::everywhere(),
        world_with_a_ship_in_the_second_level(vec![IN_THE_WAY]),
    );
    let ship = ships[0];
    let question = asked_after_coming_back(&mut rig, ship);
    assert_eq!(question.title, QUESTION);
    answer(&mut rig.connection, question.id, BREAK_THE_TERRAIN);
    rig.tick();
    assert!(rig.in_memory(ship));
    assert_eq!(ship_pose(&rig, ship), landing());
    // It was the stone of that level that was in the way, and that is broken.
    assert_eq!(block_in(&rig, second(), IN_THE_WAY), air(&rig));
}

#[test]
fn a_player_whose_ship_was_in_another_level_goes_to_the_spawn_of_that_level() {
    let (mut rig, ships) = Rig::for_arrivals(
        ShipSettings::everywhere(),
        world_with_a_ship_in_the_second_level(vec![]),
    );
    let ship = ships[0];
    go_away_aboard(&mut rig, ship);
    lose_the_files(&rig);
    rig.log_in_again(1);
    let question = hear(&mut rig.connection).questions().remove(0);
    answer(&mut rig.connection, question.id, GO_TO_THE_SPAWN);
    rig.tick();
    assert_eq!(
        place_of_the_player(&rig),
        (GridId::root(second()), second_spawn())
    );
}

#[test]
fn a_player_who_leaves_a_blocked_ship_behind_in_another_level_goes_to_the_spawn_of_that_level() {
    // The second level has stone where the ship lands, and a spawn of its own.
    let (mut rig, ships) = Rig::for_arrivals(
        ShipSettings::everywhere(),
        world_with_a_ship_in_the_second_level(vec![IN_THE_WAY]),
    );
    let ship = ships[0];
    let question = asked_after_coming_back(&mut rig, ship);
    answer(&mut rig.connection, question.id, WARP_TO_THE_SPAWN);
    rig.tick();
    assert_eq!(
        place_of_the_player(&rig),
        (GridId::root(second()), second_spawn())
    );
}

#[test]
fn a_player_who_leaves_without_answering_is_not_waited_for_and_is_asked_again_when_they_return() {
    let (mut rig, ship) = blocked_rig();
    let first = asked_after_coming_back(&mut rig, ship);
    assert_eq!(rig.report().waiting, 1);
    rig.log_out();
    assert_eq!(rig.report().waiting, 0);
    assert!(rig.in_a_file(ship) && waits(&rig, ship));
    // They return and are asked again, once, with a question of its own.
    rig.log_in_again(1);
    let questions = hear(&mut rig.connection).questions();
    assert_eq!(questions.len(), 1);
    assert_ne!(questions[0].id, first.id);
    assert_eq!(rig.report().waiting, 1);
    answer(&mut rig.connection, questions[0].id, PUT_IT_THERE);
    rig.tick();
    assert!(rig.in_memory(ship));
    assert_eq!(rig.report().waiting, 0);
}

/// A world with the ship at `landing()` and a planet in the main level: a ship with terrain, held, made by the level's own generator (solid below height 10 in the
/// planet's own coordinates), with its origin `planet_height` up. Nothing is stored in the planet and nothing is in the level but its ground.
fn world_with_a_ship_and_a_planet(
    planet_height: f32,
) -> impl FnOnce(&Game, &mut WorldData) -> Vec<GridId> {
    move |game, world| {
        let mut ship = game.sample_ship(world, 0);
        ship.pose = landing();
        let ship = world.grids_mut().create(ship);
        let mut planet = game.sample_ship(world, 100);
        planet.voxels =
            voxel_grid::Grid::new(std::sync::Arc::clone(world.main().overrides.layout()));
        planet.pose = GridPose::new(
            WorldPosition::new(BrickCoordinate::ORIGIN, [0.0, planet_height, 0.0]),
            Orientation::IDENTITY,
        );
        planet.linear_velocity = [0.0; 3];
        planet.angular_velocity = [0.0; 3];
        planet.sections = Vec::new();
        planet.on_pilot_logout = PilotLogout::Stay;
        planet.terrain = Some(GridTerrain {
            generator: world.main().terrain.default_generator(),
            extent: BrickBox::around(BrickCoordinate::ORIGIN, 1),
            mass: 1.0e20,
        });
        vec![ship, world.grids_mut().create(planet)]
    }
}

/// The blocks in these voxels of the planet, as the planet has them: what was stored over its ground, else its ground. (Asked in a tick of the server, which is one
/// more tick of the world.)
fn blocks_of_the_planet(rig: &mut Rig, planet: GridId, at: &[[i64; 3]]) -> Vec<u32> {
    let channel = rig
        .server
        .world()
        .main()
        .overrides
        .layout()
        .channel_index(&id("core:block"))
        .unwrap();
    let found = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (slot, at) = (std::sync::Arc::clone(&found), at.to_vec());
    rig.in_a_tick(move |world| {
        let mut view = world.grid(planet).unwrap();
        *slot.lock().unwrap() = at
            .iter()
            .map(|at| {
                view.get(channel, VoxelCoordinate::new(at[0], at[1], at[2]))
                    .unwrap()
            })
            .collect();
    });
    std::mem::take(&mut *found.lock().unwrap())
}

#[test]
fn the_ground_of_a_planet_that_nobody_has_touched_is_where_a_ship_cannot_land_as_any_ground_is() {
    // The planet's ground is solid below 10 in its own coordinates, and its origin is at 10, so its ground reaches 20 in the world: the deck of the ship, at 13, is in
    // it. (The level's own ground reaches 10, and the deck is above it.)
    let (mut rig, ships) = Rig::for_arrivals(
        ShipSettings::everywhere(),
        world_with_a_ship_and_a_planet(10.0),
    );
    let ship = ships[0];
    let question = asked_after_coming_back(&mut rig, ship);
    assert_eq!(question.title, QUESTION);
    for _ in 0..30 {
        rig.tick();
    }
    assert!(rig.in_a_file(ship) && !rig.in_memory(ship));
    assert_eq!(rig.report().waiting, 1);
    assert_eq!(rig.report().brought_in, 0);
}

#[test]
fn a_ship_over_the_ground_of_a_planet_or_beside_it_comes_in_without_a_question() {
    // The planet is lower, so that its ground reaches 12: the deck at 13 rests on it.
    let (mut rig, ships) = Rig::for_arrivals(
        ShipSettings::everywhere(),
        world_with_a_ship_and_a_planet(2.0),
    );
    let ship = ships[0];
    go_away_aboard(&mut rig, ship);
    rig.log_in_again(1);
    assert!(rig.in_memory(ship));
    assert_eq!(ship_pose(&rig, ship), landing());
    assert_eq!(hear(&mut rig.connection).questions(), []);
    assert_eq!(rig.report().waiting, 0);
}

#[test]
fn a_ship_that_would_land_in_the_ground_of_a_planet_can_be_moved_to_the_nearest_free_place_above_it()
 {
    let (mut rig, ships) = Rig::for_arrivals(
        ShipSettings::everywhere(),
        world_with_a_ship_and_a_planet(10.0),
    );
    let ship = ships[0];
    let question = asked_after_coming_back(&mut rig, ship);
    answer(&mut rig.connection, question.id, MOVE_THE_SHIP);
    rig.tick();
    assert!(rig.in_memory(ship));
    // The deck's centres are 13.5 over the ground of the level and the planet's ground reaches 20: seven up is the first place where they are above it.
    assert_eq!(
        ship_pose(&rig, ship).position,
        landing().position.translated([0.0, 7.0, 0.0])
    );
}

#[test]
fn the_ground_of_a_planet_that_is_in_the_way_can_be_broken_and_only_that() {
    let (mut rig, ships) = Rig::for_arrivals(
        ShipSettings::everywhere(),
        world_with_a_ship_and_a_planet(10.0),
    );
    let (ship, planet) = (ships[0], ships[1]);
    let question = asked_after_coming_back(&mut rig, ship);
    let stone = stone(&rig);
    // The deck's cell (1, 1, 1) is in the voxel (9, 13, 9) of the world, which is (9, 3, 9) in the planet; the voxel beside it is no cell of the ship's.
    let in_the_way = [9, 3, 9];
    let beside = [20, 3, 20];
    assert_eq!(
        blocks_of_the_planet(&mut rig, planet, &[in_the_way]),
        [stone]
    );
    answer(&mut rig.connection, question.id, BREAK_THE_TERRAIN);
    rig.tick();
    assert!(rig.in_memory(ship));
    assert_eq!(ship_pose(&rig, ship), landing());
    let air = air(&rig);
    let cells = [[8, 3, 8], [9, 3, 9], [10, 3, 8], [11, 3, 10]];
    assert_eq!(blocks_of_the_planet(&mut rig, planet, &cells), [air; 4]);
    assert_eq!(blocks_of_the_planet(&mut rig, planet, &[beside]), [stone]);
    // The ship is whole, and nothing was dug out of the ground of the level (nothing is stored of it).
    assert_eq!(block_of_the_ship(&rig, ship, [1, 1, 1]), stone);
    assert_eq!(rig.server.world().main().overrides.brick_count(), 0);
}
