//! `/ship`: a player makes a ship of what they built, and pushes it, holds it still, and pilots it. (Ships are made by the engine from
//! voxels of the terrain; which voxels, and what a pilot's keys do, are Cave's. Until there is a command chair these commands are how a
//! ship comes to be and to fly. See `docs/moving-grids-spec.md`, 2.7.)

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use block_models::BlockTable;
use game_shell::GameContent;
use grid_bodies::collision::BlockCollisions;
use namespaced_id::NamespacedId;
use physics::Collision;
use player_presence::{Presence, SharedPresence, lock};
use plugin_host::{
    Argument, Arguments, CommandSpec, HandlerSpec, Phase, PlayerLeft, Plugin, Registrar, SystemSpec,
};
use protocol::ClientId;
use server::{GridView, LogoutPolicy, ServerWorld};
use voxel_grid::ChannelIndex;
use voxel_space::{
    BrickCoordinate, GridId, GridPose, LevelIndex, Orientation, VoxelCoordinate, WorldPosition,
};
use world_storage::PilotLogout;
use world_view::cast_ray;

use super::movement::{SharedMovement, locked};
use crate::aboard::{Extent, in_voxels};
use crate::content::CaveContent;
use crate::ships::{
    LOOK_REACH, MAXIMUM_BLOCKS, NEAR, PilotKeys, TICK_SECONDS, connected_blocks, pilot_force,
};

const USAGE: &str = "usage: /ship create [all] | push X Y Z | hold on|off | pilot | vanish on|off";

/// The `/ship` command, and the system that flies the ships that have pilots.
#[derive(Debug)]
pub struct ShipCommandsPlugin {
    ships: Arc<Ships>,
}

#[derive(Debug)]
struct Ships {
    presence: SharedPresence,
    movement: SharedMovement,
    /// Who pilots which ship.
    pilots: Mutex<BTreeMap<ClientId, GridId>>,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    table: BlockTable,
}

impl ShipCommandsPlugin {
    pub fn new(content: &CaveContent, players: SharedPresence, movement: SharedMovement) -> Self {
        Self {
            ships: Arc::new(Ships {
                presence: players,
                movement,
                pilots: Mutex::new(BTreeMap::new()),
                block_channel: content.block_channel(),
                state_channel: content.state_channel(),
                table: content.block_table(),
            }),
        }
    }
}

/// What a ship is called to a player: its number in the order ships were made.
fn number(ship: GridId) -> u64 {
    ship.get() - GridId::FIRST_ALLOCATED
}

impl Plugin for ShipCommandsPlugin {
    fn id(&self) -> NamespacedId {
        "voxunami_cave:ships"
            .parse()
            .expect("constant identifier is valid")
    }

    fn depends_on(&self) -> Vec<NamespacedId> {
        [
            "player_presence:players",
            "voxunami_cave:movement",
            "engine:grid_bodies",
        ]
        .map(|id| id.parse().expect("constant identifier is valid"))
        .to_vec()
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        let ships = Arc::clone(&self.ships);
        registrar.command(
            CommandSpec::new(
                "ship",
                "makes ships of what you built, and pushes, holds, pilots and puts them away",
            )
            .argument(Argument::word("action"))
            .argument(Argument::rest("details").optional()),
            move |context, client, arguments| {
                let reply = ships.command(context.world(), client, arguments);
                context.reply(client, &reply);
            },
        );

        // The pilots' keys push their ships before the ships move, so that this tick's keys are in this tick's motion.
        let ships = Arc::clone(&self.ships);
        registrar.system(
            SystemSpec::new("voxunami_cave:pilots_fly_ships", Phase::Physics)
                .before("engine:grid_bodies_move"),
            move |context| ships.fly(context.world()),
        );

        // A pilot who leaves stops piloting, and their ship may go with them. (After the players' own handler has taken the pilot out of the record
        // of who is where, so that the ship finds only the others who are aboard.)
        let ships = Arc::clone(&self.ships);
        registrar.on::<PlayerLeft>(
            HandlerSpec::new("voxunami_cave:a_player_who_leaves_stops_piloting")
                .after("player_presence:hide_players_who_leave"),
            move |event, context| {
                let piloted = ships.pilots().remove(&event.client);
                if let Some(ship) = piloted {
                    ships.pilot_left(context.world(), ship);
                }
            },
        );
    }
}

impl Ships {
    fn pilots(&self) -> MutexGuard<'_, BTreeMap<ClientId, GridId>> {
        self.pilots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn command(
        &self,
        world: &mut ServerWorld,
        client: ClientId,
        arguments: &Arguments<'_>,
    ) -> String {
        let details = arguments.rest(1);
        match (arguments.word(0), details) {
            ("create", []) => self.create(world, client, false),
            ("create", ["all"]) => self.create(world, client, true),
            ("push", [x, y, z]) => match [x, y, z].map(|word| word.parse::<f32>()) {
                [Ok(x), Ok(y), Ok(z)] if [x, y, z].iter().all(|value| value.is_finite()) => {
                    self.push(world, client, [x, y, z])
                }
                _ => "usage: /ship push X Y Z (the change of speed, in voxels a second)".to_owned(),
            },
            ("hold", ["on"]) => self.hold(world, client, true),
            ("hold", ["off"]) => self.hold(world, client, false),
            ("pilot", []) => self.pilot(world, client),
            ("vanish", ["on"]) => self.vanish(world, client, PilotLogout::Vanish),
            ("vanish", ["off"]) => self.vanish(world, client, PilotLogout::Stay),
            _ => USAGE.to_owned(),
        }
    }

    /// Whether a voxel of a level is something a ship is made of: a block bodies cannot pass through. `None` where the world cannot say.
    fn solid(
        &self,
        world: &mut ServerWorld,
        level: LevelIndex,
        collisions: &BlockCollisions,
        voxel: VoxelCoordinate,
    ) -> Option<bool> {
        let mut terrain = world.grid(GridId::root(level)).ok()?;
        let block = terrain.get(self.block_channel, voxel).ok()?;
        let state = terrain.get(self.state_channel, voxel).ok()?;
        Some(collisions.of(block, state) != Collision::Empty)
    }

    /// Whether the block at a voxel is not what the terrain made there: someone built it.
    fn built(&self, world: &mut ServerWorld, level: LevelIndex, voxel: VoxelCoordinate) -> bool {
        let Ok(mut terrain) = world.grid(GridId::root(level)) else {
            return false;
        };
        match (
            terrain.get(self.block_channel, voxel),
            terrain.generated(self.block_channel, voxel),
        ) {
            (Ok(now), Ok(made)) => now != made,
            _ => false,
        }
    }

    /// `/ship create`: the blocks joined to the one the player looks at, that they built (or any, with `all`), become a ship where they are.
    fn create(&self, world: &mut ServerWorld, client: ClientId, everything: bool) -> String {
        let (eyes, view, level) = {
            let presence = lock(&self.presence);
            let Some((eyes, view)) = presence.view_in_the_world(world.data(), client) else {
                return "you have not joined yet".to_owned();
            };
            (eyes, view, presence.level_of(world.data(), client))
        };
        let nowhere = WorldPosition::new(BrickCoordinate::ORIGIN, [0.0; 3]);
        let direction = GridPose::new(nowhere, view).direction_to_world([0.0, 0.0, -1.0]);
        let collisions = BlockCollisions::new(
            self.table.clone(),
            Some(crate::block_properties::passable(&self.table)),
        );
        let Some(hit) = cast_ray(GridId::root(level), eyes, direction, LOOK_REACH, |voxel| {
            self.solid(world, level, &collisions, voxel)
        }) else {
            return "you are not looking at a block".to_owned();
        };
        let blocks = connected_blocks(hit.voxel, MAXIMUM_BLOCKS, |voxel| {
            self.solid(world, level, &collisions, voxel) == Some(true)
                && (everything || self.built(world, level, voxel))
        });
        let Some(blocks) = blocks else {
            return format!(
                "more than {MAXIMUM_BLOCKS} blocks are joined to that one{}",
                if everything {
                    ""
                } else {
                    " (counting only what has been built)"
                }
            );
        };
        if blocks.is_empty() {
            return "that was not built by anyone; /ship create all takes what is there, ground and all"
                .to_owned();
        }
        match world.detach_blocks(GridId::root(level), &blocks) {
            Ok(ship) => format!(
                "made ship {} of {} blocks (it falls unless you /ship hold on)",
                number(ship),
                blocks.len()
            ),
            Err(error) => format!("could not make a ship: {error}"),
        }
    }

    /// The ship a command is about: the one the player is aboard, else the nearest to them within [`NEAR`].
    fn target(
        &self,
        world: &ServerWorld,
        presence: &Presence,
        client: ClientId,
    ) -> Result<GridId, String> {
        if let Some(frame) = presence.frame_of(world.data(), client)
            && frame.root_level().is_none()
        {
            return Ok(frame);
        }
        let Some((eyes, _)) = presence.view_in_the_world(world.data(), client) else {
            return Err("you have not joined yet".to_owned());
        };
        let level = presence.level_of(world.data(), client);
        let mut nearest: Option<(f64, GridId)> = None;
        for (id, ship) in world.data().grids().iter() {
            let (Some(extent), Some(here)) = (Extent::of(ship), ship.pose.world_to_local(eyes))
            else {
                continue;
            };
            let distance = extent.distance_to(in_voxels(here));
            if ship.level == level
                && distance <= NEAR
                && nearest.is_none_or(|(best, _)| distance < best)
            {
                nearest = Some((distance, id));
            }
        }
        nearest
            .map(|(_, id)| id)
            .ok_or_else(|| "no ship is near; /ship create makes one of what you look at".to_owned())
    }

    /// `/ship push`: changes the ship's speed by this much, with a force for one tick.
    fn push(&self, world: &mut ServerWorld, client: ClientId, change: [f32; 3]) -> String {
        let ship = match self.target(world, &lock(&self.presence), client) {
            Ok(ship) => ship,
            Err(problem) => return problem,
        };
        let Some(moving) = world.data().grids().get(ship) else {
            return "that ship is gone".to_owned();
        };
        if moving.held {
            return format!("ship {} is held; /ship hold off lets it go", number(ship));
        }
        let mass = moving.mass;
        if mass <= 0.0 {
            return format!(
                "ship {} has no weight yet; try again in a moment",
                number(ship)
            );
        }
        let force = change.map(|speed| mass * speed / TICK_SECONDS);
        match world.apply_grid_force(ship, force, None) {
            Ok(()) => format!("pushed ship {}", number(ship)),
            Err(_) => "that ship is gone".to_owned(),
        }
    }

    /// `/ship hold`: the ship stays where it is (on) or is let go to fall and fly (off).
    fn hold(&self, world: &mut ServerWorld, client: ClientId, held: bool) -> String {
        let ship = match self.target(world, &lock(&self.presence), client) {
            Ok(ship) => ship,
            Err(problem) => return problem,
        };
        match world.set_grid_held(ship, held) {
            Ok(()) if held => format!("ship {} is held", number(ship)),
            Ok(()) => format!("ship {} is let go", number(ship)),
            Err(_) => "that ship is gone".to_owned(),
        }
    }

    /// `/ship vanish`: whether the ship goes away with its pilot when they log out, and is where it was when they come back. (Until there is a
    /// command chair, which will say this, this is how a ship is told.)
    fn vanish(&self, world: &mut ServerWorld, client: ClientId, wish: PilotLogout) -> String {
        let ship = match self.target(world, &lock(&self.presence), client) {
            Ok(ship) => ship,
            Err(problem) => return problem,
        };
        if world.set_grid_on_pilot_logout(ship, wish).is_err() {
            return "that ship is gone".to_owned();
        }
        match wish {
            PilotLogout::Vanish => format!(
                "ship {} will go away with its pilot when they log out, and be where it was when they come back",
                number(ship)
            ),
            PilotLogout::Stay => format!(
                "ship {} will stay where it is when its pilot logs out",
                number(ship)
            ),
        }
    }

    /// The pilot of a ship has left (and is out of the record of who is where). The ship goes with them if the host says ships do, or leaves it
    /// to the ship and the ship says it does. If someone else is aboard, who would lose their floor, it stays, held, for the pilot to find.
    fn pilot_left(&self, world: &mut ServerWorld, ship: GridId) {
        let Some(moving) = world.data().grids().get(ship) else {
            return;
        };
        let (wish, linear, angular) = (
            moving.on_pilot_logout,
            moving.linear_velocity,
            moving.angular_velocity,
        );
        let goes = match world.ship_settings().logout_policy {
            LogoutPolicy::Chair => wish == PilotLogout::Vanish,
            LogoutPolicy::AlwaysStay => false,
            LogoutPolicy::AlwaysVanish => true,
        };
        if !goes {
            return;
        }
        let someone_else_is_aboard = {
            let presence = lock(&self.presence);
            presence
                .players(world.data())
                .into_iter()
                .any(|(client, ..)| presence.frame_of(world.data(), client) == Some(ship))
        };
        if someone_else_is_aboard {
            let _ = world.set_grid_held(ship, true);
            return;
        }
        // It stops, and waits in its file for whoever is aboard it (the pilot, whose place in it is kept). If it cannot be put in a file
        // (the world has nowhere to keep it, or the disk fails) it stays, as a ship that does not vanish does.
        let _ = world.set_grid_velocity(ship, [0.0; 3], [0.0; 3]);
        if world.park_ship(ship, true).is_err() {
            let _ = world.set_grid_velocity(ship, linear, angular);
        }
    }

    /// `/ship pilot`: the player's keys fly the ship they are aboard, until they say so again, leave it, or leave.
    fn pilot(&self, world: &mut ServerWorld, client: ClientId) -> String {
        let presence = lock(&self.presence);
        let mut pilots = self.pilots();
        if let Some(ship) = pilots.remove(&client) {
            locked(&self.movement).set_piloting(client, false);
            return format!("you let go of the controls of ship {}", number(ship));
        }
        let Some(ship) = presence
            .frame_of(world.data(), client)
            .filter(|frame| frame.root_level().is_none())
        else {
            return "stand on a ship to pilot it".to_owned();
        };
        if pilots.values().any(|piloted| *piloted == ship) {
            return format!("someone else is piloting ship {}", number(ship));
        }
        pilots.insert(client, ship);
        locked(&self.movement).set_piloting(client, true);
        format!(
            "you pilot ship {}: W A S D fly it the way you look, space and shift go up and down, control goes faster; /ship pilot again lets go",
            number(ship)
        )
    }

    /// Each tick: every pilot's keys push the ship. A pilot who is no longer aboard it, or whose ship is gone, stops piloting.
    fn fly(&self, world: &mut ServerWorld) {
        let presence = lock(&self.presence);
        let mut pilots = self.pilots();
        if pilots.is_empty() {
            return;
        }
        let mut movement = locked(&self.movement);
        // A ship that was cut apart is gone, and its pilot has the part that they are aboard.
        for (client, ship) in pilots.iter_mut() {
            if world.data().grids().get(*ship).is_none()
                && let Some(frame) = presence.frame_of(world.data(), *client)
                && world
                    .data()
                    .grids()
                    .resolve(*ship)
                    .iter()
                    .any(|(part, _)| *part == frame)
            {
                *ship = frame;
            }
        }
        let mut finished = Vec::new();
        for (&client, &ship) in pilots.iter() {
            let aboard = presence.frame_of(world.data(), client) == Some(ship);
            let (Some(moving), true) = (world.data().grids().get(ship), aboard) else {
                finished.push(client);
                continue;
            };
            let (Some(request), Some((_, pose, orientation))) = (
                movement.request_of(client),
                presence.local_pose_of(world.data(), client),
            ) else {
                finished.push(client);
                continue;
            };
            // The way they look is what their keys say now, in the turn of the room they are in (the record of their view is a tick behind).
            let view = moving
                .pose
                .orientation
                .compose(pose.frame_rotation_of(orientation))
                .compose(Orientation::from_yaw_pitch(
                    request.input.yaw,
                    request.pitch,
                ));
            let (mass, velocity, level) = (moving.mass, moving.linear_velocity, moving.level);
            let centre = moving
                .pose
                .local_to_world(WorldPosition::new(
                    BrickCoordinate::ORIGIN,
                    moving.centre_of_mass,
                ))
                .unwrap_or(moving.pose.position);
            let gravity = world.gravity_at(level, centre);
            let keys = PilotKeys {
                forward: request.input.forward,
                strafe: request.input.strafe,
                vertical: request.input.vertical,
                sprint: request.input.sprint,
            };
            let force = pilot_force(mass, gravity, velocity, keys, view);
            let _ = world.apply_grid_force(ship, force, None);
        }
        for client in finished {
            pilots.remove(&client);
            movement.set_piloting(client, false);
        }
    }
}
