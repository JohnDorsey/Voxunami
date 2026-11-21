//! A probe of 2026-11-21 (`docs/specs/viewpoints-spec.md` section 3), kept as a record and not compiled with the workspace: what a frame costs when the renderer holds N coarse nodes that nothing sees (they are behind the camera), against holding none,
//! To run it, copy it to `engine/renderer/tests/`, then `cargo test --release -p renderer --test probe_held_meshes -- --nocapture --test-threads=1`, and remove the copy (the code is of commit `aa419ee`).
//! with the shadows off and on; and what uploading N nodes costs. This is the cost of "kept in memory and not drawn" in a renderer whose `gather` walks everything it holds.

use std::time::Instant;

use renderer::{Atmosphere, Camera, GraphicsContext, VoxelRenderer};
use voxel_space::{BrickCoordinate, WorldPosition};

const RED: u32 = 1;

fn mesh(variation: usize) -> voxel_meshing::PackedBrickMesh {
    use coarse_cells::{CoarseCell, CoarseNode};
    let mut cells = Vec::with_capacity(32 * 32 * 32);
    for z in 0..32usize {
        for y in 0..32usize {
            for x in 0..32usize {
                let height = 4 + (x * 7 + z * 3 + variation) % 6;
                cells.push(if y < height {
                    CoarseCell {
                        block: RED,
                        fill: 255,
                        light: voxel_meshing::FULL_LIGHT,
                        exposure: 0,
                    }
                } else {
                    CoarseCell::empty(0, voxel_meshing::FULL_LIGHT)
                });
            }
        }
    }
    voxel_meshing::mesh_coarse_node(&CoarseNode::from_cells(cells), &[None; 6]).pack()
}

fn renderer_for(context: &GraphicsContext, shadows: bool) -> VoxelRenderer {
    let mut renderer = VoxelRenderer::new(context, wgpu::TextureFormat::Rgba8UnormSrgb);
    renderer.set_palette(&[[0.0, 0.0, 0.0], [0.9, 0.1, 0.1], [0.1, 0.8, 0.1]]);
    renderer.set_atmosphere(Atmosphere {
        sky_color: [0.2, 0.4, 0.8],
        fog_start: 1.0e15,
        fog_end: 2.0e15,
        sun_light: if shadows { 200 } else { 0 },
        sky_light: if shadows { 60 } else { 0 },
        shadows,
        ..Atmosphere::default()
    });
    renderer
}

fn cube_at(index: usize, side: usize, level: u8) -> region_octree::BrickCube {
    let edge = 1i128 << level;
    let x = (index % side) as i128 - (side / 2) as i128;
    let z = (index / side) as i128 - (side / 2) as i128;
    // Behind the camera (the camera looks along -z from z = 0): z positive.
    region_octree::BrickCube::new(
        BrickCoordinate::new(x * edge, -edge, (z.abs() + 1) * edge),
        level,
    )
    .unwrap()
}

fn frame_ms(renderer: &mut VoxelRenderer, camera: &Camera, frames: usize) -> (f64, renderer::RenderStatistics) {
    // Warm up, then take the median of `frames` frames.
    let mut statistics = renderer::RenderStatistics::default();
    for _ in 0..5 {
        statistics = renderer.render_to_rgba(camera, 64, 64).1;
    }
    let mut times: Vec<f64> = (0..frames)
        .map(|_| {
            let began = Instant::now();
            statistics = renderer.render_to_rgba(camera, 64, 64).1;
            began.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    times.sort_by(f64::total_cmp);
    (times[times.len() / 2], statistics)
}

#[test]
fn what_held_but_unseen_nodes_cost_a_frame() {
    let contexts = GraphicsContext::all_physical_adapters();
    assert!(!contexts.is_empty(), "no graphics adapter");
    let meshes: Vec<_> = (0..6).map(mesh).collect();
    println!(
        "a mesh: {} quads, {} bytes",
        meshes[0].quads.len(),
        meshes[0].quads.len() * 8
    );
    for context in &contexts {
        println!("\n=== {}", renderer::describe(&context.adapter));
        for shadows in [false, true] {
            let mut renderer = renderer_for(context, shadows);
            let camera = Camera::new(WorldPosition::new(BrickCoordinate::new(0, 8, 0), [0.0; 3]));
            let mut held = 0usize;
            let mut upload_total = 0.0;
            for target in [0usize, 1_000, 5_000, 20_000, 50_000] {
                let side = (target as f64).sqrt().ceil().max(1.0) as usize;
                let began = Instant::now();
                while held < target {
                    renderer.upload_coarse_node(cube_at(held, side.max(224), 3), &meshes[held % 6]);
                    held += 1;
                }
                let uploaded = began.elapsed().as_secs_f64() * 1000.0;
                upload_total += uploaded;
                let (ms, statistics) = frame_ms(&mut renderer, &camera, 15);
                println!(
                    "shadows {shadows:5}: {held:6} held (none seen: drawn {} culled {}), median frame {ms:7.2} ms (64 x 64 picture); the uploads since the last row took {uploaded:8.1} ms",
                    statistics.bricks_drawn, statistics.bricks_culled
                );
            }
            let _ = upload_total;
        }
    }
}
