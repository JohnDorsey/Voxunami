//! The GPU's side of the shadow maps: the texture the cascades are drawn into, the sampler that compares against it, and the pipelines that draw the casters.
//!
//! The geometry (which cascades, where, and what is drawn into each) is in [`crate::shadow`]; this is only what the device needs. The pipelines are depth only: the same quads
//! as the pictures', drawn from the sun, keeping the greatest depth (depth grows toward the sun), and only the faces that look away from it.

use std::num::NonZeroU64;

use voxel_meshing::{PackedDetailedQuad, PackedQuad};

use crate::shadow::{CASCADE_RESOLUTION, MAP_LAYERS};
use crate::voxel_renderer::DEPTH_FORMAT;

/// The texture the cascades are drawn into, a layer for each, and a view of each layer to draw into and one of all of them to look up in.
pub(crate) struct ShadowTarget {
    pub(crate) array_view: wgpu::TextureView,
    pub(crate) layer_views: Vec<wgpu::TextureView>,
    /// Whether the maps are their full size. (Until a shadow is wanted the texture is a texel, so that the picture's bindings have something to bind.)
    pub(crate) full_size: bool,
    _texture: wgpu::Texture,
}

impl ShadowTarget {
    pub(crate) fn new(device: &wgpu::Device, full_size: bool) -> Self {
        let size = if full_size { CASCADE_RESOLUTION } else { 1 };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow maps"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: MAP_LAYERS as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let array_view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("shadow maps"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let layer_views = (0..MAP_LAYERS as u32)
            .map(|layer| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("shadow map"),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        Self {
            array_view,
            layer_views,
            full_size,
            _texture: texture,
        }
    }
}

/// The sampler the pictures look the maps up with: a comparison (a place is lit if it is not deeper than what the map holds), filtered between the four nearest texels, which
/// is the soft edge of a shadow.
pub(crate) fn comparison_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("shadow comparison"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        compare: Some(wgpu::CompareFunction::GreaterEqual),
        ..Default::default()
    })
}

/// The layout of what the shadow passes bind in the picture's first group: the frame's uniform, with an offset for each cascade's copy (its matrix in place of the camera's).
pub(crate) fn shadow_frame_layout(device: &wgpu::Device, frame_size: u64) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("shadow frame layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: NonZeroU64::new(frame_size),
            },
            count: None,
        }],
    })
}

/// The pipelines that draw casters into a map: one for whole-voxel quads and one for detailed ones. The sun's maps keep the backs of things and the sky's keep the fronts;
/// `culled` is the side that is not drawn.
pub(crate) struct ShadowPipelines {
    pub(crate) quads: wgpu::RenderPipeline,
    pub(crate) detailed: wgpu::RenderPipeline,
}

impl ShadowPipelines {
    pub(crate) fn new(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        frame_layout: &wgpu::BindGroupLayout,
        placement_layout: &wgpu::BindGroupLayout,
        culled: wgpu::Face,
    ) -> Self {
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow pipeline layout"),
            bind_group_layouts: &[Some(frame_layout), Some(placement_layout)],
            immediate_size: 0,
        });
        let make = |label: &str, entry_point: &str, vertex_buffer: wgpu::VertexBufferLayout<'_>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some(entry_point),
                    compilation_options: Default::default(),
                    buffers: &[Some(vertex_buffer)],
                },
                fragment: None,
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    // Seen from the sun the faces that look at it are the front ones; the sun's maps keep the backs (see the module `shadow`) and the sky's the fronts.
                    cull_mode: Some(culled),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    // Depth grows toward the sun, and the map keeps the greatest.
                    depth_compare: Some(wgpu::CompareFunction::Greater),
                    stencil: wgpu::StencilState::default(),
                    // What a face keeps in the map is eased away from the sun by how steeply its depth changes across a texel (a face nearly edge-on to the light
                    // changes it a lot, and a neighbouring texel's depth is then far from the one a place looks up), so a place on a face the sun reaches is never in
                    // the shadow of the one under it, and one beside a wall is in the wall's shadow from the wall's foot.
                    bias: wgpu::DepthBiasState {
                        constant: 0,
                        slope_scale: -crate::shadow::CASTER_SLOPE_BIAS,
                        clamp: 0.0,
                    },
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            quads: make(
                "shadow pipeline",
                "shadow_vertex_main",
                wgpu::VertexBufferLayout {
                    array_stride: size_of::<PackedQuad>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Uint32, 1 => Uint32],
                },
            ),
            detailed: make(
                "detailed shadow pipeline",
                "shadow_vertex_detailed",
                wgpu::VertexBufferLayout {
                    array_stride: size_of::<PackedDetailedQuad>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Uint32, 1 => Uint32, 2 => Uint32, 3 => Uint32],
                },
            ),
        }
    }
}
