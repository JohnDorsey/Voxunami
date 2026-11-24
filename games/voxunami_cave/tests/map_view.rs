//! Cave's keys for the picture's camera: `M` is the map, `V` lets the camera go and fly, `Tab` is the map's next focus (`docs/specs/viewpoints-spec.md`, phase 5).

mod common;

use common::{Player, PlayerExt, Rig, above_ground};
use game_shell::KeyCode;
use game_shell::session::ViewMode;
use game_shell::view_rig::Focus;

fn standing(rig: &mut Rig) -> Player {
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.camera = above_ground(&player, 16, 16, 2.0);
    player.teleport_to_camera();
    rig.settle(&mut [&mut player]);
    player
}

#[test]
fn m_draws_the_map_and_draws_the_players_eyes_again() {
    let mut rig = Rig::new();
    let mut player = standing(&mut rig);
    assert_eq!(player.view_mode(), ViewMode::Body);
    let eyes = player.drawn_camera();
    player.key_pressed(KeyCode::KeyM);
    assert_eq!(player.view_mode(), ViewMode::Map);
    assert_ne!(
        player.drawn_camera().position,
        eyes.position,
        "the map is not at the player's eyes"
    );
    assert_eq!(
        player.camera, eyes,
        "the player's own camera is where it was"
    );
    player.key_pressed(KeyCode::KeyM);
    assert_eq!(player.view_mode(), ViewMode::Body);
    assert_eq!(player.drawn_camera(), eyes);
}

#[test]
fn v_lets_the_camera_go_from_where_it_is_and_gives_it_back() {
    let mut rig = Rig::new();
    let mut player = standing(&mut rig);
    let eyes = player.drawn_camera();
    player.key_pressed(KeyCode::KeyV);
    assert_eq!(player.view_mode(), ViewMode::Fly);
    assert_eq!(player.drawn_camera(), eyes, "it starts where the eyes are");
    // From the map, it starts where the map is.
    player.key_pressed(KeyCode::KeyM);
    assert_eq!(player.view_mode(), ViewMode::Map);
    let map = player.drawn_camera();
    player.key_pressed(KeyCode::KeyV);
    assert_eq!(player.view_mode(), ViewMode::Fly);
    assert_eq!(player.drawn_camera(), map);
    player.key_pressed(KeyCode::KeyV);
    assert_eq!(player.view_mode(), ViewMode::Body);
}

#[test]
fn tab_is_the_maps_next_focus_only_while_the_map_is_drawn() {
    let mut rig = Rig::new();
    let mut player = standing(&mut rig);
    player.key_pressed(KeyCode::Tab);
    assert_eq!(
        player.view_mode(),
        ViewMode::Body,
        "Tab does nothing to the player's eyes"
    );
    player.key_pressed(KeyCode::KeyM);
    assert_eq!(player.focus(), Focus::Body);
    // (A world with nothing but the player has nothing else to lock on.)
    player.key_pressed(KeyCode::Tab);
    assert_eq!(player.focus(), Focus::Body);
}
