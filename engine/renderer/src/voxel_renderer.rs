use std::collections::HashMap;
use std::num::NonZeroU64;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use voxel_meshing::PackedQuad;
use voxel_space::{BrickCoordinate, WorldPosition};
use wgpu::util::DeviceExt;

use crate::camera::{Camera, Frustum};
use crate::crosshair::Crosshair;
use crate::graphics_context::GraphicsContext;

const PALETTE_SIZE: usize = 256;
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Radius of a sphere around a brick: half the diagonal of a 32-voxel cube.
const BRICK_BOUNDING_RADIUS: f32 = 27.72;
const BRICK_CENTER_OFFSET: f32 = 16.0;
const PLACEMENT_SIZE: u64 = 16;

/// Colours are given in sRGB, as in an image editor; the renderer converts them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Atmosphere {
    pub sky_color: [f32; 3],
    /// Distance in voxels where distant terrain starts to fade into the sky.
    pub fog_start: f32,
    /// Distance in voxels where it has fully faded.
    pub fog_end: f32,
    /// Direction towards the sun (need not be normalized).
    pub sun_direction: [f32; 3],
}

impl Default for Atmosphere {
    fn default() -> Self {
        Self {
            sky_color: [0.55, 0.72, 0.92],
            fog_start: 400.0,
            fog_end: 900.0,
            sun_direction: [0.4, 0.8, 0.3],
        }
    }
}

/// What a frame drew.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderStatistics {
    pub bricks_drawn: usize,
    pub bricks_culled: usize,
    pub quads_drawn: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniform {
    view_projection: [[f32; 4]; 4],
    fog_color: [f32; 4],
    fog_range: [f32; 4],
    sun_direction: [f32; 4],
    palette: [[f32; 4]; PALETTE_SIZE],
}

struct GpuBrick {
    quads: wgpu::Buffer,
    quad_count: u32,
}

struct PlacementBuffer {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    capacity: usize,
}

struct SizedTarget {
    size: (u32, u32),
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// Draws bricks of meshed voxels. Positions are camera-relative: each brick's offset from the
/// camera is computed in exact integer arithmetic, so precision does not degrade far from the
/// origin.
pub struct VoxelRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    color_format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    frame_buffer: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    placement_layout: wgpu::BindGroupLayout,
    placements: PlacementBuffer,
    placement_stride: u64,
    bricks: HashMap<BrickCoordinate, GpuBrick>,
    palette: Box<[[f32; 4]; PALETTE_SIZE]>,
    atmosphere: Atmosphere,
    depth: Option<SizedTarget>,
    offscreen: Option<SizedTarget>,
    crosshair: Crosshair,
    crosshair_enabled: bool,
}

impl std::fmt::Debug for VoxelRenderer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VoxelRenderer")
            .field("color_format", &self.color_format)
            .field("brick_count", &self.bricks.len())
            .field("quad_count", &self.total_quad_count())
            .finish()
    }
}

// sRGB to linear needs a power function; this is client-side cosmetics, not simulation.
#[allow(clippy::disallowed_methods)]
fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb_to_linear_color(color: [f32; 3]) -> [f32; 3] {
    color.map(srgb_to_linear)
}

impl VoxelRenderer {
    /// `color_format` must be an sRGB format (the shader outputs linear colour).
    pub fn new(context: &GraphicsContext, color_format: wgpu::TextureFormat) -> Self {
        assert!(
            color_format.is_srgb(),
            "the renderer needs an sRGB colour format"
        );
        let device = context.device.clone();
        let queue = context.queue.clone();

        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(size_of::<FrameUniform>() as u64),
                },
                count: None,
            }],
        });
        let placement_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("placement layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(PLACEMENT_SIZE),
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("voxel pipeline layout"),
            bind_group_layouts: &[Some(&frame_layout), Some(&placement_layout)],
            immediate_size: 0,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("voxel shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("voxel.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("voxel pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<PackedQuad>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Uint32, 1 => Uint32],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame uniform"),
            size: size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame bind group"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buffer.as_entire_binding(),
            }],
        });

        let alignment = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        let placement_stride = PLACEMENT_SIZE.next_multiple_of(alignment);
        let placements =
            Self::create_placement_buffer(&device, &placement_layout, placement_stride, 64);

        let crosshair = Crosshair::new(&device, color_format);
        let mut renderer = Self {
            device,
            queue,
            color_format,
            pipeline,
            frame_buffer,
            frame_bind_group,
            placement_layout,
            placements,
            placement_stride,
            bricks: HashMap::new(),
            palette: Box::new([[1.0, 0.0, 1.0, 1.0]; PALETTE_SIZE]),
            atmosphere: Atmosphere::default(),
            depth: None,
            offscreen: None,
            crosshair,
            crosshair_enabled: false,
        };
        renderer.set_palette(&[]);
        renderer
    }

    fn create_placement_buffer(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        stride: u64,
        capacity: usize,
    ) -> PlacementBuffer {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("brick placements"),
            size: stride * capacity as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("placement bind group"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: NonZeroU64::new(PLACEMENT_SIZE),
                }),
            }],
        });
        PlacementBuffer {
            buffer,
            bind_group,
            capacity,
        }
    }

    /// Draws a crosshair at the centre of every picture, for aiming. Off by default.
    pub fn set_crosshair(&mut self, enabled: bool) {
        self.crosshair_enabled = enabled;
    }

    /// Sets the colour of material values `0..colors.len()` (sRGB). Larger values draw magenta, which
    /// is also how content from a missing plugin shows up.
    pub fn set_palette(&mut self, colors: &[[f32; 3]]) {
        assert!(
            colors.len() <= PALETTE_SIZE,
            "at most {PALETTE_SIZE} materials can be drawn"
        );
        self.palette.fill([1.0, 0.0, 1.0, 1.0]);
        for (slot, color) in self.palette.iter_mut().zip(colors) {
            let [red, green, blue] = srgb_to_linear_color(*color);
            *slot = [red, green, blue, 1.0];
        }
    }

    pub fn set_atmosphere(&mut self, atmosphere: Atmosphere) {
        self.atmosphere = atmosphere;
    }

    /// Replaces the mesh of one brick. An empty slice removes the brick.
    pub fn upload_brick(&mut self, coordinate: BrickCoordinate, quads: &[PackedQuad]) {
        if quads.is_empty() {
            self.bricks.remove(&coordinate);
            return;
        }
        let words: Vec<u32> = quads
            .iter()
            .flat_map(|quad| [quad.geometry, quad.material])
            .collect();
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("brick quads"),
                contents: bytemuck::cast_slice(&words),
                usage: wgpu::BufferUsages::VERTEX,
            });
        self.bricks.insert(
            coordinate,
            GpuBrick {
                quads: buffer,
                quad_count: quads.len() as u32,
            },
        );
    }

    pub fn remove_brick(&mut self, coordinate: BrickCoordinate) {
        self.bricks.remove(&coordinate);
    }

    pub fn brick_count(&self) -> usize {
        self.bricks.len()
    }

    pub fn has_brick(&self, coordinate: BrickCoordinate) -> bool {
        self.bricks.contains_key(&coordinate)
    }

    pub fn total_quad_count(&self) -> usize {
        self.bricks
            .values()
            .map(|brick| brick.quad_count as usize)
            .sum()
    }

    /// Draws every visible brick into `color_view`, which must be `size` pixels in the renderer's
    /// colour format.
    pub fn render(
        &mut self,
        color_view: &wgpu::TextureView,
        size: (u32, u32),
        camera: &Camera,
    ) -> RenderStatistics {
        self.ensure_depth_target(size);
        let aspect_ratio = size.0 as f32 / size.1 as f32;
        let view_projection = camera.view_projection(aspect_ratio);
        let frustum = Frustum::from_view_projection(view_projection);

        // Camera-relative placement, computed exactly from integers, then culled and sorted
        // front to back so nearer bricks hide farther ones early.
        let mut visible: Vec<(f32, BrickCoordinate, Vec3)> = Vec::new();
        for &coordinate in self.bricks.keys() {
            let corner = WorldPosition::new(coordinate, [0.0; 3]).relative_to(camera.position);
            let corner = Vec3::from_array(corner);
            let center = corner + Vec3::splat(BRICK_CENTER_OFFSET);
            if frustum.intersects_sphere(center, BRICK_BOUNDING_RADIUS) {
                visible.push((center.length_squared(), coordinate, corner));
            }
        }
        visible.sort_by(|a, b| a.0.total_cmp(&b.0));
        let statistics = RenderStatistics {
            bricks_drawn: visible.len(),
            bricks_culled: self.bricks.len() - visible.len(),
            quads_drawn: visible
                .iter()
                .map(|(_, coordinate, _)| self.bricks[coordinate].quad_count as usize)
                .sum(),
        };

        self.write_placements(visible.iter().map(|(_, _, corner)| *corner));
        self.write_frame_uniform(view_projection);
        self.crosshair.set_screen_size(&self.queue, size);

        let depth_view = &self
            .depth
            .as_ref()
            .expect("depth target was just created")
            .view;
        let [red, green, blue] = srgb_to_linear_color(self.atmosphere.sky_color);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("voxel frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("voxel pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(red),
                            g: f64::from(green),
                            b: f64::from(blue),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.frame_bind_group, &[]);
            for (index, (_, coordinate, _)) in visible.iter().enumerate() {
                let brick = &self.bricks[coordinate];
                pass.set_bind_group(
                    1,
                    &self.placements.bind_group,
                    &[(index as u64 * self.placement_stride) as u32],
                );
                pass.set_vertex_buffer(0, brick.quads.slice(..));
                pass.draw(0..6, 0..brick.quad_count);
            }
            if self.crosshair_enabled {
                self.crosshair.draw(&mut pass);
            }
        }
        self.queue.submit([encoder.finish()]);
        statistics
    }

    /// Renders to an internal texture without reading it back. For timing: call
    /// [`VoxelRenderer::wait_for_gpu`] afterwards to include the GPU's work.
    pub fn render_offscreen(
        &mut self,
        camera: &Camera,
        width: u32,
        height: u32,
    ) -> RenderStatistics {
        let size = (width, height);
        if self
            .offscreen
            .as_ref()
            .is_none_or(|target| target.size != size)
        {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("offscreen colour"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.color_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.offscreen = Some(SizedTarget {
                size,
                texture,
                view,
            });
        }
        let view = self.offscreen.as_ref().expect("just created").view.clone();
        self.render(&view, size, camera)
    }

    /// Renders to an internal texture and returns tightly packed RGBA bytes (sRGB encoded), the
    /// way an image file stores them.
    pub fn render_to_rgba(
        &mut self,
        camera: &Camera,
        width: u32,
        height: u32,
    ) -> (Vec<u8>, RenderStatistics) {
        let statistics = self.render_offscreen(camera, width, height);

        let padded_row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(padded_row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("readback"),
            });
        encoder.copy_texture_to_buffer(
            self.offscreen
                .as_ref()
                .expect("just created")
                .texture
                .as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                sender
                    .send(result)
                    .expect("receiver outlives the map callback");
            });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the device finishes the readback");
        receiver
            .recv()
            .expect("the map callback runs")
            .expect("mapping the readback buffer succeeds");

        let mapped = readback
            .slice(..)
            .get_mapped_range()
            .expect("the readback buffer is mapped");
        let swap_red_and_blue = matches!(self.color_format, wgpu::TextureFormat::Bgra8UnormSrgb);
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for row in 0..height as usize {
            let start = row * padded_row as usize;
            for texel in mapped[start..start + (width * 4) as usize]
                .as_chunks::<4>()
                .0
            {
                if swap_red_and_blue {
                    pixels.extend_from_slice(&[texel[2], texel[1], texel[0], texel[3]]);
                } else {
                    pixels.extend_from_slice(texel);
                }
            }
        }
        drop(mapped);
        readback.unmap();
        (pixels, statistics)
    }

    /// Blocks until everything submitted so far has finished on the GPU. For timing.
    pub fn wait_for_gpu(&self) {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the device finishes its work");
    }

    fn ensure_depth_target(&mut self, size: (u32, u32)) {
        if self
            .depth
            .as_ref()
            .is_some_and(|target| target.size == size)
        {
            return;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.depth = Some(SizedTarget {
            size,
            texture,
            view,
        });
    }

    fn write_placements(&mut self, corners: impl ExactSizeIterator<Item = Vec3>) {
        if corners.len() > self.placements.capacity {
            let capacity = corners.len().next_power_of_two();
            self.placements = Self::create_placement_buffer(
                &self.device,
                &self.placement_layout,
                self.placement_stride,
                capacity,
            );
        }
        let stride = self.placement_stride as usize;
        let mut bytes = vec![0u8; corners.len() * stride];
        for (index, corner) in corners.enumerate() {
            let placement = [corner.x, corner.y, corner.z, 0.0];
            bytes[index * stride..index * stride + PLACEMENT_SIZE as usize]
                .copy_from_slice(bytemuck::cast_slice(&placement));
        }
        if !bytes.is_empty() {
            self.queue.write_buffer(&self.placements.buffer, 0, &bytes);
        }
    }

    fn write_frame_uniform(&self, view_projection: glam::Mat4) {
        let [fog_red, fog_green, fog_blue] = srgb_to_linear_color(self.atmosphere.sky_color);
        let sun = Vec3::from_array(self.atmosphere.sun_direction).normalize_or(Vec3::Y);
        let uniform = FrameUniform {
            view_projection: view_projection.to_cols_array_2d(),
            fog_color: [fog_red, fog_green, fog_blue, 1.0],
            fog_range: [self.atmosphere.fog_start, self.atmosphere.fog_end, 0.0, 0.0],
            sun_direction: [sun.x, sun.y, sun.z, 0.0],
            palette: *self.palette,
        };
        self.queue
            .write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&uniform));
    }
}
