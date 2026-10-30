//! Voxunami Cave as the shell runs it: what the player's clicks and keys do.

use std::collections::HashSet;

use game_shell::client_plugin::ClientPlugin;
use game_shell::{CameraStart, CoarseDetail, GameDefinition, KeyCode, PlayerPose, Sky, Sun};
use namespaced_id::NamespacedId;
use physics::MoveInput;
use plugin_host::Plugin;
use renderer::Camera;
use transport::Connection;
use voxel_space::{GravitySettings, WorldPosition};

use crate::client::{
    ClientBlockEditing, ClientItems, ClientLight, ClientMovement, place_block_request,
    set_block_request,
};
use crate::content::{CaveContent, game_id, mini_cosmos_id, overworld_id};
use crate::logic::REACH;
use crate::messages::{MoveRequest, SetBlock, Spawn};
use crate::multiscale_light::LightScheme;
use crate::plugins::cave_plugins;
use protocol::Message;

#[derive(Debug)]
pub struct Cave;

pub type CaveSession<C> = game_shell::session::Session<C, Cave>;

/// Asks the server to put `block` (named by identifier) at `at` of the terrain of the level the player is in, as a whole cube. The
/// server decides.
pub fn request_block<C: Connection>(
    session: &mut CaveSession<C>,
    at: voxel_space::VoxelCoordinate,
    block: &str,
) {
    request_block_with_state(session, at, block, 0);
}

/// Asks the server to put `block` in `state` (which shape it has) at `at` of the terrain of the level the player is in. The server
/// decides.
pub fn request_block_with_state<C: Connection>(
    session: &mut CaveSession<C>,
    at: voxel_space::VoxelCoordinate,
    block: &str,
    state: u32,
) {
    let grid = voxel_space::GridId::root(session.level());
    request_block_in(session, grid, at, block, state);
}

/// Asks the server to put `block` in `state` at `at` of the grid `grid`: the terrain of the level the player is in, or a ship in it.
pub fn request_block_in<C: Connection>(
    session: &mut CaveSession<C>,
    grid: voxel_space::GridId,
    at: voxel_space::VoxelCoordinate,
    block: &str,
    state: u32,
) {
    session.send_game_message(
        SetBlock::kind(),
        set_block_request(grid, at, block, state).encode(),
    );
}

/// Breaks the block under the crosshair, if there is one.
pub fn break_block<C: Connection>(session: &mut CaveSession<C>) {
    if let Some(hit) = session.target() {
        request_block_in(session, hit.grid, hit.voxel, "base_blocks:air", 0);
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
        session.send_game_message(
            SetBlock::kind(),
            place_block_request(&hit, block, state).encode(),
        );
    }
}

/// How the world's multiscale light is cut into cells in this session, if it has that light (the server
/// says).
pub fn light_scheme<C: Connection>(session: &CaveSession<C>) -> Option<LightScheme> {
    session
        .client_plugin::<ClientLight>()
        .and_then(ClientLight::scheme)
}

/// The least mass, in kilograms, of a ship that pulls other things toward it: a moon, not a ship that someone built (a ship of a thousand tonnes, 10^6
/// kilograms, pulls a player a metre from it by about two hundred-thousandths of a metre a second a second; this is ten million million times its mass).
pub const PULLING_MASS: f32 = 1.0e19;

impl GameDefinition for Cave {
    type Content = CaveContent;

    fn id() -> NamespacedId {
        game_id()
    }

    fn gravity() -> GravitySettings {
        GravitySettings {
            pull_mass_threshold: Some(PULLING_MASS),
            ..GravitySettings::default()
        }
    }

    /// Space has no pull of its own.
    fn level_gravity() -> Vec<(NamespacedId, [f32; 3])> {
        vec![(mini_cosmos_id(), [0.0; 3])]
    }

    /// A planet is drawn out to ten thousand kilometres, from which the one of the first level is a few pixels across, with the light that the bricks of its generator
    /// have (none: the nodes are as bright as the bricks beside them). The terrain of a level is drawn beyond its bricks only in the level of space (`level_detail`).
    fn grid_detail() -> Option<CoarseDetail> {
        Some(CoarseDetail {
            maximum_distance: 1.0e7,
            unlit: 0,
            cell_pixels: 3.0,
            bytes_per_second: 0,
        })
    }

    /// The level of space is drawn in coarse nodes out past the size of the universe (93 billion light years is 8.8 x 10^26 metres across), so that the great rivers are seen
    /// from anywhere; the overworld is not drawn beyond its bricks. The light of terrain that nobody has lit is none: the nodes are as dark as the bricks beside them.
    fn level_detail(level: &NamespacedId) -> Option<CoarseDetail> {
        (*level == mini_cosmos_id()).then_some(CoarseDetail {
            maximum_distance: 2.0e27,
            unlit: 0,
            cell_pixels: 3.0,
            bytes_per_second: 0,
        })
    }

    /// A day of twenty minutes of the world's clock that begins at half past six in the morning, in the level of space and in the overworld: the sun is seven degrees
    /// above the horizon where players start, so that the shadows are long and easy to see. The sun is in the plane of a path that goes a little to one side of the top of the
    /// planet at noon (so that it is never straight overhead at the place players start), and it is hidden by the planet in space and by the flat ground of the overworld. The sky is the blue the game always had, to a height of 30 kilometres over a planet (a fifth of its radius, and a clear
    /// sky to 15), black above.
    fn sun(level: &NamespacedId) -> Option<Sun> {
        let in_space = *level == mini_cosmos_id();
        (in_space || *level == overworld_id()).then_some(Sun {
            day_seconds: 1_200,
            start_hour: 6.5,
            noon: [0.0, 0.94, 0.342],
            axis: [0.0, -0.342, 0.94],
            light: 255,
            sky_light: 110,
            // A place the sun does not reach has about a quarter of the sky's light: shade is dim, not black.
            sky_light_in_shadow: 70,
            // A place with a hill or a roof over it has none of the sky's light: a cave is dark by day.
            sky_light_under_cover: 0,
            // Starlight: a night is dim where the sky is open, and still black in a cave.
            night_light: 40,
            flat_ground: !in_space,
            sky: Sky {
                color: [0.55, 0.72, 0.92],
                thickness: 30_000.0,
            },
        })
    }

    fn title() -> &'static str {
        "Voxunami Cave"
    }

    fn command() -> &'static str {
        "cave"
    }

    fn summary() -> &'static str {
        "walk or fly over planets, and break and place blocks."
    }

    fn controls() -> &'static str {
        "Left click breaks the block you look at; right click places the selected block;\n1 to 6 select stone, dirt, grass, lamp, wood, fire (which burns grass and wood); R cycles whole block, slabs and stairs.
W A S D walk, Space jumps, Ctrl runs; F flies or walks (then Space and Shift go up and down).
Commands: /level shows which level you are in, /level list lists them, /level go NAME moves you; /fly toggles flying; /tp lists the places, /tp PLACE goes to one (a planet, or the great river), /tp X Y Z teleports; /light classic or /light multiscale chooses the light.
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
