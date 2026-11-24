//! A probe of 2026-11-22 (`docs/specs/viewpoints-spec.md` section 9i), kept as a record and not compiled with the workspace: what it costs the renderer to be given the commands a
//! switch of views makes (`upload_hidden_mesh`, `show_mesh`, `hide_mesh`), the frame that follows them, what a frame costs with meshes held and not drawn, and the memory of the
//! graphics card they take. Copy it to `engine/renderer/tests/gpu_rendering/probe_hide_show.rs`, add `mod probe_hide_show;` to `gpu_rendering.rs`, then
//! `VOX_REQUIRE_GPU=1 cargo test --release -p renderer --test gpu_rendering probe_hide_show -- --nocapture --test-threads=1`, and remove both again.

use std::time::{Duration, Instant};

use renderer::{HeldMesh, RenderStatistics};

use super::*;

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

/// The mesh of a node of hills, a patch of its 32 by 32 cells wide, so that its size is about that of the nodes of the Cave's planet (2 KB of quads, measured 2026-11-22: 42.9 MB held in 24,330 meshes).
fn a_node_mesh(patch: usize) -> voxel_meshing::PackedBrickMesh {
    coarse_mesh(|x, y, z| {
        let height = 14 + ((x / 2 + z / 3) % 3);
        (x < patch && z < patch && y <= height).then_some(RED)
    })
}

fn bytes_of(mesh: &voxel_meshing::PackedBrickMesh) -> usize {
    mesh.quads.len() * 8 + mesh.detailed.len() * 16
}

/// Where the nth node goes: a flat field of nodes of two bricks, `side` across.
fn node_at(index: usize, side: usize) -> HeldMesh {
    let (column, row) = ((index % side) as i128, (index / side) as i128);
    HeldMesh::Node(cube_of_bricks([2 * column, 0, 2 * row], 1))
}

fn camera_over(side: usize) -> Camera {
    let middle = (side as i128) * 32;
    let mut camera = camera_in_voxels([middle, middle * 11 / 10, middle]);
    camera.pitch = -1.5;
    camera
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// The dedicated and shared graphics memory this process holds, in megabytes, as Windows counts it (all adapters together), if it can be asked.
fn graphics_memory() -> Option<(f64, f64)> {
    let pid = std::process::id();
    let script = format!(
        "$s=(Get-Counter '\\GPU Process Memory(pid_{pid}*)\\Dedicated Usage','\\GPU Process Memory(pid_{pid}*)\\Shared Usage' -ErrorAction SilentlyContinue).CounterSamples; \
         $d=($s | Where-Object {{ $_.Path -like '*dedicated*' }} | Measure-Object CookedValue -Sum).Sum; \
         $h=($s | Where-Object {{ $_.Path -like '*shared*' }} | Measure-Object CookedValue -Sum).Sum; \
         Write-Output \"$d $h\""
    );
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut numbers = text.split_whitespace().filter_map(|word| word.parse::<f64>().ok());
    Some((numbers.next()? / 1.0e6, numbers.next()? / 1.0e6))
}

/// What wgpu's allocator says the device has allocated and reserved, in megabytes (what the buffers of the meshes take of the graphics card's memory, as far as the allocator knows).
fn allocator(context: &GraphicsContext) -> Option<(f64, f64)> {
    let report = context.device.generate_allocator_report()?;
    Some((
        report.total_allocated_bytes as f64 / 1e6,
        report.total_reserved_bytes as f64 / 1e6,
    ))
}

/// A frame, in milliseconds, and what it drew (the picture is read back, as everywhere in these tests).
fn frame(renderer: &mut VoxelRenderer, camera: &Camera) -> (f64, RenderStatistics) {
    let began = Instant::now();
    let (_, statistics) = renderer.render_to_rgba(camera, WIDTH, HEIGHT);
    (began.elapsed().as_secs_f64() * 1e3, statistics)
}

fn steady(renderer: &mut VoxelRenderer, camera: &Camera) -> f64 {
    let _ = frame(renderer, camera);
    median((0..15).map(|_| frame(renderer, camera).0).collect())
}

#[test]
fn probe_hide_show() {
    let counts: Vec<usize> = std::env::var("PROBE_COUNTS")
        .unwrap_or_else(|_| "1000,13342,24330,100000".to_string())
        .split(',')
        .map(|text| text.trim().parse().expect("a number"))
        .collect();
    let patch: usize = std::env::var("PROBE_PATCH")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(12);
    let mesh = a_node_mesh(patch);
    println!(
        "a mesh of the probe: {} quads and {} detailed, {} bytes (the Cave's planet: 1.76 KB a mesh on average)",
        mesh.quads.len(),
        mesh.detailed.len(),
        bytes_of(&mesh)
    );
    for context in contexts() {
        let name = adapter_name(&context);
        println!("\n=== {name}");
        for &count in &counts {
            let side = (count as f64).sqrt().ceil() as usize;
            let camera = camera_over(side);
            let mut renderer = renderer_without_fog(&context);
            let mut atmosphere = shadow_atmosphere(0, SUN_OVER_THE_EAST, true);
            (atmosphere.fog_start, atmosphere.fog_end) = (1.0e15, 2.0e15);
            renderer.set_atmosphere(atmosphere);
            let empty = steady(&mut renderer, &camera);
            let memory_before = graphics_memory();
            let allocator_before = allocator(&context);

            // Held, not drawn.
            let began = Instant::now();
            for index in 0..count {
                renderer.upload_hidden_mesh(node_at(index, side), &mesh);
            }
            let upload = began.elapsed();
            let memory_held = graphics_memory();
            let allocator_held = allocator(&context);
            let held_frame = steady(&mut renderer, &camera);
            let memory_after_frames = graphics_memory();
            let allocator_after_frames = allocator(&context);

            // The switch: all of them shown in one go.
            let began = Instant::now();
            let mut shown = 0;
            for index in 0..count {
                shown += usize::from(renderer.show_mesh(node_at(index, side)));
            }
            let show = began.elapsed();
            let (first_frame, first) = frame(&mut renderer, &camera);
            let drawn_frame = steady(&mut renderer, &camera);
            let (_, drawn) = frame(&mut renderer, &camera);

            // And all of them taken down again.
            let began = Instant::now();
            let mut hidden = 0;
            for index in 0..count {
                hidden += usize::from(renderer.hide_mesh(node_at(index, side)));
            }
            let hide = began.elapsed();
            let (after_hide_frame, after_hide) = frame(&mut renderer, &camera);
            let hidden_again = steady(&mut renderer, &camera);

            // Half and half: one view's pictures taken down and another's put up in one go (what a switch is).
            let half = count / 2;
            for index in 0..half {
                renderer.show_mesh(node_at(index, side));
            }
            let _ = steady(&mut renderer, &camera);
            let began = Instant::now();
            for index in 0..half {
                renderer.hide_mesh(node_at(index, side));
            }
            for index in half..count {
                renderer.show_mesh(node_at(index, side));
            }
            let swap = began.elapsed();
            let (swap_first_frame, _) = frame(&mut renderer, &camera);

            println!(
                "{count:>7} meshes ({:.1} MB of quads): upload hidden {:7.1} ms ({:5.1} us each) | {} hidden then | frame with none {empty:5.2} ms, with them held {held_frame:5.2} ms | show all {:6.2} ms ({:4.2} us each, {shown} moved), first frame {first_frame:6.2} ms ({} cascades redrawn), steady {drawn_frame:5.2} ms (drawn {}, culled {}) | hide all {:6.2} ms ({:4.2} us each, {hidden} moved), first frame {after_hide_frame:6.2} ms ({} cascades), steady {hidden_again:5.2} ms | swap half for half {:6.2} ms, first frame {swap_first_frame:6.2} ms",
                (count * bytes_of(&mesh)) as f64 / 1e6,
                upload.as_secs_f64() * 1e3,
                upload.as_secs_f64() * 1e6 / count as f64,
                renderer.hidden_mesh_count(),
                show.as_secs_f64() * 1e3,
                show.as_secs_f64() * 1e6 / count as f64,
                first.shadow_cascades_drawn,
                drawn.bricks_drawn,
                drawn.bricks_culled,
                hide.as_secs_f64() * 1e3,
                hide.as_secs_f64() * 1e6 / count as f64,
                after_hide.shadow_cascades_drawn,
                swap.as_secs_f64() * 1e3,
            );
            if let (Some(before), Some(held)) = (memory_before, memory_held) {
                println!(
                    "          graphics memory of the process: dedicated {:+.1} MB, shared {:+.1} MB for {count} held meshes",
                    held.0 - before.0,
                    held.1 - before.1
                );
            }
            if let (Some(before), Some(held), Some(after)) =
                (allocator_before, allocator_held, allocator_after_frames)
            {
                println!(
                    "          the allocator: allocated {:+.1} MB and reserved {:+.1} MB for {count} held meshes, and after some frames {:+.1} MB and {:+.1} MB",
                    held.0 - before.0,
                    held.1 - before.1,
                    after.0 - before.0,
                    after.1 - before.1
                );
            }
            if let (Some(before), Some(after)) = (memory_before, memory_after_frames) {
                println!(
                    "          graphics memory of the process after some frames: dedicated {:+.1} MB, shared {:+.1} MB",
                    after.0 - before.0,
                    after.1 - before.1
                );
            }
            drop(renderer);
            std::thread::sleep(Duration::from_millis(300));
        }
    }
}
