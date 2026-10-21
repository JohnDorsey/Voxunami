//! Walking, flying and falling: the server moves the players' bodies.
//!
//! Each player has a [`CharacterBody`] on the server, in a local Rapier world of their own whose terrain is made, as the player goes,
//! from the world's blocks and states. The player's program says what its keys ask for and which way it faces (see
//! [`crate::messages::MoveRequest`]); the server steps the body, three steps of a sixtieth of a second to each 20 Hz tick, and says where
//! the player is. So where a player is does not depend on their word, and the same inputs give the same positions on every machine.
//!
//! Terrain collision is what a block's shape says, except that flames and the like
//! ([`BlockTable::is_passable`]) are not in the way, and terrain that cannot be read is a wall.
//!
//! A body is simulated in a *frame*: the root grid of a level, or a ship it is aboard. In a level, ships that are near are colliders that
//! stand where the ships are, and the body that ends a step standing on a block of one is put aboard it. Aboard, the body is in the
//! ship's own coordinates, where the ship's blocks are its terrain and the ship is still, so the ship carries it exactly; gravity is
//! the world's field turned into those coordinates (so on a ship that is on its side or upside down the body stands on what is the floor
//! now); and a body that has nothing of the ship under it, or is far from it, is put back in the level with the speed the ship gave it.
//! See [`crate::aboard`] for the two questions, and `docs/moving-grids-spec.md`, 2.6.

use std::collections::{BTreeMap, BTreeSet};

use crate::aboard::{Extent, has_left, in_voxels, stands_on};
use crate::block_properties;
use block_models::BlockTable;
use game_shell::PlayerPose;
use grid_bodies::collision::{BlockCollisions, ShipHull, WorldTerrain};
use physics::{CharacterBody, CharacterSettings, ForeignGrid, MoveInput};
use protocol::ClientId;
use server::ServerWorld;
use voxel_grid::ChannelIndex;
use voxel_space::{BrickBox, BrickCoordinate, GridId, LevelIndex, Orientation, WorldPosition};

use crate::messages::MoveRequest;

/// Physics steps to a tick: the tick is 50 ms and a step a sixtieth of a second.
pub const PHYSICS_STEPS_PER_TICK: u32 = 3;

/// How many bricks of the world a player may be from the bricks of a ship for the ship to be in their world: its blocks are colliders
/// there, so the player walks on it and is stopped by it.
const SHIP_REACH_BRICKS: i64 = 2;

/// What the world says a player is: where, in which frame, and which way they look. The record of the player is the truth about this
/// (the entity), and the body is made to agree with it when it does not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The frame the player is in: the root grid of their level, or a ship they are aboard.
    pub frame: GridId,
    /// The level they are in, which is the ship's if they are aboard.
    pub level: LevelIndex,
    /// The place of their eyes, in the frame.
    pub position: WorldPosition,
    /// The turn of their view in the frame.
    pub orientation: Orientation,
    pub yaw: f32,
    pub pitch: f32,
}

impl Placement {
    fn pose(&self) -> PlayerPose {
        PlayerPose {
            position: self.position,
            yaw: self.yaw,
            pitch: self.pitch,
        }
    }

    /// The turn of the player's body frame in their frame (see [`PlayerPose::frame_rotation_of`]).
    fn frame_rotation(&self) -> Orientation {
        self.pose().frame_rotation_of(self.orientation)
    }
}

/// Where a player is after a tick, in the frame their body was simulated in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moved {
    pub client: ClientId,
    pub frame: GridId,
    /// The place of their eyes in the frame, and the angles they gave.
    pub pose: PlayerPose,
    /// The turn of their view in the frame.
    pub orientation: Orientation,
}

/// A player's body and what they last asked of it.
struct PlayerBody {
    body: CharacterBody,
    /// The frame the body is simulated in: the root grid of its level, or the ship it is aboard.
    frame: GridId,
    /// The level the body's terrain is of (the level of the ship, if it is aboard one).
    level: LevelIndex,
    request: MoveRequest,
    /// A jump was asked for since the last tick, though it may not be asked for any more: a tap
    /// between two messages still jumps.
    jump_asked: bool,
    /// The player pilots a ship: their keys fly it (see [`crate::ships`]) and do not walk them.
    piloting: bool,
}

/// What the blocks of the world mean to bodies.
struct Blocks {
    collisions: BlockCollisions,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    air: u32,
}

impl Blocks {
    fn hull<'a>(&'a self, ship: &'a world_storage::MovingGrid) -> ShipHull<'a> {
        ShipHull {
            voxels: &ship.voxels,
            block_channel: self.block_channel,
            state_channel: self.state_channel,
            collisions: &self.collisions,
        }
    }

    /// The terrain of a level, as bodies in it meet it.
    fn terrain<'a>(&'a self, world: &'a mut ServerWorld, level: LevelIndex) -> WorldTerrain<'a> {
        WorldTerrain {
            world,
            level,
            block_channel: self.block_channel,
            state_channel: self.state_channel,
            air: self.air,
            collisions: &self.collisions,
        }
    }
}

/// What a tick needs to know of each ship, worked out once for all the players.
struct ShipBounds {
    level: LevelIndex,
    /// The bricks of the level the ship's bricks are in or touch.
    bricks: BrickBox,
    /// The box of the ship's bricks in its own coordinates.
    extent: Extent,
}

/// The players' bodies.
pub struct Movement {
    bodies: BTreeMap<ClientId, PlayerBody>,
    blocks: Blocks,
    /// Bricks edited since the last step, by grid, whose colliders near bodies must make again.
    changed: BTreeSet<(GridId, BrickCoordinate)>,
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
            blocks: Blocks {
                collisions: BlockCollisions::new(
                    table.clone(),
                    Some(block_properties::passable(&table)),
                ),
                block_channel,
                state_channel,
                air,
            },
            changed: BTreeSet::new(),
        }
    }

    pub fn has_body(&self, client: ClientId) -> bool {
        self.bodies.contains_key(&client)
    }

    /// Gives a player a body, eyes where `placement` says, in the frame it says, at rest. Does nothing if they have one.
    pub fn spawn(&mut self, client: ClientId, placement: Placement) {
        self.bodies.entry(client).or_insert_with(|| {
            let mut body = CharacterBody::new(placement.position, CharacterSettings::default());
            // A player who was aboard a ship when they last played is in its frame at once.
            let frame_rotation = placement.frame_rotation();
            if placement.frame.root_level().is_none() || frame_rotation != Orientation::IDENTITY {
                body.reframe(placement.position, [0.0; 3], frame_rotation.components());
            }
            PlayerBody {
                body,
                frame: placement.frame,
                level: placement.level,
                request: MoveRequest {
                    input: MoveInput {
                        yaw: placement.yaw,
                        ..MoveInput::default()
                    },
                    pitch: placement.pitch,
                },
                jump_asked: false,
                piloting: false,
            }
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

    /// Puts a player's eyes at a position in `level`, at rest, in the level and not aboard any ship. If that is inside the terrain they
    /// are moved out of it at once (the shortest way, see `CharacterBody::teleport`), so that what they are told is where they are.
    /// Returns where their eyes are, or `None` if they have no body.
    pub fn teleport(
        &mut self,
        world: &mut ServerWorld,
        level: LevelIndex,
        client: ClientId,
        position: WorldPosition,
    ) -> Option<WorldPosition> {
        let player = self.bodies.get_mut(&client)?;
        player.level = level;
        player.frame = GridId::root(level);
        player.body.teleport(position);
        // A body that was in a turned frame is upright in the level again.
        if player.body.frame_rotation() != [0.0, 0.0, 0.0, 1.0] {
            player
                .body
                .reframe(position, [0.0; 3], [0.0, 0.0, 0.0, 1.0]);
        }
        player
            .body
            .update_terrain(&mut self.blocks.terrain(world, level));
        Some(player.body.eye_position())
    }

    /// Makes a player a pilot, whose keys are not for walking, or not.
    pub fn set_piloting(&mut self, client: ClientId, piloting: bool) {
        if let Some(player) = self.bodies.get_mut(&client) {
            player.piloting = piloting;
        }
    }

    /// What the player's keys last asked, if they have a body.
    pub fn request_of(&self, client: ClientId) -> Option<MoveRequest> {
        self.bodies.get(&client).map(|player| player.request)
    }

    /// Starts or stops a player flying. Returns whether they are flying now, or `None` if they
    /// have no body.
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

    /// Where a player's eyes are, in the frame their body is in.
    pub fn eye_position(&self, client: ClientId) -> Option<WorldPosition> {
        self.bodies
            .get(&client)
            .map(|player| player.body.eye_position())
    }

    /// Tells the bodies that the blocks in a brick of a grid (a level's root grid or a ship) changed.
    pub fn brick_changed(&mut self, grid: GridId, brick: BrickCoordinate) {
        self.changed.insert((grid, brick));
    }

    /// One tick: every body takes its steps, in the order of the players' ids, in the frame the player's record says they are in now
    /// (`placement_of`; a player with none does not move). Returns where each player is.
    pub fn step(
        &mut self,
        world: &mut ServerWorld,
        placement_of: impl Fn(ClientId) -> Option<Placement>,
    ) -> Vec<Moved> {
        let changed = std::mem::take(&mut self.changed);
        let ships = if self.bodies.is_empty() {
            BTreeMap::new()
        } else {
            ship_bounds(world)
        };
        let mut moved = Vec::with_capacity(self.bodies.len());
        for (&client, player) in &mut self.bodies {
            let Some(placement) = placement_of(client) else {
                continue;
            };
            agree_with(player, &placement);
            match player.frame.root_level() {
                Some(level) => {
                    step_in_a_level(player, &self.blocks, world, &changed, &ships, level)
                }
                None => step_aboard(player, &self.blocks, world, &changed, &ships),
            }
            moved.push(moved_pose(client, player));
        }
        moved
    }
}

/// What a tick needs of each ship: where its bricks are in the world and in itself.
fn ship_bounds(world: &ServerWorld) -> BTreeMap<GridId, ShipBounds> {
    world
        .data()
        .grids()
        .iter()
        .filter_map(|(id, ship)| {
            Some((
                id,
                ShipBounds {
                    level: ship.level,
                    bricks: ship.world_brick_bounds()?,
                    extent: Extent::of(ship)?,
                },
            ))
        })
        .collect()
}

/// Makes the body agree with what the record of the player says, when it does not: the player was put in another level (the same place,
/// another terrain), or a ship was taken from under them, or they came back aboard one. The record is where they are; the body is made to
/// be there, in the frame the record names.
fn agree_with(player: &mut PlayerBody, placement: &Placement) {
    if placement.frame == player.frame && placement.level == player.level {
        return;
    }
    if placement.frame.root_level().is_some() && player.frame.root_level().is_some() {
        // Another level, at the same place: its terrain is another's.
        player.level = placement.level;
        player.frame = placement.frame;
        let eyes = player.body.eye_position();
        player.body.teleport(eyes);
        return;
    }
    // In or out of a ship. Out of one that is gone, the player is where the record says in the world, at rest: nothing is known of how
    // fast the ship was going. In, they come back as they were when they left, which was at rest.
    let velocity = [0.0; 3];
    player.body.reframe(
        placement.position,
        velocity,
        placement.frame_rotation().components(),
    );
    player.frame = placement.frame;
    player.level = placement.level;
}

/// The input for this tick: the keys, with a jump that was asked for between two messages.
fn input_for(player: &mut PlayerBody) -> MoveInput {
    let input = if player.piloting {
        // The keys of a pilot fly the ship; the pilot stands.
        MoveInput {
            yaw: player.request.input.yaw,
            ..MoveInput::default()
        }
    } else {
        MoveInput {
            jump: player.request.input.jump || player.jump_asked,
            ..player.request.input
        }
    };
    player.jump_asked = false;
    input
}

/// Whether a ship is in reach of a player's brick: its bricks in the world, widened by `SHIP_REACH_BRICKS`, hold the brick.
fn in_reach(bounds: &ShipBounds, level: LevelIndex, here: BrickCoordinate) -> bool {
    let (min, max) = (bounds.bricks.min(), bounds.bricks.max());
    bounds.level == level
        && (min.x() - SHIP_REACH_BRICKS..=max.x() + SHIP_REACH_BRICKS).contains(&here.x())
        && (min.y() - SHIP_REACH_BRICKS..=max.y() + SHIP_REACH_BRICKS).contains(&here.y())
        && (min.z() - SHIP_REACH_BRICKS..=max.z() + SHIP_REACH_BRICKS).contains(&here.z())
}

/// One tick of a body that is in a level: the terrain of the level, the ships near it as colliders, the gravity of the level's field
/// there, and then whether it ended standing on a ship.
fn step_in_a_level(
    player: &mut PlayerBody,
    blocks: &Blocks,
    world: &mut ServerWorld,
    changed: &BTreeSet<(GridId, BrickCoordinate)>,
    ships: &BTreeMap<GridId, ShipBounds>,
    level: LevelIndex,
) {
    {
        let mut terrain = blocks.terrain(world, level);
        for &(grid, brick) in changed {
            if grid == GridId::root(level) {
                player.body.brick_changed(&mut terrain, brick);
            }
        }
        player.body.update_terrain(&mut terrain);
    }

    // The ships near the player are in their world as colliders that stand where the ships are.
    let here = player.body.middle_position().brick();
    let near: Vec<GridId> = ships
        .iter()
        .filter(|(_, bounds)| in_reach(bounds, level, here))
        .map(|(id, _)| *id)
        .collect();
    {
        let data = world.data();
        let mut hulls: Vec<_> = near
            .iter()
            .filter_map(|id| {
                let ship = data.grids().get(*id)?;
                Some((*id, ship.pose, blocks.hull(ship)))
            })
            .collect();
        let mut grids: Vec<ForeignGrid<'_>> = hulls
            .iter_mut()
            .map(|(id, pose, hull)| ForeignGrid {
                id: *id,
                pose: *pose,
                source: hull,
            })
            .collect();
        for &(grid, brick) in changed {
            if let Some(ship) = grids.iter_mut().find(|ship| ship.id == grid) {
                player.body.foreign_brick_changed(ship, brick);
            }
        }
        player.body.update_foreign(&mut grids);
    }
    // A ship that has moved into the player this tick (fallen on them, sailed into them) puts them out of the way and not into the ground.
    player.body.keep_clear(&mut blocks.terrain(world, level));

    let gravity = world.gravity_at(level, player.body.eye_position());
    let input = input_for(player);
    for _ in 0..PHYSICS_STEPS_PER_TICK {
        player.body.step_with_gravity(input, gravity);
    }

    // On the ground, on a block of a ship: aboard it from here on.
    if !player.body.is_flying() && player.body.is_grounded() {
        let feet = player.body.feet_position();
        let down = player.body.up().map(|axis| -axis);
        let ship = near.iter().copied().find(|id| {
            world
                .data()
                .grids()
                .get(*id)
                .is_some_and(|ship| stands_on(&mut blocks.hull(ship), ship, feet, down))
        });
        if let Some(id) = ship {
            board(player, id, world);
        }
    }
}

/// Puts a body aboard a ship: the same place and the speed it has relative to the ship, in the ship's coordinates and axes.
fn board(player: &mut PlayerBody, id: GridId, world: &ServerWorld) {
    let Some(ship) = world.data().grids().get(id) else {
        return;
    };
    let eyes = player.body.eye_position();
    let Some(local) = ship.pose.world_to_local(eyes) else {
        return;
    };
    let carried = ship.velocity_at(eyes);
    let speed = player.body.velocity();
    let mut relative = [0, 1, 2].map(|axis| speed[axis] - carried[axis]);
    // They stand on it, so they are not moving away from it: a walker that a ship has fallen away from, or has lifted onto itself, is not
    // thrown up at the speed it had. (Moving toward it is the controller's to stop, as it always did.)
    let up = player.body.up();
    let away: f32 = (0..3).map(|axis| relative[axis] * up[axis]).sum();
    if away > 0.0 {
        for axis in 0..3 {
            relative[axis] -= up[axis] * away;
        }
    }
    let turn =
        Orientation::from_components(player.body.frame_rotation()).unwrap_or(Orientation::IDENTITY);
    let frame_rotation = ship.pose.orientation.inverse().compose(turn);
    player.body.reframe(
        local,
        ship.pose.direction_to_local(relative),
        frame_rotation.components(),
    );
    player.frame = id;
}

/// One tick of a body that is aboard a ship: the ship's blocks are its terrain, gravity is the world's in the ship's axes, and then
/// whether it has left.
fn step_aboard(
    player: &mut PlayerBody,
    blocks: &Blocks,
    world: &mut ServerWorld,
    changed: &BTreeSet<(GridId, BrickCoordinate)>,
    ships: &BTreeMap<GridId, ShipBounds>,
) {
    let id = player.frame;
    {
        let Some(ship) = world.data().grids().get(id) else {
            // The ship is not there. The record of the player says so next tick and the body is put in the level then.
            return;
        };
        let mut hull = blocks.hull(ship);
        for &(grid, brick) in changed {
            if grid == id {
                player.body.brick_changed(&mut hull, brick);
            }
        }
        player.body.update_terrain(&mut hull);
        player.body.keep_clear(&mut hull);
    }
    let gravity = world.gravity_in(id, player.body.eye_position()).unwrap_or([
        0.0,
        -player.body.settings().gravity,
        0.0,
    ]);
    let input = input_for(player);
    for _ in 0..PHYSICS_STEPS_PER_TICK {
        player.body.step_with_gravity(input, gravity);
    }

    // Has it left? Nothing of the ship under it, or far from the ship; or nothing of the ship is left.
    let left = match (world.data().grids().get(id), ships.get(&id)) {
        (Some(ship), Some(bounds)) => {
            let down = player.body.up().map(|axis| -f64::from(axis));
            has_left(
                &mut blocks.hull(ship),
                &bounds.extent,
                in_voxels(player.body.middle_position()),
                in_voxels(player.body.feet_position()),
                down,
                player.body.is_grounded(),
            )
        }
        _ => true,
    };
    if left {
        leave(player, world);
    }
}

/// Puts a body that is aboard a ship in the level the ship is in: the same place in the world, with the speed it has in the world,
/// which is what it has in the ship, turned, and the speed the ship gives that place.
fn leave(player: &mut PlayerBody, world: &ServerWorld) {
    let Some(ship) = world.data().grids().get(player.frame) else {
        return;
    };
    let eyes = player.body.eye_position();
    let Some(world_eyes) = ship.pose.local_to_world(eyes) else {
        return;
    };
    let in_the_ship = ship.pose.direction_to_world(player.body.velocity());
    let carried = ship.velocity_at(world_eyes);
    let velocity = [0, 1, 2].map(|axis| in_the_ship[axis] + carried[axis]);
    let turn =
        Orientation::from_components(player.body.frame_rotation()).unwrap_or(Orientation::IDENTITY);
    let frame_rotation = ship.pose.orientation.compose(turn);
    player
        .body
        .reframe(world_eyes, velocity, frame_rotation.components());
    player.frame = GridId::root(ship.level);
    player.level = ship.level;
}

/// Where the player is, in the frame their body is in.
fn moved_pose(client: ClientId, player: &PlayerBody) -> Moved {
    let pose = PlayerPose {
        position: player.body.eye_position(),
        yaw: player.request.input.yaw,
        pitch: player.request.pitch,
    };
    let frame_rotation =
        Orientation::from_components(player.body.frame_rotation()).unwrap_or(Orientation::IDENTITY);
    Moved {
        client,
        frame: player.frame,
        pose,
        orientation: pose.view_orientation(frame_rotation),
    }
}
