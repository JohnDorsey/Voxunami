//! What the Eels server does: gives each player an eel and a colour, steers the eels as asked, runs
//! the rounds, and tells everyone where the cameras and the standings are.
//!
//! The eels' bodies are only in the world's voxels. What the voxels do not show (who is swimming,
//! the scores) is the match's, kept in memory: a world that is saved and loaded starts a fresh
//! match, and the first round clears whatever bodies the file held.
//!
//! The server moves every player, so what a player says about where they are is not listened to (the game
//! gives its players plugin that setting).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use namespaced_id::NamespacedId;
use player_presence::{PoseBook, SharedPoseBook};
use plugin_host::{
    HandlerSpec, Phase as TickPhase, PlayerJoined, PlayerLeft, Plugin, Registrar, SystemSpec,
};
use protocol::{ClientId, Message};
use server::{Audience, ServerWorld};
use voxel_grid::ChannelIndex;
use voxel_space::{BrickCoordinate, LevelIndex, LocalVoxelIndex, VoxelCoordinate};

use crate::arena::{ARENA_EDGE, Cell, is_inside};
use crate::content::{ContentEels, blocks};
use crate::eel::Steer;
use crate::messages::{Player, Standings};
use crate::round::{Arena, MAXIMUM_EELS, Match, Paint, Phase, TICKS_PER_SECOND};
use crate::view::{eel_pose, spectator_pose};

/// The server's world as the rules see the arena.
struct WorldArena<'a> {
    world: &'a mut ServerWorld,
    block_channel: ChannelIndex,
    slot_channel: ChannelIndex,
}

fn voxel_of(cell: Cell) -> VoxelCoordinate {
    VoxelCoordinate::new(cell[0], cell[1], cell[2])
}

impl WorldArena<'_> {
    fn set(&mut self, cell: Cell, block: u32, slot: u32) {
        // Read just before, so the level and the terrain there are available.
        let voxel = voxel_of(cell);
        let _ = self
            .world
            .set_voxel(LevelIndex::MAIN, self.slot_channel, voxel, slot);
        let _ = self
            .world
            .set_voxel(LevelIndex::MAIN, self.block_channel, voxel, block);
    }
}

impl Arena for WorldArena<'_> {
    fn is_occupied(&mut self, cell: Cell) -> bool {
        // Anything that cannot be read is solid: nothing swims into it.
        !matches!(
            self.world
                .voxel(LevelIndex::MAIN, self.block_channel, voxel_of(cell)),
            Ok(blocks::AIR)
        )
    }

    fn paint(&mut self, cell: Cell, paint: Paint) {
        match paint {
            Paint::Clear => self.set(cell, blocks::AIR, 0),
            Paint::Trail(slot) => self.set(cell, blocks::TRAIL, u32::from(slot)),
            Paint::Head(slot) => self.set(cell, blocks::HEAD, u32::from(slot)),
        }
    }

    fn clear_inside(&mut self) {
        let bricks = (ARENA_EDGE + 31) / 32;
        for x in 0..bricks {
            for y in 0..bricks {
                for z in 0..bricks {
                    let brick = BrickCoordinate::new(x, y, z);
                    // A brick that is all air, as most are, has nothing to clear.
                    if let Ok(Some(blocks::AIR)) =
                        self.world
                            .uniform_value(LevelIndex::MAIN, self.block_channel, brick)
                    {
                        continue;
                    }
                    for local in LocalVoxelIndex::all() {
                        let voxel = brick.voxel_at(local);
                        let cell = [voxel.x(), voxel.y(), voxel.z()];
                        if is_inside(cell)
                            && !matches!(
                                self.world
                                    .voxel(LevelIndex::MAIN, self.block_channel, voxel),
                                Ok(blocks::AIR)
                            )
                        {
                            self.set(cell, blocks::AIR, 0);
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct LogicEels {
    block_channel: ChannelIndex,
    slot_channel: ChannelIndex,
    game: Match,
    /// The colour each player has, for as long as they are here.
    slots: BTreeMap<ClientId, u8>,
    /// The standings as last sent, so that only changes are.
    announced: Vec<u8>,
}

impl LogicEels {
    /// The rules of the game. There is no saved state: see the module's notes.
    fn new(block_channel: ChannelIndex, slot_channel: ChannelIndex) -> Self {
        Self {
            block_channel,
            slot_channel,
            game: Match::new(),
            slots: BTreeMap::new(),
            announced: Vec::new(),
        }
    }

    /// The lowest colour nobody has.
    fn free_slot(&self) -> Option<u8> {
        (0..MAXIMUM_EELS as u8).find(|slot| !self.slots.values().any(|taken| taken == slot))
    }

    /// Who is playing and how the round stands, as clients are told. The time left is in whole
    /// seconds, so that it is not sent every tick.
    fn standings(&self) -> Standings {
        let seconds = |ticks_left: u32| ticks_left.div_ceil(TICKS_PER_SECOND) * TICKS_PER_SECOND;
        let phase = match self.game.phase() {
            Phase::Countdown { ticks_left } => Phase::Countdown {
                ticks_left: seconds(ticks_left),
            },
            Phase::Over { ticks_left, winner } => Phase::Over {
                ticks_left: seconds(ticks_left),
                winner,
            },
            other => other,
        };
        let players = self
            .slots
            .iter()
            .map(|(&client, &slot)| Player {
                client,
                slot,
                alive: self.game.eel(slot).is_some_and(|eel| eel.is_alive()),
                wins: self.game.wins(slot),
            })
            .collect();
        Standings { phase, players }
    }

    fn announce(&mut self, world: &mut ServerWorld) {
        let bytes = self.standings().encode();
        if bytes != self.announced {
            world.send(Audience::Everyone, Standings::kind(), bytes.clone());
            self.announced = bytes;
        }
    }
}

impl LogicEels {
    /// A client joined: an eel and a colour if one is free, and the standings, at once.
    fn joined(&mut self, client: ClientId, world: &mut ServerWorld, poses: &mut PoseBook) {
        if let Some(slot) = self.free_slot() {
            self.slots.insert(client, slot);
            self.game.join(slot);
        }
        poses.set_pose(client, spectator_pose());
        // A newcomer is told how things stand at once; everyone else hears of them with the rest.
        world.send(
            Audience::Client(client),
            Standings::kind(),
            self.standings().encode(),
        );
    }

    fn left(&mut self, client: ClientId, world: &mut ServerWorld) {
        if let Some(slot) = self.slots.remove(&client) {
            let mut arena = WorldArena {
                world,
                block_channel: self.block_channel,
                slot_channel: self.slot_channel,
            };
            self.game.leave(slot, &mut arena);
        }
    }

    fn steer(&mut self, client: ClientId, steer: Steer) {
        if let Some(&slot) = self.slots.get(&client) {
            self.game.steer(slot, steer);
        }
    }

    /// One tick of the round, and every player's pose after it.
    fn swim(&mut self, world: &mut ServerWorld, poses: &mut PoseBook) {
        let mut arena = WorldArena {
            world,
            block_channel: self.block_channel,
            slot_channel: self.slot_channel,
        };
        self.game.tick(&mut arena);
        let fraction = self.game.fraction_of_step();
        for client in world.clients().collect::<Vec<_>>() {
            let pose = self
                .slots
                .get(&client)
                .and_then(|&slot| self.game.eel(slot))
                .map_or_else(spectator_pose, |eel| eel_pose(eel, fraction));
            poses.set_pose(client, pose);
        }
    }
}

/// The rules of Eels as a plugin: eels and colours for the players who join, steering, the round,
/// and the standings. The server moves every player (their camera is the eel's), so it uses the record of
/// where players are that the game gives it, and does not believe what players say about it.
#[derive(Debug)]
pub struct PluginEels {
    block_channel: ChannelIndex,
    slot_channel: ChannelIndex,
    players: SharedPoseBook,
}

impl PluginEels {
    pub fn new(content: &ContentEels, players: SharedPoseBook) -> Self {
        Self {
            block_channel: content.block_channel(),
            slot_channel: content.slot_channel(),
            players,
        }
    }
}

fn lock_rules(rules: &Mutex<LogicEels>) -> MutexGuard<'_, LogicEels> {
    rules
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Plugin for PluginEels {
    fn id(&self) -> NamespacedId {
        "voxunami_eels:rules"
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
        let state = Arc::new(Mutex::new(LogicEels::new(
            self.block_channel,
            self.slot_channel,
        )));
        let rules = Arc::clone(&state);
        let players = Arc::clone(&self.players);
        registrar.on::<PlayerJoined>(
            HandlerSpec::new("voxunami_eels:give_the_newcomer_an_eel"),
            move |event, context| {
                let mut book = player_presence::lock(&players);
                lock_rules(&rules).joined(event.client, context.world(), &mut book);
            },
        );
        let rules = Arc::clone(&state);
        registrar.on::<PlayerLeft>(
            HandlerSpec::new("voxunami_eels:take_the_eel_away"),
            move |event, context| lock_rules(&rules).left(event.client, context.world()),
        );
        let rules = Arc::clone(&state);
        registrar.message(move |_, client, steer: Steer| {
            lock_rules(&rules).steer(client, steer);
        });
        registrar.declares_message::<Standings>();
        // The eels swim, and the players' poses follow them, before the poses are announced (the
        // players plugin does that in the `Settle` phase); the standings follow the announcement.
        let rules = Arc::clone(&state);
        let players = Arc::clone(&self.players);
        registrar.system(
            SystemSpec::new("voxunami_eels:swim", TickPhase::Physics),
            move |context| {
                let mut book = player_presence::lock(&players);
                lock_rules(&rules).swim(context.world(), &mut book);
            },
        );
        let rules = Arc::clone(&state);
        registrar.system(
            SystemSpec::new("voxunami_eels:standings", TickPhase::Rules),
            move |context| lock_rules(&rules).announce(context.world()),
        );
    }
}
