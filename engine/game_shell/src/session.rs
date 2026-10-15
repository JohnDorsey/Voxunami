//! A player's side of a game, without a window: the client, the meshes of what it sees, the camera,
//! and the messages the player's actions turn into. Windows and headless runs both drive one.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use client::{Client, ClientError, ClientEvent, Handshake, HandshakeProgress};
use namespaced_id::NamespacedId;
use player_presence::{PlayerPose, decode_players, encode_pose, players_kind, pose_kind};
use protocol::ClientId;
use renderer::Camera;
use transport::Connection;
use voxel_space::{BrickBox, BrickCoordinate};
use world_view::{MeshUpdate, RayHit, ViewSettings, WorldMesher, cast_ray};

use crate::definition::{GameContent, GameDefinition};
use crate::host::WATCH_MARGIN_BRICKS;

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error("the server did not answer in time")]
    NoWelcome,
    #[error("the world around the camera did not finish loading in time")]
    LoadingTimedOut,
}

/// Whether the player controls the camera, or the camera follows what a recording captured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Playing,
    /// Watching a replay: the camera is wherever the recorded player was.
    Replaying,
}

#[derive(Debug)]
pub struct Session<C: Connection, G: GameDefinition> {
    client: Client<C>,
    content: G::Content,
    mesher: WorldMesher,
    pub camera: Camera,
    /// What the game keeps on the client between messages and clicks.
    pub state: G::ClientState,
    players: BTreeMap<ClientId, PlayerPose>,
    mode: Mode,
    view_radius_bricks: i64,
    watched_center: Option<BrickCoordinate>,
    last_pose_sent: Option<PlayerPose>,
}

impl<C: Connection, G: GameDefinition> Session<C, G> {
    /// Connects and builds the player's picture of the world. `wait` is called between attempts to
    /// hear the server's welcome, and is where a program that ticks its own server steps it.
    pub fn connect(
        connection: C,
        mode: Mode,
        view_radius_bricks: i64,
        start: Camera,
        mut wait: impl FnMut(),
    ) -> Result<Self, SessionError> {
        let mut handshake = Handshake::start(connection, G::id())?;
        let deadline = Instant::now() + Duration::from_secs(15);
        let welcomed = loop {
            match handshake.poll()? {
                HandshakeProgress::Welcomed(welcomed) => break welcomed,
                HandshakeProgress::Waiting(waiting) => handshake = waiting,
            }
            if Instant::now() > deadline {
                return Err(SessionError::NoWelcome);
            }
            wait();
        };
        let mut content = G::new_content();
        let seed = welcomed.world_seed()?;
        let (client, _report) = welcomed.finish(content.load_context(seed))?;

        let opacity = content.opacity_table();
        let worker_count = std::thread::available_parallelism()
            .map_or(2, |count| count.get().saturating_sub(1).max(2));
        let mesher = WorldMesher::new(
            ViewSettings {
                view_radius_bricks,
                worker_count,
            },
            content.material_channel(),
            // Values the content does not list are drawn, so unknown content shows rather than vanishes.
            move |value| opacity.get(value as usize).copied().unwrap_or(true),
        );
        Ok(Self {
            client,
            content,
            mesher,
            camera: start,
            state: G::ClientState::default(),
            players: BTreeMap::new(),
            mode,
            view_radius_bricks,
            watched_center: None,
            last_pose_sent: None,
        })
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn client(&self) -> &Client<C> {
        &self.client
    }

    pub fn content(&self) -> &G::Content {
        &self.content
    }

    pub fn mesher(&self) -> &WorldMesher {
        &self.mesher
    }

    /// Where every player is, as far as the server has said.
    pub fn players(&self) -> &BTreeMap<ClientId, PlayerPose> {
        &self.players
    }

    /// Applies the next complete tick the server has sent, if any. Returns whether there was one.
    pub fn advance_one_tick(&mut self) -> Result<bool, ClientError> {
        let Some(update) = self.client.poll_tick()? else {
            return Ok(false);
        };
        for event in update.events {
            match event {
                ClientEvent::BrickChanged { brick } => self.mesher.brick_changed(brick),
                ClientEvent::GameMessage { kind, payload } if kind == players_kind() => {
                    // A message this client cannot read is dropped; the next snapshot replaces it.
                    if let Ok(players) = decode_players(&payload) {
                        self.players = players.into_iter().collect();
                    }
                }
                ClientEvent::GameMessage { kind, payload } => {
                    G::handle_game_message(&mut self.state, &kind, &payload);
                }
                ClientEvent::WatchedRegionChanged { .. } => {}
            }
        }
        if self.mode == Mode::Replaying {
            self.follow_recorded_player();
        }
        Ok(true)
    }

    /// Applies every tick that has arrived. Returns how many.
    pub fn advance(&mut self) -> Result<usize, ClientError> {
        let mut ticks = 0;
        while self.advance_one_tick()? {
            ticks += 1;
        }
        Ok(ticks)
    }

    fn follow_recorded_player(&mut self) {
        let own = self.client.world().server_info().client;
        if let Some(pose) = self.players.get(&own) {
            self.camera.position = pose.position;
            self.camera.yaw = pose.yaw;
            self.camera.pitch = pose.pitch;
        }
    }

    /// Keeps the watched region around the camera, and the meshes around it up to date. Returns what
    /// the renderer must change.
    pub fn update_view(&mut self) -> Vec<MeshUpdate> {
        let center = self.camera.position.brick();
        if self.mode == Mode::Playing && self.watched_center != Some(center) {
            let region = BrickBox::around(
                center,
                (self.view_radius_bricks + WATCH_MARGIN_BRICKS) as u32,
            );
            // A closed connection shows up in the next `advance`, which reports it.
            let _ = self.client.watch(region);
            self.watched_center = Some(center);
        }
        let world = self.client.world().world();
        self.mesher.update(world, center)
    }

    /// Runs until the world around the camera is fully loaded and meshed, returning every mesh
    /// change on the way. `step_server` is called between rounds, and is where a program that ticks
    /// its own server steps it. A replay is not advanced: it stays at the tick it is at.
    pub fn settle(
        &mut self,
        mut step_server: impl FnMut(),
    ) -> Result<Vec<MeshUpdate>, SessionError> {
        let deadline = Instant::now() + Duration::from_secs(120);
        let mut all_updates = Vec::new();
        let mut quiet_rounds = 0;
        loop {
            step_server();
            if self.mode == Mode::Playing {
                self.advance()?;
            }
            let updates = self.update_view();
            let loaded =
                self.mode == Mode::Replaying || self.client.world().watched_region().is_some();
            if updates.is_empty() && loaded && self.mesher.pending_jobs() == 0 {
                quiet_rounds += 1;
            } else {
                quiet_rounds = 0;
            }
            all_updates.extend(updates);
            if quiet_rounds >= 5 {
                return Ok(all_updates);
            }
            if Instant::now() > deadline {
                return Err(SessionError::LoadingTimedOut);
            }
            std::thread::sleep(Duration::from_micros(200));
        }
    }

    /// Tells the server where the player is, if that changed since the last time.
    pub fn send_pose(&mut self) {
        if self.mode != Mode::Playing {
            return;
        }
        let pose = PlayerPose {
            position: self.camera.position,
            yaw: self.camera.yaw,
            pitch: self.camera.pitch,
        };
        if self.last_pose_sent != Some(pose) {
            let _ = self
                .client
                .send_game_message(pose_kind(), encode_pose(&pose));
            self.last_pose_sent = Some(pose);
        }
    }

    /// The solid voxel under the crosshair, within the game's reach.
    pub fn target(&self) -> Option<RayHit> {
        let forward = self.camera.forward().to_array();
        cast_ray(self.camera.position, forward, G::reach(), |voxel| {
            self.mesher.is_opaque_at(voxel)
        })
    }

    /// Sends the server a message for the game. A replay asks for nothing, so it sends nothing.
    pub fn send_game_message(&mut self, kind: NamespacedId, payload: Vec<u8>) {
        if self.mode != Mode::Playing {
            return;
        }
        // A closed connection shows up in the next `advance`, which reports it.
        let _ = self.client.send_game_message(kind, payload);
    }

    /// Leaves the server politely.
    pub fn say_goodbye(&mut self) {
        let _ = self.client.say_goodbye();
    }
}
