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
    /// The most bricks of ships that may be in one client's picture. A ship that would take it past this is not sent, and the
    /// client sees nothing of it (levels of detail will fill that gap).
    pub maximum_ship_bricks: u64,
    /// A client that has connected but not finished logging in (the hello, then the proof of its identity)
    /// within this many ticks is dropped.
    pub handshake_timeout_ticks: u32,
    /// Where the challenges players prove their identity against come from: `None` for the operating
    /// system's randomness, which a real server must use; `Some(seed)` for a counter, so that a test's
    /// stream is the same every time.
    pub challenge_seed: Option<u64>,
    /// What every client is sent in its welcome besides the world's description: the game's package data, which the
    /// server passes on without reading.
    pub package: Vec<u8>,
}

impl ServerConfig {
    /// The default tick rate and limits: 20 ticks per second, a 33-brick-wide cube of bricks per
    /// client, and ten seconds to say hello.
    pub fn new(game: NamespacedId) -> Self {
        Self {
            game,
            ticks_per_second: 20,
            maximum_watched_bricks: 33 * 33 * 33,
            maximum_ship_bricks: 16 * 1024,
            handshake_timeout_ticks: 200,
            challenge_seed: None,
            package: Vec::new(),
        }
    }

    /// The same, sending clients this package data in their welcome.
    pub fn with_package(mut self, package: Vec<u8>) -> Self {
        self.package = package;
        self
    }

    /// The same, with challenges from a counter: for tests, whose streams must not change from run to run.
    pub fn with_counted_challenges(mut self, seed: u64) -> Self {
        self.challenge_seed = Some(seed);
        self
    }
}
