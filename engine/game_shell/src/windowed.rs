//! The interactive game: a window, a fly camera, and whatever the game's inputs do; or the same
//! window playing a recording back in real time.

use std::collections::{BTreeMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use client::ClientError;
use glam::DVec3;
use namespaced_id::NamespacedId;
use protocol::ClientId;
use renderer::{Avatar, GraphicsContext, RenderStatistics, VoxelRenderer, describe, new_instance};
use transport::Connection;
use voxel_space::GridId;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::client_commands::{PaintRequest, is_client_command};
use crate::client_plugin::MouseButton as GameButton;
use crate::command_entry::{CommandEntry, opens_a_command};
use crate::definition::GameDefinition;
use crate::files::{open_recording, open_world, record_connection, recorded_game, save_world};
use crate::following::{GridFollower, PoseFollower, TickClock, TurnFollower, turned_toward};
use crate::host::{Autosave, HostSettings, ServerThread, connect_to, listen_on};
use crate::options::{PlayOptions, ReplayOptions, ViewOptions};
use crate::paint_bridge::{Buttons, Painter};
use crate::pointer_hold::PointerHold;
use crate::reload::{DiskSource, DiskSources, Reloadable, Reloads, Watcher};
use crate::screen::{Screen, ScreenKey, ScreenText, Source};
use crate::session::{Mode, Session};
use crate::view::{
    apply_occupancy, apply_shader_pack, apply_updates, camera_at, configure_renderer,
    hexadecimal_voxel_coordinates, light_the_scene, prepare_shader_packs, show_entity_boxes,
    show_screen, start_with_engine_shaders, start_with_texture_pack, swap_engine_shaders,
};

const MOVEMENT_SPEED: f64 = 24.0; // voxels per second
const SPRINT_MULTIPLIER: f64 = 5.0;
const MOUSE_SENSITIVITY: f64 = 0.0025; // radians per pixel
const POSE_INTERVAL: Duration = Duration::from_millis(50);

/// How often what the keys ask is sent again when it has not changed.
const MOVEMENT_REPEAT: Duration = Duration::from_millis(100);
/// How long the answer to a command stays in the window title.
const REPLY_SHOWN_FOR: Duration = Duration::from_secs(8);
type GameSession<G> = Session<Box<dyn Connection>, G>;

/// Plays: starts a server in this process (open to the network if asked) and joins it, or joins a
/// server elsewhere, and runs the window until it closes. The world is saved on exit if a file was
/// named.
pub fn play<G: GameDefinition>(options: PlayOptions) -> Result<(), String> {
    let view = options.view.clone();
    let identity = crate::player_identity::load(options.identity.as_deref())?;
    let (connection, host): (Box<dyn Connection>, Option<ServerThread<G>>) = match &options.join {
        Some(address) => (Box::new(connect_to(address)?), None),
        None => {
            let mut content = crate::packs::content_with_packs::<G>(options.packs.as_deref())?;
            let world = open_world(&mut content, options.world_file.as_deref(), options.seed)?;
            let mut settings = HostSettings::local(view.view_radius);
            if let Some(address) = &options.host {
                settings.network = Some(Box::new(listen_on(address)?));
                settings.announce_players = true;
            }
            settings.world_file = options.world_file.clone();
            settings.packs = options.packs.clone();
            settings.watch_packs = view.watch;
            if let (Some(seconds), Some(_)) = (options.autosave_seconds, &options.world_file) {
                settings.autosave = Some(Autosave {
                    interval: Duration::from_secs_f32(seconds),
                });
            }
            let host = ServerThread::<G>::start(content, world, settings)
                .map_err(|error| error.to_string())?;
            let connection = host.connect().map_err(|error| error.to_string())?;
            (Box::new(connection), Some(host))
        }
    };
    let connection: Box<dyn Connection> = match &options.record {
        Some(path) => record_connection(connection, path)?,
        None => connection,
    };
    let start = camera_at(options.camera, options.yaw_degrees, options.pitch_degrees);
    // The packs of a server joined over the network are kept in the folder for that server; a server of this process sends its own and nothing is kept.
    let packs: Box<dyn client::PackStore> = match &options.join {
        Some(address) => crate::remote_world::pack_store_for_joining(&G::id(), address),
        None => Box::new(client::NoPackStore),
    };
    let mut session = Session::<_, G>::connect_with_packs(
        connection,
        &identity,
        Mode::Playing,
        view.view_radius,
        start,
        || {
            std::thread::sleep(Duration::from_millis(1));
        },
        packs,
    )
    .map_err(|error| error.to_string())?;
    session.enable_level_detail();
    session.enable_occupancy();
    session.pin_time(view.time);
    session.set_shadows(view.shadows);
    session.set_pixelated_shadows(view.pixelated_shadows);
    if let Some(detail) = G::grid_detail() {
        session.enable_grid_detail(detail);
    }

    let (_session, failure) = run_window::<G>(session, &view)?;
    if let Some(host) = host {
        let (mut world, content) = host.stop().map_err(|crash| match &options.world_file {
            Some(_) => {
                format!("{crash}; the world file was not written, so the last save is the world")
            }
            None => format!("{crash}; there was no world file, so the world is lost"),
        })?;
        if let Some(path) = &options.world_file {
            save_world(&content, &mut world, path)?;
            println!("saved the world to {}", path.display());
        }
    }
    failure.map_or(Ok(()), Err)
}

/// Watches a recording in real time from the recorded player's point of view.
pub fn replay<G: GameDefinition>(options: ReplayOptions) -> Result<(), String> {
    let game = recorded_game(&options.file)?;
    if game != G::id() {
        return Err(format!(
            "this recording was made by {game}, not {}",
            G::id()
        ));
    }
    let view = options.view;
    let connection: Box<dyn Connection> = Box::new(open_recording(&options.file)?);
    let start = camera_at([0.0, 0.0, 0.0], 0.0, 0.0);
    let mut session = Session::<_, G>::connect(
        connection,
        &crate::player_identity::for_replay(),
        Mode::Replaying,
        view.view_radius,
        start,
        || {},
    )
    .map_err(|error| error.to_string())?;
    session.enable_level_detail();
    session.enable_occupancy();
    session.pin_time(view.time);
    session.set_shadows(view.shadows);
    session.set_pixelated_shadows(view.pixelated_shadows);
    if let Some(detail) = G::grid_detail() {
        session.enable_grid_detail(detail);
    }
    let (_, failure) = run_window::<G>(session, &view)?;
    failure.map_or(Ok(()), Err)
}

/// Runs the window. Returns the session, and what went wrong during play if something did.
fn run_window<G: GameDefinition>(
    session: GameSession<G>,
    view: &ViewOptions,
) -> Result<(GameSession<G>, Option<String>), String> {
    let event_loop = EventLoop::new().map_err(|error| error.to_string())?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::<G> {
        view: view.clone(),
        session: Some(session),
        viewer: None,
        failure: None,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|error| error.to_string())?;
    let mut session = app
        .viewer
        .map(|viewer| viewer.session)
        .or(app.session)
        .ok_or("the window closed before the game started")?;
    session.say_goodbye();
    Ok((session, app.failure))
}

struct App<G: GameDefinition> {
    view: ViewOptions,
    /// Held until the window exists.
    session: Option<GameSession<G>>,
    viewer: Option<Viewer<G>>,
    failure: Option<String>,
}

struct Viewer<G: GameDefinition> {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    surface_configuration: wgpu::SurfaceConfiguration,
    context: GraphicsContext,
    renderer: VoxelRenderer,
    /// The shader packs the player has, which `.shader` chooses among (see `crate::shader_packs`), and the files that keep what they chose of the settings.
    shader_packs: crate::view::ShaderPacks,
    /// The places the renderer draws with a picture of `--texture-pack` and not the world's.
    pictures_in_use: crate::texture_pack::PicturesInUse,
    /// The folder of loose pictures (`--texture-pack`), written again over the pictures if the server replaces the package.
    texture_pack: Option<std::path::PathBuf>,
    /// What can be reloaded while the game runs (`.reload`, `--watch`): taken out while it reloads, since a reload changes the viewer it belongs to.
    reloads: Option<Reloads<Viewer<G>>>,
    /// Paint mode (`.paint`): a tool that paints on the pictures of the blocks, at the crosshair while the mouse is captured and at the pointer while Left Alt holds it free,
    /// with the buttons of the mouse that are down.
    painter: Option<Painter>,
    paint_buttons: Buttons,
    /// What is owed to the capture of the mouse: Left Alt gave it back, or the window was left with it captured.
    pointer_hold: PointerHold,
    session: GameSession<G>,
    keys: HashSet<KeyCode>,
    mouse_captured: bool,
    /// Where the pointer is, in pixels of the window, for clicks on the screen's buttons.
    cursor: (f64, f64),
    /// Whether a shift key is down (for Tab backwards).
    shift: bool,
    /// Which interface has the player's input, if one does; and which of its buttons has the keyboard focus.
    modal_source: Option<Source>,
    focus: Option<u32>,
    /// The command being typed, if one is.
    entry: CommandEntry,
    /// The latest answer to a command, and when it came.
    reply: Option<(String, Instant)>,
    started: Instant,
    last_frame: Instant,
    last_pose_sent: Instant,
    /// What the keys last asked of the player's body, in a game where the server moves it, and when it
    /// was last sent.
    last_movement: Option<(NamespacedId, Vec<u8>)>,
    last_movement_sent: Instant,
    /// Shows the camera smoothly where the server says the player is.
    follower: PoseFollower,
    /// Shows the room the camera's angles are measured in smoothly, the way the server says it is turned.
    room_follower: TurnFollower,
    /// Shows each ship (and planet) smoothly where the server says it is, as the camera is, so that the two keep their distance: a camera carried on between ticks
    /// that looks at a ship shown where it was last told shakes by a tick of the ship's travel.
    grid_followers: BTreeMap<GridId, GridFollower>,
    /// The same for the other players.
    player_followers: BTreeMap<ClientId, PoseFollower>,
    /// When the ticks of the server come, which the followers are told so that they work out speeds from the beat of the ticks and not from which frame saw them.
    tick_clock: TickClock,
    frames_since_title_update: u32,
    last_title_update: Instant,
    frames_total: u64,
    last_statistics: RenderStatistics,
    exit_after: Option<Duration>,
    ticks_played: u64,
    recording_ended: bool,
}

impl<G: GameDefinition> Viewer<G> {
    fn new(
        event_loop: &ActiveEventLoop,
        view: &ViewOptions,
        mut session: GameSession<G>,
    ) -> Result<Self, String> {
        let attributes = Window::default_attributes()
            .with_title(G::title())
            .with_inner_size(PhysicalSize::new(view.width, view.height));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| error.to_string())?,
        );
        let instance = new_instance();
        let surface = instance
            .create_surface(Arc::clone(&window))
            .map_err(|error| error.to_string())?;
        let context = GraphicsContext::new(instance, view.adapter, Some(&surface))
            .map_err(|error| error.to_string())?;
        println!("graphics adapter: {}", describe(&context.adapter));

        let capabilities = surface.get_capabilities(&context.adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .ok_or("the display offers no sRGB surface format")?;
        let size = window.inner_size();
        session.set_viewport_height(size.height);
        let surface_configuration = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: if view.vsync {
                wgpu::PresentMode::AutoVsync
            } else {
                wgpu::PresentMode::AutoNoVsync
            },
            desired_maximum_frame_latency: 2,
            alpha_mode: capabilities.alpha_modes[0],
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: vec![],
        };
        surface.configure(&context.device, &surface_configuration);

        let mut renderer = VoxelRenderer::new(&context, format);
        configure_renderer(
            &mut renderer,
            session.block_table(),
            session.package(),
            session.fog_radius_bricks(),
        );
        let shader_packs = prepare_shader_packs(view, &mut session)?;
        if let Err(error) = start_with_engine_shaders(&mut renderer, view) {
            eprintln!("warning: the program's own engine shaders are used: {error}");
        }
        let pictures_in_use = match start_with_texture_pack(&mut renderer, session.package(), view)
        {
            Ok(in_use) => in_use,
            Err(error) => {
                eprintln!("warning: the world's own pictures are used: {error}");
                Default::default()
            }
        };
        let reloads = what_can_be_reloaded::<G>(view);
        renderer.set_crosshair(session.mode() == Mode::Playing && G::shows_crosshair());
        let now = Instant::now();
        let ticks_per_second = session.client().world().server_info().ticks_per_second;
        Ok(Self {
            window,
            surface,
            surface_configuration,
            context,
            renderer,
            shader_packs,
            pictures_in_use,
            texture_pack: view.texture_pack.clone(),
            reloads: Some(reloads),
            painter: None,
            paint_buttons: Buttons::default(),
            pointer_hold: PointerHold::default(),
            session,
            keys: HashSet::new(),
            mouse_captured: false,
            cursor: (0.0, 0.0),
            shift: false,
            modal_source: None,
            focus: None,
            entry: CommandEntry::default(),
            reply: None,
            started: now,
            last_frame: now,
            last_pose_sent: now,
            last_movement: None,
            last_movement_sent: now,
            follower: PoseFollower::new(),
            room_follower: TurnFollower::new(),
            grid_followers: BTreeMap::new(),
            player_followers: BTreeMap::new(),
            tick_clock: TickClock::new(ticks_per_second, now),
            frames_since_title_update: 0,
            last_title_update: now,
            frames_total: 0,
            last_statistics: RenderStatistics::default(),
            exit_after: view.exit_after_seconds.map(Duration::from_secs_f32),
            ticks_played: 0,
            recording_ended: false,
        })
    }

    /// Does what `.reload` asked, and, with `--watch`, what the files that changed ask: the replies are told in the window and on the output.
    fn run_reloads(&mut self, now: Instant) {
        let request = self.session.take_reload_request();
        let Some(mut reloads) = self.reloads.take() else {
            return;
        };
        let mut lines = Vec::new();
        if let Some(kind) = request {
            match reloads.reload(kind.as_deref(), self) {
                Ok(reports) => lines.extend(reports.iter().map(|report| report.line())),
                Err(error) => lines.push(error),
            }
        }
        let elapsed = now.duration_since(self.started);
        lines.extend(
            reloads
                .poll(elapsed, self)
                .iter()
                .map(|report| report.line()),
        );
        self.reloads = Some(reloads);
        for line in lines {
            println!("{line}");
            self.reply = Some((line, now));
        }
    }

    /// Runs what was typed on the command line: a line that starts with a period is the window's own
    /// command and goes no further; any other is the server's.
    fn run_command(&mut self, text: &str, now: Instant) {
        if is_client_command(text) {
            let reply = self.session.client_command(text);
            println!("{reply}");
            self.reply = Some((reply, now));
        } else {
            self.session.send_command(text);
        }
    }

    /// What is on the screen besides the world: see [`crate::screen`].
    fn screen(&mut self) -> Screen {
        let mut status = Vec::new();
        match self.session.mode() {
            Mode::Playing => {
                status.push(format!(
                    "players {} | {}",
                    self.session.players().len().max(1),
                    hexadecimal_voxel_coordinates(self.session.camera.position)
                ));
                status.extend(self.session.hud_lines());
            }
            Mode::Replaying => status.push(format!(
                "replay {}{}",
                self.session.client().world().tick(),
                if self.recording_ended { " (ended)" } else { "" }
            )),
        }
        let reply = self
            .reply
            .as_ref()
            .filter(|(_, when)| when.elapsed() < REPLY_SHOWN_FOR)
            .map(|(text, _)| text.clone());
        let text = ScreenText {
            status,
            typing: self.entry.display(),
            reply,
        };
        self.session.screen(&text)
    }

    /// The window's size in pixels.
    fn size(&self) -> (u32, u32) {
        (
            self.surface_configuration.width,
            self.surface_configuration.height,
        )
    }

    /// Notices that an interface that takes the player's input has appeared or gone, or been replaced by another: the mouse is let
    /// go, the keys held are forgotten, a half typed command is dropped, and no button has the focus.
    fn follow_the_modal(&mut self, screen: &Screen) {
        let source = screen.modal().map(|index| screen.sources[index]);
        if source == self.modal_source {
            return;
        }
        if source.is_some() && self.session.mode() == Mode::Playing {
            self.keys.clear();
            self.entry.cancel();
            self.set_mouse_captured(false);
        }
        self.focus = None;
        self.modal_source = source;
    }

    /// Whether an interface has the player's input in a game being played (a recording takes none).
    fn modal_has_the_input(&self) -> bool {
        self.modal_source.is_some() && self.session.mode() == Mode::Playing
    }

    /// What the keyboard does to the interface that has the input.
    fn press_on_the_screen(&mut self, key: ScreenKey) {
        let screen = self.screen();
        if let Some(action) = screen.key(&mut self.focus, key) {
            self.session.apply_screen_action(action);
        }
    }

    /// What a click on the left button does to the interface that has the input.
    fn click_on_the_screen(&mut self) {
        let screen = self.screen();
        let size = self.size();
        let laid = ui::layout(
            &screen.uis,
            (size.0 as i32, size.1 as i32),
            ui::scale_for_height(size.1),
            None,
        );
        if let Some(action) = screen.click(&laid, self.cursor.0 as i32, self.cursor.1 as i32) {
            self.session.apply_screen_action(action);
        }
    }

    /// Captures the mouse again when the player is back in the window and Left Alt is up (see `PointerHold`), unless a form that takes the pointer is open.
    fn follow_the_pointer_hold(&mut self) {
        if self.pointer_hold.recapture_due(self.mouse_captured) && !self.modal_has_the_input() {
            self.set_mouse_captured(true);
        }
    }

    fn set_mouse_captured(&mut self, captured: bool) {
        let grabbed = if captured {
            self.window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined))
                .is_ok()
        } else {
            self.window.set_cursor_grab(CursorGrabMode::None).is_ok()
        };
        self.mouse_captured = captured && grabbed;
        self.window.set_cursor_visible(!self.mouse_captured);
    }

    fn move_camera(&mut self, seconds: f64) {
        let held = |code: KeyCode| self.keys.contains(&code);
        let mut movement = DVec3::ZERO;
        if held(KeyCode::KeyW) {
            movement.z -= 1.0;
        }
        if held(KeyCode::KeyS) {
            movement.z += 1.0;
        }
        if held(KeyCode::KeyD) {
            movement.x += 1.0;
        }
        if held(KeyCode::KeyA) {
            movement.x -= 1.0;
        }
        let mut vertical = 0.0;
        if held(KeyCode::Space) {
            vertical += 1.0;
        }
        if held(KeyCode::ShiftLeft) || held(KeyCode::ShiftRight) {
            vertical -= 1.0;
        }
        let speed = MOVEMENT_SPEED
            * if held(KeyCode::ControlLeft) || held(KeyCode::ControlRight) {
                SPRINT_MULTIPLIER
            } else {
                1.0
            };
        let camera = &mut self.session.camera;
        camera.move_relative(movement.normalize_or_zero() * speed * seconds);
        camera.move_in_world(DVec3::Y * vertical * speed * seconds);
    }

    /// Tells a server that moves the player what the keys ask: when that changes, and now and then so
    /// that the server knows the player is still there.
    fn send_movement(&mut self, now: Instant) {
        let Some(message) = G::movement_message(&self.keys, &self.session.camera) else {
            return;
        };
        let changed = self.last_movement.as_ref() != Some(&message);
        if changed || now.duration_since(self.last_movement_sent) >= MOVEMENT_REPEAT {
            self.session
                .send_game_message(message.0.clone(), message.1.clone());
            self.last_movement = Some(message);
            self.last_movement_sent = now;
        }
    }

    /// Takes in what the server has sent. Returns an error to stop the game with.
    fn follow_server(&mut self, now: Instant, seconds: f64) -> Result<(), ClientError> {
        match self.session.mode() {
            Mode::Playing if G::server_moves_players() => {
                self.send_movement(now);
                self.session.advance()?;
                let seen = self.tick_seen(now);
                // The server says where the player is a few times a second; the picture is carried on
                // between those.
                if let Some(pose) = self.session.own_server_pose() {
                    self.session.camera.position =
                        self.follower.view_position_seen(pose.position, seen, now);
                    if let Some(told) = self.session.own_server_frame_rotation() {
                        let turned = self.room_follower.view_orientation_seen(told, seen, now);
                        self.session.camera.frame_rotation =
                            glam::Quat::from_array(turned.components_single());
                    }
                    if G::server_sets_view() {
                        let camera = &mut self.session.camera;
                        camera.yaw = turned_toward(camera.yaw, pose.yaw, seconds);
                        camera.pitch = turned_toward(camera.pitch, pose.pitch, seconds);
                    }
                }
            }
            Mode::Playing => {
                self.move_camera(seconds);
                if now.duration_since(self.last_pose_sent) >= POSE_INTERVAL {
                    self.session.send_pose();
                    self.last_pose_sent = now;
                }
                self.session.advance()?;
            }
            Mode::Replaying => {
                let rate = f64::from(self.session.client().world().server_info().ticks_per_second);
                let due = (now.duration_since(self.started).as_secs_f64() * rate) as u64;
                while self.ticks_played < due && !self.recording_ended {
                    match self.session.advance_one_tick() {
                        Ok(_) => self.ticks_played += 1,
                        Err(ClientError::ConnectionClosed) => {
                            self.recording_ended = true;
                            println!(
                                "the recording ended at {}",
                                self.session.client().world().tick()
                            );
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
        }
        Ok(())
    }

    /// Does what `.paint` asked, and in paint mode tells the tool where the pointer is and writes what the tool sends: the replies are told in the window and on the output.
    fn run_paint(&mut self, now: Instant) {
        let mut lines = Vec::new();
        if let Some(request) = self.session.take_paint_request() {
            let start = match request {
                PaintRequest::Start(_) => true,
                PaintRequest::Toggle(_) => self.painter.is_none(),
                PaintRequest::Stop => false,
            };
            if !start {
                lines.push(if self.painter.is_some() {
                    self.stop_painting();
                    "paint mode is off".to_owned()
                } else {
                    "paint mode was not on".to_owned()
                });
            } else {
                let (PaintRequest::Start(port) | PaintRequest::Toggle(port)) = request else {
                    unreachable!("only a request to start gets here");
                };
                let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
                match Painter::connect(address, G::title(), self.session.package()) {
                    Ok(painter) => {
                        self.painter = Some(painter);
                        self.paint_buttons = Buttons::default();
                        self.set_mouse_captured(true);
                        lines.push(format!("paint mode: talking to the tool at {address}; left and right buttons paint at the crosshair, Left Alt held gives you the pointer to paint with instead, .paint off ends it"));
                    }
                    Err(error) => lines.push(format!(
                        "paint mode: no tool at {address} ({error}); start VirtualAtlas with --bridge first"
                    )),
                }
            }
        }
        if self.painter.is_some() {
            let (width, height) = self.size();
            let cursor = self.cursor;
            // The brush is at the crosshair, in the middle of the picture, while the mouse turns the camera, and where the pointer is while it does not.
            let (x, y) = if self.mouse_captured {
                (0.0, 0.0)
            } else {
                (
                    (cursor.0 / f64::from(width) * 2.0 - 1.0) as f32,
                    (1.0 - cursor.1 / f64::from(height) * 2.0) as f32,
                )
            };
            self.renderer.set_crosshair(
                self.mouse_captured && self.session.mode() == Mode::Playing && G::shows_crosshair(),
            );
            let target = (x.abs() <= 1.0 && y.abs() <= 1.0)
                .then(|| {
                    let direction =
                        self.session
                            .camera
                            .direction_through(x, y, width as f32 / height as f32);
                    self.session
                        .target_surface_toward(direction.to_array().map(f64::from))
                })
                .flatten();
            if let Some(painter) = self.painter.as_mut() {
                lines.extend(painter.step(
                    self.session.package(),
                    target.as_ref(),
                    self.paint_buttons,
                    &mut self.renderer,
                ));
                if !painter.is_connected() {
                    self.stop_painting();
                    lines.push("paint mode is off".to_owned());
                }
            }
        }
        for line in lines {
            println!("{line}");
            self.reply = Some((line, now));
        }
    }

    fn stop_painting(&mut self) {
        self.painter = None;
        self.paint_buttons = Buttons::default();
        self.renderer
            .set_crosshair(self.session.mode() == Mode::Playing && G::shows_crosshair());
    }

    /// A button of the mouse went down or up in paint mode.
    fn paint_button(&mut self, button: MouseButton, down: bool) {
        match button {
            MouseButton::Left => self.paint_buttons.left = down,
            MouseButton::Right => self.paint_buttons.right = down,
            _ => {}
        }
    }

    /// The server replaced the package: the palette and the pictures of the renderer are made from the new one, and the pictures of `--texture-pack` are written over them
    /// again (the textures were made again, and what was written over the old ones is gone with them).
    fn package_was_replaced(&mut self, now: Instant) {
        configure_renderer(
            &mut self.renderer,
            self.session.block_table(),
            self.session.package(),
            self.session.fog_radius_bricks(),
        );
        self.pictures_in_use = Default::default();
        let mut told = vec!["the server replaced the package".to_owned()];
        if let Some(folder) = &self.texture_pack {
            match crate::texture_pack::use_pictures_from(
                &mut self.renderer,
                self.session.package(),
                &mut self.pictures_in_use,
                folder,
            ) {
                Ok(Some(detail)) => told.push(detail),
                Ok(None) => {}
                Err(error) => told.push(format!(
                    "the pictures of the folder were not written again: {error}"
                )),
            }
        }
        // What the painting tool changed was lost with the textures too.
        if let Some(painter) = self.painter.as_mut() {
            told.extend(painter.package_replaced(self.session.package(), &mut self.renderer));
        }
        let line = told.join("; ");
        println!("{line}");
        self.reply = Some((line, now));
    }

    /// When the newest tick the picture has seen is taken to have come (see [`TickClock`]).
    fn tick_seen(&mut self, now: Instant) -> Instant {
        let tick = self.session.client().world().world_clock();
        self.tick_clock.seen(tick, now)
    }

    /// Puts each ship where the server says it is and carries it on between ticks as the camera is (see `grid_followers`), over where the updates of the view put
    /// it, which is where it was last told.
    fn carry_ships_on(&mut self, now: Instant) {
        let seen = self.tick_seen(now);
        let world = self.session.client().world();
        let grids = world.grids();
        self.grid_followers.retain(|id, _| grids.get(*id).is_some());
        for (id, ship) in grids.iter() {
            let pose = self
                .grid_followers
                .entry(id)
                .or_default()
                .view_pose_seen(ship.pose, seen, now);
            self.renderer.set_grid_pose(id, pose);
        }
    }

    /// Draws the other players where the server says they are, carried on between ticks as the camera is.
    fn show_other_players_carried_on(&mut self, now: Instant) {
        let seen = self.tick_seen(now);
        let mut avatars = Vec::new();
        let mut present = Vec::new();
        if G::shows_other_players() {
            for (client, pose) in self.session.other_players() {
                let position = self
                    .player_followers
                    .entry(client)
                    .or_default()
                    .view_position_seen(pose.position, seen, now);
                present.push(client);
                avatars.push(Avatar {
                    position,
                    color: client.get(),
                });
            }
        }
        self.player_followers
            .retain(|client, _| present.contains(client));
        self.renderer.set_avatars(&avatars);
    }

    /// Draws one frame. Returns false when the game should close.
    fn frame(&mut self) -> Result<bool, String> {
        let now = Instant::now();
        let seconds = now.duration_since(self.last_frame).as_secs_f64().min(0.1);
        self.last_frame = now;
        if self
            .exit_after
            .is_some_and(|limit| now.duration_since(self.started) >= limit)
        {
            return Ok(false);
        }

        self.follow_the_pointer_hold();
        self.follow_server(now, seconds)
            .map_err(|error| error.to_string())?;
        if self.session.take_package_replaced() {
            self.package_was_replaced(now);
        }
        if let Some(report) = self.session.take_pack_check_report() {
            eprintln!("warning: {report}");
        }
        for text in self.session.take_command_replies() {
            println!("{text}");
            self.reply = Some((text, now));
        }
        let updates = self.session.update_view();
        apply_updates(&mut self.renderer, updates);
        apply_occupancy(&mut self.renderer, &mut self.session);
        self.carry_ships_on(now);
        // The light of the picture: the terrain of the level the player is in may be drawn farther than the last one (the level of space is drawn out past the universe), so
        // the fog moves; the sun moves with the clock of the world, and the sky is a colour for where the camera is.
        light_the_scene(&mut self.renderer, &self.session);
        self.run_reloads(now);
        self.run_paint(now);
        // The shader the player chose (`.shader`): at once if it is the one drawn with. A pack the graphics card refuses is taken off, and the player is told.
        if let Err(error) = apply_shader_pack(
            &mut self.renderer,
            &self.shader_packs.packs,
            self.session.settings(),
        ) {
            self.session.reject_shader_pack();
            eprintln!("{error}");
            self.reply = Some((error, now));
        }
        // What the player chose of the settings, kept in the files next to the packs (only what changed is written; a failure is said once).
        for warning in self
            .shader_packs
            .files
            .save_changes(&self.session.settings().pack_settings)
        {
            eprintln!("warning: shader settings: {warning}");
            self.reply = Some((warning, now));
        }
        self.show_other_players_carried_on(now);
        show_entity_boxes(&mut self.renderer, &self.session);

        let screen = self.screen();
        self.follow_the_modal(&screen);
        let size = self.size();
        show_screen(&mut self.renderer, &screen, size, self.focus);

        let (frame, reconfigure) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => (frame, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface
                    .configure(&self.context.device, &self.surface_configuration);
                return Ok(true);
            }
            _ => return Ok(true), // timed out, occluded or a validation error: skip this frame
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let size = (
            self.surface_configuration.width,
            self.surface_configuration.height,
        );
        self.last_statistics = self.renderer.render(&view, size, &self.session.camera);
        self.window.pre_present_notify();
        self.context.queue.present(frame);
        if reconfigure {
            self.surface
                .configure(&self.context.device, &self.surface_configuration);
        }

        self.frames_total += 1;
        self.frames_since_title_update += 1;
        let since_title = now.duration_since(self.last_title_update);
        if since_title >= Duration::from_millis(500) {
            self.update_title(since_title);
            self.last_title_update = now;
            self.frames_since_title_update = 0;
        }
        Ok(true)
    }

    fn update_title(&mut self, since_last: Duration) {
        let fps = f64::from(self.frames_since_title_update) / since_last.as_secs_f64();
        let statistics = self.last_statistics;
        // What the player is doing is on the screen; the title has the game and how the drawing is going.
        self.window.set_title(&format!(
            "{} | {fps:.0} fps | drawn {}/{} bricks, {} quads | loading {} | {}",
            G::title(),
            statistics.bricks_drawn,
            statistics.bricks_drawn + statistics.bricks_culled,
            statistics.quads_drawn,
            self.session.mesher().pending_jobs(),
            describe(&self.context.adapter),
        ));
    }
}

/// What a key means to an interface that has the input, if anything.
fn screen_key(code: KeyCode, shift: bool) -> Option<ScreenKey> {
    Some(match code {
        KeyCode::Tab if shift => ScreenKey::Previous,
        KeyCode::Tab | KeyCode::ArrowDown | KeyCode::ArrowRight => ScreenKey::Next,
        KeyCode::ArrowUp | KeyCode::ArrowLeft => ScreenKey::Previous,
        KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => ScreenKey::Activate,
        KeyCode::Escape => ScreenKey::Dismiss,
        KeyCode::Digit1 | KeyCode::Numpad1 => ScreenKey::Number(1),
        KeyCode::Digit2 | KeyCode::Numpad2 => ScreenKey::Number(2),
        KeyCode::Digit3 | KeyCode::Numpad3 => ScreenKey::Number(3),
        KeyCode::Digit4 | KeyCode::Numpad4 => ScreenKey::Number(4),
        KeyCode::Digit5 | KeyCode::Numpad5 => ScreenKey::Number(5),
        KeyCode::Digit6 | KeyCode::Numpad6 => ScreenKey::Number(6),
        KeyCode::Digit7 | KeyCode::Numpad7 => ScreenKey::Number(7),
        KeyCode::Digit8 | KeyCode::Numpad8 => ScreenKey::Number(8),
        KeyCode::Digit9 | KeyCode::Numpad9 => ScreenKey::Number(9),
        _ => return None,
    })
}

/// The folders of shader packs, as a reload reads them again (the same as at the start).
struct ShaderPackFolders {
    directory: Option<std::path::PathBuf>,
    asked: Option<std::path::PathBuf>,
}

impl<G: GameDefinition> Reloadable<Viewer<G>> for ShaderPackFolders {
    type Loaded = crate::view::ReadShaderPacks;

    fn load(&self, _viewer: &Viewer<G>) -> Result<Self::Loaded, String> {
        crate::view::read_shader_packs(self.directory.as_deref(), self.asked.as_deref())
    }

    fn swap(&self, viewer: &mut Viewer<G>, loaded: Self::Loaded) -> Result<Option<String>, String> {
        for problem in loaded.problems() {
            eprintln!("warning: {problem}");
        }
        crate::view::swap_shader_packs(
            &mut viewer.renderer,
            &mut viewer.shader_packs,
            viewer.session.settings_mut(),
            loaded,
        )
    }
}

/// The folder the engine's own shaders are read from, as a reload reads them again (the same as at the start).
struct EngineShaderFolder(std::path::PathBuf);

impl<G: GameDefinition> Reloadable<Viewer<G>> for EngineShaderFolder {
    type Loaded = renderer::EngineShaderTexts;

    fn load(&self, _viewer: &Viewer<G>) -> Result<Self::Loaded, String> {
        renderer::EngineShaderTexts::read(&self.0)
    }

    fn swap(&self, viewer: &mut Viewer<G>, loaded: Self::Loaded) -> Result<Option<String>, String> {
        swap_engine_shaders(&mut viewer.renderer, &self.0, loaded)
    }
}

/// The folder of loose pictures, as a reload reads it again (the same as at the start).
struct PictureFolder(std::path::PathBuf);

impl<G: GameDefinition> Reloadable<Viewer<G>> for PictureFolder {
    type Loaded = crate::texture_pack::ReadPictures;

    fn load(&self, viewer: &Viewer<G>) -> Result<Self::Loaded, String> {
        crate::texture_pack::read_pictures(&self.0, viewer.session.package())
    }

    fn swap(&self, viewer: &mut Viewer<G>, loaded: Self::Loaded) -> Result<Option<String>, String> {
        crate::texture_pack::swap_pictures(
            &mut viewer.renderer,
            viewer.session.package(),
            &mut viewer.pictures_in_use,
            &self.0,
            loaded,
        )
    }
}

/// The kinds of content this window can reload (`.reload`), watched for changes if `--watch` was given. A kind whose folders are not there is not offered.
fn what_can_be_reloaded<G: GameDefinition>(view: &ViewOptions) -> Reloads<Viewer<G>> {
    let mut reloads = Reloads::new(view.watch);
    let directory = crate::shader_packs::default_directory();
    let places: Vec<DiskSource> = directory
        .iter()
        .chain(view.shader_packs.iter())
        .filter(|folder| folder.is_dir())
        .map(DiskSource::new)
        .collect();
    if !places.is_empty() {
        let added = reloads.add(
            "shaders",
            DiskSources::new(places),
            Watcher::<DiskSources>::DEFAULT_SETTLE,
            Watcher::<DiskSources>::DEFAULT_INTERVAL,
            ShaderPackFolders {
                directory,
                asked: view.shader_packs.clone(),
            },
        );
        if let Err(error) = added {
            eprintln!("warning: shader packs will not be reloaded: {error}");
        }
    }
    if let Some(folder) = &view.texture_pack {
        let added = reloads.add(
            "pictures",
            DiskSource::new(folder),
            Watcher::<DiskSource>::DEFAULT_SETTLE,
            Watcher::<DiskSource>::DEFAULT_INTERVAL,
            PictureFolder(folder.clone()),
        );
        if let Err(error) = added {
            eprintln!("warning: the pictures will not be reloaded: {error}");
        }
    }
    if let Some(folder) = &view.engine_shaders {
        // (Only the files the shaders are made of: the rest of the folder is Rust.)
        let places = renderer::EngineShaderTexts::FILES
            .iter()
            .map(|file| DiskSource::new(folder.join(file)))
            .collect();
        let added = reloads.add(
            "engine-shaders",
            DiskSources::new(places),
            Watcher::<DiskSources>::DEFAULT_SETTLE,
            Watcher::<DiskSources>::DEFAULT_INTERVAL,
            EngineShaderFolder(folder.clone()),
        );
        if let Err(error) = added {
            eprintln!("warning: the engine's shaders will not be reloaded: {error}");
        }
    }
    reloads
}

impl<G: GameDefinition> ApplicationHandler for App<G> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.viewer.is_some() {
            return;
        }
        let Some(session) = self.session.take() else {
            return;
        };
        match Viewer::new(event_loop, &self.view, session) {
            Ok(viewer) => self.viewer = Some(viewer),
            Err(failure) => {
                self.failure = Some(failure);
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window: WindowId,
        event: WindowEvent,
    ) {
        let Some(viewer) = self.viewer.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                viewer.surface_configuration.width = size.width;
                viewer.surface_configuration.height = size.height;
                viewer
                    .surface
                    .configure(&viewer.context.device, &viewer.surface_configuration);
                viewer.session.set_viewport_height(size.height);
            }
            // A press the system makes up when the window gains focus while a key is down belongs
            // to whatever had the focus before; a release is harmless. (That Left Alt is down is
            // worth knowing, though: the mouse is not captured again until it comes up.)
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: true,
                ..
            } if event.state == ElementState::Pressed => {
                if event.physical_key == PhysicalKey::Code(KeyCode::AltLeft) {
                    viewer.pointer_hold.alt_found_down();
                }
            }
            // Left Alt, held, gives the pointer back: the mouse moves the pointer and not the camera until it comes up (and the next frame captures it again).
            WindowEvent::KeyboardInput { event, .. }
                if event.physical_key == PhysicalKey::Code(KeyCode::AltLeft) =>
            {
                match event.state {
                    ElementState::Pressed if !event.repeat => {
                        if viewer.pointer_hold.alt_pressed(viewer.mouse_captured) {
                            viewer.set_mouse_captured(false);
                        }
                    }
                    ElementState::Released => viewer.pointer_hold.alt_released(),
                    ElementState::Pressed => {}
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    match event.state {
                        // An interface that takes the input takes every key: the world hears none.
                        ElementState::Pressed if viewer.modal_has_the_input() => {
                            if let Some(key) = screen_key(code, viewer.shift) {
                                viewer.press_on_the_screen(key);
                            }
                            if code == KeyCode::Escape {
                                viewer.set_mouse_captured(false);
                            }
                        }
                        ElementState::Pressed if viewer.entry.is_active() => match code {
                            KeyCode::Escape => viewer.entry.cancel(),
                            KeyCode::Enter | KeyCode::NumpadEnter => {
                                if let Some(text) = viewer.entry.submit() {
                                    viewer.run_command(&text, Instant::now());
                                }
                            }
                            KeyCode::Backspace => viewer.entry.backspace(),
                            _ => {
                                if let Some(text) = &event.text {
                                    viewer.entry.insert(text);
                                }
                            }
                        },
                        ElementState::Pressed => {
                            // A recording takes no commands for the server, but the window's own work.
                            let opened_with = opens_a_command(
                                event.text.as_deref(),
                                event.repeat,
                                false,
                                viewer.keys.contains(&code),
                            )
                            .filter(|&first| {
                                viewer.session.mode() == Mode::Playing || first == "."
                            });
                            if let Some(first) = opened_with {
                                // Typing is not moving: let go of every key.
                                viewer.keys.clear();
                                viewer.entry.begin(first);
                                println!("command line open: Enter sends it, Esc drops it");
                            } else if code == KeyCode::Escape {
                                // (Left Alt gives the pointer back; Escape is not for that.)
                                viewer.keys.insert(code);
                            } else {
                                if !viewer.keys.contains(&code) {
                                    viewer.session.key_pressed(code);
                                }
                                viewer.keys.insert(code);
                            }
                        }
                        ElementState::Released => {
                            viewer.keys.remove(&code);
                        }
                    }
                }
            }
            // In paint mode the buttons are the tool's, and nothing else of the game hears them.
            // (A click on a window that is owed the mouse, back with Left Alt up, is the click that brought it back: it captures the mouse and paints nothing.)
            WindowEvent::MouseInput { state, button, .. } if viewer.painter.is_some() => {
                let pressed = state == ElementState::Pressed;
                if pressed && viewer.pointer_hold.recapture_due(viewer.mouse_captured) {
                    viewer.set_mouse_captured(true);
                } else {
                    viewer.paint_button(button, pressed);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => match (button, viewer.mouse_captured) {
                (MouseButton::Left, _) if viewer.modal_has_the_input() => {
                    viewer.click_on_the_screen()
                }
                (MouseButton::Left, false) => viewer.set_mouse_captured(true),
                (MouseButton::Left, true) => viewer.session.mouse_pressed(GameButton::Primary),
                (MouseButton::Right, true) => viewer.session.mouse_pressed(GameButton::Secondary),
                _ => {}
            },
            WindowEvent::CursorMoved { position, .. } => viewer.cursor = (position.x, position.y),
            WindowEvent::ModifiersChanged(modifiers) => {
                viewer.shift = modifiers.state().shift_key()
            }
            WindowEvent::Focused(focused) => {
                // Keys held, and a command half typed, belong to the window that had the focus.
                viewer.keys.clear();
                viewer.entry.cancel();
                if focused {
                    viewer.pointer_hold.focus_gained();
                } else {
                    // The mouse is let go of, and owed back when the player is.
                    viewer.pointer_hold.focus_lost(viewer.mouse_captured);
                    viewer.set_mouse_captured(false);
                    // (A button, or Left Alt, let go of in another window is not heard here.)
                    viewer.paint_buttons = Buttons::default();
                }
            }
            WindowEvent::RedrawRequested => match viewer.frame() {
                Ok(true) => {}
                Ok(false) => {
                    let seconds = viewer.started.elapsed().as_secs_f64();
                    println!(
                        "closed after {seconds:.1} s: {} frames, {:.0} fps average",
                        viewer.frames_total,
                        viewer.frames_total as f64 / seconds
                    );
                    event_loop.exit();
                }
                Err(failure) => {
                    self.failure = Some(failure);
                    event_loop.exit();
                }
            },
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device: DeviceId,
        event: DeviceEvent,
    ) {
        let Some(viewer) = self.viewer.as_mut() else {
            return;
        };
        if let DeviceEvent::MouseMotion { delta } = event
            && viewer.mouse_captured
            && viewer.session.mode() == Mode::Playing
            && !G::server_sets_view()
        {
            viewer
                .session
                .camera
                .look_by(-delta.0 * MOUSE_SENSITIVITY, -delta.1 * MOUSE_SENSITIVITY);
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(viewer) = &self.viewer {
            viewer.window.request_redraw();
        }
    }
}
