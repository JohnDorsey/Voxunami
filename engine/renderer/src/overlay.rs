//! Flat rectangles over the finished picture, in pixels of the screen: what a user interface is drawn with. Text is made of
//! rectangles too (one for each run of lit pixels of a letter), so every edge falls on a pixel boundary and a picture
//! of it is the same on every GPU.

use std::num::NonZeroU64;

use crate::voxel_renderer::{DEPTH_FORMAT, srgb_to_linear};

/// A rectangle to draw: pixels from the top left of the picture, and a colour with alpha (sRGB, straight alpha).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub color: [u8; 4],
}

/// What the shader reads for each rectangle.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    rect: [f32; 4],
    color: [f32; 4],
}

const INSTANCE_BYTES: u64 = std::mem::size_of::<Instance>() as u64;

impl From<&OverlayRect> for Instance {
    fn from(rect: &OverlayRect) -> Self {
        let [red, green, blue, alpha] = rect.color;
        Self {
            rect: [
                rect.x as f32,
                rect.y as f32,
                rect.width as f32,
                rect.height as f32,
            ],
            color: [
                srgb_to_linear(f32::from(red) / 255.0),
                srgb_to_linear(f32::from(green) / 255.0),
                srgb_to_linear(f32::from(blue) / 255.0),
                f32::from(alpha) / 255.0,
            ],
        }
    }
}

#[derive(Debug)]
pub(crate) struct Overlay {
    pipeline: wgpu::RenderPipeline,
    screen: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    instances: Vec<Instance>,
    buffer: Option<wgpu::Buffer>,
}

impl Overlay {
    pub(crate) fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(16),
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("overlay pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("overlay.wgsl").into()),
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: INSTANCE_BYTES,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attributes,
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let screen = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay screen size"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("overlay bind group"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen.as_entire_binding(),
            }],
        });
        Self {
            pipeline,
            screen,
            bind_group,
            instances: Vec::new(),
            buffer: None,
        }
    }

    /// What to draw from now on, in order, later over earlier. Empty draws nothing. A new vertex buffer is made, filled as it is
    /// created, whenever the rectangles change: they are small, and a buffer that is written after it was made (zeroed, then
    /// written, then read by the first frame) was seen to lose a few rectangles of that first frame on one adapter.
    pub(crate) fn set(&mut self, device: &wgpu::Device, rects: &[OverlayRect]) {
        let instances: Vec<Instance> = rects
            .iter()
            .filter(|rect| rect.width > 0 && rect.height > 0)
            .map(Instance::from)
            .collect();
        if instances == self.instances && (self.buffer.is_some() || instances.is_empty()) {
            return;
        }
        self.buffer = (!instances.is_empty()).then(|| {
            let size = instances.len() as u64 * INSTANCE_BYTES;
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("overlay rectangles"),
                size,
                usage: wgpu::BufferUsages::VERTEX,
                mapped_at_creation: true,
            });
            buffer
                .slice(..)
                .get_mapped_range_mut()
                .expect("a buffer made mapped can be written")
                .copy_from_slice(bytemuck::cast_slice(&instances));
            buffer.unmap();
            buffer
        });
        self.instances = instances;
    }

    pub(crate) fn count(&self) -> usize {
        self.instances.len()
    }

    /// Tells the shader how big the picture is; call before the pass that draws the rectangles.
    pub(crate) fn set_screen_size(&self, queue: &wgpu::Queue, size: (u32, u32)) {
        let contents: [f32; 4] = [size.0 as f32, size.1 as f32, 0.0, 0.0];
        queue.write_buffer(&self.screen, 0, bytemuck::bytes_of(&contents));
    }

    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(buffer) = &self.buffer else {
            return;
        };
        if self.instances.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(
            0,
            buffer.slice(..self.instances.len() as u64 * INSTANCE_BYTES),
        );
        pass.draw(0..6, 0..self.instances.len() as u32);
    }
}
