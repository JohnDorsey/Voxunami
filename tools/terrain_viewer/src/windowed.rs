//! The interactive viewer: a window, a fly camera and streaming terrain.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::Vec3;
use renderer::{Camera, GraphicsContext, RenderStatistics, VoxelRenderer, describe, new_instance};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::options::Options;
use crate::scene::{apply_updates, configure_renderer, initial_camera, new_streamer};
use crate::streaming::WorldStreamer;

const MOVEMENT_SPEED: f32 = 24.0; // voxels per second
const SPRINT_MULTIPLIER: f32 = 5.0;
const MOUSE_SENSITIVITY: f32 = 0.0025; // radians per pixel

pub fn run(options: Options) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|error| error.to_string())?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        options,
        state: None,
        failure: None,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|error| error.to_string())?;
    match app.failure {
        Some(failure) => Err(failure),
        None => Ok(()),
    }
}

struct App {
    options: Options,
    state: Option<Viewer>,
    failure: Option<String>,
}

struct Viewer {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    surface_configuration: wgpu::SurfaceConfiguration,
    context: GraphicsContext,
    renderer: VoxelRenderer,
    streamer: WorldStreamer,
    camera: Camera,
    keys: HashSet<KeyCode>,
    mouse_captured: bool,
    started: Instant,
    last_frame: Instant,
    frames_since_title_update: u32,
    last_title_update: Instant,
    frames_total: u64,
    last_statistics: RenderStatistics,
    exit_after: Option<Duration>,
}

impl Viewer {
    fn new(event_loop: &ActiveEventLoop, options: &Options) -> Result<Self, String> {
        let attributes = Window::default_attributes()
            .with_title("Voxunami terrain viewer")
            .with_inner_size(PhysicalSize::new(options.width, options.height));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| error.to_string())?,
        );
        let instance = new_instance();
        let surface = instance
            .create_surface(Arc::clone(&window))
            .map_err(|error| error.to_string())?;
        let context = GraphicsContext::new(instance, options.adapter, Some(&surface))
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
            present_mode: if options.vsync {
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
        configure_renderer(&mut renderer, options);
        let now = Instant::now();
        Ok(Self {
            window,
            surface,
            surface_configuration,
            context,
            renderer,
            streamer: new_streamer(options),
            camera: initial_camera(options),
            keys: HashSet::new(),
            mouse_captured: false,
            started: now,
            last_frame: now,
            frames_since_title_update: 0,
            last_title_update: now,
            frames_total: 0,
            last_statistics: RenderStatistics::default(),
            exit_after: options.exit_after_seconds.map(Duration::from_secs_f32),
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
        self.camera
            .move_relative(movement.normalize_or_zero() * speed * seconds);
        self.camera
            .move_in_world(Vec3::Y * vertical * speed * seconds);
    }

    /// Draws one frame. Returns false when the viewer should close.
    fn frame(&mut self) -> bool {
        let now = Instant::now();
        let seconds = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        if self
            .exit_after
            .is_some_and(|limit| now.duration_since(self.started) >= limit)
        {
            return false;
        }

        self.move_camera(seconds);
        let updates = self.streamer.update(self.camera.position.brick());
        apply_updates(&mut self.renderer, updates);

        let (frame, reconfigure) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => (frame, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface
                    .configure(&self.context.device, &self.surface_configuration);
                return true;
            }
            _ => return true, // timed out, occluded or a validation error: skip this frame
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let size = (
            self.surface_configuration.width,
            self.surface_configuration.height,
        );
        self.last_statistics = self.renderer.render(&view, size, &self.camera);
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
            let fps = f64::from(self.frames_since_title_update) / since_title.as_secs_f64();
            let statistics = self.last_statistics;
            self.window.set_title(&format!(
                "Voxunami terrain viewer | {fps:.0} fps | drawn {}/{} bricks, {} quads | loading {} | {}",
                statistics.bricks_drawn,
                statistics.bricks_drawn + statistics.bricks_culled,
                statistics.quads_drawn,
                self.streamer.pending_jobs(),
                describe(&self.context.adapter),
            ));
            self.frames_since_title_update = 0;
            self.last_title_update = now;
        }
        true
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_none() {
            match Viewer::new(event_loop, &self.options) {
                Ok(viewer) => self.state = Some(viewer),
                Err(failure) => {
                    self.failure = Some(failure);
                    event_loop.exit();
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window: WindowId,
        event: WindowEvent,
    ) {
        let Some(viewer) = self.state.as_mut() else {
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
                            if code == KeyCode::Escape {
                                viewer.set_mouse_captured(false);
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
                button: MouseButton::Left,
                ..
            } => {
                viewer.set_mouse_captured(true);
            }
            WindowEvent::Focused(false) => {
                viewer.keys.clear();
                viewer.set_mouse_captured(false);
            }
            WindowEvent::RedrawRequested => {
                let keep_running = viewer.frame();
                if !keep_running {
                    let seconds = viewer.started.elapsed().as_secs_f64();
                    println!(
                        "closed after {seconds:.1} s: {} frames, {:.0} fps average",
                        viewer.frames_total,
                        viewer.frames_total as f64 / seconds
                    );
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device: DeviceId,
        event: DeviceEvent,
    ) {
        let Some(viewer) = self.state.as_mut() else {
            return;
        };
        if let DeviceEvent::MouseMotion { delta } = event
            && viewer.mouse_captured
        {
            viewer.camera.look_by(
                -delta.0 as f32 * MOUSE_SENSITIVITY,
                -delta.1 as f32 * MOUSE_SENSITIVITY,
            );
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(viewer) = &self.state {
            viewer.window.request_redraw();
        }
    }
}
