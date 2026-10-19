use identity::Identity;
use namespaced_id::NamespacedId;
use protocol::{ClientMessage, PROTOCOL_VERSION, ServerMessage, Tick, Welcome};
use transport::{Connection, ConnectionClosed};
use voxel_space::{BrickBox, LevelIndex};
use world_storage::{LoadContext, LoadReport};

use crate::client_world::{ClientEvent, ClientWorld};
use crate::error::ClientError;

/// Connects to a server: sends the hello, answers the server's challenge with a proof that this
/// install holds the private key of the identity it said it is, then waits for the welcome.
///
/// Connecting happens in steps because the program must build its terrain generators from the
/// world's seed, which only the welcome carries: [`Handshake::start`], then [`Handshake::poll`]
/// until the welcome arrives (the server's challenge, which `poll` answers, comes first, so a join
/// takes the server two ticks), then [`Welcomed::finish`] with the program's content.
#[derive(Debug)]
pub struct Handshake<C: Connection> {
    connection: C,
    game: NamespacedId,
    identity: Identity,
}

/// The outcome of polling a handshake.
#[derive(Debug)]
pub enum HandshakeProgress<C: Connection> {
    /// The welcome has not arrived yet; poll again later.
    Waiting(Handshake<C>),
    Welcomed(Welcomed<C>),
}

impl<C: Connection> Handshake<C> {
    /// Sends the hello for `game`, saying who this player is.
    pub fn start(
        mut connection: C,
        game: NamespacedId,
        identity: &Identity,
    ) -> Result<Self, ClientError> {
        connection
            .send(
                ClientMessage::Hello {
                    protocol_version: PROTOCOL_VERSION,
                    game: game.clone(),
                    player_key: identity.player_key(),
                }
                .encode(),
            )
            .map_err(|ConnectionClosed| ClientError::ConnectionClosed)?;
        Ok(Self {
            connection,
            game,
            identity: identity.clone(),
        })
    }

    /// Takes in the next message from the server: answers its challenge, or checks whether the welcome
    /// has arrived. A refusal from the server is an error carrying its reason. (A recording made before
    /// there were challenges starts with the welcome, which is accepted as it is.)
    pub fn poll(mut self) -> Result<HandshakeProgress<C>, ClientError> {
        let Some(frame) = self
            .connection
            .receive()
            .map_err(|ConnectionClosed| ClientError::ConnectionClosed)?
        else {
            return Ok(HandshakeProgress::Waiting(self));
        };
        match ServerMessage::decode(&frame)? {
            ServerMessage::Challenge { nonce } => {
                let signature = self.identity.prove(&self.game, &nonce);
                self.connection
                    .send(ClientMessage::Prove { signature }.encode())
                    .map_err(|ConnectionClosed| ClientError::ConnectionClosed)?;
                Ok(HandshakeProgress::Waiting(self))
            }
            ServerMessage::Welcome(welcome) => Ok(HandshakeProgress::Welcomed(Welcomed {
                connection: self.connection,
                welcome,
            })),
            ServerMessage::Disconnected { reason } => {
                Err(ClientError::DisconnectedByServer(reason))
            }
            _ => Err(ClientError::UnexpectedMessage(
                "anything but a challenge or a welcome while connecting",
            )),
        }
    }
}

/// A connection whose welcome has arrived, waiting for the program to supply its content.
#[derive(Debug)]
pub struct Welcomed<C: Connection> {
    connection: C,
    welcome: Welcome,
}

impl<C: Connection> Welcomed<C> {
    /// The world's seed, which terrain generators need.
    pub fn world_seed(&self) -> Result<u64, ClientError> {
        Ok(world_storage::ParsedWorld::parse(&self.welcome.world_description)?.world_seed())
    }

    pub fn welcome(&self) -> &Welcome {
        &self.welcome
    }

    /// Builds the client's view of the world from `context`, the program's own content. Content the
    /// server has and the program lacks becomes placeholders; the report lists it.
    pub fn finish(self, context: LoadContext<'_>) -> Result<(Client<C>, LoadReport), ClientError> {
        let (world, report) = ClientWorld::from_welcome(self.welcome, context)?;
        Ok((
            Client {
                connection: self.connection,
                world,
                incomplete_tick: Vec::new(),
            },
            report,
        ))
    }
}

/// What one completed server tick changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickUpdate {
    pub tick: Tick,
    /// In the order the server sent them.
    pub events: Vec<ClientEvent>,
}

/// A connected client. It keeps the world view up to date, one whole server tick at a time, and
/// sends the player's requests to the server.
#[derive(Debug)]
pub struct Client<C: Connection> {
    connection: C,
    world: ClientWorld,
    /// Messages of the tick still arriving; applied when its `TickCompleted` comes.
    incomplete_tick: Vec<ServerMessage>,
}

impl<C: Connection> Client<C> {
    pub fn world(&self) -> &ClientWorld {
        &self.world
    }

    /// The connection, for what only it knows: why a replay ended, or whether a recording failed.
    pub fn connection(&self) -> &C {
        &self.connection
    }

    /// Applies the next complete tick the server has sent, if one has fully arrived. Call it until it
    /// returns `None` to catch up. The view never shows half a tick.
    pub fn poll_tick(&mut self) -> Result<Option<TickUpdate>, ClientError> {
        loop {
            let frame = self
                .connection
                .receive()
                .map_err(|ConnectionClosed| ClientError::ConnectionClosed)?;
            let Some(frame) = frame else {
                return Ok(None);
            };
            let message = ServerMessage::decode(&frame)?;
            if !matches!(message, ServerMessage::TickCompleted { .. }) {
                self.incomplete_tick.push(message);
                continue;
            }
            let mut events = Vec::new();
            for earlier in std::mem::take(&mut self.incomplete_tick) {
                events.extend(self.world.apply(earlier)?);
            }
            self.world.apply(message)?;
            return Ok(Some(TickUpdate {
                tick: self.world.tick(),
                events,
            }));
        }
    }

    /// Asks the server to send, and keep up to date, the bricks of `region` of `level`.
    pub fn watch(&mut self, level: LevelIndex, region: BrickBox) -> Result<(), ClientError> {
        self.send(&ClientMessage::SetInterest { level, region })
    }

    /// Sends a line of text as a command. The server's answer arrives as a
    /// [`ClientEvent::CommandReply`](crate::ClientEvent::CommandReply).
    pub fn send_command(&mut self, text: &str) -> Result<(), ClientError> {
        self.send(&ClientMessage::Command {
            text: text.to_owned(),
        })
    }

    /// Sends a message for the game or one of its plugins.
    pub fn send_game_message(
        &mut self,
        kind: NamespacedId,
        payload: Vec<u8>,
    ) -> Result<(), ClientError> {
        self.send(&ClientMessage::GameMessage { kind, payload })
    }

    /// Tells the server this client is leaving.
    pub fn say_goodbye(&mut self) -> Result<(), ClientError> {
        self.send(&ClientMessage::Goodbye)
    }

    fn send(&mut self, message: &ClientMessage) -> Result<(), ClientError> {
        self.connection
            .send(message.encode())
            .map_err(|ConnectionClosed| ClientError::ConnectionClosed)
    }
}
