//! A horizon swept over the heights of the columns themselves, near to far, while the nodes are made: what hides what, at the resolution of a cell, with no bounds.
//!
//! The nodes of the pruned selection are taken in the order of how near they are. For each, the heights of its columns are made (as they are for the mesh anyway)
//! and a column is *hidden* if the steepest ray that could see its highest point is under what nearer columns have shown of the horizon in every direction it
//! spans (a direction is a bin of an angle; the horizon of a bin is kept for distances in doubling buckets, and only buckets wholly nearer than the column count, so
//! that a column is never hidden by one beside it or behind it). A node with every column hidden is not meshed and not sent; the others add their columns to the horizon.
//! The result is checked against rays through the heights as they are drawn.

use std::sync::Arc;
use std::time::Instant;

use coarse_cells::{DetailSettings, ViewSelection, select_around_brick};
use region_octree::BrickCube;
use terrain::{
    FieldTerrainGenerator, FractalNoise, Heightfield, TerrainGenerator, TerrainMaterials,
};
use voxel_grid::test_support::StandardTestLayout;
use voxel_space::{BrickCoordinate, LevelOfDetail, VoxelCoordinate};

use crate::columns::{BASE, MATERIALS, batch_hills, hills};

const EDGE: usize = 32;
const BINS: usize = 8192;
const BUCKETS: usize = 22;
/// The nearest bucket begins here, in voxels; each is twice as far as the one before.
const FIRST_BUCKET: f32 = 16.0;
const MARGIN: f32 = 2.0e-4;

fn level_at(distance: f64) -> u8 {
    ((0.0039 * distance).max(1.0).log2().floor() as i64).clamp(0, 30) as u8
}

fn height_at_level(noise: &FractalNoise, x: f64, z: f64, level: u8) -> f64 {
    BASE + noise.sample(
        VoxelCoordinate::new(x.floor() as i64, 0, z.floor() as i64),
        LevelOfDetail::new(level).unwrap(),
    )
}

/// The oracle: whether the ground as it is drawn hides the point from the eye.
fn hidden_by_rays(noise: &FractalNoise, eye: [f64; 3], point: [f64; 3]) -> bool {
    let distance = ((point[0] - eye[0]).powi(2) + (point[2] - eye[2]).powi(2)).sqrt();
    let steps = ((distance / 2.0) as usize).clamp(8, 4096);
    for step in 1..steps {
        let t = step as f64 / steps as f64;
        let x = eye[0] + (point[0] - eye[0]) * t;
        let z = eye[2] + (point[2] - eye[2]) * t;
        let y = eye[1] + (point[1] - eye[1]) * t;
        let level = level_at(distance * t);
        let half_cell = f64::from(1u32 << level) / 2.0;
        if height_at_level(noise, x, z, level) > y + half_cell {
            return true;
        }
    }
    false
}

/// The top of the solid in the column of cells of the level at (x, z), as it is drawn: the cells are `2^level` voxels, a cell is drawn when half of it or more is ground, and
/// the height of the column is that of the middle of the cell.
fn drawn_top(noise: &FractalNoise, x: f64, z: f64, level: u8) -> f64 {
    let size = 1i64 << level;
    let cell_x = (x.floor() as i64).div_euclid(size) * size + size / 2;
    let cell_z = (z.floor() as i64).div_euclid(size) * size + size / 2;
    let height = BASE
        + noise.sample(
            VoxelCoordinate::new(cell_x, 0, cell_z),
            LevelOfDetail::new(level).unwrap(),
        );
    let first_air = (height - 0.5).ceil() as i64;
    (((first_air - size / 2).div_euclid(size) + 1) * size) as f64
}

/// The level of the node that really draws the ground at (x, z): the leaf that holds the point at the height of the ground, found by trying each level.
fn level_drawn(
    leaves: &std::collections::HashSet<BrickCube>,
    noise: &FractalNoise,
    x: f64,
    z: f64,
    guess: u8,
) -> u8 {
    let height = height_at_level(noise, x, z, guess);
    let (bx, by, bz) = (
        (x / 32.0).floor() as i64,
        (height / 32.0).floor() as i64,
        (z / 32.0).floor() as i64,
    );
    for level in 1..=22u8 {
        if leaves.contains(&BrickCube::containing(
            BrickCoordinate::new(bx, by, bz),
            level,
        )) {
            return level;
        }
    }
    guess
}

/// The stricter oracle: a ray through the columns exactly as they are drawn (each at the level of the node that is really there), looked at in steps of half a cell, with
/// nothing in hand for the target's own cell.
fn hidden_by_drawn_columns(
    leaves: &std::collections::HashSet<BrickCube>,
    noise: &FractalNoise,
    eye: [f64; 3],
    point: [f64; 3],
) -> bool {
    let distance = ((point[0] - eye[0]).powi(2) + (point[2] - eye[2]).powi(2)).sqrt();
    let target_cell = 1.0f64.max(f64::from(1u32 << level_at(distance)));
    let mut travelled = 0.0;
    while travelled < distance - target_cell {
        let cell = f64::from(1u32 << level_at(travelled.max(1.0)));
        travelled += (cell / 2.0).max(0.5);
        if travelled >= distance - target_cell {
            break;
        }
        let t = travelled / distance;
        let x = eye[0] + (point[0] - eye[0]) * t;
        let z = eye[2] + (point[2] - eye[2]) * t;
        let y = eye[1] + (point[1] - eye[1]) * t;
        let level = level_drawn(leaves, noise, x, z, level_at(travelled));
        if drawn_top(noise, x, z, level) > y + 1.0e-6 {
            return true;
        }
    }
    false
}

/// The angle of (x, z) from the first axis, a little under a hundred-thousandth of a radian out.
fn fast_atan2(y: f32, x: f32) -> f32 {
    let (ax, ay) = (x.abs(), y.abs());
    let (low, high) = (ax.min(ay), ax.max(ay));
    if high == 0.0 {
        return 0.0;
    }
    let a = low / high;
    let s = a * a;
    let mut r = ((-0.046_496_474 * s + 0.159_314_22) * s - 0.327_622_77) * s * a + a;
    if ay > ax {
        r = std::f32::consts::FRAC_PI_2 - r;
    }
    if x < 0.0 {
        r = std::f32::consts::PI - r;
    }
    if y < 0.0 {
        r = -r;
    }
    r
}

fn bucket_of(distance: f32) -> usize {
    if distance < FIRST_BUCKET {
        0
    } else {
        (((distance / FIRST_BUCKET).log2()) as usize + 1).min(BUCKETS - 1)
    }
}

/// For each direction and each bucket of distance, the steepest ray (rise over run) that a column whose far edge is in that bucket or a nearer one is certain to
/// stop. Kept as prefix maxima, so a query is one look.
struct Horizon {
    slope: Vec<f32>,
}

impl Horizon {
    fn new() -> Self {
        Self {
            slope: vec![f32::NEG_INFINITY; BINS * BUCKETS],
        }
    }

    /// What is certain to be stopped, in a direction, by what is wholly nearer than a column that begins `distance` away.
    fn before(&self, bin: usize, distance: f32) -> f32 {
        match bucket_of(distance) {
            0 => f32::NEG_INFINITY,
            bucket => self.slope[bin * BUCKETS + bucket - 1],
        }
    }

    /// Adds a column that ends `distance` away and stops every ray under `slope`.
    fn add(&mut self, bin: usize, distance: f32, slope: f32) {
        // Prefix maxima never fall with the bucket, so once one is as steep as this, so are the rest.
        for bucket in bucket_of(distance)..BUCKETS {
            let at = &mut self.slope[bin * BUCKETS + bucket];
            if *at >= slope {
                break;
            }
            *at = slope;
        }
    }
}

/// The bins a column that is `distance` away, in the direction `angle`, can be seen in: every bin that any ray to any point of the cell passes through (the cell is
/// inside a circle of radius `reach`). `None` for the whole round, when the eye is that near.
fn target_bins(angle: f32, distance: f32, reach: f32) -> Option<(usize, usize)> {
    if distance <= 1.5 * reach {
        return None;
    }
    let half = (reach / distance).asin();
    let centre = (angle + std::f32::consts::PI) / std::f32::consts::TAU * BINS as f32;
    let reach_in_bins = half / std::f32::consts::TAU * BINS as f32;
    let wrap = |bin: f32| (bin.floor() as isize).rem_euclid(BINS as isize) as usize;
    Some((wrap(centre - reach_in_bins), wrap(centre + reach_in_bins)))
}

/// The bins a column is sure to stop every ray of: those that lie wholly inside the angles of the circle inside the cell (radius `inner`), which every ray passing
/// that near the middle of the cell crosses. Empty when no bin does.
fn occluder_bins(angle: f32, distance: f32, inner: f32) -> Option<(usize, usize)> {
    if distance <= 1.5 * inner * std::f32::consts::SQRT_2 {
        return None;
    }
    let half = (inner / distance).asin();
    let centre = (angle + std::f32::consts::PI) / std::f32::consts::TAU * BINS as f32;
    let reach_in_bins = half / std::f32::consts::TAU * BINS as f32;
    let first = (centre - reach_in_bins).ceil();
    let last = (centre + reach_in_bins).floor() - 1.0;
    if last < first {
        return Some((1, 0)).filter(|_| false);
    }
    let wrap = |bin: f32| (bin as isize).rem_euclid(BINS as isize) as usize;
    Some((wrap(first), wrap(last)))
}

fn for_bins(range: Option<(usize, usize)>, mut visit: impl FnMut(usize)) {
    match range {
        None => {
            for bin in 0..BINS {
                visit(bin);
            }
        }
        Some((first, last)) => {
            let count = (last + BINS - first) % BINS + 1;
            for step in 0..count {
                visit((first + step) % BINS);
            }
        }
    }
}

struct Column {
    x: f32,
    z: f32,
    /// How high the solid of the column goes in this node (the top of its top cell), and whether the top is a surface seen from above.
    top: f32,
}

pub fn a_horizon_swept_over_the_columns_as_they_are_made() {
    let noise = hills();
    let fixture = StandardTestLayout::default();
    let generator = FieldTerrainGenerator::new(
        Arc::new(Heightfield::new(BASE, hills())),
        fixture.material,
        TerrainMaterials { ..MATERIALS },
    );
    let mut places: Vec<(f64, f64, f64)> = Vec::new();
    for i in 0..40 {
        for j in 0..40 {
            let (x, z) = (f64::from(i) * 100.0 + 50.0, f64::from(j) * 100.0 + 50.0);
            places.push((height_at_level(&noise, x, z, 0), x, z));
        }
    }
    places.sort_by(|a, b| a.0.total_cmp(&b.0));
    let eyes = [
        ("valley", places[0]),
        ("low", places[places.len() / 8]),
        ("middling", places[places.len() / 2]),
        ("high", places[places.len() * 7 / 8]),
        ("ridge", places[places.len() - 1]),
    ];
    for (name, (ground, ex, ez)) in eyes {
        let eye = [ex, ground + 2.0, ez];
        let brick = BrickCoordinate::new(
            (ex / 32.0).floor() as i64,
            ((ground + 2.0) / 32.0).floor() as i64,
            (ez / 32.0).floor() as i64,
        );
        let view = ViewSelection {
            detail: DetailSettings {
                cell_pixels: 3.0,
                pixel_size: 2.0 * 0.700_207_538_209_062_3 / 1080.0,
            },
            zoom: None,
            fine_radius: 0,
            maximum_distance: 20_000.0,
        };
        let cubes = select_around_brick(brick, &view, &|cube| generator.may_show(cube));
        let mut nodes: Vec<BrickCube> = cubes.into_iter().filter(|cube| cube.level() > 0).collect();
        // Near to far by the nearest point of the footprint.
        let near_of = |cube: &BrickCube| -> f64 {
            let edge = (cube.edge_length() * 32) as f64;
            let origin = cube.origin().origin();
            let gap = |low: f64, high: f64, at: f64| {
                if at < low {
                    low - at
                } else if at > high {
                    at - high
                } else {
                    0.0
                }
            };
            let (x0, z0) = (origin.x() as f64, origin.z() as f64);
            gap(x0, x0 + edge, eye[0]).hypot(gap(z0, z0 + edge, eye[2]))
        };
        nodes.sort_by(|a, b| near_of(a).total_cmp(&near_of(b)));

        let start = Instant::now();
        let mut horizon = Horizon::new();
        let (mut with_content, mut hidden_nodes, mut visible_nodes) = (0usize, 0usize, 0usize);
        let mut columns_tested = 0u64;
        let mut culled: Vec<(BrickCube, Vec<Column>)> = Vec::new();
        let mut generating = std::time::Duration::ZERO;
        for cube in &nodes {
            let level = cube.level();
            let size = 1i64 << level;
            let origin = cube.origin().origin();
            let timer = Instant::now();
            let batch = batch_hills(
                origin.x() + size / 2,
                origin.z() + size / 2,
                size,
                EDGE,
                level,
            );
            generating += timer.elapsed();
            // The node at once first, before any column is looked at: the highest the ground goes in it, as near as the node comes, against the horizon of every direction it can be seen in.
            let node_top = (origin.y() + 32 * size) as f64;
            let highest = batch
                .iter()
                .fold(f64::NEG_INFINITY, |most, noise| most.max(*noise))
                + BASE;
            if highest + (size as f64) < origin.y() as f64 {
                continue; // all air
            }
            with_content += 1;
            let top_most = (highest + size as f64).min(node_top) as f32;
            let eye_y = eye[1] as f32;
            let centre_x = (origin.x() as f32 + 16.0 * size as f32) - eye[0] as f32;
            let centre_z = (origin.z() as f32 + 16.0 * size as f32) - eye[2] as f32;
            let node_distance = centre_x.hypot(centre_z);
            let node_reach = 16.0 * size as f32 * std::f32::consts::SQRT_2;
            let (node_near, node_far) = (
                (node_distance - node_reach).max(0.0),
                node_distance + node_reach,
            );
            let node_up = if top_most > eye_y {
                (top_most - eye_y) / node_near.max(1.0)
            } else {
                (top_most - eye_y) / node_far
            };
            let mut node_seen = false;
            for_bins(
                target_bins(fast_atan2(centre_z, centre_x), node_distance, node_reach),
                |bin| {
                    if horizon.before(bin, node_near) < node_up + MARGIN {
                        node_seen = true;
                    }
                },
            );
            if !node_seen {
                hidden_nodes += 1;
                culled.push((*cube, Vec::new()));
                continue;
            }
            // The columns that have something in this node: the top of the solid as high as the node goes.
            let mut columns: Vec<Column> = Vec::with_capacity(EDGE * EDGE);
            for (index, noise_value) in batch.iter().enumerate() {
                let (i, j) = (index % EDGE, index / EDGE);
                let height = BASE + noise_value;
                let top_cell =
                    ((height - 0.5).ceil() as i64 - origin.y() - size / 2).div_euclid(size);
                if top_cell < 0 {
                    continue;
                }
                let top = if top_cell > 31 {
                    node_top
                } else {
                    (origin.y() + (top_cell + 1) * size) as f64
                };
                columns.push(Column {
                    x: (origin.x() + i as i64 * size) as f32 + size as f32 / 2.0 - eye[0] as f32,
                    z: (origin.z() + j as i64 * size) as f32 + size as f32 / 2.0 - eye[2] as f32,
                    top: top as f32,
                });
            }
            if columns.is_empty() {
                continue;
            }
            let half_size = size as f32 / 2.0;
            let reach = half_size * std::f32::consts::SQRT_2;
            // Is any column possibly seen?
            let mut seen = false;
            for column in &columns {
                columns_tested += 1;
                let distance = column.x.hypot(column.z);
                let (near, far) = ((distance - reach).max(0.0), distance + reach);
                let up = if column.top > eye_y {
                    (column.top - eye_y) / near.max(1.0)
                } else {
                    (column.top - eye_y) / far
                };
                let angle = fast_atan2(column.z, column.x);
                let mut visible = false;
                for_bins(target_bins(angle, distance, reach), |bin| {
                    if horizon.before(bin, near) < up + MARGIN {
                        visible = true;
                    }
                });
                if visible {
                    seen = true;
                    break;
                }
            }
            if !seen {
                hidden_nodes += 1;
                culled.push((*cube, columns));
                continue;
            }
            visible_nodes += 1;
            for column in &columns {
                let distance = column.x.hypot(column.z);
                let (near, far) = ((distance - reach).max(0.0), distance + reach);
                let blocks = if column.top > eye_y {
                    (column.top - eye_y) / far
                } else {
                    (column.top - eye_y) / near.max(1.0)
                };
                let angle = fast_atan2(column.z, column.x);
                if let Some(range) = occluder_bins(angle, distance, half_size) {
                    for_bins(Some(range), |bin| horizon.add(bin, far, blocks - MARGIN));
                }
            }
        }
        let sweep_time = start.elapsed();
        // The rays against the nodes that were culled: sixteen columns each.
        let leaf_set: std::collections::HashSet<BrickCube> = nodes.iter().copied().collect();
        let (mut wrong_nodes, mut checked_nodes) = (0usize, 0usize);
        let (mut wrong_columns, mut strict_nodes, mut strict_columns) = (0usize, 0usize, 0usize);
        for (_, columns) in &culled {
            checked_nodes += 1;
            let (mut node_wrong, mut node_strict) = (false, false);
            for column in columns.iter().step_by((columns.len() / 16).max(1)) {
                let point = [
                    f64::from(column.x) + eye[0],
                    f64::from(column.top),
                    f64::from(column.z) + eye[2],
                ];
                if !hidden_by_rays(&noise, eye, point) {
                    wrong_columns += 1;
                    node_wrong = true;
                }
                if !hidden_by_drawn_columns(&leaf_set, &noise, eye, point) {
                    strict_columns += 1;
                    node_strict = true;
                }
            }
            wrong_nodes += usize::from(node_wrong);
            strict_nodes += usize::from(node_strict);
        }
        eprintln!(
            "SWEEP {name} (eye {:.0} high): {} nodes chosen, {with_content} with something in them; {hidden_nodes} hidden ({:.0}%), {visible_nodes} kept; {:.1} ms ({:.1} ms of it the noise, {columns_tested} columns tested); of the hidden, {wrong_nodes} of {checked_nodes} nodes have a column the smooth rays say is seen ({wrong_columns} columns); through the columns as drawn, {strict_nodes} nodes ({strict_columns} columns)",
            eye[1],
            nodes.len(),
            100.0 * hidden_nodes as f64 / with_content.max(1) as f64,
            sweep_time.as_secs_f64() * 1000.0,
            generating.as_secs_f64() * 1000.0,
        );
    }
}
