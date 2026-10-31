//! How much of the terrain that is drawn is hidden behind nearer terrain, by rays through the heights as they are drawn.

use std::sync::Arc;

use coarse_cells::{DetailSettings, ViewSelection, select_around_brick};
use region_octree::BrickCube;
use terrain::{
    Dimensionality, FieldTerrainGenerator, FractalNoise, Heightfield, TerrainGenerator,
    TerrainMaterials,
};
use voxel_grid::test_support::StandardTestLayout;
use voxel_space::{BrickCoordinate, LevelOfDetail, VoxelCoordinate};

const MATERIALS: TerrainMaterials = TerrainMaterials {
    air: 0,
    stone: 1,
    dirt: 2,
    grass: 3,
};
const BASE: f64 = 64.0;

fn hills() -> FractalNoise {
    FractalNoise::standard(2024, Dimensionality::Planar, 7, 6, 40.0, 0.5)
}

fn settings() -> DetailSettings {
    DetailSettings {
        cell_pixels: 3.0,
        pixel_size: 2.0 * 0.700_207_538_209_062_3 / 1080.0,
    }
}

/// The detail level a cell is drawn at when it is `distance` voxels away: cells of about 3 pixels.
fn level_at(distance: f64) -> u8 {
    ((0.0039 * distance).max(1.0).log2().floor() as i64).clamp(0, 30) as u8
}

/// The ground at full detail, or as it is drawn at a level.
fn height_at_level(noise: &FractalNoise, x: f64, z: f64, level: u8) -> f64 {
    BASE + noise.sample(
        VoxelCoordinate::new(x.floor() as i64, 0, z.floor() as i64),
        LevelOfDetail::new(level).unwrap(),
    )
}

fn height(noise: &FractalNoise, x: f64, z: f64) -> f64 {
    height_at_level(noise, x, z, 0)
}

/// Whether the ground as it is drawn hides the point from the eye: the ray goes under the surface of the cells drawn at that distance, by more than half a cell
/// (so that a cell stepped up or down by rounding does not hide what it should not).
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

pub fn how_much_of_what_is_drawn_is_hidden_by_nearer_ground() {
    let noise = hills();
    let fixture = StandardTestLayout::default();
    let generator = FieldTerrainGenerator::new(
        Arc::new(Heightfield::new(BASE, hills())),
        fixture.material,
        MATERIALS,
    );
    // Eyes: the lowest, the highest and a middling place in a field of 40 x 40 places 100 voxels apart.
    let mut places: Vec<(f64, f64, f64)> = Vec::new();
    for i in 0..40 {
        for j in 0..40 {
            let (x, z) = (f64::from(i) * 100.0 + 50.0, f64::from(j) * 100.0 + 50.0);
            places.push((height(&noise, x, z), x, z));
        }
    }
    places.sort_by(|a, b| a.0.total_cmp(&b.0));
    let eyes = [
        ("valley", places[0]),
        ("middling", places[places.len() / 2]),
        ("ridge", places[places.len() - 1]),
    ];
    let bands = [
        (0.0, 300.0),
        (300.0, 1000.0),
        (1000.0, 3000.0),
        (3000.0, 10_000.0),
        (10_000.0, 20_000.0),
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
        let mut count = vec![(0usize, 0usize, 0usize); bands.len()];
        for cube in cubes.iter().filter(|cube| cube.level() > 0) {
            let origin = cube.origin().origin();
            let edge = (cube.edge_length() * 32) as f64;
            let (x0, y0, z0) = (origin.x() as f64, origin.y() as f64, origin.z() as f64);
            // Nine places on the surface that are in the cube.
            let mut points = Vec::new();
            for i in 0..3 {
                for j in 0..3 {
                    let (x, z) = (
                        x0 + edge * (0.1 + 0.4 * f64::from(i)),
                        z0 + edge * (0.1 + 0.4 * f64::from(j)),
                    );
                    let distance_here = ((x - eye[0]).powi(2) + (z - eye[2]).powi(2)).sqrt();
                    let y = height_at_level(&noise, x, z, level_at(distance_here));
                    if y >= y0 && y < y0 + edge {
                        points.push([x, y, z]);
                    }
                }
            }
            if points.is_empty() {
                continue;
            }
            let distance =
                ((x0 + edge / 2.0 - eye[0]).powi(2) + (z0 + edge / 2.0 - eye[2]).powi(2)).sqrt();
            let band = bands
                .iter()
                .position(|(low, high)| distance >= *low && distance < *high)
                .unwrap_or(bands.len() - 1);
            let hidden_points = points
                .iter()
                .filter(|point| hidden(&noise, eye, **point))
                .count();
            count[band].0 += 1;
            count[band].1 += usize::from(hidden_points == points.len());
            count[band].2 += usize::from(hidden_points * 2 > points.len());
        }
        for (band, (low, high)) in bands.iter().enumerate() {
            let (total, all_hidden, mostly_hidden) = count[band];
            eprintln!(
                "OCCLUSION {name} (eye {:.0} high): {low:.0} to {high:.0} voxels: {total} cubes with surface, {all_hidden} wholly hidden ({:.0}%), {mostly_hidden} mostly hidden ({:.0}%)",
                ground + 2.0,
                100.0 * all_hidden as f64 / total.max(1) as f64,
                100.0 * mostly_hidden as f64 / total.max(1) as f64,
            );
        }
    }
    let _: Option<BrickCube> = None;
}

pub fn sanity_how_much_of_the_ground_around_is_seen() {
    let noise = hills();
    for (name, ex, ez) in [
        ("a", 1050.0, 2050.0),
        ("b", 3350.0, 1250.0),
        ("c", 777.0, 3333.0),
    ] {
        let ground = height(&noise, ex, ez);
        let eye = [ex, ground + 2.0, ez];
        for (low, high) in [(100.0, 300.0), (300.0, 1000.0), (1000.0, 3000.0)] {
            let (mut seen, mut total) = (0, 0);
            for i in 0..2000u64 {
                let angle = (i as f64) * 2.399_963; // golden angle
                let fraction = ((i * 7919) % 1000) as f64 / 1000.0;
                let distance = low + (high - low) * fraction;
                let (x, z) = (ex + distance * angle.cos(), ez + distance * angle.sin());
                let d = ((x - ex).powi(2) + (z - ez).powi(2)).sqrt();
                let y = height_at_level(&noise, x, z, level_at(d));
                total += 1;
                seen += usize::from(!hidden(&noise, eye, [x, y, z]));
            }
            eprintln!(
                "SANITY {name} eye {:.0} high: {low:.0} to {high:.0}: {seen} of {total} places on the ground are seen",
                ground + 2.0
            );
        }
    }
}
