//! Game logic driven by closures, for tests of servers and of code that talks to them. Enabled by
//! the `test-support` feature.

use std::fmt;
use std::sync::{Arc, Mutex};

use entities::EntityCatalog;
use identity::Identity;
use namespaced_id::NamespacedId;
use protocol::{ClientId, Tick};
use transport::Connection;
use world_storage::Section;

use crate::{GameLogic, Server, ServerWorld};

/// What the server told the game logic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogicEvent {
    Joined(ClientId),
    Left(ClientId),
    Message {
        client: ClientId,
        kind: NamespacedId,
        payload: Vec<u8>,
    },
    Command {
        client: ClientId,
        text: String,
    },
    Tick(Tick),
}

type TickAction = Box<dyn FnMut(Tick, &mut ServerWorld) + Send>;
type MessageAction = Box<dyn FnMut(ClientId, &NamespacedId, &[u8], &mut ServerWorld) + Send>;
type ClientAction = Box<dyn FnMut(ClientId, &mut ServerWorld) + Send>;
type CommandAction = Box<dyn FnMut(ClientId, &str, &mut ServerWorld) + Send>;

/// Game logic that records every call it receives and otherwise does only what the test gives it.
#[derive(Default)]
pub struct ScriptedLogic {
    events: Arc<Mutex<Vec<LogicEvent>>>,
    on_tick: Option<TickAction>,
    on_message: Option<MessageAction>,
    on_join: Option<ClientAction>,
    on_command: Option<CommandAction>,
    saved_sections: Vec<Section>,
    catalog: Arc<EntityCatalog>,
}

impl fmt::Debug for ScriptedLogic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ScriptedLogic")
    }
}

impl ScriptedLogic {
    pub fn new() -> Self {
        Self::default()
    }

    /// Called at every tick, after the tick's messages.
    pub fn on_tick(mut self, action: impl FnMut(Tick, &mut ServerWorld) + Send + 'static) -> Self {
        self.on_tick = Some(Box::new(action));
        self
    }

    pub fn on_message(
        mut self,
        action: impl FnMut(ClientId, &NamespacedId, &[u8], &mut ServerWorld) + Send + 'static,
    ) -> Self {
        self.on_message = Some(Box::new(action));
        self
    }

    pub fn on_join(
        mut self,
        action: impl FnMut(ClientId, &mut ServerWorld) + Send + 'static,
    ) -> Self {
        self.on_join = Some(Box::new(action));
        self
    }

    /// Called when a client sends a command. Without it, commands are answered with the default
    /// text saying the game has none.
    pub fn on_command(
        mut self,
        action: impl FnMut(ClientId, &str, &mut ServerWorld) + Send + 'static,
    ) -> Self {
        self.on_command = Some(Box::new(action));
        self
    }

    /// A section the logic saves with the world, as a game saves its own state.
    pub fn saving(mut self, section: Section) -> Self {
        self.saved_sections.push(section);
        self
    }

    /// The kinds of entity and the items the logic has.
    pub fn with_entity_catalog(mut self, catalog: EntityCatalog) -> Self {
        self.catalog = Arc::new(catalog);
        self
    }

    /// A handle to the log of calls, which stays usable after the logic moves into a server.
    pub fn events(&self) -> Arc<Mutex<Vec<LogicEvent>>> {
        Arc::clone(&self.events)
    }

    fn record(&self, event: LogicEvent) {
        self.events
            .lock()
            .expect("no test panics while logging")
            .push(event);
    }
}

impl GameLogic for ScriptedLogic {
    fn client_joined(&mut self, client: ClientId, world: &mut ServerWorld) {
        self.record(LogicEvent::Joined(client));
        if let Some(action) = &mut self.on_join {
            action(client, world);
        }
    }

    fn client_left(&mut self, client: ClientId, _world: &mut ServerWorld) {
        self.record(LogicEvent::Left(client));
    }

    fn message_received(
        &mut self,
        client: ClientId,
        kind: &NamespacedId,
        payload: &[u8],
        world: &mut ServerWorld,
    ) {
        self.record(LogicEvent::Message {
            client,
            kind: kind.clone(),
            payload: payload.to_vec(),
        });
        if let Some(action) = &mut self.on_message {
            action(client, kind, payload, world);
        }
    }

    fn command_received(&mut self, client: ClientId, command: &str, world: &mut ServerWorld) {
        self.record(LogicEvent::Command {
            client,
            text: command.to_owned(),
        });
        match &mut self.on_command {
            Some(action) => action(client, command, world),
            None => world.reply(client, "this game has no commands"),
        }
    }

    fn save_sections(&self) -> Vec<Section> {
        self.saved_sections.clone()
    }

    fn entity_catalog(&self) -> Arc<EntityCatalog> {
        Arc::clone(&self.catalog)
    }

    fn tick(&mut self, tick: Tick, world: &mut ServerWorld) {
        self.record(LogicEvent::Tick(tick));
        if let Some(action) = &mut self.on_tick {
            action(tick, world);
        }
    }
}

/// The identity of the `ordinal`-th test client: the same every run, and different for each ordinal.
pub fn test_identity(ordinal: u8) -> Identity {
    Identity::from_secret([ordinal; 32])
}

/// Logs a test client in as `test_identity(ordinal)`: it says hello, the server steps and challenges it, it
/// answers, and the server steps and welcomes it. What the server sent from the welcome on is left to read.
pub fn log_in(
    server: &mut Server,
    connection: &mut impl Connection,
    game: &NamespacedId,
    ordinal: u8,
) {
    let identity = test_identity(ordinal);
    connection
        .send(identity.hello(game).encode())
        .expect("the server is listening");
    server.step();
    let frame = connection
        .receive()
        .expect("the server is listening")
        .expect("the server challenged the client");
    let proof = identity
        .answer(game, &frame)
        .expect("the first thing the server sends is its challenge");
    connection
        .send(proof.encode())
        .expect("the server is listening");
    server.step();
}
