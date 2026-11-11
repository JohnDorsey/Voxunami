//! Playing or replaying without a window: one screenshot, or a timing run.

use std::io::BufWriter;
use std::path::Path;
use std::time::Instant;

use client::ClientError;
use renderer::{GraphicsContext, VoxelRenderer, describe};
use transport::Connection;

use crate::definition::GameDefinition;
use crate::files::{open_recording, open_world, record_connection, recorded_game, save_world};
use crate::host::{build_server_with_ship_files, connect_to};
use crate::options::{PlayOptions, ReplayOptions};
use crate::screen::ScreenText;
use crate::session::{Mode, Session};
use crate::view::{
    apply_occupancy, apply_shader_pack, apply_updates, camera_at, configure_renderer,
    light_the_scene, prepare_shader_packs, show_entity_boxes, show_other_players, show_screen,
};
use world_storage::{DirectoryShipFiles, MemoryShipFiles, ShipFiles};

pub fn play<G: GameDefinition>(options: &PlayOptions) -> Result<(), String> {
    let view = &options.view;
    let context = GraphicsContext::headless(view.adapter).map_err(|error| error.to_string())?;
    println!("graphics adapter: {}", describe(&context.adapter));

    if options.host.is_some() {
        return Err(
            "--host needs the window or `voxunami serve`; a screenshot run does not stay up"
                .to_owned(),
        );
    }
    // Either a server in this process, which this function ticks, or one somewhere else.
    let identity = crate::player_identity::load(options.identity.as_deref())?;
    let mut content = crate::packs::content_with_packs::<G>(options.packs.as_deref())?;
    let mut local = None;
    let connection: Box<dyn Connection> = match &options.join {
        Some(address) => Box::new(connect_to(address)?),
        None => {
            let world = open_world(&mut content, options.world_file.as_deref(), options.seed)?;
            let ship_files: Box<dyn ShipFiles> = match &options.world_file {
                Some(path) => Box::new(DirectoryShipFiles::beside(path)),
                None => Box::new(MemoryShipFiles::new()),
            };
            let (server, connector) =
                build_server_with_ship_files::<G>(&content, world, view.view_radius, ship_files)
                    .map_err(|error| error.to_string())?;
            local = Some(server);
            Box::new(connector.connect().map_err(|error| error.to_string())?)
        }
    };
    let mut step_server = || match &mut local {
        Some(server) => {
            server.step();
        }
        None => std::thread::sleep(std::time::Duration::from_millis(1)),
    };
    let connection: Box<dyn Connection> = match &options.record {
        Some(path) => record_connection(connection, path)?,
        None => connection,
    };
    let start = camera_at(options.camera, options.yaw_degrees, options.pitch_degrees);
    let mut session = Session::<_, G>::connect(
        connection,
        &identity,
        Mode::Playing,
        view.view_radius,
        start,
        &mut step_server,
    )
    .map_err(|error| error.to_string())?;
    session.enable_level_detail();
    enable_sun_ray(&mut session);
    session.pin_time(view.time);
    session.set_shadows(view.shadows);
    if let Some(detail) = G::grid_detail() {
        session.enable_grid_detail(detail);
    }

    let mut renderer = VoxelRenderer::new(&context, wgpu::TextureFormat::Rgba8UnormSrgb);
    configure_renderer(
        &mut renderer,
        session.block_table(),
        session.package(),
        session.fog_radius_bricks(),
    );
    apply_face_switch(&mut renderer);
    let shader_packs = prepare_shader_packs(&options.view, &mut session)?;
    apply_shader_pack(&mut renderer, &shader_packs, session.settings())?;

    // (A measurement for `docs/specs/contact-trace-spec.md`, phase 0: how many bricks of each kind the sun's ray is told of.)
    let reports_occupancy = std::env::var_os(REPORT_OCCUPANCY).is_some();
    session.set_viewport_height(options.view.height);
    let started = Instant::now();
    session.send_pose();
    let updates = session
        .settle(&mut step_server)
        .map_err(|error| error.to_string())?;
    apply_updates(&mut renderer, updates);
    apply_occupancy(&mut renderer, &mut session);
    println!(
        "loaded radius {} in {:.2} s: {} bricks held, {} meshes with {} quads",
        view.view_radius,
        started.elapsed().as_secs_f64(),
        session.mesher().loaded_brick_count(),
        renderer.brick_count(),
        renderer.total_quad_count(),
    );
    if reports_occupancy {
        report_occupancy(&renderer, &session);
    }

    render_outputs(
        &mut renderer,
        &mut session,
        &options.view,
        options.benchmark,
    )?;

    session.say_goodbye();
    step_server();
    if let (Some(path), Some(server)) = (&options.world_file, local) {
        let mut world = server.into_world();
        save_world(&content, &mut world, path)?;
        println!("saved the world to {}", path.display());
    }
    Ok(())
}

pub fn replay<G: GameDefinition>(options: &ReplayOptions) -> Result<(), String> {
    let view = &options.view;
    let game = recorded_game(&options.file)?;
    if game != G::id() {
        return Err(format!(
            "this recording was made by {game}, not {}",
            G::id()
        ));
    }
    let connection: Box<dyn Connection> = Box::new(open_recording(&options.file)?);
    let start = camera_at([0.0, 0.0, 0.0], 0.0, 0.0);
    let mut session = Session::<_, G>::connect(
        connection,
        &crate::player_identity::for_replay(),
        Mode::Replaying,
        view.view_radius,
        start,
        || {},
    )
    .map_err(|error| error.to_string())?;
    session.enable_level_detail();
    enable_sun_ray(&mut session);
    session.pin_time(view.time);
    session.set_shadows(view.shadows);
    if let Some(detail) = G::grid_detail() {
        session.enable_grid_detail(detail);
    }
    let context = GraphicsContext::headless(view.adapter).map_err(|error| error.to_string())?;
    println!("graphics adapter: {}", describe(&context.adapter));

    let mut ended = false;
    while options
        .tick
        .is_none_or(|target| session.client().world().tick().get() < target)
    {
        match session.advance_one_tick() {
            Ok(_) => {}
            Err(ClientError::ConnectionClosed) => {
                ended = true;
                break;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    println!(
        "replayed to {}{}",
        session.client().world().tick(),
        if ended {
            " (the end of the recording)"
        } else {
            ""
        }
    );

    let mut renderer = VoxelRenderer::new(&context, wgpu::TextureFormat::Rgba8UnormSrgb);
    configure_renderer(
        &mut renderer,
        session.block_table(),
        session.package(),
        session.fog_radius_bricks(),
    );
    apply_face_switch(&mut renderer);
    let shader_packs = prepare_shader_packs(view, &mut session)?;
    apply_shader_pack(&mut renderer, &shader_packs, session.settings())?;
    session.set_viewport_height(view.height);
    let updates = session.settle(|| {}).map_err(|error| error.to_string())?;
    apply_updates(&mut renderer, updates);
    apply_occupancy(&mut renderer, &mut session);
    render_outputs(&mut renderer, &mut session, view, false)
}

/// The environment variable that makes a screenshot run count the bricks by what the sun's ray is told of them (empty, full, mixed), and say what that costs.
const REPORT_OCCUPANCY: &str = "VOX_REPORT_OCCUPANCY";

/// The environment variable that makes a screenshot or a benchmark run without the walk of the sun's ray through the voxels (`docs/specs/contact-trace-spec.md`): the shadows
/// are the map's alone, with the lit strip it leaves at the foot of a wall. For comparing pictures and times.
const WITHOUT_SUN_RAY: &str = "VOX_WITHOUT_SUN_RAY";

/// The environment variable that makes a screenshot or a benchmark run draw all six faces of every brick, as the renderer once did, and not only those that can look at the
/// camera. The pictures are the same; for comparing times.
const WITH_EVERY_FACE: &str = "VOX_WITH_EVERY_FACE";

/// The environment variable that makes a screenshot or a benchmark run leave the boxes of other players and items out of the walk of the sun's ray (section 16 of the spec):
/// their shadows are the map's alone, with the lit strip it leaves at their feet. For comparing pictures and times.
const WITHOUT_ENTITY_RAY: &str = "VOX_WITHOUT_ENTITY_RAY";

fn apply_face_switch(renderer: &mut VoxelRenderer) {
    if std::env::var_os(WITH_EVERY_FACE).is_some() {
        renderer.set_skip_faces_looking_away(false);
    }
    if std::env::var_os(WITHOUT_ENTITY_RAY).is_some() {
        renderer.set_entities_in_the_sun_walk(false);
    }
}

/// Makes the session tell the renderer which voxels block the sun, unless the run is asked to be without the walk.
fn enable_sun_ray<C: Connection, G: GameDefinition>(session: &mut Session<C, G>) {
    if std::env::var_os(WITHOUT_SUN_RAY).is_none() {
        session.enable_occupancy();
    }
}

fn report_occupancy<C: Connection, G: GameDefinition>(
    renderer: &VoxelRenderer,
    session: &Session<C, G>,
) {
    /// What a mixed brick costs on the GPU: a bit for each of its 32 by 32 by 32 voxels.
    const MIXED_BRICK_BYTES: usize = 4096;
    let held = renderer.occupancy_statistics();
    println!(
        "the renderer holds {} windows of voxels that block the sun: {} empty, {} full and {} mixed bricks, {} slots of the pool reached, {} bricks refused",
        held.windows, held.empty, held.full, held.mixed, held.slots_reached, held.rejected
    );
    let report = session.occupancy_statistics();
    let line = |label: String, counts: world_view::OccupancyStatistics| {
        println!(
            "occupancy of {label}: {} empty, {} full, {} mixed, {} shaped ({:.1} MB for the mixed and shaped)",
            counts.empty,
            counts.full,
            counts.mixed,
            counts.shaped,
            ((counts.mixed + counts.shaped) * MIXED_BRICK_BYTES) as f64 / 1.0e6
        );
    };
    line("the terrain of the level".to_owned(), report.terrain);
    for (id, counts) in report.planets {
        line(format!("planet {}", id.get()), counts);
    }
    line("the ships".to_owned(), report.ships);
}

/// Writes the screenshot and runs the benchmark, as asked.
fn render_outputs<C: Connection, G: GameDefinition>(
    renderer: &mut VoxelRenderer,
    session: &mut Session<C, G>,
    view: &crate::options::ViewOptions,
    benchmark: bool,
) -> Result<(), String> {
    light_the_scene(renderer, session);
    show_other_players(renderer, session);
    show_entity_boxes(renderer, session);
    // What the server has put on the screen (forms, a question) is in a picture of the session; the window's own status and
    // command line are not.
    show_screen(
        renderer,
        &session.screen(&ScreenText::default()),
        (view.width, view.height),
        None,
    );
    let mut camera = session.camera;
    if let Some(path) = &view.screenshot {
        renderer.set_crosshair(session.mode() == Mode::Playing && G::shows_crosshair());
        let (pixels, statistics) = renderer.render_to_rgba(&camera, view.width, view.height);
        write_png(path, view.width, view.height, &pixels)?;
        println!(
            "wrote {} ({}x{}); drew {} of {} bricks, {} quads",
            path.display(),
            view.width,
            view.height,
            statistics.bricks_drawn,
            statistics.bricks_drawn + statistics.bricks_culled,
            statistics.quads_drawn
        );
    }
    if benchmark {
        // Each wait is for a batch of frames, and a frame's time is its batch's over their number: a wait for one frame comes back in whole milliseconds on the
        // development machine (the medians of different pictures all fell on 11, 12 and 13 ms in turn), which hides a difference of a fraction of one.
        const WARM_UP_BATCHES: usize = 3;
        const MEASURED_BATCHES: usize = 30;
        let mut frame_times = Vec::with_capacity(MEASURED_BATCHES);
        let mut last_statistics = None;
        for batch in 0..WARM_UP_BATCHES + MEASURED_BATCHES {
            let batch_started = Instant::now();
            for _ in 0..FRAMES_PER_WAIT {
                camera.look_by(0.01, 0.0); // turn slowly so culling and overdraw vary
                last_statistics = Some(renderer.render_offscreen(&camera, view.width, view.height));
            }
            renderer.wait_for_gpu();
            if batch >= WARM_UP_BATCHES {
                frame_times
                    .push(batch_started.elapsed().as_secs_f64() * 1000.0 / FRAMES_PER_WAIT as f64);
            }
        }
        let statistics = last_statistics.expect("at least one frame was rendered");
        println!(
            "{}",
            frame_time_report(view.width, view.height, &mut frame_times)
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

/// How many frames the benchmark gives the GPU between one wait and the next.
const FRAMES_PER_WAIT: usize = 10;

/// The line the benchmark prints: the average, median, 95th percentile and worst of the times of a frame, each time that of a batch of [`FRAMES_PER_WAIT`] frames divided by
/// their number (so a spike of one frame is spread over its batch).
fn frame_time_report(width: u32, height: u32, frame_times: &mut [f64]) -> String {
    frame_times.sort_by(f64::total_cmp);
    let average = frame_times.iter().sum::<f64>() / frame_times.len() as f64;
    let percentile =
        |fraction: f64| frame_times[((frame_times.len() - 1) as f64 * fraction) as usize];
    format!(
        "{width}x{height} frame time over {} batches of {FRAMES_PER_WAIT} frames: average {average:.2} ms ({:.0} fps), median {:.2} ms, 95th percentile {:.2} ms, worst batch {:.2} ms",
        frame_times.len(),
        1000.0 / average,
        percentile(0.5),
        percentile(0.95),
        frame_times.last().expect("batches were measured"),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_gives_the_average_the_median_the_95th_percentile_and_the_worst_batch_whatever_the_order()
     {
        // 1 to 20 ms in a shuffled order: the average is 10.5, the median is the tenth (10.0), the 95th percentile the 19th index of 20 sorted (19.0 at 0.95 * 19 = 18), the worst 20.
        let mut times: Vec<f64> = (1..=20).map(f64::from).collect();
        times.reverse();
        times.swap(3, 11);
        assert_eq!(
            frame_time_report(1280, 720, &mut times),
            "1280x720 frame time over 20 batches of 10 frames: average 10.50 ms (95 fps), median 10.00 ms, 95th percentile 19.00 ms, worst batch 20.00 ms"
        );
    }

    #[test]
    fn a_single_batch_is_every_statistic() {
        assert_eq!(
            frame_time_report(64, 64, &mut [4.0]),
            "64x64 frame time over 1 batches of 10 frames: average 4.00 ms (250 fps), median 4.00 ms, 95th percentile 4.00 ms, worst batch 4.00 ms"
        );
    }
}
