//! Voxunami 2048 as the shell runs it: what the player's clicks and keys do.

use game_shell::session::Session;
use game_shell::{CameraStart, GameDefinition, KeyCode};
use namespaced_id::NamespacedId;
use server::GameLogic;
use transport::Connection;
use world_storage::WorldData;

use crate::content::{Content2048, game_id};
use crate::layout::push_direction_for_plate;
use crate::logic::Logic2048;
use crate::messages::{Status, decode_status, encode_push, new_game_kind, push_kind, status_kind};

/// What the 2048 client remembers: how the game stands, as the server last said.
#[derive(Debug, Default)]
pub struct ClientState2048 {
    pub status: Status,
}

#[derive(Debug)]
pub struct Game2048;

pub type Session2048<C> = Session<C, Game2048>;

/// Pushes the tiles toward the side opposite the plate under the crosshair, if one is.
pub fn push_with_tool<C: Connection>(session: &mut Session2048<C>) {
    let Some(hit) = session.target() else {
        return;
    };
    if let Some(toward) = push_direction_for_plate(hit.voxel) {
        session.send_game_message(push_kind(), encode_push(toward));
    }
}

pub fn start_new_game<C: Connection>(session: &mut Session2048<C>) {
    session.send_game_message(new_game_kind(), Vec::new());
}

impl GameDefinition for Game2048 {
    type Content = Content2048;
    type ClientState = ClientState2048;

    fn id() -> NamespacedId {
        game_id()
    }

    fn title() -> &'static str {
        "Voxunami 2048"
    }

    fn command() -> &'static str {
        "2048"
    }

    fn summary() -> &'static str {
        "3D 2048: push the whole board of tiles and merge equal ones."
    }

    fn controls() -> &'static str {
        "Left click an orange push plate to push every tile toward the opposite side; Enter starts a new game.\nTiles of equal colour merge into the next colour; the title shows the largest tile."
    }

    fn default_seed() -> u64 {
        2048
    }

    fn default_camera() -> CameraStart {
        // Above and to the side of the board, looking at its middle.
        CameraStart {
            position: [40.0, 30.0, 45.0],
            yaw_degrees: 40.0,
            pitch_degrees: -22.0,
        }
    }

    fn reach() -> f32 {
        120.0
    }

    fn new_content() -> Content2048 {
        Content2048::new()
    }

    fn new_logic(content: &Content2048, world: &WorldData) -> Box<dyn GameLogic> {
        Box::new(Logic2048::new(content, world))
    }

    fn handle_game_message(state: &mut ClientState2048, kind: &NamespacedId, payload: &[u8]) {
        if *kind == status_kind()
            && let Ok(status) = decode_status(payload)
        {
            state.status = status;
        }
    }

    fn primary_action<C: Connection>(session: &mut Session2048<C>) {
        push_with_tool(session);
    }

    fn key_pressed<C: Connection>(session: &mut Session2048<C>, key: KeyCode) {
        if key == KeyCode::Enter {
            start_new_game(session);
        }
    }

    fn status_text<C: Connection>(session: &Session2048<C>) -> String {
        let status = &session.state.status;
        let largest = if status.highest == 0 {
            "-".to_owned()
        } else {
            (1u64 << status.highest).to_string()
        };
        let ending = if status.game_over {
            " | GAME OVER (Enter for a new game)"
        } else if status.won() {
            " | 2048!"
        } else {
            ""
        };
        format!(
            "score {} | moves {} | largest tile {largest}{ending}",
            status.score, status.moves
        )
    }
}
