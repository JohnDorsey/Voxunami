use std::collections::BTreeMap;
use std::sync::Arc;

use entities::{ComponentBag, Entity, EntityError, EntityKindSpec, Hidden, Inventory};
use namespaced_id::NamespacedId;
use protocol::{ClientId, EntityId, Message, PlayerKey};
use server::{Audience, CommandLine, ServerWorld};
use voxel_space::{EntityPose, GridId, LevelIndex, Orientation, WorldPosition};
use world_storage::WorldData;

use crate::messages::{
    LevelChange, PlayerClient, PlayerKeyComponent, PlayerLook, PlayerPlaced, PlayerPose, PoseReport,
};

/// The kind of entity a player is.
pub fn player_kind() -> NamespacedId {
    "player_presence:player"
        .parse()
        .expect("constant identifier is valid")
}

/// How many slots a player's inventory has.
pub const PLAYER_INVENTORY_SLOTS: usize = 9;

/// What a player entity is made of. The game registers it (the `PlayersPlugin` does): the key stays on
/// the server, the connection's number, the look angles and the inventory are told to clients, and the hidden
/// flag is how a player who is away stays in the world without being seen.
pub fn player_kind_spec() -> EntityKindSpec {
    EntityKindSpec::new(player_kind().as_str())
        .component(PlayerKeyComponent(PlayerKey::new([0; 32])), false)
        .component(PlayerClient(ClientId::new(0)), true)
        .component(
            PlayerLook {
                yaw: 0.0,
                pitch: 0.0,
            },
            true,
        )
        .component(Inventory::new(PLAYER_INVENTORY_SLOTS), true)
        .component(Hidden(false), false)
}

/// Which entity is which connected player, and the calls that read and move players through them.
///
/// A player is an entity of the world (see [`player_kind`]); this holds only the table from connection
/// to entity, and a level asked for before a player has an entity. A game's rules hold one (shared, see
/// `SharedPresence`) and pass the world to each call.
#[derive(Debug, Default)]
pub struct Presence {
    entities: BTreeMap<ClientId, EntityId>,
    /// Only players who have asked to move before their first pose; everyone else is in the main level.
    pending_levels: BTreeMap<ClientId, LevelIndex>,
    /// Where the game says players arrive in a level they are sent to, if it says.
    level_arrival: Option<LevelArrival>,
}

/// Where the game puts a player who is sent from one level to another (the arguments, in that order): a place in the world's axes, or `None` to leave them at the
/// place they were at, in the other level.
type ArrivalRule =
    dyn Fn(&ServerWorld, LevelIndex, LevelIndex) -> Option<WorldPosition> + Send + Sync;

#[derive(Clone)]
struct LevelArrival(Arc<ArrivalRule>);

impl std::fmt::Debug for LevelArrival {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("LevelArrival")
    }
}

impl Presence {
    pub fn new() -> Self {
        Self::default()
    }

    /// Says where a player arrives in a level they are sent to (by `/level go` or [`Presence::move_to_level`]): `rule` is given the world, the level they are in
    /// and the level they are sent to, and answers a place in the world's axes, or `None` to leave them where they were. Without a rule they always are: a level is
    /// another world at the same places.
    pub fn set_level_arrival(
        &mut self,
        rule: impl Fn(&ServerWorld, LevelIndex, LevelIndex) -> Option<WorldPosition>
        + Send
        + Sync
        + 'static,
    ) {
        self.level_arrival = Some(LevelArrival(Arc::new(rule)));
    }

    /// A player has connected: if the world holds the player entity of their key from before, they take it
    /// over (it is shown again where it was); otherwise they get one when their first pose arrives.
    pub fn client_joined(&mut self, client: ClientId, world: &mut ServerWorld) {
        let Some(key) = world.player_key(client) else {
            return;
        };
        let kind = player_kind();
        let saved = world
            .entities()
            .iter()
            .find(|entity| {
                entity.kind == kind
                    && entity.components.get::<PlayerKeyComponent>()
                        == Ok(Some(PlayerKeyComponent(key)))
            })
            .map(|entity| entity.id);
        let Some(id) = saved else {
            return;
        };
        self.entities.insert(client, id);
        update(world, id, |world, id| {
            world.set_entity_component(id, &PlayerClient(client))?;
            world.set_entity_component(id, &Hidden(false))
        });
        // The client watches the bricks around its camera, which starts where its program starts it; a player who left somewhere else is
        // told where they are, in the world (aboard a ship, where the ship has them), so that the view goes there.
        self.tell_placed(world, client);
        // A player who left in another level is back in it, and their client does not know.
        let level = self.level_of(world.data(), client);
        if level != LevelIndex::MAIN
            && let Some(level_id) = world.data().level_id(level)
        {
            let level_id = level_id.clone();
            world.send(
                Audience::Client(client),
                LevelChange::kind(),
                LevelChange(level_id).encode(),
            );
        }
    }

    /// Tells a player where they are in the world (a player aboard a ship is where the ship has them), so that their view goes
    /// there. A client only watches the bricks around its camera: a game that moves a player far (a teleport) calls this after
    /// the move, or the player is somewhere their client does not show. Nothing is sent to a player with no pose yet.
    pub fn tell_placed(&self, world: &mut ServerWorld, client: ClientId) {
        if let Some(position) = self.pose_of(world.data(), client).map(|pose| pose.position) {
            world.send(
                Audience::Client(client),
                PlayerPlaced::kind(),
                PlayerPlaced(position).encode(),
            );
        }
    }

    /// A player has gone: their entity stays in the world, and in the save, out of sight.
    pub fn client_left(&mut self, client: ClientId, world: &mut ServerWorld) {
        self.pending_levels.remove(&client);
        if let Some(id) = self.entities.remove(&client) {
            update(world, id, |world, id| {
                world.set_entity_component(id, &Hidden(true))
            });
        }
    }

    /// The last pose the player reported, if they have one: where they are in the world (a player aboard a ship is where the ship
    /// puts them) and the angles they look at, as they gave them.
    pub fn pose_of(&self, world: &WorldData, client: ClientId) -> Option<PlayerPose> {
        let entity = world.entities().get(*self.entities.get(&client)?)?;
        Some(placed_pose(world, entity))
    }

    /// Where the player's eyes are in the world, and the turn of their view there (the turn of their room in the world, and then their
    /// angles): what a ray from their eyes along their view needs. A player aboard a ship has the ship's turn in it.
    pub fn view_in_the_world(
        &self,
        world: &WorldData,
        client: ClientId,
    ) -> Option<(WorldPosition, Orientation)> {
        let entity = world.entities().get(*self.entities.get(&client)?)?;
        let (_, position, orientation) = world_placement(world, entity);
        Some((position, orientation))
    }

    /// The level the player is in: the main level until they move. A player aboard a ship is in the level the ship is in.
    pub fn level_of(&self, world: &WorldData, client: ClientId) -> LevelIndex {
        self.entities
            .get(&client)
            .and_then(|id| world.entities().get(*id))
            .map(|entity| world_placement(world, entity).0)
            .or_else(|| self.pending_levels.get(&client).copied())
            .unwrap_or(LevelIndex::MAIN)
    }

    /// The frame the player is in: the root grid of their level, or the ship they are aboard.
    pub fn frame_of(&self, world: &WorldData, client: ClientId) -> Option<GridId> {
        let entity = world.entities().get(*self.entities.get(&client)?)?;
        Some(entity.pose.grid)
    }

    /// Where the player is in their own frame: the frame, the pose in it (the position in the frame's own coordinates, and the angles
    /// the player gave), and the orientation of their view there, which is the turn of their body in the frame and then their
    /// angles. For a player in a level that is their pose in the level.
    pub fn local_pose_of(
        &self,
        world: &WorldData,
        client: ClientId,
    ) -> Option<(GridId, PlayerPose, Orientation)> {
        let entity = world.entities().get(*self.entities.get(&client)?)?;
        Some((
            entity.pose.grid,
            player_pose(entity),
            entity.pose.orientation,
        ))
    }

    /// Puts a player at a pose, in the level they are in. For a game that moves players itself instead of
    /// taking their word for where they are (see [`Presence::handle_message`]). The first pose a player
    /// has makes their entity, unless they took over one that was saved.
    pub fn set_pose(&mut self, world: &mut ServerWorld, client: ClientId, pose: PlayerPose) {
        let level = self.level_of(world.data(), client);
        let orientation = Orientation::from_yaw_pitch(pose.yaw, pose.pitch);
        self.set_pose_in(world, client, GridId::root(level), pose, orientation)
            .expect("a level's root grid is a frame");
    }

    /// Puts a player at a pose in a frame: the root grid of a level, or a ship (an error if there is no such ship, and nothing
    /// changes). `pose` has the position in the frame's own coordinates and the angles the player gave; `orientation` is the turn of
    /// their view in the frame (for a player standing upright in a level, the one the angles make). The first pose a player has makes
    /// their entity, unless they took over one that was saved.
    pub fn set_pose_in(
        &mut self,
        world: &mut ServerWorld,
        client: ClientId,
        frame: GridId,
        pose: PlayerPose,
        orientation: Orientation,
    ) -> Result<(), EntityError> {
        let entity_pose = EntityPose::new(frame, pose.position, orientation);
        let look = PlayerLook {
            yaw: pose.yaw,
            pitch: pose.pitch,
        };
        match self.entities.get(&client).copied() {
            Some(id) => {
                let unchanged = world.entities().get(id).is_some_and(|entity| {
                    entity.pose == entity_pose && player_pose(entity) == pose
                });
                if unchanged {
                    return Ok(());
                }
                world.set_entity_pose(id, entity_pose)?;
                update(world, id, |world, id| world.set_entity_component(id, &look));
            }
            None => {
                let Some(key) = world.player_key(client) else {
                    return Ok(()); // not a connected client: nothing to make a player of
                };
                let mut components = ComponentBag::new();
                components.set(&PlayerKeyComponent(key));
                components.set(&PlayerClient(client));
                components.set(&look);
                match world.spawn_entity(&player_kind(), entity_pose, components) {
                    Ok(id) => {
                        self.pending_levels.remove(&client);
                        self.entities.insert(client, id);
                    }
                    Err(EntityError::NoSuchGrid(grid)) => {
                        return Err(EntityError::NoSuchGrid(grid));
                    }
                    Err(error) => panic!("the player kind is not registered: {error}"),
                }
            }
        }
        Ok(())
    }

    /// Puts a player in a level and tells them so. A game calls this to move players itself (a portal, a
    /// teleport); the `/level go` command is built on it.
    pub fn move_to_level(&mut self, client: ClientId, level: LevelIndex, world: &mut ServerWorld) {
        let Some(id) = world.data().level_id(level) else {
            return; // not a level of this world: nowhere to go
        };
        let id = id.clone();
        match self.entities.get(&client).copied() {
            Some(entity) => {
                let Some(current) = world.entities().get(entity) else {
                    return;
                };
                // A player aboard a ship leaves it: they are where they are in the world, in the other level, or where the game puts them there.
                let (from, position, orientation) = world_placement(world.data(), current);
                let position = self
                    .level_arrival
                    .as_ref()
                    .and_then(|arrival| (arrival.0)(world, from, level))
                    .unwrap_or(position);
                let pose = EntityPose::new(GridId::root(level), position, orientation);
                update(world, entity, |world, entity| {
                    world.set_entity_pose(entity, pose)
                });
            }
            None => {
                self.pending_levels.insert(client, level);
            }
        }
        world.send(
            Audience::Client(client),
            LevelChange::kind(),
            LevelChange(id).encode(),
        );
    }

    /// Every connected player who has a pose, in the order of their connections, with the pose (where they are in the world: a
    /// player aboard a ship is where the ship puts them) and the level they are in.
    pub fn players(&self, world: &WorldData) -> Vec<(ClientId, PlayerPose, LevelIndex)> {
        self.entities
            .iter()
            .filter_map(|(&client, &id)| {
                let entity = world.entities().get(id)?;
                Some((
                    client,
                    placed_pose(world, entity),
                    world_placement(world, entity).0,
                ))
            })
            .collect()
    }

    /// The entity that is this connection's player, if they have one.
    pub fn entity_of(&self, client: ClientId) -> Option<EntityId> {
        self.entities.get(&client).copied()
    }

    /// How many connected players have a pose.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Takes in a pose message. Returns whether the message was one (a malformed pose counts: it is
    /// dropped), so a game can pass on what is not.
    pub fn handle_message(
        &mut self,
        world: &mut ServerWorld,
        client: ClientId,
        kind: &NamespacedId,
        payload: &[u8],
    ) -> bool {
        if *kind != PoseReport::kind() {
            return false;
        }
        if let Ok(PoseReport(pose)) = PoseReport::decode(payload) {
            self.set_pose(world, client, pose);
        }
        true
    }

    /// Takes in the `/level` command, which tells a player which level they are in and moves them:
    /// `/level` (where am I), `/level list`, `/level go NAME`, where NAME is a level's identifier
    /// or just the part after the colon. Replies to the player. Returns whether the command was
    /// one, so a game can pass on what is not.
    pub fn handle_command(
        &mut self,
        client: ClientId,
        command: &str,
        world: &mut ServerWorld,
    ) -> bool {
        let Some(line) = CommandLine::parse(command) else {
            return false;
        };
        if line.name != "level" {
            return false;
        }
        self.level_command(client, &line.arguments, world);
        true
    }

    /// The `/level` command, given the words after its name: see [`Presence::handle_command`].
    pub fn level_command(&mut self, client: ClientId, arguments: &[&str], world: &mut ServerWorld) {
        let here = self.level_of(world.data(), client);
        let name_of = |world: &ServerWorld, level: LevelIndex| {
            world
                .data()
                .level_id(level)
                .map_or_else(|| level.to_string(), ToString::to_string)
        };
        match arguments {
            [] => {
                let text = format!("you are in {}", name_of(world, here));
                world.reply(client, &text);
            }
            ["list"] => {
                let levels: Vec<String> = world
                    .data()
                    .levels()
                    .map(|(level, id, _)| {
                        if level == here {
                            format!("{id} (you are here)")
                        } else {
                            id.to_string()
                        }
                    })
                    .collect();
                world.reply(client, &format!("levels: {}", levels.join(", ")));
            }
            ["go", wanted] => match find_level(world, wanted) {
                Ok(level) if level == here => {
                    let text = format!("you are already in {}", name_of(world, level));
                    world.reply(client, &text);
                }
                Ok(level) => {
                    self.move_to_level(client, level, world);
                    let text = format!("you are now in {}", name_of(world, level));
                    world.reply(client, &text);
                }
                Err(problem) => world.reply(client, &problem),
            },
            _ => world.reply(client, "usage: /level, /level list, /level go NAME"),
        }
    }
}

/// Runs an edit of a player's entity. The entity is one the table made or found, so an edit that fails is a
/// mistake in the game's catalog (a kind without a component), which is loud.
fn update(
    world: &mut ServerWorld,
    id: EntityId,
    edit: impl FnOnce(&mut ServerWorld, EntityId) -> Result<(), EntityError>,
) {
    if let Err(error) = edit(world, id) {
        panic!("a player entity cannot be updated: {error}");
    }
}

/// Where an entity is in the world: the level it is in, and its position and orientation there. An entity in the frame of a ship is
/// where the ship puts it, whether the ship is in memory or in a file (the world knows where it was put away). (One in the frame of
/// a ship that the world does not know, which it does not allow, is taken to be in the main level at the coordinates it has: nothing
/// better is known.)
fn world_placement(world: &WorldData, entity: &Entity) -> (LevelIndex, WorldPosition, Orientation) {
    let pose = entity.pose;
    if let Some(level) = pose.grid.root_level() {
        return (level, pose.position, pose.orientation);
    }
    world
        .grids()
        .frame_of(pose.grid)
        .and_then(|(level, ship)| {
            let (position, orientation) = ship.place_in_world(pose.position, pose.orientation)?;
            Some((level, position, orientation))
        })
        .unwrap_or((LevelIndex::MAIN, pose.position, pose.orientation))
}

/// A player entity's pose with its position in the world.
fn placed_pose(world: &WorldData, entity: &Entity) -> PlayerPose {
    PlayerPose {
        position: world_placement(world, entity).1,
        ..player_pose(entity)
    }
}

/// A player entity's pose as the games think of it: where, and the angles the player reported.
fn player_pose(entity: &Entity) -> PlayerPose {
    let look = entity
        .components
        .get::<PlayerLook>()
        .ok()
        .flatten()
        .unwrap_or(PlayerLook {
            yaw: 0.0,
            pitch: 0.0,
        });
    PlayerPose {
        position: entity.pose.position,
        yaw: look.yaw,
        pitch: look.pitch,
    }
}

/// The level a player asked for by name: an identifier, or the part after the colon if only one
/// level has it.
fn find_level(world: &ServerWorld, wanted: &str) -> Result<LevelIndex, String> {
    if let Ok(id) = wanted.parse::<NamespacedId>()
        && let Some(level) = world.level_index(&id)
    {
        return Ok(level);
    }
    let matching: Vec<LevelIndex> = world
        .data()
        .levels()
        .filter(|(_, id, _)| id.name() == wanted)
        .map(|(level, _, _)| level)
        .collect();
    match matching[..] {
        [level] => Ok(level),
        [] => Err(format!(
            "there is no level {wanted}; /level list shows them"
        )),
        _ => Err(format!(
            "{wanted} could mean several levels; use the whole name"
        )),
    }
}
