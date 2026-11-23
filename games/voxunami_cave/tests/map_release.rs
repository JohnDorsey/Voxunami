//! A map kept ready over a planet, and taken away (`docs/specs/viewpoints-spec.md`, 9i): the pictures made for it alone are let go of when the player's setting says so
//! (`.maprelease`, on by default), and left to the budget when it says not.

mod common;

use std::time::{Duration, Instant};

use common::{Scene, say, tick_and_follow};
use game_shell::GameDefinition;
use voxel_space::GridId;
use voxunami_cave::Cave;
use world_view::MeshUpdate;

/// Where the surface of the planet is, over the middle of its hills (the planet's radius is 63,710 voxels).
const SURFACE_Y: f64 = 64_200.0;

/// Updates the player's view until nothing has changed for a good many updates in a row, telling `note` of every change.
fn update_until_quiet(
    scene: &mut Scene,
    player: &mut common::Player,
    mut note: impl FnMut(&MeshUpdate),
) {
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut quiet = 0;
    while quiet < 250 {
        assert!(Instant::now() < deadline, "the view does not come to rest");
        tick_and_follow(scene, player, 1);
        let updates = player.update_view();
        if updates.is_empty() {
            quiet += 1;
            std::thread::sleep(Duration::from_millis(2));
        } else {
            quiet = 0;
            updates.iter().for_each(&mut note);
        }
    }
}

/// How many pictures of nodes were put up hidden for a place that is kept ready 5 km along the ground, and how many the renderer was told to take down when it was not
/// kept ready any more, with the player's setting as given.
fn pictures_made_for_a_map_and_taken_down_when_it_is_taken_away(release: bool) -> (usize, usize) {
    let mut scene = Scene::in_a_new_world();
    let mut player = scene
        .rig
        .join_walking(Cave::default_camera().position, 0.0, 0.0);
    player.enable_level_detail();
    player.hold_hidden_pictures(game_shell::session::HELD_PICTURE_BUDGET);
    player.enable_grid_detail(
        <Cave as GameDefinition>::grid_detail().expect("Cave draws planets in detail"),
    );
    player.set_release_map_pictures(release);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 5);
    player.take_command_replies();
    say(&mut scene, &mut player, "/fly");

    // The player is over the planet's surface, and the place the map will be at is read off by going there and coming back.
    let mut planet: Option<GridId> = None;
    let mut note_planet = |update: &MeshUpdate| {
        if let MeshUpdate::UploadGridCoarse { grid, .. } = update {
            planet = Some(*grid);
        }
    };
    player.send_command(&format!("/tp 16 {SURFACE_Y} 16"));
    tick_and_follow(&mut scene, &mut player, 2);
    update_until_quiet(&mut scene, &mut player, &mut note_planet);
    let planet = planet.expect("the planet is drawn");
    player.send_command(&format!("/tp 5016 {} 16", SURFACE_Y - 196.0));
    tick_and_follow(&mut scene, &mut player, 2);
    let map = player
        .camera_place_in(planet)
        .expect("a place on the planet");
    player.send_command(&format!("/tp 16 {SURFACE_Y} 16"));
    tick_and_follow(&mut scene, &mut player, 2);
    update_until_quiet(&mut scene, &mut player, |_| {});

    let mut put_up_hidden = 0;
    player.keep_ready(planet, Some(map));
    update_until_quiet(&mut scene, &mut player, |update| {
        if matches!(update, MeshUpdate::UploadHiddenGridCoarse { mesh, .. } if !mesh.is_empty()) {
            put_up_hidden += 1;
        }
    });
    let mut taken_down = 0;
    player.keep_ready(planet, None);
    update_until_quiet(&mut scene, &mut player, |update| {
        if matches!(update, MeshUpdate::RemoveGridCoarse { .. }) {
            taken_down += 1;
        }
    });
    (put_up_hidden, taken_down)
}

#[test]
fn the_pictures_made_for_a_map_over_a_planet_are_let_go_of_when_it_is_taken_away() {
    let (made, taken_down) = pictures_made_for_a_map_and_taken_down_when_it_is_taken_away(true);
    assert!(made > 1000, "{made} pictures were made for the map");
    assert_eq!(
        taken_down, made,
        "every picture made for the map alone is taken down with it"
    );
}

#[test]
fn the_pictures_made_for_a_map_over_a_planet_are_kept_when_the_player_says_to_keep_them() {
    let (made, taken_down) = pictures_made_for_a_map_and_taken_down_when_it_is_taken_away(false);
    assert!(made > 1000, "{made} pictures were made for the map");
    assert_eq!(
        taken_down, 0,
        "nothing is taken down: the budget is not full"
    );
}
