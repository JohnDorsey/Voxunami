//! Draws the far terrain made from columns with the real renderer: the pruned selection, every node of the hills made by the batch noise and the mesh made
//! straight from the heights, and nothing else (no bricks, so there is a hole under the eye). Writes `picture_valley.png`, `picture_ridge.png` and
//! `picture_high.png` to the current directory.

use std::fs::File;
use std::io::BufWriter;
use std::sync::Arc;

use coarse_cells::{DetailSettings, ViewSelection, select_around_brick};
use renderer::{AdapterPreference, Atmosphere, Camera, GraphicsContext, VoxelRenderer};
use terrain::{FieldTerrainGenerator, Heightfield, TerrainGenerator, TerrainMaterials};
use voxel_grid::test_support::StandardTestLayout;
use voxel_meshing::PackedBrickMesh;
use voxel_space::{BrickCoordinate, LevelOfDetail, VoxelCoordinate, WorldPosition};

use crate::columns::{BASE, MATERIALS, batch_hills, hills, mesh_tops_fast, tops_from_heights_fast};

const EDGE: usize = 32;
const WIDTH: u32 = 1600;
const HEIGHT: u32 = 900;

fn ground(x: f64, z: f64) -> f64 {
    BASE + hills().sample(
        VoxelCoordinate::new(x.floor() as i64, 0, z.floor() as i64),
        LevelOfDetail::FULL_DETAIL,
    )
}

fn write_png(name: &str, pixels: &[u8]) {
    let file = BufWriter::new(File::create(name).expect("create the picture"));
    let mut encoder = png::Encoder::new(file, WIDTH, HEIGHT);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("png header");
    writer.write_image_data(pixels).expect("png data");
}

pub fn draw_the_far_terrain_made_from_columns() {
    let context =
        GraphicsContext::headless(AdapterPreference::HighPerformance).expect("a graphics adapter");
    let fixture = StandardTestLayout::default();
    let generator = FieldTerrainGenerator::new(
        Arc::new(Heightfield::new(BASE, hills())),
        fixture.material,
        TerrainMaterials { ..MATERIALS },
    );
    // The lowest, the highest and a middling place in a field of 40 x 40 places 100 voxels apart.
    let mut places: Vec<(f64, f64, f64)> = Vec::new();
    for i in 0..40 {
        for j in 0..40 {
            let (x, z) = (f64::from(i) * 100.0 + 50.0, f64::from(j) * 100.0 + 50.0);
            places.push((ground(x, z), x, z));
        }
    }
    places.sort_by(|a, b| a.0.total_cmp(&b.0));
    let views = [
        ("valley", places[0], 2.0, -0.02, 0.3),
        ("ridge", places[places.len() - 1], 2.0, -0.03, 0.3),
        ("high", places[places.len() / 2], 500.0, -0.22, -1.1),
    ];
    for (name, (height, ex, ez), above, pitch, yaw) in views {
        let mut renderer = VoxelRenderer::new(&context, wgpu::TextureFormat::Rgba8UnormSrgb);
        renderer.set_palette(&[
            [0.0; 3],
            [0.5, 0.5, 0.52],
            [0.45, 0.32, 0.2],
            [0.28, 0.6, 0.22],
        ]);
        renderer.set_atmosphere(Atmosphere {
            sky_color: [0.55, 0.72, 0.92],
            fog_start: 4_000.0,
            fog_end: 24_000.0,
            sun_direction: [0.5, 0.8, 0.2],
            ambient_light: 60,
        });
        let eye_y = height + above;
        let brick = BrickCoordinate::new(
            (ex / 32.0).floor() as i64,
            (eye_y / 32.0).floor() as i64,
            (ez / 32.0).floor() as i64,
        );
        let view = ViewSelection {
            detail: DetailSettings {
                cell_pixels: 3.0,
                pixel_size: 2.0 * 0.700_207_538_209_062_3 / f64::from(HEIGHT),
            },
            zoom: None,
            fine_radius: 0,
            maximum_distance: 24_000.0,
        };
        let cubes = select_around_brick(brick, &view, &|cube| generator.may_show(cube));
        let mut nodes = 0;
        let mut quads = 0;
        for cube in cubes.iter().filter(|cube| cube.level() > 0) {
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
            let heights: Vec<f64> = batch.iter().map(|noise| BASE + noise).collect();
            let tops = tops_from_heights_fast(&heights, origin.y(), level);
            let mut packed = Vec::with_capacity(2048);
            mesh_tops_fast(&tops, &mut packed);
            if packed.is_empty() {
                continue;
            }
            nodes += 1;
            quads += packed.len();
            renderer.upload_coarse_node(
                *cube,
                &PackedBrickMesh {
                    quads: packed,
                    detailed: Vec::new(),
                },
            );
        }
        let mut camera = Camera::new(WorldPosition::new(
            brick,
            [
                (ex - brick.x() as f64 * 32.0) as f32,
                (eye_y - brick.y() as f64 * 32.0) as f32,
                (ez - brick.z() as f64 * 32.0) as f32,
            ],
        ));
        camera.yaw = yaw as f32;
        camera.pitch = pitch as f32;
        let (pixels, statistics) = renderer.render_to_rgba(&camera, WIDTH, HEIGHT);
        let file = format!("picture_{name}.png");
        write_png(&file, &pixels);
        eprintln!(
            "PICTURE {file}: eye {eye_y:.0} high; {nodes} nodes with something in them ({quads} quads) of {} cubes chosen; drawn {} nodes and {} quads",
            cubes.len(),
            statistics.coarse_nodes_drawn,
            statistics.quads_drawn
        );
    }
}
