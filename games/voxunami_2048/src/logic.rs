//! What the 2048 server does: pushes blocks for the players, moves them every tick, merges equal
//! ones that meet, keeps score, and tells everyone how the game stands.
//!
//! The blocks are only in the world's voxels: the server never keeps a second copy. What the
//! voxels do not show (the score, and which blocks are in motion) is saved with the world through
//! [`GameLogic::save_sections`].
//!
//! A player pushes blocks of the level they are in, and blocks move within their level.

use game_shell::PoseBook;
use namespaced_id::NamespacedId;
use protocol::{ClientId, Tick};
use server::{Audience, GameLogic, ServerWorld};
use voxel_grid::ChannelIndex;
use voxel_space::{LevelIndex, VoxelCoordinate};
use world_storage::{Section, WorldData};

use crate::content::{Content2048, landscape_id};
use crate::messages::{
    Progress, PushRequest, SavedMover, SavedState, decode_push, encode_progress, progress_kind,
    push_kind, read_saved_state, saved_state_section,
};
use crate::motion::{BlockField, Mover, Movers, Occupant};
use crate::numbered_block::{AIR, NumberedBlock};

/// How far from a player, in voxels, a block can be pushed.
pub const REACH: f64 = 12.0;

/// The server's world as moving blocks see it. Terrain this server cannot generate, and blocks
/// this program does not know, are obstacles.
struct WorldField<'a> {
    world: &'a mut ServerWorld,
    block_channel: ChannelIndex,
    number_channel: ChannelIndex,
}

impl BlockField for WorldField<'_> {
    fn occupant(&mut self, level: LevelIndex, at: VoxelCoordinate) -> Occupant {
        match self.world.voxel(level, self.block_channel, at) {
            Ok(AIR) => Occupant::Empty,
            Ok(id) => match self.world.voxel(level, self.number_channel, at) {
                Ok(number) => NumberedBlock::from_voxel(id, number)
                    .map_or(Occupant::Obstacle, Occupant::Block),
                Err(_) => Occupant::Obstacle,
            },
            Err(_) => Occupant::Obstacle,
        }
    }

    fn place(&mut self, level: LevelIndex, at: VoxelCoordinate, block: Option<NumberedBlock>) {
        // The voxel was just read, so the level and the terrain there are available. Air has no
        // number, so emptying a voxel resets that too.
        let _ = self.world.set_voxel(
            level,
            self.number_channel,
            at,
            block.map_or(0, NumberedBlock::state),
        );
        let _ = self.world.set_voxel(
            level,
            self.block_channel,
            at,
            block.map_or(AIR, NumberedBlock::block_id),
        );
    }
}

#[derive(Debug)]
pub struct Logic2048 {
    block_channel: ChannelIndex,
    number_channel: ChannelIndex,
    /// The world's levels by identifier, in index order, for naming them in the saved state. Levels
    /// are never added or removed while a server runs.
    level_ids: Vec<NamespacedId>,
    progress: Progress,
    movers: Movers,
    poses: PoseBook,
    progress_changed: bool,
}

impl Logic2048 {
    /// The rules for the game in `world`, continuing from the state saved with it, if any.
    pub fn new(content: &Content2048, world: &WorldData) -> Self {
        let level_ids: Vec<NamespacedId> = world.levels().map(|(_, id, _)| id.clone()).collect();
        let saved = read_saved_state(world.opaque_sections(), &landscape_id()).unwrap_or_default();
        // A block saved in a level this world no longer has is not moving any more.
        let movers = saved.movers.into_iter().filter_map(|saved| {
            Some(Mover {
                level: world.level_index(&saved.level)?,
                position: saved.position,
                direction: saved.direction,
                steps_left: saved.steps_left,
            })
        });
        Self {
            block_channel: content.block_channel(),
            number_channel: content.number_channel(),
            level_ids,
            progress: saved.progress,
            movers: Movers::from_saved(movers),
            poses: PoseBook::new(),
            progress_changed: false,
        }
    }

    fn push(&mut self, client: ClientId, request: PushRequest, world: &mut ServerWorld) {
        let Some(pose) = self.poses.pose_of(client) else {
            return; // a player who has not said where they are
        };
        if pose.distance_to_voxel(request.voxel) > REACH {
            return;
        }
        let level = self.poses.level_of(client);
        let mut field = WorldField {
            world,
            block_channel: self.block_channel,
            number_channel: self.number_channel,
        };
        if self
            .movers
            .push_block(&mut field, level, request.voxel, request.direction)
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

    fn command_received(&mut self, client: ClientId, command: &str, world: &mut ServerWorld) {
        if !self.poses.handle_command(client, command, world) {
            world.reply(client, "unknown command; try /level");
        }
    }

    fn tick(&mut self, _tick: Tick, world: &mut ServerWorld) {
        self.poses.tell_everyone(world);
        let mut field = WorldField {
            world,
            block_channel: self.block_channel,
            number_channel: self.number_channel,
        };
        for merged in self.movers.advance(&mut field) {
            self.progress.score = self
                .progress
                .score
                .saturating_add(u64::from(merged.number()));
            self.progress.largest_merge = self.progress.largest_merge.max(merged.number());
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
        let movers = self
            .movers
            .as_slice()
            .iter()
            .filter_map(|mover| {
                Some(SavedMover {
                    level: self.level_ids.get(mover.level.get() as usize)?.clone(),
                    position: mover.position,
                    direction: mover.direction,
                    steps_left: mover.steps_left,
                })
            })
            .collect();
        vec![saved_state_section(&SavedState {
            progress: self.progress,
            movers,
        })]
    }
}
