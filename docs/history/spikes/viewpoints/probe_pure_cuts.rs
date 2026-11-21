//! A probe of 2026-11-21 (`docs/specs/viewpoints-spec.md` section 3), kept as a record and not compiled with the workspace: the cubes that the rule of `lod-spec.md` 3.3 draws of Cave's planet for a viewer near the ground (a ship) and for a viewer far
//! To run it, copy it to `games/voxunami_cave/tests/`, then `cargo test --release -p voxunami_cave --test probe_pure_cuts -- --nocapture --test-threads=1`, and remove the copy (the code is of commit `aa419ee`).
//! from it (a map), worked out with no session and no history, and how much the two share.

use std::collections::{BTreeMap, HashSet};
use std::time::Instant;

use coarse_cells::{DetailSettings, ViewSelection, select_around_brick};
use region_octree::{BrickCube, ROOT_LEVEL};
use terrain::{SolidBall, TerrainGenerator};
use voxel_grid::GridLayout;
use voxel_space::{BrickBox, BrickCoordinate, WorldPosition};
use voxunami_cave::generators::{PlanetSettings, planet, surface_layers};

const EDGE: f64 = 32.0;
const MARGIN: f64 = EDGE * 1.733;

fn difference(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [first[0] - second[0], first[1] - second[1], first[2] - second[2]]
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

/// A copy of `world_view::horizon::is_hidden_by` (private there).
fn is_hidden_by(core: &SolidBall, viewer: [f64; 3], cube: BrickCube, margin: f64) -> bool {
    let edge = cube.edge_length() as f64 * EDGE;
    let origin = cube.origin();
    let centre = [
        origin.x() as f64 * EDGE + edge / 2.0,
        origin.y() as f64 * EDGE + edge / 2.0,
        origin.z() as f64 * EDGE + edge / 2.0,
    ];
    let sphere = edge / 2.0 * 3.0f64.sqrt() + margin;
    let radius = core.radius - margin;
    let to_core = difference(core.centre, viewer);
    let to_cube = difference(centre, viewer);
    let core_distance = dot(to_core, to_core).sqrt();
    let cube_distance = dot(to_cube, to_cube).sqrt();
    if radius <= 0.0 || core_distance <= radius || cube_distance <= sphere {
        return false;
    }
    let sine_of_cone = radius / core_distance;
    let sine_of_sphere = sphere / cube_distance;
    if sine_of_sphere >= sine_of_cone {
        return false;
    }
    let cosine_of_cone = (1.0 - sine_of_cone * sine_of_cone).sqrt();
    let cosine_of_sphere = (1.0 - sine_of_sphere * sine_of_sphere).sqrt();
    let cosine_of_limit = cosine_of_cone * cosine_of_sphere + sine_of_cone * sine_of_sphere;
    let cosine_of_angle = dot(to_core, to_cube) / (core_distance * cube_distance);
    let touching_distance = (core_distance * core_distance - radius * radius).sqrt();
    cosine_of_angle > cosine_of_limit && cube_distance - sphere > touching_distance
}

/// Kilobytes of mesh a cube of this level had in the real session (a measurement of `probe_two_views`: bytes of non-empty meshes over their number; level 0 is a brick of
/// planet in a cave, so a high figure).
fn kilobytes_of(level: u8) -> f64 {
    match level {
        0 => 11.0,
        1 => 1.5,
        2 => 1.6,
        3 => 1.7,
        4 => 1.8,
        5 => 2.2,
        6 => 3.5,
        7 => 3.4,
        8 => 4.7,
        9 => 4.3,
        _ => 4.0,
    }
}

fn cut(
    generator: &dyn TerrainGenerator,
    core: Option<SolidBall>,
    extent: BrickBox,
    position: [f64; 3],
    fine_radius: u32,
) -> Vec<BrickCube> {
    let brick = WorldPosition::new(BrickCoordinate::ORIGIN, [0.0; 3]);
    let _ = brick;
    let brick = BrickCoordinate::new(
        (position[0] / EDGE).floor() as i128,
        (position[1] / EDGE).floor() as i128,
        (position[2] / EDGE).floor() as i128,
    );
    let may_hold = |cube: BrickCube| {
        cube.meets(extent)
            && generator.may_show(cube)
            && !core.is_some_and(|core| is_hidden_by(&core, position, cube, MARGIN))
    };
    select_around_brick(
        brick,
        &ViewSelection {
            detail: DetailSettings {
                cell_pixels: 3.0,
                pixel_size: 2.0 * 0.700_207_538_209_062_3 / 1080.0,
            },
            zoom: None,
            fine_radius,
            maximum_distance: 1.0e7,
        },
        &may_hold,
    )
}

fn per_level(cubes: &[BrickCube]) -> BTreeMap<u8, usize> {
    let mut levels = BTreeMap::new();
    for cube in cubes {
        *levels.entry(cube.level()).or_default() += 1;
    }
    levels
}

fn megabytes(cubes: impl IntoIterator<Item = BrickCube>) -> f64 {
    cubes.into_iter().map(|cube| kilobytes_of(cube.level())).sum::<f64>() / 1024.0
}

fn holders(cubes: &HashSet<BrickCube>) -> HashSet<BrickCube> {
    let mut found = HashSet::new();
    for cube in cubes {
        for level in cube.level() + 1..=ROOT_LEVEL {
            found.insert(BrickCube::containing(cube.origin(), level));
        }
    }
    found
}

#[test]
fn what_a_ship_view_and_a_map_view_of_the_planet_share() {
    let base = base_blocks::BaseBlocks::new();
    let ids = base.ids();
    let layout = GridLayout::new(vec![ids.block_channel()]).expect("a layout");
    let channel = layout.channel_indices().next().expect("a channel");
    let generator = planet(
        PlanetSettings::one_percent_of_earth(2024),
        channel,
        surface_layers(&ids),
    );
    let core = generator.solid_core();
    println!("solid core: {core:?}");
    let extent = BrickBox::around(BrickCoordinate::ORIGIN, 2100);

    // Ship views: over the ground (about 490 m up), and in a low orbit-ish place (20 km up).
    let ships: Vec<(&str, [f64; 3])> = vec![
        ("ship 490 m up", [16.0, 64_200.0, 16.0]),
        ("ship 20 km up", [16.0, 83_710.0, 16.0]),
    ];
    let heights = [1.0e3, 3.0e3, 1.0e4, 3.0e4, 1.0e5, 3.0e5, 1.0e6, 3.0e6, 1.0e7];
    for (ship_name, ship_position) in ships {
        let began = Instant::now();
        let ship = cut(&generator, core, extent, ship_position, 12);
        println!(
            "\n== {ship_name}: {} cubes ({:.2} MB at the session's mesh sizes if all had a mesh) in {:.0} ms; per level {:?}",
            ship.len(),
            megabytes(ship.iter().copied()),
            began.elapsed().as_secs_f64() * 1000.0,
            per_level(&ship)
        );
        let ship_set: HashSet<BrickCube> = ship.iter().copied().collect();
        let ship_holders = holders(&ship_set);
        for height in heights {
            let map_position = [
                ship_position[0],
                ship_position[1] + height,
                ship_position[2],
            ];
            let began = Instant::now();
            let map = cut(&generator, core, extent, map_position, 12);
            let spent = began.elapsed().as_secs_f64() * 1000.0;
            let map_set: HashSet<BrickCube> = map.iter().copied().collect();
            let map_holders = holders(&map_set);
            // Of the map's cubes: the same as the ship's; coarser (hold some of the ship's); finer (inside one of the ship's); unrelated.
            let (mut same, mut over, mut under, mut apart) = (vec![], vec![], vec![], vec![]);
            for &cube in &map {
                if ship_set.contains(&cube) {
                    same.push(cube);
                } else if ship_holders.contains(&cube) {
                    over.push(cube);
                } else if (cube.level() + 1..=ROOT_LEVEL)
                    .any(|level| ship_set.contains(&BrickCube::containing(cube.origin(), level)))
                {
                    under.push(cube);
                } else {
                    apart.push(cube);
                }
            }
            // And the ship's cubes the map does not have (what stays resident only for the way back).
            let ship_only: Vec<BrickCube> = ship
                .iter()
                .copied()
                .filter(|cube| !map_set.contains(cube))
                .collect();
            let ship_only_inside_map = ship_only
                .iter()
                .filter(|cube| {
                    (cube.level() + 1..=ROOT_LEVEL)
                        .any(|level| map_set.contains(&BrickCube::containing(cube.origin(), level)))
                })
                .count();
            let _ = &map_holders;
            println!(
                "map {:>9.0} m higher: {:6} cubes ({:6.2} MB) {:3.0} ms | shared with ship {:6} ({:5.1}%) | map coarser over ship's {:5} | map finer inside ship's {:5} | map-only elsewhere {:6} | to build going out: {:6} cubes {:6.2} MB | ship-only (kept for the way back): {:6} cubes {:6.2} MB (of which {} lie inside a map cube) | levels {:?}",
                height,
                map.len(),
                megabytes(map.iter().copied()),
                spent,
                same.len(),
                100.0 * same.len() as f64 / map.len().max(1) as f64,
                over.len(),
                under.len(),
                apart.len(),
                over.len() + under.len() + apart.len(),
                megabytes(over.iter().chain(&under).chain(&apart).copied()),
                ship_only.len(),
                megabytes(ship_only.iter().copied()),
                ship_only_inside_map,
                per_level(&map)
            );
        }
    }
}
