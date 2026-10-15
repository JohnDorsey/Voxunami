//! Running the game's server inside the game's own process: what singleplayer is.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use server::{Server, ServerConfig, ServerError};
use transport::ConnectionClosed;
use transport::loopback::{LoopbackConnection, LoopbackConnector, loopback_listener};
use world_storage::WorldData;

use crate::content::{CaveContent, game_id};
use crate::logic::CaveLogic;

pub const TICKS_PER_SECOND: u32 = 20;

/// How far beyond the drawn distance a client watches, so every drawn brick has its neighbours.
pub const WATCH_MARGIN_BRICKS: i64 = 2;

/// A server for `world`, and the way to connect to it. The caller decides when it ticks.
pub fn build_server(
    content: &CaveContent,
    world: WorldData,
    view_radius_bricks: i64,
) -> Result<(Server, LoopbackConnector), ServerError> {
    let (listener, connector) = loopback_listener();
    let mut config = ServerConfig::new(game_id());
    config.ticks_per_second = TICKS_PER_SECOND;
    let edge = (2 * (view_radius_bricks + WATCH_MARGIN_BRICKS) + 1) as u64;
    config.maximum_watched_bricks = edge * edge * edge;
    let server = Server::new(
        config,
        world,
        &content.registries(),
        Box::new(CaveLogic::new(content)),
        Box::new(listener),
    )?;
    Ok((server, connector))
}

/// A server ticking on its own thread at the game's tick rate.
#[derive(Debug)]
pub struct IntegratedServer {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<WorldData>>,
    connector: LoopbackConnector,
}

impl IntegratedServer {
    pub fn start(
        content: &CaveContent,
        world: WorldData,
        view_radius_bricks: i64,
    ) -> Result<Self, ServerError> {
        let (mut server, connector) = build_server(content, world, view_radius_bricks)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            // The clock only paces the loop; the server itself never reads it.
            let tick_length = Duration::from_secs_f64(1.0 / f64::from(TICKS_PER_SECOND));
            let mut next_tick = Instant::now();
            while !stopped.load(Ordering::Relaxed) {
                server.step();
                next_tick += tick_length;
                let now = Instant::now();
                if next_tick > now {
                    std::thread::sleep(next_tick - now);
                } else if now - next_tick > Duration::from_secs(1) {
                    next_tick = now; // far behind: skip ahead rather than race to catch up
                }
            }
            server.into_world()
        });
        Ok(Self {
            stop,
            thread: Some(thread),
            connector,
        })
    }

    pub fn connect(&self) -> Result<LoopbackConnection, ConnectionClosed> {
        self.connector.connect()
    }

    /// Stops ticking and returns the world, to be saved.
    pub fn stop(mut self) -> WorldData {
        self.stop.store(true, Ordering::Relaxed);
        self.thread
            .take()
            .expect("the thread runs until stopped")
            .join()
            .expect("the server thread does not panic")
    }
}

impl Drop for IntegratedServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
