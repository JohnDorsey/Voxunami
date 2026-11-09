//! The graphics card's side of the voxels that block the sun: the tables of the windows, the pool of slots, the uniform that says where each window is, and the variant of the voxel
//! shader that walks them (`contact_on.wgsl`). The books are kept by [`crate::occupancy_volume::Volume`]; this writes what it hands over.
//!
//! It is made when the first voxel that blocks the sun is told of, and not before: a renderer without such voxels draws with the shader that has no walk in it (a binding the
//! default fragment shader uses made two shadow tests flake on the integrated AMD GPU: `docs/specs/contact-trace-spec.md`, 4.4).

use std::num::NonZeroU64;

use bytemuck::{Pod, Zeroable};

use crate::occupancy_volume::{Changes, MAXIMUM_WINDOWS, SLOT_BYTES, WINDOW_ENTRIES};
use crate::voxel_renderer::{voxel_pipelines, voxel_shader};

/// Where a window is, as the shader has it (`Window` in `contact_on.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct WindowUniform {
    /// Where the window's origin is relative to the camera, in voxels.
    pub origin: [f32; 4],
    /// The columns of the matrix from the world's axes to the grid's.
    pub to_local: [[f32; 4]; 3],
    /// The brick coordinates of the origin brick modulo the width of a window (xyz), and where the window's table begins, in entries.
    pub brick: [u32; 4],
}

/// The windows, and what to draw of the walk (`Windows` in `contact_on.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct WindowsUniform {
    pub windows: [WindowUniform; MAXIMUM_WINDOWS],
    /// x: how many cells to search whatever the rule says (0: by the rule), y: 1 to draw the walk's answer and nothing else.
    pub debug: [u32; 4],
}

/// The slots the pool has room for from the start.
const FIRST_POOL_SLOTS: usize = 256;

pub(crate) struct ContactGpu {
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    windows: wgpu::Buffer,
    tables: wgpu::Buffer,
    pool: wgpu::Buffer,
    /// How many slots the pool has room for, and how many it may have.
    pool_slots: usize,
    maximum_slots: usize,
    bind_group: wgpu::BindGroup,
    /// The voxel shader with the walk, with and without the pictures, and the pipelines made with each: made when they are first wanted.
    shaders: [Option<wgpu::ShaderModule>; 2],
    pipelines: [Option<(wgpu::RenderPipeline, wgpu::RenderPipeline)>; 2],
}

fn storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: NonZeroU64::new(4),
        },
        count: None,
    }
}

impl ContactGpu {
    pub(crate) fn new(
        device: &wgpu::Device,
        frame_layout: &wgpu::BindGroupLayout,
        placement_layout: &wgpu::BindGroupLayout,
        maximum_slots: usize,
    ) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("contact layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(size_of::<WindowsUniform>() as u64),
                    },
                    count: None,
                },
                storage_entry(1),
                storage_entry(2),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("contact pipeline layout"),
            bind_group_layouts: &[Some(frame_layout), Some(placement_layout), Some(&layout)],
            immediate_size: 0,
        });
        let windows = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("contact windows"),
            size: size_of::<WindowsUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let tables = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("contact tables"),
            size: (MAXIMUM_WINDOWS * WINDOW_ENTRIES * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let pool_slots = FIRST_POOL_SLOTS.min(maximum_slots.max(1));
        let pool = Self::pool_buffer(device, pool_slots);
        let bind_group = Self::bind_group(device, &layout, &windows, &tables, &pool);
        Self {
            layout,
            pipeline_layout,
            windows,
            tables,
            pool,
            pool_slots,
            maximum_slots,
            bind_group,
            shaders: [None, None],
            pipelines: [None, None],
        }
    }

    fn pool_buffer(device: &wgpu::Device, slots: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("contact pool"),
            size: (slots * SLOT_BYTES) as u64,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        })
    }

    fn bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        windows: &wgpu::Buffer,
        tables: &wgpu::Buffer,
        pool: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("contact bind group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: windows.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: tables.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: pool.as_entire_binding(),
                },
            ],
        })
    }

    /// Writes what changed into the tables and the pool, growing the pool (by doubling, keeping what it holds) to hold `slots_wanted` slots.
    pub(crate) fn write(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        changes: Changes,
        slots_wanted: usize,
    ) {
        if slots_wanted > self.pool_slots {
            let slots = slots_wanted.next_power_of_two().min(self.maximum_slots);
            let grown = Self::pool_buffer(device, slots);
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("contact pool growth"),
            });
            encoder.copy_buffer_to_buffer(
                &self.pool,
                0,
                &grown,
                0,
                (self.pool_slots * SLOT_BYTES) as u64,
            );
            queue.submit([encoder.finish()]);
            self.pool = grown;
            self.pool_slots = slots;
            self.bind_group = Self::bind_group(
                device,
                &self.layout,
                &self.windows,
                &self.tables,
                &self.pool,
            );
        }
        for write in changes.tables {
            let first = write.window * WINDOW_ENTRIES + write.first;
            queue.write_buffer(
                &self.tables,
                (first * 4) as u64,
                bytemuck::cast_slice(&write.entries),
            );
        }
        // Slots that follow one another are one write.
        let mut slots = changes.slots.into_iter().peekable();
        while let Some(first) = slots.next() {
            let first_slot = first.slot;
            let mut words: Vec<u32> = first.words.to_vec();
            let mut last = first_slot;
            while let Some(next) = slots.next_if(|next| next.slot == last + 1) {
                words.extend_from_slice(&next.words[..]);
                last = next.slot;
            }
            queue.write_buffer(
                &self.pool,
                u64::from(first_slot) * SLOT_BYTES as u64,
                bytemuck::cast_slice(&words),
            );
        }
    }

    pub(crate) fn write_windows(&self, queue: &wgpu::Queue, uniform: &WindowsUniform) {
        queue.write_buffer(&self.windows, 0, bytemuck::bytes_of(uniform));
    }

    pub(crate) fn bind_group_ref(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }

    /// Makes the pipelines of the shader variant with the walk (with the pictures, or without), if they are not made yet.
    pub(crate) fn ensure_pipelines(
        &mut self,
        device: &wgpu::Device,
        color_format: wgpu::TextureFormat,
        with_pictures: bool,
    ) {
        let index = usize::from(with_pictures);
        if self.pipelines[index].is_some() {
            return;
        }
        let shader =
            self.shaders[index].get_or_insert_with(|| voxel_shader(device, with_pictures, true));
        self.pipelines[index] = Some(voxel_pipelines(
            device,
            &self.pipeline_layout,
            shader,
            color_format,
        ));
    }

    /// The pipelines of the variant with the walk; `ensure_pipelines` has made them.
    pub(crate) fn pipelines(
        &self,
        with_pictures: bool,
    ) -> &(wgpu::RenderPipeline, wgpu::RenderPipeline) {
        self.pipelines[usize::from(with_pictures)]
            .as_ref()
            .expect("the pipelines of the walk were made before they are used")
    }
}
