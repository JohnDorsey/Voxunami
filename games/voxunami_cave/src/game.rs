//! Voxunami Cave as the shell runs it: what the player's clicks and keys do.

use base_blocks::{SHAPE_NAMES, shape_name};
use std::collections::HashSet;

use game_shell::{CameraStart, GameContent, GameDefinition, KeyCode, PlayerPose};
use namespaced_id::NamespacedId;
use physics::MoveInput;
use renderer::Camera;
use server::GameLogic;
use transport::Connection;
use voxel_space::WorldPosition;
use world_storage::WorldData;
use world_view::{ScaledLight, ViewChannels};

use crate::content::{CaveContent, game_id};
use crate::logic::{CaveLogic, REACH};
use crate::messages::{
    MoveRequest, SetBlock, decode_light_scheme, encode_move_request, encode_set_block,
    encode_spawn, light_scheme_kind, move_input_kind, set_block_kind, spawn_kind,
};
use crate::multiscale_light::{LEVEL_FORMAT, LightScheme, full_brightness};

/// The blocks a player can place, in the order the number keys choose them.
pub const PLACEABLE_BLOCKS: [&str; 6] = [
    "base_blocks:stone",
    "base_blocks:dirt",
    "base_blocks:grass",
    "base_blocks:lamp",
    "base_blocks:wood",
    "base_blocks:fire",
];

/// What the cave client remembers.
#[derive(Debug, Default)]
pub struct CaveClientState {
    pub selected_block: usize,
    /// The state, which is the shape, that placed blocks get (see [`SHAPE_NAMES`]).
    pub selected_shape: u32,
    /// How the world's multiscale light is cut into cells, if it has that light (the server says).
    pub light_scheme: Option<LightScheme>,
}

#[derive(Debug)]
pub struct Cave;

pub type CaveSession<C> = game_shell::session::Session<C, Cave>;

/// Asks the server to put `block` (named by identifier) at `at`, as a whole cube. The server
/// decides.
pub fn request_block<C: Connection>(
    session: &mut CaveSession<C>,
    at: voxel_space::VoxelCoordinate,
    block: &str,
) {
    request_block_with_state(session, at, block, 0);
}

/// Asks the server to put `block` in `state` (which shape it has) at `at`. The server decides.
pub fn request_block_with_state<C: Connection>(
    session: &mut CaveSession<C>,
    at: voxel_space::VoxelCoordinate,
    block: &str,
    state: u32,
) {
    let request = SetBlock {
        at,
        block: block.parse().expect("block names are valid identifiers"),
        state,
    };
    session.send_game_message(set_block_kind(), encode_set_block(&request));
}

/// Breaks the block under the crosshair, if there is one.
pub fn break_block<C: Connection>(session: &mut CaveSession<C>) {
    if let Some(hit) = session.target() {
        request_block(session, hit.voxel, "base_blocks:air");
    }
}

/// Places `block`, as a whole cube, against the face under the crosshair, if there is one.
pub fn place_block<C: Connection>(session: &mut CaveSession<C>, block: &str) {
    place_block_with_state(session, block, 0);
}

/// Places `block` in `state` against the face under the crosshair, if there is one.
pub fn place_block_with_state<C: Connection>(
    session: &mut CaveSession<C>,
    block: &str,
    state: u32,
) {
    if let Some(hit) = session.target() {
        request_block_with_state(session, hit.previous, block, state);
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
        "Left click breaks the block you look at; right click places the selected block;\n1 to 6 select stone, dirt, grass, lamp, wood, fire (which burns grass and wood); R cycles whole block, slabs and stairs.
W A S D walk, Space jumps, Ctrl runs; F flies or walks (then Space and Shift go up and down).
Commands: /level shows which level you are in, /level list lists them, /level go NAME moves you; /fly toggles flying; /tp X Y Z teleports; /light classic or /light multiscale chooses the light.
Window commands (start with a period): .ambient 0-255 sets how bright a face is that no light reaches (0, the default, is black; 20 to 40 is dim like a cave), .help lists them."
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

    fn server_moves_players() -> bool {
        true
    }

    fn join_message(position: WorldPosition) -> Option<(NamespacedId, Vec<u8>)> {
        let pose = PlayerPose {
            position,
            yaw: 0.0,
            pitch: 0.0,
        };
        Some((spawn_kind(), encode_spawn(&pose)))
    }

    fn movement_message(
        keys: &HashSet<KeyCode>,
        camera: &Camera,
    ) -> Option<(NamespacedId, Vec<u8>)> {
        let held = |code: KeyCode| i8::from(keys.contains(&code));
        let request = MoveRequest {
            input: MoveInput {
                forward: held(KeyCode::KeyW) - held(KeyCode::KeyS),
                strafe: held(KeyCode::KeyD) - held(KeyCode::KeyA),
                vertical: held(KeyCode::Space)
                    - held(KeyCode::ShiftLeft).max(held(KeyCode::ShiftRight)),
                jump: keys.contains(&KeyCode::Space),
                sprint: keys.contains(&KeyCode::ControlLeft)
                    || keys.contains(&KeyCode::ControlRight),
                yaw: camera.yaw,
            },
            pitch: camera.pitch,
        };
        Some((move_input_kind(), encode_move_request(&request)))
    }

    fn new_content() -> CaveContent {
        CaveContent::new()
    }

    fn new_logic(content: &CaveContent, world: &WorldData) -> Box<dyn GameLogic> {
        Box::new(CaveLogic::new(content, world))
    }

    fn view_channels(content: &CaveContent, state: &CaveClientState) -> ViewChannels {
        let mut channels = content.view_channels();
        if let Some(scheme) = state.light_scheme {
            for (scale, slot) in channels
                .scaled_light
                .iter_mut()
                .enumerate()
                .take(scheme.scales())
            {
                let width = scheme.widths()[scale];
                *slot = Some(ScaledLight {
                    channel: content.scale_channel(scale),
                    cell_width: width,
                    full_brightness: full_brightness(width),
                    format: LEVEL_FORMAT,
                });
            }
        }
        channels
    }

    fn handle_game_message(state: &mut CaveClientState, kind: &NamespacedId, payload: &[u8]) {
        // A message this client cannot read leaves the light as it was drawn.
        if *kind == light_scheme_kind()
            && let Ok(scheme) = decode_light_scheme(payload)
        {
            state.light_scheme = scheme;
        }
    }

    fn primary_action<C: Connection>(session: &mut CaveSession<C>) {
        break_block(session);
    }

    fn secondary_action<C: Connection>(session: &mut CaveSession<C>) {
        let block = PLACEABLE_BLOCKS[session.state.selected_block];
        let state = session.state.selected_shape;
        place_block_with_state(session, block, state);
    }

    fn key_pressed<C: Connection>(session: &mut CaveSession<C>, key: KeyCode) {
        match key {
            KeyCode::Digit1 => session.state.selected_block = 0,
            KeyCode::Digit2 => session.state.selected_block = 1,
            KeyCode::Digit3 => session.state.selected_block = 2,
            KeyCode::Digit4 => session.state.selected_block = 3,
            KeyCode::Digit5 => session.state.selected_block = 4,
            KeyCode::Digit6 => session.state.selected_block = 5,
            KeyCode::KeyF => session.send_command("/fly"),
            KeyCode::KeyR => {
                session.state.selected_shape =
                    (session.state.selected_shape + 1) % SHAPE_NAMES.len() as u32;
            }
            _ => {}
        }
    }

    fn status_text<C: Connection>(session: &CaveSession<C>) -> String {
        let block = PLACEABLE_BLOCKS[session.state.selected_block];
        format!(
            "placing {} ({})",
            block.split(':').next_back().unwrap_or_default(),
            shape_name(session.state.selected_shape)
        )
    }
}
