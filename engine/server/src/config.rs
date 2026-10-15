use namespaced_id::NamespacedId;

/// How a server is set up. Everything here is a property of the game or the host, not of the world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerConfig {
    /// The game this server runs. Clients of another game are refused.
    pub game: NamespacedId,
    pub ticks_per_second: u32,
    /// The most bricks one client may watch at once. A request for more disconnects the client,
    /// since the server would otherwise spend unbounded time on it.
    pub maximum_watched_bricks: u64,
    /// A client that has connected but not said hello within this many ticks is dropped.
    pub handshake_timeout_ticks: u32,
}

impl ServerConfig {
    /// The default tick rate and limits: 20 ticks per second, a 33-brick-wide cube of bricks per
    /// client, and ten seconds to say hello.
    pub fn new(game: NamespacedId) -> Self {
        Self {
            game,
            ticks_per_second: 20,
            maximum_watched_bricks: 33 * 33 * 33,
            handshake_timeout_ticks: 200,
        }
    }
}
