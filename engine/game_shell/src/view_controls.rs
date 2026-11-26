//! Which of the window's input goes to the player and which to the picture's camera (`docs/specs/viewpoints-spec.md`, phase 5, and 9s).
//!
//! **A map takes no key.** The keys that walk are the keys that fly a rocket, and a pilot flies with the map up: while the map is drawn every key held goes where it goes
//! when the player's own eyes are drawn, and the map is worked with the mouse (which turns it) and its wheel (which zooms it). Only a camera that was detached and flies is
//! moved by the keys, since it has nothing else to be moved with; the player's body hears none meanwhile.

use crate::session::ViewMode;

/// Whose the keys that are held down are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeysBelongTo {
    /// The player's body, or the ship they pilot.
    Body,
    /// The camera that was let go of.
    DetachedCamera,
}

pub(crate) fn keys_belong_to(mode: ViewMode) -> KeysBelongTo {
    match mode {
        ViewMode::Body | ViewMode::Map => KeysBelongTo::Body,
        ViewMode::Fly => KeysBelongTo::DetachedCamera,
    }
}

/// What a turn of the wheel is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WheelGoesTo {
    /// The map comes nearer or goes farther. The game does not hear the turn.
    MapZoom,
    /// The game, as presses of [`crate::client_plugin::MouseButton::WheelUp`] and `WheelDown`.
    Game,
    Nothing,
}

/// `modal` is whether an interface on the screen has the player's input. The map is zoomed whether or not the mouse is captured; the game hears the wheel only while it is.
pub(crate) fn wheel_goes_to(mode: ViewMode, mouse_captured: bool, modal: bool) -> WheelGoesTo {
    if modal {
        WheelGoesTo::Nothing
    } else if mode == ViewMode::Map {
        WheelGoesTo::MapZoom
    } else if mouse_captured {
        WheelGoesTo::Game
    } else {
        WheelGoesTo::Nothing
    }
}

/// How much farther a notch of the wheel toward the player sends the map, as a power of e.
const MAP_WHEEL_ZOOM: f64 = 0.15;

/// What the distance of the map is multiplied by for `lines` of the wheel (positive is the wheel turned away from the player, which brings the map nearer).
pub(crate) fn map_zoom_factor(lines: f64) -> f64 {
    libm::exp(-lines * MAP_WHEEL_ZOOM)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_are_the_bodys_with_a_map_up_and_a_detached_cameras_only_while_it_flies() {
        assert_eq!(keys_belong_to(ViewMode::Body), KeysBelongTo::Body);
        assert_eq!(keys_belong_to(ViewMode::Map), KeysBelongTo::Body);
        assert_eq!(keys_belong_to(ViewMode::Fly), KeysBelongTo::DetachedCamera);
    }

    #[test]
    fn the_wheel_zooms_a_map_with_the_mouse_captured_or_not_and_the_game_does_not_hear_it() {
        for captured in [false, true] {
            assert_eq!(
                wheel_goes_to(ViewMode::Map, captured, false),
                WheelGoesTo::MapZoom,
                "captured {captured}"
            );
        }
    }

    #[test]
    fn without_a_map_the_wheel_is_the_games_while_the_mouse_is_captured_and_nobodys_otherwise() {
        for mode in [ViewMode::Body, ViewMode::Fly] {
            assert_eq!(wheel_goes_to(mode, true, false), WheelGoesTo::Game);
            assert_eq!(wheel_goes_to(mode, false, false), WheelGoesTo::Nothing);
        }
    }

    #[test]
    fn an_interface_that_has_the_input_keeps_the_wheel_from_the_map_and_the_game() {
        for mode in [ViewMode::Body, ViewMode::Map, ViewMode::Fly] {
            for captured in [false, true] {
                assert_eq!(
                    wheel_goes_to(mode, captured, true),
                    WheelGoesTo::Nothing,
                    "{mode:?} {captured}"
                );
            }
        }
    }

    #[test]
    fn a_turn_of_the_wheel_away_brings_the_map_nearer_and_the_same_turn_back_undoes_it() {
        assert!(map_zoom_factor(1.0) < 1.0);
        assert!(map_zoom_factor(-1.0) > 1.0);
        assert!((map_zoom_factor(1.0) * map_zoom_factor(-1.0) - 1.0).abs() < 1e-12);
        assert_eq!(map_zoom_factor(0.0), 1.0);
    }
}
