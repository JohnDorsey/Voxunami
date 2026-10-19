use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use content_registry::IdentifierRegistry;
use namespaced_id::NamespacedId;
use protocol::{
    ClientId, ClientMessage, GeneratorAssignment, PROTOCOL_VERSION, ServerMessage, Tick, Welcome,
};
use region_octree::BrickCube;
use transport::{Connection, ConnectionClosed, Listener};
use voxel_grid::Grid;
use voxel_space::{BrickBox, BrickCoordinate, LevelIndex};
use world_storage::{SaveError, WorldData};

use crate::config::ServerConfig;
use crate::game_logic::GameLogic;
use crate::server_world::{Audience, OutgoingGameMessage, ServerWorld};

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
    Joined {
        /// The region of a level the client has been told it watches.
        watched: Option<(LevelIndex, BrickBox)>,
        /// The latest region it asked for, not yet answered.
        requested: Option<(LevelIndex, BrickBox)>,
    },
}

#[derive(Debug)]
struct Session {
    connection: Box<dyn Connection>,
    phase: Phase,
}

/// What one edited brick needs sent to the clients that already hold it.
struct EditedBrick {
    level: LevelIndex,
    brick: BrickCoordinate,
    update: ServerMessage,
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
        Ok(Self {
            config,
            world: ServerWorld::new(world, logic.entity_catalog()),
            logic,
            listener,
            world_description,
            sessions: BTreeMap::new(),
            next_client_id: 1,
        })
    }

    pub fn world(&self) -> &WorldData {
        self.world.data()
    }

    /// The last tick completed.
    pub fn tick(&self) -> Tick {
        self.world.tick()
    }

    pub fn connected_clients(&self) -> impl Iterator<Item = ClientId> + '_ {
        self.sessions.keys().copied()
    }

    /// The world as a world file, as it is right now, with the game's own state in it. `registries`
    /// are the content tables, as for [`Server::new`]. The server keeps running; this is how a
    /// server that never stops saves its world.
    pub fn save(
        &mut self,
        registries: &[(NamespacedId, &dyn IdentifierRegistry)],
    ) -> Result<Vec<u8>, SaveError> {
        for section in self.logic.save_sections() {
            self.world.data_mut().set_opaque_section(section);
        }
        self.world.data().save(registries)
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
        let mut report = StepReport {
            tick,
            joined: Vec::new(),
            left: Vec::new(),
        };
        self.accept_new_connections();
        self.receive_messages(&mut report);
        self.logic.tick(tick, &mut self.world);
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
        if let Phase::AwaitingHello { ticks_waited } = &mut session.phase {
            *ticks_waited += 1;
            if *ticks_waited > limit {
                return Some(self.refuse(id, "no hello arrived in time".to_owned()));
            }
        }
        None
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
        let joined = matches!(
            self.sessions.get(&id).expect("the session exists").phase,
            Phase::Joined { .. }
        );
        match (joined, message) {
            (_, ClientMessage::Goodbye) => Some(LeaveReason::Goodbye),
            (
                false,
                ClientMessage::Hello {
                    protocol_version,
                    game,
                },
            ) => self.welcome(id, protocol_version, &game, report),
            (false, _) => Some(self.refuse(id, "the first message must be hello".to_owned())),
            (true, ClientMessage::Hello { .. }) => {
                Some(self.refuse(id, "hello was already sent".to_owned()))
            }
            (true, ClientMessage::SetInterest { level, region }) => {
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
                    *requested = Some((level, region));
                }
                None
            }
            (true, ClientMessage::GameMessage { kind, payload }) => {
                self.logic
                    .message_received(id, &kind, &payload, &mut self.world);
                None
            }
            (true, ClientMessage::Command { text }) => {
                self.logic.command_received(id, &text, &mut self.world);
                None
            }
        }
    }

    fn welcome(
        &mut self,
        id: ClientId,
        protocol_version: u32,
        game: &NamespacedId,
        report: &mut StepReport,
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
        let welcome = ServerMessage::Welcome(Welcome {
            protocol_version: PROTOCOL_VERSION,
            game: self.config.game.clone(),
            client: id,
            ticks_per_second: self.config.ticks_per_second,
            tick: Tick::new(self.world.tick().get() - 1),
            world_description: self.world_description.clone(),
        });
        let session = self.sessions.get_mut(&id).expect("the session exists");
        if session.connection.send(welcome.encode()).is_err() {
            return Some(LeaveReason::ConnectionClosed);
        }
        session.phase = Phase::Joined {
            watched: None,
            requested: None,
        };
        self.world.add_client(id);
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
        let edited = self.edited_bricks();
        let ids: Vec<ClientId> = self
            .sessions
            .iter()
            .filter(|(_, session)| matches!(session.phase, Phase::Joined { .. }))
            .map(|(id, _)| *id)
            .collect();
        let mut closed = Vec::new();
        for id in ids {
            let frames = self.frames_for(id, tick, &edited);
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

    /// The updates this tick's edits need, once, whatever the number of clients.
    fn edited_bricks(&self) -> Vec<EditedBrick> {
        self.world
            .edits
            .iter()
            .map(|(&(level, brick), edits)| {
                let contents = Arc::clone(
                    self.world
                        .data()
                        .level(level)
                        .expect("an edited level exists")
                        .overrides
                        .brick(brick)
                        .expect("an edited brick has contents"),
                );
                let update = if edits.created {
                    ServerMessage::BrickContents {
                        level,
                        brick,
                        contents,
                    }
                } else {
                    ServerMessage::VoxelEdits {
                        level,
                        brick,
                        changes: edits.changes.clone(),
                        resulting_digest: contents.content_digest(),
                    }
                };
                EditedBrick {
                    level,
                    brick,
                    update,
                }
            })
            .collect()
    }

    /// Everything one client is sent for this tick, in order.
    fn frames_for(&mut self, id: ClientId, tick: Tick, edited: &[EditedBrick]) -> Vec<Vec<u8>> {
        let session = self.sessions.get_mut(&id).expect("the session exists");
        let Phase::Joined { watched, requested } = &mut session.phase else {
            unreachable!("only joined clients are sent updates");
        };
        let previous = *watched;
        let current = match requested.take() {
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
                        level,
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
                    .filter(|entry| entry.level == level && kept.contains(entry.brick))
                    .map(|entry| entry.update.clone()),
            );
        }
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
