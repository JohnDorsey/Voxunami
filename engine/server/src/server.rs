use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use content_registry::IdentifierRegistry;
use identity::ChallengeSource;
use namespaced_id::NamespacedId;
use protocol::{
    ClientId, ClientMessage, GeneratorAssignment, PROTOCOL_VERSION, PlayerKey, ServerMessage, Tick,
    Welcome,
};
use region_octree::BrickCube;
use transport::{Connection, ConnectionClosed, Listener};
use voxel_grid::Grid;
use voxel_space::{BrickBox, BrickCoordinate, GridId, LevelIndex};
use world_storage::{MemoryShipFiles, SaveError, ShipFiles, ShipLoadError, WorldData, WorldSave};

use crate::config::ServerConfig;
use crate::game_logic::GameLogic;
use crate::grid_replication::{self, EditedBrick, KnownGrids};
use crate::replication::{KnownEntities, ViewCache, entity_messages};
use crate::server_world::{Audience, OutgoingGameMessage, ServerWorld};
use crate::ship_settings::ShipSettings;

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("the world cannot be described to clients: {0}")]
    CannotDescribeWorld(#[from] SaveError),
}

/// Why a client is no longer connected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeaveReason {
    /// The client said goodbye.
    Goodbye,
    /// The connection closed without a goodbye.
    ConnectionClosed,
    /// The server disconnected the client for breaking the protocol, with the reason it was told.
    Refused(String),
    /// The game sent the client away, with the reason it was told.
    SentAway(String),
}

/// What one tick changed about who is connected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepReport {
    pub tick: Tick,
    pub joined: Vec<ClientId>,
    /// Every connection that ended, including ones that never finished saying hello.
    pub left: Vec<(ClientId, LeaveReason)>,
}

#[derive(Debug)]
enum Phase {
    AwaitingHello {
        ticks_waited: u32,
    },
    /// The client has said who it is, and has been challenged to prove it.
    AwaitingProof {
        ticks_waited: u32,
        key: PlayerKey,
        challenge: [u8; 32],
    },
    Joined {
        /// The region of a level the client has been told it watches.
        watched: Option<(LevelIndex, BrickBox)>,
        /// The latest region it asked for, not yet answered. (Boxed: it is mostly none, and a region is two coordinates of 128 bits each, which would make this variant
        /// much larger than the others.)
        requested: Option<Box<(LevelIndex, BrickBox)>>,
        /// The entities it has been told of, and as what.
        known_entities: KnownEntities,
        /// The ships it has been told of.
        known_grids: KnownGrids,
    },
}

/// Where a session is, without what it carries.
#[derive(Clone, Copy)]
enum Stage {
    Hello,
    Proof,
    Joined,
}

#[derive(Debug)]
struct Session {
    connection: Box<dyn Connection>,
    phase: Phase,
}

/// The authoritative server: it owns the world, runs the game's ticks, and sends each client the
/// part of the world it watches.
///
/// A tick, in order: new connections are accepted; each client's messages are handled (clients in id
/// order, each client's messages in the order sent); the game's [`GameLogic::tick`] runs; then every
/// joined client is sent, in this order, the change of its watched region, the edits inside it, the
/// game messages meant for it, and `TickCompleted`. All of it is a pure function of the incoming
/// messages and the game logic, so a recording of what a client received can stand in for the
/// server.
///
/// Call [`Server::step`] at the game's tick rate. The server itself never looks at the clock.
#[derive(Debug)]
pub struct Server {
    config: ServerConfig,
    world: ServerWorld,
    logic: Box<dyn GameLogic>,
    listener: Box<dyn Listener>,
    world_description: Vec<u8>,
    sessions: BTreeMap<ClientId, Session>,
    next_client_id: u32,
    /// What clients are told of each entity, kept until it changes.
    entity_views: ViewCache,
    /// Where the challenges players prove their identity against come from.
    challenges: ChallengeSource,
}

impl Server {
    /// `registries` supplies the content tables clients are told, as `WorldData::save` takes them.
    pub fn new(
        config: ServerConfig,
        world: WorldData,
        registries: &[(NamespacedId, &dyn IdentifierRegistry)],
        logic: Box<dyn GameLogic>,
        listener: Box<dyn Listener>,
    ) -> Result<Self, ServerError> {
        let world_description = world.describe(registries)?;
        let challenges = config
            .challenge_seed
            .map_or_else(ChallengeSource::random, ChallengeSource::counter);
        let format = world.ship_format(registries)?;
        let mut world = ServerWorld::new(world, logic.entity_catalog());
        world.set_gravity_settings(config.gravity);
        for (level, base) in &config.level_gravity {
            if let Some(index) = world.level_index(level) {
                world.set_level_gravity(index, *base);
            }
        }
        world.set_ship_settings(config.ships);
        // (A server keeps the files of its ships in memory until it is told where; see `Server::with_ship_files`.)
        world.set_ship_storage(format, Box::new(MemoryShipFiles::new()));
        Ok(Self {
            config,
            world,
            logic,
            listener,
            world_description,
            sessions: BTreeMap::new(),
            next_client_id: 1,
            entity_views: ViewCache::default(),
            challenges,
        })
    }

    pub fn world(&self) -> &WorldData {
        self.world.data()
    }

    /// The same server, keeping its ships as these say (a test of what ships do uses [`ShipSettings::everywhere`]).
    pub fn with_ship_settings(mut self, settings: ShipSettings) -> Self {
        self.set_ship_settings(settings);
        self
    }

    /// Keeps the ships as these say from the next tick.
    pub fn set_ship_settings(&mut self, settings: ShipSettings) {
        self.config.ships = settings;
        self.world.set_ship_settings(settings);
    }

    /// The same server, keeping the files of its ships in these. The ships the world file names must be there: they are read from them.
    pub fn with_ship_files(mut self, files: Box<dyn ShipFiles>) -> Self {
        let format = self
            .world
            .ship_format()
            .expect("a server made with a ship format has one")
            .clone();
        self.world.set_ship_storage(format, files);
        self
    }

    /// Brings every ship that is in a file into memory. Returns the ones that could not be brought, with why; they stay in their files
    /// and the world file goes on naming them, so nothing is lost.
    pub fn load_parked_ships(&mut self) -> Vec<(voxel_space::GridId, ShipLoadError)> {
        let parked: Vec<_> = self
            .world
            .data()
            .grids()
            .parked()
            .map(|(id, _)| id)
            .collect();
        let mut problems = Vec::new();
        for id in parked {
            if let Err(error) = self.world.load_ship(id) {
                match error {
                    crate::ShipStorageError::Load(error) => problems.push((id, error)),
                    other => panic!("a server has somewhere to keep ships: {other}"),
                }
            }
        }
        problems
    }

    /// The files of this server's ships: for a save to write into, and to clean.
    pub fn ship_files_mut(&mut self) -> &mut (dyn ShipFiles + '_) {
        self.world
            .ship_files_mut()
            .expect("a server has somewhere to keep ships")
    }

    /// The last tick completed.
    pub fn tick(&self) -> Tick {
        self.world.tick()
    }

    /// Who a joined client proved to be: the public key of its identity.
    pub fn player_key(&self, client: ClientId) -> Option<PlayerKey> {
        self.world.player_key(client)
    }

    pub fn connected_clients(&self) -> impl Iterator<Item = ClientId> + '_ {
        self.sessions.keys().copied()
    }

    /// The world as a world file and the files of the ships that are in memory, as it is right now, with the game's own state in it.
    /// `registries` are the content tables, as for [`Server::new`]. The server keeps running; this is how a server that never stops saves
    /// its world. Writing the set is `WorldSave::write`, with this server's [`Server::ship_files_mut`], before the next tick.
    pub fn save(
        &mut self,
        registries: &[(NamespacedId, &dyn IdentifierRegistry)],
    ) -> Result<WorldSave, SaveError> {
        for section in self.logic.save_sections() {
            self.world.data_mut().set_opaque_section(section);
        }
        self.world.data_mut().save_with_ships(registries)
    }

    /// Ends the server and returns its world, for saving, with the game's own state in its plugin
    /// sections.
    pub fn into_world(self) -> WorldData {
        let sections = self.logic.save_sections();
        let mut world = self.world.into_data();
        for section in sections {
            world.set_opaque_section(section);
        }
        world
    }

    /// Runs one tick.
    pub fn step(&mut self) -> StepReport {
        let tick = self.world.tick().next();
        self.world.set_tick(tick);
        // (The time of the world, which is saved with it, passes a tick at the start of each: what runs in this tick sees it.)
        self.world.data.advance_clock(1);
        // (And what is on rails is where its rail says it is at that time, before anything in the tick looks at it.)
        self.world.apply_rails(self.config.ticks_per_second);
        let mut report = StepReport {
            tick,
            joined: Vec::new(),
            left: Vec::new(),
        };
        self.accept_new_connections();
        self.receive_messages(&mut report);
        self.logic.tick(tick, &mut self.world);
        // (A ship that came into memory in this tick is on its rail before anyone is told where it is. Where a rail has put a ship already, this puts it again, and nothing changes.)
        self.world.apply_rails(self.config.ticks_per_second);
        self.send_away(&mut report);
        self.send_updates(tick, &mut report);
        self.world.edits.clear();
        self.world.outgoing.clear();
        self.world.replies.clear();
        report
    }

    fn accept_new_connections(&mut self) {
        while let Some(connection) = self.listener.accept() {
            let id = ClientId::new(self.next_client_id);
            self.next_client_id = self
                .next_client_id
                .checked_add(1)
                .expect("client ids ran out");
            self.sessions.insert(
                id,
                Session {
                    connection,
                    phase: Phase::AwaitingHello { ticks_waited: 0 },
                },
            );
        }
    }

    fn receive_messages(&mut self, report: &mut StepReport) {
        let ids: Vec<ClientId> = self.sessions.keys().copied().collect();
        for id in ids {
            if let Some(reason) = self.receive_from(id, report) {
                self.remove_session(id, reason, report);
            }
        }
    }

    /// Handles everything a client has sent. Returns why to drop it, if it should be dropped.
    fn receive_from(&mut self, id: ClientId, report: &mut StepReport) -> Option<LeaveReason> {
        loop {
            let session = self.sessions.get_mut(&id).expect("the session exists");
            match session.connection.receive() {
                Ok(Some(frame)) => {
                    if let Some(reason) = self.handle_frame(id, &frame, report) {
                        return Some(reason);
                    }
                }
                Ok(None) => break,
                Err(ConnectionClosed) => return Some(LeaveReason::ConnectionClosed),
            }
        }
        let limit = self.config.handshake_timeout_ticks;
        let session = self.sessions.get_mut(&id).expect("the session exists");
        let late = match &mut session.phase {
            Phase::AwaitingHello { ticks_waited } => {
                *ticks_waited += 1;
                (*ticks_waited > limit).then_some("no hello arrived in time")
            }
            Phase::AwaitingProof { ticks_waited, .. } => {
                *ticks_waited += 1;
                (*ticks_waited > limit).then_some("no proof of identity arrived in time")
            }
            Phase::Joined { .. } => None,
        };
        late.map(|reason| self.refuse(id, reason.to_owned()))
    }

    fn handle_frame(
        &mut self,
        id: ClientId,
        frame: &[u8],
        report: &mut StepReport,
    ) -> Option<LeaveReason> {
        let message = match ClientMessage::decode(frame) {
            Ok(message) => message,
            Err(error) => return Some(self.refuse(id, format!("malformed message: {error}"))),
        };
        let stage = match self.sessions.get(&id).expect("the session exists").phase {
            Phase::AwaitingHello { .. } => Stage::Hello,
            Phase::AwaitingProof { .. } => Stage::Proof,
            Phase::Joined { .. } => Stage::Joined,
        };
        match (stage, message) {
            (_, ClientMessage::Goodbye) => Some(LeaveReason::Goodbye),
            (
                Stage::Hello,
                ClientMessage::Hello {
                    protocol_version,
                    game,
                    player_key,
                },
            ) => self.challenge(id, protocol_version, &game, player_key),
            (Stage::Hello, _) => {
                Some(self.refuse(id, "the first message must be hello".to_owned()))
            }
            (Stage::Proof, ClientMessage::Prove { signature }) => {
                self.welcome(id, &signature, report)
            }
            (Stage::Proof, _) => Some(self.refuse(
                id,
                "the proof of identity must follow the challenge".to_owned(),
            )),
            (Stage::Joined, ClientMessage::Hello { .. } | ClientMessage::Prove { .. }) => {
                Some(self.refuse(id, "hello was already sent".to_owned()))
            }
            (Stage::Joined, ClientMessage::SetInterest { level, region }) => {
                if self.world.data().level(level).is_none() {
                    return Some(self.refuse(id, format!("the world has no {level}")));
                }
                if region.brick_count() > u128::from(self.config.maximum_watched_bricks) {
                    return Some(self.refuse(
                        id,
                        format!(
                            "watching {} bricks exceeds the limit of {}",
                            region.brick_count(),
                            self.config.maximum_watched_bricks
                        ),
                    ));
                }
                if let Phase::Joined { requested, .. } = &mut self
                    .sessions
                    .get_mut(&id)
                    .expect("the session exists")
                    .phase
                {
                    *requested = Some(Box::new((level, region)));
                }
                None
            }
            (Stage::Joined, ClientMessage::GameMessage { kind, payload }) => {
                self.logic
                    .message_received(id, &kind, &payload, &mut self.world);
                None
            }
            (Stage::Joined, ClientMessage::Command { text }) => {
                self.logic.command_received(id, &text, &mut self.world);
                None
            }
        }
    }

    /// The hello has arrived: checks the version and the game, and that nobody is playing as this key, and
    /// sends the challenge the client must sign.
    fn challenge(
        &mut self,
        id: ClientId,
        protocol_version: u32,
        game: &NamespacedId,
        key: PlayerKey,
    ) -> Option<LeaveReason> {
        if protocol_version != PROTOCOL_VERSION {
            return Some(self.refuse(
                id,
                format!(
                    "this server speaks protocol version {PROTOCOL_VERSION}, not {protocol_version}"
                ),
            ));
        }
        if *game != self.config.game {
            return Some(self.refuse(
                id,
                format!("this server runs {}, not {game}", self.config.game),
            ));
        }
        let taken = self.sessions.iter().any(|(other, session)| {
            *other != id
                && match &session.phase {
                    Phase::AwaitingHello { .. } => false,
                    Phase::AwaitingProof { key: other_key, .. } => *other_key == key,
                    Phase::Joined { .. } => self.world.player_key(*other) == Some(key),
                }
        });
        if taken {
            return Some(self.refuse(id, "this identity is already playing".to_owned()));
        }
        let challenge = self.challenges.next_challenge();
        let session = self.sessions.get_mut(&id).expect("the session exists");
        if session
            .connection
            .send(ServerMessage::Challenge { nonce: challenge }.encode())
            .is_err()
        {
            return Some(LeaveReason::ConnectionClosed);
        }
        session.phase = Phase::AwaitingProof {
            ticks_waited: 0,
            key,
            challenge,
        };
        None
    }

    /// The proof has arrived: if it is good, welcomes the client and lets the game know it has joined.
    fn welcome(
        &mut self,
        id: ClientId,
        signature: &[u8; 64],
        report: &mut StepReport,
    ) -> Option<LeaveReason> {
        let Phase::AwaitingProof { key, challenge, .. } =
            self.sessions.get(&id).expect("the session exists").phase
        else {
            unreachable!("only a client that was challenged is asked for its proof");
        };
        if !identity::verify(&key, &self.config.game, &challenge, signature) {
            return Some(self.refuse(id, "the proof of identity was not good".to_owned()));
        }
        let welcome = ServerMessage::Welcome(Welcome {
            protocol_version: PROTOCOL_VERSION,
            game: self.config.game.clone(),
            client: id,
            ticks_per_second: self.config.ticks_per_second,
            tick: Tick::new(self.world.tick().get() - 1),
            world_description: self.world_description.clone(),
            package: self.config.package.clone(),
        });
        let session = self.sessions.get_mut(&id).expect("the session exists");
        if session.connection.send(welcome.encode()).is_err() {
            return Some(LeaveReason::ConnectionClosed);
        }
        session.phase = Phase::Joined {
            watched: None,
            requested: None,
            known_entities: KnownEntities::new(),
            known_grids: KnownGrids::new(),
        };
        self.world.add_client(id, key);
        report.joined.push(id);
        self.logic.client_joined(id, &mut self.world);
        None
    }

    /// Tells the client why it is being dropped, as far as the connection still allows.
    fn refuse(&mut self, id: ClientId, reason: String) -> LeaveReason {
        if let Some(session) = self.sessions.get_mut(&id) {
            let _ = session.connection.send(
                ServerMessage::Disconnected {
                    reason: reason.clone(),
                }
                .encode(),
            );
        }
        LeaveReason::Refused(reason)
    }

    fn remove_session(&mut self, id: ClientId, reason: LeaveReason, report: &mut StepReport) {
        let session = self.sessions.remove(&id).expect("the session exists");
        if matches!(session.phase, Phase::Joined { .. }) {
            self.world.remove_client(id);
            self.logic.client_left(id, &mut self.world);
        }
        report.left.push((id, reason));
    }

    fn send_updates(&mut self, tick: Tick, report: &mut StepReport) {
        let changed = self.world.data_mut().entities_mut().take_changed();
        self.entity_views.invalidate(changed);
        let edited = self.edited_bricks();
        let ships = grid_replication::ship_views(&self.world);
        let ids: Vec<ClientId> = self
            .sessions
            .iter()
            .filter(|(_, session)| matches!(session.phase, Phase::Joined { .. }))
            .map(|(id, _)| *id)
            .collect();
        let mut closed = Vec::new();
        for id in ids {
            let frames = self.frames_for(id, tick, &edited, &ships);
            let session = self.sessions.get_mut(&id).expect("the session exists");
            if frames
                .into_iter()
                .any(|frame| session.connection.send(frame).is_err())
            {
                closed.push(id);
            }
        }
        for id in closed {
            self.remove_session(id, LeaveReason::ConnectionClosed, report);
        }
    }

    /// Sends away the clients the game asked to send away, telling each why.
    fn send_away(&mut self, report: &mut StepReport) {
        for (id, reason) in self.world.take_disconnects() {
            if self.sessions.contains_key(&id) {
                let _ = self.refuse(id, reason.clone());
                self.remove_session(id, LeaveReason::SentAway(reason), report);
            }
        }
    }

    /// The updates this tick's edits need, once, whatever the number of clients.
    fn edited_bricks(&self) -> Vec<EditedBrick> {
        self.world
            .edits
            .iter()
            // A ship that was removed in the tick it was edited has nothing left to send.
            .filter_map(|(&(grid, brick), edits)| {
                let voxels = match grid.root_level() {
                    Some(level) => {
                        &self
                            .world
                            .data()
                            .level(level)
                            .expect("an edited level exists")
                            .overrides
                    }
                    None => &self.world.data().grids().get(grid)?.voxels,
                };
                let contents =
                    Arc::clone(voxels.brick(brick).expect("an edited brick has contents"));
                let update = if edits.created {
                    ServerMessage::BrickContents {
                        grid,
                        brick,
                        contents,
                    }
                } else {
                    ServerMessage::VoxelEdits {
                        grid,
                        brick,
                        changes: edits.changes.clone(),
                        resulting_digest: contents.content_digest(),
                    }
                };
                Some(EditedBrick {
                    grid,
                    brick,
                    update,
                })
            })
            .collect()
    }

    /// Everything one client is sent for this tick, in order.
    fn frames_for(
        &mut self,
        id: ClientId,
        tick: Tick,
        edited: &[EditedBrick],
        ships: &[grid_replication::ShipView],
    ) -> Vec<Vec<u8>> {
        let session = self.sessions.get_mut(&id).expect("the session exists");
        let Phase::Joined {
            watched, requested, ..
        } = &mut session.phase
        else {
            unreachable!("only joined clients are sent updates");
        };
        let previous = *watched;
        let current = match requested.take().map(|asked| *asked) {
            Some(asked) if previous != Some(asked) => {
                *watched = Some(asked);
                Some(asked)
            }
            _ => previous,
        };

        let mut messages = Vec::new();
        if current != previous
            && let Some((level, region)) = current
        {
            // Within the same level only the newly watched part is new; another level is new whole.
            let newly_watched = match previous {
                Some((before_level, before)) if before_level == level => region.without(before),
                _ => vec![region],
            };
            messages.push(ServerMessage::InterestChanged {
                level,
                region,
                assignments: self.assignments_covering(level, &newly_watched),
            });
            let overrides = &self
                .world
                .data()
                .level(level)
                .expect("a watched level exists")
                .overrides;
            for slab in newly_watched {
                for brick in edited_bricks_in(overrides, slab) {
                    messages.push(ServerMessage::BrickContents {
                        grid: GridId::root(level),
                        brick,
                        contents: Arc::clone(
                            overrides.brick(brick).expect("the brick was just found"),
                        ),
                    });
                }
            }
        }
        // Bricks the client has just been sent are in their final state already.
        if let (Some((before_level, before)), Some((level, now))) = (previous, current)
            && before_level == level
            && let Some(kept) = before.intersection(now)
        {
            messages.extend(
                edited
                    .iter()
                    .filter(|entry| entry.grid == GridId::root(level) && kept.contains(entry.brick))
                    .map(|entry| entry.update.clone()),
            );
        }
        // Ships come after the terrain, entities after the voxels they stand in, and both before the messages of the game.
        let Phase::Joined {
            known_entities,
            known_grids,
            ..
        } = &mut self
            .sessions
            .get_mut(&id)
            .expect("the session exists")
            .phase
        else {
            unreachable!("only joined clients are sent updates");
        };
        messages.extend(grid_replication::grid_messages(
            &self.world,
            ships,
            known_grids,
            current,
            self.config.maximum_ship_bricks,
            edited,
        ));
        messages.extend(entity_messages(
            &self.world,
            &mut self.entity_views,
            known_entities,
            current,
            known_grids,
        ));
        for outgoing in &self.world.outgoing {
            if reaches(outgoing, id, current) {
                messages.push(ServerMessage::GameMessage {
                    kind: outgoing.kind.clone(),
                    payload: outgoing.payload.clone(),
                });
            }
        }
        for (client, text) in &self.world.replies {
            if *client == id {
                messages.push(ServerMessage::CommandReply { text: text.clone() });
            }
        }
        messages.push(ServerMessage::TickCompleted { tick });
        messages.iter().map(ServerMessage::encode).collect()
    }

    /// Which generator made the terrain of every assignment cube of `level` touching `regions`,
    /// assigning the level's current default to cubes that have none yet.
    fn assignments_covering(
        &mut self,
        level: LevelIndex,
        regions: &[BrickBox],
    ) -> Vec<GeneratorAssignment> {
        let terrain = &mut self
            .world
            .data_mut()
            .level_mut(level)
            .expect("a watched level exists")
            .terrain;
        let cube_level = terrain.assignment_cube_level();
        let origins: BTreeSet<BrickCoordinate> = regions
            .iter()
            .flat_map(|&region| BrickCube::covering(region, cube_level))
            .map(|cube| cube.origin())
            .collect();
        origins
            .into_iter()
            .map(|origin| {
                let generator = terrain.assign_unassigned(origin);
                GeneratorAssignment {
                    cube: BrickCube::new(origin, cube_level)
                        .expect("covering yields aligned cubes"),
                    generator: Some(generator.raw()),
                }
            })
            .collect()
    }
}

fn reaches(
    message: &OutgoingGameMessage,
    client: ClientId,
    watched: Option<(LevelIndex, BrickBox)>,
) -> bool {
    match message.audience {
        Audience::Everyone => true,
        Audience::Client(target) => target == client,
        Audience::Watching(level, brick) => watched.is_some_and(|(watched_level, region)| {
            watched_level == level && region.contains(brick)
        }),
    }
}

/// The edited bricks inside `region`, sorted, however the region and the edits compare in size.
fn edited_bricks_in(overrides: &Grid, region: BrickBox) -> Vec<BrickCoordinate> {
    let mut found: Vec<BrickCoordinate> = if region.brick_count() < overrides.brick_count() as u128
    {
        region
            .bricks()
            .filter(|brick| overrides.brick(*brick).is_some())
            .collect()
    } else {
        overrides
            .bricks()
            .map(|(brick, _)| brick)
            .filter(|brick| region.contains(*brick))
            .collect()
    };
    found.sort();
    found
}
