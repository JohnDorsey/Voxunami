//! Walking, flying and falling: the server moves the players' bodies.
//!
//! Each player has a [`CharacterBody`] on the server, in a local Rapier world of their own whose
//! terrain is made, as the player goes, from the world's blocks and states. The player's program says
//! what its keys ask for and which way it faces (see [`crate::messages::MoveRequest`]); the server
//! steps the body, three steps of a sixtieth of a second to each 20 Hz tick, and says where the
//! player is. So where a player is does not depend on their word, and the same inputs give the
//! same positions on every machine.
//!
//! Terrain collision is what a block's shape says, except that flames and the like
//! ([`BlockTable::is_passable`]) are not in the way, and terrain that cannot be read is a wall.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::block_properties;
use block_models::{BlockTable, PropertyHandle};
use game_shell::PlayerPose;
use physics::{BrickHint, CharacterBody, CharacterSettings, Collision, CollisionSource, MoveInput};
use protocol::ClientId;
use server::ServerWorld;
use voxel_grid::ChannelIndex;
use voxel_space::{BrickCoordinate, LevelIndex, VoxelCoordinate, WorldPosition};

use crate::messages::MoveRequest;

/// Physics steps to a tick: the tick is 50 ms and a step a sixtieth of a second.
pub const PHYSICS_STEPS_PER_TICK: u32 = 3;

/// What each block's shape means to something moving through it, worked out once.
#[derive(Debug)]
struct ModelCollisions {
    table: BlockTable,
    passable: PropertyHandle<bool>,
    cache: HashMap<u32, Collision>,
}

impl ModelCollisions {
    fn of(&mut self, block: u32, state: u32) -> Collision {
        let model = self.table.model_of(block, state);
        let table = &self.table;
        let passable = self.passable;
        self.cache
            .entry(model.get())
            .or_insert_with(|| {
                let shape = table.shape(model);
                if table.get(passable, model) || shape.is_empty() {
                    Collision::Empty
                } else if shape.is_full() {
                    Collision::Full
                } else {
                    Collision::Partial(shape.boxes().to_vec().into())
                }
            })
            .clone()
    }
}

/// One level of the server's world, as terrain for a body to move through.
struct Terrain<'a> {
    world: &'a mut ServerWorld,
    level: LevelIndex,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    air: u32,
    collisions: &'a mut ModelCollisions,
}

impl CollisionSource for Terrain<'_> {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        let (Ok(block), Ok(state)) = (
            self.world.voxel(self.level, self.block_channel, voxel),
            self.world.voxel(self.level, self.state_channel, voxel),
        ) else {
            return Collision::Full; // terrain that cannot be read is a wall, not a void
        };
        self.collisions.of(block, state)
    }

    fn brick_hint(&mut self, brick: BrickCoordinate) -> BrickHint {
        let Ok(Some(block)) = self
            .world
            .uniform_value(self.level, self.block_channel, brick)
        else {
            return BrickHint::Unknown;
        };
        if block == self.air {
            return BrickHint::Empty;
        }
        match self
            .world
            .uniform_value(self.level, self.state_channel, brick)
        {
            Ok(Some(state)) => match self.collisions.of(block, state) {
                Collision::Empty => BrickHint::Empty,
                Collision::Full => BrickHint::Solid,
                Collision::Partial(_) => BrickHint::Unknown,
            },
            _ => BrickHint::Unknown,
        }
    }
}

/// A player's body and what they last asked of it.
struct PlayerBody {
    body: CharacterBody,
    /// The level the body's terrain is of.
    level: LevelIndex,
    request: MoveRequest,
    /// A jump was asked for since the last tick, though it may not be asked for any more: a tap
    /// between two messages still jumps.
    jump_asked: bool,
}

/// The players' bodies.
pub struct Movement {
    bodies: BTreeMap<ClientId, PlayerBody>,
    collisions: ModelCollisions,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    air: u32,
    /// Bricks edited since the last step, whose terrain bodies near them must make again.
    changed: BTreeSet<(LevelIndex, BrickCoordinate)>,
}

impl std::fmt::Debug for Movement {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Movement")
            .field("bodies", &self.bodies.len())
            .finish_non_exhaustive()
    }
}

impl Movement {
    pub fn new(
        table: BlockTable,
        block_channel: ChannelIndex,
        state_channel: ChannelIndex,
        air: u32,
    ) -> Self {
        Self {
            bodies: BTreeMap::new(),
            collisions: ModelCollisions {
                passable: block_properties::passable(&table),
                table,
                cache: HashMap::new(),
            },
            block_channel,
            state_channel,
            air,
            changed: BTreeSet::new(),
        }
    }

    pub fn has_body(&self, client: ClientId) -> bool {
        self.bodies.contains_key(&client)
    }

    /// Gives a player a body, eyes at `pose`, in `level`, at rest. Does nothing if they have one.
    pub fn spawn(&mut self, client: ClientId, pose: PlayerPose, level: LevelIndex) {
        self.bodies.entry(client).or_insert_with(|| PlayerBody {
            body: CharacterBody::new(pose.position, CharacterSettings::default()),
            level,
            request: MoveRequest {
                input: MoveInput {
                    yaw: pose.yaw,
                    ..MoveInput::default()
                },
                pitch: pose.pitch,
            },
            jump_asked: false,
        });
    }

    pub fn remove(&mut self, client: ClientId) {
        self.bodies.remove(&client);
    }

    /// What a player's keys now ask of their body.
    pub fn ask(&mut self, client: ClientId, request: MoveRequest) {
        if let Some(player) = self.bodies.get_mut(&client) {
            player.jump_asked |= request.input.jump;
            player.request = request;
        }
    }

    /// Puts a player's eyes at a position, at rest. Returns whether they have a body.
    pub fn teleport(&mut self, client: ClientId, position: WorldPosition) -> bool {
        let Some(player) = self.bodies.get_mut(&client) else {
            return false;
        };
        player.body.teleport(position);
        true
    }

    /// Starts or stops a player flying. Returns whether they are flying now, or `None` if they have
    /// no body.
    pub fn toggle_flying(&mut self, client: ClientId) -> Option<bool> {
        let player = self.bodies.get_mut(&client)?;
        let flying = !player.body.is_flying();
        player.body.set_flying(flying);
        Some(flying)
    }

    pub fn is_flying(&self, client: ClientId) -> bool {
        self.bodies
            .get(&client)
            .is_some_and(|player| player.body.is_flying())
    }

    /// Where a player's eyes are.
    pub fn eye_position(&self, client: ClientId) -> Option<WorldPosition> {
        self.bodies
            .get(&client)
            .map(|player| player.body.eye_position())
    }

    /// Tells the bodies that the terrain in a brick was changed.
    pub fn brick_changed(&mut self, level: LevelIndex, brick: BrickCoordinate) {
        self.changed.insert((level, brick));
    }

    /// One tick: every body takes its steps, in the order of the players' ids, in the level the
    /// player is in now (`level_of`). Returns where each player is.
    pub fn step(
        &mut self,
        world: &mut ServerWorld,
        level_of: impl Fn(ClientId) -> LevelIndex,
    ) -> Vec<(ClientId, PlayerPose)> {
        let changed = std::mem::take(&mut self.changed);
        let mut poses = Vec::with_capacity(self.bodies.len());
        for (&client, player) in &mut self.bodies {
            let level = level_of(client);
            let mut terrain = Terrain {
                world,
                level,
                block_channel: self.block_channel,
                state_channel: self.state_channel,
                air: self.air,
                collisions: &mut self.collisions,
            };
            if player.level != level {
                // Moved to another level, at the same place: its terrain is another's.
                player.level = level;
                let eyes = player.body.eye_position();
                player.body.teleport(eyes);
            }
            for &(changed_level, brick) in &changed {
                if changed_level == level {
                    player.body.brick_changed(&mut terrain, brick);
                }
            }
            player.body.update_terrain(&mut terrain);
            let input = MoveInput {
                jump: player.request.input.jump || player.jump_asked,
                ..player.request.input
            };
            player.jump_asked = false;
            for _ in 0..PHYSICS_STEPS_PER_TICK {
                player.body.step(input);
            }
            poses.push((
                client,
                PlayerPose {
                    position: player.body.eye_position(),
                    yaw: player.request.input.yaw,
                    pitch: player.request.pitch,
                },
            ));
        }
        poses
    }
}
