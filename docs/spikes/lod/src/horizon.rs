//! Horizon culling from the eye's place only (not where it looks), with the heights a generator can promise, against the rays.

use std::sync::Arc;
use std::time::Instant;

use coarse_cells::{DetailSettings, ViewSelection, select_around_brick};
use region_octree::BrickCube;
use terrain::{
    DensityField, Dimensionality, FieldTerrainGenerator, FractalNoise, Heightfield,
    TerrainGenerator, TerrainMaterials,
};
use voxel_grid::test_support::StandardTestLayout;
use voxel_space::{BrickCoordinate, LevelOfDetail, VoxelBox, VoxelCoordinate};

const MATERIALS: TerrainMaterials = TerrainMaterials {
    air: 0,
    stone: 1,
    dirt: 2,
    grass: 3,
};
const BASE: f64 = 64.0;
const BINS: usize = 4096;

fn hills() -> FractalNoise {
    FractalNoise::standard(2024, Dimensionality::Planar, 7, 6, 40.0, 0.5)
}

fn settings() -> DetailSettings {
    DetailSettings {
        cell_pixels: 3.0,
        pixel_size: 2.0 * 0.700_207_538_209_062_3 / 1080.0,
    }
}

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
fn hidden(noise: &FractalNoise, eye: [f64; 3], point: [f64; 3]) -> bool {
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

/// A rectangle on the ground, as seen from the eye: how near and far it is and the directions it spans.
struct Seen {
    near: f64,
    far: f64,
    first: usize,
    last: usize,
    everything: bool,
}

fn seen(eye: [f64; 3], x0: f64, z0: f64, x1: f64, z1: f64) -> Seen {
    let (x0, z0, x1, z1) = (x0 - eye[0], z0 - eye[2], x1 - eye[0], z1 - eye[2]);
    let gap = |low: f64, high: f64| {
        if low > 0.0 {
            low
        } else if high < 0.0 {
            -high
        } else {
            0.0
        }
    };
    let near = (gap(x0, x1).powi(2) + gap(z0, z1).powi(2)).sqrt();
    let far = x0.abs().max(x1.abs()).hypot(z0.abs().max(z1.abs()));
    if x0 <= 0.0 && x1 >= 0.0 && z0 <= 0.0 && z1 >= 0.0 {
        return Seen {
            near: 0.0,
            far,
            first: 0,
            last: BINS - 1,
            everything: true,
        };
    }
    let angles: Vec<f64> = [(x0, z0), (x1, z0), (x0, z1), (x1, z1)]
        .iter()
        .map(|(x, z)| z.atan2(*x))
        .collect();
    let mut best = (f64::MAX, 0.0, 0.0);
    for &start in &angles {
        let mut widest = 0.0f64;
        for &other in &angles {
            let mut delta = other - start;
            while delta < 0.0 {
                delta += std::f64::consts::TAU;
            }
            widest = widest.max(delta);
        }
        if widest < best.0 {
            best = (widest, start, start + widest);
        }
    }
    let bin = |angle: f64| -> usize {
        let mut a = angle;
        while a < 0.0 {
            a += std::f64::consts::TAU;
        }
        ((a / std::f64::consts::TAU * BINS as f64) as usize).min(BINS - 1)
    };
    Seen {
        near,
        far,
        first: bin(best.1 - 1.0e-6),
        last: bin(best.2 + 1.0e-6),
        everything: false,
    }
}

fn bins_of(f: &Seen) -> impl Iterator<Item = usize> + use<> {
    let (first, last, everything) = (f.first, f.last, f.everything);
    let count = if everything {
        BINS
    } else {
        (last + BINS - first) % BINS + 1
    };
    (0..count).map(move |i| (first + i) % BINS)
}

/// The lowest and highest the ground can be over a rectangle, at a level of detail, with a cell either way for the cells being drawn.
fn range_over(field: &Heightfield, rect: &Rect, level: u8) -> (f64, f64) {
    let region = VoxelBox::new(
        VoxelCoordinate::new(rect.x0, 0, rect.z0),
        VoxelCoordinate::new(rect.x0 + rect.size - 1, 0, rect.z0 + rect.size - 1),
    )
    .unwrap();
    let density = field.bounds(&region, LevelOfDetail::new(level).unwrap());
    let cell = f64::from(1u32 << level);
    (density.lower() + 0.5 - cell, density.upper() + 0.5 + cell)
}

struct Rect {
    x0: i64,
    z0: i64,
    size: i64,
}

fn quarters(r: &Rect) -> [Rect; 4] {
    let h = r.size / 2;
    [
        Rect {
            x0: r.x0,
            z0: r.z0,
            size: h,
        },
        Rect {
            x0: r.x0 + h,
            z0: r.z0,
            size: h,
        },
        Rect {
            x0: r.x0,
            z0: r.z0 + h,
            size: h,
        },
        Rect {
            x0: r.x0 + h,
            z0: r.z0 + h,
            size: h,
        },
    ]
}

fn seen_rect(eye: [f64; 3], r: &Rect) -> Seen {
    seen(
        eye,
        r.x0 as f64,
        r.z0 as f64,
        (r.x0 + r.size) as f64,
        (r.z0 + r.size) as f64,
    )
}

/// Whether some of the ground of the rectangle, up to `cube_top`, might be seen over the horizon; refined by `depth` more splits, counting the bounds asked for.
#[allow(clippy::too_many_arguments)]
fn might_be_seen(
    field: &Heightfield,
    eye: [f64; 3],
    horizon: &[f64],
    rect: &Rect,
    level: u8,
    cube_top: f64,
    depth: u32,
    asked: &mut u64,
) -> bool {
    let (_, high) = range_over(field, rect, level);
    *asked += 1;
    let top = high.min(cube_top);
    let f = seen_rect(eye, rect);
    if f.near < 64.0 {
        return true;
    }
    let up = if top > eye[1] {
        (top - eye[1]) / f.near
    } else {
        (top - eye[1]) / f.far
    };
    if bins_of(&f).all(|bin| horizon[bin] >= up) {
        return false;
    }
    if depth == 0 || rect.size <= 32 {
        return true;
    }
    quarters(rect).iter().any(|quarter| {
        might_be_seen(
            field,
            eye,
            horizon,
            quarter,
            level,
            cube_top,
            depth - 1,
            asked,
        )
    })
}

/// Adds to the horizon what the ground over a rectangle is sure to stop, in pieces `depth` splits deep.
fn add_to_horizon(
    field: &Heightfield,
    eye: [f64; 3],
    horizon: &mut [f64],
    rect: &Rect,
    level: u8,
    depth: u32,
    asked: &mut u64,
) {
    if depth > 0 && rect.size > 32 {
        for quarter in quarters(rect) {
            add_to_horizon(field, eye, horizon, &quarter, level, depth - 1, asked);
        }
        return;
    }
    let (low, _) = range_over(field, rect, level);
    *asked += 1;
    let f = seen_rect(eye, rect);
    if f.near < 64.0 {
        return;
    }
    let stops = if low > eye[1] {
        (low - eye[1]) / f.far
    } else {
        (low - eye[1]) / f.near
    };
    for bin in bins_of(&f) {
        if horizon[bin] < stops {
            horizon[bin] = stops;
        }
    }
}

/// The cubes of `leaves` that horizon culling says are hidden from `eye`: tested in pieces `test_depth` splits deep, with the horizon built from pieces `build_depth`
/// splits deep. Returns which are culled and how many bounds were asked.
fn horizon_cull(
    field: &Heightfield,
    eye: [f64; 3],
    leaves: &[BrickCube],
    test_depth: u32,
    build_depth: u32,
) -> (Vec<bool>, u64) {
    let rect_of = |cube: &BrickCube| {
        let edge = (cube.edge_length() * 32) as i64;
        let origin = cube.origin().origin();
        Rect {
            x0: origin.x(),
            z0: origin.z(),
            size: edge,
        }
    };
    let nears: Vec<f64> = leaves
        .iter()
        .map(|cube| seen_rect(eye, &rect_of(cube)).near)
        .collect();
    let mut order: Vec<usize> = (0..leaves.len()).collect();
    order.sort_by(|a, b| nears[*a].total_cmp(&nears[*b]));
    let mut horizon = vec![f64::NEG_INFINITY; BINS];
    let mut culled = vec![false; leaves.len()];
    let mut asked = 0u64;
    for index in order {
        let cube = leaves[index];
        let rect = rect_of(&cube);
        let cube_top = (cube.origin().y() * 32 + (cube.edge_length() * 32) as i64) as f64;
        if !might_be_seen(
            field,
            eye,
            &horizon,
            &rect,
            cube.level(),
            cube_top,
            test_depth,
            &mut asked,
        ) {
            culled[index] = true;
            continue;
        }
        add_to_horizon(
            field,
            eye,
            &mut horizon,
            &rect,
            cube.level(),
            build_depth,
            &mut asked,
        );
    }
    (culled, asked)
}

pub fn how_many_cubes_does_the_horizon_of_the_eyes_place_cull() {
    let noise = hills();
    let field = Heightfield::new(BASE, hills());
    let fixture = StandardTestLayout::default();
    let generator = FieldTerrainGenerator::new(
        Arc::new(Heightfield::new(BASE, hills())),
        fixture.material,
        MATERIALS,
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
            detail: settings(),
            zoom: None,
            fine_radius: 8,
            maximum_distance: 20_000.0,
        };
        let cubes = select_around_brick(brick, &view, &|cube| generator.may_show(cube));
        let leaves: Vec<BrickCube> = cubes.into_iter().filter(|cube| cube.level() > 0).collect();
        for (test_depth, build_depth) in [(0u32, 0u32), (2, 2), (3, 3), (4, 4)] {
            let start = Instant::now();
            let (culled, asked) = horizon_cull(&field, eye, &leaves, test_depth, build_depth);
            let time = start.elapsed();
            let hidden_count = culled.iter().filter(|c| **c).count();
            // The oracle: nine places on the surface in each cube. A cube has surface if some are in it; one that is culled and has a place the rays say is seen is wrong.
            let (mut with_surface, mut surface_culled, mut wrong) = (0usize, 0usize, 0usize);
            for (cube, &was_culled) in leaves.iter().zip(&culled) {
                let origin = cube.origin().origin();
                let edge = (cube.edge_length() * 32) as f64;
                let (x0, y0, z0) = (origin.x() as f64, origin.y() as f64, origin.z() as f64);
                let (mut has, mut visible) = (false, false);
                for i in 0..3 {
                    for j in 0..3 {
                        let (x, z) = (
                            x0 + edge * (0.1 + 0.4 * f64::from(i)),
                            z0 + edge * (0.1 + 0.4 * f64::from(j)),
                        );
                        let d = ((x - eye[0]).powi(2) + (z - eye[2]).powi(2)).sqrt();
                        let y = height_at_level(&noise, x, z, level_at(d));
                        if y >= y0 && y < y0 + edge {
                            has = true;
                            if was_culled {
                                visible |= !hidden(&noise, eye, [x, y, z]);
                            }
                        }
                    }
                }
                with_surface += usize::from(has);
                surface_culled += usize::from(has && was_culled);
                wrong += usize::from(has && was_culled && visible);
            }
            eprintln!(
                "HORIZON {name} (eye {:.0} high) depth {test_depth}/{build_depth}: {} cubes kept by pruning, {} culled ({:.0}%) in {:.1} ms with {asked} bounds; of the {with_surface} with surface in them {surface_culled} are culled ({:.0}%), {wrong} of those have a place the rays say is seen",
                ground + 2.0,
                leaves.len(),
                hidden_count,
                100.0 * hidden_count as f64 / leaves.len() as f64,
                time.as_secs_f64() * 1000.0,
                100.0 * surface_culled as f64 / with_surface.max(1) as f64,
            );
        }
    }
}

pub fn debug_what_the_horizon_looks_like_from_a_middling_eye() {
    let noise = hills();
    let field = Heightfield::new(BASE, hills());
    let fixture = StandardTestLayout::default();
    let generator = FieldTerrainGenerator::new(
        Arc::new(Heightfield::new(BASE, hills())),
        fixture.material,
        MATERIALS,
    );
    let (ex, ez) = (2350.0, 1850.0);
    let ground = height_at_level(&noise, ex, ez, 0);
    let eye = [ex, ground + 2.0, ez];
    let brick = BrickCoordinate::new(
        (ex / 32.0).floor() as i64,
        ((ground + 2.0) / 32.0).floor() as i64,
        (ez / 32.0).floor() as i64,
    );
    let view = ViewSelection {
        detail: settings(),
        zoom: None,
        fine_radius: 8,
        maximum_distance: 20_000.0,
    };
    let cubes = select_around_brick(brick, &view, &|cube| generator.may_show(cube));
    let leaves: Vec<BrickCube> = cubes.into_iter().filter(|cube| cube.level() > 0).collect();
    let mut horizon = vec![f64::NEG_INFINITY; BINS];
    let mut asked = 0;
    for cube in &leaves {
        let edge = (cube.edge_length() * 32) as i64;
        let origin = cube.origin().origin();
        let rect = Rect {
            x0: origin.x(),
            z0: origin.z(),
            size: edge,
        };
        add_to_horizon(
            &field,
            eye,
            &mut horizon,
            &rect,
            cube.level(),
            3,
            &mut asked,
        );
    }
    let mut sorted: Vec<f64> = horizon.iter().copied().filter(|h| h.is_finite()).collect();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q) as usize];
    eprintln!(
        "DEBUG eye {:.0} high; {} bins of {BINS} have a horizon; slopes: min {:.3}, 10% {:.3}, median {:.3}, 90% {:.3}, max {:.3}",
        eye[1],
        sorted.len(),
        at(0.0),
        at(0.1),
        at(0.5),
        at(0.9),
        at(1.0)
    );
    // And what the true horizon is, by the rays: the steepest ground seen in each direction out to 3000 voxels.
    let mut true_horizon = Vec::new();
    for bin in (0..BINS).step_by(64) {
        let angle = (bin as f64 + 0.5) / BINS as f64 * std::f64::consts::TAU;
        let mut steepest = f64::NEG_INFINITY;
        let mut d = 20.0;
        while d < 3000.0 {
            let level = level_at(d);
            let h = height_at_level(
                &noise,
                eye[0] + d * angle.cos(),
                eye[2] + d * angle.sin(),
                level,
            );
            steepest = steepest.max((h - eye[1]) / d);
            d += f64::from(1u32 << level).max(8.0);
        }
        true_horizon.push(steepest);
    }
    true_horizon.sort_by(|a, b| a.total_cmp(b));
    let t = |q: f64| true_horizon[((true_horizon.len() - 1) as f64 * q) as usize];
    eprintln!(
        "DEBUG true horizon (rays to 3000): min {:.3}, 10% {:.3}, median {:.3}, 90% {:.3}, max {:.3}",
        t(0.0),
        t(0.1),
        t(0.5),
        t(0.9),
        t(1.0)
    );
    // The tops the far cubes would need to be under: the global highest ground over the distance they are at.
    eprintln!(
        "DEBUG the highest the ground goes is {:.0}, so a cube 5000 away needs a horizon of at least {:.3}",
        BASE + 80.0,
        (BASE + 80.0 - eye[1]) / 5000.0
    );
}

pub fn the_dense_oracle_how_much_of_the_surface_is_really_hidden() {
    let noise = hills();
    let fixture = StandardTestLayout::default();
    let generator = FieldTerrainGenerator::new(
        Arc::new(Heightfield::new(BASE, hills())),
        fixture.material,
        MATERIALS,
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
        ("middling", places[places.len() / 2]),
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
            detail: settings(),
            zoom: None,
            fine_radius: 8,
            maximum_distance: 6_000.0,
        };
        let cubes = select_around_brick(brick, &view, &|cube| generator.may_show(cube));
        let (mut with_surface, mut all_hidden, mut points_total, mut points_hidden) =
            (0usize, 0usize, 0usize, 0usize);
        for cube in cubes.iter().filter(|cube| cube.level() > 0) {
            let origin = cube.origin().origin();
            let edge = (cube.edge_length() * 32) as f64;
            let (x0, y0, z0) = (origin.x() as f64, origin.y() as f64, origin.z() as f64);
            let (mut has, mut any_seen) = (false, false);
            for i in 0..8 {
                for j in 0..8 {
                    let (x, z) = (
                        x0 + edge * (0.0625 + 0.125 * f64::from(i)),
                        z0 + edge * (0.0625 + 0.125 * f64::from(j)),
                    );
                    let d = ((x - eye[0]).powi(2) + (z - eye[2]).powi(2)).sqrt();
                    let y = height_at_level(&noise, x, z, level_at(d));
                    if y >= y0 && y < y0 + edge {
                        has = true;
                        let h = hidden(&noise, eye, [x, y, z]);
                        points_total += 1;
                        points_hidden += usize::from(h);
                        any_seen |= !h;
                    }
                }
            }
            with_surface += usize::from(has);
            all_hidden += usize::from(has && !any_seen);
        }
        eprintln!(
            "DENSE {name} (eye {:.0} high), to 6000: {all_hidden} of {with_surface} cubes with surface have all of 64 places hidden ({:.0}%); {:.0}% of {points_total} places are hidden",
            ground + 2.0,
            100.0 * all_hidden as f64 / with_surface.max(1) as f64,
            100.0 * points_hidden as f64 / points_total.max(1) as f64
        );
    }
}
