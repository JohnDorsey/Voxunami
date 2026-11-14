//! The graphics card's side of the voxels that block the sun: the tables of the windows, the pool of slots, the pool of partial slots, the shapes, the uniform that says where each window is, and the variant of the voxel
//! shader that walks them (`contact_on.wgsl`). The books are kept by [`crate::occupancy_volume::Volume`]; this writes what it hands over.
//!
//! It is made when the first voxel that blocks the sun is told of, and not before: a renderer without such voxels draws with the shader that has no walk in it (a binding the
//! default fragment shader uses made two shadow tests flake on the integrated AMD GPU: `docs/specs/contact-trace-spec.md`, 4.4).

use std::num::NonZeroU64;

use bytemuck::{Pod, Zeroable};

use crate::occupancy_volume::{
    Changes, MAXIMUM_WINDOWS, PARTIAL_SLOT_BYTES, SLOT_BYTES, WINDOW_ENTRIES,
};
use crate::shader_pack::{Relief, Shading};
use crate::sun_boxes::{MAXIMUM_SUN_BOXES, SunBoxUniform};
use crate::voxel_renderer::{SunWalk, voxel_pipelines, voxel_shader};

/// Where a window is, as the shader has it (`Window` in `contact_on.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct WindowUniform {
    /// Where the window's origin is relative to the camera, in voxels; and in the fourth place 1 if the window is traced and 0 if it is not.
    pub origin: [f32; 4],
    /// The columns of the matrix from the world's axes to the grid's.
    pub to_local: [[f32; 4]; 3],
    /// The brick coordinates of the origin brick modulo the width of a window (xyz), and where the window's table begins, in entries.
    pub brick: [u32; 4],
    /// The lowest and the highest corner of the box that holds the window's bricks that block the sun, in voxels from the window's origin, in the grid's own axes.
    pub bounds_low: [f32; 4],
    pub bounds_high: [f32; 4],
}

/// The windows, and what to draw of the walk (`Windows` in `contact_on.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct WindowsUniform {
    pub windows: [WindowUniform; MAXIMUM_WINDOWS],
    /// x: how many cells to search whatever the rule says (0: by the rule), y: 1 to draw the walk's answer and nothing else.
    pub debug: [u32; 4],
    /// x: how many of `boxes` are set.
    pub entities: [u32; 4],
    /// The boxes of the nearest entities, nearest first (`entities_on.wgsl`).
    pub boxes: [SunBoxUniform; MAXIMUM_SUN_BOXES],
}

/// The slots the pool has room for from the start, and the partial slots.
const FIRST_POOL_SLOTS: usize = 256;
const FIRST_PARTIAL_POOL_SLOTS: usize = 8;
/// The bytes of one shape: 512 bits, one for each eighth of a voxel.
const SHAPE_BYTES: usize = 64;

pub(crate) struct ContactGpu {
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    /// The same with the specular pictures as the fourth group, for the shaders that draw pictures.
    pipeline_layout_with_pictures: wgpu::PipelineLayout,
    windows: wgpu::Buffer,
    tables: wgpu::Buffer,
    pool: wgpu::Buffer,
    /// How many slots the pool has room for, and how many it may have.
    pool_slots: usize,
    maximum_slots: usize,
    /// The same of the pool of partial slots (64 KB each).
    partial_pool: wgpu::Buffer,
    partial_pool_slots: usize,
    maximum_partial_slots: usize,
    /// The cells of the shapes of the block table that stop the sun in part, 16 words each (`BlockTable::shape_cells`).
    shapes: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// The voxel shader with the walk, with and without the pictures, the walk through shapes, the walk through the windows of other grids and the test against the boxes of
    /// entities, and the pipelines made with each (the number of the kind is `usize::from(with_pictures) + 2 * usize::from(with_shapes) + 4 * usize::from(with_other_windows) +
    /// 8 * usize::from(with_entities)`): made when they are first wanted.
    shaders: [Option<wgpu::ShaderModule>; 16],
    /// The text of the shader pack's `pack_shade` the shaders are made with.
    lighting: String,
    /// How the shaders with pictures draw their relief.
    relief: Relief,
    pipelines: [Option<(wgpu::RenderPipeline, wgpu::RenderPipeline)>; 16],
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
        shading: Shading<'_>,
        device: &wgpu::Device,
        frame_layout: &wgpu::BindGroupLayout,
        placement_layout: &wgpu::BindGroupLayout,
        companion_layout: &wgpu::BindGroupLayout,
        maximum_slots: usize,
        maximum_partial_slots: usize,
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
                storage_entry(3),
                storage_entry(4),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("contact pipeline layout"),
            bind_group_layouts: &[Some(frame_layout), Some(placement_layout), Some(&layout)],
            immediate_size: 0,
        });
        let pipeline_layout_with_pictures =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("contact pipeline layout with pictures"),
                bind_group_layouts: &[
                    Some(frame_layout),
                    Some(placement_layout),
                    Some(&layout),
                    Some(companion_layout),
                ],
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
        let partial_pool_slots = FIRST_PARTIAL_POOL_SLOTS.min(maximum_partial_slots.max(1));
        let partial_pool = Self::partial_pool_buffer(device, partial_pool_slots);
        let shapes = Self::shapes_buffer(device, 1);
        let bind_group = Self::bind_group(
            device,
            &layout,
            &[&windows, &tables, &pool, &partial_pool, &shapes],
        );
        Self {
            layout,
            pipeline_layout,
            pipeline_layout_with_pictures,
            windows,
            tables,
            pool,
            pool_slots,
            maximum_slots,
            partial_pool,
            partial_pool_slots,
            maximum_partial_slots,
            shapes,
            bind_group,
            shaders: std::array::from_fn(|_| None),
            lighting: shading.lighting.to_owned(),
            relief: shading.relief,
            pipelines: std::array::from_fn(|_| None),
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

    fn partial_pool_buffer(device: &wgpu::Device, slots: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("contact partial pool"),
            size: (slots.max(1) * PARTIAL_SLOT_BYTES) as u64,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        })
    }

    fn shapes_buffer(device: &wgpu::Device, shapes: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("contact shapes"),
            size: (shapes.max(1) * SHAPE_BYTES) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// The bind group of the buffers, in the order of their bindings: the windows, the tables, the pool, the partial pool, the shapes.
    fn bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        buffers: &[&wgpu::Buffer; 5],
    ) -> wgpu::BindGroup {
        let entries: Vec<wgpu::BindGroupEntry<'_>> = buffers
            .iter()
            .enumerate()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect();
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("contact bind group"),
            layout,
            entries: &entries,
        })
    }

    fn remake_bind_group(&mut self, device: &wgpu::Device) {
        self.bind_group = Self::bind_group(
            device,
            &self.layout,
            &[
                &self.windows,
                &self.tables,
                &self.pool,
                &self.partial_pool,
                &self.shapes,
            ],
        );
    }

    /// Writes the cells of the shapes (see `BlockTable::shape_cells`): the shader reads them by their place in the list.
    pub(crate) fn set_shapes(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        shapes: &[[u64; 8]],
    ) {
        self.shapes = Self::shapes_buffer(device, shapes.len());
        if !shapes.is_empty() {
            queue.write_buffer(&self.shapes, 0, bytemuck::cast_slice(shapes));
        }
        self.remake_bind_group(device);
    }

    /// Writes what changed into the tables and the pool, growing the pool (by doubling, keeping what it holds) to hold `slots_wanted` slots.
    pub(crate) fn write(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        changes: Changes,
        slots_wanted: usize,
        partial_slots_wanted: usize,
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
            self.remake_bind_group(device);
        }
        if partial_slots_wanted > self.partial_pool_slots {
            let slots = partial_slots_wanted
                .next_power_of_two()
                .min(self.maximum_partial_slots.max(1));
            let grown = Self::partial_pool_buffer(device, slots);
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("contact partial pool growth"),
            });
            encoder.copy_buffer_to_buffer(
                &self.partial_pool,
                0,
                &grown,
                0,
                (self.partial_pool_slots * PARTIAL_SLOT_BYTES) as u64,
            );
            queue.submit([encoder.finish()]);
            self.partial_pool = grown;
            self.partial_pool_slots = slots;
            self.remake_bind_group(device);
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

    /// The partial slots written, after `write`: they are not a part of it so that a slot of the pool and a partial slot are written by what holds them.
    pub(crate) fn write_partial_slots(
        &self,
        queue: &wgpu::Queue,
        slots: Vec<crate::occupancy_volume::PartialWrite>,
    ) {
        for write in slots {
            queue.write_buffer(
                &self.partial_pool,
                u64::from(write.slot) * PARTIAL_SLOT_BYTES as u64,
                bytemuck::cast_slice(&write.words[..]),
            );
        }
    }

    pub(crate) fn write_windows(&self, queue: &wgpu::Queue, uniform: &WindowsUniform) {
        queue.write_buffer(&self.windows, 0, bytemuck::bytes_of(uniform));
    }

    pub(crate) fn bind_group_ref(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }

    /// Makes the shaders of the variants with the walk from this text of the shader pack's `pack_shade` and with this relief from now on: the ones made before are forgotten.
    pub(crate) fn set_shading(&mut self, shading: Shading<'_>) {
        if shading.lighting != self.lighting || shading.relief != self.relief {
            self.lighting = shading.lighting.to_owned();
            self.relief = shading.relief;
            self.shaders = std::array::from_fn(|_| None);
            self.pipelines = std::array::from_fn(|_| None);
        }
    }

    /// Makes the pipelines of the shader variant with the walk (with the pictures or without, with the walk through shapes or without), if they are not made yet.
    pub(crate) fn ensure_pipelines(
        &mut self,
        device: &wgpu::Device,
        color_format: wgpu::TextureFormat,
        with_pictures: bool,
        walk: SunWalk,
    ) {
        let index = Self::kind(with_pictures, walk);
        if self.pipelines[index].is_some() {
            return;
        }
        let shader = self.shaders[index].get_or_insert_with(|| {
            voxel_shader(device, with_pictures, self.relief, walk, &self.lighting)
        });
        let layout = if with_pictures {
            &self.pipeline_layout_with_pictures
        } else {
            &self.pipeline_layout
        };
        self.pipelines[index] = Some(voxel_pipelines(device, layout, shader, color_format));
    }

    /// The number of the kind of shader: with the pictures or without, and what of the walk it has.
    fn kind(with_pictures: bool, walk: SunWalk) -> usize {
        let SunWalk::Voxels {
            shapes,
            other_windows,
            entities,
        } = walk
        else {
            panic!("a variant with the walk has a walk");
        };
        usize::from(with_pictures)
            + 2 * usize::from(shapes)
            + 4 * usize::from(other_windows)
            + 8 * usize::from(entities)
    }

    /// The pipelines of the variant with the walk; `ensure_pipelines` has made them.
    pub(crate) fn pipelines(
        &self,
        with_pictures: bool,
        walk: SunWalk,
    ) -> &(wgpu::RenderPipeline, wgpu::RenderPipeline) {
        self.pipelines[Self::kind(with_pictures, walk)]
            .as_ref()
            .expect("the pipelines of the walk were made before they are used")
    }
}
