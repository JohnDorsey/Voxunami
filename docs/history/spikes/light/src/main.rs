//! How much does putting light in the quad cost? See ../../../light-spike.md.
//!
//! Meshes a patch of Voxunami Cave terrain with and without a light channel, under several made-up
//! light fields and several numbers of brightness levels, and prints how many quads there are and
//! how long meshing takes. The mesher merges faces into one quad only when block *and* light match,
//! so every distinct light value is a place where a big quad has to be cut.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use base_blocks::{BaseBlocks, rolling_hills_generator_identifier};
use block_models::BlockTable;
use voxel_grid::{Brick, ChannelIndex, ChannelStorage, GridLayout};
use voxel_meshing::{BrickChannels, BrickNeighbourhood, mesh_brick};
use voxel_space::{BrickCoordinate, FaceDirection};

const SEED: u64 = 2024;
/// Bricks meshed: a square of this radius around the origin, from the lowest to the highest brick.
const RADIUS: i64 = 6;
const LOWEST_BRICK: i64 = 0;
const HIGHEST_BRICK: i64 = 7;
const EDGE: i64 = 32;
/// Distance between lamps, in voxels, in x and z.
const LAMP_SPACING: i64 = 14;
/// How far a smooth lamp reaches, in voxels.
const LAMP_REACH: i64 = 24;

/// The generated bricks around the meshed ones (one extra brick all round, for neighbours) and the
/// same voxels as flat arrays, which is easier for making up light.
struct Region {
    bricks: HashMap<BrickCoordinate, Brick>,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    /// Lowest voxel of the flat arrays.
    origin: [i64; 3],
    size: [usize; 3],
    solid: Vec<bool>,
}

impl Region {
    fn index(&self, voxel: [i64; 3]) -> Option<usize> {
        let mut local = [0usize; 3];
        for axis in 0..3 {
            let offset = voxel[axis] - self.origin[axis];
            if offset < 0 || offset as usize >= self.size[axis] {
                return None;
            }
            local[axis] = offset as usize;
        }
        Some((local[2] * self.size[1] + local[1]) * self.size[0] + local[0])
    }

    fn voxel_of(&self, index: usize) -> [i64; 3] {
        let x = index % self.size[0];
        let y = (index / self.size[0]) % self.size[1];
        let z = index / (self.size[0] * self.size[1]);
        [
            self.origin[0] + x as i64,
            self.origin[1] + y as i64,
            self.origin[2] + z as i64,
        ]
    }

    fn meshed_bricks(&self) -> impl Iterator<Item = BrickCoordinate> {
        (-RADIUS..=RADIUS).flat_map(|x| {
            (LOWEST_BRICK..=HIGHEST_BRICK)
                .flat_map(move |y| (-RADIUS..=RADIUS).map(move |z| BrickCoordinate::new(x, y, z)))
        })
    }
}

fn generate_region() -> (Region, GridLayout, BaseBlocks) {
    let blocks = BaseBlocks::new();
    let layout =
        GridLayout::new(vec![blocks.block_channel(), blocks.state_channel()]).expect("distinct");
    let block_channel = layout
        .channel_index(&base_blocks::block_channel_identifier())
        .unwrap();
    let state_channel = layout
        .channel_index(&base_blocks::state_channel_identifier())
        .unwrap();
    let generators = blocks.terrain_generators(SEED, block_channel);
    let id = generators
        .runtime_id(&rolling_hills_generator_identifier())
        .unwrap();
    let generator = generators.entry(id).unwrap().content().unwrap();

    let low = [-RADIUS - 1, LOWEST_BRICK - 1, -RADIUS - 1];
    let high = [RADIUS + 1, HIGHEST_BRICK + 1, RADIUS + 1];
    let mut bricks = HashMap::new();
    for x in low[0]..=high[0] {
        for y in low[1]..=high[1] {
            for z in low[2]..=high[2] {
                let coordinate = BrickCoordinate::new(x, y, z);
                bricks.insert(coordinate, generator.generate_brick(&layout, coordinate));
            }
        }
    }
    let size = [
        ((high[0] - low[0] + 1) * EDGE) as usize,
        ((high[1] - low[1] + 1) * EDGE) as usize,
        ((high[2] - low[2] + 1) * EDGE) as usize,
    ];
    let mut region = Region {
        bricks,
        block_channel,
        state_channel,
        origin: [low[0] * EDGE, low[1] * EDGE, low[2] * EDGE],
        size,
        solid: vec![false; size[0] * size[1] * size[2]],
    };
    let air = blocks.air;
    let mut solid = vec![false; region.solid.len()];
    for (coordinate, brick) in &region.bricks {
        let storage = brick.channel(block_channel);
        for local in voxel_space::LocalVoxelIndex::all() {
            let voxel = [
                coordinate.x() * EDGE + i64::from(local.x()),
                coordinate.y() * EDGE + i64::from(local.y()),
                coordinate.z() * EDGE + i64::from(local.z()),
            ];
            let index = region.index(voxel).unwrap();
            solid[index] = storage.get(local) != air;
        }
    }
    region.solid = solid;
    (region, layout, blocks)
}

fn hash(values: [i64; 4]) -> u64 {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    for value in values {
        state ^= value as u64;
        state = state.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        state ^= state >> 31;
    }
    state
}

/// One lamp for every `LAMP_SPACING` by `LAMP_SPACING` columns, hovering two voxels above the ground
/// at a random column of its cell.
fn lamps(region: &Region) -> Vec<[i64; 3]> {
    let mut lamps = Vec::new();
    let reach = RADIUS * EDGE;
    let mut cell_x = -reach;
    while cell_x < reach + EDGE {
        let mut cell_z = -reach;
        while cell_z < reach + EDGE {
            let random = hash([cell_x, cell_z, 1, 7]);
            let x = cell_x + (random % LAMP_SPACING as u64) as i64;
            let z = cell_z + ((random >> 20) % LAMP_SPACING as u64) as i64;
            let top = (region.origin[1]..region.origin[1] + region.size[1] as i64)
                .rev()
                .find(|&y| region.index([x, y, z]).is_some_and(|index| region.solid[index]));
            if let Some(top) = top
                && let Some(_) = region.index([x, top + 2, z])
            {
                lamps.push([x, top + 2, z]);
            }
            cell_z += LAMP_SPACING;
        }
        cell_x += LAMP_SPACING;
    }
    lamps
}

/// A light field: 0 to 255 in every voxel (solid voxels are not looked at).
struct Field {
    name: &'static str,
    note: &'static str,
    light: Vec<u8>,
}

fn uniform_field(region: &Region) -> Field {
    Field {
        name: "uniform",
        note: "every voxel 255 (a lit world, or no lighting at all)",
        light: vec![255; region.solid.len()],
    }
}

/// Minecraft-like: lamps of level 15 flooding through air, one level lost per voxel, shown as 17 per level.
fn flood_field(region: &Region, lamps: &[[i64; 3]]) -> Field {
    let mut level = vec![0u8; region.solid.len()];
    let mut queue = VecDeque::new();
    for &lamp in lamps {
        let index = region.index(lamp).unwrap();
        level[index] = 15;
        queue.push_back(index);
    }
    while let Some(index) = queue.pop_front() {
        let here = level[index];
        if here <= 1 {
            continue;
        }
        let voxel = region.voxel_of(index);
        for face in FaceDirection::ALL {
            let step = face.offset();
            let next = [voxel[0] + step[0], voxel[1] + step[1], voxel[2] + step[2]];
            if let Some(next) = region.index(next)
                && !region.solid[next]
                && level[next] < here - 1
            {
                level[next] = here - 1;
                queue.push_back(next);
            }
        }
    }
    Field {
        name: "flood16",
        note: "lamps flooding 15 levels through air, Minecraft style (x17)",
        light: level.into_iter().map(|level| level * 17).collect(),
    }
}

/// Smooth: every lamp lights the air within `LAMP_REACH` voxels, fading linearly with the straight-line
/// distance, in 256 levels.
fn smooth_field(region: &Region, lamps: &[[i64; 3]]) -> Field {
    let mut light = vec![0u8; region.solid.len()];
    let reach = LAMP_REACH;
    for &lamp in lamps {
        for dz in -reach..=reach {
            for dy in -reach..=reach {
                for dx in -reach..=reach {
                    let squared = dx * dx + dy * dy + dz * dz;
                    if squared > reach * reach {
                        continue;
                    }
                    let Some(index) = region.index([lamp[0] + dx, lamp[1] + dy, lamp[2] + dz])
                    else {
                        continue;
                    };
                    if region.solid[index] {
                        continue;
                    }
                    let distance = (squared as f32).sqrt();
                    let value = (255.0 * (1.0 - distance / reach as f32)).round() as u8;
                    light[index] = light[index].max(value);
                }
            }
        }
    }
    Field {
        name: "smooth256",
        note: "lamps fading linearly over 24 voxels, 256 levels, no occlusion",
        light,
    }
}

/// A slow gradient across the whole world: one more level every 4 voxels along x + z. The kind of
/// light a sun-angle or fog tint would give.
fn gradient_field(region: &Region) -> Field {
    let light = (0..region.solid.len())
        .map(|index| {
            let voxel = region.voxel_of(index);
            (((voxel[0] + voxel[2]) >> 2) & 255) as u8
        })
        .collect();
    Field {
        name: "gradient",
        note: "one level more every 4 voxels along x + z (wraps at 256)",
        light,
    }
}

/// The worst case: unrelated light in every voxel.
fn noise_field(region: &Region) -> Field {
    let light = (0..region.solid.len())
        .map(|index| {
            let [x, y, z] = region.voxel_of(index);
            hash([x, y, z, 0]) as u8
        })
        .collect();
    Field {
        name: "noise",
        note: "a different random level in nearly every voxel (worst case)",
        light,
    }
}

/// `value` rounded down into one of `bands` evenly spaced brightnesses (255 stays 255).
fn quantise(value: u8, bands: u32) -> u8 {
    if bands >= 256 {
        return value;
    }
    let band = u32::from(value) * bands / 256;
    (band * 255 / (bands - 1)) as u8
}

fn light_channels(region: &Region, field: &Field, bands: u32) -> HashMap<BrickCoordinate, ChannelStorage> {
    region
        .bricks
        .keys()
        .map(|&coordinate| {
            let mut values = vec![0u32; voxel_space::BRICK_VOXEL_COUNT];
            for local in voxel_space::LocalVoxelIndex::all() {
                let voxel = [
                    coordinate.x() * EDGE + i64::from(local.x()),
                    coordinate.y() * EDGE + i64::from(local.y()),
                    coordinate.z() * EDGE + i64::from(local.z()),
                ];
                let index = region.index(voxel).unwrap();
                values[local.index()] = u32::from(quantise(field.light[index], bands));
            }
            (coordinate, ChannelStorage::from_values(&values))
        })
        .collect()
}

struct Measurement {
    quads: usize,
    meshed_bricks: usize,
    time: Duration,
}

fn measure(
    region: &Region,
    table: &BlockTable,
    light: Option<&HashMap<BrickCoordinate, ChannelStorage>>,
) -> Measurement {
    let channels_of = |coordinate: BrickCoordinate| {
        let brick = &region.bricks[&coordinate];
        BrickChannels {
            blocks: brick.channel(region.block_channel),
            states: Some(brick.channel(region.state_channel)),
            light: light.map(|light| &light[&coordinate]),
        }
    };
    let mut best = Duration::MAX;
    let mut quads = 0;
    let mut meshed_bricks = 0;
    for _ in 0..5 {
        let started = Instant::now();
        let mut total = 0;
        let mut bricks = 0;
        for coordinate in region.meshed_bricks() {
            let neighbours = std::array::from_fn(|face| {
                let step = FaceDirection::from_index(face as u8).unwrap().offset();
                Some(channels_of(BrickCoordinate::new(
                    coordinate.x() + step[0],
                    coordinate.y() + step[1],
                    coordinate.z() + step[2],
                )))
            });
            let mesh = mesh_brick(
                &BrickNeighbourhood {
                    center: channels_of(coordinate),
                    neighbours,
                },
                table,
            );
            let count = mesh.quads.len() + mesh.detailed.len();
            total += count;
            bricks += usize::from(count > 0);
        }
        best = best.min(started.elapsed());
        quads = total;
        meshed_bricks = bricks;
    }
    Measurement {
        quads,
        meshed_bricks,
        time: best,
    }
}

fn main() {
    let (region, _layout, blocks) = generate_region();
    let table = blocks.block_table();
    let lamps = lamps(&region);
    println!(
        "region: {} x {} bricks, {} generated, {} lamps (seed {SEED})\n",
        2 * RADIUS + 1,
        HIGHEST_BRICK - LOWEST_BRICK + 1,
        region.bricks.len(),
        lamps.len()
    );

    let baseline = measure(&region, &table, None);
    println!(
        "baseline, no light channel: {} quads in {} surface bricks, {:.1} ms\n",
        baseline.quads,
        baseline.meshed_bricks,
        baseline.time.as_secs_f64() * 1000.0
    );

    let fields = [
        uniform_field(&region),
        flood_field(&region, &lamps),
        smooth_field(&region, &lamps),
        gradient_field(&region),
        noise_field(&region),
    ];
    let bands_list = [256u32, 64, 32, 16, 8, 4];
    println!("| field | bands | quads | x baseline | quads/brick | ms | x baseline time |");
    println!("|---|---|---|---|---|---|---|");
    for field in &fields {
        for bands in bands_list {
            let light = light_channels(&region, field, bands);
            let result = measure(&region, &table, Some(&light));
            println!(
                "| {} | {} | {} | {:.2} | {:.0} | {:.1} | {:.2} |",
                field.name,
                bands,
                result.quads,
                result.quads as f64 / baseline.quads as f64,
                result.quads as f64 / result.meshed_bricks.max(1) as f64,
                result.time.as_secs_f64() * 1000.0,
                result.time.as_secs_f64() / baseline.time.as_secs_f64(),
            );
        }
    }
    println!();
    for field in &fields {
        println!("- {}: {}", field.name, field.note);
    }

    // What the alternatives would cost in memory for the same bricks.
    let quad_bytes = 8;
    println!(
        "\nbaseline quads: {} KiB at {quad_bytes} bytes each; a light texture of 32x32x32 bytes for each of the {} surface bricks: {} KiB",
        baseline.quads * quad_bytes / 1024,
        baseline.meshed_bricks,
        baseline.meshed_bricks * 32 * 32 * 32 / 1024
    );
}
