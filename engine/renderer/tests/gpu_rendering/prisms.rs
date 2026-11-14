//! Prisms (`Relief::Prisms`, `docs/specs/block-textures-spec.md`, "Prisms" under "As built", `docs/history/parallax-spike.md`): each pixel of a picture is a square column as deep as the height in the
//! alpha of its normal picture (LabPBR `_n`) says, the eye's ray is walked through the columns, and what it meets is drawn: a flat top in the picture's colours, or a side wall
//! lit as the sun says. They are on by default (`DEFAULT_RELIEF`) and a pack turns them off with `ShaderPack::with_relief(Relief::Flat)`.
//!
//! The scene is a floor of 32 by 32 blocks of one picture, a sun with no shadow maps, and a camera over it; the pictures are the real dirt's or made for the test.
use renderer::{Relief, ShaderPack, default_lighting};

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

const PICTURE_SIDE: usize = 16;

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

/// A normal picture with no tilt and no occlusion whose pixel `(column, row)` is `height(column, row)` high (255 is the top).
fn heights(height: impl Fn(usize, usize) -> u8) -> Vec<u8> {
    (0..PICTURE_SIDE * PICTURE_SIDE)
        .flat_map(|index| {
            let (column, row) = (index % PICTURE_SIDE, index / PICTURE_SIDE);
            [128, 128, 255, height(column, row)]
        })
        .collect()
}

/// A square plateau at the top, 6 by 6 pixels in the middle of the picture, in a floor 0.1 of a voxel lower.
fn plateau() -> Vec<u8> {
    heights(|column, row| {
        if (5..11).contains(&column) && (5..11).contains(&row) {
            255
        } else {
            0
        }
    })
}

/// The picture of a block with no normal companion's stand-in: every height at the top.
fn all_at_the_top() -> Vec<u8> {
    heights(|_, _| 255)
}

fn plain() -> Vec<u8> {
    (0..PICTURE_SIDE * PICTURE_SIDE)
        .flat_map(|_| [200, 200, 200, 255])
        .collect()
}

/// A floor of 32 by 32 blocks with this picture and this normal picture, in a sun in direction `sun` with some sky and ambient light and no shadow maps.
fn floor(
    context: &GraphicsContext,
    picture: &[u8],
    normal: &[u8],
    sun: [f32; 3],
    picture_distance: f32,
) -> VoxelRenderer {
    let table = table();
    let mut renderer = new_renderer(context);
    renderer.set_palette_with_pictures(
        &table.palette(),
        &table.palette_pictures(),
        &[PictureSize {
            size: PICTURE_SIDE as u32,
            tiles: vec![picture],
            normal: vec![Some(normal)],
            specular: vec![None],
        }],
    );
    renderer.set_picture_distance(picture_distance);
    let blocks: Vec<([u8; 3], u32, u32)> = (0..32u8)
        .flat_map(|x| (0..32u8).map(move |z| ([x, 16, z], 1, 0)))
        .collect();
    place_blocks_in(&mut renderer, &table, ORIGIN_BRICK, &blocks, Some(0));
    set_sun(&mut renderer, sun);
    renderer
}

fn set_sun(renderer: &mut VoxelRenderer, sun: [f32; 3]) {
    renderer.set_atmosphere(Atmosphere {
        sun_direction: sun,
        sun_light: 255,
        sky_light: 40,
        ambient_light: 20,
        shadows: false,
        ..Atmosphere::default()
    });
}

/// A camera `distance` voxels from `target`, `elevation` degrees over it, round to the side by `azimuth` degrees (0 is in front of it, toward +z; 90 is from +x), and looking at it.
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

fn with_relief(renderer: &mut VoxelRenderer, relief: Option<Relief>) {
    match relief {
        Some(relief) => {
            let pack = ShaderPack::new("test:relief", "Relief", default_lighting())
                .expect("the default lighting is a good pack")
                .with_relief(relief);
            renderer
                .set_shader_pack(Some(&pack))
                .expect("a card takes the relief");
        }
        None => renderer.set_shader_pack(None).expect("the default pack"),
    }
}

const WIDTH: u32 = 320;
const HEIGHT: u32 = 200;

/// The picture of the camera once two frames agree (the first frames of a fresh renderer on the integrated adapter drop pixels).
fn settled(renderer: &mut VoxelRenderer, camera: &Camera) -> Vec<u8> {
    let mut previous = renderer.render_to_rgba(camera, WIDTH, HEIGHT).0;
    for _ in 0..30 {
        let next = renderer.render_to_rgba(camera, WIDTH, HEIGHT).0;
        if next == previous {
            return next;
        }
        previous = next;
    }
    panic!("the picture never settled");
}

/// How many pixels of the two pictures differ by more than `by` in some channel.
fn differing(first: &[u8], second: &[u8], by: u8) -> usize {
    first
        .chunks(4)
        .zip(second.chunks(4))
        .filter(|(a, b)| (0..3).any(|channel| a[channel].abs_diff(b[channel]) > by))
        .count()
}

#[test]
fn prisms_are_what_a_pack_that_says_nothing_gets_and_a_flat_pack_turns_them_off() {
    let (dirt, normal) = (decode(DIRT), decode(DIRT_NORMAL));
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = floor(&context, &dirt, &normal, [0.5, 0.5, 0.7], 100.0);
        let camera = view_of([16.0, 17.0, 16.0], 40.0, 0.0, 1.2, 0.38);
        let by_default = settled(&mut renderer, &camera);
        with_relief(&mut renderer, Some(Relief::Flat));
        let flat = settled(&mut renderer, &camera);
        let changed = differing(&by_default, &flat, 6);
        assert!(
            changed > (WIDTH * HEIGHT) as usize / 10,
            "{name}: prisms on dirt change only {changed} pixels of {} against flat",
            WIDTH * HEIGHT
        );
        with_relief(&mut renderer, Some(Relief::Prisms));
        assert_eq!(
            settled(&mut renderer, &camera),
            by_default,
            "{name}: a pack that asks for prisms is the default"
        );
        with_relief(&mut renderer, Some(Relief::Flat));
        with_relief(&mut renderer, None);
        assert_eq!(
            settled(&mut renderer, &camera),
            by_default,
            "{name}: the default pack again has the default relief"
        );
    }
}

#[test]
fn where_every_height_is_at_the_top_prisms_change_nothing() {
    let dirt = decode(DIRT);
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = floor(&context, &dirt, &all_at_the_top(), [0.5, 0.5, 0.7], 100.0);
        let camera = view_of([16.0, 17.0, 16.0], 25.0, 30.0, 1.2, 0.38);
        let prisms = settled(&mut renderer, &camera);
        with_relief(&mut renderer, Some(Relief::Flat));
        assert_eq!(
            settled(&mut renderer, &camera),
            prisms,
            "{name}: a picture with no relief is drawn alike"
        );
    }
}

/// The mean brightness of the pixels of `image` that differ between it and `other` (a wall of a prism, or the floor a prism has moved), on the left and on the right of the middle
/// of those pixels in `reference` (the pixels that differ in the first picture of the pair of suns, so that the same line divides both).
fn brightness_left_and_right(image: &[u8], other: &[u8], line: f64) -> (f64, f64) {
    let (mut left, mut right) = ((0.0, 0u32), (0.0, 0u32));
    for (index, (a, b)) in image.chunks(4).zip(other.chunks(4)).enumerate() {
        if (0..3).all(|channel| a[channel].abs_diff(b[channel]) <= 6) {
            continue;
        }
        let side = if f64::from(index as u32 % WIDTH) < line {
            &mut left
        } else {
            &mut right
        };
        side.0 += f64::from(brightness([a[0], a[1], a[2], a[3]]));
        side.1 += 1;
    }
    (
        left.0 / f64::from(left.1.max(1)),
        right.0 / f64::from(right.1.max(1)),
    )
}

/// The middle, across the picture, of the pixels that differ.
fn middle_of_the_difference(image: &[u8], other: &[u8]) -> f64 {
    let (mut total, mut count) = (0.0, 0u32);
    for (index, (a, b)) in image.chunks(4).zip(other.chunks(4)).enumerate() {
        if (0..3).any(|channel| a[channel].abs_diff(b[channel]) > 6) {
            total += f64::from(index as u32 % WIDTH);
            count += 1;
        }
    }
    total / f64::from(count.max(1))
}

/// A plateau of one colour seen from between +x and +z: its +x wall is on the right of the picture and its +z wall on the left. The wall that faces the sun is the lit one, so
/// the right is brighter than the left in a sun over +x and the left is brighter in a sun over +z.
#[test]
fn the_wall_of_a_prism_that_faces_the_sun_is_the_bright_one() {
    let (picture, plateau) = (plain(), plateau());
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = floor(&context, &picture, &plateau, [1.0, 0.55, 0.0], 100.0);
        let camera = view_of([16.5, 16.9, 16.5], 35.0, 45.0, 1.0, 0.45);
        let mut scores = Vec::new();
        let mut line = None;
        for sun in [[1.0f32, 0.55, 0.0], [0.0, 0.55, 1.0]] {
            set_sun(&mut renderer, sun);
            with_relief(&mut renderer, Some(Relief::Prisms));
            let prisms = settled(&mut renderer, &camera);
            with_relief(&mut renderer, Some(Relief::Flat));
            let flat = settled(&mut renderer, &camera);
            let line = *line.get_or_insert_with(|| middle_of_the_difference(&prisms, &flat));
            let (left, right) = brightness_left_and_right(&prisms, &flat, line);
            scores.push(right - left);
        }
        assert!(
            scores[0] > 25.0,
            "{name}: in a sun over +x the wall on the right is brighter than the one on the left by {}",
            scores[0]
        );
        assert!(
            scores[1] < -25.0,
            "{name}: in a sun over +z the wall on the left is brighter than the one on the right by {}",
            -scores[1]
        );
    }
}

/// Where the picture is not drawn (beyond the picture distance) or is smaller than a pixel of the screen (a picture pixel is a sixteenth of a voxel and so there is no relief to see
/// from far), prisms are as flat is, and cost nothing.
#[test]
fn prisms_are_left_out_beyond_the_picture_distance_and_where_a_picture_pixel_is_smaller_than_the_screen_s()
 {
    let (dirt, normal) = (decode(DIRT), decode(DIRT_NORMAL));
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = floor(&context, &dirt, &normal, [0.5, 0.5, 0.7], 100.0);
        let near = view_of([16.0, 17.0, 16.0], 40.0, 0.0, 1.2, 0.38);
        // Sixty voxels away a voxel is ten pixels of this frame, and a picture pixel two thirds of one.
        let far = view_of([16.0, 17.0, 16.0], 40.0, 0.0, 60.0, 0.5);
        let prisms: Vec<_> = [&near, &far]
            .map(|camera| settled(&mut renderer, camera))
            .into();
        with_relief(&mut renderer, Some(Relief::Flat));
        let flat: Vec<_> = [&near, &far]
            .map(|camera| settled(&mut renderer, camera))
            .into();
        assert!(
            differing(&prisms[0], &flat[0], 6) > 0,
            "{name}: the test's near view shows no prisms"
        );
        assert_eq!(
            prisms[1], flat[1],
            "{name}: from far away, where a picture pixel is smaller than a pixel of the screen, prisms are not drawn"
        );
        // (And with the picture distance shorter than the nearest face is from the camera, the picture is not drawn at all: a face of its flat colour.)
        renderer.set_picture_distance(0.3);
        with_relief(&mut renderer, Some(Relief::Prisms));
        let short_prisms = settled(&mut renderer, &near);
        with_relief(&mut renderer, Some(Relief::Flat));
        assert_eq!(
            settled(&mut renderer, &near),
            short_prisms,
            "{name}: beyond the picture distance there is no relief"
        );
    }
}

#[test]
#[ignore = "for looking at: writes a sheet of the default and the flat relief"]
fn prisms_sheet() {
    let directory = std::env::var_os("VOX_PRISMS_OUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("E:/s/vx/prisms"));
    std::fs::create_dir_all(&directory).expect("sheet directory");
    let (dirt, normal) = (decode(DIRT), decode(DIRT_NORMAL));
    for context in contexts() {
        let name: String = adapter_name(&context)
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        let mut renderer = floor(&context, &dirt, &normal, [0.5, 0.5, 0.7], 100.0);
        let elevations = [65.0, 40.0, 22.0];
        let (width, height) = (WIDTH * 2, HEIGHT * elevations.len() as u32);
        let mut sheet = vec![0u8; (width * height * 4) as usize];
        for (column, relief) in [Relief::Flat, Relief::Prisms].into_iter().enumerate() {
            with_relief(&mut renderer, Some(relief));
            for (row, elevation) in elevations.iter().enumerate() {
                let camera = view_of([16.0, 17.0, 16.0], *elevation, 0.0, 1.2, 0.38);
                let image = settled(&mut renderer, &camera);
                for y in 0..HEIGHT {
                    let from = (y * WIDTH * 4) as usize;
                    let to =
                        (((row as u32 * HEIGHT + y) * width + column as u32 * WIDTH) * 4) as usize;
                    sheet[to..to + (WIDTH * 4) as usize]
                        .copy_from_slice(&image[from..from + (WIDTH * 4) as usize]);
                }
            }
        }
        let file = std::fs::File::create(directory.join(format!("prisms_{name}.png")))
            .expect("sheet file");
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .expect("png header")
            .write_image_data(&sheet)
            .expect("png data");
        eprintln!(
            "SHEET {name}: columns flat and prisms, rows the eye 65, 40 and 22 degrees over the floor"
        );
    }
}

#[test]
#[ignore = "for measuring: what prisms cost over flat"]
fn prisms_cost() {
    let (dirt, normal) = (decode(DIRT), decode(DIRT_NORMAL));
    for context in contexts() {
        let name = adapter_name(&context);
        let elevation: f64 = std::env::var("VOX_PRISMS_ELEVATION")
            .ok()
            .and_then(|text| text.parse().ok())
            .unwrap_or(55.0);
        let camera = view_of([16.0, 17.0, 16.0], elevation, 0.0, 4.0, 1.0);
        for (label, normal) in [
            ("dirt", &normal),
            ("flat (every height at the top)", &all_at_the_top()),
        ] {
            let mut renderer = floor(&context, &dirt, normal, [0.5, 0.5, 0.7], 100.0);
            let mut totals = [0.0f64; 2];
            let mut counted = 0u32;
            for round in 0..7 {
                for (index, relief) in [Relief::Flat, Relief::Prisms].into_iter().enumerate() {
                    with_relief(&mut renderer, Some(relief));
                    renderer.render_to_rgba(&camera, 1280, 720);
                    let start = std::time::Instant::now();
                    for _ in 0..20 {
                        renderer.render_to_rgba(&camera, 1280, 720);
                    }
                    if round > 0 {
                        totals[index] += start.elapsed().as_secs_f64() * 1000.0 / 20.0;
                    }
                }
                if round > 0 {
                    counted += 1;
                }
            }
            let mean = |index: usize| totals[index] / f64::from(counted);
            eprintln!(
                "COST {name}, {label}, eye {elevation} degrees: flat {:.3} ms, prisms {:+.3}",
                mean(0),
                mean(1) - mean(0)
            );
        }
    }
}
