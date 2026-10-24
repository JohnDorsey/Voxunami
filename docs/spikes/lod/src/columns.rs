//! Nodes that are columns of a height field: separable batch noise (the same bits as the per-column sample), a mesh made straight from the heights, and what each
//! costs against the cells-and-greedy-mesher way, with many threads, and for a whole view.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use coarse_cells::{CoarseCell, CoarseNode};
use region_octree::BrickCube;
use terrain::{
    Dimensionality, FieldTerrainGenerator, FractalNoise, Heightfield, TerrainGenerator,
    TerrainMaterials, lattice_value, mix,
};
use voxel_grid::test_support::StandardTestLayout;
use voxel_meshing::{Quad, mesh_coarse_node, quad_axes};
use voxel_space::{
    BrickCoordinate, FaceDirection, LevelOfDetail, LocalVoxelIndex, VoxelCoordinate,
};

pub(crate) const MATERIALS: TerrainMaterials = TerrainMaterials {
    air: 0,
    stone: 1,
    dirt: 2,
    grass: 3,
};
const EDGE: usize = 32;
const SEED: u64 = 2024;
pub(crate) const BASE: f64 = 64.0;
const SCALE: f64 = 1.0 / 4_503_599_627_370_496.0;

/// The hills of the rolling-hills terrain: octaves of 128 voxels down, 6 of them, 40 high, half as strong each.
pub(crate) fn hills() -> FractalNoise {
    FractalNoise::standard(SEED, Dimensionality::Planar, 7, 6, 40.0, 0.5)
}

fn octave_list() -> Vec<(u8, f64)> {
    let mut list = Vec::new();
    let mut amplitude = 40.0;
    for step in 0..6u8 {
        list.push((7 - step, amplitude));
        amplitude *= 0.5;
    }
    list
}

fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// The hills at `count` x `count` columns, the first at voxel (x0, z0) and `step` voxels apart, evaluated octave by octave: one lattice, one blend along x
/// for each row of it, one blend along z for each column. The same operations on the same numbers in the same order as `FractalNoise::sample`, so the same bits.
pub(crate) fn batch_hills(x0: i64, z0: i64, step: i64, count: usize, footprint: u8) -> Vec<f64> {
    let mut total = vec![0.0f64; count * count];
    for (index, &(wavelength_log2, amplitude)) in octave_list().iter().enumerate() {
        if wavelength_log2 < footprint {
            continue;
        }
        let octave_seed = mix(SEED ^ (index as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93));
        let size = 1i64 << wavelength_log2;
        let along = |origin: i64| {
            let mut cells = [0i64; EDGE];
            let mut weights = [0.0f64; EDGE];
            for i in 0..count {
                let coordinate = origin + i as i64 * step;
                cells[i] = coordinate >> wavelength_log2;
                weights[i] = fade(((coordinate & (size - 1)) as f64 + 0.5) / size as f64);
            }
            (cells, weights)
        };
        let (cell_x, weight_x) = along(x0);
        let (cell_z, weight_z) = along(z0);
        let (first_x, last_x) = (cell_x[0], cell_x[count - 1] + 1);
        let (first_z, last_z) = (cell_z[0], cell_z[count - 1] + 1);
        let width = (last_x - first_x + 1) as usize;
        let depth = (last_z - first_z + 1) as usize;
        let base = mix(octave_seed);
        let mut lattice = [0.0f64; 34 * 34];
        for ix in 0..width {
            let hash_x = mix(mix(base ^ (first_x + ix as i64) as u64) ^ 0);
            for iz in 0..depth {
                let hash = mix(hash_x ^ (first_z + iz as i64) as u64);
                lattice[iz * width + ix] = (hash >> 11) as f64 * SCALE - 1.0;
            }
        }
        let mut along_x = [0.0f64; 34 * EDGE];
        for iz in 0..depth {
            for i in 0..count {
                let at = iz * width + (cell_x[i] - first_x) as usize;
                along_x[iz * count + i] = lerp(lattice[at], lattice[at + 1], weight_x[i]);
            }
        }
        for j in 0..count {
            let row = (cell_z[j] - first_z) as usize;
            for i in 0..count {
                total[j * count + i] += amplitude
                    * lerp(
                        along_x[row * count + i],
                        along_x[(row + 1) * count + i],
                        weight_z[j],
                    );
            }
        }
    }
    total
}

/// The top drawn cell of a column of cells of `size` voxels whose node begins at voxel row `origin_y`, for a surface at `height`: -1 for none, and 32 or more
/// for a surface over the node. A cell is drawn when half of it or more is ground.
fn top_cell(height: f64, origin_y: i64, size: i64) -> i64 {
    let first_air = (height - 0.5).ceil() as i64;
    (first_air - origin_y - size / 2).div_euclid(size)
}

struct Columns {
    /// The top drawn cell of each column, -1 for none, 31 for a column that goes on over the node; indexed `z * 32 + x`.
    top: [i8; EDGE * EDGE],
    /// Whether the surface is in the node in this column (so the top cell is grass and has a top face).
    surface_inside: [bool; EDGE * EDGE],
}

fn columns_from_heights(heights: &[f64], origin_y: i64, size: i64) -> Columns {
    let mut columns = Columns {
        top: [-1; EDGE * EDGE],
        surface_inside: [false; EDGE * EDGE],
    };
    for (index, &height) in heights.iter().enumerate() {
        let top = top_cell(height, origin_y, size);
        columns.surface_inside[index] = (0..EDGE as i64).contains(&top);
        columns.top[index] = top.clamp(-1, 31) as i8;
    }
    columns
}

const STONE_KEY: (u32, u8) = (MATERIALS.stone, 255);
const GRASS_KEY: (u32, u8) = (MATERIALS.grass, 255);

/// The mesh of the columns, made straight from them: tops merged along x then z where equal, walls merged along their length where equal. Air beside the node.
fn mesh_columns(columns: &Columns) -> Vec<Quad> {
    let mut quads = Vec::new();
    let at = |x: usize, z: usize| z * EDGE + x;
    // Tops (+Y): the quads span z (u) then x (v) for a face along y. A row of equal columns along z, and rows alike on top of one another.
    let mut done = [false; EDGE * EDGE];
    for x in 0..EDGE {
        let mut z = 0;
        while z < EDGE {
            let index = at(x, z);
            if done[index] || !columns.surface_inside[index] {
                z += 1;
                continue;
            }
            let height = columns.top[index];
            let mut width = 1;
            while z + width < EDGE
                && !done[at(x, z + width)]
                && columns.surface_inside[at(x, z + width)]
                && columns.top[at(x, z + width)] == height
            {
                width += 1;
            }
            let mut rows = 1;
            'grow: while x + rows < EDGE {
                for step in 0..width {
                    let other = at(x + rows, z + step);
                    if done[other] || !columns.surface_inside[other] || columns.top[other] != height
                    {
                        break 'grow;
                    }
                }
                rows += 1;
            }
            for row in 0..rows {
                for step in 0..width {
                    done[at(x + row, z + step)] = true;
                }
            }
            quads.push(Quad {
                origin: [x as u8, height as u8, z as u8],
                width: width as u8,
                height: rows as u8,
                face: FaceDirection::PositiveY,
                material: GRASS_KEY.0,
                light: GRASS_KEY.1,
            });
            z += width;
        }
    }
    // Walls. For a face along x, u is y and v is z; along z, u is x and v is y. A wall of a column is its drawn cells above the column beside it: stone, and the
    // grass cell on top if the surface is in the node. Runs of columns along the wall with the same wall are one rectangle.
    for face in [
        FaceDirection::PositiveX,
        FaceDirection::NegativeX,
        FaceDirection::PositiveZ,
        FaceDirection::NegativeZ,
    ] {
        let axis = face.axis();
        let step = face.offset();
        let (along, across) = if axis == 0 { (2, 0) } else { (0, 2) };
        for slice in 0..EDGE {
            // The columns of this slice in order along the wall.
            let mut previous: Option<(i64, i64, bool)> = None;
            let mut run_start = 0usize;
            let mut flush = |previous: Option<(i64, i64, bool)>,
                             start: usize,
                             end: usize,
                             quads: &mut Vec<Quad>| {
                let Some((low, high, inside)) = previous else {
                    return;
                };
                let emit = |first: i64, last: i64, key: (u32, u8), quads: &mut Vec<Quad>| {
                    if last < first {
                        return;
                    }
                    let mut origin = [0u8; 3];
                    origin[axis] = slice as u8;
                    origin[along] = start as u8;
                    origin[1] = first as u8;
                    // u/v: along x faces u = y, v = z; along z faces u = x, v = y.
                    let (width, height) = if axis == 0 {
                        ((last - first + 1) as u8, (end - start) as u8)
                    } else {
                        ((end - start) as u8, (last - first + 1) as u8)
                    };
                    quads.push(Quad {
                        origin,
                        width,
                        height,
                        face,
                        material: key.0,
                        light: key.1,
                    });
                };
                if inside {
                    emit(low, high - 1, STONE_KEY, quads);
                    emit(high, high, GRASS_KEY, quads);
                } else {
                    emit(low, high, STONE_KEY, quads);
                }
            };
            for position in 0..EDGE {
                let (x, z) = if axis == 0 {
                    (slice, position)
                } else {
                    (position, slice)
                };
                let own = columns.top[at(x, z)] as i64;
                let beside = {
                    let nx = x as i64 + step[0];
                    let nz = z as i64 + step[2];
                    if (0..EDGE as i64).contains(&nx) && (0..EDGE as i64).contains(&nz) {
                        columns.top[at(nx as usize, nz as usize)] as i64
                    } else {
                        -1
                    }
                };
                let wall =
                    (own > beside).then_some((beside + 1, own, columns.surface_inside[at(x, z)]));
                if wall != previous {
                    flush(previous, run_start, position, &mut quads);
                    previous = wall;
                    run_start = position;
                }
            }
            flush(previous, run_start, EDGE, &mut quads);
            let _ = across;
        }
    }
    quads
}

/// The cells of a node from its columns: stone below the top, grass on it if the surface is in the node.
fn cells_from_columns(columns: &Columns) -> CoarseNode {
    let mut cells = vec![CoarseCell::empty(MATERIALS.air, 255); EDGE * EDGE * EDGE];
    for z in 0..EDGE {
        for x in 0..EDGE {
            let top = columns.top[z * EDGE + x] as i64;
            for k in 0..=top {
                let block = if k == top && columns.surface_inside[z * EDGE + x] {
                    MATERIALS.grass
                } else {
                    MATERIALS.stone
                };
                cells[LocalVoxelIndex::new(x as u8, k as u8, z as u8).index()] = CoarseCell {
                    block,
                    fill: 255,
                    light: 255,
                    exposure: 0,
                };
            }
        }
    }
    CoarseNode::from_cells(cells)
}

/// Every unit face of some quads: (position, face, material, light).
fn unit_faces(quads: &[Quad]) -> HashSet<([u8; 3], u8, u32, u8)> {
    let mut faces = HashSet::new();
    for quad in quads {
        let (u_axis, v_axis) = quad_axes(quad.face);
        for v in 0..quad.height {
            for u in 0..quad.width {
                let mut at = quad.origin;
                at[u_axis] += u;
                at[v_axis] += v;
                assert!(
                    faces.insert((at, quad.face.index(), quad.material, quad.light)),
                    "a face covered twice at {at:?} by {quad:?}"
                );
            }
        }
    }
    faces
}

fn cubes_around_the_surface(level: u8, count: i64) -> Vec<BrickCube> {
    let width = 1i64 << level;
    let size = 1i64 << level;
    let noise = hills();
    let mut cubes = Vec::new();
    for i in 0..count {
        for j in 0..count {
            let brick_x = (i * 3 + 1) * width;
            let brick_z = (j * 5 + 2) * width;
            let centre =
                VoxelCoordinate::new(brick_x * 32 + 16 * size, 0, brick_z * 32 + 16 * size);
            let height = BASE + noise.sample(centre, LevelOfDetail::new(level).unwrap());
            let brick_y = ((height / (32.0 * size as f64)).floor() as i64) * width;
            cubes.push(
                BrickCube::new(BrickCoordinate::new(brick_x, brick_y, brick_z), level).unwrap(),
            );
        }
    }
    cubes
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

pub fn batch_noise_has_the_same_bits_as_sampling_each_column() {
    let noise = hills();
    for level in 1..=7u8 {
        let detail = LevelOfDetail::new(level).unwrap();
        let size = 1i64 << level;
        for (x0, z0) in [
            (0i64, 0i64),
            (-5000, 777),
            (123_456, -99_999),
            (1 << 40, -(1 << 41)),
        ] {
            let (x0, z0) = (x0 & !(32 * size - 1), z0 & !(32 * size - 1));
            let batch = batch_hills(x0 + size / 2, z0 + size / 2, size, EDGE, level);
            for j in 0..EDGE {
                for i in 0..EDGE {
                    let position = VoxelCoordinate::new(
                        x0 + i as i64 * size + size / 2,
                        0,
                        z0 + j as i64 * size + size / 2,
                    );
                    let one = noise.sample(position, detail);
                    assert_eq!(
                        batch[j * EDGE + i].to_bits(),
                        one.to_bits(),
                        "level {level} column {i} {j} at {x0} {z0}"
                    );
                }
            }
        }
    }
}

pub fn the_mesh_made_from_the_columns_covers_the_faces_the_cells_and_the_greedy_mesher_give() {
    for level in 1..=5u8 {
        let size = 1i64 << level;
        let mut total_direct = 0usize;
        let mut total_greedy = 0usize;
        for cube in cubes_around_the_surface(level, 3) {
            let origin = cube.origin().origin();
            let heights: Vec<f64> = batch_hills(
                origin.x() + size / 2,
                origin.z() + size / 2,
                size,
                EDGE,
                level,
            )
            .into_iter()
            .map(|noise| BASE + noise)
            .collect();
            let columns = columns_from_heights(&heights, origin.y(), size);
            let node = cells_from_columns(&columns);
            let greedy = mesh_coarse_node(&node, &[None; 6]).quads;
            let direct = mesh_columns(&columns);
            // The greedy mesher with no neighbours also makes the faces of the bottom of the node and the stone tops of the columns that go on over it; they are
            // buried when the nodes below and above are there. The direct mesh leaves them out.
            let seen = |faces: HashSet<([u8; 3], u8, u32, u8)>| -> HashSet<([u8; 3], u8, u32, u8)> {
                faces
                    .into_iter()
                    .filter(|(_, face, material, _)| {
                        *face != FaceDirection::NegativeY.index()
                            && !(*face == FaceDirection::PositiveY.index()
                                && *material != MATERIALS.grass)
                    })
                    .collect()
            };
            assert_eq!(
                seen(unit_faces(&direct)),
                seen(unit_faces(&greedy)),
                "{cube:?}"
            );
            total_direct += direct.len();
            total_greedy += greedy.len();
        }
        eprintln!(
            "COLUMNS level {level}: {total_direct} quads made straight from the columns, {total_greedy} by the greedy mesher"
        );
    }
}

pub fn what_each_way_of_making_a_node_costs() {
    let fixture = StandardTestLayout::default();
    let generator = FieldTerrainGenerator::new(
        Arc::new(Heightfield::new(BASE, hills())),
        fixture.material,
        MATERIALS,
    );
    let noise = hills();
    for level in 1..=5u8 {
        let size = 1i64 << level;
        let detail = LevelOfDetail::new(level).unwrap();
        let cubes = cubes_around_the_surface(level, 3);
        let count = cubes.len() as u32;
        // 1: the generator as it is (a lattice of samples per cell, tight bounds).
        let start = Instant::now();
        let mut generated = Vec::new();
        for cube in &cubes {
            generated.push(generator.coarse_node(*cube, 255).unwrap());
        }
        let generator_time = start.elapsed() / count;
        // 2: one sample for each column, one at a time.
        let start = Instant::now();
        let mut sum = 0.0;
        for cube in &cubes {
            let origin = cube.origin().origin();
            for j in 0..EDGE as i64 {
                for i in 0..EDGE as i64 {
                    sum += noise.sample(
                        VoxelCoordinate::new(
                            origin.x() + i * size + size / 2,
                            0,
                            origin.z() + j * size + size / 2,
                        ),
                        detail,
                    );
                }
            }
        }
        let per_column_time = start.elapsed() / count;
        // 3: the batch.
        let start = Instant::now();
        let mut batches = Vec::new();
        for cube in &cubes {
            let origin = cube.origin().origin();
            batches.push(batch_hills(
                origin.x() + size / 2,
                origin.z() + size / 2,
                size,
                EDGE,
                level,
            ));
        }
        let batch_time = start.elapsed() / count;
        // 4: from heights to columns.
        let start = Instant::now();
        let mut all_columns = Vec::new();
        for (cube, batch) in cubes.iter().zip(&batches) {
            let heights: Vec<f64> = batch.iter().map(|noise| BASE + noise).collect();
            all_columns.push(columns_from_heights(
                &heights,
                cube.origin().origin().y(),
                size,
            ));
        }
        let columns_time = start.elapsed() / count;
        // 5: cells from the columns and the greedy mesher.
        let start = Instant::now();
        let mut greedy_quads = 0usize;
        for columns in &all_columns {
            let node = cells_from_columns(columns);
            greedy_quads += mesh_coarse_node(&node, &[None; 6]).quads.len();
        }
        let greedy_time = start.elapsed() / count;
        // 6: straight from the columns.
        let start = Instant::now();
        let mut direct_quads = 0usize;
        for columns in &all_columns {
            direct_quads += mesh_columns(columns).len();
        }
        let direct_time = start.elapsed() / count;
        // The generator's own cells against the columns': the share of cells drawn alike.
        let mut alike = 0usize;
        let mut cells = 0usize;
        for (node, columns) in generated.iter().zip(&all_columns) {
            let mine = cells_from_columns(columns);
            for index in 0..EDGE * EDGE * EDGE {
                let own = node.cell(LocalVoxelIndex::from_index(index).unwrap());
                let other = mine.cell(LocalVoxelIndex::from_index(index).unwrap());
                alike += usize::from(own.is_drawn() == other.is_drawn());
                cells += 1;
            }
        }
        eprintln!(
            "COSTS level {level}: generator {:.3} ms | per-column samples {:.1} us | batch {:.1} us | columns {:.1} us | cells+greedy mesh {:.3} ms ({} quads) | direct mesh {:.1} us ({} quads) | cells drawn alike {:.2}% ({sum:.0})",
            ms(generator_time),
            per_column_time.as_secs_f64() * 1e6,
            batch_time.as_secs_f64() * 1e6,
            columns_time.as_secs_f64() * 1e6,
            ms(greedy_time),
            greedy_quads / count as usize,
            direct_time.as_secs_f64() * 1e6,
            direct_quads / count as usize,
            100.0 * alike as f64 / cells as f64,
        );
    }
}

#[allow(dead_code)]
fn unused() -> f64 {
    lattice_value(1, 2, 3, 4)
}

pub fn how_many_nodes_a_second_with_many_threads() {
    // 12,000 nodes over levels 1 to 5 (the sort of mix a view of a few kilometres is made of), made whole: noise, columns, mesh.
    let mut jobs: Vec<(u8, BrickCube)> = Vec::new();
    for level in 1..=5u8 {
        for cube in cubes_around_the_surface(level, 49) {
            jobs.push((level, cube));
        }
    }
    let total_quads = std::sync::atomic::AtomicUsize::new(0);
    for threads in [1usize, 2, 4, 6, 12] {
        let start = Instant::now();
        std::thread::scope(|scope| {
            for t in 0..threads {
                let jobs = &jobs;
                let total_quads = &total_quads;
                scope.spawn(move || {
                    let mut quads = 0usize;
                    for (index, (level, cube)) in jobs.iter().enumerate() {
                        if index % threads != t {
                            continue;
                        }
                        let size = 1i64 << *level;
                        let origin = cube.origin().origin();
                        let batch = batch_hills(
                            origin.x() + size / 2,
                            origin.z() + size / 2,
                            size,
                            EDGE,
                            *level,
                        );
                        let heights: Vec<f64> = batch.iter().map(|noise| BASE + noise).collect();
                        let tops = tops_from_heights_fast(&heights, origin.y(), u32::from(*level));
                        let mut packed = Vec::with_capacity(2048);
                        mesh_tops_fast(&tops, &mut packed);
                        quads += packed.len();
                    }
                    total_quads.fetch_add(quads, std::sync::atomic::Ordering::Relaxed);
                });
            }
        });
        let time = start.elapsed();
        eprintln!(
            "SCALING {threads} threads: {} nodes in {:.1} ms = {:.0} nodes a second ({:.1} us a node, per thread {:.1} us)",
            jobs.len(),
            ms(time),
            jobs.len() as f64 / time.as_secs_f64(),
            time.as_secs_f64() * 1e6 / jobs.len() as f64,
            time.as_secs_f64() * 1e6 * threads as f64 / jobs.len() as f64,
        );
    }
    let _ = total_quads;
}

// ---- The same, faster: the experiment's second pass (appended to zz_columns.rs).

/// `top_cell` for a power-of-two cell, with no division and no call to `ceil`.
fn top_cell_fast(height: f64, origin_y: i64, level: u32) -> i64 {
    let x = height - 0.5;
    let truncated = x as i64;
    let first_air = truncated + i64::from((truncated as f64) < x);
    (first_air - origin_y - ((1i64 << level) >> 1)) >> level
}

/// The heights of the columns as a top cell each: -1 for none, 0 to 31, and `OVER` for a column that goes on over the node (no top face). One byte a column.
const OVER: i8 = 32;

pub(crate) fn tops_from_heights_fast(
    heights: &[f64],
    origin_y: i64,
    level: u32,
) -> [i8; EDGE * EDGE] {
    let mut tops = [-1i8; EDGE * EDGE];
    for (index, &height) in heights.iter().enumerate() {
        let top = top_cell_fast(height, origin_y, level);
        tops[index] = if top > 31 { OVER } else { top.max(-1) as i8 };
    }
    tops
}

/// The mesh of columns given as `tops`, made straight from them, packed.
pub(crate) fn mesh_tops_fast(tops: &[i8; EDGE * EDGE], quads: &mut Vec<voxel_meshing::PackedQuad>) {
    let top_of = |x: usize, z: usize| i32::from(tops[z * EDGE + x]);
    // Tops (+Y), merged along z and then x. The top cell of a column that goes on over the node (`OVER`) has no top face.
    let mut done = [0u32; EDGE];
    for x in 0..EDGE {
        let mut z = 0;
        while z < EDGE {
            let height = top_of(x, z);
            if (done[x] >> z) & 1 == 1 || !(0..32).contains(&height) {
                z += 1;
                continue;
            }
            let mut width = 1;
            while z + width < EDGE
                && (done[x] >> (z + width)) & 1 == 0
                && top_of(x, z + width) == height
            {
                width += 1;
            }
            let run_mask = (((1u64 << width) - 1) as u32) << z;
            let mut rows = 1;
            while x + rows < EDGE {
                let row = x + rows;
                if done[row] & run_mask != 0 {
                    break;
                }
                let mut same = true;
                for step in 0..width {
                    if top_of(row, z + step) != height {
                        same = false;
                        break;
                    }
                }
                if !same {
                    break;
                }
                rows += 1;
            }
            for row in 0..rows {
                done[x + row] |= run_mask;
            }
            quads.push(voxel_meshing::PackedQuad {
                geometry: x as u32
                    | (height as u32) << 5
                    | (z as u32) << 10
                    | ((width - 1) as u32) << 15
                    | ((rows - 1) as u32) << 20
                    | u32::from(FaceDirection::PositiveY.index()) << 25,
                material: MATERIALS.grass | 255 << 24,
            });
            z += width;
        }
    }
    // Walls. For each direction and slice, the columns along it in order; a run of columns with the same wall is one rectangle (two: the stone, and the grass cell
    // on top where the surface is in the node).
    for face in [
        FaceDirection::PositiveX,
        FaceDirection::NegativeX,
        FaceDirection::PositiveZ,
        FaceDirection::NegativeZ,
    ] {
        let axis = face.axis();
        let step = face.offset();
        let face_bits = u32::from(face.index()) << 25;
        for slice in 0..EDGE {
            // A wall as one number: 0 for none, else (low + 1) | high + 1 << 6 | inside << 12 (low and high are 0 to 31).
            let mut run_wall = 0u32;
            let mut run_start = 0usize;
            for position in 0..=EDGE {
                let wall = if position == EDGE {
                    0
                } else {
                    let (x, z) = if axis == 0 {
                        (slice, position)
                    } else {
                        (position, slice)
                    };
                    let own = i32::from(tops[z * EDGE + x]);
                    let (nx, nz) = (x as i64 + step[0], z as i64 + step[2]);
                    let beside = if (0..EDGE as i64).contains(&nx) && (0..EDGE as i64).contains(&nz)
                    {
                        i32::from(tops[nz as usize * EDGE + nx as usize]).min(31)
                    } else {
                        -1
                    };
                    let own_cell = own.min(31);
                    if own_cell > beside {
                        ((beside + 1) as u32 + 1)
                            | ((own_cell as u32 + 1) << 6)
                            | (u32::from(own < OVER.into()) << 12)
                    } else {
                        0
                    }
                };
                if wall != run_wall {
                    if run_wall != 0 {
                        let low = (run_wall & 63) - 1;
                        let high = ((run_wall >> 6) & 63) - 1;
                        let inside = (run_wall >> 12) & 1 == 1;
                        let length = (position - run_start) as u32;
                        let mut emit = |first: u32, last: u32, material: u32| {
                            if last < first {
                                return;
                            }
                            let extent = last - first + 1;
                            let (origin, width, height) = if axis == 0 {
                                ([slice as u32, first, run_start as u32], extent, length)
                            } else {
                                ([run_start as u32, first, slice as u32], length, extent)
                            };
                            quads.push(voxel_meshing::PackedQuad {
                                geometry: origin[0]
                                    | origin[1] << 5
                                    | origin[2] << 10
                                    | (width - 1) << 15
                                    | (height - 1) << 20
                                    | face_bits,
                                material: material | 255 << 24,
                            });
                        };
                        if inside {
                            if high > low {
                                emit(low, high - 1, MATERIALS.stone);
                            }
                            emit(high, high, MATERIALS.grass);
                        } else {
                            emit(low, high, MATERIALS.stone);
                        }
                    }
                    run_wall = wall;
                    run_start = position;
                }
            }
        }
    }
}

pub fn the_faster_way_makes_the_same_faces_and_how_much_faster_it_is() {
    let mut direct_total = Duration::ZERO;
    let mut fast_total = Duration::ZERO;
    let mut count = 0u32;
    for level in 1..=5u8 {
        let size = 1i64 << level;
        for cube in cubes_around_the_surface(level, 4) {
            let origin = cube.origin().origin();
            let batch = batch_hills(
                origin.x() + size / 2,
                origin.z() + size / 2,
                size,
                EDGE,
                level,
            );
            let heights: Vec<f64> = batch.iter().map(|noise| BASE + noise).collect();
            let columns = columns_from_heights(&heights, origin.y(), size);
            let tops = tops_from_heights_fast(&heights, origin.y(), u32::from(level));
            // The same columns...
            for index in 0..EDGE * EDGE {
                let own = i64::from(columns.top[index]);
                let fast = i64::from(tops[index]);
                if columns.surface_inside[index] {
                    assert_eq!(own, fast, "{cube:?} column {index}");
                } else {
                    assert!(fast == i64::from(OVER) || fast == -1, "{cube:?}");
                    assert_eq!(own == -1, fast == -1, "{cube:?} column {index}");
                }
            }
            // ...the same faces.
            let reference: Vec<Quad> = mesh_columns(&columns);
            let mut packed = Vec::with_capacity(2048);
            mesh_tops_fast(&tops, &mut packed);
            let unpacked: Vec<Quad> = packed.iter().map(|quad| quad.unpack()).collect();
            assert_eq!(unit_faces(&unpacked), unit_faces(&reference), "{cube:?}");
            // And the time of each.
            let start = Instant::now();
            for _ in 0..20 {
                let columns = columns_from_heights(&heights, origin.y(), size);
                let quads = mesh_columns(&columns);
                let packed: Vec<voxel_meshing::PackedQuad> =
                    quads.iter().map(|quad| quad.pack()).collect();
                std::hint::black_box(packed);
            }
            direct_total += start.elapsed() / 20;
            let start = Instant::now();
            for _ in 0..20 {
                let tops = tops_from_heights_fast(&heights, origin.y(), u32::from(level));
                let mut packed = Vec::with_capacity(2048);
                mesh_tops_fast(&tops, &mut packed);
                std::hint::black_box(packed);
            }
            fast_total += start.elapsed() / 20;
            count += 1;
        }
    }
    eprintln!(
        "FASTER heights to packed quads: {:.1} us a node the way it was, {:.1} us a node the faster way ({count} nodes)",
        direct_total.as_secs_f64() * 1e6 / f64::from(count),
        fast_total.as_secs_f64() * 1e6 / f64::from(count),
    );
}

pub fn top_cell_fast_is_top_cell() {
    for level in 1..=6u32 {
        let size = 1i64 << level;
        for origin_y in [-4096i64, -64, 0, 32, 1000, 1 << 20] {
            for step in 0..4000 {
                let height = origin_y as f64
                    + (f64::from(step) - 1000.0) * 0.37 * f64::from(1u32 << level) / 8.0;
                assert_eq!(
                    top_cell_fast(height, origin_y, level),
                    top_cell(height, origin_y, size),
                    "{height} {origin_y} {level}"
                );
            }
        }
    }
}

// ---- The whole view at once (appended to zz_columns.rs): the pruned selection, then every node made by the columns, on many threads.

pub fn the_whole_view_made_from_nothing_at_several_distances() {
    use coarse_cells::{DetailSettings, ViewSelection, select_around_brick};
    use terrain::TerrainGenerator;

    let fixture = StandardTestLayout::default();
    let generator = FieldTerrainGenerator::new(
        Arc::new(Heightfield::new(BASE, hills())),
        fixture.material,
        MATERIALS,
    );
    let settings = DetailSettings {
        cell_pixels: 3.0,
        pixel_size: 2.0 * 0.700_207_538_209_062_3 / 1080.0,
    };
    for distance in [1_500.0, 20_000.0, 1_000_000.0, 100_000_000.0] {
        let view = ViewSelection {
            detail: settings,
            zoom: None,
            fine_radius: 8,
            maximum_distance: distance,
        };
        let centre = BrickCoordinate::new(100, 3, 100);
        let start = Instant::now();
        let cubes = select_around_brick(centre, &view, &|cube| generator.may_show(cube));
        let selecting = start.elapsed();
        let nodes: Vec<BrickCube> = cubes.into_iter().filter(|cube| cube.level() > 0).collect();
        for threads in [1usize, 12] {
            let start = Instant::now();
            let made = std::sync::atomic::AtomicUsize::new(0);
            let quad_count = std::sync::atomic::AtomicUsize::new(0);
            std::thread::scope(|scope| {
                for t in 0..threads {
                    let (nodes, made, quad_count) = (&nodes, &made, &quad_count);
                    scope.spawn(move || {
                        let (mut count, mut quads) = (0usize, 0usize);
                        for (index, cube) in nodes.iter().enumerate() {
                            if index % threads != t {
                                continue;
                            }
                            let level = u32::from(cube.level());
                            let size = 1i64 << level;
                            let origin = cube.origin().origin();
                            let batch = batch_hills(
                                origin.x() + size / 2,
                                origin.z() + size / 2,
                                size,
                                EDGE,
                                cube.level(),
                            );
                            let heights: Vec<f64> =
                                batch.iter().map(|noise| BASE + noise).collect();
                            let tops = tops_from_heights_fast(&heights, origin.y(), level);
                            let mut packed = Vec::with_capacity(2048);
                            mesh_tops_fast(&tops, &mut packed);
                            count += 1;
                            quads += packed.len();
                        }
                        made.fetch_add(count, std::sync::atomic::Ordering::Relaxed);
                        quad_count.fetch_add(quads, std::sync::atomic::Ordering::Relaxed);
                    });
                }
            });
            let time = start.elapsed();
            eprintln!(
                "VIEW to {distance}: {} nodes chosen in {:?}; made on {threads} thread(s) in {:.1} ms ({} quads, {:.1} MB of quads)",
                nodes.len(),
                selecting,
                ms(time),
                quad_count.load(std::sync::atomic::Ordering::Relaxed),
                quad_count.load(std::sync::atomic::Ordering::Relaxed) as f64 * 8.0 / 1.0e6,
            );
        }
    }
}
