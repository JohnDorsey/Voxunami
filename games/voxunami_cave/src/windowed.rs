//! The interactive game: a window, a fly camera, and block breaking and placing; or the same window
//! playing a recording back in real time.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use client::ClientError;
use glam::Vec3;
use renderer::{GraphicsContext, RenderStatistics, VoxelRenderer, describe, new_instance};
use transport::Connection;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::content::{CaveContent, game_id};
use crate::files::{open_recording, open_world, record_connection, recorded_game, save_world};
use crate::host::IntegratedServer;
use crate::options::{PlayOptions, ReplayOptions, ViewOptions};
use crate::session::{Mode, Session};
use crate::view::{apply_updates, camera_at, configure_renderer};

const MOVEMENT_SPEED: f32 = 24.0; // voxels per second
const SPRINT_MULTIPLIER: f32 = 5.0;
const MOUSE_SENSITIVITY: f32 = 0.0025; // radians per pixel
const POSE_INTERVAL: Duration = Duration::from_millis(50);
const PLACEABLE_BLOCKS: [&str; 3] = ["base_blocks:stone", "base_blocks:dirt", "base_blocks:grass"];

type GameSession = Session<Box<dyn Connection>>;

/// Plays: starts a server in this process, joins it, and runs the window until it closes. The world
/// is saved on exit if a file was named.
pub fn play(options: PlayOptions) -> Result<(), String> {
    let view = options.view.clone();
    let mut content = CaveContent::new();
    let world = open_world(&mut content, options.world_file.as_deref(), options.seed)?;
    let host = IntegratedServer::start(&content, world, view.view_radius)
        .map_err(|error| error.to_string())?;
    let connection = host.connect().map_err(|error| error.to_string())?;
    let connection: Box<dyn Connection> = match &options.record {
        Some(path) => record_connection(connection, path)?,
        None => Box::new(connection),
    };
    let start = camera_at(
        options.camera,
        options.yaw_degrees,
        options.pitch_degrees,
        view.view_radius,
    );
    let session = Session::connect(connection, Mode::Playing, view.view_radius, start, || {
        std::thread::sleep(Duration::from_millis(1));
    })
    .map_err(|error| error.to_string())?;

    let (_session, failure) = run_window(session, &view)?;
    let mut world = host.stop();
    if let Some(path) = &options.world_file {
        save_world(&content, &mut world, path)?;
        println!("saved the world to {}", path.display());
    }
    failure.map_or(Ok(()), Err)
}

/// Watches a recording in real time from the recorded player's point of view.
pub fn replay(options: ReplayOptions) -> Result<(), String> {
    let game = recorded_game(&options.file)?;
    if game != game_id() {
        return Err(format!(
            "this recording was made by {game}, not {}",
            game_id()
        ));
    }
    let view = options.view;
    let connection: Box<dyn Connection> = Box::new(open_recording(&options.file)?);
    let start = camera_at([0.0, 0.0, 0.0], 0.0, 0.0, view.view_radius);
    let session = Session::connect(connection, Mode::Replaying, view.view_radius, start, || {})
        .map_err(|error| error.to_string())?;
    let (_, failure) = run_window(session, &view)?;
    failure.map_or(Ok(()), Err)
}

/// Runs the window. Returns the session, and what went wrong during play if something did.
fn run_window(
    session: GameSession,
    view: &ViewOptions,
) -> Result<(GameSession, Option<String>), String> {
    let event_loop = EventLoop::new().map_err(|error| error.to_string())?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
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

struct App {
    view: ViewOptions,
    /// Held until the window exists.
    session: Option<GameSession>,
    viewer: Option<Viewer>,
    failure: Option<String>,
}

struct Viewer {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    surface_configuration: wgpu::SurfaceConfiguration,
    context: GraphicsContext,
    renderer: VoxelRenderer,
    session: GameSession,
    keys: HashSet<KeyCode>,
    mouse_captured: bool,
    selected_block: usize,
    started: Instant,
    last_frame: Instant,
    last_pose_sent: Instant,
    frames_since_title_update: u32,
    last_title_update: Instant,
    frames_total: u64,
    last_statistics: RenderStatistics,
    exit_after: Option<Duration>,
    ticks_played: u64,
    recording_ended: bool,
}

impl Viewer {
    fn new(
        event_loop: &ActiveEventLoop,
        view: &ViewOptions,
        session: GameSession,
    ) -> Result<Self, String> {
        let attributes = Window::default_attributes()
            .with_title("Voxunami Cave")
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
        renderer.set_crosshair(session.mode() == Mode::Playing);
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
            selected_block: 0,
            started: now,
            last_frame: now,
            last_pose_sent: now,
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

    /// Takes in what the server has sent. Returns an error to stop the game with.
    fn follow_server(&mut self, now: Instant, seconds: f32) -> Result<(), ClientError> {
        match self.session.mode() {
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
        let updates = self.session.update_view();
        apply_updates(&mut self.renderer, updates);

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
        let mode = match self.session.mode() {
            Mode::Playing => format!(
                "players {} | placing {}",
                self.session.players().len().max(1),
                PLACEABLE_BLOCKS[self.selected_block]
                    .split(':')
                    .next_back()
                    .unwrap_or_default()
            ),
            Mode::Replaying => format!(
                "replay {}{}",
                self.session.client().world().tick(),
                if self.recording_ended { " (ended)" } else { "" }
            ),
        };
        self.window.set_title(&format!(
            "Voxunami Cave | {fps:.0} fps | drawn {}/{} bricks, {} quads | loading {} | {mode} | {}",
            statistics.bricks_drawn,
            statistics.bricks_drawn + statistics.bricks_culled,
            statistics.quads_drawn,
            self.session.mesher().pending_jobs(),
            describe(&self.context.adapter),
        ));
    }
}

impl ApplicationHandler for App {
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
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    match event.state {
                        ElementState::Pressed => {
                            match code {
                                KeyCode::Escape => viewer.set_mouse_captured(false),
                                KeyCode::Digit1 => viewer.selected_block = 0,
                                KeyCode::Digit2 => viewer.selected_block = 1,
                                KeyCode::Digit3 => viewer.selected_block = 2,
                                _ => {}
                            }
                            viewer.keys.insert(code);
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
                (MouseButton::Left, true) => viewer.session.break_block(),
                (MouseButton::Right, true) => viewer
                    .session
                    .place_block(PLACEABLE_BLOCKS[viewer.selected_block]),
                _ => {}
            },
            WindowEvent::Focused(false) => {
                viewer.keys.clear();
                viewer.set_mouse_captured(false);
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
