//! Walking, flying and falling: the server moves the players' bodies.
//!
//! Each player has a [`CharacterBody`] on the server, in a local Rapier world of their own whose terrain is made, as the player goes,
//! from the world's blocks and states. The player's program says what its keys ask for and which way it faces (see
//! [`crate::messages::MoveRequest`]); the server steps the body, three steps of a sixtieth of a second to each 20 Hz tick, and says where
//! the player is. So where a player is does not depend on their word, and the same inputs give the same positions on every machine.
//!
//! Terrain collision is what a block's shape says, except that flames and the like
//! (blocks with the `passable` property) are not in the way, and terrain that cannot be read is a wall.
//!
//! A body is simulated in a *frame*: the root grid of a level, or a ship it is aboard. In a level, ships that are near are colliders that
//! stand where the ships are, and the body that ends a step standing on a block of one is put aboard it. Aboard, the body is in the
//! ship's own coordinates, where the ship's blocks are its terrain and the ship is still, so the ship carries it exactly; gravity is
//! the world's field turned into those coordinates (so on a ship that is on its side or upside down the body stands on what is the floor
//! now); and a body that has nothing of the ship under it, or is far from it, is put back in the level with the speed the ship gave it.
//! See [`crate::aboard`] for the two questions, and `docs/specs/moving-grids-spec.md`, 2.6.

use std::collections::{BTreeMap, BTreeSet};

use crate::aboard::{Extent, has_left, in_voxels, stands_on};
use crate::block_properties;
use crate::frame_terms::{frame_motion, pull_in_a_frame};
use block_models::BlockTable;
use game_shell::PlayerPose;
use grid_bodies::collision::{BlockCollisions, GeneratedHull, ShipHull, ShipSource, WorldTerrain};
use physics::{CharacterBody, CharacterSettings, ForeignGrid, MoveInput};
use protocol::ClientId;
use server::ServerWorld;
use voxel_grid::ChannelIndex;
use voxel_space::{
    BrickBox, BrickCoordinate, Displacement, GridId, GridPose, LevelIndex, Orientation,
    WorldPosition,
};

use crate::messages::MoveRequest;
use crate::renaming::successor;
use crate::ships::TICK_SECONDS;

/// Physics steps to a tick: the tick is 50 ms and a step a sixtieth of a second.
pub const PHYSICS_STEPS_PER_TICK: u32 = 3;

/// How many bricks of the world a player may be from the bricks of a ship for the ship to be in their world: its blocks are colliders
/// there, so the player walks on it and is stopped by it.
const SHIP_REACH_BRICKS: i128 = 2;

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
    pub yaw: f64,
    pub pitch: f64,
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

/// What the world carried a player by in a tick: a planet on a rail carries what is in the level inside its sphere of influence (`ServerWorld::apply_rails`).
///
/// **The speed of a body in a level is against the planet that carries it** (the world's speed if none does), because the carry moves the body by the planet's displacement
/// and the body's own speed moves it from there. A ship's speed (`MovingGrid::linear_velocity`) is the world's: so a speed that passes between a body and a ship (boarding it,
/// leaving it, being stepped against it) is made against the carrier or the world's by the speed of the planet, or the body has the planet's speed twice, once from the carry
/// and once from itself, and goes 125 metres a second faster than the ship it left, past a planet that goes round at that speed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Carried {
    /// The planet whose sphere the player is in.
    pub by: GridId,
    /// How far it moved them this tick, nothing if it did not move.
    pub displacement: Displacement,
}

/// A speed in the world from a speed against a planet that goes at `carrier` (the same speed if no planet does).
fn in_the_world(against: [f64; 3], carrier: [f64; 3]) -> [f64; 3] {
    if carrier == [0.0; 3] {
        return against;
    }
    [0, 1, 2].map(|axis| against[axis] + carrier[axis])
}

/// A speed against a planet that goes at `carrier`, from a speed in the world (the same speed if no planet does).
fn against_the_planet(in_the_world: [f64; 3], carrier: [f64; 3]) -> [f64; 3] {
    if carrier == [0.0; 3] {
        return in_the_world;
    }
    [0, 1, 2].map(|axis| in_the_world[axis] - carrier[axis])
}

/// How fast a planet on a rail goes in the world: nothing for none, or for one that is gone.
fn planet_speed(world: &ServerWorld, planet: Option<GridId>) -> [f64; 3] {
    planet
        .and_then(|planet| world.data().grids().get(planet))
        .map_or([0.0; 3], |planet| planet.linear_velocity)
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
    /// The ship the player is buckled to: their body does not move in it (not by their keys, by gravity or by anything they touch), so the ship carries it exactly,
    /// however it moves and turns. Let go when they are not aboard that ship any more.
    buckled: Option<GridId>,
    /// Where the body left a ship, until it stands on something again (see [`Departure`]).
    departure: Option<Departure>,
    /// The planet whose speed the body's is against, while the body is in a level: the one that carried the player in the last tick (see [`Carried`]), or none.
    carrier: Option<GridId>,
    /// The body was put where it is since its last step (it came in, was sent somewhere with `/tp`, or was made to agree with a record in another frame): it is at rest against
    /// whatever carries it, and what the world says carries it is not a change of carrier (see [`pass_to_next_carrier`]).
    put: bool,
    /// How fast the ship the body is aboard went when it was last stepped, to measure how it accelerates from (see [`felt_aboard`]).
    motion: Option<Motion>,
}

/// How a ship went when a body aboard it was last stepped: the speed of its centre of mass and how it turned, in the world's axes, and the planet on a rail that carried the place,
/// with its speed.
#[derive(Clone, Copy, Debug)]
struct Motion {
    ship: GridId,
    velocity: [f64; 3],
    spin: [f64; 3],
    carrier: Option<(GridId, [f64; 3])>,
}

impl Motion {
    /// How the ship `id`, which is `ship`, goes now, with the planet that carries the place of a body aboard it, if one does.
    fn of(
        id: GridId,
        ship: &world_storage::MovingGrid,
        carrier: Option<(GridId, [f64; 3])>,
    ) -> Self {
        Self {
            ship: id,
            velocity: ship.linear_velocity,
            spin: ship.angular_velocity,
            carrier,
        }
    }
}

/// The place in a ship that a body stepped off. Until the body stands on something again, what its keys ask and the air does are measured from the speed of that place of the ship, as a
/// person who steps off a train goes on at its speed and walks from there; measured from the speed of the level, a ship at 500 voxels a second left a walker behind at once, the air
/// taking the 500 from them at 14 a second a second.
#[derive(Clone, Copy, Debug)]
struct Departure {
    ship: GridId,
    /// The place of the eyes in the ship's own coordinates, where they left it.
    place: WorldPosition,
}

/// What the blocks of the world mean to bodies.
struct Blocks {
    collisions: BlockCollisions,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    orientation_channel: ChannelIndex,
    air: u32,
}

impl Blocks {
    fn hull<'a>(&'a self, ship: &'a world_storage::MovingGrid) -> ShipHull<'a> {
        ShipHull {
            voxels: &ship.voxels,
            block_channel: self.block_channel,
            state_channel: self.state_channel,
            orientation_channel: Some(self.orientation_channel),
            collisions: &self.collisions,
        }
    }

    /// What a body meets of a ship: its stored blocks, or for a planet (a ship with terrain) the blocks its generator makes as well, which are read through the
    /// world.
    fn source<'a>(
        &'a self,
        world: &'a ServerWorld,
        id: GridId,
        ship: &'a world_storage::MovingGrid,
    ) -> ShipSource<'a> {
        if ship.terrain.is_some() {
            ShipSource::Generated(GeneratedHull {
                world,
                grid: id,
                block_channel: self.block_channel,
                state_channel: self.state_channel,
                orientation_channel: Some(self.orientation_channel),
                air: self.air,
                collisions: &self.collisions,
            })
        } else {
            ShipSource::Stored(self.hull(ship))
        }
    }

    /// The terrain of a level, as bodies in it meet it.
    fn terrain<'a>(&'a self, world: &'a mut ServerWorld, level: LevelIndex) -> WorldTerrain<'a> {
        WorldTerrain {
            world,
            level,
            block_channel: self.block_channel,
            state_channel: self.state_channel,
            orientation_channel: Some(self.orientation_channel),
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
        orientation_channel: ChannelIndex,
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
                orientation_channel,
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
                    roll: 0,
                },
                jump_asked: false,
                piloting: false,
                buckled: None,
                departure: None,
                carrier: None,
                put: true,
                motion: None,
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
        player.departure = None;
        player.put = true;
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

    /// Buckles a player to the ship they are aboard: their body stops where it is in the ship, at rest, and stays there, turned as it is, until they are let go.
    /// Returns whether they were: they need a body, aboard `ship`.
    pub fn buckle(&mut self, client: ClientId, ship: GridId) -> bool {
        let Some(player) = self.bodies.get_mut(&client) else {
            return false;
        };
        if player.frame != ship {
            return false;
        }
        player.buckled = Some(ship);
        // (At rest in the ship: whatever speed they had in it when they were buckled is not theirs when they are let go.)
        let eyes = player.body.eye_position();
        let frame_rotation = player.body.frame_rotation();
        player.body.reframe(eyes, [0.0; 3], frame_rotation);
        true
    }

    /// Lets a player go. Returns the ship they were buckled to, if they were.
    pub fn unbuckle(&mut self, client: ClientId) -> Option<GridId> {
        self.bodies.get_mut(&client)?.buckled.take()
    }

    /// The ship a player is buckled to, if they are.
    pub fn buckled_to(&self, client: ClientId) -> Option<GridId> {
        self.bodies.get(&client)?.buckled
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

    /// Switches a player's suit thrusters (see [`CharacterBody::set_thrusters`]). Returns whether they are on now, or `None` if the player has no body.
    pub fn toggle_thrusters(&mut self, client: ClientId) -> Option<bool> {
        let player = self.bodies.get_mut(&client)?;
        let thrusters = !player.body.thrusters_on();
        player.body.set_thrusters(thrusters);
        Some(thrusters)
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
    /// (`placement_of`; a player with none does not move), carried first by what the world carried the player by this tick (`carried_by_rails`). Returns where
    /// each player is.
    pub fn step(
        &mut self,
        world: &mut ServerWorld,
        placement_of: impl Fn(ClientId) -> Option<Placement>,
        carried_by_rails: impl Fn(ClientId) -> Option<Carried>,
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
            let carried = carried_by_rails(client);
            follow_renamed_ships(player, world, &placement);
            agree_with(
                player,
                &placement,
                carried.map(|carried| carried.displacement),
            );
            player.carrier = carried.map(|carried| carried.by);
            // A buckled player stays where they are in the ship; one whose ship is gone, or who is not in it, is let go.
            if player.buckled.is_some_and(|ship| {
                ship != player.frame || world.data().grids().get(ship).is_none()
            }) {
                player.buckled = None;
            }
            if player.buckled.is_some() {
                // (The ship carries them however it goes; its speed is kept up to date, so that the tick they are let go is not taken for a blow.)
                player.motion = world
                    .data()
                    .grids()
                    .get(player.frame)
                    .map(|ship| Motion::of(player.frame, ship, None));
                player.put = false;
                moved.push(moved_pose(client, player));
                continue;
            }
            match player.frame.root_level() {
                Some(level) => {
                    step_in_a_level(player, &self.blocks, world, &changed, &ships, level)
                }
                None => step_aboard(player, &self.blocks, world, &changed, &ships),
            }
            player.put = false;
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

/// A ship that was split, merged, docked with a planet or given a new id is not the ship the body knows by its old id: the body goes on in the ship that took its place, as it was.
/// The record of the player has been moved to that ship by the world (with the transform, if the ship's coordinates were turned), so the body is put where the record says, and its
/// speed in the ship is turned by the rotation of the transform, not lost: nothing was done to the player. Every id the body holds is made the new one here (see [`crate::renaming`]).
/// A ship that is merely gone is left to [`agree_with`], which puts the body where the record says in the world, at rest.
fn follow_renamed_ships(player: &mut PlayerBody, world: &ServerWorld, placement: &Placement) {
    let grids = world.data().grids();
    let frame = successor(grids, player.frame, Some(placement.frame))
        .filter(|(new, _)| *new == placement.frame && placement.level == player.level);
    if let Some((new, transform)) = frame {
        let old = player.frame;
        let velocity = transform.apply_direction(player.body.velocity());
        player.body.reframe(
            placement.position,
            velocity,
            placement.frame_rotation().components(),
        );
        player.frame = new;
        if player.buckled == Some(old) {
            player.buckled = Some(new);
        }
        if let Some(motion) = player.motion.as_mut().filter(|motion| motion.ship == old) {
            motion.ship = new;
        }
    }
    // The place a body left a ship is in the coordinates of that ship, and is the same place in what took its place.
    if let Some(departure) = player.departure
        && let Some((new, transform)) = successor(grids, departure.ship, None)
    {
        player.departure = transform
            .apply_position(departure.place)
            .map(|place| Departure { ship: new, place });
    }
}

/// Makes the body agree with what the record of the player says, when it does not: the player was put in another level (the same place,
/// another terrain, or the place the game sends players to in it), or a ship was taken from under them, or they came back aboard one. The record is where they are; the body is made to
/// be there, in the frame the record names.
fn agree_with(player: &mut PlayerBody, placement: &Placement, carried: Option<Displacement>) {
    if placement.frame == player.frame && placement.level == player.level {
        // In a level, the world may have carried the record of the player this tick: a planet that goes round carries what is in the air near it (`apply_rails`, and
        // `ServerWorld::carried_by_rails` says by how much). The body is carried by the same, and keeps its velocity (which is against whatever carried it: see [`Carried`]). (A record
        // that is not the body's is not always the newer: a player who has just come in is where the body was put, and the record catches up when the body has stepped.)
        if let Some(carried) = carried {
            player.body.displace(carried.in_voxels().map(|part| part));
        }
        return;
    }
    if placement.frame.root_level().is_some() && player.frame.root_level().is_some() {
        // Another level: its terrain is another's. The record says where they are in it (the place they were at, unless the game put them somewhere).
        player.level = placement.level;
        player.frame = placement.frame;
        player.departure = None;
        player.put = true;
        player.body.teleport(placement.position);
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
    player.departure = None;
    player.put = true;
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

/// The bricks round a place in the ground of the planets in `near`, if the player is coming to it, are made by a worker before they are met. `world_position` is
/// the player's middle in the world.
fn prefetch_planet_ground(world: &mut ServerWorld, near: &[GridId], world_position: WorldPosition) {
    for id in near {
        let Some(planet) = world
            .data()
            .grids()
            .get(*id)
            .filter(|ship| ship.terrain.is_some())
        else {
            continue;
        };
        let Some(there) = planet.pose.world_to_local(world_position) else {
            continue;
        };
        let there = there.brick();
        let bricks: Vec<BrickCoordinate> = (-2..=2)
            .flat_map(|x| {
                (-2..=2).flat_map(move |y| {
                    (-2..=2).map(move |z| {
                        BrickCoordinate::new(there.x() + x, there.y() + y, there.z() + z)
                    })
                })
            })
            .collect();
        world.prefetch_ship_bricks(*id, bricks);
    }
}

/// Puts the blocks of the ships `near` where they are, as colliders in the body's frame, so that the body walks on them and is stopped by them. `frame` is where the grid
/// the body is simulated in stands in the world (a ship it is aboard), or `None` if the frame is a level's own.
fn meet_ships(
    player: &mut PlayerBody,
    blocks: &Blocks,
    world: &ServerWorld,
    changed: &BTreeSet<(GridId, BrickCoordinate)>,
    near: &[GridId],
    frame: Option<GridPose>,
) {
    let data = world.data();
    let mut hulls: Vec<_> = near
        .iter()
        .filter_map(|id| {
            let ship = data.grids().get(*id)?;
            let pose = match frame {
                None => ship.pose,
                Some(frame) => in_the_frame_of(frame, ship.pose)?,
            };
            Some((*id, pose, blocks.source(world, *id, ship)))
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

/// Where a grid that stands at `pose` in the world is, in the coordinates of the grid that stands at `frame`.
fn in_the_frame_of(frame: GridPose, pose: GridPose) -> Option<GridPose> {
    Some(GridPose::new(
        frame.world_to_local(pose.position)?,
        frame.orientation.inverse().compose(pose.orientation),
    ))
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
    // The terrain of the level round them is made by a worker before they walk into it.
    world.prefetch_level_bricks(
        level,
        (-2..=2).flat_map(|x| {
            (-1..=1).flat_map(move |y| {
                (-2..=2)
                    .map(move |z| BrickCoordinate::new(here.x() + x, here.y() + y, here.z() + z))
            })
        }),
    );
    let near: Vec<GridId> = ships
        .iter()
        .filter(|(_, bounds)| in_reach(bounds, level, here))
        .map(|(id, _)| *id)
        .collect();
    prefetch_planet_ground(world, &near, player.body.middle_position());
    meet_ships(player, blocks, world, changed, &near, None);
    // A ship that has moved into the player this tick (fallen on them, sailed into them) puts them out of the way and not into the ground.
    player.body.keep_clear(&mut blocks.terrain(world, level));

    // (What a planet on a rail carries falls with it: pulled by everything but what the planet goes round, as the ships in its frame are.)
    let gravity = match player.carrier {
        Some(planet) => world.gravity_in_the_free_fall_of(planet, player.body.eye_position()),
        None => world.gravity_at(level, player.body.eye_position()),
    };
    let input = input_for(player);
    let against = speed_of_what_they_left(player, world, level);
    for _ in 0..PHYSICS_STEPS_PER_TICK {
        player
            .body
            .step_with_gravity_against(input, gravity, against);
    }
    // Standing on something (or flying, which is the level's own) there is nothing to measure from but the level.
    if player.body.is_grounded() || player.body.is_flying() {
        player.departure = None;
    }

    // On the ground, on a block of a ship: aboard it from here on.
    if !player.body.is_flying() && player.body.is_grounded() {
        let feet = player.body.feet_position();
        let down = player.body.up().map(|axis| -axis);
        let shared: &ServerWorld = world;
        let ship = near.iter().copied().find(|id| {
            shared.data().grids().get(*id).is_some_and(|ship| {
                stands_on(&mut blocks.source(shared, *id, ship), ship, feet, down)
            })
        });
        if let Some(id) = ship {
            board(player, id, world);
        }
    }
    if player.frame == GridId::root(level) {
        pass_to_next_carrier(player, world, level);
    }
}

/// A body that goes out of the sphere of the planet that carries it, into the sphere of one, or from one into another's, goes on at the speed it had in the world: its speed is
/// against its carrier (see [`Carried`]), so it is changed by the speed of the old carrier less the new one's, or it would lose (or gain) the planet's speed in a tick, as a kink of
/// the planet's own step in its path (measured 2026-11-07: 6.26 voxels, at the edge of a planet that goes at 125.7 metres a second).
///
/// The planet that will carry the body in the next tick is found here, from where the body ends this tick, **as the world finds it**: with the planet that carried the body this
/// tick keeping it a quarter farther out (`ServerWorld::planet_carrying`). So the next tick's `carried_by_rails` agrees. A body that was put this tick has no speed to keep (it is at
/// rest against whatever carries it, and what the world said carried it was for where it was before), and one that stands on something or flies has the speed that the ground and its
/// keys ask for, which is not carried.
fn pass_to_next_carrier(player: &mut PlayerBody, world: &ServerWorld, level: LevelIndex) {
    let next = world.planet_carrying(level, player.body.eye_position(), player.carrier);
    if next == player.carrier {
        return;
    }
    if !player.put && !player.body.is_grounded() && !player.body.is_flying() {
        let before = planet_speed(world, player.carrier);
        let after = planet_speed(world, next);
        player
            .body
            .change_velocity([0, 1, 2].map(|axis| before[axis] - after[axis]));
    }
    player.carrier = next;
}

/// The speed (in the level's axes) of the place of a ship that a body in a level stepped off, which its keys and the air are measured from; nothing if there is no such place
/// (it never left a ship, or the ship is gone or in another level, and then it is forgotten).
fn speed_of_what_they_left(
    player: &mut PlayerBody,
    world: &ServerWorld,
    level: LevelIndex,
) -> [f64; 3] {
    let Some(departure) = player.departure else {
        return [0.0; 3];
    };
    let place = world
        .data()
        .grids()
        .get(departure.ship)
        .filter(|ship| ship.level == level)
        .and_then(|ship| Some((ship, ship.pose.local_to_world(departure.place)?)));
    match place {
        // (The speed of the place is the world's, and the body's is against the planet that carries it, if one does: see [`Carried`].)
        Some((ship, place)) => {
            against_the_planet(ship.velocity_at(place), planet_speed(world, player.carrier))
        }
        None => {
            player.departure = None;
            [0.0; 3]
        }
    }
}

/// Puts a body aboard a ship: the same place and the speed it has relative to the ship, in the ship's coordinates and axes.
fn board(player: &mut PlayerBody, id: GridId, world: &ServerWorld) {
    let Some(ship) = world.data().grids().get(id) else {
        return;
    };
    player.departure = None;
    let eyes = player.body.eye_position();
    let Some(local) = ship.pose.world_to_local(eyes) else {
        return;
    };
    // (The ship's speed is the world's, and the body's is against the planet that carries it, if one does: see [`Carried`].)
    let carried = ship.velocity_at(eyes);
    let speed = in_the_world(player.body.velocity(), planet_speed(world, player.carrier));
    let mut relative = against_the_planet(speed, carried);
    // They stand on it, so they are not moving away from it: a walker that a ship has fallen away from, or has lifted onto itself, is not
    // thrown up at the speed it had. (Moving toward it is the controller's to stop, as it always did.)
    let up = player.body.up();
    let away: f64 = (0..3).map(|axis| relative[axis] * up[axis]).sum();
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
    // (Aboard, nothing carries the body but the ship.)
    player.carrier = None;
    // (What the ship does from here is measured against how it went as the body came aboard.)
    player.motion = Some(Motion::of(id, ship, None));
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
    // The bricks round the walker, if the ship has terrain that is made as it is read (a planet), are made by a worker before they are walked into: the ones
    // the body will want colliders of are within a brick, and a collider is made looking at the voxels beside its brick, which are the next ring.
    let here = player.body.eye_position().brick();
    world.prefetch_ship_bricks(
        id,
        (-2..=2).flat_map(|x| {
            (-1..=1).flat_map(move |y| {
                (-2..=2)
                    .map(move |z| BrickCoordinate::new(here.x() + x, here.y() + y, here.z() + z))
            })
        }),
    );
    // The other ships near the player are colliders too, in the coordinates of the ship they are aboard: a planet is a ship, and a walker on one meets what stands on it.
    let frame = world
        .data()
        .grids()
        .get(id)
        .map(|ship| (ship.level, ship.pose));
    let Some((level, frame_pose)) = frame else {
        // The ship is not there. The record of the player says so next tick and the body is put in the level then.
        return;
    };
    let near: Vec<GridId> = frame_pose
        .local_to_world(player.body.middle_position())
        .map(|here| {
            ships
                .iter()
                .filter(|(other, bounds)| **other != id && in_reach(bounds, level, here.brick()))
                .map(|(other, _)| *other)
                .collect()
        })
        .unwrap_or_default();
    if let Some(here) = frame_pose.local_to_world(player.body.middle_position()) {
        prefetch_planet_ground(world, &near, here);
    }
    {
        let Some(ship) = world.data().grids().get(id) else {
            return;
        };
        let mut hull = blocks.source(world, id, ship);
        for &(grid, brick) in changed {
            if grid == id {
                player.body.brick_changed(&mut hull, brick);
            }
        }
        player.body.update_terrain(&mut hull);
    }
    meet_ships(player, blocks, world, changed, &near, Some(frame_pose));
    {
        let Some(ship) = world.data().grids().get(id) else {
            return;
        };
        player.body.keep_clear(&mut blocks.source(world, id, ship));
    }
    let felt = felt_aboard(player, world, id);
    let input = input_for(player);
    let step = player.body.settings().time_step;
    for _ in 0..PHYSICS_STEPS_PER_TICK {
        // The Coriolis pull, `-2 spin x velocity`, alone turns what the body goes at in the ship's axes at twice the ship's own spin, the other way: an exact turn each step, and
        // not a push at right angles to the speed, which would grow it. (Turned by the ship's own spin only, it is half of it: the other half is the place changing under it.)
        player
            .body
            .turn_velocity(felt.spin.map(|spin| -2.0 * spin * step));
        player.body.step_with_gravity(input, felt.pull);
    }

    // Has it left? Nothing of the ship under it, or far from the ship; or nothing of the ship is left.
    let left = match (world.data().grids().get(id), ships.get(&id)) {
        (Some(ship), Some(bounds)) => {
            let down = player.body.up().map(|axis| -axis);
            has_left(
                &mut blocks.source(world, id, ship),
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
        return;
    }

    // On the ground, on a block of another ship that stands on this one (a ship on a planet): aboard that one from here on, which is how a walker on a planet
    // comes to be aboard a ship that rests on it.
    if !player.body.is_flying() && player.body.is_grounded() {
        let down = player.body.up().map(|axis| -axis);
        let shared: &ServerWorld = world;
        let standing_on = shared.data().grids().get(id).and_then(|frame| {
            let feet = frame.pose.local_to_world(player.body.feet_position())?;
            let down = frame.pose.direction_to_world(down);
            near.iter().copied().find(|other| {
                shared.data().grids().get(*other).is_some_and(|ship| {
                    stands_on(&mut blocks.source(shared, *other, ship), ship, feet, down)
                })
            })
        });
        if let Some(other) = standing_on {
            leave(player, world);
            board(player, other, world);
        }
    }
}

/// A pull aboard a ship weaker than this, in voxels a second a second (a two-hundredth of Cave's), is none: the walker floats.
const FLOATING_BELOW: f64 = 0.05;

/// What a body aboard a ship is moved by in a tick, in the ship's axes.
struct Felt {
    /// What it is pulled by: the pull of the world where its eyes are, less the acceleration of the ship, less the centrifugal and the Euler pull of its turn.
    pull: [f64; 3],
    /// How the ship turns, an axis scaled by radians a second: what the body goes at turns the other way by as much (the Coriolis pull).
    spin: [f64; 3],
}

/// What a body aboard the ship `id` is moved by (see [`Felt`]), from how the ship went since the last tick (see [`crate::frame_terms`]): the acceleration of the ship is the change
/// of the speed of its centre of mass over the length of a tick (a rail gives it exactly, an engine gives its thrust, the ground gives what the ground does), and the change of its
/// turn likewise. So a ship in free fall, on a rail or not, is a place to float; a ship that burns weighs more; one that goes at any steady speed weighs what it did; "up" follows the
/// thrust, because the character's up is against what it is pulled by; and in a ship that spins the floor is held down less the farther it is from the axis.
///
/// **What a planet on a rail carries falls with it** (as `step_in_a_level` has it, and `ServerWorld::gravity_in_the_free_fall_of`): in the sphere of such a planet the pull is
/// everything's but that of what the planet goes round, and the acceleration that is taken off is the ship's less the planet's own, so the planet's rail, which is not a force,
/// is nothing to a ship carried with it. The first tick aboard has nothing to measure from and is moved by the field and the spin alone; so is the tick in which the carrier changes.
fn felt_aboard(player: &mut PlayerBody, world: &ServerWorld, id: GridId) -> Felt {
    let fallback = Felt {
        pull: [0.0, -player.body.settings().gravity, 0.0],
        spin: [0.0; 3],
    };
    let Some(ship) = world.data().grids().get(id) else {
        return fallback;
    };
    let place = player.body.eye_position();
    let Some(eyes) = ship.pose.local_to_world(place) else {
        return fallback;
    };
    let before = player.motion.filter(|motion| motion.ship == id);
    let carrier = world.planet_carrying(
        ship.level,
        eyes,
        before.and_then(|motion| motion.carrier.map(|(planet, _)| planet)),
    );
    let planet_speed = carrier
        .and_then(|planet| world.data().grids().get(planet))
        .map_or([0.0; 3], |planet| planet.linear_velocity);
    player.motion = Some(Motion::of(
        id,
        ship,
        carrier.map(|planet| (planet, planet_speed)),
    ));
    let field = ship.pose.direction_to_local(match carrier {
        Some(planet) => world.gravity_in_the_free_fall_of(planet, eyes),
        None => world.gravity_at(ship.level, eyes),
    });
    // Where the body is from the centre of mass, in the ship's axes.
    let centre = WorldPosition::new(BrickCoordinate::ORIGIN, ship.centre_of_mass);
    let arm = place.relative_to(centre).map(f64::from);
    // Without a tick to measure from the ship is taken to neither accelerate nor change its turn; it turns as it does.
    let measured = before.filter(|before| before.carrier.map(|(planet, _)| planet) == carrier);
    let (acceleration, spin_before) = match measured {
        Some(before) => {
            let before_planet = before.carrier.map_or([0.0; 3], |(_, speed)| speed);
            (
                [0, 1, 2].map(|axis| {
                    ((ship.linear_velocity[axis] - before.velocity[axis])
                        - (planet_speed[axis] - before_planet[axis]))
                        / TICK_SECONDS
                }),
                before.spin,
            )
        }
        None => ([0.0; 3], ship.angular_velocity),
    };
    let motion = frame_motion(
        &ship.pose,
        acceleration,
        ship.angular_velocity,
        spin_before,
        TICK_SECONDS,
    );
    let felt = pull_in_a_frame(field, &motion, arm);
    // In free fall what is left is the tide and the rounding of the speeds, a ten-thousandth of a metre a second a second pointing anywhere, and a body takes the direction of any
    // pull for its down: its floor would turn from tick to tick and put it out of the ship. A pull that nobody could feel is none.
    let strength = (felt[0] * felt[0] + felt[1] * felt[1] + felt[2] * felt[2]).sqrt();
    Felt {
        pull: if strength < FLOATING_BELOW {
            [0.0; 3]
        } else {
            felt
        },
        spin: motion.spin,
    }
}

/// Puts a body that is aboard a ship in the level the ship is in: the same place in the world, with the speed it has in the world,
/// which is what it has in the ship, turned, and the speed the ship gives that place; made against the planet that will carry the body in the level, if one does (see [`Carried`]).
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
    let in_the_world = [0, 1, 2].map(|axis| in_the_ship[axis] + carried[axis]);
    // The planet that carries a place is found as the world finds it for the player's record in the next tick, which was not carried while aboard.
    let carrier = world.planet_carrying(ship.level, world_eyes, None);
    let velocity = against_the_planet(in_the_world, planet_speed(world, carrier));
    let turn =
        Orientation::from_components(player.body.frame_rotation()).unwrap_or(Orientation::IDENTITY);
    let frame_rotation = ship.pose.orientation.compose(turn);
    player
        .body
        .reframe(world_eyes, velocity, frame_rotation.components());
    player.departure = Some(Departure {
        ship: player.frame,
        place: eyes,
    });
    player.frame = GridId::root(ship.level);
    player.level = ship.level;
    player.carrier = carrier;
    player.motion = None;
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
