//! Voxunami Cave as the shell runs it: what the player's clicks and keys do.

use game_shell::{CameraStart, GameDefinition, KeyCode};
use namespaced_id::NamespacedId;
use server::GameLogic;
use transport::Connection;
use world_storage::WorldData;

use crate::content::{CaveContent, game_id};
use crate::logic::{CaveLogic, REACH};
use crate::messages::{SetBlock, encode_set_block, set_block_kind};

/// The blocks a player can place, in the order the number keys choose them.
pub const PLACEABLE_BLOCKS: [&str; 3] =
    ["base_blocks:stone", "base_blocks:dirt", "base_blocks:grass"];

/// What the cave client remembers.
#[derive(Debug, Default)]
pub struct CaveClientState {
    pub selected_block: usize,
}

#[derive(Debug)]
pub struct Cave;

pub type CaveSession<C> = game_shell::session::Session<C, Cave>;

/// Asks the server to put `block` (named by identifier) at `at`. The server decides.
pub fn request_block<C: Connection>(
    session: &mut CaveSession<C>,
    at: voxel_space::VoxelCoordinate,
    block: &str,
) {
    let request = SetBlock {
        at,
        block: block.parse().expect("block names are valid identifiers"),
    };
    session.send_game_message(set_block_kind(), encode_set_block(&request));
}

/// Breaks the block under the crosshair, if there is one.
pub fn break_block<C: Connection>(session: &mut CaveSession<C>) {
    if let Some(hit) = session.target() {
        request_block(session, hit.voxel, "base_blocks:air");
    }
}

/// Places `block` against the face under the crosshair, if there is one.
pub fn place_block<C: Connection>(session: &mut CaveSession<C>, block: &str) {
    if let Some(hit) = session.target() {
        request_block(session, hit.previous, block);
    }
}

impl GameDefinition for Cave {
    type Content = CaveContent;
    type ClientState = CaveClientState;

    fn id() -> NamespacedId {
        game_id()
    }

    fn title() -> &'static str {
        "Voxunami Cave"
    }

    fn command() -> &'static str {
        "cave"
    }

    fn summary() -> &'static str {
        "fly through caverns and break and place blocks."
    }

    fn controls() -> &'static str {
        "Left click breaks the block you look at; right click places the selected block;\n1, 2, 3 select stone, dirt, grass."
    }

    fn default_seed() -> u64 {
        2024
    }

    fn default_camera() -> CameraStart {
        CameraStart {
            position: [16.0, 110.0, 16.0],
            yaw_degrees: 0.0,
            pitch_degrees: -15.0,
        }
    }

    fn reach() -> f32 {
        REACH as f32
    }

    fn new_content() -> CaveContent {
        CaveContent::new()
    }

    fn new_logic(content: &CaveContent, _world: &WorldData) -> Box<dyn GameLogic> {
        Box::new(CaveLogic::new(content))
    }

    fn primary_action<C: Connection>(session: &mut CaveSession<C>) {
        break_block(session);
    }

    fn secondary_action<C: Connection>(session: &mut CaveSession<C>) {
        let block = PLACEABLE_BLOCKS[session.state.selected_block];
        place_block(session, block);
    }

    fn key_pressed<C: Connection>(session: &mut CaveSession<C>, key: KeyCode) {
        match key {
            KeyCode::Digit1 => session.state.selected_block = 0,
            KeyCode::Digit2 => session.state.selected_block = 1,
            KeyCode::Digit3 => session.state.selected_block = 2,
            _ => {}
        }
    }

    fn status_text<C: Connection>(session: &CaveSession<C>) -> String {
        let block = PLACEABLE_BLOCKS[session.state.selected_block];
        format!(
            "placing {}",
            block.split(':').next_back().unwrap_or_default()
        )
    }
}
