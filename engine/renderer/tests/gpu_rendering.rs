//! Renders small scenes on every physical graphics adapter and checks the pixels.
//!
//! The tests need a GPU. Without one they print a notice and pass, unless the environment
//! variable `VOX_REQUIRE_GPU` is set, in which case they fail. Set `VOX_SAVE_IMAGES` to a
//! directory to also write the rendered images there as PNG files.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

use block_models::{BlockDefinition, BlockTable, Shape};
use glam::Vec3;
use renderer::{
    Atmosphere, Avatar, Camera, EntityBox, GraphicsContext, OverlayRect, RenderStatistics,
    VoxelRenderer,
};
use voxel_grid::ChannelStorage;
use voxel_meshing::{BrickChannels, BrickNeighbourhood, mesh_brick};
use voxel_space::{
    BRICK_VOXEL_COUNT, BrickCoordinate, FaceDirection, LocalVoxelIndex, WorldPosition,
};

const SIZE: u32 = 64;
const SKY: [f32; 3] = [0.2, 0.4, 0.8];
const SKY_BYTES: [u8; 3] = [51, 102, 204];
const RED: u32 = 1;
const GREEN: u32 = 2;
const UNKNOWN_MATERIAL: u32 = 77;

/// Held while a test uses the GPUs: tests take turns. Creating and using many graphics devices at
/// once has twice hung the test process on this project's development machine, for a reason never
/// found, and nothing here is gained by running these tests at the same time.
static GPU_TURN: Mutex<()> = Mutex::new(());

/// The graphics contexts to test on, which keeps the test's turn on the GPUs until dropped.
struct Contexts {
    _turn: MutexGuard<'static, ()>,
    contexts: std::vec::IntoIter<GraphicsContext>,
}

impl Iterator for Contexts {
    type Item = GraphicsContext;

    fn next(&mut self) -> Option<GraphicsContext> {
        self.contexts.next()
    }
}

fn contexts() -> Contexts {
    // A test that failed while holding its turn must not stop the others.
    let turn = GPU_TURN.lock().unwrap_or_else(PoisonError::into_inner);
    let contexts = GraphicsContext::all_physical_adapters();
    if contexts.is_empty() {
        assert!(
            std::env::var_os("VOX_REQUIRE_GPU").is_none(),
            "VOX_REQUIRE_GPU is set but no graphics adapter is available"
        );
        eprintln!("no graphics adapter available; skipping GPU test");
    }
    Contexts {
        _turn: turn,
        contexts: contexts.into_iter(),
    }
}

fn new_renderer(context: &GraphicsContext) -> VoxelRenderer {
    let mut renderer = VoxelRenderer::new(context, wgpu::TextureFormat::Rgba8UnormSrgb);
    renderer.set_palette(&[[0.0, 0.0, 0.0], [0.9, 0.1, 0.1], [0.1, 0.8, 0.1]]);
    renderer.set_atmosphere(Atmosphere {
        sky_color: SKY,
        fog_start: 1000.0,
        fog_end: 2000.0,
        ..Atmosphere::default()
    });
    renderer
}

/// A brick containing the given voxels (local x, y, z, material), meshed and uploaded.
fn place_voxels(renderer: &mut VoxelRenderer, brick: BrickCoordinate, voxels: &[([u8; 3], u32)]) {
    let mut values = vec![0u32; BRICK_VOXEL_COUNT];
    for &([x, y, z], material) in voxels {
        values[LocalVoxelIndex::new(x, y, z).index()] = material;
    }
    let storage = ChannelStorage::from_values(&values);
    let mut opaque = vec![true; 256];
    opaque[0] = false;
    let mesh = mesh_brick(
        &BrickNeighbourhood {
            center: BrickChannels::blocks_only(&storage),
            neighbours: [None; 6],
        },
        &BlockTable::of_cubes(&opaque),
    );
    renderer.upload_brick(brick, &mesh.pack());
}

fn camera_at(brick: BrickCoordinate, offset: [f32; 3]) -> Camera {
    Camera::new(WorldPosition::new(brick, offset))
}

fn pixel(image: &[u8], x: u32, y: u32) -> [u8; 4] {
    let start = ((y * SIZE + x) * 4) as usize;
    [
        image[start],
        image[start + 1],
        image[start + 2],
        image[start + 3],
    ]
}

fn close_to(pixel: [u8; 4], expected: [u8; 3], tolerance: u8) -> bool {
    (0..3).all(|channel| pixel[channel].abs_diff(expected[channel]) <= tolerance)
}

fn is_sky(pixel: [u8; 4]) -> bool {
    close_to(pixel, SKY_BYTES, 2)
}

fn is_red(pixel: [u8; 4]) -> bool {
    pixel[0] > pixel[1].saturating_add(60) && pixel[0] > pixel[2].saturating_add(60)
}

fn is_green(pixel: [u8; 4]) -> bool {
    pixel[1] > pixel[0].saturating_add(60) && pixel[1] > pixel[2].saturating_add(60)
}

fn brightness(pixel: [u8; 4]) -> u32 {
    u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2])
}

fn render(renderer: &mut VoxelRenderer, camera: &Camera) -> Vec<u8> {
    renderer.render_to_rgba(camera, SIZE, SIZE).0
}

fn save_image(name: &str, adapter: &str, image: &[u8]) {
    let Some(directory) = std::env::var_os("VOX_SAVE_IMAGES").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&directory).expect("image directory can be created");
    let adapter: String = adapter
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let file =
        std::fs::File::create(directory.join(format!("{name}_{adapter}.png"))).expect("image file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), SIZE, SIZE);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(image))
        .expect("the PNG is written");
}

fn adapter_name(context: &GraphicsContext) -> String {
    renderer::describe(&context.adapter)
}

const CUBE: [([u8; 3], u32); 1] = [([16, 16, 16], RED)];
const ORIGIN_BRICK: BrickCoordinate = BrickCoordinate::ORIGIN;

#[test]
fn the_crosshair_is_drawn_at_the_centre_only_when_asked_for() {
    for context in contexts() {
        let mut renderer = new_renderer(&context);
        let camera = camera_at(BrickCoordinate::ORIGIN, [0.0; 3]);
        let without = render(&mut renderer, &camera);
        assert!(
            without
                .chunks(4)
                .all(|chunk| is_sky(chunk.try_into().unwrap())),
            "an empty scene is all sky"
        );

        renderer.set_crosshair(true);
        let with = render(&mut renderer, &camera);
        save_image("crosshair", &adapter_name(&context), &with);
        let changed: Vec<(u32, u32)> = (0..SIZE)
            .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
            .filter(|&(x, y)| !is_sky(pixel(&with, x, y)))
            .collect();
        assert!(!changed.is_empty());
        // Everything it draws is near the middle, symmetric about it, and the very centre is white.
        for &(x, y) in &changed {
            assert!(
                x.abs_diff(SIZE / 2) <= 8 && y.abs_diff(SIZE / 2) <= 8,
                "({x}, {y})"
            );
            assert!(
                changed.contains(&(SIZE - 1 - x, SIZE - 1 - y)),
                "({x}, {y}) has no mirror"
            );
        }
        assert!(close_to(
            pixel(&with, SIZE / 2, SIZE / 2),
            [255, 255, 255],
            2
        ));
        assert!(close_to(
            pixel(&with, SIZE / 2, SIZE / 2 + 5),
            [255, 255, 255],
            2
        ));
        assert!(
            is_sky(pixel(&with, SIZE / 2 + 5, SIZE / 2 + 5)),
            "the corners of the plus are empty"
        );

        renderer.set_crosshair(false);
        assert_eq!(render(&mut renderer, &camera), without);
    }
}

#[test]
fn an_empty_scene_is_all_sky() {
    for context in contexts() {
        let mut renderer = new_renderer(&context);
        let image = render(&mut renderer, &camera_at(ORIGIN_BRICK, [0.0; 3]));
        assert!(
            image
                .as_chunks::<4>()
                .0
                .iter()
                .all(|texel| is_sky([texel[0], texel[1], texel[2], texel[3]])),
            "{}",
            adapter_name(&context)
        );
        assert!(image.as_chunks::<4>().0.iter().all(|texel| texel[3] == 255));
    }
}

#[test]
fn a_cube_is_drawn_in_its_material_colour_with_sky_around_it() {
    for context in contexts() {
        let mut renderer = new_renderer(&context);
        place_voxels(&mut renderer, ORIGIN_BRICK, &CUBE);
        let image = render(&mut renderer, &camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]));
        save_image("cube", &adapter_name(&context), &image);

        let name = adapter_name(&context);
        assert!(
            is_red(pixel(&image, SIZE / 2, SIZE / 2)),
            "{name}: centre {:?}",
            pixel(&image, 32, 32)
        );
        for (x, y) in [
            (0, 0),
            (SIZE - 1, 0),
            (0, SIZE - 1),
            (SIZE - 1, SIZE - 1),
            (5, 40),
        ] {
            assert!(
                is_sky(pixel(&image, x, y)),
                "{name}: ({x}, {y}) is {:?}",
                pixel(&image, x, y)
            );
        }
    }
}

fn is_blue(pixel: [u8; 4]) -> bool {
    pixel[2] > pixel[0].saturating_add(60) && pixel[2] > pixel[1].saturating_add(30)
}

fn avatar_at(brick: BrickCoordinate, offset: [f32; 3], color: u32) -> Avatar {
    Avatar {
        position: WorldPosition::new(brick, offset),
        color,
    }
}

#[test]
fn an_avatar_is_drawn_where_the_player_is_in_its_own_colour_and_only_while_it_is_set() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);

        renderer.set_avatars(&[avatar_at(ORIGIN_BRICK, [16.5, 16.5, 16.5], 0)]);
        let red = render(&mut renderer, &camera);
        save_image("avatar", &name, &red);
        assert!(is_red(pixel(&red, SIZE / 2, SIZE / 2)), "{name}");
        assert!(is_sky(pixel(&red, 3, 3)), "{name}: sky around it");
        let (_, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_eq!(statistics.avatars_drawn, 1);

        // The eyes are in the head: the body is the voxel below them.
        let below = camera_at(ORIGIN_BRICK, [16.5, 15.5, 22.0]);
        assert!(is_red(pixel(
            &render(&mut renderer, &below),
            SIZE / 2,
            SIZE / 2
        )));
        let above = camera_at(ORIGIN_BRICK, [16.5, 17.5, 22.0]);
        assert!(is_sky(pixel(
            &render(&mut renderer, &above),
            SIZE / 2,
            SIZE / 2
        )));

        // The colour is the player's: 1 is blue, and numbers wrap around.
        renderer.set_avatars(&[avatar_at(ORIGIN_BRICK, [16.5, 16.5, 16.5], 1)]);
        assert!(is_blue(pixel(
            &render(&mut renderer, &camera),
            SIZE / 2,
            SIZE / 2
        )));
        renderer.set_avatars(&[avatar_at(ORIGIN_BRICK, [16.5, 16.5, 16.5], 16)]);
        assert_eq!(render(&mut renderer, &camera), red, "colour 16 is colour 0");

        // Behind the camera, or gone, it is not there.
        renderer.set_avatars(&[avatar_at(ORIGIN_BRICK, [16.5, 16.5, 28.5], 0)]);
        let (behind, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_eq!(statistics.avatars_drawn, 0);
        assert!(is_sky(pixel(&behind, SIZE / 2, SIZE / 2)));
        renderer.set_avatars(&[]);
        assert!(is_sky(pixel(
            &render(&mut renderer, &camera),
            SIZE / 2,
            SIZE / 2
        )));
    }
}

#[test]
fn an_avatar_is_placed_exactly_across_bricks_and_far_from_the_origin() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        // The avatar is 6 voxels ahead, in the brick before the camera's.
        let camera = camera_at(BrickCoordinate::new(1000, -3, 7), [16.5, 16.5, 5.0]);
        renderer.set_avatars(&[avatar_at(
            BrickCoordinate::new(1000, -3, 6),
            [16.5, 16.5, 31.0],
            1,
        )]);
        let image = render(&mut renderer, &camera);
        assert!(is_blue(pixel(&image, SIZE / 2, SIZE / 2)), "{name}");
        assert!(is_sky(pixel(&image, 3, 3)), "{name}");
    }
}

#[test]
fn avatars_hide_behind_blocks_and_blocks_hide_behind_avatars() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        place_voxels(&mut renderer, ORIGIN_BRICK, &CUBE);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        // Behind the cube, as seen from the camera.
        renderer.set_avatars(&[avatar_at(ORIGIN_BRICK, [16.5, 16.5, 10.5], 1)]);
        assert!(
            is_red(pixel(&render(&mut renderer, &camera), SIZE / 2, SIZE / 2)),
            "{name}"
        );
        // In front of it.
        renderer.set_avatars(&[avatar_at(ORIGIN_BRICK, [16.5, 16.5, 19.5], 1)]);
        assert!(
            is_blue(pixel(&render(&mut renderer, &camera), SIZE / 2, SIZE / 2)),
            "{name}"
        );
    }
}

#[test]
fn all_six_faces_wind_so_they_show_from_outside_and_the_sun_shades_them_differently() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        place_voxels(&mut renderer, ORIGIN_BRICK, &CUBE);
        // (camera offset, yaw, pitch) looking at the cube centre from each side, 6 voxels away.
        let half_pi = std::f32::consts::FRAC_PI_2;
        let views = [
            ("+z", [16.5, 16.5, 22.5], 0.0, 0.0),
            ("-z", [16.5, 16.5, 10.5], std::f32::consts::PI, 0.0),
            ("+x", [22.5, 16.5, 16.5], half_pi, 0.0),
            ("-x", [10.5, 16.5, 16.5], -half_pi, 0.0),
            ("+y", [16.5, 22.5, 16.5], 0.0, -Camera::MAXIMUM_PITCH),
            ("-y", [16.5, 10.5, 16.5], 0.0, Camera::MAXIMUM_PITCH),
        ];
        let mut centres = Vec::new();
        for (label, offset, yaw, pitch) in views {
            let mut camera = camera_at(ORIGIN_BRICK, offset);
            camera.yaw = yaw;
            camera.pitch = pitch;
            let image = render(&mut renderer, &camera);
            save_image(
                &format!(
                    "face_{}",
                    label
                        .replace(['+', '-'], "p_n")
                        .replace("p_n", if label.starts_with('+') { "pos" } else { "neg" })
                ),
                &name,
                &image,
            );
            let centre = pixel(&image, SIZE / 2, SIZE / 2);
            assert!(
                is_red(centre),
                "{name}: face {label} not visible, centre {centre:?}"
            );
            centres.push((label, brightness(centre)));
        }
        // The sun is at (0.4, 0.8, 0.3): faces towards it brighten in that order, and faces
        // turned away all get the same ambient light.
        let shade = |label: &str| centres.iter().find(|(l, _)| *l == label).unwrap().1;
        assert!(
            shade("+y") > shade("+x") && shade("+x") > shade("+z"),
            "{name}: sunlit faces out of order: {centres:?}"
        );
        for away in ["-x", "-y", "-z"] {
            assert!(
                shade("+z") > shade(away),
                "{name}: {away} is not darker than +z: {centres:?}"
            );
            assert_eq!(
                shade(away),
                shade("-x"),
                "{name}: unlit faces differ: {centres:?}"
            );
        }
    }
}

#[test]
fn from_inside_a_solid_voxel_every_face_is_culled() {
    for context in contexts() {
        let mut renderer = new_renderer(&context);
        place_voxels(&mut renderer, ORIGIN_BRICK, &CUBE);
        let image = render(&mut renderer, &camera_at(ORIGIN_BRICK, [16.5, 16.5, 16.5]));
        assert!(
            is_sky(pixel(&image, SIZE / 2, SIZE / 2)),
            "{}",
            adapter_name(&context)
        );
    }
}

#[test]
fn nearer_geometry_hides_farther_geometry_whatever_the_upload_order() {
    for context in contexts() {
        let name = adapter_name(&context);
        let far_red = (ORIGIN_BRICK, vec![([16, 16, 10], RED)]);
        let near_green = (BrickCoordinate::new(0, 0, 0), vec![([16, 16, 20], GREEN)]);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 30.0]);

        // Same brick holding both voxels, and two separate bricks uploaded in both orders.
        let mut renderer = new_renderer(&context);
        place_voxels(
            &mut renderer,
            ORIGIN_BRICK,
            &[far_red.1[0], near_green.1[0]],
        );
        let together = render(&mut renderer, &camera);
        assert!(
            is_green(pixel(&together, 32, 32)),
            "{name}: {:?}",
            pixel(&together, 32, 32)
        );

        let behind = BrickCoordinate::new(0, 0, -1);
        let front = BrickCoordinate::new(0, 0, 0);
        let mut images = Vec::new();
        for far_first in [true, false] {
            let mut renderer = new_renderer(&context);
            let upload_far = |renderer: &mut VoxelRenderer| {
                place_voxels(renderer, behind, &[([16, 16, 26], RED)])
            };
            let upload_near = |renderer: &mut VoxelRenderer| {
                place_voxels(renderer, front, &[([16, 16, 20], GREEN)])
            };
            if far_first {
                upload_far(&mut renderer);
                upload_near(&mut renderer);
            } else {
                upload_near(&mut renderer);
                upload_far(&mut renderer);
            }
            images.push(render(&mut renderer, &camera));
        }
        assert!(is_green(pixel(&images[0], 32, 32)), "{name}");
        assert_eq!(
            images[0], images[1],
            "{name}: upload order changed the image"
        );
    }
}

#[test]
fn the_image_is_identical_a_hundred_billion_bricks_from_the_origin() {
    for context in contexts() {
        let name = adapter_name(&context);
        let render_at = |shift: [i64; 3]| {
            let mut renderer = new_renderer(&context);
            let at = |x: i64, y: i64, z: i64| {
                BrickCoordinate::new(x + shift[0], y + shift[1], z + shift[2])
            };
            place_voxels(
                &mut renderer,
                at(0, 0, 0),
                &[([16, 16, 16], RED), ([3, 4, 5], GREEN)],
            );
            place_voxels(&mut renderer, at(1, 0, 0), &[([2, 20, 9], GREEN)]);
            place_voxels(&mut renderer, at(0, 0, -1), &[([16, 16, 20], RED)]);
            let mut camera = camera_at(at(0, 0, 0), [14.3, 18.1, 29.7]);
            camera.yaw = 0.2;
            camera.pitch = -0.1;
            render(&mut renderer, &camera)
        };
        let near_origin = render_at([0, 0, 0]);
        let far_away = render_at([100_000_000_000, -100_000_000_000, 7]);
        save_image("near_origin", &name, &near_origin);
        assert!(
            near_origin
                .as_chunks::<4>()
                .0
                .iter()
                .any(|t| !is_sky([t[0], t[1], t[2], t[3]])),
            "{name}: scene is empty"
        );
        assert_eq!(
            near_origin, far_away,
            "{name}: precision differs far from the origin"
        );
    }
}

#[test]
fn material_values_beyond_the_palette_draw_magenta() {
    for context in contexts() {
        let mut renderer = new_renderer(&context);
        place_voxels(
            &mut renderer,
            ORIGIN_BRICK,
            &[([16, 16, 16], UNKNOWN_MATERIAL)],
        );
        let image = render(&mut renderer, &camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]));
        let centre = pixel(&image, 32, 32);
        assert!(
            centre[0] > 100 && centre[2] > 100 && u32::from(centre[1]) * 3 < u32::from(centre[0]),
            "{}: {centre:?}",
            adapter_name(&context)
        );
    }
}

#[test]
fn bricks_outside_the_view_are_culled_and_counted() {
    for context in contexts() {
        let mut renderer = new_renderer(&context);
        place_voxels(
            &mut renderer,
            BrickCoordinate::new(0, 0, -1),
            &[([16, 16, 16], RED)],
        ); // ahead
        place_voxels(
            &mut renderer,
            BrickCoordinate::new(0, 0, 2),
            &[([16, 16, 16], RED)],
        ); // behind
        place_voxels(
            &mut renderer,
            BrickCoordinate::new(9, 0, 0),
            &[([16, 16, 16], RED)],
        ); // far to the side
        let (_, statistics) =
            renderer.render_to_rgba(&camera_at(ORIGIN_BRICK, [16.0, 16.0, 16.0]), SIZE, SIZE);
        assert_eq!(
            (statistics.bricks_drawn, statistics.bricks_culled),
            (1, 2),
            "{}",
            adapter_name(&context)
        );
        assert_eq!(statistics.quads_drawn, 6);
        assert_eq!(renderer.brick_count(), 3);
    }
}

#[test]
fn distant_terrain_fades_into_the_sky() {
    for context in contexts() {
        let mut renderer = new_renderer(&context);
        renderer.set_atmosphere(Atmosphere {
            sky_color: SKY,
            fog_start: 5.0,
            fog_end: 10.0,
            ..Atmosphere::default()
        });
        place_voxels(&mut renderer, ORIGIN_BRICK, &CUBE);
        // 20 voxels away, well beyond the end of the fog.
        let image = render(&mut renderer, &camera_at(ORIGIN_BRICK, [16.5, 16.5, 36.0]));
        assert!(
            is_sky(pixel(&image, 32, 32)),
            "{}: {:?}",
            adapter_name(&context),
            pixel(&image, 32, 32)
        );
    }
}

#[test]
fn replacing_and_removing_bricks_changes_what_is_drawn() {
    for context in contexts() {
        let mut renderer = new_renderer(&context);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        place_voxels(&mut renderer, ORIGIN_BRICK, &CUBE);
        assert!(is_red(pixel(&render(&mut renderer, &camera), 32, 32)));

        place_voxels(&mut renderer, ORIGIN_BRICK, &[([16, 16, 16], GREEN)]);
        assert!(is_green(pixel(&render(&mut renderer, &camera), 32, 32)));

        renderer.remove_brick(ORIGIN_BRICK);
        assert!(is_sky(pixel(&render(&mut renderer, &camera), 32, 32)));
        assert_eq!(renderer.brick_count(), 0);

        // Uploading no quads also leaves nothing.
        place_voxels(&mut renderer, ORIGIN_BRICK, &[]);
        assert!(!renderer.has_brick(ORIGIN_BRICK));
    }
}

#[test]
fn many_bricks_beyond_the_initial_placement_capacity_all_draw() {
    for context in contexts() {
        let mut renderer = new_renderer(&context);
        // More bricks than the placement buffer starts with, in a row the camera looks along.
        for index in 0..150 {
            place_voxels(
                &mut renderer,
                BrickCoordinate::new(0, 0, -index),
                &[([16, 16, 16], RED)],
            );
        }
        let mut camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 28.0]);
        camera.far_plane = 6000.0;
        camera.vertical_field_of_view = 1.0;
        let (image, statistics): (Vec<u8>, RenderStatistics) =
            renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_eq!(statistics.bricks_drawn, 150, "{}", adapter_name(&context));
        assert!(is_red(pixel(&image, 32, 32)));
    }
}

#[test]
fn the_sun_direction_is_normalized_so_its_length_does_not_change_the_shading() {
    for context in contexts() {
        let render_with_sun = |sun: Vec3| {
            let mut renderer = new_renderer(&context);
            renderer.set_atmosphere(Atmosphere {
                sky_color: SKY,
                sun_direction: sun.to_array(),
                ..Atmosphere::default()
            });
            place_voxels(&mut renderer, ORIGIN_BRICK, &CUBE);
            render(&mut renderer, &camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]))
        };
        assert_eq!(
            render_with_sun(Vec3::new(1.0, 2.0, 0.5)),
            render_with_sun(Vec3::new(10.0, 20.0, 5.0))
        );
    }
}

/// How much light a pixel stands for: its colour channels decoded from sRGB and added.
#[allow(clippy::disallowed_methods)] // a test of cosmetics, not simulation
fn linear_brightness(pixel: [u8; 4]) -> f64 {
    pixel[..3]
        .iter()
        .map(|&byte| {
            let value = f64::from(byte) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        })
        .sum()
}

/// Blocks for the shape tests: air, red (a block with states: whole, bottom slab, top slab), green.
fn shaped_table() -> BlockTable {
    let air = BlockDefinition::air();
    let red = BlockDefinition::cube([0.9, 0.1, 0.1]).with_shapes(vec![
        Shape::full(),
        Shape::half(FaceDirection::NegativeY),
        Shape::half(FaceDirection::PositiveY),
    ]);
    let green = BlockDefinition::cube([0.1, 0.8, 0.1]);
    BlockTable::from_definitions(&[Some(&air), Some(&red), Some(&green)])
}

/// A brick holding the given blocks (local position, block, state) with `light` in every voxel, if
/// given, meshed against the shaped table and uploaded.
fn place_blocks(
    renderer: &mut VoxelRenderer,
    brick: BrickCoordinate,
    blocks: &[([u8; 3], u32, u32)],
    light: Option<u32>,
) {
    let mut block_values = vec![0u32; BRICK_VOXEL_COUNT];
    let mut state_values = vec![0u32; BRICK_VOXEL_COUNT];
    for &([x, y, z], block, state) in blocks {
        let index = LocalVoxelIndex::new(x, y, z).index();
        block_values[index] = block;
        state_values[index] = state;
    }
    let (blocks, states) = (
        ChannelStorage::from_values(&block_values),
        ChannelStorage::from_values(&state_values),
    );
    let light = light.map(ChannelStorage::uniform);
    let mesh = mesh_brick(
        &BrickNeighbourhood {
            center: BrickChannels {
                blocks: &blocks,
                states: Some(&states),
                light: light.as_ref(),
            },
            neighbours: [None; 6],
        },
        &shaped_table(),
    );
    renderer.upload_brick(brick, &mesh.pack());
}

#[test]
fn a_slab_fills_only_half_of_its_cell_and_is_drawn_through_the_detailed_pipeline() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        place_blocks(&mut renderer, ORIGIN_BRICK, &[([16, 16, 16], 1, 1)], None);
        // Looking along -z at the slab's cell from the front: a ray through its lower half hits the
        // slab; one through its upper half goes over it.
        let through_the_slab = render(&mut renderer, &camera_at(ORIGIN_BRICK, [16.5, 16.25, 22.0]));
        let over_the_slab = render(&mut renderer, &camera_at(ORIGIN_BRICK, [16.5, 16.75, 22.0]));
        save_image("slab", &name, &through_the_slab);
        assert!(
            is_red(pixel(&through_the_slab, SIZE / 2, SIZE / 2)),
            "{name}"
        );
        assert!(is_sky(pixel(&over_the_slab, SIZE / 2, SIZE / 2)), "{name}");
        // Seen from above the slab's top is a red surface, and from below its bottom.
        let top = camera_at(ORIGIN_BRICK, [16.5, 22.0, 16.5]);
        let mut from_above = top;
        from_above.look_by(0.0, -std::f32::consts::FRAC_PI_2);
        assert!(is_red(pixel(
            &render(&mut renderer, &from_above),
            SIZE / 2,
            SIZE / 2
        )));

        let (_, statistics) =
            renderer.render_to_rgba(&camera_at(ORIGIN_BRICK, [16.5, 16.25, 22.0]), SIZE, SIZE);
        assert_eq!(
            (statistics.quads_drawn, statistics.detailed_quads_drawn),
            (6, 6)
        );
        assert_eq!(renderer.total_quad_count(), 6);

        // A top slab is the other half.
        place_blocks(&mut renderer, ORIGIN_BRICK, &[([16, 16, 16], 1, 2)], None);
        assert!(is_sky(pixel(
            &render(&mut renderer, &camera_at(ORIGIN_BRICK, [16.5, 16.25, 22.0])),
            SIZE / 2,
            SIZE / 2
        )));
        assert!(is_red(pixel(
            &render(&mut renderer, &camera_at(ORIGIN_BRICK, [16.5, 16.75, 22.0])),
            SIZE / 2,
            SIZE / 2
        )));

        // In state 0 the same block is a whole cube, drawn the cheap way.
        place_blocks(&mut renderer, ORIGIN_BRICK, &[([16, 16, 16], 1, 0)], None);
        let (_, statistics) =
            renderer.render_to_rgba(&camera_at(ORIGIN_BRICK, [16.5, 16.75, 22.0]), SIZE, SIZE);
        assert_eq!(
            (statistics.quads_drawn, statistics.detailed_quads_drawn),
            (6, 0)
        );
    }
}

#[test]
fn slabs_and_cubes_in_one_brick_both_draw_and_the_slab_sits_on_the_floor_of_its_cell() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        // A green cube, and a red bottom slab right behind it.
        place_blocks(
            &mut renderer,
            ORIGIN_BRICK,
            &[([16, 16, 17], 2, 0), ([16, 16, 14], 1, 1)],
            None,
        );
        // Straight on at the height of the lower half: the green cube hides the slab behind it.
        let image = render(&mut renderer, &camera_at(ORIGIN_BRICK, [16.5, 16.25, 24.0]));
        assert!(is_green(pixel(&image, SIZE / 2, SIZE / 2)), "{name}");
        // Higher up, looking down over the cube: the slab's top is visible beyond it.
        let mut looking_down = camera_at(ORIGIN_BRICK, [16.5, 20.0, 24.0]);
        looking_down.look_by(0.0, -0.40);
        let image = render(&mut renderer, &looking_down);
        save_image("slab_and_cube", &name, &image);
        let reds = (0..SIZE)
            .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
            .filter(|&(x, y)| is_red(pixel(&image, x, y)))
            .count();
        let greens = (0..SIZE)
            .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
            .filter(|&(x, y)| is_green(pixel(&image, x, y)))
            .count();
        assert!(reds > 0 && greens > 0, "{name}: {reds} red, {greens} green");
    }
}

#[test]
fn ambient_light_is_the_brightness_of_a_face_no_light_reaches_and_light_adds_to_it() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        let atmosphere = |ambient_light| Atmosphere {
            sky_color: SKY,
            fog_start: 1000.0,
            fog_end: 2000.0,
            ambient_light,
            ..Atmosphere::default()
        };
        let seen = |renderer: &mut VoxelRenderer, ambient: u8, light: u32| {
            renderer.set_atmosphere(atmosphere(ambient));
            place_blocks(renderer, ORIGIN_BRICK, &[([16, 16, 16], 2, 0)], Some(light));
            linear_brightness(pixel(&render(renderer, &camera), SIZE / 2, SIZE / 2))
        };
        let full = seen(&mut renderer, 0, 255);
        // With no light at all the face is as bright as the ambient light says.
        let ambient_only = seen(&mut renderer, 128, 0) / full;
        assert!(
            (0.48..0.52).contains(&ambient_only),
            "{name}: {ambient_only}"
        );
        // Light and ambient combine like a screen blend: half and half is three quarters.
        let both = seen(&mut renderer, 128, 128) / full;
        assert!((0.74..0.76).contains(&both), "{name}: {both}");
        // Full light is full brightness whatever the ambient light is, and no ambient light leaves
        // the light as it was.
        assert_eq!(seen(&mut renderer, 128, 255), full, "{name}");
        let without = seen(&mut renderer, 0, 128) / full;
        assert!((0.48..0.52).contains(&without), "{name}: {without}");
        assert_eq!(
            seen(&mut renderer, 255, 0),
            full,
            "{name}: full ambient light is full light"
        );
    }
}

#[test]
fn a_face_is_as_bright_as_its_light_says() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        let mut brightness_at = |light: Option<u32>| {
            place_blocks(&mut renderer, ORIGIN_BRICK, &[([16, 16, 16], 2, 0)], light);
            pixel(&render(&mut renderer, &camera), SIZE / 2, SIZE / 2)
        };
        let full = brightness_at(None);
        assert_eq!(
            brightness_at(Some(255)),
            full,
            "{name}: full light is no light value at all"
        );
        let half = brightness_at(Some(128));
        let dark = brightness_at(Some(0));
        let ratio = linear_brightness(half) / linear_brightness(full);
        assert!(
            (0.48..0.52).contains(&ratio),
            "{name}: half light is {ratio} of full, in the light the colours stand for"
        );
        assert_eq!(brightness(dark), 0, "{name}: no light is black");
    }
}

fn box_at(offset: [f32; 3], size: [u8; 3], material: u32) -> EntityBox {
    EntityBox {
        position: WorldPosition::new(ORIGIN_BRICK, offset),
        size,
        material,
    }
}

/// How many pixels of the image are red.
fn red_pixels(image: &[u8]) -> usize {
    (0..SIZE)
        .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
        .filter(|(x, y)| is_red(pixel(image, *x, *y)))
        .count()
}

#[test]
fn a_box_is_drawn_where_the_entity_is_in_its_own_size_and_material_and_only_while_it_is_set() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);

        // Half a voxel each way, red.
        renderer.set_entity_boxes(&[box_at([16.5, 16.5, 16.5], [4, 4, 4], RED)]);
        let half = render(&mut renderer, &camera);
        save_image("entity_box", &name, &half);
        assert!(is_red(pixel(&half, SIZE / 2, SIZE / 2)), "{name}");
        assert!(is_sky(pixel(&half, 3, 3)), "{name}: sky around it");
        let (_, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_eq!(statistics.entity_boxes_drawn, 1);

        // A whole voxel is bigger than half of one, and half as tall is smaller.
        renderer.set_entity_boxes(&[box_at([16.5, 16.5, 16.5], [8, 8, 8], RED)]);
        let whole = red_pixels(&render(&mut renderer, &camera));
        renderer.set_entity_boxes(&[box_at([16.5, 16.5, 16.5], [4, 4, 4], RED)]);
        let half_pixels = red_pixels(&half);
        renderer.set_entity_boxes(&[box_at([16.5, 16.5, 16.5], [4, 2, 4], RED)]);
        let flat = red_pixels(&render(&mut renderer, &camera));
        assert!(
            flat < half_pixels && half_pixels < whole,
            "{name}: {flat} < {half_pixels} < {whole}"
        );

        // The material is the box's own.
        renderer.set_entity_boxes(&[box_at([16.5, 16.5, 16.5], [4, 4, 4], GREEN)]);
        assert!(is_green(pixel(
            &render(&mut renderer, &camera),
            SIZE / 2,
            SIZE / 2
        )));

        // Where it is: lower, it is not at the middle of the picture.
        renderer.set_entity_boxes(&[box_at([16.5, 12.5, 16.5], [4, 4, 4], RED)]);
        let lower = render(&mut renderer, &camera);
        assert!(is_sky(pixel(&lower, SIZE / 2, SIZE / 2)), "{name}");
        assert!(red_pixels(&lower) > 0, "{name}: still in view, lower down");

        // Behind the camera, or gone, it is not there.
        renderer.set_entity_boxes(&[box_at([16.5, 16.5, 28.5], [4, 4, 4], RED)]);
        let (behind, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_eq!(statistics.entity_boxes_drawn, 0);
        assert!(is_sky(pixel(&behind, SIZE / 2, SIZE / 2)));
        renderer.set_entity_boxes(&[]);
        assert!(is_sky(pixel(
            &render(&mut renderer, &camera),
            SIZE / 2,
            SIZE / 2
        )));
    }
}

#[test]
fn a_box_is_placed_exactly_across_bricks_and_hides_behind_blocks() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        // The box is in the brick before the camera's, 6 voxels ahead.
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 2.0]);
        renderer.set_entity_boxes(&[EntityBox {
            position: WorldPosition::new(BrickCoordinate::new(0, 0, -1), [16.5, 16.5, 28.0]),
            size: [4, 4, 4],
            material: RED,
        }]);
        assert!(
            is_red(pixel(&render(&mut renderer, &camera), SIZE / 2, SIZE / 2)),
            "{name}: across the edge of a brick"
        );

        // A block in front of it hides it.
        place_voxels(&mut renderer, ORIGIN_BRICK, &[([16, 16, 0], GREEN)]);
        let hidden = render(&mut renderer, &camera);
        assert!(is_green(pixel(&hidden, SIZE / 2, SIZE / 2)), "{name}");
        assert_eq!(red_pixels(&hidden), 0, "{name}");
    }
}

// ---- the overlay: rectangles in pixels over the picture ----

/// A picture of what is set, taken after a few frames. The integrated AMD GPU of the development machine drops about one in a
/// thousand one-pixel rectangles in its first four or so frames (measured: 1 to 7 of 2000, on Vulkan and on DX12, never on
/// the NVIDIA card, and never after the fourth frame whatever was drawn), so pictures that are compared to the pixel are
/// taken once it has settled.
fn settled(renderer: &mut VoxelRenderer, camera: &Camera) -> Vec<u8> {
    for _ in 0..5 {
        render(renderer, camera);
    }
    render(renderer, camera)
}

fn overlay_rect(x: i32, y: i32, width: i32, height: i32, color: [u8; 4]) -> OverlayRect {
    OverlayRect {
        x,
        y,
        width,
        height,
        color,
    }
}

fn pixels_where(image: &[u8], test: impl Fn([u8; 4]) -> bool) -> usize {
    (0..SIZE)
        .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
        .filter(|(x, y)| test(pixel(image, *x, *y)))
        .count()
}

#[test]
fn an_overlay_rectangle_covers_exactly_the_pixels_it_names_and_only_while_it_is_set() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        let white = [255, 255, 255, 255];

        renderer.set_overlay(&[overlay_rect(10, 20, 30, 5, white)]);
        assert_eq!(renderer.overlay_rect_count(), 1);
        let image = settled(&mut renderer, &camera);
        save_image("overlay_rectangle", &name, &image);
        for (x, y) in [(10, 20), (39, 20), (10, 24), (39, 24), (25, 22)] {
            assert_eq!(pixel(&image, x, y), white, "{name}: ({x}, {y}) is inside");
        }
        for (x, y) in [(9, 20), (40, 20), (10, 19), (10, 25), (39, 25), (0, 0)] {
            assert!(is_sky(pixel(&image, x, y)), "{name}: ({x}, {y}) is outside");
        }
        assert_eq!(
            pixels_where(&image, |p| p == white),
            150,
            "{name}: 30 by 5 pixels"
        );

        // Gone when the overlay is emptied; a rectangle with no size is not counted or drawn.
        renderer.set_overlay(&[
            overlay_rect(10, 20, 0, 5, white),
            overlay_rect(1, 1, 5, 0, white),
        ]);
        assert_eq!(renderer.overlay_rect_count(), 0);
        assert_eq!(
            pixels_where(&settled(&mut renderer, &camera), |p| p == white),
            0
        );
        renderer.set_overlay(&[]);
        assert!(is_sky(pixel(&settled(&mut renderer, &camera), 12, 21)));
    }
}

#[test]
fn overlay_rectangles_are_in_the_colour_given_blend_by_alpha_and_later_ones_are_over_earlier_ones()
{
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        renderer.set_overlay(&[
            overlay_rect(0, 0, 32, 32, [200, 30, 30, 255]),
            overlay_rect(16, 0, 16, 32, [30, 30, 200, 255]),
            // Half black over the middle, across the two.
            overlay_rect(8, 8, 16, 16, [0, 0, 0, 128]),
        ]);
        let image = settled(&mut renderer, &camera);
        save_image("overlay_blend", &name, &image);
        assert!(
            close_to(pixel(&image, 2, 2), [200, 30, 30], 1),
            "{name}: the first colour"
        );
        assert!(
            close_to(pixel(&image, 28, 28), [30, 30, 200], 1),
            "{name}: the second is over it"
        );
        // Half black over each: darker than either, and not black. (Blending in linear light, so about 0.74 of the
        // sRGB value, not 0.5.)
        let over_red = pixel(&image, 10, 10);
        assert!(
            over_red[0] < 200 && over_red[0] > 100,
            "{name}: {over_red:?}"
        );
        let over_blue = pixel(&image, 20, 20);
        assert!(
            over_blue[2] < 200 && over_blue[2] > 100,
            "{name}: {over_blue:?}"
        );
        // Outside everything: sky.
        assert!(is_sky(pixel(&image, 50, 50)), "{name}");
    }
}

#[test]
fn an_overlay_rectangle_partly_or_wholly_off_the_picture_is_clipped_and_many_of_them_all_draw() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        let white = [255, 255, 255, 255];
        renderer.set_overlay(&[
            overlay_rect(-10, -10, 20, 20, white),
            overlay_rect(60, 60, 20, 20, white),
            overlay_rect(500, 500, 5, 5, white),
        ]);
        let image = settled(&mut renderer, &camera);
        // 10 by 10 in the top left, 4 by 4 in the bottom right.
        assert_eq!(pixels_where(&image, |p| p == white), 100 + 16, "{name}");

        // More rectangles than the buffer starts with: one pixel each, along the rows.
        let many: Vec<OverlayRect> = (0..2000)
            .map(|index| overlay_rect(index % 64, index / 64, 1, 1, white))
            .collect();
        renderer.set_overlay(&many);
        let image = settled(&mut renderer, &camera);
        assert_eq!(pixels_where(&image, |p| p == white), 2000, "{name}");
    }
}

#[test]
fn overlay_pixels_are_counted_from_the_top_left_in_a_picture_that_is_not_square() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        let white = [255, 255, 255, 255];
        // In the bottom right corner of a picture 96 wide and 48 tall, and a pixel at each of the other corners.
        renderer.set_overlay(&[
            overlay_rect(86, 43, 10, 5, white),
            overlay_rect(0, 0, 1, 1, white),
            overlay_rect(95, 0, 1, 1, white),
            overlay_rect(0, 47, 1, 1, white),
        ]);
        for _ in 0..5 {
            let _ = renderer.render_to_rgba(&camera, 96, 48);
        }
        let (image, _) = renderer.render_to_rgba(&camera, 96, 48);
        let at = |x: usize, y: usize| {
            let start = (y * 96 + x) * 4;
            [
                image[start],
                image[start + 1],
                image[start + 2],
                image[start + 3],
            ]
        };
        for (x, y) in [(86, 43), (95, 47), (0, 0), (95, 0), (0, 47), (90, 45)] {
            assert_eq!(at(x, y), white, "{name}: ({x}, {y})");
        }
        let white_pixels = (0..48)
            .flat_map(|y| (0..96).map(move |x| (x, y)))
            .filter(|(x, y)| at(*x, *y) == white)
            .count();
        assert_eq!(white_pixels, 50 + 3, "{name}");
    }
}
