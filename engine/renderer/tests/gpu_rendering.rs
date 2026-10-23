//! Renders small scenes on every physical graphics adapter and checks the pixels.
//!
//! The tests need a GPU. Without one they print a notice and pass, unless the environment
//! variable `VOX_REQUIRE_GPU` is set, in which case they fail. Set `VOX_SAVE_IMAGES` to a
//! directory to also write the rendered images there as PNG files.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

use block_models::{BlockDefinition, BlockTable, Shape, ShapeBox};
use glam::Vec3;
use renderer::{
    Atmosphere, Avatar, Camera, EntityBox, GraphicsContext, OverlayRect, RenderStatistics,
    VoxelRenderer,
};
use voxel_grid::ChannelStorage;
use voxel_meshing::{BrickChannels, BrickNeighbourhood, FULL_LIGHT, mesh_brick};
use voxel_space::{
    BRICK_VOXEL_COUNT, BrickCoordinate, FaceDirection, GridId, GridPose, LocalVoxelIndex,
    Orientation, WorldPosition,
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
    renderer.upload_brick(brick, &mesh_of(voxels));
}

/// The mesh of a brick containing these voxels.
fn mesh_of(voxels: &[([u8; 3], u32)]) -> voxel_meshing::PackedBrickMesh {
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
    mesh.pack()
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
        camera.vertical_field_of_view = 1.0;
        let (image, statistics): (Vec<u8>, RenderStatistics) =
            renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_eq!(statistics.bricks_drawn, 150, "{}", adapter_name(&context));
        assert!(is_red(pixel(&image, 32, 32)));
    }
}

/// Two walls of 32 by 32 voxels `gap` voxels apart a long way ahead, the nearer red and the farther green, seen along the line through their middles, in a view
/// so narrow that they cover a few pixels, with no fog. The nearer is drawn first when `nearer_first` (the camera looks along +z, and a brick's faces are emitted
/// from its low z end) and last when not; a depth test that does not tell them apart shows green in one of the two. Returns the pixel in the middle.
fn depth_of_two_walls(
    context: &GraphicsContext,
    distance_in_bricks: i64,
    gap: u8,
    nearer_first: bool,
) -> [u8; 4] {
    let mut renderer = VoxelRenderer::new(context, wgpu::TextureFormat::Rgba8UnormSrgb);
    renderer.set_palette(&[[0.0, 0.0, 0.0], [0.9, 0.1, 0.1], [0.1, 0.8, 0.1]]);
    renderer.set_atmosphere(Atmosphere {
        sky_color: SKY,
        fog_start: 1.0e15,
        fog_end: 2.0e15,
        ..Atmosphere::default()
    });
    let mut walls = Vec::new();
    for x in 0..32u8 {
        for y in 0..32u8 {
            let (near, far) = if nearer_first {
                (0, gap)
            } else {
                (31, 31 - gap)
            };
            walls.push(([x, y, near], RED));
            walls.push(([x, y, far], GREEN));
        }
    }
    let (brick_z, yaw) = if nearer_first {
        (distance_in_bricks, std::f32::consts::PI)
    } else {
        (-distance_in_bricks, 0.0)
    };
    place_voxels(&mut renderer, BrickCoordinate::new(0, 0, brick_z), &walls);
    let mut camera = camera_at(BrickCoordinate::new(0, 0, 0), [16.0, 16.0, 0.0]);
    camera.yaw = yaw;
    // 0.01 radians at 100 kilometres, and narrower in proportion farther out, so that the 32 voxel wall covers a few pixels at every distance.
    camera.vertical_field_of_view = 0.01 * 3125.0 / distance_in_bricks as f32;
    pixel(&render(&mut renderer, &camera), 32, 32)
}

#[test]
fn surfaces_keep_their_order_at_planet_distances_to_the_limits_measured_whichever_is_drawn_first() {
    // Measured on one adapter (an RTX 3090; the test passes on the others too): walls 2 metres apart are in order at 100 kilometres and at 10,000
    // kilometres, and at 100,000 kilometres the limit is between 4 and 8 metres (2 and 4 were wrong, in one of the two draw orders). That is what reversed
    // depth in 32 bits gives, and it is what is asserted here: 2 metres at the first two, 8 at the last. (A depth buffer that is not reversed does not do
    // this at 100 kilometres: its steps there are thousands of metres, and a far plane would have cut the walls off long before.)
    for context in contexts() {
        for (distance_in_bricks, gap) in [(3_125, 2), (312_500, 2), (3_125_000, 8)] {
            for nearer_first in [true, false] {
                let found = depth_of_two_walls(&context, distance_in_bricks, gap, nearer_first);
                assert!(
                    is_red(found),
                    "{}: walls {gap} apart, {} metres away, nearer {}: {found:?}",
                    adapter_name(&context),
                    distance_in_bricks * 32,
                    if nearer_first { "first" } else { "last" }
                );
            }
        }
        // And the sky is still the sky around a small wall in a narrow view.
        let mut renderer = new_renderer(&context);
        renderer.set_atmosphere(Atmosphere {
            sky_color: SKY,
            ..Atmosphere::default()
        });
        let image = render(
            &mut renderer,
            &camera_at(BrickCoordinate::new(0, 0, 0), [16.0, 16.0, 0.0]),
        );
        assert!(is_sky(pixel(&image, 2, 2)));
    }
}

// --- coarse nodes ---

/// The mesh of a coarse node whose cell at `(x, y, z)` is a cube of this block where `solid` says so, and sky elsewhere.
fn coarse_mesh(
    solid: impl Fn(usize, usize, usize) -> Option<u32>,
) -> voxel_meshing::PackedBrickMesh {
    use coarse_cells::{CoarseCell, CoarseNode};
    let mut cells = Vec::with_capacity(32 * 32 * 32);
    for z in 0..32 {
        for y in 0..32 {
            for x in 0..32 {
                cells.push(match solid(x, y, z) {
                    Some(block) => CoarseCell {
                        block,
                        fill: 255,
                        light: FULL_LIGHT,
                        exposure: 0,
                    },
                    None => CoarseCell::empty(0, FULL_LIGHT),
                });
            }
        }
    }
    voxel_meshing::mesh_coarse_node(&CoarseNode::from_cells(cells), &[None; 6]).pack()
}

fn cube_of_bricks(origin: [i64; 3], level: u8) -> region_octree::BrickCube {
    region_octree::BrickCube::new(BrickCoordinate::new(origin[0], origin[1], origin[2]), level)
        .unwrap()
}

/// A renderer with no fog at all, for things a long way off.
fn renderer_without_fog(context: &GraphicsContext) -> VoxelRenderer {
    let mut renderer = new_renderer(context);
    renderer.set_atmosphere(Atmosphere {
        sky_color: SKY,
        fog_start: 1.0e15,
        fog_end: 2.0e15,
        ..Atmosphere::default()
    });
    renderer
}

/// A camera at a place given in voxels, looking along -z.
fn camera_in_voxels(position: [i64; 3]) -> Camera {
    camera_at(
        BrickCoordinate::new(
            position[0].div_euclid(32),
            position[1].div_euclid(32),
            position[2].div_euclid(32),
        ),
        [
            position[0].rem_euclid(32) as f32,
            position[1].rem_euclid(32) as f32,
            position[2].rem_euclid(32) as f32,
        ],
    )
}

/// Uploads, as bricks of a renderer, the voxels of material `block` inside the box `low` to `high` (voxels, from the origin of the bricks).
fn fine_bricks_of_a_box(renderer: &mut VoxelRenderer, low: [i64; 3], high: [i64; 3], block: u32) {
    for bz in low[2].div_euclid(32)..=(high[2] - 1).div_euclid(32) {
        for by in low[1].div_euclid(32)..=(high[1] - 1).div_euclid(32) {
            for bx in low[0].div_euclid(32)..=(high[0] - 1).div_euclid(32) {
                let mut voxels = Vec::new();
                for z in 0..32i64 {
                    for y in 0..32i64 {
                        for x in 0..32i64 {
                            let at = [bx * 32 + x, by * 32 + y, bz * 32 + z];
                            if (0..3).all(|axis| (low[axis]..high[axis]).contains(&at[axis])) {
                                voxels.push(([x as u8, y as u8, z as u8], block));
                            }
                        }
                    }
                }
                if !voxels.is_empty() {
                    place_voxels(renderer, BrickCoordinate::new(bx, by, bz), &voxels);
                }
            }
        }
    }
}

#[test]
fn a_coarse_node_is_drawn_as_the_bricks_it_stands_for_at_every_detail_level() {
    for context in contexts() {
        let name = adapter_name(&context);
        for level in 1u8..=3 {
            let cell = 1i64 << level;
            // A block of cells (8 by 8 by 8, a little off the corner of the node) of the red block.
            let (low_cell, high_cell) = ([8i64, 0, 8], [16i64, 8, 16]);
            let mut coarse = new_renderer(&context);
            coarse.upload_coarse_node(
                cube_of_bricks([0, 0, 0], level),
                &coarse_mesh(|x, y, z| {
                    let at = [x as i64, y as i64, z as i64];
                    (0..3)
                        .all(|axis| (low_cell[axis]..high_cell[axis]).contains(&at[axis]))
                        .then_some(RED)
                }),
            );
            let mut fine = new_renderer(&context);
            fine_bricks_of_a_box(
                &mut fine,
                low_cell.map(|c| c * cell),
                high_cell.map(|c| c * cell),
                RED,
            );
            // Seen from a place that is above, to the side and in front: the top and two sides.
            let size = 8 * cell;
            let mut camera = camera_in_voxels([
                low_cell[0] * cell + size * 2,
                size * 2,
                high_cell[2] * cell + size * 3,
            ]);
            camera.yaw = 0.35;
            camera.pitch = -0.4;
            let from_coarse = settled(&mut coarse, &camera);
            let from_fine = settled(&mut fine, &camera);
            let drawn = from_fine
                .chunks(4)
                .filter(|chunk| !is_sky((*chunk).try_into().unwrap()))
                .count();
            assert!(
                drawn > 100,
                "{name} level {level}: the scene is empty ({drawn} pixels)"
            );
            // The same picture, to the pixel at the finer levels. At level 3 one pixel of the silhouette differed (measured: 1 of 4,096): the outline of
            // one big quad and the outline of many brick quads have the same straight edge, but the vertices where the bricks meet are snapped to the
            // rasteriser's grid on their own and can lie a hair off the line, so a pixel whose middle is on the edge can fall the other way. A few such
            // pixels are allowed; a wrong scale or place would differ in hundreds.
            let differing = from_coarse
                .chunks(4)
                .zip(from_fine.chunks(4))
                .filter(|(a, b)| a != b)
                .count();
            let allowed = if level < 3 { 0 } else { 4 };
            assert!(
                differing <= allowed,
                "{name} level {level}: the coarse node differs from the bricks in {differing} pixels"
            );
        }
    }
}

#[test]
fn a_ship_with_bricks_near_and_a_coarse_node_beyond_them_is_drawn_as_all_bricks_turned_and_placed()
{
    for context in contexts() {
        let name = adapter_name(&context);
        // A block of red voxels, 8 across, near the ship's origin (one voxel would be under a pixel from here).
        let red_block: Vec<([u8; 3], u32)> = (12..20u8)
            .flat_map(|x| (12..20u8).flat_map(move |y| (12..20u8).map(move |z| ([x, y, z], RED))))
            .collect();
        let turned = pose([1, 0, 1], [0.0; 3], half_turn_about_y());
        // The ship has a red cube of fine bricks near its origin, and beyond, in the cube of bricks from (2, 0, 0) of detail level 1 (cells of 2 voxels), a
        // green block of cells 0 to 8 along each axis (voxels 64 to 80 along x and 0 to 16 along y and z).
        let mut mixed = new_renderer(&context);
        mixed.set_grid_pose(a_ship(), turned);
        place_ship_voxels(&mut mixed, a_ship(), ORIGIN_BRICK, &red_block);
        mixed.upload_grid_coarse_node(
            a_ship(),
            cube_of_bricks([2, 0, 0], 1),
            &coarse_mesh(|x, y, z| (x < 8 && y < 8 && z < 8).then_some(GREEN)),
        );
        let mut bricks = new_renderer(&context);
        bricks.set_grid_pose(a_ship(), turned);
        place_ship_voxels(&mut bricks, a_ship(), ORIGIN_BRICK, &red_block);
        let green_voxels: Vec<([u8; 3], u32)> = (0..16u8)
            .flat_map(|x| (0..16u8).flat_map(move |y| (0..16u8).map(move |z| ([x, y, z], GREEN))))
            .collect();
        place_ship_voxels(
            &mut bricks,
            a_ship(),
            BrickCoordinate::new(2, 0, 0),
            &green_voxels,
        );
        // The ship is turned half way round, so its +x is the world's -x from its origin at voxel (32, 0, 32): seen from the front, in the ship's
        // coordinates, it is at x from -80 to -64 and the red cube near -16.
        let mut camera = camera_in_voxels([-12, 30, 110]);
        camera.pitch = -0.2;
        let from_mixed = settled(&mut mixed, &camera);
        let from_bricks = settled(&mut bricks, &camera);
        let colours = |image: &[u8]| {
            let pixels: Vec<[u8; 4]> = image.chunks(4).map(|c| c.try_into().unwrap()).collect();
            (
                pixels.iter().filter(|p| is_red(**p)).count(),
                pixels.iter().filter(|p| is_green(**p)).count(),
            )
        };
        let (red, green) = colours(&from_bricks);
        assert!(
            red > 20 && green > 20,
            "{name}: the scene has {red} red and {green} green pixels"
        );
        assert!(
            from_mixed == from_bricks,
            "{name}: the fine bricks and the coarse node together differ from all bricks"
        );
    }
}

#[test]
fn a_cell_of_a_node_of_the_coarsest_detail_a_thousand_kilometres_off_is_drawn_to_scale() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = renderer_without_fog(&context);
        // A cell of 2^18 voxels (262 kilometres): the first cell of the node of detail level 18 at the origin.
        renderer.upload_coarse_node(
            cube_of_bricks([0, 0, 0], 18),
            &coarse_mesh(|x, y, z| (x == 0 && y == 0 && z == 0).then_some(RED)),
        );
        let edge = 1i64 << 18;
        // Looking at the face toward +z from 1,000 kilometres in front of it, in line with its middle: it is 2 * atan(131072 / 10^6) = 15 degrees wide in a
        // view of 70, about 12 pixels of 64.
        let mut camera = camera_in_voxels([edge / 2, edge / 2, edge + 1_000_000]);
        camera.vertical_field_of_view = 70.0f32.to_radians();
        let image = render(&mut renderer, &camera);
        assert!(
            is_red(pixel(&image, 32, 32)),
            "{name}: {:?}",
            pixel(&image, 32, 32)
        );
        assert!(
            is_red(pixel(&image, 28, 28)) && is_red(pixel(&image, 36, 36)),
            "{name}"
        );
        assert!(
            is_sky(pixel(&image, 22, 32)) && is_sky(pixel(&image, 42, 32)),
            "{name}: wider than it should be"
        );
        assert!(is_sky(pixel(&image, 2, 2)), "{name}");
    }
}

#[test]
fn the_far_corner_of_a_huge_node_is_not_culled_for_being_far_from_the_middle_of_a_small_one() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = renderer_without_fog(&context);
        renderer.upload_coarse_node(
            cube_of_bricks([0, 0, 0], 18),
            &coarse_mesh(|x, y, z| (x == 31 && y == 31 && z == 31).then_some(RED)),
        );
        // The last cell: voxels 31 * 2^18 up to 32 * 2^18 along each axis. Seen from 1,000 kilometres in front of its +z face, in line with its middle.
        let edge = 1i64 << 18;
        let camera = camera_in_voxels([
            31 * edge + edge / 2,
            31 * edge + edge / 2,
            32 * edge + 1_000_000,
        ]);
        let (image, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_eq!(statistics.coarse_nodes_drawn, 1, "{name}");
        assert!(
            is_red(pixel(&image, 32, 32)),
            "{name}: {:?}",
            pixel(&image, 32, 32)
        );
    }
}

#[test]
fn coarse_nodes_are_held_counted_culled_replaced_and_removed_like_bricks() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        let node = |origin: [i64; 3]| cube_of_bricks(origin, 2);
        let cell = coarse_mesh(|x, y, z| (x < 4 && y < 4 && z < 4).then_some(RED));
        renderer.upload_coarse_node(node([0, 0, 0]), &cell);
        // A node behind the camera.
        renderer.upload_coarse_node(node([0, 0, 40]), &cell);
        assert_eq!(renderer.coarse_node_count(), 2);
        assert!(renderer.has_coarse_node(node([0, 0, 0])));
        let mut camera = camera_in_voxels([8, 8, 60]);
        camera.vertical_field_of_view = 70.0f32.to_radians();
        let (_, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_eq!(statistics.coarse_nodes_drawn, 1, "{name}");
        assert_eq!(statistics.bricks_drawn, 1, "{name}");
        assert_eq!(statistics.bricks_culled, 1, "{name}");
        assert!(renderer.total_quad_count() >= 12, "{name}");
        // An empty mesh removes the node, and so does asking for it.
        renderer.upload_coarse_node(node([0, 0, 40]), &coarse_mesh(|_, _, _| None));
        assert_eq!(renderer.coarse_node_count(), 1);
        renderer.remove_coarse_node(node([0, 0, 0]));
        assert_eq!(renderer.coarse_node_count(), 0);
        // Those of a ship go with the ship, and one ship does not disturb another.
        let (first, second) = (a_ship(), GridId::new(GridId::FIRST_ALLOCATED + 1));
        renderer.upload_grid_coarse_node(first, node([0, 0, 0]), &cell);
        renderer.upload_grid_coarse_node(second, node([0, 0, 0]), &cell);
        assert_eq!(renderer.grid_coarse_node_count(), 2);
        renderer.remove_grid_coarse_node(second, node([0, 0, 0]));
        assert_eq!(renderer.grid_coarse_node_count(), 1);
        renderer.upload_grid_coarse_node(second, node([4, 0, 0]), &cell);
        renderer.remove_grid(second);
        assert_eq!(renderer.grid_coarse_node_count(), 1, "{name}");
        // A ship with no pose is not drawn, coarse nodes too.
        let (_, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_eq!(statistics.coarse_nodes_drawn, 0, "{name}");
    }
}

#[test]
#[should_panic(expected = "a cube of level 0 is a brick")]
fn a_cube_of_bricks_of_level_0_is_not_a_coarse_node() {
    let Some(context) = GraphicsContext::all_physical_adapters().into_iter().next() else {
        panic!("a cube of level 0 is a brick");
    };
    let mut renderer = new_renderer(&context);
    renderer.upload_coarse_node(
        cube_of_bricks([0, 0, 0], 0),
        &coarse_mesh(|_, _, _| Some(RED)),
    );
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
                orientations: None,
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

/// A picture of what is set, once a frame after it repeats it exactly. The integrated AMD GPU of the development machine drops
/// about one in a thousand one-pixel rectangles in its first few frames, and now and then a pixel or two of a thin rectangle
/// later (measured: 1 to 7 of 2000 in the first four frames, on Vulkan and on DX12, and two pixels of a line of text in
/// the sixth; never on the NVIDIA card), so pictures that are compared to the pixel are taken once two frames in a row agree.
fn settled(renderer: &mut VoxelRenderer, camera: &Camera) -> Vec<u8> {
    let mut previous = render(renderer, camera);
    for _ in 0..30 {
        let next = render(renderer, camera);
        if next == previous {
            return next;
        }
        previous = next;
    }
    panic!("the picture never settled");
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

// ---- a wide palette: more materials than a byte, a short, or the old 240 ----

/// A brick with one voxel of this material at the middle, meshed with a table that has an opaque cube for every value up to it.
fn place_wide_cube(renderer: &mut VoxelRenderer, material: u32) {
    let mut values = vec![0u32; BRICK_VOXEL_COUNT];
    values[LocalVoxelIndex::new(16, 16, 16).index()] = material;
    let storage = ChannelStorage::from_values(&values);
    let mut opaque = vec![true; material as usize + 1];
    opaque[0] = false;
    let mesh = mesh_brick(
        &BrickNeighbourhood {
            center: BrickChannels::blocks_only(&storage),
            neighbours: [None; 6],
        },
        &BlockTable::of_cubes(&opaque),
    );
    renderer.upload_brick(ORIGIN_BRICK, &mesh.pack());
}

/// A palette of this many colours in which each is different, and what the colour of one of them is.
fn wide_palette(length: usize) -> Vec<[f32; 3]> {
    (0..length)
        .map(|index| {
            [
                (index % 251) as f32 / 251.0,
                (index / 251 % 241) as f32 / 241.0,
                (index / (251 * 241) % 7) as f32 / 7.0,
            ]
        })
        .collect()
}

#[test]
fn a_material_far_up_a_wide_palette_is_drawn_in_its_colour_exactly_as_the_same_colour_low_in_it() {
    // The values around 255 and 256, beyond a short (65535), and the last of 300,000.
    let materials = [1u32, 255, 256, 4095, 65_535, 65_536, 70_000, 299_999];
    for context in contexts() {
        let name = adapter_name(&context);
        let palette = wide_palette(300_000);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        for material in materials {
            let colour = palette[material as usize];
            // The reference: that colour as material 1 of a palette of two.
            let mut reference = new_renderer(&context);
            reference.set_palette(&[[0.0; 3], colour]);
            place_wide_cube(&mut reference, 1);
            let expected = settled(&mut reference, &camera);
            // The same colour at its own place in the wide palette.
            let mut wide = new_renderer(&context);
            wide.set_palette(&palette);
            place_wide_cube(&mut wide, material);
            let image = settled(&mut wide, &camera);
            assert_eq!(
                pixel(&image, SIZE / 2, SIZE / 2),
                pixel(&expected, SIZE / 2, SIZE / 2),
                "{name}: material {material}"
            );
            assert!(
                !is_sky(pixel(&image, SIZE / 2, SIZE / 2)) || colour == [0.0; 3],
                "{name}: material {material} was drawn"
            );
            // Not magenta, unless that is what the colour is.
            if colour != [1.0, 0.0, 1.0] {
                let centre = pixel(&image, SIZE / 2, SIZE / 2);
                assert!(
                    !(centre[0] > 100 && centre[1] < 40 && centre[2] > 100),
                    "{name}: material {material} is magenta, {centre:?}"
                );
            }
        }
    }
}

#[test]
fn materials_beyond_a_wide_palette_draw_magenta_up_to_the_last_value_before_the_avatars() {
    for context in contexts() {
        let name = adapter_name(&context);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        let mut reference = new_renderer(&context);
        reference.set_palette(&[[0.0; 3], [1.0, 0.0, 1.0]]);
        place_wide_cube(&mut reference, 1);
        let magenta = settled(&mut reference, &camera);
        let mut renderer = new_renderer(&context);
        renderer.set_palette(&wide_palette(1000));
        for material in [1000u32, 1001, 70_000] {
            place_wide_cube(&mut renderer, material);
            let image = settled(&mut renderer, &camera);
            assert_eq!(
                pixel(&image, SIZE / 2, SIZE / 2),
                pixel(&magenta, SIZE / 2, SIZE / 2),
                "{name}: material {material}, past the end of the palette"
            );
        }
    }
}

#[test]
fn avatars_and_boxes_are_drawn_in_their_own_colours_whatever_the_size_of_the_palette() {
    for context in contexts() {
        let name = adapter_name(&context);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        // The avatar of colour 1, as it is with a small palette.
        let mut small = new_renderer(&context);
        small.set_avatars(&[avatar_at(ORIGIN_BRICK, [16.5, 16.5, 16.5], 1)]);
        let expected = settled(&mut small, &camera);
        let mut wide = new_renderer(&context);
        wide.set_palette(&wide_palette(500_000));
        wide.set_avatars(&[avatar_at(ORIGIN_BRICK, [16.5, 16.5, 16.5], 1)]);
        assert!(
            settled(&mut wide, &camera) == expected,
            "{name}: the avatar"
        );

        // A box of a high material is drawn in that colour (the same box of that colour low in a small palette).
        let colour = [0.1, 0.9, 0.2];
        let mut palette = vec![[0.0; 3]; 400_000];
        palette[399_999] = colour;
        let mut high = new_renderer(&context);
        high.set_palette(&palette);
        high.set_entity_boxes(&[box_at([16.5, 16.5, 16.5], [4, 4, 4], 399_999)]);
        let mut low = new_renderer(&context);
        low.set_palette(&[[0.0; 3], colour]);
        low.set_entity_boxes(&[box_at([16.5, 16.5, 16.5], [4, 4, 4], 1)]);
        assert!(
            settled(&mut high, &camera) == settled(&mut low, &camera),
            "{name}: the box"
        );
    }
}

#[test]
fn a_palette_of_the_most_there_may_be_is_drawn_and_one_more_is_refused() {
    for context in contexts() {
        let mut renderer = new_renderer(&context);
        let palette = wide_palette(renderer::MAXIMUM_GAME_MATERIALS);
        renderer.set_palette(&palette);
        place_wide_cube(&mut renderer, 1);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        assert!(!is_sky(pixel(
            &settled(&mut renderer, &camera),
            SIZE / 2,
            SIZE / 2
        )));
        let too_many = vec![[0.0; 3]; renderer::MAXIMUM_GAME_MATERIALS + 1];
        let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            renderer.set_palette(&too_many);
        }));
        assert!(refused.is_err(), "one more than the most was accepted");
    }
}

// ---- ships: bricks drawn under the pose of a grid ----

fn a_ship() -> GridId {
    GridId::new(GridId::FIRST_ALLOCATED)
}

/// A ship at this voxel (given as brick and offset), turned this way.
fn pose(brick: [i64; 3], offset: [f32; 3], orientation: Orientation) -> GridPose {
    GridPose::new(
        WorldPosition::new(BrickCoordinate::new(brick[0], brick[1], brick[2]), offset),
        orientation,
    )
}

/// A quarter of the way round about the vertical axis twice: the quaternion is exact, so the turn is.
fn half_turn_about_y() -> Orientation {
    Orientation::from_components([0.0, 1.0, 0.0, 0.0]).unwrap()
}

fn half_turn_about_x() -> Orientation {
    Orientation::from_components([1.0, 0.0, 0.0, 0.0]).unwrap()
}

fn place_ship_voxels(
    renderer: &mut VoxelRenderer,
    grid: GridId,
    brick: BrickCoordinate,
    voxels: &[([u8; 3], u32)],
) {
    renderer.upload_grid_brick(grid, brick, &mesh_of(voxels));
}

/// An L of two colours, which looks different from every side.
const ELL: [([u8; 3], u32); 3] = [
    ([16, 16, 16], RED),
    ([17, 16, 16], GREEN),
    ([16, 17, 16], RED),
];

#[test]
fn a_ship_turned_half_way_round_is_drawn_exactly_as_the_terrain_it_amounts_to() {
    for context in contexts() {
        let name = adapter_name(&context);
        // The ship's origin is the voxel (32, 0, 32) and it is turned about the vertical axis, so its voxel (x, y, z) is the world
        // voxel (31 - x, y, 31 - z).
        let mut ship = new_renderer(&context);
        ship.set_grid_pose(a_ship(), pose([1, 0, 1], [0.0; 3], half_turn_about_y()));
        place_ship_voxels(&mut ship, a_ship(), ORIGIN_BRICK, &ELL);
        let mut terrain = new_renderer(&context);
        let mirrored: Vec<([u8; 3], u32)> = ELL
            .iter()
            .map(|&([x, y, z], material)| ([31 - x, y, 31 - z], material))
            .collect();
        place_voxels(&mut terrain, ORIGIN_BRICK, &mirrored);
        let mut camera = camera_at(ORIGIN_BRICK, [15.0, 20.0, 22.0]);
        camera.pitch = -0.5;
        let from_the_ship = settled(&mut ship, &camera);
        let from_the_terrain = settled(&mut terrain, &camera);
        save_image("ship_turned_about_y", &name, &from_the_ship);
        let drawn = from_the_terrain
            .chunks(4)
            .filter(|chunk| !is_sky((*chunk).try_into().unwrap()))
            .count();
        assert!(drawn > 100, "{name}: the scene is empty ({drawn} pixels)");
        assert!(from_the_ship == from_the_terrain, "{name}");
    }
}

#[test]
fn a_ship_upside_down_is_lit_by_the_sun_on_the_faces_that_point_to_it() {
    for context in contexts() {
        let name = adapter_name(&context);
        // Turned over about the x axis, the ship's voxel (x, y, z) is the world voxel (x, 31 - y, 31 - z): a face that is the top of a
        // cube in the ship is the bottom of it in the world, and is shaded as a bottom.
        let mut ship = new_renderer(&context);
        ship.set_grid_pose(a_ship(), pose([0, 1, 1], [0.0; 3], half_turn_about_x()));
        place_ship_voxels(&mut ship, a_ship(), ORIGIN_BRICK, &ELL);
        let mut terrain = new_renderer(&context);
        let flipped: Vec<([u8; 3], u32)> = ELL
            .iter()
            .map(|&([x, y, z], material)| ([x, 31 - y, 31 - z], material))
            .collect();
        place_voxels(&mut terrain, ORIGIN_BRICK, &flipped);
        // Seen from above and the side, where the top and the side of a block are shaded differently.
        let mut camera = camera_at(ORIGIN_BRICK, [17.0, 19.0, 22.0]);
        camera.pitch = -0.5;
        let from_the_ship = settled(&mut ship, &camera);
        let from_the_terrain = settled(&mut terrain, &camera);
        save_image("ship_upside_down", &name, &from_the_ship);
        let drawn = from_the_terrain
            .chunks(4)
            .filter(|chunk| !is_sky((*chunk).try_into().unwrap()))
            .count();
        assert!(drawn > 100, "{name}: the scene is empty ({drawn} pixels)");
        assert!(from_the_ship == from_the_terrain, "{name}");
    }
}

/// How many pixels wide the drawn part of a picture is.
fn drawn_width(image: &[u8]) -> u32 {
    let columns: Vec<u32> = (0..SIZE)
        .filter(|&x| (0..SIZE).any(|y| !is_sky(pixel(image, x, y))))
        .collect();
    columns.last().unwrap() - columns.first().unwrap() + 1
}

#[test]
fn a_ship_turned_45_degrees_shows_the_diagonal_of_its_block() {
    for context in contexts() {
        let name = adapter_name(&context);
        // A block seen from straight above, turned by 45 degrees about the vertical axis: it is as wide as its diagonal.
        let seen = |orientation: Orientation| {
            let mut renderer = new_renderer(&context);
            // The block's centre is the middle of the voxel at the ship's origin plus half a voxel on each side.
            renderer.set_grid_pose(a_ship(), pose([0, 0, 0], [16.0, 16.0, 16.0], orientation));
            place_ship_voxels(&mut renderer, a_ship(), ORIGIN_BRICK, &[([0, 0, 0], RED)]);
            let mut camera = camera_at(ORIGIN_BRICK, [16.5, 19.0, 16.5]);
            camera.pitch = -std::f32::consts::FRAC_PI_2;
            settled(&mut renderer, &camera)
        };
        let straight = drawn_width(&seen(Orientation::IDENTITY));
        let turned = drawn_width(&seen(Orientation::from_yaw_pitch(
            std::f32::consts::FRAC_PI_4,
            0.0,
        )));
        let ratio = turned as f32 / straight as f32;
        assert!(
            (1.25..1.6).contains(&ratio),
            "{name}: {straight} pixels wide straight, {turned} turned"
        );
    }
}

#[test]
fn a_ship_far_from_the_origin_is_drawn_exactly_as_one_at_it() {
    for context in contexts() {
        let name = adapter_name(&context);
        let seen = |brick: [i64; 3], local_brick: BrickCoordinate| {
            let mut renderer = new_renderer(&context);
            // The ship's origin is `brick`, and its block is `local_brick` along: both far, one the negative of the other, so that the
            // block is at the origin of the world.
            renderer.set_grid_pose(a_ship(), pose(brick, [0.0; 3], Orientation::IDENTITY));
            place_ship_voxels(&mut renderer, a_ship(), local_brick, &CUBE);
            let mut camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
            camera.position = WorldPosition::new(
                BrickCoordinate::new(
                    brick[0] + local_brick.x(),
                    brick[1] + local_brick.y(),
                    brick[2] + local_brick.z(),
                ),
                [16.5, 16.5, 22.0],
            );
            settled(&mut renderer, &camera)
        };
        let near = seen([0, 0, 0], ORIGIN_BRICK);
        assert!(
            !is_sky(pixel(&near, SIZE / 2, SIZE / 2)),
            "{name}: the block is in the middle"
        );
        let far_origin = seen([2_000_000, -3_000_000, 1_000_000], ORIGIN_BRICK);
        assert!(
            far_origin == near,
            "{name}: a ship far from the world's origin"
        );
        let far_along = seen(
            [-1_500_000, 7, 2_500_000],
            BrickCoordinate::new(1_500_000, 0, -2_499_999),
        );
        assert!(far_along == near, "{name}: a brick far along a ship");
    }
}

#[test]
fn a_ship_is_drawn_only_with_a_pose_and_only_until_it_is_removed_and_its_bricks_are_counted_and_culled()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        place_ship_voxels(&mut renderer, a_ship(), ORIGIN_BRICK, &CUBE);
        assert_eq!(renderer.grid_brick_count(), 1);
        // Without a pose it is nowhere.
        let image = render(&mut renderer, &camera);
        assert!(is_sky(pixel(&image, SIZE / 2, SIZE / 2)), "{name}: no pose");
        renderer.set_grid_pose(a_ship(), pose([0, 0, 0], [0.0; 3], Orientation::IDENTITY));
        let image = settled(&mut renderer, &camera);
        assert!(
            is_red(pixel(&image, SIZE / 2, SIZE / 2)),
            "{name}: with a pose"
        );
        let (_, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_eq!((statistics.bricks_drawn, statistics.bricks_culled), (1, 0));
        assert_eq!(renderer.total_quad_count(), 6);

        // Moved behind the camera, it is culled.
        renderer.set_grid_pose(a_ship(), pose([0, 0, 3], [0.0; 3], Orientation::IDENTITY));
        let (_, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_eq!((statistics.bricks_drawn, statistics.bricks_culled), (0, 1));

        // Moved back and then removed whole, it and its bricks are gone, and the terrain is not touched.
        renderer.set_grid_pose(a_ship(), pose([0, 0, 0], [0.0; 3], Orientation::IDENTITY));
        place_voxels(&mut renderer, BrickCoordinate::new(0, -1, 0), &CUBE);
        renderer.remove_grid(a_ship());
        assert_eq!(renderer.grid_brick_count(), 0);
        assert_eq!(renderer.brick_count(), 1);
        let (_, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_eq!(statistics.bricks_drawn + statistics.bricks_culled, 1);
    }
}

#[test]
fn a_ship_brick_replaced_by_an_empty_mesh_goes_and_one_ship_does_not_disturb_another() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        let (first, second) = (a_ship(), GridId::new(GridId::FIRST_ALLOCATED + 1));
        renderer.set_grid_pose(first, pose([0, 0, 0], [0.0; 3], Orientation::IDENTITY));
        renderer.set_grid_pose(second, pose([5, 0, 0], [0.0; 3], Orientation::IDENTITY));
        place_ship_voxels(&mut renderer, first, ORIGIN_BRICK, &CUBE);
        place_ship_voxels(&mut renderer, second, ORIGIN_BRICK, &CUBE);
        assert_eq!(renderer.grid_brick_count(), 2);
        renderer.upload_grid_brick(first, ORIGIN_BRICK, &mesh_of(&[]));
        assert_eq!(renderer.grid_brick_count(), 1, "{name}");
        renderer.remove_grid_brick(second, ORIGIN_BRICK);
        assert_eq!(renderer.grid_brick_count(), 0, "{name}");
        place_ship_voxels(&mut renderer, first, ORIGIN_BRICK, &CUBE);
        place_ship_voxels(&mut renderer, second, ORIGIN_BRICK, &CUBE);
        renderer.remove_grid(second);
        assert_eq!(renderer.grid_brick_count(), 1, "{name}");
    }
}

/// The mesh of a brick that has a half block of material 1 at each of these voxels: the half is the box `from` to `to` (in eighths of a
/// voxel) of the voxel.
fn mesh_of_slabs(voxels: &[[u8; 3]], from: [u8; 3], to: [u8; 3]) -> voxel_meshing::PackedBrickMesh {
    let mut values = vec![0u32; BRICK_VOXEL_COUNT];
    for &[x, y, z] in voxels {
        values[LocalVoxelIndex::new(x, y, z).index()] = 1;
    }
    let storage = ChannelStorage::from_values(&values);
    let slab = BlockDefinition::cube([0.5; 3])
        .with_shapes(vec![Shape::from_boxes(&[ShapeBox { min: from, max: to }])]);
    let air = BlockDefinition::air();
    let definitions: Vec<Option<&BlockDefinition>> = vec![Some(&air), Some(&slab)];
    mesh_brick(
        &BrickNeighbourhood {
            center: BrickChannels::blocks_only(&storage),
            neighbours: [None; 6],
        },
        &BlockTable::from_definitions(&definitions),
    )
    .pack()
}

#[test]
fn a_ship_of_blocks_that_do_not_fill_their_cells_is_turned_like_one_of_whole_cubes() {
    for context in contexts() {
        let name = adapter_name(&context);
        // Turned half way round about the vertical axis, a block that fills the voxel's x from 0 to a half and z from 0 to a half
        // fills the world's voxel from a half to the whole on both: the box is mirrored in its cell as the voxels are in the world.
        let mut ship = new_renderer(&context);
        ship.set_grid_pose(a_ship(), pose([1, 0, 1], [0.0; 3], half_turn_about_y()));
        let voxels = [[16, 16, 16], [17, 16, 16], [16, 17, 16]];
        ship.upload_grid_brick(
            a_ship(),
            ORIGIN_BRICK,
            &mesh_of_slabs(&voxels, [0, 0, 0], [4, 8, 4]),
        );
        let mut terrain = new_renderer(&context);
        let mirrored: Vec<[u8; 3]> = voxels
            .iter()
            .map(|&[x, y, z]| [31 - x, y, 31 - z])
            .collect();
        terrain.upload_brick(
            ORIGIN_BRICK,
            &mesh_of_slabs(&mirrored, [4, 0, 4], [8, 8, 8]),
        );
        let mut camera = camera_at(ORIGIN_BRICK, [15.0, 20.0, 22.0]);
        camera.pitch = -0.5;
        let from_the_ship = settled(&mut ship, &camera);
        let from_the_terrain = settled(&mut terrain, &camera);
        let drawn = from_the_terrain
            .chunks(4)
            .filter(|chunk| !is_sky((*chunk).try_into().unwrap()))
            .count();
        assert!(drawn > 50, "{name}: the scene is empty ({drawn} pixels)");
        assert!(from_the_ship == from_the_terrain, "{name}");
    }
}

#[test]
fn a_ship_tumbled_about_a_diagonal_is_drawn_exactly_as_the_terrain_it_amounts_to() {
    for context in contexts() {
        let name = adapter_name(&context);
        // The quaternion (1/2, 1/2, 1/2, 1/2) turns the x axis onto y, y onto z and z onto x, and is exact: the ship's voxel
        // (x, y, z) is the world's (z, x, y). Every entry of the matrix that turns it is used, in the place it is in.
        let mut ship = new_renderer(&context);
        ship.set_grid_pose(
            a_ship(),
            pose(
                [0, 0, 0],
                [0.0; 3],
                Orientation::from_components([0.5, 0.5, 0.5, 0.5]).unwrap(),
            ),
        );
        place_ship_voxels(&mut ship, a_ship(), ORIGIN_BRICK, &ELL);
        let mut terrain = new_renderer(&context);
        let tumbled: Vec<([u8; 3], u32)> = ELL
            .iter()
            .map(|&([x, y, z], material)| ([z, x, y], material))
            .collect();
        place_voxels(&mut terrain, ORIGIN_BRICK, &tumbled);
        let mut camera = camera_at(ORIGIN_BRICK, [17.0, 20.0, 24.0]);
        camera.pitch = -0.4;
        let from_the_ship = settled(&mut ship, &camera);
        let from_the_terrain = settled(&mut terrain, &camera);
        save_image("ship_tumbled", &name, &from_the_ship);
        let drawn = from_the_terrain
            .chunks(4)
            .filter(|chunk| !is_sky((*chunk).try_into().unwrap()))
            .count();
        assert!(drawn > 100, "{name}: the scene is empty ({drawn} pixels)");
        assert!(from_the_ship == from_the_terrain, "{name}");
    }
}

#[test]
fn a_ship_is_drawn_when_the_camera_is_inside_its_brick_and_looks_at_the_far_side_of_it() {
    for context in contexts() {
        let name = adapter_name(&context);
        // The brick's own corner is far behind the camera, and so is all of what a sphere around it would cover.
        let mut renderer = new_renderer(&context);
        renderer.set_grid_pose(a_ship(), pose([0, 0, 0], [0.0; 3], Orientation::IDENTITY));
        place_ship_voxels(
            &mut renderer,
            a_ship(),
            ORIGIN_BRICK,
            &[([30, 16, 16], RED)],
        );
        let mut camera = camera_at(ORIGIN_BRICK, [28.5, 16.5, 16.5]);
        camera.yaw = -std::f32::consts::FRAC_PI_2;
        let (image, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        let _ = image;
        assert_eq!(statistics.bricks_drawn, 1, "{name}");
        let image = settled(&mut renderer, &camera);
        assert!(is_red(pixel(&image, SIZE / 2, SIZE / 2)), "{name}");
    }
}

#[test]
fn a_brick_far_along_a_tumbled_ship_is_drawn_where_the_turn_of_its_place_puts_it() {
    for context in contexts() {
        let name = adapter_name(&context);
        // The ship's voxel (x, y, z) is the world's (z, x, y): a brick at (1, 2, 3) of the ship is the brick (3, 1, 2) of the world, and
        // every entry of the matrix that turns the place of the brick matters.
        let mut ship = new_renderer(&context);
        ship.set_grid_pose(
            a_ship(),
            pose(
                [0, 0, 0],
                [0.0; 3],
                Orientation::from_components([0.5, 0.5, 0.5, 0.5]).unwrap(),
            ),
        );
        place_ship_voxels(&mut ship, a_ship(), BrickCoordinate::new(1, 2, 3), &ELL);
        let mut terrain = new_renderer(&context);
        let tumbled: Vec<([u8; 3], u32)> = ELL
            .iter()
            .map(|&([x, y, z], material)| ([z, x, y], material))
            .collect();
        terrain.upload_brick(BrickCoordinate::new(3, 1, 2), &mesh_of(&tumbled));
        let mut camera = camera_at(BrickCoordinate::new(3, 1, 2), [17.0, 20.0, 24.0]);
        camera.pitch = -0.4;
        let from_the_ship = settled(&mut ship, &camera);
        let from_the_terrain = settled(&mut terrain, &camera);
        let drawn = from_the_terrain
            .chunks(4)
            .filter(|chunk| !is_sky((*chunk).try_into().unwrap()))
            .count();
        assert!(drawn > 100, "{name}: the scene is empty ({drawn} pixels)");
        assert!(from_the_ship == from_the_terrain, "{name}");
    }
}
