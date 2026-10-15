//! Running a game's server: inside the game's own process for singleplayer, with a door open to the
//! network for a hosted game, or on its own as a dedicated server.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use server::{Server, ServerConfig, ServerError};
use transport::loopback::{LoopbackConnection, LoopbackConnector, loopback_listener};
use transport::tcp::{TcpConnection, TcpServerListener};
use transport::{ConnectionClosed, Listener, ListenerSet};
use world_storage::WorldData;
use world_storage::file::write_atomically;

use crate::definition::{GameContent, GameDefinition};

pub const TICKS_PER_SECOND: u32 = 20;

/// How far beyond the drawn distance a client watches, so every drawn brick has its neighbours.
pub const WATCH_MARGIN_BRICKS: i64 = 2;

/// The port servers listen on, and clients connect to, unless told otherwise.
pub const DEFAULT_PORT: u16 = 24816;

fn server_config<G: GameDefinition>(view_radius_bricks: i64) -> ServerConfig {
    let mut config = ServerConfig::new(G::id());
    config.ticks_per_second = TICKS_PER_SECOND;
    let edge = (2 * (view_radius_bricks + WATCH_MARGIN_BRICKS) + 1) as u64;
    config.maximum_watched_bricks = edge * edge * edge;
    config
}

fn new_server<G: GameDefinition>(
    content: &G::Content,
    world: WorldData,
    view_radius_bricks: i64,
    listener: Box<dyn Listener>,
) -> Result<Server, ServerError> {
    let logic = G::new_logic(content, &world);
    Server::new(
        server_config::<G>(view_radius_bricks),
        world,
        &content.registries(),
        logic,
        listener,
    )
}

/// A server for `world`, and the way to connect to it from this process. The caller decides when
/// it ticks.
pub fn build_server<G: GameDefinition>(
    content: &G::Content,
    world: WorldData,
    view_radius_bricks: i64,
) -> Result<(Server, LoopbackConnector), ServerError> {
    let (listener, connector) = loopback_listener();
    let server = new_server::<G>(content, world, view_radius_bricks, Box::new(listener))?;
    Ok((server, connector))
}

/// Starts accepting players on the network at `address`.
pub fn listen_on(address: &str) -> Result<TcpServerListener, String> {
    let listener = TcpServerListener::bind(address)
        .map_err(|error| format!("cannot listen on {address}: {error}"))?;
    if let Ok(bound) = listener.local_address() {
        println!("accepting players on {bound}");
    }
    Ok(listener)
}

/// Connects to a server on the network.
pub fn connect_to(address: &str) -> Result<TcpConnection, String> {
    TcpConnection::connect(address, Duration::from_secs(10))
        .map_err(|error| format!("cannot connect to {address}: {error}"))
}

/// Saves the world to a file at regular intervals while the server runs.
#[derive(Clone, Debug)]
pub struct Autosave {
    pub interval: Duration,
    pub path: PathBuf,
}

/// How a server thread is set up beyond its world.
pub struct HostSettings {
    /// How far its players see; it limits how much of the world one player may watch.
    pub view_radius_bricks: i64,
    /// Where players on other machines connect, if anywhere.
    pub network: Option<Box<dyn Listener>>,
    pub autosave: Option<Autosave>,
    /// Print a line when a player joins or leaves.
    pub announce_players: bool,
}

impl HostSettings {
    /// A server for the player at this computer only.
    pub fn local(view_radius_bricks: i64) -> Self {
        Self {
            view_radius_bricks,
            network: None,
            autosave: None,
            announce_players: false,
        }
    }
}

impl fmt::Debug for HostSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HostSettings")
            .field("view_radius_bricks", &self.view_radius_bricks)
            .field("network", &self.network.is_some())
            .field("autosave", &self.autosave)
            .finish_non_exhaustive()
    }
}

/// A server ticking on its own thread at the game's tick rate. It owns the game's content while it
/// runs and hands it back, with the world, when it stops.
pub struct ServerThread<G: GameDefinition> {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<(WorldData, G::Content)>>,
    connector: LoopbackConnector,
}

impl<G: GameDefinition> fmt::Debug for ServerThread<G> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServerThread")
            .field("running", &self.thread.is_some())
            .finish_non_exhaustive()
    }
}

impl<G: GameDefinition> ServerThread<G> {
    pub fn start(
        content: G::Content,
        world: WorldData,
        settings: HostSettings,
    ) -> Result<Self, ServerError> {
        let (loopback, connector) = loopback_listener();
        let mut listeners = ListenerSet::new().with(loopback);
        if let Some(network) = settings.network {
            listeners = listeners.with_boxed(network);
        }
        let mut server = new_server::<G>(
            &content,
            world,
            settings.view_radius_bricks,
            Box::new(listeners),
        )?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let (autosave, announce) = (settings.autosave, settings.announce_players);
        let thread = std::thread::spawn(move || {
            // The clock only paces the loop; the server itself never reads it.
            let tick_length = Duration::from_secs_f64(1.0 / f64::from(TICKS_PER_SECOND));
            let mut next_tick = Instant::now();
            let mut last_save = Instant::now();
            while !stopped.load(Ordering::Relaxed) {
                let report = server.step();
                if announce {
                    for client in &report.joined {
                        println!("{client} joined");
                    }
                    for (client, reason) in &report.left {
                        println!("{client} left ({reason:?})");
                    }
                }
                if let Some(autosave) = &autosave
                    && last_save.elapsed() >= autosave.interval
                {
                    last_save = Instant::now();
                    match server.save(&content.registries()) {
                        Ok(bytes) => {
                            if let Err(error) = write_atomically(&autosave.path, &bytes) {
                                eprintln!(
                                    "autosave to {} failed: {error}",
                                    autosave.path.display()
                                );
                            }
                        }
                        Err(error) => eprintln!("autosave failed: {error}"),
                    }
                }
                next_tick += tick_length;
                let now = Instant::now();
                if next_tick > now {
                    std::thread::sleep(next_tick - now);
                } else if now - next_tick > Duration::from_secs(1) {
                    next_tick = now; // far behind: skip ahead rather than race to catch up
                }
            }
            (server.into_world(), content)
        });
        Ok(Self {
            stop,
            thread: Some(thread),
            connector,
        })
    }

    /// Connects the player at this computer.
    pub fn connect(&self) -> Result<LoopbackConnection, ConnectionClosed> {
        self.connector.connect()
    }

    /// Stops ticking and returns the world, to be saved, and the content.
    pub fn stop(mut self) -> (WorldData, G::Content) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread
            .take()
            .expect("the thread runs until stopped")
            .join()
            .expect("the server thread does not panic")
    }
}

impl<G: GameDefinition> Drop for ServerThread<G> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
