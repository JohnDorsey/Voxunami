//! SPIKE (`docs/history/parallax-spike.md`): what the three kinds of parallax look like and cost on a floor of dirt. Both tests are ignored (they are for looking and measuring, and
//! assert nothing about the picture); run them with
//! `cargo test -p renderer --test gpu_rendering --release parallax_spike -- --ignored --nocapture`.
//! The sheet is written to `VOX_SPIKE_OUT` (a directory; `E:/s/vx/parallax` if unset): one PNG for each adapter, a row for each view (the eye 65, 40 and 22 degrees over the
//! floor) and a column for each mode: none, offset limiting, parallax occlusion (smooth), prisms, parallax occlusion at twice the depth, prisms at twice the depth.
use super::emission::table;
use super::*;

const DIRT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../plugins/base_blocks/textures/dirt.png"
);
const DIRT_NORMAL: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../plugins/base_blocks/textures/dirt_n.png"
);

/// A mode of the spike (`VoxelRenderer::set_parallax_spike`): its name, the mode, and the depth of the relief in voxels (0 for the default, a tenth).
const MODES: [(&str, u32, f32); 6] = [
    ("none", 0, 0.0),
    ("offset limiting", 1, 0.0),
    ("parallax occlusion", 2, 0.0),
    ("prisms", 3, 0.0),
    ("parallax occlusion, twice as deep", 2, 0.2),
    ("prisms, twice as deep", 3, 0.2),
];

fn decode(path: &str) -> Vec<u8> {
    let file = std::fs::File::open(path).expect("picture file");
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = decoder.read_info().expect("png header");
    let mut buffer = vec![0; reader.output_buffer_size().expect("size")];
    let info = reader.next_frame(&mut buffer).expect("png frame");
    assert_eq!(info.color_type, png::ColorType::Rgba, "{path}");
    assert_eq!(info.bit_depth, png::BitDepth::Eight, "{path}");
    buffer.truncate(info.buffer_size());
    buffer
}

/// A floor of 32 by 32 blocks of dirt (the real pictures) seen in a sun that is 30 degrees over the floor, from the right and the front, with some sky and ambient light and no shadow maps.
fn floor(context: &GraphicsContext, picture: &[u8], normal: &[u8]) -> VoxelRenderer {
    let table = table();
    let mut renderer = new_renderer(context);
    renderer.set_palette_with_pictures(
        &table.palette(),
        &table.palette_pictures(),
        &[PictureSize {
            size: 16,
            tiles: vec![picture],
            normal: vec![Some(normal)],
            specular: vec![None],
        }],
    );
    renderer.set_picture_distance(100.0);
    let blocks: Vec<([u8; 3], u32, u32)> = (0..32u8)
        .flat_map(|x| (0..32u8).map(move |z| ([x, 16, z], 1, 0)))
        .collect();
    place_blocks_in(&mut renderer, &table, ORIGIN_BRICK, &blocks, Some(0));
    renderer.set_atmosphere(Atmosphere {
        sun_direction: [0.5, 0.5, 0.7],
        sun_light: 255,
        sky_light: 90,
        ambient_light: 40,
        shadows: false,
        ..Atmosphere::default()
    });
    renderer
}

/// A camera `distance` voxels from the point on the floor at (16, 17, 16), `elevation` degrees over it, in front of it (toward +z) and looking at it, or round to the side by
/// `azimuth` degrees (90 is from +x).
fn view(elevation: f64, azimuth: f64, distance: f64, field_of_view: f32) -> Camera {
    view_of(
        [16.0, 17.0, 16.0],
        elevation,
        azimuth,
        distance,
        field_of_view,
    )
}

fn view_of(
    target: [f64; 3],
    elevation: f64,
    azimuth: f64,
    distance: f64,
    field_of_view: f32,
) -> Camera {
    let radians = elevation.to_radians();
    let round = azimuth.to_radians();
    let eye = [
        target[0] + distance * libm::cos(radians) * libm::sin(round),
        target[1] + distance * libm::sin(radians),
        target[2] + distance * libm::cos(radians) * libm::cos(round),
    ];
    let mut camera = camera_aimed_at(eye, target);
    camera.vertical_field_of_view = field_of_view;
    camera
}

fn write_png(path: &std::path::Path, width: u32, height: u32, pixels: &[u8]) {
    let file = std::fs::File::create(path).expect("sheet file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .expect("png header")
        .write_image_data(pixels)
        .expect("png data");
}

#[test]
#[ignore = "a spike: for looking at"]
fn parallax_spike_sheet() {
    const TILE: (u32, u32) = (640, 400);
    let directory = std::env::var_os("VOX_SPIKE_OUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("E:/s/vx/parallax"));
    std::fs::create_dir_all(&directory).expect("sheet directory");
    let (dirt, normal) = (decode(DIRT), decode(DIRT_NORMAL));
    for context in contexts() {
        let name: String = adapter_name(&context)
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        let mut renderer = floor(&context, &dirt, &normal);
        let elevations = [65.0, 40.0, 22.0];
        // Two sheets of three modes each (the sheets are wide enough as they are): the three kinds, and then the offset and the two at twice the depth.
        for (sheet_name, chosen) in [("a", [0usize, 2, 3]), ("b", [1, 4, 5])] {
            let (width, height) = (TILE.0 * 3, TILE.1 * elevations.len() as u32);
            let mut sheet = vec![0u8; (width * height * 4) as usize];
            for (row, elevation) in elevations.iter().enumerate() {
                let camera = view(*elevation, 0.0, 1.2, 0.38);
                for (column, mode_index) in chosen.iter().enumerate() {
                    let (_, mode, depth) = MODES[*mode_index];
                    renderer.set_parallax_spike(mode, depth);
                    // (Two frames: the second, for the integrated GPU's first frames.)
                    renderer.render_to_rgba(&camera, TILE.0, TILE.1);
                    let image = renderer.render_to_rgba(&camera, TILE.0, TILE.1).0;
                    for y in 0..TILE.1 {
                        let from = (y * TILE.0 * 4) as usize;
                        let to = (((row as u32 * TILE.1 + y) * width + column as u32 * TILE.0) * 4)
                            as usize;
                        sheet[to..to + (TILE.0 * 4) as usize]
                            .copy_from_slice(&image[from..from + (TILE.0 * 4) as usize]);
                    }
                }
            }
            write_png(
                &directory.join(format!("sheet_{sheet_name}_{name}.png")),
                width,
                height,
                &sheet,
            );
            eprintln!(
                "SHEET {sheet_name} {name}: columns {:?}",
                chosen.map(|index| MODES[index].0)
            );
        }
    }
}

/// A check of the geometry: a picture whose relief is one square plateau (4 by 4 of its 16 by 16 pixels, at the top, the rest at the lowest), seen from the four sides. A mode that
/// puts the walls and the plateau where they are shows the plateau's near wall from every side, lit as the sun says (the sun is over +x and +z).
#[test]
#[ignore = "a spike: for looking at"]
fn parallax_spike_plateaus() {
    const TILE: (u32, u32) = (480, 300);
    let directory = std::env::var_os("VOX_SPIKE_OUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("E:/s/vx/parallax"));
    std::fs::create_dir_all(&directory).expect("sheet directory");
    let dirt = decode(DIRT);
    let plateau: Vec<u8> = (0..16 * 16)
        .flat_map(|index| {
            let (column, row) = (index % 16, index / 16);
            let top = (6..10).contains(&column) && (6..10).contains(&row);
            [128, 128, 255, if top { 255 } else { 0 }]
        })
        .collect();
    for context in contexts() {
        let name: String = adapter_name(&context)
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        let mut renderer = floor(&context, &dirt, &plateau);
        let chosen = [0usize, 4, 5];
        let azimuths = [0.0, 90.0, 180.0, 270.0];
        let (width, height) = (TILE.0 * 3, TILE.1 * azimuths.len() as u32);
        let mut sheet = vec![0u8; (width * height * 4) as usize];
        for (row, azimuth) in azimuths.iter().enumerate() {
            let camera = view(38.0, *azimuth, 1.5, 0.5);
            for (column, mode_index) in chosen.iter().enumerate() {
                let (_, mode, depth) = MODES[*mode_index];
                renderer.set_parallax_spike(mode, depth);
                renderer.render_to_rgba(&camera, TILE.0, TILE.1);
                let image = renderer.render_to_rgba(&camera, TILE.0, TILE.1).0;
                for y in 0..TILE.1 {
                    let from = (y * TILE.0 * 4) as usize;
                    let to =
                        (((row as u32 * TILE.1 + y) * width + column as u32 * TILE.0) * 4) as usize;
                    sheet[to..to + (TILE.0 * 4) as usize]
                        .copy_from_slice(&image[from..from + (TILE.0 * 4) as usize]);
                }
            }
        }
        write_png(
            &directory.join(format!("plateaus_{name}.png")),
            width,
            height,
            &sheet,
        );
        eprintln!(
            "SHEET plateaus {name}: columns {:?}, rows seen from +z, +x, -z, -x",
            chosen.map(|index| MODES[index].0)
        );
    }
}

/// Which walls of a plateau are lit: the eye is between +x and +z (so the plateau's +x and +z walls are the ones it sees), and the sun is over +x alone in the first row and over +z
/// alone in the second. The wall that faces the sun must be the bright one.
#[test]
#[ignore = "a spike: for looking at"]
fn parallax_spike_walls() {
    const TILE: (u32, u32) = (640, 400);
    let directory = std::env::var_os("VOX_SPIKE_OUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("E:/s/vx/parallax"));
    std::fs::create_dir_all(&directory).expect("sheet directory");
    let dirt = decode(DIRT);
    // A single plateau of one colour (the walls take the colour of their column's pixel, so a plain top and plain floor make the lighting the only thing to see).
    let plain: Vec<u8> = (0..16 * 16).flat_map(|_| [200, 200, 200, 255]).collect();
    let plateau: Vec<u8> = (0..16 * 16)
        .flat_map(|index| {
            let (column, row) = (index % 16, index / 16);
            let top = (5..11).contains(&column) && (5..11).contains(&row);
            [128, 128, 255, if top { 255 } else { 0 }]
        })
        .collect();
    let _ = dirt;
    for context in contexts() {
        let name: String = adapter_name(&context)
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        let mut renderer = floor(&context, &plain, &plateau);
        let suns = [[1.0f32, 0.55, 0.0], [0.0, 0.55, 1.0]];
        let (width, height) = (TILE.0 * 2, TILE.1 * suns.len() as u32);
        let mut sheet = vec![0u8; (width * height * 4) as usize];
        for (row, sun) in suns.iter().enumerate() {
            renderer.set_atmosphere(Atmosphere {
                sun_direction: *sun,
                sun_light: 255,
                sky_light: 40,
                ambient_light: 20,
                shadows: false,
                ..Atmosphere::default()
            });
            let camera = view_of([16.5, 16.9, 16.5], 35.0, 45.0, 1.0, 0.45);
            for (column, (mode, depth)) in [(0u32, 0.0f32), (3, 0.2)].iter().enumerate() {
                renderer.set_parallax_spike(*mode, *depth);
                renderer.render_to_rgba(&camera, TILE.0, TILE.1);
                let image = renderer.render_to_rgba(&camera, TILE.0, TILE.1).0;
                for y in 0..TILE.1 {
                    let from = (y * TILE.0 * 4) as usize;
                    let to =
                        (((row as u32 * TILE.1 + y) * width + column as u32 * TILE.0) * 4) as usize;
                    sheet[to..to + (TILE.0 * 4) as usize]
                        .copy_from_slice(&image[from..from + (TILE.0 * 4) as usize]);
                }
            }
        }
        write_png(
            &directory.join(format!("walls_{name}.png")),
            width,
            height,
            &sheet,
        );
        eprintln!("SHEET walls {name}: columns none and prisms, rows the sun over +x and over +z");
    }
}

#[test]
#[ignore = "a spike: for measuring"]
fn parallax_spike_cost() {
    let (dirt, normal) = (decode(DIRT), decode(DIRT_NORMAL));
    // The same picture with every height at the top, as the stand-in of a picture with no normal companion is (grass).
    let mut flat = normal.clone();
    for pixel in flat.chunks_mut(4) {
        pixel[3] = 255;
    }
    for context in contexts() {
        let name = adapter_name(&context);
        let mut reliefs = [
            floor(&context, &dirt, &normal),
            floor(&context, &dirt, &flat),
        ];
        // The eye over the floor, looking down at it, so that the floor fills the frame (the worst case: every pixel dirt); `VOX_SPIKE_ELEVATION` (degrees over the floor, 55 if
        // unset) makes the view more grazing, where the ray crosses more pixels of the picture.
        let elevation: f64 = std::env::var("VOX_SPIKE_ELEVATION")
            .ok()
            .and_then(|text| text.parse().ok())
            .unwrap_or(55.0);
        let camera = view(elevation, 0.0, 4.0, 1.0);
        let modes = [(0u32, 0.0f32), (1, 0.0), (2, 0.0), (3, 0.0), (3, 0.2)];
        {
            let image = reliefs[0].render_to_rgba(&camera, 320, 180).0;
            let floor_share = image
                .chunks(4)
                .filter(|pixel| !is_sky([pixel[0], pixel[1], pixel[2], pixel[3]]))
                .count() as f64
                / (320.0 * 180.0);
            eprintln!(
                "COST {name}: the eye is {elevation} degrees over the floor, which is {:.0} percent of the frame",
                floor_share * 100.0
            );
        }
        for (relief, label) in [(0usize, "dirt"), (1, "flat (every height at the top)")] {
            let mut totals = [0.0f64; 5];
            let mut counted = 0u32;
            for round in 0..7 {
                for (index, (mode, depth)) in modes.iter().enumerate() {
                    let renderer = &mut reliefs[relief];
                    renderer.set_parallax_spike(*mode, *depth);
                    renderer.render_to_rgba(&camera, 1280, 720);
                    let start = std::time::Instant::now();
                    for _ in 0..20 {
                        renderer.render_to_rgba(&camera, 1280, 720);
                    }
                    let elapsed = start.elapsed().as_secs_f64() * 1000.0 / 20.0;
                    if round > 0 {
                        totals[index] += elapsed;
                    }
                }
                if round > 0 {
                    counted += 1;
                }
            }
            let mean = |index: usize| totals[index] / f64::from(counted);
            eprintln!(
                "COST {name}, {label}: none {:.3} ms, offset {:+.3}, parallax occlusion {:+.3}, prisms {:+.3}, prisms twice as deep {:+.3}",
                mean(0),
                mean(1) - mean(0),
                mean(2) - mean(0),
                mean(3) - mean(0),
                mean(4) - mean(0)
            );
        }
    }
}
