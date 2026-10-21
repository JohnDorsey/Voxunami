//! Ships that go away with their pilot and are there when the pilot comes back, in Cave: `/ship vanish`, what the host of a server says
//! about it, and what a pilot meets when they return and something is where their ship was left or the ship cannot be found.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use common::{
    FLOOR, FlakyFiles, Player, Scene, block_at, eyes_in_their_frame, flat_floor, make_ship,
    player_pose, pose_at, put_stone, say, scene_with_a_deck, ship_id, ship_pose, tick_and_follow,
    walker_on_the_deck,
};
use server::{LogoutPolicy, ShipSettings};
use voxel_space::{GridId, GridPose, LevelIndex};
use world_storage::{MovingGrid, ParkedGrid, ShipFiles};

/// Where a returning pilot says they start: nowhere near the ship, so that what puts them back aboard it is the world's memory of them.
const ELSEWHERE: [f64; 3] = [200.0, FLOOR as f64 + 30.0, 200.0];

const PILOT_REPLY: &str = "you pilot ship 0: W A S D fly it the way you look, space and shift go up and down, control goes faster; /ship pilot again lets go";
const VANISH_ON: &str =
    "ship 0 will go away with its pilot when they log out, and be where it was when they come back";
const VANISH_OFF: &str = "ship 0 will stay where it is when its pilot logs out";

fn parked(scene: &Scene, number: u64) -> Option<&ParkedGrid> {
    scene.rig.server.world().grids().get_parked(ship_id(number))
}

fn in_memory(scene: &Scene, number: u64) -> bool {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(number))
        .is_some()
}

/// The ship as its file has it (this takes a tick).
fn ship_in_its_file(scene: &mut Scene, number: u64) -> MovingGrid {
    let found = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&found);
    scene.run(move |world| {
        *slot.lock().unwrap() = Some(world.read_parked_ship(ship_id(number)).unwrap());
    });
    found
        .lock()
        .unwrap()
        .take()
        .expect("the tick ran the action")
}

/// A deck lying on the floor, not held, with a walker who has fallen onto it (the first player).
fn deck_on_the_floor() -> (Scene, Player) {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    make_ship(
        &mut scene,
        pose_at(5.0, FLOOR, 20.0),
        false,
        [30, 2, 8],
        |_| true,
    );
    let walker = walker_on_the_floor_deck(&mut scene, 20.0);
    (scene, walker)
}

/// A walker who falls onto the deck of `deck_on_the_floor` at this x (and z 24) and stands there.
fn walker_on_the_floor_deck(scene: &mut Scene, x: f64) -> Player {
    let mut walker = scene
        .rig
        .join_walking([x, f64::from(FLOOR) + 2.0 + 8.0, 24.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut walker]);
    tick_and_follow(scene, &mut walker, 60);
    walker
}

/// The pilot of a held deck, who has taken the controls and said what the ship does when they leave.
fn pilot_on_the_deck(vanish: Option<bool>) -> (Scene, Player) {
    let mut scene = scene_with_a_deck();
    let mut pilot = walker_on_the_deck(&mut scene);
    assert_eq!(say(&mut scene, &mut pilot, "/ship pilot"), PILOT_REPLY);
    match vanish {
        Some(true) => assert_eq!(say(&mut scene, &mut pilot, "/ship vanish on"), VANISH_ON),
        Some(false) => assert_eq!(say(&mut scene, &mut pilot, "/ship vanish off"), VANISH_OFF),
        None => {}
    }
    (scene, pilot)
}

/// The pilot logs in again (the same player, who says they start somewhere else), and the server has done what it does when they join.
fn pilot_returns(scene: &mut Scene) -> Player {
    let mut back = scene.rig.join_walking_as(1, ELSEWHERE, 0.0, 0.0);
    scene.rig.settle(&mut [&mut back]);
    tick_and_follow(scene, &mut back, 3);
    back
}

#[test]
fn a_ship_stays_in_the_world_when_its_pilot_logs_out_unless_it_was_told_to_go_with_them() {
    let (mut scene, pilot) = pilot_on_the_deck(None);
    drop(pilot);
    scene.ticks(5);
    assert!(in_memory(&scene, 0));
    assert!(parked(&scene, 0).is_none());
    assert_eq!(scene.rig.files.len(), 0);
}

#[test]
fn a_ship_told_to_vanish_is_put_in_its_file_with_its_pilot_and_is_where_it_was_when_they_come_back()
{
    let (mut scene, pilot) = pilot_on_the_deck(Some(true));
    let pose = ship_pose(&scene, 0);
    let place_in_the_ship = eyes_in_their_frame(&scene, 1);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
    drop(pilot);
    scene.ticks(3);
    // It is in a file, waiting for them, and does not come back for anyone else: the settings here put ships to sleep nowhere.
    assert!(!in_memory(&scene, 0));
    assert!(parked(&scene, 0).unwrap().waits_for_those_aboard);
    assert_eq!(scene.rig.files.len(), 1);
    scene.ticks(60);
    assert!(!in_memory(&scene, 0));

    let mut back = pilot_returns(&mut scene);
    // The pilot is the second connection. The ship is there, where it was, and so are they in it.
    assert!(in_memory(&scene, 0));
    assert_eq!(ship_pose(&scene, 0), pose);
    assert_eq!(player_pose(&scene, 2).grid, ship_id(0));
    assert_eq!(eyes_in_their_frame(&scene, 2), place_in_the_ship);
    assert!(
        !back.hud_text().contains("(.choose N)"),
        "{}",
        back.hud_text()
    );
}

#[test]
fn a_ship_told_to_stay_after_it_was_told_to_vanish_stays() {
    let (mut scene, mut pilot) = pilot_on_the_deck(Some(true));
    assert_eq!(say(&mut scene, &mut pilot, "/ship vanish off"), VANISH_OFF);
    drop(pilot);
    scene.ticks(5);
    assert!(in_memory(&scene, 0));
    assert_eq!(scene.rig.files.len(), 0);
}

#[test]
fn what_the_host_of_the_server_says_beats_what_the_ship_says() {
    // Ships go away with their pilots whatever they say.
    let (mut scene, pilot) = pilot_on_the_deck(Some(false));
    scene.rig.server.set_ship_settings(
        ShipSettings::everywhere().with_logout_policy(LogoutPolicy::AlwaysVanish),
    );
    drop(pilot);
    scene.ticks(3);
    assert!(!in_memory(&scene, 0) && parked(&scene, 0).is_some());

    // And stay, whatever they say.
    let (mut scene, pilot) = pilot_on_the_deck(Some(true));
    scene
        .rig
        .server
        .set_ship_settings(ShipSettings::everywhere().with_logout_policy(LogoutPolicy::AlwaysStay));
    drop(pilot);
    scene.ticks(3);
    assert!(in_memory(&scene, 0) && parked(&scene, 0).is_none());
}

#[test]
fn a_ship_that_is_not_piloted_does_not_go_with_a_player_who_leaves_it() {
    // The ship wants to vanish with its pilot, but this player is only standing on it.
    let mut scene = scene_with_a_deck();
    let mut passenger = walker_on_the_deck(&mut scene);
    assert_eq!(
        say(&mut scene, &mut passenger, "/ship vanish on"),
        VANISH_ON
    );
    drop(passenger);
    scene.ticks(5);
    assert!(in_memory(&scene, 0));
    assert_eq!(scene.rig.files.len(), 0);
}

#[test]
fn a_ship_with_someone_else_aboard_stays_and_is_held_where_it_is_when_its_pilot_logs_out() {
    // (The passenger comes first: a ship whose pilot has the controls hovers, and a player cannot finish loading a view of one that moves.)
    let (mut scene, mut pilot) = deck_on_the_floor();
    let mut passenger = walker_on_the_floor_deck(&mut scene, 24.0);
    assert_eq!(player_pose(&scene, 2).grid, ship_id(0));
    assert_eq!(say(&mut scene, &mut pilot, "/ship pilot"), PILOT_REPLY);
    assert_eq!(say(&mut scene, &mut pilot, "/ship vanish on"), VANISH_ON);
    assert!(
        !scene
            .rig
            .server
            .world()
            .grids()
            .get(ship_id(0))
            .unwrap()
            .held
    );
    drop(pilot);
    tick_and_follow(&mut scene, &mut passenger, 3);
    // The passenger would lose their floor, so the ship stays, held; the pilot finds it where it is.
    assert!(in_memory(&scene, 0));
    assert!(
        scene
            .rig
            .server
            .world()
            .grids()
            .get(ship_id(0))
            .unwrap()
            .held
    );
    assert_eq!(player_pose(&scene, 2).grid, ship_id(0));
    assert_eq!(scene.rig.files.len(), 0);
}

#[test]
fn passengers_who_are_away_do_not_keep_the_ship_and_whoever_of_them_comes_back_has_it_brought_in() {
    let (mut scene, mut pilot) = deck_on_the_floor();
    let passenger = walker_on_the_floor_deck(&mut scene, 24.0);
    assert_eq!(say(&mut scene, &mut pilot, "/ship pilot"), PILOT_REPLY);
    assert_eq!(say(&mut scene, &mut pilot, "/ship vanish on"), VANISH_ON);
    // The passenger goes first: the ship stays, since its pilot is still at the controls.
    drop(passenger);
    tick_and_follow(&mut scene, &mut pilot, 3);
    assert!(in_memory(&scene, 0));
    // (Where they are in the ship now is where they stay: nothing moves a player who is away.)
    let passenger_place = eyes_in_their_frame(&scene, 2);
    // Then the pilot, and the ship goes with them though the passenger's record is aboard it.
    drop(pilot);
    scene.ticks(3);
    assert!(!in_memory(&scene, 0));
    assert!(parked(&scene, 0).unwrap().waits_for_those_aboard);

    // The passenger comes back first, not the pilot: they are aboard, so the ship is brought in for them where it was left.
    let mut passenger = scene.rig.join_walking_as(2, ELSEWHERE, 0.0, 0.0);
    scene.rig.settle(&mut [&mut passenger]);
    tick_and_follow(&mut scene, &mut passenger, 3);
    assert!(in_memory(&scene, 0));
    assert_eq!(player_pose(&scene, 3).grid, ship_id(0));
    // (A ship that is not held settles under a walker a little in the three ticks: the same place to a hundredth of a voxel.)
    let found = eyes_in_their_frame(&scene, 3);
    for axis in 0..3 {
        assert!(
            (found[axis] - passenger_place[axis]).abs() < 0.01,
            "{found:?} against {passenger_place:?}"
        );
    }
}

#[test]
fn a_ship_that_was_moving_when_its_pilot_left_is_at_rest_when_they_come_back() {
    let (mut scene, mut pilot) = deck_on_the_floor();
    assert_eq!(say(&mut scene, &mut pilot, "/ship pilot"), PILOT_REPLY);
    assert_eq!(say(&mut scene, &mut pilot, "/ship vanish on"), VANISH_ON);
    assert_eq!(
        say(&mut scene, &mut pilot, "/ship push 6 0 0"),
        "pushed ship 0"
    );
    tick_and_follow(&mut scene, &mut pilot, 2);
    let speed = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(0))
        .unwrap()
        .linear_velocity;
    assert!(speed[0] > 1.0, "it is moving: {speed:?}");
    drop(pilot);
    scene.ticks(3);
    assert!(!in_memory(&scene, 0));
    // What is in its file is a ship at rest. (Friction would have it at rest soon after it came back; this is what was written.)
    let in_the_file = ship_in_its_file(&mut scene, 0);
    assert_eq!(in_the_file.linear_velocity, [0.0; 3]);
    assert_eq!(in_the_file.angular_velocity, [0.0; 3]);

    let _back = pilot_returns(&mut scene);
    let ship = scene.rig.server.world().grids().get(ship_id(0)).unwrap();
    assert_eq!(ship.linear_velocity, [0.0; 3]);
    assert_eq!(ship.angular_velocity, [0.0; 3]);
}

#[test]
fn a_ship_that_went_away_with_its_pilot_is_there_when_the_program_has_been_started_again() {
    let (mut scene, pilot) = pilot_on_the_deck(Some(true));
    let pose = ship_pose(&scene, 0);
    let place_in_the_ship = eyes_in_their_frame(&scene, 1);
    drop(pilot);
    scene.ticks(3);
    assert!(!in_memory(&scene, 0));

    let mut scene = scene.restarted();
    // The world knows it is in a file and that it waits for those aboard, and does not bring it in for anyone else.
    assert!(parked(&scene, 0).unwrap().waits_for_those_aboard);
    scene.ticks(20);
    assert!(!in_memory(&scene, 0));

    let _back = pilot_returns(&mut scene);
    assert!(in_memory(&scene, 0));
    assert_eq!(ship_pose(&scene, 0), pose);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
    assert_eq!(eyes_in_their_frame(&scene, 1), place_in_the_ship);
}

#[test]
fn a_ship_that_cannot_be_put_in_a_file_stays_as_it_was_and_loses_nothing() {
    let failing = Arc::new(AtomicBool::new(false));
    let disk = Arc::clone(&failing);
    let mut scene = Scene::with_disk(move |files| {
        Box::new(FlakyFiles {
            inner: files,
            failing: disk,
        })
    });
    flat_floor(&mut scene);
    make_ship(
        &mut scene,
        pose_at(5.0, FLOOR, 20.0),
        false,
        [30, 2, 8],
        |_| true,
    );
    let mut pilot = walker_on_the_floor_deck(&mut scene, 20.0);
    assert_eq!(say(&mut scene, &mut pilot, "/ship pilot"), PILOT_REPLY);
    assert_eq!(say(&mut scene, &mut pilot, "/ship vanish on"), VANISH_ON);
    assert_eq!(
        say(&mut scene, &mut pilot, "/ship push 6 0 0"),
        "pushed ship 0"
    );
    tick_and_follow(&mut scene, &mut pilot, 1);
    let speed_before = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(0))
        .unwrap()
        .linear_velocity;
    assert!(speed_before[0] > 1.0, "{speed_before:?}");

    failing.store(true, Ordering::SeqCst);
    drop(pilot);
    scene.ticks(1);
    // It could not be put away: it is where it is and goes on as it was (it is not stopped as a ship that is put away is).
    assert!(in_memory(&scene, 0));
    assert!(parked(&scene, 0).is_none());
    assert_eq!(scene.rig.files.len(), 0);
    let speed_after = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(0))
        .unwrap()
        .linear_velocity;
    assert!(
        speed_after[0] > 0.5 * speed_before[0],
        "{speed_before:?} then {speed_after:?}"
    );
}

#[test]
fn a_vanish_that_is_asked_for_is_said_so_and_a_command_that_is_not_one_says_how_it_goes() {
    let mut scene = scene_with_a_deck();
    let mut pilot = walker_on_the_deck(&mut scene);
    assert_eq!(say(&mut scene, &mut pilot, "/ship vanish on"), VANISH_ON);
    assert_eq!(say(&mut scene, &mut pilot, "/ship vanish off"), VANISH_OFF);
    assert_eq!(
        say(&mut scene, &mut pilot, "/ship vanish maybe"),
        "usage: /ship create [all] | push X Y Z | hold on|off | pilot | vanish on|off"
    );
    assert_eq!(
        say(&mut scene, &mut pilot, "/ship vanish"),
        "usage: /ship create [all] | push X Y Z | hold on|off | pilot | vanish on|off"
    );
    // Away from every ship there is none to tell.
    pilot.send_command("/tp 40 300 40");
    tick_and_follow(&mut scene, &mut pilot, 5);
    pilot.take_command_replies();
    assert!(say(&mut scene, &mut pilot, "/ship vanish on").contains("no ship is near"));
}

const LOST: &str = "The ship you were aboard cannot be found. 1) Go to the world spawn  2) Appear in the air where you were  3) Quit to the title  (.choose N)";

/// The pilot went away with the ship, the file of the ship was lost, and they have come back.
fn pilot_whose_ship_is_lost() -> (Scene, Player) {
    let (mut scene, pilot) = pilot_on_the_deck(Some(true));
    drop(pilot);
    scene.ticks(3);
    assert!(!in_memory(&scene, 0));
    let mut files = scene.rig.files.clone();
    for name in files.names().unwrap() {
        files.remove(&name).unwrap();
    }
    assert_eq!(scene.rig.files.len(), 0);
    let back = pilot_returns(&mut scene);
    (scene, back)
}

#[test]
fn a_pilot_whose_ship_cannot_be_found_is_asked_and_can_go_to_the_spawn() {
    let (mut scene, mut back) = pilot_whose_ship_is_lost();
    assert!(back.hud_text().starts_with(LOST), "{}", back.hud_text());
    assert!(!in_memory(&scene, 0));
    assert_eq!(
        back.client_command(".choose 1"),
        "you chose Go to the world spawn"
    );
    tick_and_follow(&mut scene, &mut back, 3);
    let pose = player_pose(&scene, 2);
    assert_eq!(pose.grid, GridId::root(LevelIndex::MAIN));
    let [x, y, z] = voxel_space_in_voxels(&pose);
    assert!(
        (x - 16.0).abs() < 0.5 && (z - 16.0).abs() < 0.5 && (100.0..=110.5).contains(&y),
        "{x} {y} {z}"
    );
    assert!(!back.hud_text().contains("(.choose N)"));
}

fn voxel_space_in_voxels(pose: &voxel_space::EntityPose) -> [f64; 3] {
    voxunami_cave::aboard::in_voxels(pose.position)
}

#[test]
fn a_pilot_whose_ship_cannot_be_found_can_quit_instead() {
    let (mut scene, mut back) = pilot_whose_ship_is_lost();
    assert_eq!(
        back.client_command(".choose 3"),
        "you chose Quit to the title"
    );
    scene.ticks(3);
    assert_eq!(scene.rig.server.connected_clients().count(), 0);
}

const IN_THE_WAY: [i64; 3] = [20, FLOOR as i64 + 3, 24];
const BLOCKED: &str = "Something is where your ship was left. 1) Move the ship to the nearest free place  2) Break the terrain that is in the way  3) Break the blocks of the ship that are in the way  4) Put the ship there anyway (it may be glitchy)  5) Go to the world spawn and leave the ship behind  (.choose N)";

/// The pilot went away with the deck, someone built a block where the lowest layer of it was, and the pilot has come back and been asked.
fn pilot_whose_ship_would_land_in_a_block() -> (Scene, Player, GridPose) {
    let (mut scene, pilot) = pilot_on_the_deck(Some(true));
    let pose = ship_pose(&scene, 0);
    drop(pilot);
    scene.ticks(3);
    assert!(!in_memory(&scene, 0));
    put_stone(&mut scene, IN_THE_WAY);
    let mut back = pilot_returns(&mut scene);
    assert!(back.hud_text().starts_with(BLOCKED), "{}", back.hud_text());
    assert!(!in_memory(&scene, 0));
    (scene, back, pose)
}

#[test]
fn a_pilot_whose_ship_would_land_in_a_block_is_asked_and_can_have_the_ship_moved_to_the_nearest_free_place()
 {
    let (mut scene, mut back, pose) = pilot_whose_ship_would_land_in_a_block();
    let place_in_the_ship = eyes_in_their_frame(&scene, 2);
    assert_eq!(
        back.client_command(".choose 1"),
        "you chose Move the ship to the nearest free place"
    );
    tick_and_follow(&mut scene, &mut back, 3);
    assert!(in_memory(&scene, 0));
    // A voxel up clears the one block that was in the way, which is under the deck's upper layer.
    let moved = GridPose::new(pose.position.translated([0.0, 1.0, 0.0]), pose.orientation);
    assert_eq!(ship_pose(&scene, 0), moved);
    assert_eq!(player_pose(&scene, 2).grid, ship_id(0));
    assert_eq!(eyes_in_their_frame(&scene, 2), place_in_the_ship);
    let stone = voxunami_cave::content::CaveContent::new().blocks.stone;
    assert_eq!(block_at(&mut scene, IN_THE_WAY), stone);
    assert!(!back.hud_text().contains("(.choose N)"));
}

#[test]
fn a_pilot_whose_ship_would_land_in_a_block_can_have_the_block_broken() {
    let (mut scene, mut back, pose) = pilot_whose_ship_would_land_in_a_block();
    assert_eq!(
        back.client_command(".choose 2"),
        "you chose Break the terrain that is in the way"
    );
    tick_and_follow(&mut scene, &mut back, 3);
    assert!(in_memory(&scene, 0));
    assert_eq!(ship_pose(&scene, 0), pose);
    let air = voxunami_cave::content::CaveContent::new().blocks.air;
    assert_eq!(block_at(&mut scene, IN_THE_WAY), air);
}

#[test]
fn a_pilot_whose_ship_would_land_in_a_block_can_leave_the_ship_behind_and_the_ship_is_then_anyones()
{
    let (mut scene, pilot) = pilot_on_the_deck(Some(true));
    drop(pilot);
    scene.ticks(3);
    put_stone(&mut scene, IN_THE_WAY);
    // From here on ships are kept by how near the players are; the spawn is far below the deck.
    scene
        .rig
        .server
        .set_ship_settings(ShipSettings::new(40.0, 400.0).unwrap());
    let mut back = pilot_returns(&mut scene);
    assert!(back.hud_text().starts_with(BLOCKED), "{}", back.hud_text());
    assert_eq!(
        back.client_command(".choose 5"),
        "you chose Go to the world spawn and leave the ship behind"
    );
    tick_and_follow(&mut scene, &mut back, 3);
    assert_eq!(player_pose(&scene, 2).grid, GridId::root(LevelIndex::MAIN));
    assert!(!in_memory(&scene, 0));
    assert!(!parked(&scene, 0).unwrap().waits_for_those_aboard);
}
