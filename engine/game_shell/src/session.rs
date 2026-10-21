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
    LevelChange, PlayerClient, PlayerLook, PlayerPose, PlayerState, PoseReport, player_kind,
};
use plugin_host::{FormId, Prompt, PromptClosed, UiAction, UiClose, UiResponse, UiShow};
use protocol::{ClientId, Message};
use renderer::Camera;
use transport::Connection;
use voxel_space::{BrickBox, BrickCoordinate, LevelIndex, WorldPosition};
use winit::keyboard::KeyCode;
use world_view::{MeshUpdate, RayHit, ViewChannels, ViewSettings, WorldMesher, cast_ray};

use crate::client_commands::ClientSettings;
use crate::client_plugin::{
    ClientContext, ClientHost, ClientPlugin, MouseButton, Outgoing, SessionView,
};
use crate::definition::{GameContent, GameDefinition};
use crate::host::WATCH_MARGIN_BRICKS;
use crate::package_data::{PackageData, PackageError, check_materials};

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error(transparent)]
    Package(#[from] PackageError),
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
    /// What the server said the world looks like, and the table made from it in this client's numbering.
    package: PackageData,
    block_table: Arc<BlockTable>,
    /// The channels the view reads, before the client plugins have their say.
    view_channels: ViewChannels,
    mesher: WorldMesher,
    pub camera: Camera,
    /// The client halves of the game's plugins, and what they registered.
    client_plugins: Vec<Box<dyn ClientPlugin>>,
    client_host: ClientHost,
    /// What the window's own commands change.
    settings: ClientSettings,
    /// The players the client knows of, by connection: those among the entities it was told of.
    players: BTreeMap<ClientId, PlayerState>,
    /// The questions the server has asked and not closed, the oldest first.
    prompts: Vec<Prompt>,
    /// The forms the server has shown and not closed, in the order they were first shown: what the screen has besides the world.
    forms: Vec<(FormId, ui::Ui)>,
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
        let package =
            PackageData::decode(&welcomed.welcome().package).map_err(PackageError::from)?;
        let (client, _report) = welcomed.finish(content.load_context(seed))?;

        let join = G::join_message(start.position);
        let client_plugins = G::client_plugins(&content);
        let client_host = ClientHost::new(&client_plugins).unwrap_or_else(|error| {
            panic!(
                "the client plugins of {} cannot be registered: {error}",
                G::id()
            )
        });
        // What blocks look like comes from the server, not from this program's copy of the game.
        let registries = content.registries();
        let numbering = registries
            .iter()
            .find(|(kind, _)| *kind == package.block_registry)
            .map(|(_, registry)| *registry)
            .ok_or_else(|| PackageError::NoBlockRegistry(package.block_registry.clone()))?;
        let block_table = package
            .block_table_numbered_by(numbering)
            .map_err(PackageError::from)?;
        check_materials(&block_table)?;
        let block_table = Arc::new(block_table);
        let view_channels =
            package.view_channels(client.world().world().main().overrides.layout())?;
        let settings = ClientSettings::new(package.ambient_light);
        let worker_count = std::thread::available_parallelism()
            .map_or(2, |count| count.get().saturating_sub(1).max(2));
        let mesher = WorldMesher::new(
            ViewSettings {
                view_radius_bricks,
                worker_count,
            },
            view_channels.clone(),
            Arc::clone(&block_table),
        );
        let mut session = Self {
            client,
            content,
            package,
            block_table,
            view_channels,
            mesher,
            camera: start,
            client_plugins,
            client_host,
            settings,
            players: BTreeMap::new(),
            prompts: Vec::new(),
            forms: Vec::new(),
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

    /// The forms the server has shown and not closed, in the order they were first shown.
    pub fn forms(&self) -> &[(FormId, ui::Ui)] {
        &self.forms
    }

    /// Tells the server what the player did with a form. A replay sends nothing.
    pub fn respond_to_form(&mut self, id: FormId, action: UiAction) {
        use protocol::Message;
        self.send_game_message(UiResponse::kind(), UiResponse { id, action }.encode());
    }

    /// What the server said the world looks like.
    pub fn package(&self) -> &PackageData {
        &self.package
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
                // The bricks of ships are drawn from Phase 32d on; the mesher is the terrain's.
                ClientEvent::BrickChanged { grid, brick } if grid.root_level().is_some() => {
                    self.mesher.brick_changed(brick);
                }
                ClientEvent::BrickChanged { .. } | ClientEvent::GridsChanged => {}
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
                ClientEvent::GameMessage { kind, payload } if kind == Prompt::kind() => {
                    // A question the server asks. One that cannot be read is dropped (nothing can answer it).
                    if let Ok(prompt) = Prompt::decode(&payload) {
                        self.prompts.push(prompt);
                    }
                }
                ClientEvent::GameMessage { kind, payload } if kind == UiShow::kind() => {
                    // A form the server shows, or a new version of one it showed. One that cannot be read is dropped.
                    if let Ok(UiShow { id, ui }) = UiShow::decode(&payload) {
                        match self.forms.iter_mut().find(|(shown, _)| *shown == id) {
                            Some(form) => form.1 = ui,
                            None => self.forms.push((id, ui)),
                        }
                    }
                }
                ClientEvent::GameMessage { kind, payload } if kind == UiClose::kind() => {
                    if let Ok(UiClose(id)) = UiClose::decode(&payload) {
                        self.forms.retain(|(shown, _)| *shown != id);
                    }
                }
                ClientEvent::GameMessage { kind, payload } if kind == PromptClosed::kind() => {
                    if let Ok(PromptClosed(id)) = PromptClosed::decode(&payload) {
                        self.prompts.retain(|waiting| waiting.id != id);
                    }
                }
                ClientEvent::GameMessage { kind, payload } => {
                    // A kind no client plugin handles is another game's, or a message only a server needs.
                    self.client_host.message(&kind, &payload);
                }
                ClientEvent::EntitiesChanged => {
                    self.players = self.players_among_the_entities();
                }
                ClientEvent::WatchedRegionChanged { .. } => {}
            }
        }
        self.mesher
            .set_channels(self.client_host.view(self.view_channels.clone()));
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
            package: &self.package,
            players: &self.players,
            entities: self.client.world().entities(),
            prompts: &self.prompts,
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

    /// The HUD lines of the client plugins, one each (the question the server is waiting on is not among them: it is on the
    /// screen as a panel).
    pub fn hud_lines(&mut self) -> Vec<String> {
        self.with_client_context(|host, context| host.hud_lines(context))
    }

    /// The questions the server is waiting for the player to answer, the oldest first.
    pub fn questions(&self) -> &[Prompt] {
        &self.prompts
    }

    /// What is on the screen besides the world: see [`crate::screen`]. `text` is what the window knows (where the player is,
    /// what they are typing).
    pub fn screen(&mut self, text: &crate::screen::ScreenText) -> crate::screen::Screen {
        let overlays = self.with_client_context(|host, context| host.overlays(context));
        crate::screen::build(&self.prompts, &self.forms, &overlays, text)
    }

    /// Does what the player's input on the screen asked: answers the question, or tells the server what they did with a
    /// form. A replay sends nothing.
    pub fn apply_screen_action(&mut self, action: crate::screen::ScreenAction) {
        use crate::screen::ScreenAction;
        match action {
            ScreenAction::Answer { question, choice } => {
                self.send_game_message(
                    plugin_host::PromptAnswer::kind(),
                    plugin_host::PromptAnswer {
                        id: question,
                        choice,
                    }
                    .encode(),
                );
            }
            ScreenAction::Form { form, action } => self.respond_to_form(form, action),
        }
    }

    /// The HUD lines of the client plugins, joined; the old place for them was the window title.
    pub fn hud_text(&mut self) -> String {
        self.with_client_context(|host, context| host.hud(context))
    }

    /// Leaves the server politely.
    pub fn say_goodbye(&mut self) {
        let _ = self.client.say_goodbye();
    }
}

/// What a session makes of the package a server sends, with servers whose package is not the client's own.
#[cfg(test)]
mod package_tests {
    use block_models::{BlockDefinition, MISSING_BLOCK_COLOR, PropertySchema};
    use plugin_host::PluginHost;
    use server::{Server, ServerConfig};
    use transport::Connection;
    use transport::loopback::loopback_listener;
    use world_storage::test_support::id;

    use super::*;
    use crate::package_data::PackageError;
    use crate::test_support::{ExampleContent, ExampleGame};
    use crate::view::camera_at;

    fn content() -> ExampleContent {
        ExampleGame::new_content()
    }

    /// A session joined to a server that sends `package`, and the server's own content.
    fn joined_with(
        package: &PackageData,
    ) -> Result<Session<Box<dyn Connection>, ExampleGame>, SessionError> {
        joined_with_bytes(package.encode())
    }

    fn joined_with_bytes(
        package: Vec<u8>,
    ) -> Result<Session<Box<dyn Connection>, ExampleGame>, SessionError> {
        let served = content();
        let world = served.new_world(7);
        let (listener, connector) = loopback_listener();
        let logic = PluginHost::new(Vec::new(), &world).unwrap();
        let mut server = Server::new(
            ServerConfig::new(ExampleGame::id())
                .with_counted_challenges(1)
                .with_package(package),
            world,
            &served.registries(),
            Box::new(logic),
            Box::new(listener),
        )
        .unwrap();
        let connection: Box<dyn Connection> = Box::new(connector.connect().unwrap());
        Session::<_, ExampleGame>::connect(
            connection,
            &Identity::from_secret([1; 32]),
            Mode::Playing,
            3,
            camera_at([16.0, 110.0, 16.0], 0.0, 0.0, 3),
            || {
                server.step();
            },
        )
    }

    /// The package the client's own copy of the game would make.
    fn compiled_in() -> PackageData {
        content().package_data()
    }

    fn raw(session: &Session<Box<dyn Connection>, ExampleGame>, block: &str) -> u32 {
        session.content().game.raw_block(block)
    }

    #[test]
    fn a_client_with_no_changes_of_its_own_draws_what_its_own_copy_of_the_game_says() {
        let session = joined_with(&compiled_in()).unwrap();
        assert_eq!(session.package(), &compiled_in());
        let stone = raw(&session, "base_blocks:stone");
        assert_eq!(session.block_table().color(stone), [0.5; 3]);
    }

    #[test]
    fn what_the_server_says_blocks_look_like_wins_over_the_clients_own_copy() {
        let mut package = compiled_in();
        let stone = package
            .blocks
            .iter_mut()
            .find(|(identifier, _)| *identifier == id("base_blocks:stone"))
            .unwrap();
        stone.1 = Some(BlockDefinition::cube([0.9, 0.1, 0.2]));
        package.ambient_light = 77;
        let session = joined_with(&package).unwrap();
        let stone = raw(&session, "base_blocks:stone");
        let dirt = raw(&session, "base_blocks:dirt");
        assert_eq!(session.block_table().color(stone), [0.9, 0.1, 0.2]);
        assert_eq!(session.block_table().color(dirt), [0.5; 3]);
        assert_eq!(session.package().ambient_light, 77);
        assert_eq!(session.settings().ambient_light, 77);
    }

    #[test]
    fn blocks_are_matched_by_identifier_not_by_the_order_the_server_lists_them() {
        let mut package = compiled_in();
        // Different colours for each, listed in the opposite order.
        for (index, (_, definition)) in package.blocks.iter_mut().enumerate() {
            if index > 0 {
                *definition = Some(BlockDefinition::cube([index as f32 / 10.0; 3]));
            }
        }
        let expected: Vec<(NamespacedId, [f32; 3])> = package
            .blocks
            .iter()
            .skip(1)
            .map(|(identifier, definition)| {
                (identifier.clone(), definition.as_ref().unwrap().color)
            })
            .collect();
        package.blocks.reverse();
        let session = joined_with(&package).unwrap();
        for (identifier, color) in expected {
            let block = session.content().game.raw_block(identifier.as_str());
            assert_eq!(session.block_table().color(block), color, "{identifier}");
        }
    }

    #[test]
    fn a_block_the_server_has_no_definition_for_is_unknown_to_the_client() {
        let mut package = compiled_in();
        for (identifier, definition) in &mut package.blocks {
            if *identifier == id("base_blocks:dirt") {
                *definition = None;
            }
        }
        let session = joined_with(&package).unwrap();
        let dirt = raw(&session, "base_blocks:dirt");
        let stone = raw(&session, "base_blocks:stone");
        assert_eq!(session.block_table().color(dirt), MISSING_BLOCK_COLOR);
        assert_eq!(session.block_table().color(stone), [0.5; 3]);
    }

    #[test]
    fn a_package_that_cannot_be_used_is_an_error_naming_why_and_never_a_panic() {
        let refused = |package: &PackageData| match joined_with(package) {
            Err(SessionError::Package(error)) => error,
            other => panic!("expected the package to be refused, got {other:?}"),
        };

        let mut no_channel = compiled_in();
        no_channel.light_channel = Some(id("core:no_such_light"));
        assert_eq!(
            refused(&no_channel),
            PackageError::NoChannel(id("core:no_such_light"))
        );

        let mut no_registry = compiled_in();
        no_registry.block_registry = id("other:block");
        assert_eq!(
            refused(&no_registry),
            PackageError::NoBlockRegistry(id("other:block"))
        );

        let mut undeclared = compiled_in();
        undeclared.properties =
            PropertySchema::decode(&mut binary_codec::ByteReader::new(&[0])).unwrap();
        undeclared.blocks[1].1 =
            Some(BlockDefinition::cube([0.5; 3]).with_property(&block_models::engine::DENSITY, 5));
        assert!(matches!(refused(&undeclared), PackageError::Blocks(_)));

        // More colours than the renderer can draw would be a panic when a window opens: refuse them here.
        let mut too_many = compiled_in();
        too_many.blocks[1].1 = Some(
            BlockDefinition::cube([0.5; 3])
                .with_state_colors(vec![[0.1; 3]; renderer::MAXIMUM_GAME_MATERIALS]),
        );
        assert_eq!(
            refused(&too_many),
            PackageError::TooManyMaterials {
                materials: renderer::MAXIMUM_GAME_MATERIALS + 4,
                limit: renderer::MAXIMUM_GAME_MATERIALS
            }
        );

        for damaged in [Vec::new(), vec![1, 2, 3], vec![0xFF; 40]] {
            assert!(
                matches!(
                    joined_with_bytes(damaged),
                    Err(SessionError::Package(PackageError::Damaged(_)))
                ),
                "damaged bytes are refused"
            );
        }
    }
}
