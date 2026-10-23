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
    apply_updates, camera_at, configure_renderer, show_entity_boxes, show_other_players,
    show_screen,
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

    let mut renderer = VoxelRenderer::new(&context, wgpu::TextureFormat::Rgba8UnormSrgb);
    configure_renderer(
        &mut renderer,
        session.block_table(),
        session.package().ambient_light,
        view.view_radius,
    );

    let started = Instant::now();
    session.send_pose();
    let updates = session
        .settle(&mut step_server)
        .map_err(|error| error.to_string())?;
    apply_updates(&mut renderer, updates);
    println!(
        "loaded radius {} in {:.2} s: {} bricks held, {} meshes with {} quads",
        view.view_radius,
        started.elapsed().as_secs_f64(),
        session.mesher().loaded_brick_count(),
        renderer.brick_count(),
        renderer.total_quad_count(),
    );

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
        session.package().ambient_light,
        view.view_radius,
    );
    let updates = session.settle(|| {}).map_err(|error| error.to_string())?;
    apply_updates(&mut renderer, updates);
    render_outputs(&mut renderer, &mut session, view, false)
}

/// Writes the screenshot and runs the benchmark, as asked.
fn render_outputs<C: Connection, G: GameDefinition>(
    renderer: &mut VoxelRenderer,
    session: &mut Session<C, G>,
    view: &crate::options::ViewOptions,
    benchmark: bool,
) -> Result<(), String> {
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
        const WARM_UP_FRAMES: usize = 20;
        const MEASURED_FRAMES: usize = 200;
        let mut frame_times = Vec::with_capacity(MEASURED_FRAMES);
        let mut last_statistics = None;
        for frame in 0..WARM_UP_FRAMES + MEASURED_FRAMES {
            camera.look_by(0.01, 0.0); // turn slowly so culling and overdraw vary
            let frame_started = Instant::now();
            last_statistics = Some(renderer.render_offscreen(&camera, view.width, view.height));
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
            view.width,
            view.height,
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
