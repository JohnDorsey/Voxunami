//! A player's side of a game, without a window: the client, the meshes of what it sees, the camera,
//! and the messages the player's actions turn into. Windows and headless runs both drive one.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use block_models::BlockTable;
use client::{Client, ClientError, ClientEvent, Handshake, HandshakeProgress};
use identity::Identity;
use namespaced_id::NamespacedId;
use player_presence::{
    LevelChange, PlayerClient, PlayerLook, PlayerPose, PlayerState, Players, PoseReport,
    player_kind,
};
use protocol::{ClientId, Message};
use renderer::Camera;
use transport::Connection;
use voxel_space::{BrickBox, BrickCoordinate, LevelIndex, WorldPosition};
use winit::keyboard::KeyCode;
use world_view::{MeshUpdate, RayHit, ViewSettings, WorldMesher, cast_ray};

use crate::client_commands::ClientSettings;
use crate::client_plugin::{
    ClientContext, ClientHost, ClientPlugin, MouseButton, Outgoing, SessionView,
};
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
    block_table: Arc<BlockTable>,
    mesher: WorldMesher,
    pub camera: Camera,
    /// The client halves of the game's plugins, and what they registered.
    client_plugins: Vec<Box<dyn ClientPlugin>>,
    client_host: ClientHost,
    /// What the window's own commands change.
    settings: ClientSettings,
    /// The players the client knows of, by connection: those among the entities it was told of, or, in a
    /// recording made before protocol version 4, those the old `Players` message listed.
    players: BTreeMap<ClientId, PlayerState>,
    /// Whether the stream is one that lists players in `Players` messages instead of as entities.
    players_listed_by_message: bool,
    mode: Mode,
    view_radius_bricks: i64,
    /// The level the player is in, and so the one they ask to watch.
    level: LevelIndex,
    /// Where and in which level the server was last asked to send what the player sees.
    watched_center: Option<(LevelIndex, BrickCoordinate)>,
    /// The level the meshes are of: the one the server last confirmed watching.
    meshed_level: Option<LevelIndex>,
    command_replies: Vec<String>,
    last_pose_sent: Option<PlayerPose>,
    /// What a server that moves the player was last told they are asking of their body.
    last_movement_sent: Option<(NamespacedId, Vec<u8>)>,
}

impl<C: Connection, G: GameDefinition> Session<C, G> {
    /// Connects and builds the player's picture of the world, proving to the server that the player is
    /// `identity`. `wait` is called between attempts to hear the server's welcome, and is where a program
    /// that ticks its own server steps it.
    pub fn connect(
        connection: C,
        identity: &Identity,
        mode: Mode,
        view_radius_bricks: i64,
        start: Camera,
        mut wait: impl FnMut(),
    ) -> Result<Self, SessionError> {
        let mut handshake = Handshake::start(connection, G::id(), identity)?;
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

        let join = G::join_message(start.position);
        let client_plugins = G::client_plugins(&content);
        let client_host = ClientHost::new(&client_plugins).unwrap_or_else(|error| {
            panic!(
                "the client plugins of {} cannot be registered: {error}",
                G::id()
            )
        });
        let settings = ClientSettings::new(content.ambient_light());
        let block_table = Arc::new(content.block_table());
        let worker_count = std::thread::available_parallelism()
            .map_or(2, |count| count.get().saturating_sub(1).max(2));
        let mesher = WorldMesher::new(
            ViewSettings {
                view_radius_bricks,
                worker_count,
            },
            content.view_channels(),
            Arc::clone(&block_table),
        );
        let mut session = Self {
            client,
            content,
            block_table,
            mesher,
            camera: start,
            client_plugins,
            client_host,
            settings,
            players: BTreeMap::new(),
            players_listed_by_message: false,
            mode,
            view_radius_bricks,
            level: LevelIndex::MAIN,
            watched_center: None,
            meshed_level: None,
            command_replies: Vec::new(),
            last_pose_sent: None,
            last_movement_sent: None,
        };
        // A server that moves players is told where this one joins.
        if let Some((kind, payload)) = join {
            session.send_game_message(kind, payload);
        }
        Ok(session)
    }

    /// The level the player is in: while playing, the one they asked for, which the view follows
    /// once the server has confirmed it; while replaying, the one the recording watches.
    pub fn level(&self) -> LevelIndex {
        match self.mode {
            Mode::Playing => self.level,
            Mode::Replaying => self
                .client
                .world()
                .watched()
                .map_or(LevelIndex::MAIN, |(level, _)| level),
        }
    }

    /// Moves the player's view to another level of the world. The picture changes when the server
    /// has sent the new level's bricks.
    pub fn change_level(&mut self, level: LevelIndex) {
        self.level = level;
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

    /// The entities the server has told this session of, in order of id.
    pub fn entities(&self) -> &client::ClientEntities {
        self.client.world().entities()
    }

    pub fn mesher(&self) -> &WorldMesher {
        &self.mesher
    }

    /// What every block and state of this game looks like and how it fills its cell.
    pub fn block_table(&self) -> &BlockTable {
        &self.block_table
    }

    /// Where every player is, and in which level, as far as the server has said.
    pub fn players(&self) -> &BTreeMap<ClientId, PlayerState> {
        &self.players
    }

    /// Every player in the same level as this one but this one, as the server last said where they
    /// are.
    pub fn other_players(&self) -> impl Iterator<Item = (ClientId, &PlayerPose)> {
        let own = self.client.world().server_info().client;
        let level = self.level();
        self.players
            .iter()
            .filter(move |(client, state)| **client != own && state.level == level)
            .map(|(client, state)| (*client, &state.pose))
    }

    /// Applies the next complete tick the server has sent, if any. Returns whether there was one.
    pub fn advance_one_tick(&mut self) -> Result<bool, ClientError> {
        let Some(update) = self.client.poll_tick()? else {
            return Ok(false);
        };
        for event in update.events {
            match event {
                ClientEvent::BrickChanged { brick, .. } => self.mesher.brick_changed(brick),
                ClientEvent::CommandReply { text } => self.command_replies.push(text),
                ClientEvent::GameMessage { kind, payload } if kind == LevelChange::kind() => {
                    // The server moved the player. A recording shows the level by what it watches.
                    if self.mode == Mode::Playing
                        && let Ok(LevelChange(id)) = LevelChange::decode(&payload)
                        && let Some(level) = self.client.world().world().level_index(&id)
                    {
                        self.change_level(level);
                    }
                }
                ClientEvent::GameMessage { kind, payload } if kind == Players::kind() => {
                    // A message this client cannot read is dropped; the next snapshot replaces it.
                    // (Only streams from before protocol version 4 have it; later ones use entities.)
                    self.players_listed_by_message = true;
                    if let Ok(Players(players)) = Players::decode(&payload) {
                        self.players = players.into_iter().collect();
                    }
                }
                ClientEvent::GameMessage { kind, payload } => {
                    // A kind no client plugin handles is another game's, or a message only a server needs.
                    self.client_host.message(&kind, &payload);
                }
                ClientEvent::EntitiesChanged => {
                    if !self.players_listed_by_message {
                        self.players = self.players_among_the_entities();
                    }
                }
                ClientEvent::WatchedRegionChanged { .. } => {}
            }
        }
        self.mesher
            .set_channels(self.client_host.view(self.content.view_channels()));
        if self.mode == Mode::Replaying {
            self.follow_recorded_player();
        } else if G::server_moves_players() {
            self.follow_own_pose();
        }
        Ok(true)
    }

    /// The players among the entities the client was told of: the ones whose entity says whose it is, where
    /// it is and which way it looks.
    fn players_among_the_entities(&self) -> BTreeMap<ClientId, PlayerState> {
        let kind = player_kind();
        self.client
            .world()
            .entities()
            .iter()
            .filter(|entity| entity.kind == kind)
            .filter_map(|entity| {
                let PlayerClient(client) = entity.component().ok()??;
                let PlayerLook { yaw, pitch } = entity.component().ok()??;
                let level = entity.pose.grid.root_level()?;
                let pose = PlayerPose {
                    position: entity.pose.position,
                    yaw,
                    pitch,
                };
                Some((client, PlayerState { pose, level }))
            })
            .collect()
    }

    /// The entities the plugins draw as boxes, as the renderer is told of them: those of the player's level
    /// the server has told of, in order of identifier.
    pub fn entity_boxes(&self) -> Vec<renderer::EntityBox> {
        self.client
            .world()
            .entities()
            .iter()
            .filter_map(|entity| {
                let appearance = self.client_host.appearance_of(entity)?;
                Some(renderer::EntityBox {
                    position: entity.pose.position.translated(appearance.centre_offset),
                    size: appearance.size,
                    material: appearance.material,
                })
            })
            .collect()
    }

    /// Where the server last said this player is, if it has said.
    pub fn own_server_position(&self) -> Option<WorldPosition> {
        let own = self.client.world().server_info().client;
        self.players.get(&own).map(|state| state.pose.position)
    }

    /// Where the server last said this player is and which way they look, if it has said.
    pub fn own_server_pose(&self) -> Option<PlayerPose> {
        let own = self.client.world().server_info().client;
        self.players.get(&own).map(|state| state.pose)
    }

    /// In a game where the server moves players, the camera is where the server says the player is
    /// (the way they face is their own, unless the server sets the view too).
    fn follow_own_pose(&mut self) {
        if let Some(pose) = self.own_server_pose() {
            self.camera.position = pose.position;
            if G::server_sets_view() {
                self.camera.yaw = pose.yaw;
                self.camera.pitch = pose.pitch;
            }
        }
    }

    /// Sends the server a line of text as a command; its answer arrives with a later tick (see
    /// [`Session::take_command_replies`]). A replay asks for nothing, so it sends nothing.
    pub fn send_command(&mut self, text: &str) {
        if self.mode != Mode::Playing {
            return;
        }
        let _ = self.client.send_command(text);
    }

    /// The answers to commands that have arrived since this was last called, oldest first.
    pub fn take_command_replies(&mut self) -> Vec<String> {
        std::mem::take(&mut self.command_replies)
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
        if let Some(state) = self.players.get(&own) {
            self.camera.position = state.pose.position;
            self.camera.yaw = state.pose.yaw;
            self.camera.pitch = state.pose.pitch;
        }
    }

    /// Keeps the watched region around the camera, and the meshes around it up to date. Returns what
    /// the renderer must change.
    pub fn update_view(&mut self) -> Vec<MeshUpdate> {
        let center = self.camera.position.brick();
        if self.mode == Mode::Playing && self.watched_center != Some((self.level, center)) {
            let region = BrickBox::around(
                center,
                (self.view_radius_bricks + WATCH_MARGIN_BRICKS) as u32,
            );
            // A closed connection shows up in the next `advance`, which reports it.
            let _ = self.client.watch(self.level, region);
            self.watched_center = Some((self.level, center));
        }
        let Some((level, data)) = self.client.world().watched_level() else {
            return Vec::new();
        };
        let mut updates = Vec::new();
        if self.meshed_level != Some(level) {
            // Another level's bricks have nothing to do with the ones the meshes are of.
            updates.extend(self.mesher.reset());
            self.meshed_level = Some(level);
        }
        updates.extend(self.mesher.update(data, center));
        updates
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
            let loaded = match self.mode {
                Mode::Replaying => true,
                Mode::Playing => self
                    .client
                    .world()
                    .watched()
                    .is_some_and(|(level, _)| level == self.level),
            };
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
        // A server that moves players is the one that says where they are; the player says which way
        // they face, with no keys held (the window sends what is held itself).
        if G::server_moves_players() {
            if let Some(message) = G::movement_message(&HashSet::new(), &self.camera)
                && self.last_movement_sent.as_ref() != Some(&message)
            {
                let _ = self
                    .client
                    .send_game_message(message.0.clone(), message.1.clone());
                self.last_movement_sent = Some(message);
            }
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
                .send_game_message(PoseReport::kind(), PoseReport(pose).encode());
            self.last_pose_sent = Some(pose);
        }
    }

    /// The solid voxel under the crosshair, within the game's reach.
    pub fn target(&self) -> Option<RayHit> {
        let forward = self.camera.forward().to_array();
        cast_ray(self.camera.position, forward, G::reach(), |voxel| {
            self.mesher.is_solid_at(voxel)
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

    /// The client half of a plugin, if the game has one of this type.
    pub fn client_plugin<P: ClientPlugin>(&self) -> Option<&P> {
        self.client_plugins
            .iter()
            .find_map(|plugin| (&**plugin as &dyn std::any::Any).downcast_ref::<P>())
    }

    /// What the window's own commands change.
    pub fn settings(&self) -> &ClientSettings {
        &self.settings
    }

    /// How many payloads of messages the client plugins handle did not decode, and were ignored.
    pub fn malformed_messages(&self) -> u64 {
        self.client_host.malformed_messages()
    }

    /// Runs `with` on the client host with a context made of the session, then sends what the hooks asked
    /// for. A replay asks for nothing, so it sends nothing.
    fn with_client_context<R>(
        &mut self,
        with: impl FnOnce(&mut ClientHost, &mut ClientContext<'_>) -> R,
    ) -> R {
        let target = self.target();
        let target_content = target.and_then(|hit| {
            Some((
                self.mesher.material_at(hit.voxel)?,
                self.mesher.state_at(hit.voxel)?,
            ))
        });
        let tick = self.client.world().tick();
        let own_client = self.client.world().server_info().client;
        let mut context = ClientContext::new(SessionView {
            camera: &self.camera,
            block_table: &self.block_table,
            players: &self.players,
            entities: self.client.world().entities(),
            settings: &mut self.settings,
            target,
            target_content,
            tick,
            mode: self.mode,
            own_client,
        });
        let result = with(&mut self.client_host, &mut context);
        let outgoing = context.into_outgoing();
        for item in outgoing {
            match item {
                Outgoing::Command(text) => self.send_command(&text),
                Outgoing::Message(kind, payload) => self.send_game_message(kind, payload),
            }
        }
        result
    }

    /// A key other than the movement keys went down: runs the actions on it.
    pub fn key_pressed(&mut self, key: KeyCode) {
        if self.mode == Mode::Playing {
            self.with_client_context(|host, context| host.press_key(key, context));
        }
    }

    /// A mouse button went down while the mouse is captured: runs the actions on it.
    pub fn mouse_pressed(&mut self, button: MouseButton) {
        if self.mode == Mode::Playing {
            self.with_client_context(|host, context| host.press_mouse(button, context));
        }
    }

    /// Runs a line typed with a period (`.ambient 20`) and returns what to tell the player.
    pub fn client_command(&mut self, line: &str) -> String {
        self.with_client_context(|host, context| host.command(line, context))
    }

    /// The HUD lines of the client plugins, joined; shown in the window title.
    pub fn hud_text(&mut self) -> String {
        self.with_client_context(|host, context| host.hud(context))
    }

    /// Leaves the server politely.
    pub fn say_goodbye(&mut self) {
        let _ = self.client.say_goodbye();
    }
}
