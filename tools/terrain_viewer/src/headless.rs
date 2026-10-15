//! Rendering without a window: one screenshot, or a timing run.

use std::io::BufWriter;
use std::path::Path;
use std::time::Instant;

use renderer::{GraphicsContext, VoxelRenderer, describe};

use crate::options::Options;
use crate::scene::{apply_updates, configure_renderer, initial_camera, new_streamer};

pub fn run(options: &Options) -> Result<(), String> {
    let context = GraphicsContext::headless(options.adapter).map_err(|error| error.to_string())?;
    println!("graphics adapter: {}", describe(&context.adapter));
    let mut renderer = VoxelRenderer::new(&context, wgpu::TextureFormat::Rgba8UnormSrgb);
    configure_renderer(&mut renderer, options);
    let mut streamer = new_streamer(options);
    let mut camera = initial_camera(options);

    let started = Instant::now();
    apply_updates(&mut renderer, streamer.settle(camera.position.brick()));
    println!(
        "loaded radius {} in {:.2} s: {} bricks generated, {} meshes with {} quads",
        options.view_radius,
        started.elapsed().as_secs_f64(),
        streamer.loaded_brick_count(),
        renderer.brick_count(),
        renderer.total_quad_count(),
    );

    if let Some(path) = &options.screenshot {
        let (pixels, statistics) = renderer.render_to_rgba(&camera, options.width, options.height);
        write_png(path, options.width, options.height, &pixels)?;
        println!(
            "wrote {} ({}x{}); drew {} of {} bricks, {} quads",
            path.display(),
            options.width,
            options.height,
            statistics.bricks_drawn,
            statistics.bricks_drawn + statistics.bricks_culled,
            statistics.quads_drawn
        );
    }

    if options.benchmark {
        const WARM_UP_FRAMES: usize = 20;
        const MEASURED_FRAMES: usize = 200;
        let mut frame_times = Vec::with_capacity(MEASURED_FRAMES);
        let mut last_statistics = None;
        for frame in 0..WARM_UP_FRAMES + MEASURED_FRAMES {
            camera.look_by(0.01, 0.0); // turn slowly so culling and overdraw vary
            let frame_started = Instant::now();
            last_statistics =
                Some(renderer.render_offscreen(&camera, options.width, options.height));
            renderer.wait_for_gpu();
            if frame >= WARM_UP_FRAMES {
                frame_times.push(frame_started.elapsed().as_secs_f64() * 1000.0);
            }
        }
        frame_times.sort_by(f64::total_cmp);
        let average = frame_times.iter().sum::<f64>() / frame_times.len() as f64;
        let percentile =
            |fraction: f64| frame_times[((frame_times.len() - 1) as f64 * fraction) as usize];
        let statistics = last_statistics.expect("at least one frame was rendered");
        println!(
            "{}x{} frame time over {} frames: average {:.2} ms ({:.0} fps), median {:.2} ms, 95th percentile {:.2} ms, worst {:.2} ms",
            options.width,
            options.height,
            frame_times.len(),
            average,
            1000.0 / average,
            percentile(0.5),
            percentile(0.95),
            frame_times.last().expect("frames were measured"),
        );
        println!(
            "last frame drew {} of {} bricks and {} quads",
            statistics.bricks_drawn,
            statistics.bricks_drawn + statistics.bricks_culled,
            statistics.quads_drawn
        );
    }
    Ok(())
}

pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file =
        std::fs::File::create(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(rgba))
        .map_err(|error| format!("{}: {error}", path.display()))
}
