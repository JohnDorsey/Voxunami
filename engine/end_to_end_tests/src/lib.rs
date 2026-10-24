//! The pieces of an end-to-end scenario: a server running a small pretend game, players that join it
//! (live, recorded, or as a replay of a recording), and the ways to compare what they saw.
//!
//! The pretend game: players place blocks by name in a level (`test_game:place_block`), and the main
//! level also changes by itself (a block appears at a computed place every fifth tick). The game tells players
//! near a placement about it, and everyone the time every tenth tick.

use std::collections::HashMap;
use std::io::{Cursor, Write};
use std::sync::{Arc, Mutex};

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use client::{Client, ClientError, Handshake, HandshakeProgress, TickUpdate, ViewSummary};
use content_registry::IdentifierRegistry;
use entities::{ComponentBag, EntityCatalog, EntityKindSpec};
use identity::Identity;
use namespaced_id::NamespacedId;
use protocol::{EntityId, Message, Tick};
use replay::{RecordingConnection, ReplayConnection};
use server::test_support::ScriptedLogic;
use server::{Audience, GridView, Server, ServerConfig, ServerWorld};
use transport::Connection;
use transport::loopback::{LoopbackConnection, LoopbackConnector, loopback_listener};
use voxel_grid::Brick;
use voxel_space::{
    BrickBox, BrickCoordinate, EntityPose, GridId, GridPose, LevelIndex, Orientation,
    VoxelCoordinate, WorldPosition,
};
use world_storage::test_support::{ALL_PLUGINS, Game, Plugin, id};
use world_storage::{BrickSource, LevelData, WorldData};

pub fn game_id() -> NamespacedId {
    id("test_game:game")
}

fn place_block_kind() -> NamespacedId {
    id("test_game:place_block")
}

/// The kind of the game's own messages, for tests that look at them.
pub fn block_placed_kind() -> NamespacedId {
    id("test_game:block_placed")
}

pub fn time_kind() -> NamespacedId {
    id("test_game:time")
}

/// How brightly a wisp glows: the one component of the pretend game's one kind of entity, which clients
/// are told of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glow(pub u32);

impl Message for Glow {
    fn kind() -> NamespacedId {
        id("test_game:glow")
    }
    fn encode(&self) -> Vec<u8> {
        self.0.to_le_bytes().to_vec()
    }
    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let value = reader.read_u32()?;
        reader.finish()?;
        Ok(Self(value))
    }
    fn samples() -> Vec<Self> {
        vec![Self(0), Self(u32::MAX)]
    }
}

/// The kind of the pretend game's entities, when it has a herd (see [`Fixture::with_herd`]).
pub fn wisp_kind() -> NamespacedId {
    id("test_game:wisp")
}

/// Where a wisp is `step` ticks after it was made at `start`: it drifts along x, crossing bricks.
fn wisp_pose(start: i64, step: u64) -> EntityPose {
    EntityPose::new(
        GridId::MAIN,
        WorldPosition::new(
            BrickCoordinate::new(start + (step * 3 / 32) as i64, 0, 0),
            [(step * 3 % 32) as f32, 5.0, 5.0],
        ),
        Orientation::IDENTITY,
    )
}

/// Where the wisp that rides the ship of [`Fixture::with_ship`] is in it at this tick: in the frame of the ship.
pub fn rider_pose(ship: GridId, count: u64) -> EntityPose {
    EntityPose::new(
        ship,
        WorldPosition::new(
            BrickCoordinate::new(0, 0, 0),
            [2.0 + (count % 7) as f32, 3.0, 2.0],
        ),
        Orientation::from_yaw_pitch(0.3, 0.0),
    )
}

/// The ship of [`Fixture::with_ship`] at this tick.
fn ship_at_tick(count: u64, world: &mut ServerWorld, marble: u32, glass: u32) {
    let ship = GridId::new(GridId::FIRST_ALLOCATED);
    let block = world
        .data()
        .main()
        .overrides
        .layout()
        .channel_index(&id("core:block"))
        .unwrap();
    let pose = |count: u64| {
        let along = count as i64 - 8;
        GridPose::new(
            WorldPosition::new(BrickCoordinate::new(0, 0, 0), [0.0, 4.0, 0.0]).translated([
                along as f32,
                0.0,
                0.0,
            ]),
            Orientation::from_yaw_pitch(0.1 * along as f32, 0.0),
        )
    };
    match count {
        8 => {
            let made = world.create_grid(LevelIndex::MAIN, pose(count)).unwrap();
            assert_eq!(made, ship);
            let mut view = world.grid(ship).unwrap();
            for at in [[1, 1, 1], [2, 1, 1], [40, 3, 3]] {
                view.set(block, VoxelCoordinate::new(at[0], at[1], at[2]), marble)
                    .unwrap();
            }
        }
        9..=49 => {
            if count == 9 {
                let rider = world
                    .spawn_entity(&wisp_kind(), rider_pose(ship, 0), ComponentBag::new())
                    .expect("the kind is declared");
                assert_eq!(rider, EntityId::new(1));
            } else if count.is_multiple_of(4) {
                world
                    .set_entity_pose(EntityId::new(1), rider_pose(ship, count))
                    .expect("the rider is aboard");
            }
            world.set_grid_pose(ship, pose(count)).unwrap();
            if count.is_multiple_of(6) {
                let mut view = world.grid(ship).unwrap();
                let at = VoxelCoordinate::new(count as i64 % 5, 2, 2);
                view.set(block, at, glass).unwrap();
                if count.is_multiple_of(12) {
                    // A voxel in a brick the ship did not have.
                    view.set(block, VoxelCoordinate::new(-20, 1, count as i64), glass)
                        .unwrap();
                }
            }
        }
        50 => {
            assert!(world.remove_grid(ship).is_some());
        }
        _ => {}
    }
}

/// The planet of [`Fixture::with_planet`] at this tick: edits to bricks it generates (the first one stores a brick), more to the brick that is stored, one to a
/// brick that was stored from the start, and then it goes.
fn planet_at_tick(count: u64, world: &mut ServerWorld, marble: u32, glass: u32) {
    let planet = GridId::new(GridId::FIRST_ALLOCATED);
    let block = world
        .data()
        .main()
        .overrides
        .layout()
        .channel_index(&id("core:block"))
        .unwrap();
    let edit = |world: &mut ServerWorld, at: [i64; 3], value: u32| {
        world
            .grid(planet)
            .unwrap()
            .set(block, VoxelCoordinate::new(at[0], at[1], at[2]), value)
            .unwrap();
    };
    match count {
        // In bricks the generator makes, so each edit stores a whole brick.
        8 => edit(world, [100, 41, -3], marble),
        10 | 12 => edit(world, [100 + count as i64, 41, -3], glass),
        // In the brick that is stored, and in one of the two the planet was made with.
        14 => edit(world, [101, 42, -3], glass),
        16 => edit(world, [2, 1, 1], glass),
        20 => edit(world, [-70, 30, 90], marble),
        40 => {
            assert!(world.remove_grid(planet).is_some());
        }
        _ => {}
    }
}

/// A running server with the pretend game, and a way for players to connect to it.
#[derive(Debug)]
pub struct Fixture {
    pub server: Server,
    pub game: Game,
    connector: LoopbackConnector,
    /// How many players have joined: each gets an identity of its own, the same in every run.
    joined: u8,
}

impl Fixture {
    /// A server for an empty world of the full pretend game.
    pub fn new() -> Self {
        let game = Game::new(&ALL_PLUGINS);
        let world = game.new_world();
        Self::with_world(game, world)
    }

    /// A server for an empty world of the pretend game, with a herd of wisps: from tick 7 on, one is made
    /// every 7th tick (up to six) in a brick that depends on the tick, each drifts along x every tick,
    /// the oldest is removed every 11th tick, and every 3rd tick the glow of all of them is set.
    pub fn with_herd() -> Self {
        let game = Game::new(&ALL_PLUGINS);
        let world = game.new_world();
        Self::build(game, world, true, false)
    }

    /// A server for an empty world of the pretend game, with a ship: made on tick 8 with two bricks, turning a little more and moving
    /// one voxel along x every tick, edited every 6th tick (a voxel of a brick it has, and every 12th tick one in a new brick), and
    /// removed on tick 50. A wisp rides it from tick 9 (entity 1), moving about in it every 4th tick; when the ship goes the wisp is in the
    /// level where the ship had it.
    pub fn with_ship() -> Self {
        let game = Game::new(&ALL_PLUGINS);
        let world = game.new_world();
        Self::build(game, world, false, true)
    }

    /// A server for a world that has a planet from the start (the ship of [`Game::sample_planet`]: terrain, and two bricks that are edits), edited
    /// on ticks 8 to 20 (see `planet_at_tick`) and removed on tick 40.
    pub fn with_planet() -> Self {
        let game = Game::new(&ALL_PLUGINS);
        let mut world = game.new_world();
        let planet = game.sample_planet(&world, 0);
        let made = world.grids_mut().create(planet);
        assert_eq!(made, GridId::new(GridId::FIRST_ALLOCATED));
        Self::build_with(game, world, false, false, true)
    }

    pub fn with_world(game: Game, world: WorldData) -> Self {
        Self::build(game, world, false, false)
    }

    fn build(game: Game, world: WorldData, herd: bool, ship: bool) -> Self {
        Self::build_with(game, world, herd, ship, false)
    }

    fn build_with(game: Game, world: WorldData, herd: bool, ship: bool, planet: bool) -> Self {
        let block_ids: HashMap<NamespacedId, u32> = (0..)
            .map_while(|raw| {
                game.blocks
                    .identifier_of_raw(raw)
                    .map(|identifier| (identifier.clone(), raw))
            })
            .collect();
        let (listener, connector) = loopback_listener();
        let mut catalog = EntityCatalog::default();
        if herd || ship {
            catalog
                .kinds
                .register(EntityKindSpec::new("test_game:wisp").component(Glow(1), true))
                .expect("the kind is new");
        }
        // The tick on which each wisp was made, and where, by id.
        let mut wisps: std::collections::BTreeMap<EntityId, (u64, i64)> = Default::default();
        let logic = ScriptedLogic::new()
            .with_entity_catalog(catalog)
            .on_message(move |_, kind, payload, world| {
                if *kind != place_block_kind() {
                    return;
                }
                let mut reader = ByteReader::new(payload);
                let level = LevelIndex::new(reader.read_varint().unwrap() as u32);
                let at = VoxelCoordinate::new(
                    reader.read_signed_varint().unwrap(),
                    reader.read_signed_varint().unwrap(),
                    reader.read_signed_varint().unwrap(),
                );
                let block = reader.read_identifier().unwrap();
                let Some(&raw) = block_ids.get(&block) else {
                    return; // a block this server does not have: ignored
                };
                let channel = world
                    .data()
                    .main()
                    .overrides
                    .layout()
                    .channel_index(&id("core:block"))
                    .unwrap();
                if world
                    .grid(GridId::root(level))
                    .unwrap()
                    .set(channel, at, raw)
                    == Ok(true)
                {
                    let mut announcement = ByteWriter::new();
                    announcement.write_signed_varint(at.x());
                    announcement.write_signed_varint(at.y());
                    announcement.write_signed_varint(at.z());
                    world.send(
                        Audience::Watching(level, at.split().0),
                        block_placed_kind(),
                        announcement.into_bytes(),
                    );
                }
            })
            .on_tick({
                let marble = game.raw_block("fancy_blocks:marble");
                let glass = game.raw_block("fancy_blocks:glass");
                move |tick, world| {
                    let count = tick.get();
                    if ship {
                        ship_at_tick(count, world, marble, glass);
                    }
                    if planet {
                        planet_at_tick(count, world, marble, glass);
                    }
                    if herd {
                        if count % 7 == 0 && wisps.len() < 6 {
                            let start = (count % 5) as i64 - 2;
                            let id = world
                                .spawn_entity(
                                    &wisp_kind(),
                                    wisp_pose(start, 0),
                                    ComponentBag::new(),
                                )
                                .expect("the kind is declared");
                            wisps.insert(id, (count, start));
                        }
                        for (&id, &(made, start)) in &wisps {
                            world
                                .set_entity_pose(id, wisp_pose(start, count - made))
                                .expect("a wisp exists");
                            if count % 3 == 0 {
                                world
                                    .set_entity_component(id, &Glow(count as u32))
                                    .expect("a wisp has a glow");
                            }
                        }
                        if count % 11 == 0
                            && let Some((&oldest, _)) = wisps.iter().next()
                        {
                            world.despawn_entity(oldest);
                            wisps.remove(&oldest);
                        }
                    }
                    if count % 5 == 0 {
                        let channel = world
                            .data()
                            .main()
                            .overrides
                            .layout()
                            .channel_index(&id("core:block"))
                            .unwrap();
                        let at = VoxelCoordinate::new(
                            ((count * 7) % 61) as i64 - 30,
                            2,
                            ((count * 13) % 61) as i64 - 30,
                        );
                        let block = if count % 10 == 0 { marble } else { glass };
                        world
                            .grid(GridId::root(LevelIndex::MAIN))
                            .unwrap()
                            .set(channel, at, block)
                            .expect("flat terrain is always available");
                    }
                    if count % 10 == 0 {
                        world.send(
                            Audience::Everyone,
                            time_kind(),
                            count.to_le_bytes().to_vec(),
                        );
                    }
                }
            });
        let server = Server::new(
            ServerConfig::new(game_id()).with_counted_challenges(1),
            world,
            &game.registries(),
            Box::new(logic),
            Box::new(listener),
        )
        .expect("the pretend game's world can be described");
        Self {
            server,
            game,
            connector,
            joined: 0,
        }
    }

    /// Joins a player whose game has `plugins`, recording nothing.
    pub fn join(&mut self, plugins: &[Plugin]) -> Player<LoopbackConnection> {
        let connection = self.connector.connect().expect("the server is running");
        self.complete_join(connection, plugins)
    }

    /// Joins a player whose connection is recorded; the buffer holds the recording as it grows.
    pub fn join_recorded(
        &mut self,
        plugins: &[Plugin],
    ) -> (
        Player<RecordingConnection<LoopbackConnection, SharedBuffer>>,
        SharedBuffer,
    ) {
        let connection = self.connector.connect().expect("the server is running");
        let buffer = SharedBuffer::default();
        let recording = RecordingConnection::new(connection, buffer.clone()).unwrap();
        (self.complete_join(recording, plugins), buffer)
    }

    /// Joins takes the server two ticks: it challenges the player's identity, and welcomes them once they
    /// have proved it.
    fn complete_join<C: Connection>(&mut self, connection: C, plugins: &[Plugin]) -> Player<C> {
        self.joined += 1;
        let identity = Identity::from_secret([self.joined; 32]);
        let mut handshake = Handshake::start(connection, game_id(), &identity).unwrap();
        for _ in 0..2 {
            self.server.step();
            handshake = match handshake.poll().unwrap() {
                HandshakeProgress::Waiting(waiting) => waiting,
                HandshakeProgress::Welcomed(welcomed) => {
                    return Player::finish(welcomed, plugins).unwrap();
                }
            };
        }
        match handshake.poll().unwrap() {
            HandshakeProgress::Welcomed(welcomed) => Player::finish(welcomed, plugins).unwrap(),
            HandshakeProgress::Waiting(_) => panic!("the server welcomed the client in two ticks"),
        }
    }

    /// Runs one server tick.
    pub fn step(&mut self) {
        self.server.step();
    }
}

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}

/// A recording the test can read while the player holds the writing end.
#[derive(Clone, Debug, Default)]
pub struct SharedBuffer {
    bytes: Arc<Mutex<Vec<u8>>>,
}

impl SharedBuffer {
    pub fn contents(&self) -> Vec<u8> {
        self.bytes.lock().unwrap().clone()
    }
}

impl Write for SharedBuffer {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.bytes.lock().unwrap().extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A client with the game it runs, and a log of everything it was told, for comparing runs.
#[derive(Debug)]
pub struct Player<C: Connection> {
    pub client: Client<C>,
    pub game: Game,
    /// What each tick applied, in order.
    pub updates: Vec<TickUpdate>,
    /// The view after each tick, in order.
    pub summaries: Vec<ViewSummary>,
}

impl<C: Connection> Player<C> {
    fn finish(welcomed: client::Welcomed<C>, plugins: &[Plugin]) -> Result<Self, ClientError> {
        let mut game = Game::new(plugins);
        let (client, _) = welcomed.finish(game.load_context())?;
        Ok(Self {
            client,
            game,
            updates: Vec::new(),
            summaries: Vec::new(),
        })
    }

    /// Applies every tick that has fully arrived, logging each. Returns how many.
    pub fn catch_up(&mut self) -> Result<usize, ClientError> {
        let mut applied = 0;
        while let Some(update) = self.client.poll_tick()? {
            self.summaries.push(self.client.world().summary());
            self.updates.push(update);
            applied += 1;
        }
        Ok(applied)
    }

    /// Watches a region of the main level.
    pub fn watch(&mut self, region: BrickBox) {
        self.watch_level(LevelIndex::MAIN, region);
    }

    pub fn watch_level(&mut self, level: LevelIndex, region: BrickBox) {
        self.client.watch(level, region).unwrap();
    }

    /// Asks the server to place a block in the main level.
    pub fn place(&mut self, at: [i64; 3], block: &str) {
        self.place_in(LevelIndex::MAIN, at, block);
    }

    pub fn place_in(&mut self, level: LevelIndex, at: [i64; 3], block: &str) {
        let mut payload = ByteWriter::new();
        payload.write_varint(u64::from(level.get()));
        for component in at {
            payload.write_signed_varint(component);
        }
        payload.write_identifier(&id(block));
        self.client
            .send_game_message(place_block_kind(), payload.into_bytes())
            .unwrap();
    }

    pub fn tick(&self) -> Tick {
        self.client.world().tick()
    }
}

/// Plays a recording as a client of `plugins`' game.
pub fn replay_player(
    recording: Vec<u8>,
    plugins: &[Plugin],
) -> Result<Player<ReplayConnection<Cursor<Vec<u8>>>>, ClientError> {
    let connection = ReplayConnection::new(Cursor::new(recording)).expect("a valid recording");
    // A replay asks for nothing, so who it says it is does not matter.
    let mut handshake = Handshake::start(connection, game_id(), &Identity::from_secret([0; 32]))?;
    loop {
        handshake = match handshake.poll()? {
            HandshakeProgress::Waiting(waiting) => waiting,
            HandshakeProgress::Welcomed(welcomed) => return Player::finish(welcomed, plugins),
        };
    }
}

/// The brick as the world shows it: the edited brick if there is one, else the generated one. `None`
/// where the world cannot say (no generator assigned, or its plugin missing).
pub fn brick_contents(level: &LevelData, coordinate: BrickCoordinate) -> Option<Brick> {
    match level.effective_brick(coordinate) {
        BrickSource::Edited(brick) => Some((*brick).clone()),
        BrickSource::Generated(brick) => Some(brick),
        BrickSource::Unassigned | BrickSource::MissingGenerator(_) => None,
    }
}

/// Whether a player and the server agree about one brick, by what it means (numbering may differ).
fn same_meaning(
    player_level: &LevelData,
    player_game: &Game,
    server_level: &LevelData,
    server_game: &Game,
    coordinate: BrickCoordinate,
) -> bool {
    let (Some(theirs), Some(ours)) = (
        brick_contents(player_level, coordinate),
        brick_contents(server_level, coordinate),
    ) else {
        return true; // terrain that one side cannot generate has nothing to compare
    };
    let theirs = player_game.brick_meaning(player_level.overrides.layout(), &theirs);
    let ours = server_game.brick_meaning(server_level.overrides.layout(), &ours);
    ["core:block", "core:temperature"]
        .iter()
        .all(|channel| theirs[*channel] == ours[*channel])
}

/// What is wrong with a player's picture of the watched region, cheaply: the set of edited bricks
/// must be the server's, the generators of the region's terrain must be the server's, and the
/// bricks named in `changed` must hold what the server's hold.
pub fn quick_differences<C: Connection>(
    player: &Player<C>,
    server_world: &WorldData,
    server_game: &Game,
    changed: &[BrickCoordinate],
) -> Vec<String> {
    let mut differences = Vec::new();
    let Some((level, region)) = player.client.world().watched() else {
        return differences;
    };
    let (Some(player_level), Some(server_level)) = (
        player.client.world().world().level(level),
        server_world.level(level),
    ) else {
        return vec![format!("the server or the client has no {level}")];
    };
    for coordinate in region.bricks() {
        let theirs = player_level.overrides.brick(coordinate).is_some();
        let ours = server_level.overrides.brick(coordinate).is_some();
        if theirs != ours {
            differences.push(format!(
                "brick {coordinate} is edited on the server: {ours}, on the client: {theirs}"
            ));
        }
        let player_generator = player_level
            .terrain
            .assigned_generator(coordinate)
            .and_then(|id| player_level.terrain.generators().entry(id))
            .map(|entry| entry.identifier().clone());
        let server_generator = server_level
            .terrain
            .assigned_generator(coordinate)
            .and_then(|id| server_level.terrain.generators().entry(id))
            .map(|entry| entry.identifier().clone());
        if player_generator != server_generator {
            differences.push(format!(
                "brick {coordinate} is generated by {server_generator:?} on the server, {player_generator:?} on the client"
            ));
        }
    }
    for &coordinate in changed {
        if region.contains(coordinate)
            && !same_meaning(
                player_level,
                &player.game,
                server_level,
                server_game,
                coordinate,
            )
        {
            differences.push(format!("brick {coordinate} holds different things"));
        }
    }
    differences.extend(ship_differences(player, server_world, level, region));
    differences
}

/// What is wrong with a player's picture of the ships: it must have exactly the ships of the watched level that touch the watched
/// region, each where the server has it and (when both number content alike) holding what the server's holds.
fn ship_differences<C: Connection>(
    player: &Player<C>,
    server_world: &WorldData,
    level: LevelIndex,
    region: BrickBox,
) -> Vec<String> {
    let mut differences = Vec::new();
    let world = player.client.world();
    for (id, ship) in server_world.grids().iter() {
        let wanted = ship.level == level
            && ship
                .world_brick_bounds()
                .is_some_and(|bounds| bounds.intersection(region).is_some());
        match (wanted, world.grids().get(id)) {
            (true, None) => differences.push(format!("{id} is missing on the client")),
            (false, Some(_)) => differences.push(format!("{id} should not be on the client")),
            (true, Some(theirs)) => {
                if theirs.pose != ship.pose || theirs.level != ship.level {
                    differences.push(format!("{id} is somewhere else on the client"));
                }
                // What generates the rest of its bricks: the same generator (by name) over the same bricks.
                let names = |generator| {
                    server_world
                        .main()
                        .terrain
                        .generators()
                        .entry(generator)
                        .map(|entry| entry.identifier().clone())
                };
                let ours = ship
                    .terrain
                    .map(|terrain| (names(terrain.generator), terrain.extent));
                let theirs_terrain = theirs.terrain.map(|terrain| {
                    (
                        world
                            .world()
                            .main()
                            .terrain
                            .generators()
                            .entry(terrain.generator)
                            .map(|entry| entry.identifier().clone()),
                        terrain.extent,
                    )
                });
                if ours != theirs_terrain {
                    differences.push(format!("{id} is generated differently on the client"));
                }
                if world.verifies_edit_digests()
                    && theirs.voxels.content_digest() != ship.voxels.content_digest()
                {
                    differences.push(format!("{id} holds different things on the client"));
                }
            }
            (false, None) => {}
        }
    }
    for (id, _) in world.grids().iter() {
        if server_world.grids().get(id).is_none() {
            differences.push(format!("{id} is on the client and not on the server"));
        }
    }
    differences
}

/// Everything wrong with a player's picture compared with the server's, for the region it watches:
/// the quick checks, plus every brick of the region, generated ones included. Empty when the two
/// agree. Slow for large regions, since it generates every unedited brick on both sides.
pub fn differences_from_server<C: Connection>(
    player: &Player<C>,
    server_world: &WorldData,
    server_game: &Game,
) -> Vec<String> {
    let Some((_, region)) = player.client.world().watched() else {
        return Vec::new();
    };
    let everything: Vec<BrickCoordinate> = region.bricks().collect();
    quick_differences(player, server_world, server_game, &everything)
}
