use std::fmt::Debug;

use std::sync::Arc;

use entities::EntityCatalog;
use namespaced_id::NamespacedId;
use protocol::{ClientId, Tick};
use world_storage::Section;

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

    /// A client sent a line of text as a command (see [`CommandLine`](crate::CommandLine) for taking it
    /// apart). Answer it with [`ServerWorld::reply`]. Commands arrive with the client's other
    /// messages, in the order sent. The default says the game has no commands.
    fn command_received(&mut self, client: ClientId, _command: &str, world: &mut ServerWorld) {
        world.reply(client, "this game has no commands");
    }

    /// Advances the game by one tick, after the tick's incoming messages.
    fn tick(&mut self, tick: Tick, world: &mut ServerWorld);

    /// The kinds of entity and the items the game has, which the server needs to make entities, to tell
    /// clients of them and to save them. Asked once, when the server is built. The default is none.
    fn entity_catalog(&self) -> Arc<EntityCatalog> {
        Arc::default()
    }

    /// The game's own state that is not in the voxels (a score, a counter), as world file sections
    /// to save with the world. A game restores it from the world's plugin sections when it is built.
    fn save_sections(&self) -> Vec<Section> {
        Vec::new()
    }
}
