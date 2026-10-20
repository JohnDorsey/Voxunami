//! Voxunami Cave as the shell runs it: what the player's clicks and keys do.

use std::collections::HashSet;

use game_shell::client_plugin::ClientPlugin;
use game_shell::{CameraStart, GameDefinition, KeyCode, PlayerPose};
use namespaced_id::NamespacedId;
use physics::MoveInput;
use plugin_host::Plugin;
use renderer::Camera;
use transport::Connection;
use voxel_space::WorldPosition;

use crate::client::{
    ClientBlockEditing, ClientItems, ClientLight, ClientMovement, set_block_request,
};
use crate::content::{CaveContent, game_id};
use crate::logic::REACH;
use crate::messages::{MoveRequest, SetBlock, Spawn};
use crate::multiscale_light::LightScheme;
use crate::plugins::cave_plugins;
use protocol::Message;

/// The blocks a player can place, in the order the number keys choose them.
pub const PLACEABLE_BLOCKS: [&str; 6] = [
    "base_blocks:stone",
    "base_blocks:dirt",
    "base_blocks:grass",
    "base_blocks:lamp",
    "base_blocks:wood",
    "base_blocks:fire",
];

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
    session.send_game_message(
        SetBlock::kind(),
        set_block_request(at, block, state).encode(),
    );
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

/// How the world's multiscale light is cut into cells in this session, if it has that light (the server
/// says).
pub fn light_scheme<C: Connection>(session: &CaveSession<C>) -> Option<LightScheme> {
    session
        .client_plugin::<ClientLight>()
        .and_then(ClientLight::scheme)
}

impl GameDefinition for Cave {
    type Content = CaveContent;

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
        Some((Spawn::kind(), Spawn(pose).encode()))
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
        Some((MoveRequest::kind(), request.encode()))
    }

    fn new_content() -> CaveContent {
        CaveContent::new()
    }

    fn plugins(content: &CaveContent) -> Vec<Box<dyn Plugin>> {
        cave_plugins(content)
    }

    fn client_plugins(content: &CaveContent) -> Vec<Box<dyn ClientPlugin>> {
        vec![
            Box::new(ClientBlockEditing::default()),
            Box::new(ClientItems::new(content)),
            Box::new(ClientLight::new(content)),
            Box::new(ClientMovement),
        ]
    }
}
