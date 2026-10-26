#![allow(dead_code)] // each test file uses a different part of this shared module

//! A server running the lifecycle plugin with the pretend game of `world_storage::test_support`, and a player who can be moved about, asked
//! questions and logged out and in.

use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use block_models::{BlockDefinition, BlockTable};
use grid_bodies::collision::BlockCollisions;
use grid_lifecycle::{ArrivalConfig, GridLifecyclePlugin, LifecycleReport};
use namespaced_id::NamespacedId;
use player_presence::{PlayerPlaced, PlayerPose, PlayersPlugin, PoseReport, SharedPresence};
use plugin_host::{
    CommandSpec, HandlerSpec, Phase, Plugin, PluginHost, Prompt, PromptAnswer, PromptAnswered,
    PromptClosed, PromptId, Registrar, SystemSpec,
};
use protocol::{ClientId, ClientMessage, Message, ServerMessage};
use server::test_support::log_in;
use server::{Server, ServerConfig, ServerWorld, ShipSettings};
use transport::Connection;
use transport::loopback::{LoopbackConnection, LoopbackConnector, loopback_listener};
use voxel_space::{BrickCoordinate, GridId, GridPose, LevelIndex, Orientation, WorldPosition};
use world_storage::test_support::{ALL_PLUGINS, Game, id};
use world_storage::{MemoryShipFiles, MovingGrid, ShipFileName, ShipFiles, WorldData};

pub type Action = Box<dyn FnOnce(&mut ServerWorld) + Send>;

/// A plugin that does what the test asks inside the next tick, before anything else.
struct Script {
    actions: Arc<Mutex<Vec<Action>>>,
}

impl std::fmt::Debug for Script {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Script")
    }
}

impl Plugin for Script {
    fn id(&self) -> NamespacedId {
        id("test:script")
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        let actions = Arc::clone(&self.actions);
        registrar.system(
            SystemSpec::new("test:script_runs", Phase::Decide),
            move |context| {
                let queued: Vec<Action> = std::mem::take(&mut *actions.lock().unwrap());
                for action in queued {
                    action(context.world());
                }
            },
        );
    }
}

/// A plugin of another kind, which asks its own question of whoever says `/ask`, and remembers the answers to any question that it hears.
struct Asker {
    answers: Arc<Mutex<Vec<PromptAnswered>>>,
}

impl std::fmt::Debug for Asker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Asker")
    }
}

impl Plugin for Asker {
    fn id(&self) -> NamespacedId {
        id("test:asker")
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        registrar.command(
            CommandSpec::new("ask", "asks a question of its own"),
            |context, client, _| {
                let _ = context.prompt(client, "Another question?", &["Yes", "No"]);
            },
        );
        let answers = Arc::clone(&self.answers);
        registrar.on::<PromptAnswered>(HandlerSpec::new("test:hear_answers"), move |event, _| {
            answers.lock().unwrap().push(*event)
        });
    }
}

/// Files that fail on the calls they are told to, and count the rest.
#[derive(Clone, Debug)]
pub struct Flaky {
    pub inner: MemoryShipFiles,
    pub failing: Arc<AtomicBool>,
    pub writes: Arc<AtomicUsize>,
}

impl ShipFiles for Flaky {
    fn read(&self, file: &ShipFileName) -> io::Result<Vec<u8>> {
        self.inner.read(file)
    }

    fn write(&mut self, file: &ShipFileName, bytes: &[u8]) -> io::Result<()> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        if self.failing.load(Ordering::SeqCst) {
            return Err(io::Error::other("the disk is full"));
        }
        self.inner.write(file, bytes)
    }

    fn remove(&mut self, file: &ShipFileName) -> io::Result<()> {
        self.inner.remove(file)
    }

    fn names(&self) -> io::Result<Vec<ShipFileName>> {
        self.inner.names()
    }
}

pub struct Rig {
    pub game: Game,
    pub server: Server,
    pub connector: LoopbackConnector,
    /// The connection of the player who is logged in (the first, and then whoever logged in last through `log_in_again`).
    pub connection: LoopbackConnection,
    pub presence: SharedPresence,
    pub actions: Arc<Mutex<Vec<Action>>>,
    pub files: MemoryShipFiles,
    pub report: Box<dyn Fn() -> LifecycleReport + Send>,
    /// The answers to questions that the other plugin heard (every plugin hears every answer).
    pub answers: Arc<Mutex<Vec<PromptAnswered>>>,
}

/// Where a player goes who chooses the world's spawn.
pub fn spawn() -> WorldPosition {
    WorldPosition::new(BrickCoordinate::new(0, 4, 0), [16.0, 8.0, 16.0])
}

/// Where a player goes who chooses the spawn of the second level, which is not the same place (a game's levels do not all have ground at the same height).
pub fn second_spawn() -> WorldPosition {
    WorldPosition::new(BrickCoordinate::new(3, 1, 0), [4.0, 20.0, 9.0])
}

pub fn second() -> LevelIndex {
    LevelIndex::new(1)
}

/// A ship of two bricks (the box from 0 to 64 along x and y and from 0 to 32 along z) at brick (`brick`, 0, 0).
pub fn ship_at(game: &Game, world: &WorldData, brick: i64) -> MovingGrid {
    let mut ship = game.sample_ship(world, 0);
    ship.pose = GridPose::new(
        WorldPosition::new(BrickCoordinate::new(brick, 0, 0), [0.0; 3]),
        Orientation::IDENTITY,
    );
    ship.held = false;
    ship
}

/// What the blocks of the pretend game are to something that rests on them: air is nothing and every other block is a cube.
pub fn collisions(game: &Game) -> BlockCollisions {
    let air = BlockDefinition::air();
    let cube = BlockDefinition::cube([0.5; 3]);
    let definitions: Vec<Option<&BlockDefinition>> = game
        .blocks
        .iter()
        .map(|(_, entry)| {
            Some(if *entry.identifier() == id("base_blocks:air") {
                &air
            } else {
                &cube
            })
        })
        .collect();
    BlockCollisions::new(BlockTable::from_definitions(&definitions), None)
}

impl Rig {
    /// A server with these settings, whose world has ships at these bricks along x (in the main level), and one player who has joined.
    pub fn new(settings: ShipSettings, ships: &[i64]) -> (Self, Vec<GridId>) {
        Self::with_files(settings, ships, MemoryShipFiles::new(), None)
    }

    pub fn with_files(
        settings: ShipSettings,
        ships: &[i64],
        files: MemoryShipFiles,
        flaky: Option<Flaky>,
    ) -> (Self, Vec<GridId>) {
        let ships = ships.to_vec();
        Self::assemble(
            settings,
            move |game, world| {
                ships
                    .iter()
                    .map(|&brick| {
                        let ship = ship_at(game, world, brick);
                        world.grids_mut().create(ship)
                    })
                    .collect()
            },
            files,
            flaky,
            false,
        )
    }

    /// A server whose lifecycle also looks at where a ship lands when the player who went away with it comes back, in a world that
    /// `prepare` has made what it should be (it returns the ships it made).
    pub fn for_arrivals(
        settings: ShipSettings,
        prepare: impl FnOnce(&Game, &mut WorldData) -> Vec<GridId>,
    ) -> (Self, Vec<GridId>) {
        Self::assemble(settings, prepare, MemoryShipFiles::new(), None, true)
    }

    /// The same, but the lifecycle does not look at where ships land.
    pub fn without_arrivals(
        settings: ShipSettings,
        prepare: impl FnOnce(&Game, &mut WorldData) -> Vec<GridId>,
    ) -> (Self, Vec<GridId>) {
        Self::assemble(settings, prepare, MemoryShipFiles::new(), None, false)
    }

    fn assemble(
        settings: ShipSettings,
        prepare: impl FnOnce(&Game, &mut WorldData) -> Vec<GridId>,
        files: MemoryShipFiles,
        flaky: Option<Flaky>,
        arrivals: bool,
    ) -> (Self, Vec<GridId>) {
        let game = Game::new(&ALL_PLUGINS);
        let mut world = game.new_world();
        world
            .add_level(
                id("test_content:second"),
                game.new_level("base_terrain:flat_v1"),
            )
            .unwrap();
        let made = prepare(&game, &mut world);
        let players = PlayersPlugin::new(true);
        let presence = players.presence();
        let lifecycle = GridLifecyclePlugin::new(presence.clone(), |_, level| {
            if level == second() {
                second_spawn()
            } else {
                spawn()
            }
        });
        let lifecycle = if arrivals {
            let layout = world.main().overrides.layout();
            lifecycle.with_arrivals(ArrivalConfig {
                collisions: collisions(&game),
                block_channel: layout.channel_index(&id("core:block")).unwrap(),
                state_channel: layout.channel_index(&id("core:temperature")).unwrap(),
                orientation_channel: None,
                air: game.raw_block("base_blocks:air"),
            })
        } else {
            lifecycle
        };
        let report = Box::new(lifecycle.report_handle());
        let actions: Arc<Mutex<Vec<Action>>> = Arc::default();
        let answers: Arc<Mutex<Vec<PromptAnswered>>> = Arc::default();
        let plugins: Vec<Box<dyn Plugin>> = vec![
            Box::new(players),
            Box::new(lifecycle),
            Box::new(Script {
                actions: Arc::clone(&actions),
            }),
            Box::new(Asker {
                answers: Arc::clone(&answers),
            }),
        ];
        let host = PluginHost::new(plugins, &world).unwrap();
        let (listener, connector) = loopback_listener();
        let server = Server::new(
            ServerConfig::new(id("test_game:game")).with_counted_challenges(1),
            world,
            &game.registries(),
            Box::new(host),
            Box::new(listener),
        )
        .unwrap();
        // (The player logs in over two ticks in which they have no place yet, and ships would be frozen and put away while nobody is near:
        // so the ships are kept wherever they are until they have joined.)
        let server = match flaky {
            Some(flaky) => server.with_ship_files(Box::new(flaky)),
            None => server.with_ship_files(Box::new(files.clone())),
        }
        .with_ship_settings(ShipSettings::everywhere());
        let mut server = server;
        let mut connection = connector.connect().unwrap();
        log_in(&mut server, &mut connection, &id("test_game:game"), 1);
        let server = server.with_ship_settings(settings);
        (
            Self {
                game,
                server,
                connector,
                connection,
                presence,
                actions,
                files,
                report,
                answers,
            },
            made,
        )
    }

    pub fn tick(&mut self) {
        self.server.step();
    }

    pub fn in_a_tick(&mut self, action: impl FnOnce(&mut ServerWorld) + Send + 'static) {
        self.actions.lock().unwrap().push(Box::new(action));
        self.server.step();
    }

    /// The player reports being this many voxels along x from the origin (and a little way up and along z), and a tick passes.
    pub fn player_at(&mut self, x: f64) {
        let brick = (x / 32.0).floor() as i64;
        let offset = (x - (brick * 32) as f64) as f32;
        self.connection
            .send(
                ClientMessage::GameMessage {
                    kind: PoseReport::kind(),
                    payload: PoseReport(PlayerPose {
                        position: WorldPosition::new(
                            BrickCoordinate::new(brick, 0, 0),
                            [offset, 5.0, 5.0],
                        ),
                        yaw: 0.0,
                        pitch: 0.0,
                    })
                    .encode(),
                }
                .encode(),
            )
            .unwrap();
        self.server.step();
    }

    /// The player says something to the server as a command, and a tick passes.
    pub fn say(&mut self, text: &str) {
        self.connection
            .send(
                ClientMessage::Command {
                    text: text.to_owned(),
                }
                .encode(),
            )
            .unwrap();
        self.tick();
    }

    /// The player logs out, and a tick passes.
    pub fn log_out(&mut self) {
        self.connection
            .send(ClientMessage::Goodbye.encode())
            .unwrap();
        self.tick();
        assert_eq!(self.server.connected_clients().count(), 0);
    }

    /// The player who is the install numbered `ordinal` logs in, and the server has done what it does when they join. Their connection is
    /// `self.connection` from now on.
    pub fn log_in_again(&mut self, ordinal: u8) {
        self.connection = self.log_in_as(ordinal);
    }

    /// The player who is the install numbered `ordinal` logs in on a connection of their own.
    pub fn log_in_as(&mut self, ordinal: u8) -> LoopbackConnection {
        let mut connection = self.connector.connect().unwrap();
        log_in(
            &mut self.server,
            &mut connection,
            &id("test_game:game"),
            ordinal,
        );
        connection
    }

    /// The only player who is logged in.
    pub fn client(&self) -> ClientId {
        let mut clients = self.server.connected_clients();
        let client = clients.next().expect("a player is logged in");
        assert!(clients.next().is_none(), "only one player is logged in");
        client
    }

    /// Whether the ship is in memory and frozen (one in a file is not).
    pub fn frozen(&self, ship: GridId) -> bool {
        self.server
            .world()
            .grids()
            .get(ship)
            .is_some_and(|ship| ship.frozen)
    }

    pub fn in_memory(&self, ship: GridId) -> bool {
        self.server.world().grids().get(ship).is_some()
    }

    pub fn in_a_file(&self, ship: GridId) -> bool {
        self.server.world().grids().get_parked(ship).is_some()
    }

    pub fn report(&self) -> LifecycleReport {
        (self.report)()
    }
}

/// What a player has been sent since they last looked.
#[derive(Debug, Default)]
pub struct Heard {
    pub messages: Vec<ServerMessage>,
}

impl Heard {
    /// Questions that were asked, in order.
    pub fn questions(&self) -> Vec<Prompt> {
        self.game_messages(Prompt::kind(), |payload| Prompt::decode(payload).unwrap())
    }

    /// The endings of questions, in order.
    pub fn closed(&self) -> Vec<PromptClosed> {
        self.game_messages(PromptClosed::kind(), |payload| {
            PromptClosed::decode(payload).unwrap()
        })
    }

    /// Where the player was told they are, in order.
    pub fn placed(&self) -> Vec<WorldPosition> {
        self.game_messages(PlayerPlaced::kind(), |payload| {
            PlayerPlaced::decode(payload).unwrap().0
        })
    }

    /// Why the player was disconnected, if they were.
    pub fn disconnected(&self) -> Option<String> {
        self.messages.iter().find_map(|message| match message {
            ServerMessage::Disconnected { reason } => Some(reason.clone()),
            _ => None,
        })
    }

    /// What the server said to the player in answer to a command, in order.
    pub fn replies(&self) -> Vec<String> {
        self.messages
            .iter()
            .filter_map(|message| match message {
                ServerMessage::CommandReply { text } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn game_messages<T>(&self, wanted: NamespacedId, decode: impl Fn(&[u8]) -> T) -> Vec<T> {
        self.messages
            .iter()
            .filter_map(|message| match message {
                ServerMessage::GameMessage { kind, payload } if *kind == wanted => {
                    Some(decode(payload))
                }
                _ => None,
            })
            .collect()
    }
}

/// Takes in everything the server has sent on a connection since the last time.
pub fn hear(connection: &mut LoopbackConnection) -> Heard {
    let mut messages = Vec::new();
    while let Some(frame) = connection.receive().unwrap_or(None) {
        messages.push(ServerMessage::decode(&frame).unwrap());
    }
    Heard { messages }
}

/// The player answers a question: the option numbered from 0.
pub fn answer(connection: &mut LoopbackConnection, question: PromptId, choice: u32) {
    connection
        .send(
            ClientMessage::GameMessage {
                kind: PromptAnswer::kind(),
                payload: PromptAnswer {
                    id: question,
                    choice,
                }
                .encode(),
            }
            .encode(),
        )
        .unwrap();
}
