//! Voxunami 2048 as the shell runs it: what the player's clicks do.

use game_shell::client_plugin::ClientPlugin;
use game_shell::session::Session;
use game_shell::{CameraStart, GameDefinition};
use namespaced_id::NamespacedId;
use player_presence::PlayersPlugin;
use plugin_host::Plugin;
use transport::Connection;
use voxel_space::{FaceDirection, VoxelCoordinate};
use world_view::RayHit;

use crate::client::Client2048;
use crate::content::{Content2048, game_id};
use crate::logic::{Plugin2048, REACH};
use crate::messages::{Progress, PushRequest};
use crate::tool::Tool;
use protocol::Message;

#[derive(Debug)]
pub struct Game2048;

pub type Session2048<C> = Session<C, Game2048>;

/// The direction from one voxel to a voxel next to it, or `None` if they are not neighbours.
pub fn direction_between(from: VoxelCoordinate, to: VoxelCoordinate) -> Option<FaceDirection> {
    let step = [to.x() - from.x(), to.y() - from.y(), to.z() - from.z()];
    FaceDirection::ALL
        .into_iter()
        .find(|direction| direction.offset() == step)
}

/// What a click on a block asks of the server: to push it away from the player (`pull` false) or toward
/// them, with this tool. Nothing if there is no block under the crosshair.
pub fn push_request(target: Option<RayHit>, tool: Tool, pull: bool) -> Option<PushRequest> {
    let hit = target?;
    let direction = if pull {
        direction_between(hit.voxel, hit.previous)
    } else {
        direction_between(hit.previous, hit.voxel)
    }?;
    Some(PushRequest {
        voxel: hit.voxel,
        direction,
        tool,
    })
}

/// The tool the player has chosen in this session.
pub fn chosen_tool<C: Connection>(session: &Session2048<C>) -> Tool {
    session
        .client_plugin::<Client2048>()
        .map_or_else(Tool::default, Client2048::tool)
}

/// How the game has gone, as the server last said, in this session.
pub fn progress<C: Connection>(session: &Session2048<C>) -> Progress {
    session
        .client_plugin::<Client2048>()
        .map_or_else(Progress::default, Client2048::progress)
}

fn request_push<C: Connection>(session: &mut Session2048<C>, pull: bool) {
    if let Some(request) = push_request(session.target(), chosen_tool(session), pull) {
        session.send_game_message(PushRequest::kind(), request.encode());
    }
}

/// Asks the server to push the block under the crosshair away from the player, with the chosen tool.
pub fn push_with_tool<C: Connection>(session: &mut Session2048<C>) {
    request_push(session, false);
}

/// Asks the server to pull the block under the crosshair toward the player, with the chosen tool.
pub fn pull_with_tool<C: Connection>(session: &mut Session2048<C>) {
    request_push(session, true);
}

impl GameDefinition for Game2048 {
    type Content = Content2048;

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
        "push numbered blocks around an endless landscape and merge equal ones."
    }

    fn controls() -> &'static str {
        "Left click pushes the block you look at away from you; right click pulls it toward you.\nStone moves one step and pushes the row of blocks in front of it (up to 12 blocks, into empty space); ice coasts until it meets another block; spring ice bounces back off blocks it cannot merge with.\n1, 2, 3 choose the tool: double merges equal numbers into their double, add adds any two numbers, subtract takes the smaller from the larger (equal numbers cancel).\nThe title shows the tool and the block you look at.
W A S D move, Space and Shift go up and down, Ctrl moves five times faster. Commands: /level shows which level you are in, /level list lists them, /level go NAME moves you."
    }

    fn default_seed() -> u64 {
        2048
    }

    fn default_camera() -> CameraStart {
        CameraStart {
            position: [8.0, 30.0, 8.0],
            yaw_degrees: 30.0,
            pitch_degrees: -30.0,
        }
    }

    fn reach() -> f32 {
        REACH as f32
    }

    fn new_content() -> Content2048 {
        Content2048::new()
    }

    fn plugins(content: &Content2048) -> Vec<Box<dyn Plugin>> {
        // This game believes what players say about where they are.
        let players = PlayersPlugin::new(true);
        let rules = Plugin2048::new(content, players.book());
        vec![Box::new(players), Box::new(rules)]
    }

    fn client_plugins(_content: &Content2048) -> Vec<Box<dyn ClientPlugin>> {
        vec![Box::new(Client2048::default())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions_between_neighbours_and_nothing_else() {
        let origin = VoxelCoordinate::new(5, 5, 5);
        for direction in FaceDirection::ALL {
            let [dx, dy, dz] = direction.offset();
            let neighbour = origin.checked_offset(dx, dy, dz).unwrap();
            assert_eq!(direction_between(origin, neighbour), Some(direction));
            assert_eq!(
                direction_between(neighbour, origin),
                Some(direction.opposite())
            );
        }
        assert_eq!(direction_between(origin, origin), None);
        assert_eq!(
            direction_between(origin, VoxelCoordinate::new(7, 5, 5)),
            None
        );
        assert_eq!(
            direction_between(origin, VoxelCoordinate::new(6, 6, 5)),
            None
        );
    }
}
