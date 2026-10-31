//! What pruning does to the cubes a view is made of, what the walk costs, and whether a cube that is left out ever has anything in it.

use std::cell::Cell;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use coarse_cells::{DetailSettings, ViewSelection, select_around_brick};
use region_octree::BrickCube;
use terrain::{
    Dimensionality, FieldTerrainGenerator, FractalNoise, Heightfield, RollingHillsSettings,
    TerrainGenerator, TerrainMaterials, rolling_hills,
};
use voxel_grid::test_support::StandardTestLayout;
use voxel_space::BrickCoordinate;

const MATERIALS: TerrainMaterials = TerrainMaterials {
    air: 0,
    stone: 1,
    dirt: 2,
    grass: 3,
};

fn settings() -> DetailSettings {
    DetailSettings {
        cell_pixels: 3.0,
        pixel_size: 2.0 * 0.700_207_538_209_062_3 / 1080.0,
    }
}

fn by_level(cubes: &[BrickCube]) -> Vec<usize> {
    let mut counts = vec![0usize; 31];
    for cube in cubes {
        counts[usize::from(cube.level())] += 1;
    }
    while counts.last() == Some(&0) {
        counts.pop();
    }
    counts
}

fn generators() -> Vec<(&'static str, FieldTerrainGenerator)> {
    let fixture = StandardTestLayout::default();
    let settings = RollingHillsSettings::for_one_metre_voxels(2024);
    let hills_only = FieldTerrainGenerator::new(
        Arc::new(Heightfield::new(
            settings.surface_height,
            FractalNoise::standard(
                settings.seed,
                Dimensionality::Planar,
                settings.largest_hill_wavelength_log2,
                settings.hill_octave_count,
                settings.hill_height,
                0.5,
            ),
        )),
        fixture.material,
        MATERIALS,
    );
    let flat = FieldTerrainGenerator::new(
        Arc::new(Heightfield::new(10.0, FractalNoise::none())),
        fixture.material,
        MATERIALS,
    );
    vec![
        ("flat ground", flat),
        ("hills", hills_only),
        (
            "hills with caves",
            rolling_hills(settings, fixture.material, MATERIALS),
        ),
    ]
}

pub fn how_many_cubes_and_how_long_with_and_without_pruning() {
    for (name, generator) in generators() {
        let centre = BrickCoordinate::new(0, 3, 0);
        for distance in [
            700.0,
            1_500.0,
            20_000.0,
            200_000.0,
            2_000_000.0,
            20_000_000.0,
            100_000_000.0,
        ] {
            let view = ViewSelection {
                detail: settings(),
                zoom: None,
                fine_radius: 8,
                maximum_distance: distance,
            };
            let start = Instant::now();
            let all = select_around_brick(centre, &view, &|_| true);
            let unpruned_time = start.elapsed();
            let questions = Cell::new(0u64);
            let start = Instant::now();
            let kept = select_around_brick(centre, &view, &|cube| {
                questions.set(questions.get() + 1);
                generator.may_show(cube)
            });
            let pruned_time = start.elapsed();
            eprintln!(
                "PRUNE {name} to {distance}: {} cubes in {:?} unpruned; {} cubes in {:?} pruned with {} questions ({:?} each); by level {:?}",
                all.len(),
                unpruned_time,
                kept.len(),
                pruned_time,
                questions.get(),
                pruned_time / questions.get().max(1) as u32,
                by_level(&kept),
            );
        }
    }
}

pub fn no_cube_that_pruning_drops_has_anything_in_it() {
    for (name, generator) in generators() {
        let centre = BrickCoordinate::new(0, 3, 0);
        let distances: &[f64] = if name == "hills with caves" {
            &[700.0]
        } else {
            &[700.0, 1_500.0, 6_000.0]
        };
        for &distance in distances {
            let view = ViewSelection {
                detail: settings(),
                zoom: None,
                fine_radius: 8,
                maximum_distance: distance,
            };
            let all = select_around_brick(centre, &view, &|_| true);
            let kept = select_around_brick(centre, &view, &|cube| generator.may_show(cube));
            let kept_set: HashSet<BrickCube> = kept.iter().copied().collect();
            let mut dropped = 0usize;
            let mut dropped_air = 0usize;
            let mut dropped_solid = 0usize;
            let mut wrong = Vec::new();
            let every = if name == "hills with caves" { 7 } else { 1 };
            for cube in all
                .iter()
                .filter(|cube| cube.level() > 0 && !kept_set.contains(cube))
                .step_by(every)
            {
                dropped += 1;
                let node = generator
                    .coarse_node(*cube, 255)
                    .expect("a field generator makes a node");
                match node.uniform_cell() {
                    Some(cell) if cell.fill == 0 => dropped_air += 1,
                    Some(cell) if cell.fill == 255 => dropped_solid += 1,
                    _ => wrong.push(*cube),
                }
            }
            // Cubes that are kept: how many are of nothing, of ground, of both.
            let (mut air, mut solid, mut mixed) = (0usize, 0usize, 0usize);
            for cube in kept.iter().filter(|cube| cube.level() > 0).step_by(every) {
                let node = generator.coarse_node(*cube, 255).unwrap();
                match node.uniform_cell() {
                    Some(cell) if cell.fill == 0 => air += 1,
                    Some(_) => solid += 1,
                    None => mixed += 1,
                }
            }
            eprintln!(
                "DROPPED {name} to {distance}: {} of {} cubes dropped ({dropped_air} air, {dropped_solid} solid, {} with something in them); kept nodes: {air} air, {solid} solid, {mixed} mixed",
                dropped,
                all.len(),
                wrong.len(),
            );
            assert!(
                wrong.is_empty(),
                "{name}: {:?}",
                &wrong[..wrong.len().min(5)]
            );
        }
    }
}

/// How the number of cubes of a pruned view of the hills depends on how many pixels a cell is drawn at (the setting that is the quality of the far view).
pub fn how_the_cell_size_changes_the_cube_count() {
    for (name, generator) in generators()
        .into_iter()
        .filter(|(name, _)| *name == "hills")
    {
        let centre = BrickCoordinate::new(0, 3, 0);
        for cell_pixels in [1.5, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0] {
            let mut line = format!("DETAIL {name}, cells of {cell_pixels} pixels:");
            for distance in [1_500.0, 20_000.0, 1_000_000.0, 100_000_000.0] {
                let view = ViewSelection {
                    detail: DetailSettings {
                        cell_pixels,
                        pixel_size: 2.0 * 0.700_207_538_209_062_3 / 1080.0,
                    },
                    zoom: None,
                    fine_radius: 8,
                    maximum_distance: distance,
                };
                let kept = select_around_brick(centre, &view, &|cube| generator.may_show(cube));
                let nodes = kept.iter().filter(|cube| cube.level() > 0).count();
                let bricks = kept.len() - nodes;
                line += &format!(" to {distance:.0}: {nodes} nodes + {bricks} bricks;");
            }
            eprintln!("{line}");
        }
    }
}
