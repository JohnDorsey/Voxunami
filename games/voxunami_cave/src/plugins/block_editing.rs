//! Players changing blocks: the reach, the states a block has, and whether anyone objects.

use std::collections::HashMap;

use block_models::BlockTable;
use game_shell::GameContent;
use namespaced_id::NamespacedId;
use player_presence::{SharedPoseBook, lock};
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

/// Lets players change the blocks within reach of them. What the change does to light, fire and
/// bodies is theirs: they hear of it as a change to the block channels.
#[derive(Debug)]
pub struct BlockEditingPlugin {
    block_ids: HashMap<NamespacedId, u32>,
    table: BlockTable,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    players: SharedPoseBook,
}

impl BlockEditingPlugin {
    pub fn new(content: &CaveContent, players: SharedPoseBook) -> Self {
        let registries = content.registries();
        let blocks = registries[0].1;
        let block_ids = (0..blocks.entry_count() as u32)
            .filter_map(|raw| Some((blocks.identifier_of_raw(raw)?.clone(), raw)))
            .collect();
        Self {
            block_ids,
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
        let block_ids = self.block_ids.clone();
        let table = self.table.clone();
        let (block_channel, state_channel) = (self.block_channel, self.state_channel);
        let players = std::sync::Arc::clone(&self.players);
        registrar.message(move |context, client, request: SetBlock| {
            let (pose, level) = {
                let book = lock(&players);
                (book.pose_of(client).copied(), book.level_of(client))
            };
            let (Some(pose), Some(&raw)) = (pose, block_ids.get(&request.block)) else {
                return; // a player who has not said where they are, or a block this server lacks
            };
            // Air has one state, so breaking a block always resets its state too.
            if pose.distance_to_voxel(request.at) > REACH || request.state >= table.state_count(raw)
            {
                return;
            }
            // Players change the level they are in.
            let grid = GridId::root(level);
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
            if edited.set(state_channel, request.at, request.state).is_ok() {
                let _ = edited.set(block_channel, request.at, raw);
            }
        });
    }
}
