//! What drawing very many nodes costs on the GPU, by how they are submitted: a buffer and a draw each, or one arena and one indirect draw (culled by the CPU or by a compute shader).

use std::num::NonZeroU64;
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use renderer::{Camera, Frustum, GraphicsContext, describe};
use voxel_space::{BrickCoordinate, WorldPosition};
use wgpu::util::DeviceExt;

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const BRICK_BOUNDING_RADIUS: f32 = 27.712_812; // 16 * sqrt(3)
const BRICK_CENTER_OFFSET: f32 = 16.0;

const PULLED_SHADER: &str = r#"
struct Frame {
    view_projection: mat4x4<f32>,
    fog_color: vec4<f32>,
    fog_range: vec4<f32>,
    sun_direction: vec4<f32>,
    avatars: array<vec4<f32>, 16>,
}
struct Placement {
    offset: vec4<f32>,
    rotation: mat3x3<f32>,
}
@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<storage, read> palette: array<vec4<f32>>;
@group(1) @binding(0) var<storage, read> quads: array<vec2<u32>>;
@group(1) @binding(1) var<storage, read> placements: array<Placement>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) color: vec3<f32>,
    @location(1) distance: f32,
}

fn axis_unit(axis: u32) -> vec3<f32> {
    return vec3<f32>(f32(axis == 0u), f32(axis == 1u), f32(axis == 2u));
}
fn corner_of(vertex_index: u32, positive: bool) -> u32 {
    var order = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u);
    if (!positive) {
        order = array<u32, 6>(0u, 2u, 1u, 0u, 3u, 2u);
    }
    return order[vertex_index];
}
fn corner_position(vertex_index: u32, face: u32, origin: vec3<f32>, width: f32, height: f32) -> vec3<f32> {
    let axis = face / 2u;
    let corner = corner_of(vertex_index, (face & 1u) == 1u);
    var u_extent = 0.0;
    if (corner == 1u || corner == 2u) {
        u_extent = width;
    }
    var v_extent = 0.0;
    if (corner == 2u || corner == 3u) {
        v_extent = height;
    }
    return origin + axis_unit((axis + 1u) % 3u) * u_extent + axis_unit((axis + 2u) % 3u) * v_extent;
}

@vertex
fn vertex_pulled(@builtin(vertex_index) vertex_index: u32, @builtin(instance_index) node: u32) -> VertexOutput {
    let quad = quads[vertex_index / 6u];
    let placement = placements[node];
    let geometry = quad.x;
    let origin = vec3<f32>(f32(geometry & 31u), f32((geometry >> 5u) & 31u), f32((geometry >> 10u) & 31u));
    let width = f32(((geometry >> 15u) & 31u) + 1u);
    let height = f32(((geometry >> 20u) & 31u) + 1u);
    let face = (geometry >> 25u) & 7u;
    let plane = origin + axis_unit(face / 2u) * select(0.0, 1.0, (face & 1u) == 1u);
    let local = corner_position(vertex_index % 6u, face, plane, width, height);
    let relative = placement.rotation * (local * placement.offset.w) + placement.offset.xyz;
    let normal = placement.rotation * (axis_unit(face / 2u) * select(-1.0, 1.0, (face & 1u) == 1u));
    let light = quad.y >> 24u;
    let material = quad.y & 0xFFFFFFu;
    let lit = 1.0 - (1.0 - frame.fog_range.z) * (1.0 - f32(light) / 255.0);
    let shade = (0.4 + 0.6 * max(dot(normal, frame.sun_direction.xyz), 0.0)) * lit;
    var base = vec3<f32>(1.0, 0.0, 1.0);
    if (material < arrayLength(&palette)) {
        base = palette[material].rgb;
    }
    var out: VertexOutput;
    out.position = frame.view_projection * vec4<f32>(relative, 1.0);
    out.color = base * shade;
    out.distance = length(relative);
    return out;
}

@fragment
fn fragment_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let fog = smoothstep(frame.fog_range.x, frame.fog_range.y, in.distance);
    return vec4<f32>(mix(in.color, frame.fog_color.rgb, fog), 1.0);
}
"#;

const CULL_SHADER: &str = r#"
struct Placement {
    offset: vec4<f32>,
    rotation: mat3x3<f32>,
}
struct Node {
    // The node's lowest corner in the world and its scale (voxels to a unit of the mesh).
    corner: vec4<f32>,
    first_quad: u32,
    quad_count: u32,
    pad0: u32,
    pad1: u32,
}
struct Draw {
    vertex_count: u32,
    instance_count: u32,
    first_vertex: u32,
    first_instance: u32,
}
struct Cull {
    camera: vec4<f32>,
    planes: array<vec4<f32>, 6>,
    count: u32,
}
@group(0) @binding(0) var<uniform> cull: Cull;
@group(0) @binding(1) var<storage, read> nodes: array<Node>;
@group(0) @binding(2) var<storage, read_write> placements: array<Placement>;
@group(0) @binding(3) var<storage, read_write> draws: array<Draw>;

@compute @workgroup_size(64)
fn cull_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= cull.count) {
        return;
    }
    let node = nodes[i];
    let scale = node.corner.w;
    let offset = node.corner.xyz - cull.camera.xyz;
    let center = offset + vec3<f32>(16.0 * scale);
    let radius = 27.712812 * scale;
    var visible = true;
    for (var plane = 0u; plane < 6u; plane = plane + 1u) {
        let p = cull.planes[plane];
        if (dot(p.xyz, center) + p.w < -radius) {
            visible = false;
        }
    }
    placements[i] = Placement(vec4<f32>(offset, scale), mat3x3<f32>(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 0.0, 1.0)));
    var draw: Draw;
    draw.vertex_count = select(0u, node.quad_count * 6u, visible);
    draw.instance_count = 1u;
    draw.first_vertex = node.first_quad * 6u;
    draw.first_instance = i;
    draws[i] = draw;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniform {
    view_projection: [[f32; 4]; 4],
    fog_color: [f32; 4],
    fog_range: [f32; 4],
    sun_direction: [f32; 4],
    avatars: [[f32; 4]; 16],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Placement {
    offset: [f32; 4],
    rotation: [[f32; 4]; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct NodeInfo {
    corner: [f32; 4],
    first_quad: u32,
    quad_count: u32,
    pad: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DrawArgs {
    vertex_count: u32,
    instance_count: u32,
    first_vertex: u32,
    first_instance: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CullUniform {
    camera: [f32; 4],
    planes: [[f32; 4]; 6],
    count: u32,
    pad: [u32; 3],
}

/// A node's quads, as packed words: hills that step up and down, as a node of coarse terrain does.
fn node_quads() -> Vec<[u32; 2]> {
    let mut cells = vec![vec![vec![0u8; 32]; 32]; 32];
    for z in 0..32usize {
        for x in 0..32usize {
            let height = 12 + (((x / 6) * 7 + (z / 6) * 13) % 4);
            for y in 0..height {
                cells[x][y][z] = if y + 1 == height { 3 } else { 1 };
            }
        }
    }
    let node = {
        use coarse_cells::{CoarseCell, CoarseNode};
        let mut list = Vec::new();
        for z in 0..32usize {
            for y in 0..32usize {
                for x in 0..32usize {
                    let block = cells[x][y][z];
                    list.push(if block == 0 {
                        CoarseCell::empty(0, 255)
                    } else {
                        CoarseCell {
                            block: u32::from(block),
                            fill: 255,
                            light: 255,
                            exposure: 0,
                        }
                    });
                }
            }
        }
        CoarseNode::from_cells(list)
    };
    voxel_meshing::mesh_coarse_node(&node, &[None; 6])
        .pack()
        .quads
        .iter()
        .map(|quad| [quad.geometry, quad.material])
        .collect()
}

/// Where the nodes are: a sheet of them in front of the camera, all in view, or a ring all round it of which about a third is.
struct Scene {
    corners: Vec<[f32; 4]>,
}

fn scene_in_view(nodes: usize, level: u8) -> Scene {
    let side = (nodes as f64).sqrt().ceil() as i64;
    let width = 32.0 * (1u64 << level) as f32;
    let scale = (1u64 << level) as f32;
    let mut corners = Vec::new();
    'outer: for row in 0..side {
        for column in 0..side {
            if corners.len() == nodes {
                break 'outer;
            }
            corners.push([
                (column - side / 2) as f32 * width,
                0.0,
                -((row + 2) as f32) * width,
                scale,
            ]);
        }
    }
    Scene { corners }
}

fn scene_all_round(nodes: usize, level: u8) -> Scene {
    let side = (nodes as f64).sqrt().ceil() as i64;
    let width = 32.0 * (1u64 << level) as f32;
    let scale = (1u64 << level) as f32;
    let mut corners = Vec::new();
    'outer: for row in 0..side {
        for column in 0..side {
            if corners.len() == nodes {
                break 'outer;
            }
            corners.push([
                (column - side / 2) as f32 * width,
                0.0,
                (row - side / 2) as f32 * width,
                scale,
            ]);
        }
    }
    Scene { corners }
}

struct Target {
    color_texture: wgpu::Texture,
    color: wgpu::TextureView,
    depth: wgpu::TextureView,
}

fn target(device: &wgpu::Device) -> Target {
    let make = |format, label, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let color_texture = make(
        COLOR,
        "color",
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let depth_texture = make(DEPTH, "depth", wgpu::TextureUsages::RENDER_ATTACHMENT);
    Target {
        color: color_texture.create_view(&wgpu::TextureViewDescriptor::default()),
        depth: depth_texture.create_view(&wgpu::TextureViewDescriptor::default()),
        color_texture,
    }
}

struct Timing {
    query_set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
}

fn timing(device: &wgpu::Device) -> Timing {
    Timing {
        query_set: device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: 4,
        }),
        resolve: device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resolve"),
            size: 32,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        }),
        read: device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("read"),
            size: 32,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        }),
    }
}

fn read_timestamps(device: &wgpu::Device, timing: &Timing) -> [u64; 4] {
    let slice = timing.read.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| result.unwrap());
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let data = slice.get_mapped_range().expect("mapped");
    let words: &[u64] = bytemuck::cast_slice(&data);
    let found = [words[0], words[1], words[2], words[3]];
    drop(data);
    timing.read.unmap();
    found
}

/// A hash of the picture and how many pixels are not the clear colour.
fn picture(device: &wgpu::Device, queue: &wgpu::Queue, target: &wgpu::Texture) -> (u64, usize) {
    let bytes_per_row = WIDTH * 4;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(bytes_per_row * HEIGHT),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(HEIGHT),
            },
        },
        wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| result.unwrap());
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let data = slice.get_mapped_range().expect("mapped");
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut drawn = 0usize;
    let sky = data[0..4].to_vec();
    for pixel in data.chunks_exact(4) {
        for byte in pixel {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01B3);
        }
        drawn += usize::from(pixel != sky.as_slice());
    }
    (hash, drawn)
}

struct Result {
    cpu_ms: f64,
    gpu_ms: f64,
    wall_ms: f64,
    drawn: usize,
}

#[allow(clippy::too_many_lines)]
fn run(adapter: &wgpu::Adapter, label: &str, quads: &[[u32; 2]], scene: &Scene, camera: &Camera) {
    let features = adapter.features();
    let wanted = wgpu::Features::INDIRECT_FIRST_INSTANCE | wgpu::Features::TIMESTAMP_QUERY;
    if !features.contains(wanted) {
        eprintln!("GPU {label}: the adapter lacks {:?}", wanted - features);
        return;
    }
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("bench"),
        required_features: wanted,
        ..Default::default()
    }))
    .expect("device");
    let period = f64::from(queue.get_timestamp_period());
    let nodes = scene.corners.len();
    let quad_count = quads.len();

    // The palette and the frame.
    let palette: Vec<[f32; 4]> = vec![
        [0.0, 0.0, 0.0, 1.0],
        [0.2, 0.2, 0.2, 1.0],
        [0.1, 0.07, 0.04, 1.0],
        [0.03, 0.4, 0.03, 1.0],
    ];
    let palette_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("palette"),
        contents: bytemuck::cast_slice(&palette),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("frame"),
        size: size_of::<FrameUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("frame layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(size_of::<FrameUniform>() as u64),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(16),
                },
                count: None,
            },
        ],
    });
    let frame_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("frame"),
        layout: &frame_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: palette_buffer.as_entire_binding(),
            },
        ],
    });

    let aspect = WIDTH as f32 / HEIGHT as f32;
    let view_projection = camera.view_projection(aspect);
    let frustum = Frustum::from_view_projection(view_projection);
    let frame = FrameUniform {
        view_projection: view_projection.to_cols_array_2d(),
        fog_color: [0.5, 0.6, 0.8, 1.0],
        fog_range: [1.0e15, 2.0e15, 0.3, 0.0],
        sun_direction: [0.4, 0.8, 0.3, 0.0],
        avatars: [[0.0; 4]; 16],
    };
    queue.write_buffer(&frame_buffer, 0, bytemuck::bytes_of(&frame));
    let target = target(&device);
    let timing = timing(&device);

    // The camera is at the origin of this scene, so the places relative to it are the corners.
    let camera_world = [0.0f32; 3];
    let place = |corner: [f32; 4]| Placement {
        offset: [
            corner[0] - camera_world[0],
            corner[1] - camera_world[1],
            corner[2] - camera_world[2],
            corner[3],
        ],
        rotation: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
    };
    let visible_on_cpu = |corner: [f32; 4]| {
        let offset = Vec3::new(corner[0], corner[1], corner[2]);
        frustum.intersects_sphere(
            offset + Vec3::splat(BRICK_CENTER_OFFSET * corner[3]),
            BRICK_BOUNDING_RADIUS * corner[3],
        )
    };

    let frames = 12;
    let mut results: Vec<(String, Result)> = Vec::new();

    // ---- instanced: a vertex buffer per node, a dynamic offset for its place.
    {
        let original = include_str!("../../../../engine/renderer/src/voxel.wgsl");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("original"),
            source: wgpu::ShaderSource::Wgsl(original.into()),
        });
        let alignment = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        let stride = 64u64.next_multiple_of(alignment);
        let placement_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("placement layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(64),
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("layout"),
            bind_group_layouts: &[Some(&frame_layout), Some(&placement_layout)],
            immediate_size: 0,
        });
        let pipeline = make_pipeline(
            &device,
            &layout,
            &shader,
            "vertex_main",
            Some(wgpu::VertexBufferLayout {
                array_stride: 8,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0 => Uint32, 1 => Uint32],
            }),
        );
        let flat: Vec<u32> = quads.iter().flat_map(|quad| *quad).collect();
        let buffers: Vec<wgpu::Buffer> = (0..nodes)
            .map(|_| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("node"),
                    contents: bytemuck::cast_slice(&flat),
                    usage: wgpu::BufferUsages::VERTEX,
                })
            })
            .collect();
        let placement_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("placements"),
            size: stride * nodes as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let placement_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("placements"),
            layout: &placement_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &placement_buffer,
                    offset: 0,
                    size: NonZeroU64::new(64),
                }),
            }],
        });
        let mut total = Result {
            cpu_ms: 0.0,
            gpu_ms: 0.0,
            wall_ms: 0.0,
            drawn: 0,
        };
        for frame_number in 0..frames + 2 {
            let start = Instant::now();
            // The renderer culls on the CPU, then writes the place of each node that is left, then draws them.
            let mut visible = Vec::new();
            for (index, corner) in scene.corners.iter().enumerate() {
                if visible_on_cpu(*corner) {
                    visible.push(index);
                }
            }
            let mut bytes = vec![0u8; stride as usize * visible.len().max(1)];
            for (slot, &index) in visible.iter().enumerate() {
                let placement = place(scene.corners[index]);
                bytes[slot * stride as usize..slot * stride as usize + 64]
                    .copy_from_slice(bytemuck::bytes_of(&placement));
            }
            queue.write_buffer(&placement_buffer, 0, &bytes);
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            {
                let mut pass = begin_render_pass(&mut encoder, &target, &timing, 0, 1);
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &frame_group, &[]);
                for (slot, &index) in visible.iter().enumerate() {
                    pass.set_bind_group(1, &placement_group, &[(slot as u64 * stride) as u32]);
                    pass.set_vertex_buffer(0, buffers[index].slice(..));
                    pass.draw(0..6, 0..quad_count as u32);
                }
            }
            encoder.resolve_query_set(&timing.query_set, 0..2, &timing.resolve, 0);
            encoder.copy_buffer_to_buffer(&timing.resolve, 0, &timing.read, 0, 16);
            queue.submit([encoder.finish()]);
            let cpu = start.elapsed();
            let stamps = read_timestamps(&device, &timing);
            let wall = start.elapsed();
            if frame_number >= 2 {
                total.cpu_ms += cpu.as_secs_f64() * 1000.0;
                total.gpu_ms += (stamps[1] - stamps[0]) as f64 * period / 1.0e6;
                total.wall_ms += wall.as_secs_f64() * 1000.0;
                total.drawn = visible.len();
            }
        }
        if nodes <= 1_000 {
            let (hash, drawn) = picture(&device, &queue, &target.color_texture);
            eprintln!(
                "GPU {label} picture after instanced: hash {hash:016x}, {drawn} pixels drawn"
            );
        }
        results.push(("instanced".to_string(), total));
    }

    // ---- the arena: all the quads in one buffer.
    let pulled_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("pulled"),
        source: wgpu::ShaderSource::Wgsl(PULLED_SHADER.into()),
    });
    let arena_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("arena layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(8),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(64),
                },
                count: None,
            },
        ],
    });
    let pulled_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("pulled layout"),
        bind_group_layouts: &[Some(&frame_layout), Some(&arena_layout)],
        immediate_size: 0,
    });
    let pulled_pipeline = make_pipeline(
        &device,
        &pulled_layout,
        &pulled_shader,
        "vertex_pulled",
        None,
    );
    let arena_words: Vec<u32> = (0..nodes)
        .flat_map(|_| quads.iter().flat_map(|quad| *quad))
        .collect();
    let arena = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("arena"),
        contents: bytemuck::cast_slice(&arena_words),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let placements_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("places"),
        size: 64 * nodes as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let arena_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("arena"),
        layout: &arena_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: arena.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: placements_buffer.as_entire_binding(),
            },
        ],
    });
    let draws_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("draws"),
        size: 16 * nodes as u64,
        usage: wgpu::BufferUsages::INDIRECT
            | wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    // ---- arena + indirect, the CPU culling and writing the arguments.
    {
        let mut total = Result {
            cpu_ms: 0.0,
            gpu_ms: 0.0,
            wall_ms: 0.0,
            drawn: 0,
        };
        for frame_number in 0..frames + 2 {
            let start = Instant::now();
            let mut places: Vec<Placement> = Vec::new();
            let mut draws: Vec<DrawArgs> = Vec::new();
            for (index, corner) in scene.corners.iter().enumerate() {
                if visible_on_cpu(*corner) {
                    draws.push(DrawArgs {
                        vertex_count: quad_count as u32 * 6,
                        instance_count: 1,
                        first_vertex: (index * quad_count) as u32 * 6,
                        first_instance: places.len() as u32,
                    });
                    places.push(place(*corner));
                }
            }
            queue.write_buffer(&placements_buffer, 0, bytemuck::cast_slice(&places));
            queue.write_buffer(&draws_buffer, 0, bytemuck::cast_slice(&draws));
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            {
                let mut pass = begin_render_pass(&mut encoder, &target, &timing, 0, 1);
                pass.set_pipeline(&pulled_pipeline);
                pass.set_bind_group(0, &frame_group, &[]);
                pass.set_bind_group(1, &arena_group, &[]);
                pass.multi_draw_indirect(&draws_buffer, 0, draws.len() as u32);
            }
            encoder.resolve_query_set(&timing.query_set, 0..2, &timing.resolve, 0);
            encoder.copy_buffer_to_buffer(&timing.resolve, 0, &timing.read, 0, 16);
            queue.submit([encoder.finish()]);
            let cpu = start.elapsed();
            let stamps = read_timestamps(&device, &timing);
            let wall = start.elapsed();
            if frame_number >= 2 {
                total.cpu_ms += cpu.as_secs_f64() * 1000.0;
                total.gpu_ms += (stamps[1] - stamps[0]) as f64 * period / 1.0e6;
                total.wall_ms += wall.as_secs_f64() * 1000.0;
                total.drawn = draws.len();
            }
        }
        if nodes <= 1_000 {
            let (hash, drawn) = picture(&device, &queue, &target.color_texture);
            eprintln!("GPU {label} picture after arena: hash {hash:016x}, {drawn} pixels drawn");
        }
        results.push(("arena + indirect, CPU culled".to_string(), total));
    }

    // ---- arena + indirect, the GPU culling.
    {
        let cull_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cull"),
            source: wgpu::ShaderSource::Wgsl(CULL_SHADER.into()),
        });
        let cull_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cull layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(size_of::<CullUniform>() as u64),
                    },
                    count: None,
                },
                storage_entry(1, true, 32),
                storage_entry(2, false, 64),
                storage_entry(3, false, 16),
            ],
        });
        let cull_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cull pipeline layout"),
            bind_group_layouts: &[Some(&cull_layout)],
            immediate_size: 0,
        });
        let cull_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("cull"),
            layout: Some(&cull_pipeline_layout),
            module: &cull_shader,
            entry_point: Some("cull_main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let infos: Vec<NodeInfo> = scene
            .corners
            .iter()
            .enumerate()
            .map(|(index, corner)| NodeInfo {
                corner: *corner,
                first_quad: (index * quad_count) as u32,
                quad_count: quad_count as u32,
                pad: [0; 2],
            })
            .collect();
        let nodes_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("nodes"),
            contents: bytemuck::cast_slice(&infos),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let cull_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cull uniform"),
            size: size_of::<CullUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cull_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cull"),
            layout: &cull_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: cull_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: nodes_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: placements_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: draws_buffer.as_entire_binding(),
                },
            ],
        });
        let planes = frustum_planes(view_projection);
        let uniform = CullUniform {
            camera: [camera_world[0], camera_world[1], camera_world[2], 0.0],
            planes,
            count: nodes as u32,
            pad: [0; 3],
        };
        queue.write_buffer(&cull_buffer, 0, bytemuck::bytes_of(&uniform));
        let mut total = Result {
            cpu_ms: 0.0,
            gpu_ms: 0.0,
            wall_ms: 0.0,
            drawn: 0,
        };
        for frame_number in 0..frames + 2 {
            let start = Instant::now();
            queue.write_buffer(&cull_buffer, 0, bytemuck::bytes_of(&uniform));
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("cull"),
                    timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                        query_set: &timing.query_set,
                        beginning_of_pass_write_index: Some(2),
                        end_of_pass_write_index: Some(3),
                    }),
                });
                pass.set_pipeline(&cull_pipeline);
                pass.set_bind_group(0, &cull_group, &[]);
                pass.dispatch_workgroups((nodes as u32).div_ceil(64), 1, 1);
            }
            {
                let mut pass = begin_render_pass(&mut encoder, &target, &timing, 0, 1);
                pass.set_pipeline(&pulled_pipeline);
                pass.set_bind_group(0, &frame_group, &[]);
                pass.set_bind_group(1, &arena_group, &[]);
                pass.multi_draw_indirect(&draws_buffer, 0, nodes as u32);
            }
            encoder.resolve_query_set(&timing.query_set, 0..4, &timing.resolve, 0);
            encoder.copy_buffer_to_buffer(&timing.resolve, 0, &timing.read, 0, 32);
            queue.submit([encoder.finish()]);
            let cpu = start.elapsed();
            let stamps = read_timestamps(&device, &timing);
            let wall = start.elapsed();
            if frame_number >= 2 {
                total.cpu_ms += cpu.as_secs_f64() * 1000.0;
                total.gpu_ms +=
                    ((stamps[1] - stamps[0]) + (stamps[3] - stamps[2])) as f64 * period / 1.0e6;
                total.wall_ms += wall.as_secs_f64() * 1000.0;
                total.drawn = scene
                    .corners
                    .iter()
                    .filter(|corner| visible_on_cpu(**corner))
                    .count();
            }
        }
        if nodes <= 1_000 {
            let (hash, drawn) = picture(&device, &queue, &target.color_texture);
            eprintln!(
                "GPU {label} picture after gpu culled: hash {hash:016x}, {drawn} pixels drawn"
            );
        }
        results.push(("arena + indirect, GPU culled".to_string(), total));
    }

    for (name, total) in results {
        let each = f64::from(frames);
        eprintln!(
            "GPU {label} | {nodes} nodes of {quad_count} quads, {} in view | {name}: CPU {:.2} ms, GPU {:.2} ms, wall {:.2} ms",
            total.drawn,
            total.cpu_ms / each,
            total.gpu_ms / each,
            total.wall_ms / each,
        );
    }
}

fn storage_entry(binding: u32, read_only: bool, size: u64) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: NonZeroU64::new(size),
        },
        count: None,
    }
}

fn frustum_planes(view_projection: Mat4) -> [[f32; 4]; 6] {
    // Gribb and Hartmann, for a clip space with z from 0 to 1 (and reverse-Z: near is z = w, far is infinitely far).
    let rows = [
        view_projection.row(0),
        view_projection.row(1),
        view_projection.row(2),
        view_projection.row(3),
    ];
    let normalised = |plane: Vec4| {
        let length = plane.truncate().length();
        (plane / length).to_array()
    };
    // (No far plane: the projection has an infinite one.)
    [
        normalised(rows[3] + rows[0]),
        normalised(rows[3] - rows[0]),
        normalised(rows[3] + rows[1]),
        normalised(rows[3] - rows[1]),
        normalised(rows[3] - rows[2]),
        normalised(rows[3] - rows[2]),
    ]
}

fn begin_render_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    target: &'a Target,
    timing: &'a Timing,
    begin: u32,
    end: u32,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("bench"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &target.color,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: 0.1,
                    g: 0.2,
                    b: 0.4,
                    a: 1.0,
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &target.depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(0.0),
                store: wgpu::StoreOp::Discard,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: Some(wgpu::RenderPassTimestampWrites {
            query_set: &timing.query_set,
            beginning_of_pass_write_index: Some(begin),
            end_of_pass_write_index: Some(end),
        }),
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

fn make_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    vertex_entry: &str,
    vertex_buffer: Option<wgpu::VertexBufferLayout<'_>>,
) -> wgpu::RenderPipeline {
    let buffers: Vec<Option<wgpu::VertexBufferLayout<'_>>> =
        vertex_buffer.into_iter().map(Some).collect();
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(vertex_entry),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vertex_entry),
            compilation_options: Default::default(),
            buffers: &buffers,
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fragment_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR,
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
            format: DEPTH,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Greater),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

pub fn what_submitting_very_many_nodes_costs() {
    let quads = node_quads();
    eprintln!("GPU one node: {} quads", quads.len());
    let instance = renderer::new_instance();
    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::PRIMARY));
    let mut camera = Camera::new(WorldPosition::new(BrickCoordinate::new(0, 0, 0), [0.0; 3]));
    camera.vertical_field_of_view = 70.0f32.to_radians();
    camera.pitch = -0.35;
    for adapter in adapters {
        if matches!(adapter.get_info().device_type, wgpu::DeviceType::Cpu) {
            continue;
        }
        let name = describe(&adapter);
        for nodes in [1_000usize, 4_000, 16_000, 32_000] {
            run(&adapter, &name, &quads, &scene_in_view(nodes, 5), &camera);
        }
        for nodes in [16_000usize, 32_000] {
            run(
                &adapter,
                &format!("{name} all round"),
                &quads,
                &scene_all_round(nodes, 5),
                &camera,
            );
        }
    }
    let _ = GraphicsContext::all_physical_adapters;
}
