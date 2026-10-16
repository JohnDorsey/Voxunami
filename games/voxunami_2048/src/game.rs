//! Voxunami 2048 as the shell runs it: what the player's clicks do.

use game_shell::session::Session;
use game_shell::{CameraStart, GameDefinition, KeyCode};
use namespaced_id::NamespacedId;
use server::GameLogic;
use transport::Connection;
use voxel_space::{FaceDirection, VoxelCoordinate};
use world_storage::WorldData;

use crate::content::{Content2048, game_id};
use crate::logic::{Logic2048, REACH};
use crate::messages::{
    Progress, PushRequest, decode_progress, encode_push, progress_kind, push_kind,
};
use crate::numbered_block::NumberedBlock;
use crate::tool::Tool;

/// What the 2048 client remembers: how the game has gone, as the server last said, and the tool
/// the player has chosen.
#[derive(Debug, Default)]
pub struct ClientState2048 {
    pub progress: Progress,
    pub tool: Tool,
}

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

/// Asks the server to push the block under the crosshair away from the player, with the chosen tool.
pub fn push_with_tool<C: Connection>(session: &mut Session2048<C>) {
    if let Some(hit) = session.target()
        && let Some(direction) = direction_between(hit.previous, hit.voxel)
    {
        request_push(session, hit.voxel, direction);
    }
}

/// Asks the server to pull the block under the crosshair toward the player, with the chosen tool.
pub fn pull_with_tool<C: Connection>(session: &mut Session2048<C>) {
    if let Some(hit) = session.target()
        && let Some(direction) = direction_between(hit.voxel, hit.previous)
    {
        request_push(session, hit.voxel, direction);
    }
}

fn request_push<C: Connection>(
    session: &mut Session2048<C>,
    voxel: VoxelCoordinate,
    direction: FaceDirection,
) {
    let tool = session.state.tool;
    session.send_game_message(
        push_kind(),
        encode_push(&PushRequest {
            voxel,
            direction,
            tool,
        }),
    );
}

/// What a block is, in words: "spring ice 8".
fn describe(block: NumberedBlock) -> String {
    format!(
        "{} {}",
        block.material.name().replace('_', " "),
        block.number()
    )
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
        "push numbered blocks around an endless landscape and merge equal ones."
    }

    fn controls() -> &'static str {
        "Left click pushes the block you look at away from you; right click pulls it toward you.\nStone moves one step and pushes the row of blocks in front of it (up to 12 blocks, into empty space); ice coasts until it meets another block; spring ice bounces back off blocks it cannot merge with.\n1, 2, 3 choose the tool: double merges equal numbers into their double, add adds any two numbers, subtract takes the smaller from the larger (equal numbers cancel).\nThe title shows the tool and the block you look at."
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

    fn new_logic(content: &Content2048, world: &WorldData) -> Box<dyn GameLogic> {
        Box::new(Logic2048::new(content, world))
    }

    fn handle_game_message(state: &mut ClientState2048, kind: &NamespacedId, payload: &[u8]) {
        if *kind == progress_kind()
            && let Ok(progress) = decode_progress(payload)
        {
            state.progress = progress;
        }
    }

    fn key_pressed<C: Connection>(session: &mut Session2048<C>, key: KeyCode) {
        match key {
            KeyCode::Digit1 => session.state.tool = Tool::Double,
            KeyCode::Digit2 => session.state.tool = Tool::Add,
            KeyCode::Digit3 => session.state.tool = Tool::Subtract,
            _ => {}
        }
    }

    fn primary_action<C: Connection>(session: &mut Session2048<C>) {
        push_with_tool(session);
    }

    fn secondary_action<C: Connection>(session: &mut Session2048<C>) {
        pull_with_tool(session);
    }

    fn status_text<C: Connection>(session: &Session2048<C>) -> String {
        let progress = &session.state.progress;
        let largest = match progress.largest_merge {
            0 => "-".to_owned(),
            number => number.to_string(),
        };
        let looking_at = session
            .target()
            .and_then(|hit| {
                let block = session.mesher().material_at(hit.voxel)?;
                NumberedBlock::from_voxel(block, session.mesher().state_at(hit.voxel)?)
            })
            .map_or_else(|| "nothing".to_owned(), describe);
        format!(
            "tool {} | score {} | pushes {} | largest merge {largest} | looking at {looking_at}",
            session.state.tool.name(),
            progress.score,
            progress.pushes
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material::Material;

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

    #[test]
    fn blocks_are_described_by_material_and_number() {
        assert_eq!(describe(NumberedBlock::new(Material::Stone, 2)), "stone 2");
        assert_eq!(
            describe(NumberedBlock::new(Material::SpringIce, 32)),
            "spring ice 32"
        );
        assert_eq!(
            describe(NumberedBlock::new(Material::Ice, 3_000_000_001)),
            "ice 3000000001"
        );
    }
}
