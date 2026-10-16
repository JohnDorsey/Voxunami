//! What the 2048 server does: pushes blocks for the players, moves them every tick, merges equal
//! ones that meet, keeps score, and tells everyone how the game stands.
//!
//! The blocks are only in the world's voxels: the server never keeps a second copy. What the
//! voxels do not show (the score, and which blocks are in motion) is saved with the world through
//! [`GameLogic::save_sections`].

use game_shell::PoseBook;
use namespaced_id::NamespacedId;
use protocol::{ClientId, Tick};
use server::{Audience, GameLogic, ServerWorld};
use voxel_grid::ChannelIndex;
use voxel_space::VoxelCoordinate;
use world_storage::{Section, WorldData};

use crate::content::Content2048;
use crate::messages::{
    Progress, SavedState, decode_push, encode_progress, progress_kind, push_kind, read_saved_state,
    saved_state_section,
};
use crate::motion::{BlockField, Movers, Occupant};
use crate::numbered_block::{AIR, NumberedBlock};

/// How far from a player, in voxels, a block can be pushed.
pub const REACH: f64 = 12.0;

/// The server's world as moving blocks see it. Terrain this server cannot generate, and blocks
/// this program does not know, are obstacles.
struct WorldField<'a> {
    world: &'a mut ServerWorld,
    channel: ChannelIndex,
}

impl BlockField for WorldField<'_> {
    fn occupant(&mut self, at: VoxelCoordinate) -> Occupant {
        match self.world.voxel(self.channel, at) {
            Ok(AIR) => Occupant::Empty,
            Ok(id) => NumberedBlock::from_id(id).map_or(Occupant::Obstacle, Occupant::Block),
            Err(_) => Occupant::Obstacle,
        }
    }

    fn place(&mut self, at: VoxelCoordinate, block: Option<NumberedBlock>) {
        // The voxel was just read, so the terrain there is available.
        let _ = self
            .world
            .set_voxel(self.channel, at, block.map_or(AIR, NumberedBlock::id));
    }
}

#[derive(Debug)]
pub struct Logic2048 {
    channel: ChannelIndex,
    progress: Progress,
    movers: Movers,
    poses: PoseBook,
    progress_changed: bool,
}

impl Logic2048 {
    /// The rules for the game in `world`, continuing from the state saved with it, if any.
    pub fn new(content: &Content2048, world: &WorldData) -> Self {
        let saved = read_saved_state(world.opaque_sections()).unwrap_or_default();
        Self {
            channel: content.block_channel(),
            progress: saved.progress,
            movers: Movers::from_saved(saved.movers),
            poses: PoseBook::new(),
            progress_changed: false,
        }
    }

    fn push(
        &mut self,
        client: ClientId,
        request: crate::messages::PushRequest,
        world: &mut ServerWorld,
    ) {
        let Some(pose) = self.poses.pose_of(client) else {
            return; // a player who has not said where they are
        };
        if pose.distance_to_voxel(request.voxel) > REACH {
            return;
        }
        let mut field = WorldField {
            world,
            channel: self.channel,
        };
        if self
            .movers
            .push_block(&mut field, request.voxel, request.direction)
        {
            self.progress.pushes += 1;
            self.progress_changed = true;
        }
    }
}

impl GameLogic for Logic2048 {
    fn client_joined(&mut self, client: ClientId, world: &mut ServerWorld) {
        world.send(
            Audience::Client(client),
            progress_kind(),
            encode_progress(&self.progress),
        );
    }

    fn client_left(&mut self, client: ClientId, _world: &mut ServerWorld) {
        self.poses.client_left(client);
    }

    fn message_received(
        &mut self,
        client: ClientId,
        kind: &NamespacedId,
        payload: &[u8],
        world: &mut ServerWorld,
    ) {
        if self.poses.handle_message(client, kind, payload) {
            return;
        }
        if *kind == push_kind()
            && let Ok(request) = decode_push(payload)
        {
            self.push(client, request, world);
        }
        // Anything else is another game's, or malformed: ignored.
    }

    fn tick(&mut self, _tick: Tick, world: &mut ServerWorld) {
        self.poses.tell_everyone(world);
        let mut field = WorldField {
            world,
            channel: self.channel,
        };
        for merged in self.movers.advance(&mut field) {
            self.progress.score += merged.value();
            self.progress.largest_merge = self.progress.largest_merge.max(merged.exponent());
            self.progress_changed = true;
        }
        if self.progress_changed {
            world.send(
                Audience::Everyone,
                progress_kind(),
                encode_progress(&self.progress),
            );
            self.progress_changed = false;
        }
    }

    fn save_sections(&self) -> Vec<Section> {
        vec![saved_state_section(&SavedState {
            progress: self.progress,
            movers: self.movers.as_slice().to_vec(),
        })]
    }
}
