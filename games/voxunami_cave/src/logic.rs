//! What the Voxunami Cave server does: lets players change blocks within reach, and tells everyone
//! where everyone is.

use std::collections::HashMap;

use game_shell::{PlayerPose, PoseBook};
use namespaced_id::NamespacedId;
use protocol::{ClientId, Tick};
use server::{GameLogic, ServerWorld};
use voxel_grid::ChannelIndex;
use voxel_space::{VoxelCoordinate, WorldPosition};

use crate::content::CaveContent;
use crate::messages::{decode_set_block, set_block_kind};

/// How far from a player, in voxels, a block can be changed.
pub const REACH: f64 = 10.0;

#[derive(Debug)]
pub struct CaveLogic {
    block_ids: HashMap<NamespacedId, u32>,
    block_channel: ChannelIndex,
    poses: PoseBook,
}

impl CaveLogic {
    pub fn new(content: &CaveContent) -> Self {
        let block_ids = content
            .blocks
            .blocks
            .iter()
            .map(|(id, entry)| (entry.identifier().clone(), id.raw()))
            .collect();
        Self {
            block_ids,
            block_channel: content.block_channel(),
            poses: PoseBook::new(),
        }
    }

    fn set_block(
        &mut self,
        client: ClientId,
        at: VoxelCoordinate,
        block: &NamespacedId,
        world: &mut ServerWorld,
    ) {
        let (Some(pose), Some(&raw)) = (self.poses.pose_of(client), self.block_ids.get(block))
        else {
            return; // a player who has not said where they are, or a block this server lacks
        };
        if distance_to_voxel(pose, at) > REACH {
            return;
        }
        // Terrain this server cannot generate cannot be changed; the request is dropped.
        let _ = world.set_voxel(self.block_channel, at, raw);
    }
}

/// The distance in voxels from a player to the centre of a voxel.
fn distance_to_voxel(pose: &PlayerPose, voxel: VoxelCoordinate) -> f64 {
    let position: WorldPosition = pose.position;
    let brick = position.brick();
    let offset = position.offset();
    let along =
        |voxel: i64, brick: i64, offset: f32| (voxel - brick * 32) as f64 + 0.5 - f64::from(offset);
    let (dx, dy, dz) = (
        along(voxel.x(), brick.x(), offset[0]),
        along(voxel.y(), brick.y(), offset[1]),
        along(voxel.z(), brick.z(), offset[2]),
    );
    (dx * dx + dy * dy + dz * dz).sqrt()
}

impl GameLogic for CaveLogic {
    fn client_joined(&mut self, _client: ClientId, _world: &mut ServerWorld) {}

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
        if *kind == set_block_kind()
            && let Ok(request) = decode_set_block(payload)
        {
            self.set_block(client, request.at, &request.block, world);
        }
        // Anything else is another game's, or malformed: ignored.
    }

    fn tick(&mut self, _tick: Tick, world: &mut ServerWorld) {
        self.poses.tell_everyone(world);
    }
}
