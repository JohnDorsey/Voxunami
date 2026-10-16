//! The interactive game: a window, a fly camera, and whatever the game's inputs do; or the same
//! window playing a recording back in real time.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use client::ClientError;
use glam::Vec3;
use namespaced_id::NamespacedId;
use renderer::{GraphicsContext, RenderStatistics, VoxelRenderer, describe, new_instance};
use transport::Connection;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::command_entry::{CommandEntry, opens_a_command};
use crate::definition::GameDefinition;
use crate::files::{open_recording, open_world, record_connection, recorded_game, save_world};
use crate::following::{PoseFollower, turned_toward};
use crate::host::{Autosave, HostSettings, ServerThread, connect_to, listen_on};
use crate::options::{PlayOptions, ReplayOptions, ViewOptions};
use crate::session::{Mode, Session};
use crate::view::{apply_updates, camera_at, configure_renderer, show_other_players};

const MOVEMENT_SPEED: f32 = 24.0; // voxels per second
const SPRINT_MULTIPLIER: f32 = 5.0;
const MOUSE_SENSITIVITY: f32 = 0.0025; // radians per pixel
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
    let (connection, host): (Box<dyn Connection>, Option<ServerThread<G>>) = match &options.join {
        Some(address) => (Box::new(connect_to(address)?), None),
        None => {
            let mut content = G::new_content();
            let world = open_world(&mut content, options.world_file.as_deref(), options.seed)?;
            let mut settings = HostSettings::local(view.view_radius);
            if let Some(address) = &options.host {
                settings.network = Some(Box::new(listen_on(address)?));
                settings.announce_players = true;
            }
            if let (Some(seconds), Some(path)) = (options.autosave_seconds, &options.world_file) {
                settings.autosave = Some(Autosave {
                    interval: Duration::from_secs_f32(seconds),
                    path: path.clone(),
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
    let start = camera_at(
        options.camera,
        options.yaw_degrees,
        options.pitch_degrees,
        view.view_radius,
    );
    let session =
        Session::<_, G>::connect(connection, Mode::Playing, view.view_radius, start, || {
            std::thread::sleep(Duration::from_millis(1));
        })
        .map_err(|error| error.to_string())?;

    let (_session, failure) = run_window::<G>(session, &view)?;
    if let Some(host) = host {
        let (mut world, content) = host.stop();
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
    let start = camera_at([0.0, 0.0, 0.0], 0.0, 0.0, view.view_radius);
    let session =
        Session::<_, G>::connect(connection, Mode::Replaying, view.view_radius, start, || {})
            .map_err(|error| error.to_string())?;
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
    session: GameSession<G>,
    keys: HashSet<KeyCode>,
    mouse_captured: bool,
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
        session: GameSession<G>,
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
        configure_renderer(&mut renderer, session.content(), view.view_radius);
        renderer.set_crosshair(session.mode() == Mode::Playing && G::shows_crosshair());
        let now = Instant::now();
        Ok(Self {
            window,
            surface,
            surface_configuration,
            context,
            renderer,
            session,
            keys: HashSet::new(),
            mouse_captured: false,
            entry: CommandEntry::default(),
            reply: None,
            started: now,
            last_frame: now,
            last_pose_sent: now,
            last_movement: None,
            last_movement_sent: now,
            follower: PoseFollower::new(),
            frames_since_title_update: 0,
            last_title_update: now,
            frames_total: 0,
            last_statistics: RenderStatistics::default(),
            exit_after: view.exit_after_seconds.map(Duration::from_secs_f32),
            ticks_played: 0,
            recording_ended: false,
        })
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

    fn move_camera(&mut self, seconds: f32) {
        let held = |code: KeyCode| self.keys.contains(&code);
        let mut movement = Vec3::ZERO;
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
        camera.move_in_world(Vec3::Y * vertical * speed * seconds);
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
    fn follow_server(&mut self, now: Instant, seconds: f32) -> Result<(), ClientError> {
        match self.session.mode() {
            Mode::Playing if G::server_moves_players() => {
                self.send_movement(now);
                self.session.advance()?;
                // The server says where the player is a few times a second; the picture is carried on
                // between those.
                if let Some(pose) = self.session.own_server_pose() {
                    self.session.camera.position = self.follower.view_position(pose.position, now);
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

    /// Draws one frame. Returns false when the game should close.
    fn frame(&mut self) -> Result<bool, String> {
        let now = Instant::now();
        let seconds = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        if self
            .exit_after
            .is_some_and(|limit| now.duration_since(self.started) >= limit)
        {
            return Ok(false);
        }

        self.follow_server(now, seconds)
            .map_err(|error| error.to_string())?;
        for text in self.session.take_command_replies() {
            println!("{text}");
            self.reply = Some((text, now));
        }
        let updates = self.session.update_view();
        apply_updates(&mut self.renderer, updates);
        show_other_players(&mut self.renderer, &self.session);

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

    fn update_title(&self, since_last: Duration) {
        let fps = f64::from(self.frames_since_title_update) / since_last.as_secs_f64();
        let statistics = self.last_statistics;
        let typing = self.entry.display();
        let reply = self
            .reply
            .as_ref()
            .filter(|(_, when)| when.elapsed() < REPLY_SHOWN_FOR)
            .map(|(text, _)| text.as_str());
        let mode = match self.session.mode() {
            Mode::Playing => format!(
                "players {} | {}{}",
                self.session.players().len().max(1),
                G::status_text(&self.session),
                match (&typing, reply) {
                    (Some(line), _) => format!(" | {line}"),
                    (None, Some(text)) => format!(" | {text}"),
                    (None, None) => String::new(),
                }
            ),
            Mode::Replaying => format!(
                "replay {}{}",
                self.session.client().world().tick(),
                if self.recording_ended { " (ended)" } else { "" }
            ),
        };
        // What the player is doing comes first: a narrow window cuts the end of its title off.
        self.window.set_title(&format!(
            "{mode} | {} | {fps:.0} fps | drawn {}/{} bricks, {} quads | loading {} | {}",
            G::title(),
            statistics.bricks_drawn,
            statistics.bricks_drawn + statistics.bricks_culled,
            statistics.quads_drawn,
            self.session.mesher().pending_jobs(),
            describe(&self.context.adapter),
        ));
    }
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
            }
            // A press the system makes up when the window gains focus while a key is down belongs
            // to whatever had the focus before; a release is harmless.
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: true,
                ..
            } if event.state == ElementState::Pressed => {}
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    match event.state {
                        ElementState::Pressed if viewer.entry.is_active() => match code {
                            KeyCode::Escape => viewer.entry.cancel(),
                            KeyCode::Enter | KeyCode::NumpadEnter => {
                                if let Some(text) = viewer.entry.submit() {
                                    viewer.session.send_command(&text);
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
                            let opens_a_command = viewer.session.mode() == Mode::Playing
                                && opens_a_command(
                                    event.text.as_deref(),
                                    event.repeat,
                                    false,
                                    viewer.keys.contains(&code),
                                );
                            if opens_a_command {
                                // Typing is not moving: let go of every key.
                                viewer.keys.clear();
                                viewer.entry.begin("/");
                                println!("command line open: Enter sends it, Esc drops it");
                            } else if code == KeyCode::Escape {
                                viewer.set_mouse_captured(false);
                                viewer.keys.insert(code);
                            } else {
                                if !viewer.keys.contains(&code) {
                                    G::key_pressed(&mut viewer.session, code);
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
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => match (button, viewer.mouse_captured) {
                (MouseButton::Left, false) => viewer.set_mouse_captured(true),
                (MouseButton::Left, true) => G::primary_action(&mut viewer.session),
                (MouseButton::Right, true) => G::secondary_action(&mut viewer.session),
                _ => {}
            },
            WindowEvent::Focused(focused) => {
                // Keys held, and a command half typed, belong to the window that had the focus.
                viewer.keys.clear();
                viewer.entry.cancel();
                if !focused {
                    viewer.set_mouse_captured(false);
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
            viewer.session.camera.look_by(
                -delta.0 as f32 * MOUSE_SENSITIVITY,
                -delta.1 as f32 * MOUSE_SENSITIVITY,
            );
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(viewer) = &self.viewer {
            viewer.window.request_redraw();
        }
    }
}
