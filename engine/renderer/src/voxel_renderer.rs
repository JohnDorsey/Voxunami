use std::collections::HashMap;
use std::num::NonZeroU64;

use block_models::{BlockDefinition, BlockTable, Shape, ShapeBox};
use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use voxel_grid::ChannelStorage;
use voxel_meshing::{
    BrickChannels, BrickNeighbourhood, PackedBrickMesh, PackedDetailedQuad, PackedQuad, mesh_brick,
};
use voxel_space::{BRICK_VOXEL_COUNT, BrickCoordinate, LocalVoxelIndex, WorldPosition};
use wgpu::util::DeviceExt;

use crate::camera::{Camera, Frustum};
use crate::crosshair::Crosshair;
use crate::graphics_context::GraphicsContext;
use crate::overlay::{Overlay, OverlayRect};

const PALETTE_SIZE: usize = 256;
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Radius of a sphere around a brick: half the diagonal of a 32-voxel cube.
const BRICK_BOUNDING_RADIUS: f32 = 27.72;
const BRICK_CENTER_OFFSET: f32 = 16.0;
const PLACEMENT_SIZE: u64 = 16;

/// How many different avatar colours there are. They take the last palette values, so games can
/// use the values below `MAXIMUM_GAME_MATERIALS`.
pub const AVATAR_COLOR_COUNT: usize = 16;
pub const MAXIMUM_GAME_MATERIALS: usize = PALETTE_SIZE - AVATAR_COLOR_COUNT;
/// sRGB, chosen to be told apart from each other and from the usual terrain colours.
const AVATAR_COLORS: [[f32; 3]; AVATAR_COLOR_COUNT] = [
    [1.00, 0.20, 0.20],
    [0.20, 0.45, 1.00],
    [1.00, 0.85, 0.10],
    [0.75, 0.25, 0.95],
    [1.00, 0.55, 0.05],
    [0.10, 0.90, 0.90],
    [1.00, 0.45, 0.75],
    [0.95, 0.95, 0.95],
    [0.10, 0.10, 0.12],
    [0.55, 0.95, 0.15],
    [0.55, 0.30, 0.10],
    [0.30, 0.30, 0.95],
    [0.90, 0.10, 0.50],
    [0.20, 0.65, 0.55],
    [0.65, 0.65, 0.20],
    [0.50, 0.50, 0.60],
];
/// An avatar's mesh is a voxel model in a brick; this local voxel is where the player's eyes are.
const AVATAR_EYE_VOXEL: u8 = 16;
/// Radius of a sphere around an avatar, for culling.
const AVATAR_BOUNDING_RADIUS: f32 = 2.0;

/// Another player, drawn where their eyes are: a head over a body, in a colour of their own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Avatar {
    /// Where the player's eyes are.
    pub position: WorldPosition,
    /// Which of the [`AVATAR_COLOR_COUNT`] colours; larger numbers wrap around.
    pub color: u32,
}

/// Radius of a sphere around an entity's box, for culling.
const ENTITY_BOX_BOUNDING_RADIUS: f32 = 1.0;

/// An entity drawn as a box: a thing that is not a player, and has no model yet (an item on the ground).
/// Boxes are placed like avatars, and made like blocks that do not fill their cell, so their sizes are in
/// eighths of a voxel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityBox {
    /// The middle of the box.
    pub position: WorldPosition,
    /// How big the box is along each axis, in eighths of a voxel. Each is made even and from 2 to 8, so that
    /// the box can be centred on `position`.
    pub size: [u8; 3],
    /// The palette value it is drawn in (the same values the blocks of the world are drawn in); the values
    /// the avatars use are not for boxes.
    pub material: u32,
}

impl EntityBox {
    /// What the box is made of, as the renderer remembers its mesh: the size made even and put from 2 to 8,
    /// and the material put among the game's.
    fn key(&self) -> (u32, [u8; 3]) {
        let material = self.material.clamp(1, MAXIMUM_GAME_MATERIALS as u32 - 1);
        (
            material,
            self.size.map(|size| size.clamp(1, 8).div_ceil(2) * 2),
        )
    }
}

/// The mesh of a box of one material and size: a voxel at the middle of a brick whose shape is a box.
fn box_mesh(material: u32, size: [u8; 3]) -> PackedBrickMesh {
    let mut values = vec![0u32; BRICK_VOXEL_COUNT];
    let at = LocalVoxelIndex::new(AVATAR_EYE_VOXEL, AVATAR_EYE_VOXEL, AVATAR_EYE_VOXEL);
    values[at.index()] = material;
    let storage = ChannelStorage::from_values(&values);
    let shape = Shape::from_boxes(&[ShapeBox {
        min: size.map(|size| (8 - size) / 2),
        max: size.map(|size| (8 + size) / 2),
    }]);
    let solid = BlockDefinition::cube([0.5; 3]).with_shapes(vec![shape]);
    let air = BlockDefinition::air();
    let definitions: Vec<Option<&BlockDefinition>> = (0..=material)
        .map(|block| Some(if block == material { &solid } else { &air }))
        .collect();
    mesh_brick(
        &BrickNeighbourhood {
            center: BrickChannels::blocks_only(&storage),
            neighbours: [None; 6],
        },
        &BlockTable::from_definitions(&definitions),
    )
    .pack()
}

/// The mesh of an avatar of one colour: two voxels, the head with the eyes in it over the body.
fn avatar_mesh(color: usize) -> PackedBrickMesh {
    let material = (MAXIMUM_GAME_MATERIALS + color) as u32;
    let mut values = vec![0u32; BRICK_VOXEL_COUNT];
    for y in [AVATAR_EYE_VOXEL - 1, AVATAR_EYE_VOXEL] {
        let at = LocalVoxelIndex::new(AVATAR_EYE_VOXEL, y, AVATAR_EYE_VOXEL);
        values[at.index()] = material;
    }
    let storage = ChannelStorage::from_values(&values);
    // Every palette value is an opaque cube; air is the value that is not used.
    let mut opaque = vec![true; PALETTE_SIZE];
    opaque[0] = false;
    mesh_brick(
        &BrickNeighbourhood {
            center: BrickChannels::blocks_only(&storage),
            neighbours: [None; 6],
        },
        &BlockTable::of_cubes(&opaque),
    )
    .pack()
}

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
    /// How bright, from 0 to 255, a face is that no light reaches. A face's brightness is its light
    /// and this one combined like screen blending in an image editor (each as a fraction of full
    /// brightness, one minus the product of what is missing from each), so light adds to the
    /// ambient and never goes beyond full. 0, the default, is no ambient light: brightness is the
    /// face's light alone.
    pub ambient_light: u8,
}

impl Default for Atmosphere {
    fn default() -> Self {
        Self {
            sky_color: [0.55, 0.72, 0.92],
            fog_start: 400.0,
            fog_end: 900.0,
            sun_direction: [0.4, 0.8, 0.3],
            ambient_light: 0,
        }
    }
}

/// What a frame drew.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderStatistics {
    pub bricks_drawn: usize,
    pub bricks_culled: usize,
    /// Whole-voxel quads and detailed quads together.
    pub quads_drawn: usize,
    /// Of those, the detailed ones.
    pub detailed_quads_drawn: usize,
    pub avatars_drawn: usize,
    pub entity_boxes_drawn: usize,
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

/// The meshes of a brick on the GPU: whole-voxel quads, and detailed ones if it has any.
struct GpuBrick {
    quads: Option<wgpu::Buffer>,
    quad_count: u32,
    detailed: Option<wgpu::Buffer>,
    detailed_count: u32,
}

impl GpuBrick {
    fn quad_total(&self) -> usize {
        (self.quad_count + self.detailed_count) as usize
    }
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
    detailed_pipeline: wgpu::RenderPipeline,
    frame_buffer: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    placement_layout: wgpu::BindGroupLayout,
    placements: PlacementBuffer,
    placement_stride: u64,
    bricks: HashMap<BrickCoordinate, GpuBrick>,
    avatar_meshes: Vec<GpuBrick>,
    avatars: Vec<Avatar>,
    entity_boxes: Vec<EntityBox>,
    /// The meshes of the boxes there have been, by material and size, made when first asked for.
    box_meshes: HashMap<(u32, [u8; 3]), GpuBrick>,
    palette: Box<[[f32; 4]; PALETTE_SIZE]>,
    atmosphere: Atmosphere,
    depth: Option<SizedTarget>,
    offscreen: Option<SizedTarget>,
    crosshair: Crosshair,
    crosshair_enabled: bool,
    overlay: Overlay,
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
pub(crate) fn srgb_to_linear(value: f32) -> f32 {
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
        let make_pipeline =
            |label: &str, entry_point: &str, vertex_buffer: wgpu::VertexBufferLayout<'_>| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some(entry_point),
                        compilation_options: Default::default(),
                        buffers: &[Some(vertex_buffer)],
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
                })
            };
        let pipeline = make_pipeline(
            "voxel pipeline",
            "vertex_main",
            wgpu::VertexBufferLayout {
                array_stride: size_of::<PackedQuad>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0 => Uint32, 1 => Uint32],
            },
        );
        let detailed_pipeline = make_pipeline(
            "detailed voxel pipeline",
            "vertex_detailed",
            wgpu::VertexBufferLayout {
                array_stride: size_of::<PackedDetailedQuad>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0 => Uint32, 1 => Uint32, 2 => Uint32, 3 => Uint32],
            },
        );

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
        let device_for_overlay = device.clone();
        let mut renderer = Self {
            device,
            queue,
            color_format,
            pipeline,
            detailed_pipeline,
            frame_buffer,
            frame_bind_group,
            placement_layout,
            placements,
            placement_stride,
            bricks: HashMap::new(),
            avatar_meshes: Vec::new(),
            avatars: Vec::new(),
            entity_boxes: Vec::new(),
            box_meshes: HashMap::new(),
            palette: Box::new([[1.0, 0.0, 1.0, 1.0]; PALETTE_SIZE]),
            atmosphere: Atmosphere::default(),
            depth: None,
            offscreen: None,
            crosshair,
            crosshair_enabled: false,
            overlay: Overlay::new(&device_for_overlay, color_format),
        };
        renderer.set_palette(&[]);
        renderer.avatar_meshes = (0..AVATAR_COLOR_COUNT)
            .map(|color| renderer.gpu_brick(&avatar_mesh(color)))
            .collect();
        renderer
    }

    fn gpu_brick(&self, mesh: &PackedBrickMesh) -> GpuBrick {
        let buffer = |label: &str, words: Vec<u32>| {
            (!words.is_empty()).then(|| {
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(label),
                        contents: bytemuck::cast_slice(&words),
                        usage: wgpu::BufferUsages::VERTEX,
                    })
            })
        };
        GpuBrick {
            quads: buffer(
                "quads",
                mesh.quads
                    .iter()
                    .flat_map(|quad| [quad.geometry, quad.material])
                    .collect(),
            ),
            quad_count: mesh.quads.len() as u32,
            detailed: buffer(
                "detailed quads",
                mesh.detailed.iter().flat_map(|quad| quad.words).collect(),
            ),
            detailed_count: mesh.detailed.len() as u32,
        }
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

    /// Sets the rectangles drawn over every picture from now on, after the crosshair, in order, later over earlier (see
    /// [`OverlayRect`]); none draws nothing. The user interface is drawn with these.
    pub fn set_overlay(&mut self, rects: &[OverlayRect]) {
        self.overlay.set(&self.device, rects);
    }

    /// How many rectangles of the overlay are drawn (those with a size).
    pub fn overlay_rect_count(&self) -> usize {
        self.overlay.count()
    }

    /// Draws a crosshair at the centre of every picture, for aiming. Off by default.
    pub fn set_crosshair(&mut self, enabled: bool) {
        self.crosshair_enabled = enabled;
    }

    /// Sets the colour of material values `0..colors.len()` (sRGB). Larger values draw magenta, which
    /// is also how content from a missing plugin shows up. The last [`AVATAR_COLOR_COUNT`] values
    /// are the avatars' colours, so at most 240 materials can be drawn.
    pub fn set_palette(&mut self, colors: &[[f32; 3]]) {
        assert!(
            colors.len() <= MAXIMUM_GAME_MATERIALS,
            "at most {MAXIMUM_GAME_MATERIALS} materials can be drawn"
        );
        self.palette.fill([1.0, 0.0, 1.0, 1.0]);
        for (slot, color) in self.palette.iter_mut().zip(colors) {
            let [red, green, blue] = srgb_to_linear_color(*color);
            *slot = [red, green, blue, 1.0];
        }
        for (slot, color) in self.palette[MAXIMUM_GAME_MATERIALS..]
            .iter_mut()
            .zip(AVATAR_COLORS)
        {
            let [red, green, blue] = srgb_to_linear_color(color);
            *slot = [red, green, blue, 1.0];
        }
    }

    /// Sets which avatars are drawn from now on, replacing the previous ones.
    pub fn set_avatars(&mut self, avatars: &[Avatar]) {
        self.avatars = avatars.to_vec();
    }

    /// Sets which boxes are drawn from now on, replacing the previous ones.
    pub fn set_entity_boxes(&mut self, boxes: &[EntityBox]) {
        for entity_box in boxes {
            let (material, size) = entity_box.key();
            if !self.box_meshes.contains_key(&(material, size)) {
                let mesh = self.gpu_brick(&box_mesh(material, size));
                self.box_meshes.insert((material, size), mesh);
            }
        }
        self.entity_boxes = boxes.to_vec();
    }

    pub fn set_atmosphere(&mut self, atmosphere: Atmosphere) {
        self.atmosphere = atmosphere;
    }

    /// Replaces the mesh of one brick. An empty mesh removes the brick.
    pub fn upload_brick(&mut self, coordinate: BrickCoordinate, mesh: &PackedBrickMesh) {
        if mesh.is_empty() {
            self.bricks.remove(&coordinate);
            return;
        }
        let brick = self.gpu_brick(mesh);
        self.bricks.insert(coordinate, brick);
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

    /// Whole-voxel quads and detailed quads of every brick held.
    pub fn total_quad_count(&self) -> usize {
        self.bricks.values().map(GpuBrick::quad_total).sum()
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
        // Avatars are placed like bricks: by where their model's corner is relative to the camera.
        let eye = Vec3::splat(f32::from(AVATAR_EYE_VOXEL) + 0.5);
        let visible_avatars: Vec<(usize, Vec3)> = self
            .avatars
            .iter()
            .filter_map(|avatar| {
                let relative = Vec3::from_array(avatar.position.relative_to(camera.position));
                frustum
                    .intersects_sphere(relative, AVATAR_BOUNDING_RADIUS)
                    .then_some((avatar.color as usize % AVATAR_COLOR_COUNT, relative - eye))
            })
            .collect();
        let visible_boxes: Vec<((u32, [u8; 3]), Vec3)> = self
            .entity_boxes
            .iter()
            .filter_map(|entity_box| {
                let relative = Vec3::from_array(entity_box.position.relative_to(camera.position));
                frustum
                    .intersects_sphere(relative, ENTITY_BOX_BOUNDING_RADIUS)
                    .then_some((entity_box.key(), relative - eye))
            })
            .collect();
        let statistics = RenderStatistics {
            avatars_drawn: visible_avatars.len(),
            entity_boxes_drawn: visible_boxes.len(),
            bricks_drawn: visible.len(),
            bricks_culled: self.bricks.len() - visible.len(),
            quads_drawn: visible
                .iter()
                .map(|(_, coordinate, _)| self.bricks[coordinate].quad_total())
                .sum(),
            detailed_quads_drawn: visible
                .iter()
                .map(|(_, coordinate, _)| self.bricks[coordinate].detailed_count as usize)
                .sum(),
        };

        let corners: Vec<Vec3> = visible
            .iter()
            .map(|(_, _, corner)| *corner)
            .chain(visible_avatars.iter().map(|(_, corner)| *corner))
            .chain(visible_boxes.iter().map(|(_, corner)| *corner))
            .collect();
        self.write_placements(&corners);
        self.write_frame_uniform(view_projection);
        self.crosshair.set_screen_size(&self.queue, size);
        self.overlay.set_screen_size(&self.queue, size);

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
                let Some(quads) = &brick.quads else {
                    continue;
                };
                pass.set_bind_group(
                    1,
                    &self.placements.bind_group,
                    &[(index as u64 * self.placement_stride) as u32],
                );
                pass.set_vertex_buffer(0, quads.slice(..));
                pass.draw(0..6, 0..brick.quad_count);
            }
            // The bricks that have blocks that do not fill their cells, with the other pipeline.
            pass.set_pipeline(&self.detailed_pipeline);
            for (index, (_, coordinate, _)) in visible.iter().enumerate() {
                let brick = &self.bricks[coordinate];
                let Some(detailed) = &brick.detailed else {
                    continue;
                };
                pass.set_bind_group(
                    1,
                    &self.placements.bind_group,
                    &[(index as u64 * self.placement_stride) as u32],
                );
                pass.set_vertex_buffer(0, detailed.slice(..));
                pass.draw(0..6, 0..brick.detailed_count);
            }
            pass.set_pipeline(&self.pipeline);
            for (index, (color, _)) in visible_avatars.iter().enumerate() {
                let mesh = &self.avatar_meshes[*color];
                let Some(quads) = &mesh.quads else {
                    continue;
                };
                pass.set_bind_group(
                    1,
                    &self.placements.bind_group,
                    &[((visible.len() + index) as u64 * self.placement_stride) as u32],
                );
                pass.set_vertex_buffer(0, quads.slice(..));
                pass.draw(0..6, 0..mesh.quad_count);
            }
            // Boxes: the faces that fill a cell, then the ones that do not (a box is all of the second kind unless
            // it is a whole voxel).
            let first_box = visible.len() + visible_avatars.len();
            for (index, (key, _)) in visible_boxes.iter().enumerate() {
                let mesh = &self.box_meshes[key];
                let Some(quads) = &mesh.quads else {
                    continue;
                };
                pass.set_bind_group(
                    1,
                    &self.placements.bind_group,
                    &[((first_box + index) as u64 * self.placement_stride) as u32],
                );
                pass.set_vertex_buffer(0, quads.slice(..));
                pass.draw(0..6, 0..mesh.quad_count);
            }
            pass.set_pipeline(&self.detailed_pipeline);
            for (index, (key, _)) in visible_boxes.iter().enumerate() {
                let mesh = &self.box_meshes[key];
                let Some(detailed) = &mesh.detailed else {
                    continue;
                };
                pass.set_bind_group(
                    1,
                    &self.placements.bind_group,
                    &[((first_box + index) as u64 * self.placement_stride) as u32],
                );
                pass.set_vertex_buffer(0, detailed.slice(..));
                pass.draw(0..6, 0..mesh.detailed_count);
            }
            if self.crosshair_enabled {
                self.crosshair.draw(&mut pass);
            }
            self.overlay.draw(&mut pass);
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

    fn write_placements(&mut self, corners: &[Vec3]) {
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
        for (index, corner) in corners.iter().enumerate() {
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
            fog_range: [
                self.atmosphere.fog_start,
                self.atmosphere.fog_end,
                f32::from(self.atmosphere.ambient_light) / 255.0,
                0.0,
            ],
            sun_direction: [sun.x, sun.y, sun.z, 0.0],
            palette: *self.palette,
        };
        self.queue
            .write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&uniform));
    }
}
