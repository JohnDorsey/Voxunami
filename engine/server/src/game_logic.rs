use std::fmt::Debug;

use namespaced_id::NamespacedId;
use protocol::{ClientId, Tick};

use crate::server_world::ServerWorld;

/// What a game (with its plugins) does on the server. The server calls it from one thread, in a
/// fixed order, and nothing in it may depend on wall-clock time or other sources of
/// nondeterminism: the same inputs must give the same world on every machine.
pub trait GameLogic: Debug + Send {
    /// A client finished its handshake. Games typically add a player for it.
    fn client_joined(&mut self, client: ClientId, world: &mut ServerWorld);

    /// A client left, however it went. Games typically remove its player.
    fn client_left(&mut self, client: ClientId, world: &mut ServerWorld);

    /// A client sent a game message. Messages arrive in the order each client sent them, clients in
    /// the order of their ids.
    fn message_received(
        &mut self,
        client: ClientId,
        kind: &NamespacedId,
        payload: &[u8],
        world: &mut ServerWorld,
    );

    /// Advances the game by one tick, after the tick's incoming messages.
    fn tick(&mut self, tick: Tick, world: &mut ServerWorld);
}
