//! Players changing blocks: the reach, the states a block has, and whether anyone objects.

use std::collections::HashMap;

use block_models::BlockTable;
use game_shell::GameContent;
use namespaced_id::NamespacedId;
use player_presence::{SharedPresence, lock};
use plugin_host::{Event, Plugin, Registrar, Verdict};
use protocol::ClientId;
use server::GridView;
use voxel_grid::ChannelIndex;
use voxel_space::{GridId, VoxelCoordinate};

use crate::content::CaveContent;
use crate::logic::REACH;
use crate::messages::SetBlock;

/// A player asks to put a block (in a state) at a voxel, within reach and of a block and state that
/// exist. Handlers of this event may refuse, with a reason that the player is told.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockEditRequested {
    pub client: ClientId,
    pub grid: GridId,
    pub voxel: VoxelCoordinate,
    pub block: u32,
    pub state: u32,
}

impl Event for BlockEditRequested {
    fn id() -> NamespacedId {
        "voxunami_cave:block_edit_requested"
            .parse()
            .expect("constant identifier is valid")
    }
}

/// A player has broken a block (put air where one was). Sent after the change; `block` is what was there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockBroken {
    pub client: ClientId,
    pub grid: GridId,
    pub voxel: VoxelCoordinate,
    pub block: u32,
}

impl Event for BlockBroken {
    fn id() -> NamespacedId {
        "voxunami_cave:block_broken"
            .parse()
            .expect("constant identifier is valid")
    }
}

/// A player has put a block down. Sent after the change; `block` is the block that was put.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockPlaced {
    pub client: ClientId,
    pub grid: GridId,
    pub voxel: VoxelCoordinate,
    pub block: u32,
}

impl Event for BlockPlaced {
    fn id() -> NamespacedId {
        "voxunami_cave:block_placed"
            .parse()
            .expect("constant identifier is valid")
    }
}

/// Lets players change the blocks within reach of them. What the change does to light, fire and
/// bodies is theirs: they hear of it as a change to the block channels.
#[derive(Debug)]
pub struct BlockEditingPlugin {
    block_ids: HashMap<NamespacedId, u32>,
    air: u32,
    table: BlockTable,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    players: SharedPresence,
}

impl BlockEditingPlugin {
    pub fn new(content: &CaveContent, players: SharedPresence) -> Self {
        let registries = content.registries();
        let blocks = registries[0].1;
        let block_ids = (0..blocks.entry_count() as u32)
            .filter_map(|raw| Some((blocks.identifier_of_raw(raw)?.clone(), raw)))
            .collect();
        Self {
            block_ids,
            air: content.blocks.air,
            table: content.block_table(),
            block_channel: content.block_channel(),
            state_channel: content.state_channel(),
            players,
        }
    }
}

impl Plugin for BlockEditingPlugin {
    fn id(&self) -> NamespacedId {
        "voxunami_cave:block_editing"
            .parse()
            .expect("constant identifier is valid")
    }

    fn depends_on(&self) -> Vec<NamespacedId> {
        vec![
            "player_presence:players"
                .parse()
                .expect("constant identifier is valid"),
        ]
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        registrar.event::<BlockEditRequested>();
        registrar.event::<BlockBroken>();
        registrar.event::<BlockPlaced>();
        let air = self.air;
        let block_ids = self.block_ids.clone();
        let table = self.table.clone();
        let (block_channel, state_channel) = (self.block_channel, self.state_channel);
        let players = std::sync::Arc::clone(&self.players);
        registrar.message(move |context, client, request: SetBlock| {
            let (pose, level) = {
                let presence = lock(&players);
                let world = context.world().data();
                (
                    presence.pose_of(world, client),
                    presence.level_of(world, client),
                )
            };
            let (Some(pose), Some(&raw)) = (pose, block_ids.get(&request.block)) else {
                return; // a player who has not said where they are, or a block this server lacks
            };
            // Players change the terrain of the level they are in, or a ship in it, within reach of where they are.
            let grid = request.grid;
            let Some(distance) = context
                .world()
                .data()
                .grid_frame(grid)
                .and_then(|frame| pose.distance_to_voxel_of(level, &frame, request.at))
            else {
                return; // a grid that is not there, or is in another level
            };
            // Air has one state, so breaking a block always resets its state too.
            if distance > REACH || request.state >= table.state_count(raw) {
                return;
            }
            let asked = BlockEditRequested {
                client,
                grid,
                voxel: request.at,
                block: raw,
                state: request.state,
            };
            if let Verdict::Refuse(reason) = context.ask(&asked) {
                context.reply(client, &reason);
                return;
            }
            // Terrain this server cannot generate cannot be changed; the request is dropped.
            let Ok(mut edited) = context.grid(grid) else {
                return;
            };
            let before = edited.get(block_channel, request.at).ok();
            let changed = edited.set(state_channel, request.at, request.state).is_ok()
                && edited.set(block_channel, request.at, raw).is_ok();
            if !changed {
                return;
            }
            // Those who care about breaking and placing hear of it once it has happened.
            if raw == air {
                if let Some(block) = before.filter(|block| *block != air) {
                    context.emit(BlockBroken {
                        client,
                        grid,
                        voxel: request.at,
                        block,
                    });
                }
            } else {
                context.emit(BlockPlaced {
                    client,
                    grid,
                    voxel: request.at,
                    block: raw,
                });
            }
        });
    }
}
