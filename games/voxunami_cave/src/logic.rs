//! What the Voxunami Cave server does: lets players move and change blocks, and tells everyone
//! where everyone is.

use std::collections::{BTreeMap, HashMap};

use namespaced_id::NamespacedId;
use protocol::{ClientId, Tick};
use server::{Audience, GameLogic, ServerWorld};
use voxel_grid::ChannelIndex;
use voxel_space::{VoxelCoordinate, WorldPosition};

use crate::content::CaveContent;
use crate::messages::{
    PlayerPose, decode_pose, decode_set_block, encode_players, players_kind, pose_kind,
    set_block_kind,
};

/// How far from a player, in voxels, a block can be changed.
pub const REACH: f64 = 10.0;

#[derive(Debug)]
pub struct CaveLogic {
    block_ids: HashMap<NamespacedId, u32>,
    block_channel: ChannelIndex,
    poses: BTreeMap<ClientId, PlayerPose>,
    poses_changed: bool,
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
            poses: BTreeMap::new(),
            poses_changed: false,
        }
    }

    fn set_block(
        &mut self,
        client: ClientId,
        at: VoxelCoordinate,
        block: &NamespacedId,
        world: &mut ServerWorld,
    ) {
        let (Some(pose), Some(&raw)) = (self.poses.get(&client), self.block_ids.get(block)) else {
            return; // a player who has not said where they are, or a block this server lacks
        };
        if distance_to_voxel(pose.position, at) > REACH {
            return;
        }
        // Terrain this server cannot generate cannot be changed; the request is dropped.
        let _ = world.set_voxel(self.block_channel, at, raw);
    }
}

/// The distance in voxels from a position to the centre of a voxel.
fn distance_to_voxel(position: WorldPosition, voxel: VoxelCoordinate) -> f64 {
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
        if self.poses.remove(&client).is_some() {
            self.poses_changed = true;
        }
    }

    fn message_received(
        &mut self,
        client: ClientId,
        kind: &NamespacedId,
        payload: &[u8],
        world: &mut ServerWorld,
    ) {
        if *kind == pose_kind() {
            if let Ok(pose) = decode_pose(payload)
                && self.poses.insert(client, pose) != Some(pose)
            {
                self.poses_changed = true;
            }
        } else if *kind == set_block_kind()
            && let Ok(request) = decode_set_block(payload)
        {
            self.set_block(client, request.at, &request.block, world);
        }
        // Anything else is another game's, or malformed: ignored.
    }

    fn tick(&mut self, _tick: Tick, world: &mut ServerWorld) {
        if self.poses_changed {
            let players: Vec<_> = self.poses.iter().map(|(&c, &p)| (c, p)).collect();
            world.send(Audience::Everyone, players_kind(), encode_players(&players));
            self.poses_changed = false;
        }
    }
}
