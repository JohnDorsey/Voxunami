//! What the 2048 server does: pushes blocks for the players, moves them every tick, merges equal
//! ones that meet, keeps score, and tells everyone how the game stands.
//!
//! The blocks are only in the world's voxels: the server never keeps a second copy. What the
//! voxels do not show (the score, and which blocks are in motion) is saved with the world as a plugin's
//! saved state.
//!
//! A player pushes blocks of the level they are in, and blocks move within their level.

use std::sync::{Arc, Mutex, MutexGuard};

use namespaced_id::NamespacedId;
use player_presence::{Presence, SharedPresence};
use plugin_host::{HandlerSpec, Phase, PlayerJoined, Plugin, Registrar, SaveScope, SystemSpec};
use protocol::{ClientId, Message};
use server::{Audience, GridView, ServerWorld};
use voxel_grid::ChannelIndex;
use voxel_space::{GridId, VoxelCoordinate};
use world_storage::{Section, WorldData};

use crate::content::Content2048;
use crate::messages::{
    Progress, PushRequest, SavedMover, SavedState, read_saved_state, saved_state_section,
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
    fn occupant(&mut self, grid: GridId, at: VoxelCoordinate) -> Occupant {
        let Ok(mut view) = self.world.grid(grid) else {
            return Occupant::Obstacle;
        };
        match view.get(self.block_channel, at) {
            Ok(AIR) => Occupant::Empty,
            Ok(id) => match view.get(self.number_channel, at) {
                Ok(number) => NumberedBlock::from_voxel(id, number)
                    .map_or(Occupant::Obstacle, Occupant::Block),
                Err(_) => Occupant::Obstacle,
            },
            Err(_) => Occupant::Obstacle,
        }
    }

    fn place(&mut self, grid: GridId, at: VoxelCoordinate, block: Option<NumberedBlock>) {
        // The voxel was just read, so the grid and the voxel are there. Air has no number, so emptying a voxel resets that too.
        let Ok(mut view) = self.world.grid(grid) else {
            return;
        };
        let _ = view.set(
            self.number_channel,
            at,
            block.map_or(0, NumberedBlock::state),
        );
        let _ = view.set(
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
    progress_changed: bool,
}

impl Logic2048 {
    /// The rules for the game in `world`, continuing from the state saved with it, if any.
    fn new(block_channel: ChannelIndex, number_channel: ChannelIndex, world: &WorldData) -> Self {
        let level_ids: Vec<NamespacedId> = world.levels().map(|(_, id, _)| id.clone()).collect();
        let saved = read_saved_state(world.opaque_sections()).unwrap_or_default();
        // A block saved in a level this world no longer has is not moving any more.
        let movers = saved.movers.into_iter().filter_map(|saved| {
            Some(Mover {
                grid: GridId::root(world.level_index(&saved.level)?),
                position: saved.position,
                direction: saved.direction,
                tool: saved.tool,
                steps_left: saved.steps_left,
            })
        });
        Self {
            block_channel,
            number_channel,
            level_ids,
            progress: saved.progress,
            movers: Movers::from_saved(movers),
            progress_changed: false,
        }
    }

    fn push(
        &mut self,
        client: ClientId,
        request: PushRequest,
        world: &mut ServerWorld,
        presence: &Presence,
    ) {
        let Some(pose) = presence.pose_of(world.data(), client) else {
            return; // a player who has not said where they are
        };
        if pose.distance_to_voxel(request.voxel) > REACH {
            return;
        }
        // The terrain of the level the player is in (the request names a voxel, not a grid).
        let grid = GridId::root(presence.level_of(world.data(), client));
        let mut field = WorldField {
            world,
            block_channel: self.block_channel,
            number_channel: self.number_channel,
        };
        if self.movers.push_block(
            &mut field,
            grid,
            request.voxel,
            request.direction,
            request.tool,
        ) {
            self.progress.pushes += 1;
            self.progress_changed = true;
        }
    }
}

impl Logic2048 {
    /// Tells a client who has just joined how the game stands.
    fn welcome(&self, client: ClientId, world: &mut ServerWorld) {
        world.send(
            Audience::Client(client),
            Progress::kind(),
            self.progress.encode(),
        );
    }

    /// One tick of the blocks in motion: moves them, merges equal ones that meet, keeps score, and
    /// tells everyone if the score changed.
    fn advance(&mut self, world: &mut ServerWorld) {
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
            world.send(Audience::Everyone, Progress::kind(), self.progress.encode());
            self.progress_changed = false;
        }
    }

    fn saved_sections(&self) -> Vec<Section> {
        let movers = self
            .movers
            .as_slice()
            .iter()
            .filter_map(|mover| {
                Some(SavedMover {
                    // (A block that is moving in a ship is not saved: the ship's file does not hold it.)
                    level: self
                        .level_ids
                        .get(mover.grid.root_level()?.get() as usize)?
                        .clone(),
                    position: mover.position,
                    direction: mover.direction,
                    tool: mover.tool,
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

/// The rules of 2048 as a plugin: the push message, the blocks in motion, the score, and the state
/// saved with the world. It uses the record of where players are (to know how far one is from a block
/// and which level they are in), which the game gives it.
#[derive(Debug)]
pub struct Plugin2048 {
    block_channel: ChannelIndex,
    number_channel: ChannelIndex,
    players: SharedPresence,
}

impl Plugin2048 {
    pub fn new(content: &Content2048, players: SharedPresence) -> Self {
        Self {
            block_channel: content.block_channel(),
            number_channel: content.number_channel(),
            players,
        }
    }
}

impl Plugin for Plugin2048 {
    fn id(&self) -> NamespacedId {
        "voxunami_2048:rules"
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
        let state = Arc::new(Mutex::new(Logic2048::new(
            self.block_channel,
            self.number_channel,
            registrar.world(),
        )));
        let rules = Arc::clone(&state);
        let players = Arc::clone(&self.players);
        registrar.message(move |context, client, request: PushRequest| {
            let presence = player_presence::lock(&players);
            lock_rules(&rules).push(client, request, context.world(), &presence);
        });
        registrar.declares_message::<Progress>();
        let rules = Arc::clone(&state);
        registrar.on::<PlayerJoined>(
            HandlerSpec::new("voxunami_2048:tell_the_newcomer_the_score"),
            move |event, context| lock_rules(&rules).welcome(event.client, context.world()),
        );
        let rules = Arc::clone(&state);
        registrar.system(
            SystemSpec::new("voxunami_2048:motion", Phase::Rules),
            move |context| lock_rules(&rules).advance(context.world()),
        );
        let rules = Arc::clone(&state);
        registrar.save_section(SaveScope::Global, move || {
            lock_rules(&rules).saved_sections()
        });
    }
}

fn lock_rules(rules: &Mutex<Logic2048>) -> MutexGuard<'_, Logic2048> {
    rules
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
