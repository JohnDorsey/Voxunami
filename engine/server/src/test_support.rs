//! Game logic driven by closures, for tests of servers and of code that talks to them. Enabled by
//! the `test-support` feature.

use std::fmt;
use std::sync::{Arc, Mutex};

use namespaced_id::NamespacedId;
use protocol::{ClientId, Tick};

use crate::{GameLogic, ServerWorld};

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
    Tick(Tick),
}

type TickAction = Box<dyn FnMut(Tick, &mut ServerWorld) + Send>;
type MessageAction = Box<dyn FnMut(ClientId, &NamespacedId, &[u8], &mut ServerWorld) + Send>;
type ClientAction = Box<dyn FnMut(ClientId, &mut ServerWorld) + Send>;

/// Game logic that records every call it receives and otherwise does only what the test gives it.
#[derive(Default)]
pub struct ScriptedLogic {
    events: Arc<Mutex<Vec<LogicEvent>>>,
    on_tick: Option<TickAction>,
    on_message: Option<MessageAction>,
    on_join: Option<ClientAction>,
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

    fn tick(&mut self, tick: Tick, world: &mut ServerWorld) {
        self.record(LogicEvent::Tick(tick));
        if let Some(action) = &mut self.on_tick {
            action(tick, world);
        }
    }
}
