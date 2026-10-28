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
    SunOccluder, VoxelRenderer,
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
        let render_at = |shift: [i128; 3]| {
            let mut renderer = new_renderer(&context);
            let at = |x: i128, y: i128, z: i128| {
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
    distance_in_bricks: i128,
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

fn cube_of_bricks(origin: [i128; 3], level: u8) -> region_octree::BrickCube {
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
fn camera_in_voxels(position: [i128; 3]) -> Camera {
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
fn fine_bricks_of_a_box(renderer: &mut VoxelRenderer, low: [i128; 3], high: [i128; 3], block: u32) {
    for bz in low[2].div_euclid(32)..=(high[2] - 1).div_euclid(32) {
        for by in low[1].div_euclid(32)..=(high[1] - 1).div_euclid(32) {
            for bx in low[0].div_euclid(32)..=(high[0] - 1).div_euclid(32) {
                let mut voxels = Vec::new();
                for z in 0..32i128 {
                    for y in 0..32i128 {
                        for x in 0..32i128 {
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
            let cell = 1i128 << level;
            // A block of cells (8 by 8 by 8, a little off the corner of the node) of the red block.
            let (low_cell, high_cell) = ([8i128, 0, 8], [16i128, 8, 16]);
            let mut coarse = new_renderer(&context);
            coarse.upload_coarse_node(
                cube_of_bricks([0, 0, 0], level),
                &coarse_mesh(|x, y, z| {
                    let at = [x as i128, y as i128, z as i128];
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
        let edge = 1i128 << 18;
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
        let edge = 1i128 << 18;
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
        let node = |origin: [i128; 3]| cube_of_bricks(origin, 2);
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
fn pose(brick: [i128; 3], offset: [f32; 3], orientation: Orientation) -> GridPose {
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
        let seen = |brick: [i128; 3], local_brick: BrickCoordinate| {
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

// ---- The sun ----
//
// One green cube with no light of its own (`Some(0)`) and no ambient light, seen from the front: its face looks along +z, so a sun along +z is straight onto it. The sun's
// share of the colour is its light, times the cosine of the angle to the face, times how much of it gets past what is in the way; the sky's share is the sky light times the
// same share of the sun that reaches the place; the two add like light (a screen blend).

/// The atmosphere of the sun tests: no ambient light, a sun in this direction of this strength, and the sky's light of this strength.
fn sunny(sun_direction: [f32; 3], sun_light: u8, sky_light: u8) -> Atmosphere {
    Atmosphere {
        sky_color: SKY,
        fog_start: 1000.0,
        fog_end: 2000.0,
        sun_direction,
        sun_light,
        sky_light,
        ..Atmosphere::default()
    }
}

/// The brightness of the pixel at the middle of the green cube of the sun tests, seen from the front, with `light` of its own (None is the full light of a game with none).
fn lit_face(renderer: &mut VoxelRenderer, light: Option<u32>) -> f64 {
    place_blocks(renderer, ORIGIN_BRICK, &[([16, 16, 16], 2, 0)], light);
    let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
    linear_brightness(pixel(&render(renderer, &camera), SIZE / 2, SIZE / 2))
}

/// The brightness of a face that is fully lit and faces the sun squarely: the reference the other pictures are shares of.
fn fully_lit_face(context: &GraphicsContext) -> f64 {
    let mut renderer = new_renderer(context);
    renderer.set_atmosphere(sunny([0.0, 0.0, 1.0], 0, 0));
    lit_face(&mut renderer, None)
}

#[test]
fn a_sun_lights_a_face_by_the_cosine_of_its_angle_to_the_face() {
    for context in contexts() {
        let name = adapter_name(&context);
        let full = fully_lit_face(&context);
        let mut renderer = new_renderer(&context);
        let mut seen = |sun: [f32; 3], sun_light: u8| {
            renderer.set_atmosphere(sunny(sun, sun_light, 0));
            lit_face(&mut renderer, Some(0)) / full
        };
        // Squarely: all of it. A face squarely to a sun of no light is black (the ambient light and its own light are none).
        let squarely = seen([0.0, 0.0, 1.0], 255);
        assert!((0.99..1.01).contains(&squarely), "{name}: {squarely}");
        assert_eq!(seen([0.0, 0.0, 1.0], 0), 0.0, "{name}: a sun with no light");
        // Sixty degrees from the face: half. (The direction need not be a unit vector.)
        let sloped = seen([0.0, 3.0f32.sqrt() * 2.0, 2.0], 255);
        assert!((0.49..0.51).contains(&sloped), "{name}: {sloped}");
        // Half the light: half the brightness.
        let dim = seen([0.0, 0.0, 1.0], 128);
        assert!((0.49..0.51).contains(&dim), "{name}: {dim}");
        // Edge on, and from behind: the sun does not light a face it is behind.
        assert_eq!(seen([1.0, 0.0, 0.0], 255), 0.0, "{name}: edge on");
        assert_eq!(seen([0.0, 0.0, -1.0], 255), 0.0, "{name}: from behind");
    }
}

#[test]
fn the_sky_lights_a_face_the_sun_does_not_reach_a_little_less_than_one_it_does() {
    for context in contexts() {
        let name = adapter_name(&context);
        let full = fully_lit_face(&context);
        let mut renderer = new_renderer(&context);
        let mut seen = |sun: [f32; 3], sun_light: u8, sky_light: u8| {
            renderer.set_atmosphere(sunny(sun, sun_light, sky_light));
            lit_face(&mut renderer, Some(0)) / full
        };
        // The sun is behind the face: only the sky lights it, and as every face is shaded by the sun's side (0.4 away from it), by 0.4 of the sky's light.
        let in_shade = seen([0.0, 0.0, -1.0], 255, 255);
        assert!((0.39..0.41).contains(&in_shade), "{name}: {in_shade}");
        let dim_sky = seen([0.0, 0.0, -1.0], 255, 128);
        assert!((0.19..0.21).contains(&dim_sky), "{name}: {dim_sky}");
        // The sun and the sky together are a screen blend: the face the sun lights half (sixty degrees) and the sky lights by (0.4 + 0.6 * 0.5 =) 0.7 of 0.5 is
        // 0.35, and 0.35 and 0.5 make 0.35 + 0.5 * (1 - 0.35) = 0.675 of full.
        let both = seen([0.0, 3.0f32.sqrt() * 2.0, 2.0], 255, 128);
        assert!((0.66..0.69).contains(&both), "{name}: {both}");
        // The sky alone, with a sun of no light: the sky still lights, and a face the sun faces is the sky's full light.
        let only_sky = seen([0.0, 0.0, 1.0], 0, 255);
        assert!((0.99..1.01).contains(&only_sky), "{name}: {only_sky}");
    }
}

/// The ball of a planet whose surface is the face of the cube of the sun tests (its middle is at (16.5, 16.5, 17)): `radius` down along the face's normal, a thousandth of a
/// voxel more, so that the face is not inside it.
fn planet_under_the_face(radius: f32) -> SunOccluder {
    planet_with_the_face_this_deep(radius, f64::from(radius) + 0.001)
}

/// The ball of this radius whose middle is `depth` below the face along its normal: the face is inside the ball if the depth is less than the radius.
fn planet_with_the_face_this_deep(radius: f32, depth: f64) -> SunOccluder {
    let z = 17.0 - depth;
    let brick = (z / 32.0).floor();
    SunOccluder::Ball {
        centre: WorldPosition::new(
            BrickCoordinate::new(0, 0, brick as i128),
            [16.5, 16.5, (z - brick * 32.0) as f32],
        ),
        radius,
    }
}

#[test]
fn a_ball_in_the_way_puts_a_face_in_the_shadow_and_one_beside_it_does_not() {
    for context in contexts() {
        let name = adapter_name(&context);
        let full = fully_lit_face(&context);
        let mut renderer = new_renderer(&context);
        renderer.set_atmosphere(sunny([0.0, 0.0, 1.0], 255, 0));
        let open = lit_face(&mut renderer, Some(0)) / full;
        assert!((0.99..1.01).contains(&open), "{name}: {open}");
        // A ball between the face and the sun, 100 voxels out: the face is in its shadow.
        let in_the_way = SunOccluder::Ball {
            centre: WorldPosition::new(BrickCoordinate::new(0, 0, 3), [16.5, 16.5, 21.0]),
            radius: 30.0,
        };
        renderer.set_sun_occluders(&[in_the_way]);
        assert_eq!(
            lit_face(&mut renderer, Some(0)),
            0.0,
            "{name}: in the shadow"
        );
        // The same ball 200 voxels to the side is a long way from the sun's line: the face is in full sun.
        let beside = SunOccluder::Ball {
            centre: WorldPosition::new(BrickCoordinate::new(7, 0, 3), [16.5, 16.5, 21.0]),
            radius: 30.0,
        };
        renderer.set_sun_occluders(&[beside]);
        let at_the_side = lit_face(&mut renderer, Some(0)) / full;
        assert!((0.99..1.01).contains(&at_the_side), "{name}: {at_the_side}");
        // And none again.
        renderer.set_sun_occluders(&[]);
        let none = lit_face(&mut renderer, Some(0)) / full;
        assert!((0.99..1.01).contains(&none), "{name}: {none}");
    }
}

#[test]
fn the_shader_and_the_reference_agree_on_the_sunset_of_a_planet() {
    for context in contexts() {
        let name = adapter_name(&context);
        let full = fully_lit_face(&context);
        let mut renderer = new_renderer(&context);
        let planet = planet_under_the_face(1000.0);
        renderer.set_sun_occluders(&[planet]);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        let middle = Vec3::from_array(
            WorldPosition::new(ORIGIN_BRICK, [16.5, 16.5, 17.0]).relative_to(camera.position),
        );
        let (balls, ground) = renderer::balls_and_ground(&[planet], camera.position);
        // The sun from well up in the sky of the face to well below its horizon (the face looks along +z, which is up for the planet): the shader's share of the sun that
        // reaches the face's middle, and the rule's with the cosine of the angle to the face.
        let mut seen_something_in_between = false;
        for step in 0..=24 {
            let height = 0.3 - step as f32 * 0.025;
            let sun = Vec3::new(libm::cosf(height), 0.0, libm::sinf(height));
            renderer.set_atmosphere(sunny(sun.to_array(), 255, 0));
            let seen = lit_face(&mut renderer, Some(0)) / full;
            let share = renderer::sun_visibility(middle, sun, &balls, ground);
            let expected = f64::from(share * sun.z.max(0.0));
            assert!(
                (seen - expected).abs() < 0.03,
                "{name}: the sun at {height} radians: the shader says {seen}, the rule {expected}"
            );
            seen_something_in_between |= seen > 0.02 && seen < 0.2;
        }
        assert!(
            seen_something_in_between,
            "{name}: the steps cross the twilight"
        );
    }
}

#[test]
fn a_face_at_the_ground_of_a_planet_is_lit_by_a_normal_turned_to_the_way_out_and_one_high_over_it_is_not()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        let full = fully_lit_face(&context);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        let middle = Vec3::from_array(
            WorldPosition::new(ORIGIN_BRICK, [16.5, 16.5, 17.0]).relative_to(camera.position),
        );
        // The face looks along +z. A planet with its way out along +x from the face (the face is on its flank) turns the normal to 45 degrees; a planet whose way out is
        // along +z (the face is squarely on top of it) does not. A sun along +x lights the first by the cosine of 45 degrees and the second not at all. (The face is 5
        // voxels over the ground of both: right at the ground the light changes with the square root of the height, and a pixel is not at the middle of the face.)
        let flank = {
            // The middle of the ball is 1,005 voxels from the face along -x.
            let x: f64 = 16.5 - 1005.0;
            let brick = (x / 32.0).floor();
            SunOccluder::Ball {
                centre: WorldPosition::new(
                    BrickCoordinate::new(brick as i128, 0, 0),
                    [(x - brick * 32.0) as f32, 16.5, 17.0],
                ),
                radius: 1000.0,
            }
        };
        let on_top = planet_with_the_face_this_deep(1000.0, 1005.0);
        let high_over = {
            // 100,000 voxels from the same ball's surface: not turned.
            let x: f64 = 16.5 - 101_000.0;
            let brick = (x / 32.0).floor();
            SunOccluder::Ball {
                centre: WorldPosition::new(
                    BrickCoordinate::new(brick as i128, 0, 0),
                    [(x - brick * 32.0) as f32, 16.5, 17.0],
                ),
                radius: 1000.0,
            }
        };
        for (what, planet) in [
            ("on the flank", flank),
            ("on top", on_top),
            ("high over", high_over),
        ] {
            let mut renderer = new_renderer(&context);
            renderer.set_sun_occluders(&[planet]);
            let (balls, ground) = renderer::balls_and_ground(&[planet], camera.position);
            for sun in [Vec3::X, Vec3::new(1.0, 0.0, 1.0).normalize(), Vec3::Z] {
                renderer.set_atmosphere(sunny(sun.to_array(), 255, 0));
                let seen = lit_face(&mut renderer, Some(0)) / full;
                let light = renderer::sunlight(middle, Vec3::Z, sun, &balls, ground);
                let expected = f64::from(light.visible * light.normal.dot(sun).max(0.0));
                assert!(
                    (seen - expected).abs() < 0.03,
                    "{name}: {what}, the sun along {sun}: the shader says {seen}, the rule {expected}"
                );
            }
        }
        // And the cases differ as they should: the flank is lit by the sun along +x (a face looking along +z, turned half way to +x) and the face on top is not (its normal
        // is not turned and is at right angles to the sun).
        let seen_with = |planet: SunOccluder| {
            let mut renderer = new_renderer(&context);
            renderer.set_sun_occluders(&[planet]);
            renderer.set_atmosphere(sunny([1.0, 0.0, 0.0], 255, 0));
            lit_face(&mut renderer, Some(0)) / full
        };
        assert!(seen_with(flank) > 0.5, "{name}: {}", seen_with(flank));
        assert_eq!(seen_with(on_top), 0.0, "{name}");
        assert_eq!(
            seen_with(high_over),
            0.0,
            "{name}: a face high over a planet is not turned"
        );
    }
}

/// The sky's light and the twilight, high enough over the ground of a planet that the twilight is narrower than at it: the face is 200 voxels over a ball of 1,000, with the sun
/// below its horizon (so only the sky lights it, and by the 0.4 of a face the sun's side does not reach).
#[test]
fn the_sky_goes_with_the_sun_past_a_planet_and_the_twilight_is_narrower_the_higher_the_place() {
    for context in contexts() {
        let name = adapter_name(&context);
        let full = fully_lit_face(&context);
        let mut renderer = new_renderer(&context);
        let planet = planet_with_the_face_this_deep(1000.0, 1200.0);
        renderer.set_sun_occluders(&[planet]);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        let middle = Vec3::from_array(
            WorldPosition::new(ORIGIN_BRICK, [16.5, 16.5, 17.0]).relative_to(camera.position),
        );
        let (balls, ground) = renderer::balls_and_ground(&[planet], camera.position);
        let mut partly = 0;
        for step in 0..=30 {
            let height = -0.45 - step as f32 * 0.01;
            let sun = Vec3::new(libm::cosf(height), 0.0, libm::sinf(height));
            renderer.set_atmosphere(sunny(sun.to_array(), 0, 255));
            let seen = lit_face(&mut renderer, Some(0)) / full;
            let visible = renderer::sun_visibility(middle, sun, &balls, ground);
            let expected = f64::from(0.4 * visible);
            assert!(
                (seen - expected).abs() < 0.02,
                "{name}: the sun at {height} radians: the shader says {seen}, the rule {expected}"
            );
            partly += usize::from(seen > 0.02 && seen < 0.38);
        }
        assert!(partly >= 3, "{name}: the sky fades over {partly} steps");
        // With nothing in the way the sky lights the face at the same sun.
        renderer.set_sun_occluders(&[]);
        let open = lit_face(&mut renderer, Some(0)) / full;
        assert!((0.39..0.41).contains(&open), "{name}: {open}");
    }
}

#[test]
fn a_face_inside_a_ball_is_lit_as_the_terminator_of_the_ball_says_in_its_direction() {
    for context in contexts() {
        let name = adapter_name(&context);
        let full = fully_lit_face(&context);
        let mut renderer = new_renderer(&context);
        // The face is 300 voxels inside a ball of 1,000: a face of a far node of a planet is under the ground, inside the core, like this.
        let planet = planet_with_the_face_this_deep(1000.0, 700.0);
        renderer.set_sun_occluders(&[planet]);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        let middle = Vec3::from_array(
            WorldPosition::new(ORIGIN_BRICK, [16.5, 16.5, 17.0]).relative_to(camera.position),
        );
        let (balls, ground) = renderer::balls_and_ground(&[planet], camera.position);
        let mut lit = 0;
        let mut dark = 0;
        for step in 0..=24 {
            let height = 0.3 - step as f32 * 0.025;
            let sun = Vec3::new(libm::cosf(height), 0.0, libm::sinf(height));
            renderer.set_atmosphere(sunny(sun.to_array(), 255, 0));
            let seen = lit_face(&mut renderer, Some(0)) / full;
            let share = renderer::sun_visibility(middle, sun, &balls, ground);
            let expected = f64::from(share * sun.z.max(0.0));
            assert!(
                (seen - expected).abs() < 0.03,
                "{name}: the sun at {height} radians: the shader says {seen}, the rule {expected}"
            );
            lit += usize::from(seen > 0.2);
            dark += usize::from(seen == 0.0);
        }
        // (Above the horizon of the face the planet's day is on, and well below it the night: not all black as a place inside a ball once was.)
        assert!(lit >= 3 && dark >= 5, "{name}: {lit} lit, {dark} dark");
    }
}

#[test]
fn flat_ground_hides_a_sun_below_the_horizon_and_a_sun_above_it_is_whole() {
    for context in contexts() {
        let name = adapter_name(&context);
        let full = fully_lit_face(&context);
        let mut renderer = new_renderer(&context);
        renderer.set_sun_occluders(&[SunOccluder::FlatGround]);
        // A face that looks along +z, with the level's up along +y: the sun 30 degrees above the horizon in front of it is whole (cosine 0.866 to the face), and 30 degrees
        // below the horizon, still in front of the face, it is hidden by the ground.
        let (sine, cosine) = (0.5f32, 3.0f32.sqrt() / 2.0);
        renderer.set_atmosphere(sunny([0.0, sine, cosine], 255, 0));
        let above = lit_face(&mut renderer, Some(0)) / full;
        assert!((0.85..0.88).contains(&above), "{name}: {above}");
        renderer.set_atmosphere(sunny([0.0, -sine, cosine], 255, 0));
        assert_eq!(
            lit_face(&mut renderer, Some(0)),
            0.0,
            "{name}: below the horizon"
        );
        // Without the ground the same sun lights the face.
        renderer.set_sun_occluders(&[]);
        let without = lit_face(&mut renderer, Some(0)) / full;
        assert!((0.85..0.88).contains(&without), "{name}: {without}");
    }
}

#[test]
fn a_ball_a_billion_light_years_away_is_no_shadow_and_no_number_that_is_not_one() {
    for context in contexts() {
        let name = adapter_name(&context);
        let full = fully_lit_face(&context);
        let mut renderer = new_renderer(&context);
        renderer.set_atmosphere(sunny([0.0, 0.0, 1.0], 255, 0));
        // 10^35 voxels out: off the sun's line, and on it (a speck there: its disc is 10^-29 of a radian across).
        let far = |brick: [i128; 3]| SunOccluder::Ball {
            centre: WorldPosition::new(
                BrickCoordinate::new(brick[0], brick[1], brick[2]),
                [0.0; 3],
            ),
            radius: 1.0e6,
        };
        let a_lot: i128 = 3_000_000_000_000_000_000_000_000_000_000_000;
        renderer.set_sun_occluders(&[far([a_lot, 0, 0])]);
        let sideways = lit_face(&mut renderer, Some(0)) / full;
        assert!((0.99..1.01).contains(&sideways), "{name}: {sideways}");
        renderer.set_sun_occluders(&[far([0, 0, a_lot])]);
        let in_line = lit_face(&mut renderer, Some(0)) / full;
        assert!(
            (0.99..1.01).contains(&in_line),
            "{name}: a speck: {in_line}"
        );
    }
}

#[test]
fn the_sides_of_faces_are_shaded_by_the_side_direction_when_there_is_one_and_the_sun_otherwise() {
    for context in contexts() {
        let name = adapter_name(&context);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        let seen = |atmosphere: Atmosphere| {
            let mut renderer = new_renderer(&context);
            renderer.set_atmosphere(atmosphere);
            place_blocks(
                &mut renderer,
                ORIGIN_BRICK,
                &[([16, 16, 16], 2, 0)],
                Some(255),
            );
            render(&mut renderer, &camera)
        };
        let toward = [0.3, 0.2, 1.0];
        let away = [0.3, 0.2, -1.0];
        let by_the_sun = |direction| Atmosphere {
            sun_direction: direction,
            ..Atmosphere::default()
        };
        // With no side direction the sun's direction shades the sides of faces: a face is brighter toward the sun than away from it.
        assert!(seen(by_the_sun(toward)) != seen(by_the_sun(away)), "{name}");
        // With one, the sun can be anywhere and the sides are shaded as the side direction says: the same pictures as if it were the sun.
        let with_side = |sun, side| Atmosphere {
            sun_direction: sun,
            side_direction: Some(side),
            ..Atmosphere::default()
        };
        assert!(
            seen(with_side(away, toward)) == seen(by_the_sun(toward)),
            "{name}"
        );
        assert!(
            seen(with_side(toward, away)) == seen(by_the_sun(away)),
            "{name}"
        );
    }
}

#[test]
fn a_sun_with_no_light_leaves_every_picture_as_it_was_whatever_is_in_its_way() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        place_blocks(
            &mut renderer,
            ORIGIN_BRICK,
            &[([16, 16, 16], 2, 0)],
            Some(100),
        );
        let before = render(&mut renderer, &camera);
        renderer.set_sun_occluders(&[planet_under_the_face(1000.0), SunOccluder::FlatGround]);
        assert!(render(&mut renderer, &camera) == before, "{name}");
    }
}

// --- shadows ---
//
// A node of detail level `n` is the cells of a brick, each `2^n` voxels wide, so one scene is a picture at every scale if the camera, the near plane and the fog are scaled with it:
// a block four cells high is a block of four voxels, of 4,096, or of four million, and the shadows are the same picture. The scenes are built from cells for that reason.
//
// A shadow is told from light by the brightness of the pixel a place on the floor lands on with the shadows on, over the same pixel with them off: the sun's light on a
// floor that looks at it from 53 degrees up is `0.876` of full and the sky's `0.38` of that, and in the shadow the sky's light is the share `sky_light_in_shadow` of what it
// was, `0.095`: a shadow is `0.109` of the light and a place the sun reaches is `1.0`.

const SHADOWED_ATMOSPHERE_FILL: u8 = 64;

/// What the cell at `(x, y, z)` of the scenes of the shadow tests is made of.
#[derive(Clone, Copy)]
enum ShadowScene {
    /// The floor of the node (one cell thick) and a block of four cells on it in the middle.
    Block,
    /// The floor and a roof of one cell, eight cells up, over the first ten cells along x.
    Roof,
}

impl ShadowScene {
    fn cell(self, x: usize, y: usize, z: usize) -> Option<u32> {
        if y == 0 {
            return Some(2);
        }
        match self {
            ShadowScene::Block => {
                ((14..18).contains(&x) && (1..5).contains(&y) && (14..18).contains(&z))
                    .then_some(RED)
            }
            ShadowScene::Roof => (x < 10 && y == 7).then_some(RED),
        }
    }
}

/// The mesh of a coarse node of these cells, each lit by `light` of its own.
fn coarse_mesh_lit(
    solid: impl Fn(usize, usize, usize) -> Option<u32>,
    light: u8,
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
                        light,
                        exposure: 0,
                    },
                    None => CoarseCell::empty(0, light),
                });
            }
        }
    }
    voxel_meshing::mesh_coarse_node(&CoarseNode::from_cells(cells), &[None; 6]).pack()
}

/// Puts a scene at a detail level: the bricks of voxels for level 0, a node whose cells are `2^level` voxels for any other.
fn upload_shadow_scene(renderer: &mut VoxelRenderer, scene: ShadowScene, level: u8) {
    if level == 0 {
        let mut blocks = Vec::new();
        for z in 0..32usize {
            for y in 0..32usize {
                for x in 0..32usize {
                    if let Some(block) = scene.cell(x, y, z) {
                        blocks.push(([x as u8, y as u8, z as u8], block, 0));
                    }
                }
            }
        }
        place_blocks(renderer, ORIGIN_BRICK, &blocks, Some(0));
    } else {
        renderer.upload_coarse_node(
            cube_of_bricks([0, 0, 0], level),
            &coarse_mesh_lit(|x, y, z| scene.cell(x, y, z), 0),
        );
    }
}

/// 2^level, as an `f64`.
fn scale_of(level: u8) -> f64 {
    (1u128 << level) as f64
}

/// The atmosphere of the shadow tests for a scene of this level: the sun in this direction at full light, the sky's light, fog beyond the scene, shadows on or off.
fn shadow_atmosphere(level: u8, sun: [f32; 3], shadows: bool) -> Atmosphere {
    let scale = scale_of(level) as f32;
    Atmosphere {
        sky_color: SKY,
        fog_start: 100.0 * scale,
        fog_end: 200.0 * scale,
        sun_direction: sun,
        sun_light: 255,
        sky_light: 110,
        shadows,
        sky_light_in_shadow: SHADOWED_ATMOSPHERE_FILL,
        ..Atmosphere::default()
    }
}

/// A camera at a place given in cells of a node of this level (whole numbers), looking this way, with the near plane scaled to the level too.
fn camera_in_cells(level: u8, cells: [i128; 3], yaw: f32, pitch: f32) -> Camera {
    let mut camera = camera_in_voxels(cells.map(|cell| cell << level));
    camera.yaw = yaw;
    camera.pitch = pitch;
    camera.near_plane = 0.1 * scale_of(level) as f32;
    camera
}

/// The pixel that a place lands on, given relative to the camera in voxels, if it is in the picture.
fn pixel_of(camera: &Camera, relative: [f64; 3]) -> Option<(u32, u32)> {
    let clip = camera.view_projection(1.0)
        * glam::Vec4::new(
            relative[0] as f32,
            relative[1] as f32,
            relative[2] as f32,
            1.0,
        );
    if clip.w <= 0.0 {
        return None;
    }
    let (x, y) = (clip.x / clip.w, clip.y / clip.w);
    if x.abs() >= 1.0 || y.abs() >= 1.0 {
        return None;
    }
    Some((
        ((x * 0.5 + 0.5) * SIZE as f32) as u32,
        ((0.5 - y * 0.5) * SIZE as f32) as u32,
    ))
}

/// A place the pictures of the shadow tests are asked about: in cells of the scene, and whether it is in the shadow.
type Probe = ([f64; 3], bool);

/// What is wrong with the shadows in the picture from `camera`, which is at `camera_cells`: for each probe, the brightness of its pixel with the shadows on over that with
/// them off must be 1 for a place the sun reaches and about `0.109` for one in a shadow. Returns what is wrong, empty if all is well; and how many probes were in the picture.
fn shadow_failures(
    renderer: &mut VoxelRenderer,
    level: u8,
    sun: [f32; 3],
    camera: &Camera,
    camera_cells: [f64; 3],
    probes: &[Probe],
) -> (Vec<String>, usize) {
    renderer.set_atmosphere(shadow_atmosphere(level, sun, false));
    let off = render(renderer, camera);
    renderer.set_atmosphere(shadow_atmosphere(level, sun, true));
    let on = render(renderer, camera);
    let mut failures = Vec::new();
    let mut seen = 0;
    for &(cells, shadowed) in probes {
        let relative =
            std::array::from_fn(|axis| (cells[axis] - camera_cells[axis]) * scale_of(level));
        let Some((x, y)) = pixel_of(camera, relative) else {
            continue;
        };
        seen += 1;
        let (with, without) = (
            linear_brightness(pixel(&on, x, y)),
            linear_brightness(pixel(&off, x, y)),
        );
        let ratio = with / without;
        let right = if shadowed {
            (0.07..0.15).contains(&ratio)
        } else {
            (0.97..1.03).contains(&ratio)
        };
        if !right {
            failures.push(format!(
                "{cells:?} at pixel ({x}, {y}) is {} but is {ratio:.3} of the light",
                if shadowed { "in a shadow" } else { "lit" }
            ));
        }
    }
    (failures, seen)
}

/// The sun of the scenes: 53 degrees up, over the +x side, so a block's shadow falls to -x.
const SUN_OVER_THE_EAST: [f32; 3] = [0.6, 0.8, 0.0];

fn probe_set(shadowed: &[[f64; 3]], lit: &[[f64; 3]]) -> Vec<Probe> {
    shadowed
        .iter()
        .map(|&cells| (cells, true))
        .chain(lit.iter().map(|&cells| (cells, false)))
        .collect()
}

/// Both scenes, with a camera that sees the shadows of each and the places probed: for the block, from above; for the roof, from the side, so as to see under it.
fn shadow_scene_setup(scene: ShadowScene, level: u8) -> (Camera, [i128; 3], Vec<Probe>) {
    match scene {
        ShadowScene::Block => {
            let cells = [16, 21, 16];
            let camera = camera_in_cells(level, cells, 0.0, -Camera::MAXIMUM_PITCH);
            // The shadow of the block lies from x = 11 to 14, along the block's width (z from 14 to 18); the camera is over the middle of the block, so the block hides
            // nothing of the floor that is probed.
            let shadowed = [
                [11.8, 1.0, 15.0],
                [12.5, 1.0, 16.0],
                [13.2, 1.0, 17.0],
                [12.5, 1.0, 15.0],
            ];
            let lit = [
                [3.0, 1.0, 5.0],
                [6.0, 1.0, 16.0],
                [9.0, 1.0, 27.0],
                [22.0, 1.0, 16.0],
                [26.0, 1.0, 5.0],
                [12.5, 1.0, 6.0],
                [12.5, 1.0, 26.0],
                [16.0, 5.0, 16.0],
                [15.0, 5.0, 17.0],
            ];
            (camera, cells, probe_set(&shadowed, &lit))
        }
        ShadowScene::Roof => {
            let cells = [24, 5, 16];
            let camera = camera_in_cells(level, cells, std::f32::consts::FRAC_PI_2, -0.25);
            // The roof is over x from 0 to 10 at the height of 7 to 8; the sun comes in from the east at 53 degrees, so the floor is dark from the west edge to x = 5.5 and light
            // from there, though still under the roof.
            let shadowed = [
                [1.5, 1.0, 12.0],
                [2.5, 1.0, 16.0],
                [3.5, 1.0, 20.0],
                [4.5, 1.0, 16.0],
            ];
            let lit = [
                [6.7, 1.0, 12.0],
                [7.5, 1.0, 16.0],
                [9.0, 1.0, 20.0],
                [12.0, 1.0, 16.0],
                [16.0, 1.0, 12.0],
                [14.0, 1.0, 20.0],
            ];
            (camera, cells, probe_set(&shadowed, &lit))
        }
    }
}

#[test]
fn a_block_and_a_roof_shade_the_floor_where_the_sun_puts_the_shadow_from_a_brick_to_a_mountain_range()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        for scene in [ShadowScene::Block, ShadowScene::Roof] {
            // 0 is bricks of voxels; 1 and 4 are nodes of cells of 2 and 16 voxels; 9 is cells of 512, a node as wide as a town; 15, cells of 32,768 voxels, mountains
            // 500,000 voxels high among them; 20, cells of a million: a node 33 million voxels across, in which the block is four million voxels high.
            for level in [0u8, 1, 4, 9, 15, 20] {
                let mut renderer = new_renderer(&context);
                upload_shadow_scene(&mut renderer, scene, level);
                let (camera, cells, probes) = shadow_scene_setup(scene, level);
                let (failures, seen) = shadow_failures(
                    &mut renderer,
                    level,
                    SUN_OVER_THE_EAST,
                    &camera,
                    cells.map(|cell| cell as f64),
                    &probes,
                );
                assert_eq!(
                    seen,
                    probes.len(),
                    "{name} level {level}: a probe is out of the picture"
                );
                assert!(
                    failures.is_empty(),
                    "{name} {} level {level}:\n{}",
                    if matches!(scene, ShadowScene::Block) {
                        "block"
                    } else {
                        "roof"
                    },
                    failures.join("\n")
                );
            }
        }
    }
}

/// The floor of a brick: every voxel at height 0.
fn floor_voxels() -> Vec<([u8; 3], u32, u32)> {
    let mut blocks = Vec::new();
    for z in 0..32u8 {
        for x in 0..32u8 {
            blocks.push(([x, 0, z], 2, 0));
        }
    }
    blocks
}

#[test]
fn a_mountain_half_a_million_voxels_high_shades_a_floor_of_voxels_in_front_of_it_to_within_a_voxel_of_where_its_ridge_puts_the_shadow()
 {
    // The floor is bricks of voxels, three by three, about the origin. The mountain is a node of cells of 32,768 voxels: its west face is at x = 524,288, it is 524,288 high
    // (16 cells) and 1,048,576 wide along z. The sun is along the line from the floor to the ridge of the mountain at x = 16: a floor point of x over 17 is in the shadow of the
    // mountain (a ray from it up the sun reaches the west face below the ridge), and one under 17 sees the sun over the ridge.
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        let blocks = floor_voxels();
        for bz in -1..=1 {
            for bx in -1..=1 {
                place_blocks(
                    &mut renderer,
                    BrickCoordinate::new(bx, 0, bz),
                    &blocks,
                    Some(0),
                );
            }
        }
        renderer.upload_coarse_node(
            cube_of_bricks([0, 0, 0], 15),
            &coarse_mesh_lit(|x, y, _| (x >= 16 && y < 16).then_some(RED), 0),
        );
        let sun = [524_272.0f32, 524_288.0, 0.0];
        let cells = [17i128, 30, 24];
        let camera = camera_in_voxels(cells);
        let mut camera = camera;
        camera.pitch = -Camera::MAXIMUM_PITCH;
        let mut probes = Vec::new();
        for z in [10.0, 24.0, 40.0] {
            for x in [-2.0, 2.0, 8.0, 13.0] {
                probes.push(([x, 1.0, z], false));
            }
            for x in [21.0, 26.0, 32.0, 36.0] {
                probes.push(([x, 1.0, z], true));
            }
        }
        // (Probes are in voxels here: the scale is 1.)
        let (failures, seen) = shadow_failures(
            &mut renderer,
            0,
            sun,
            &camera,
            cells.map(|cell| cell as f64),
            &probes,
        );
        assert_eq!(seen, probes.len(), "{name}: a probe is out of the picture");
        assert!(failures.is_empty(), "{name}:\n{}", failures.join("\n"));
    }
}

#[test]
fn a_floor_nothing_stands_on_is_as_bright_with_the_shadows_on_at_every_distance_and_every_sun_up_to_a_grazing_one()
 {
    // A floor seen along its length, from 5 voxels to the fog at 600: five cascades and the seams between them, three suns. Nothing is in the way of the light, so no pixel
    // may be darker than it is with the shadows off: that is no acne, and no line where one cascade gives way to the next.
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        let blocks = floor_voxels();
        for bz in -16..=0 {
            for bx in -3..=3 {
                place_blocks(
                    &mut renderer,
                    BrickCoordinate::new(bx, 0, bz),
                    &blocks,
                    Some(0),
                );
            }
        }
        let mut camera = camera_at(ORIGIN_BRICK, [16.0, 4.0, 30.0]);
        camera.pitch = -0.15;
        for (label, sun) in [
            ("high", [0.1f32, 1.0, 0.2]),
            ("45 degrees", [1.0, 1.0, 0.0]),
            ("low", [0.3, 0.17, 0.9]),
        ] {
            let atmosphere = |shadows| Atmosphere {
                fog_start: 400.0,
                fog_end: 600.0,
                ..shadow_atmosphere(0, sun, shadows)
            };
            renderer.set_atmosphere(atmosphere(false));
            let off = render(&mut renderer, &camera);
            renderer.set_atmosphere(atmosphere(true));
            let on = render(&mut renderer, &camera);
            let mut floor = 0;
            let mut darker = Vec::new();
            for y in 0..SIZE {
                for x in 0..SIZE {
                    let (with, without) = (pixel(&on, x, y), pixel(&off, x, y));
                    if is_sky(without) {
                        continue;
                    }
                    floor += 1;
                    if without[..3]
                        .iter()
                        .zip(&with[..3])
                        .any(|(without, with)| without.abs_diff(*with) > 2)
                    {
                        darker.push((x, y, without, with));
                    }
                }
            }
            assert!(floor > 1500, "{name} {label}: only {floor} pixels of floor");
            assert!(
                darker.is_empty(),
                "{name} {label}: {} of {floor} pixels differ, the first {:?}",
                darker.len(),
                &darker[..darker.len().min(4)]
            );
        }
    }
}

#[test]
fn beyond_the_reach_of_the_cascades_the_picture_is_the_one_without_shadows_and_nothing_is_not_a_number()
 {
    // The cascades reach 67 million voxels at most; a scene of nodes of cells of 2^24 voxels or more, seen from within it, is further off than that.
    for context in contexts() {
        let name = adapter_name(&context);
        for level in [24u8, 40, 60, 90] {
            let mut renderer = new_renderer(&context);
            upload_shadow_scene(&mut renderer, ShadowScene::Block, level);
            let (camera, cells, _) = shadow_scene_setup(ShadowScene::Block, level);
            let _ = cells;
            renderer.set_atmosphere(shadow_atmosphere(level, SUN_OVER_THE_EAST, false));
            let off = render(&mut renderer, &camera);
            renderer.set_atmosphere(shadow_atmosphere(level, SUN_OVER_THE_EAST, true));
            let on = render(&mut renderer, &camera);
            assert!(
                pixels_where(&off, |pixel| !is_sky(pixel)) > 1500,
                "{name} level {level}: the scene is not in the picture"
            );
            assert!(on == off, "{name} level {level}: the pictures differ");
        }
    }
}

#[test]
fn shadows_on_in_a_world_with_no_sun_are_no_change_and_cost_nothing() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        upload_shadow_scene(&mut renderer, ShadowScene::Block, 0);
        let (camera, _, _) = shadow_scene_setup(ShadowScene::Block, 0);
        let plain = Atmosphere {
            sun_light: 0,
            sky_light: 0,
            ..shadow_atmosphere(0, SUN_OVER_THE_EAST, false)
        };
        renderer.set_atmosphere(plain);
        let before = render(&mut renderer, &camera);
        renderer.set_atmosphere(Atmosphere {
            shadows: true,
            ..plain
        });
        let (after, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert!(after == before, "{name}");
        assert_eq!(
            (
                statistics.shadow_cascades_drawn,
                statistics.shadow_casters_drawn
            ),
            (0, 0),
            "{name}"
        );
    }
}

#[test]
fn the_maps_are_drawn_only_while_the_sun_can_light_something_and_only_for_the_cascades_with_something_to_light()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        upload_shadow_scene(&mut renderer, ShadowScene::Block, 0);
        let (camera, _, _) = shadow_scene_setup(ShadowScene::Block, 0);
        // Shadows off: no maps.
        renderer.set_atmosphere(shadow_atmosphere(0, SUN_OVER_THE_EAST, false));
        let (_, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_eq!(statistics.shadow_cascades_drawn, 0, "{name}");
        // On: the scene is 20 to 40 voxels away, in the first cascade (it reaches 27.8 voxels) and the second (mixed in from 23.6, and reaching 55.6), and the block is drawn
        // into them.
        renderer.set_atmosphere(shadow_atmosphere(0, SUN_OVER_THE_EAST, true));
        let (day, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert!(
            (1..=2).contains(&statistics.shadow_cascades_drawn),
            "{name}: {}",
            statistics.shadow_cascades_drawn
        );
        assert!(statistics.shadow_casters_drawn >= 1, "{name}");
        // The ground of a flat level hides a sun below the horizon from everything: nothing is drawn.
        renderer.set_sun_occluders(&[SunOccluder::FlatGround]);
        renderer.set_atmosphere(shadow_atmosphere(0, [0.6, -0.8, 0.0], true));
        let (_, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_eq!(
            (
                statistics.shadow_cascades_drawn,
                statistics.shadow_casters_drawn
            ),
            (0, 0),
            "{name}: night over flat ground"
        );
        // And above the horizon they are lit again by the maps drawn before, which were kept: nothing is drawn, and the picture is the one of the day before.
        renderer.set_atmosphere(shadow_atmosphere(0, SUN_OVER_THE_EAST, true));
        let (again, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_eq!(
            statistics.shadow_cascades_drawn, 0,
            "{name}: the maps are kept"
        );
        assert!(again == day, "{name}: kept maps, the same shadows");
    }
}

#[test]
fn a_map_is_kept_while_nothing_changes_and_the_camera_stays_near_its_middle_and_the_shadow_is_where_the_sun_puts_it_for_the_camera_in_its_new_place()
 {
    // The camera is over the block, outside the sphere that holds the node (which is how far a node can be from the camera and still be counted as near it): the nearest the
    // scene is is six cells, and the cascades that serve it are those at least that wide.
    for context in contexts() {
        let name = adapter_name(&context);
        for level in [0u8, 6, 15] {
            let mut renderer = new_renderer(&context);
            upload_shadow_scene(&mut renderer, ShadowScene::Block, level);
            let cells = [16i128, 50, 16];
            let camera = camera_in_cells(level, cells, 0.0, -Camera::MAXIMUM_PITCH);
            let probes = probe_set(
                &[[12.5, 1.0, 15.0], [12.5, 1.0, 16.0], [12.5, 1.0, 17.0]],
                &[
                    [3.0, 1.0, 5.0],
                    [6.0, 1.0, 16.0],
                    [26.0, 1.0, 5.0],
                    [22.0, 1.0, 16.0],
                    [16.0, 5.0, 16.0],
                ],
            );
            renderer.set_atmosphere(shadow_atmosphere(level, SUN_OVER_THE_EAST, true));
            let (_, first) = renderer.render_to_rgba(&camera, SIZE, SIZE);
            assert!(first.shadow_cascades_drawn >= 1, "{name} level {level}");
            // The camera moves by a fifth of a cell along x, y, z and all of them (far less than the slack of the cascades that serve the scene, which are eight cells wide or
            // more): nothing is drawn again, and the shadow is where the sun puts it for the camera in its new place.
            let a_fifth = (scale_of(level) * 0.2) as f32;
            let cell_cells = cells.map(|cell| cell as f64);
            for movement in [
                [a_fifth, 0.0, 0.0],
                [0.0, a_fifth, 0.0],
                [0.0, 0.0, -a_fifth],
                [a_fifth, -a_fifth, a_fifth],
            ] {
                let mut moved = camera;
                moved.position = moved.position.translated(movement);
                let (_, kept) = renderer.render_to_rgba(&moved, SIZE, SIZE);
                assert_eq!(
                    kept.shadow_cascades_drawn, 0,
                    "{name} level {level} {movement:?}: a map was drawn again"
                );
                let moved_cells = std::array::from_fn(|axis| {
                    cell_cells[axis] + f64::from(movement[axis]) / scale_of(level)
                });
                let (failures, seen) = shadow_failures(
                    &mut renderer,
                    level,
                    SUN_OVER_THE_EAST,
                    &moved,
                    moved_cells,
                    &probes,
                );
                assert_eq!(seen, probes.len(), "{name} level {level} {movement:?}");
                assert!(
                    failures.is_empty(),
                    "{name} level {level} {movement:?}:\n{}",
                    failures.join("\n")
                );
            }
            // Twenty cells along x is more than the slack of the narrowest cascade that serves the scene: that one is drawn again.
            let mut far = camera;
            far.position = far
                .position
                .translated([(scale_of(level) * 20.0) as f32, 0.0, 0.0]);
            let (_, redrawn) = renderer.render_to_rgba(&far, SIZE, SIZE);
            assert!(
                redrawn.shadow_cascades_drawn >= 1,
                "{name} level {level}: the maps were not drawn again for a camera 20 cells away"
            );
        }
    }
}

/// How much of the light a place on the floor of a shadow scene gets in a picture with the shadows on, over what it gets with them off, in the picture `on` and the picture
/// `off` taken from `camera`, which is at `camera_cells`; None if the place is not in the picture.
fn light_at(
    on: &[u8],
    off: &[u8],
    level: u8,
    camera: &Camera,
    camera_cells: [f64; 3],
    cells: [f64; 3],
) -> Option<f64> {
    let relative = std::array::from_fn(|axis| (cells[axis] - camera_cells[axis]) * scale_of(level));
    let (x, y) = pixel_of(camera, relative)?;
    Some(linear_brightness(pixel(on, x, y)) / linear_brightness(pixel(off, x, y)))
}

/// The pictures of a renderer with the shadows on and off, from `camera`, and the statistics of the one with them on. (Leaves them on.)
fn on_and_off(
    renderer: &mut VoxelRenderer,
    level: u8,
    sun: [f32; 3],
    camera: &Camera,
) -> (Vec<u8>, Vec<u8>, RenderStatistics) {
    renderer.set_atmosphere(shadow_atmosphere(level, sun, true));
    let (on, statistics) = renderer.render_to_rgba(camera, SIZE, SIZE);
    renderer.set_atmosphere(shadow_atmosphere(level, sun, false));
    let off = render(renderer, camera);
    renderer.set_atmosphere(shadow_atmosphere(level, sun, true));
    (on, off, statistics)
}

#[test]
fn a_block_that_is_placed_or_taken_away_changes_the_shadow_in_the_next_picture_of_the_fine_cascades()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        let (camera, cells, _) = shadow_scene_setup(ShadowScene::Block, 0);
        let cells = cells.map(|cell| cell as f64);
        let in_the_shadow = [12.5, 1.0, 16.0];
        // The floor alone: the place the block will shade is lit.
        place_blocks(&mut renderer, ORIGIN_BRICK, &floor_voxels(), Some(0));
        let (on, off, first) = on_and_off(&mut renderer, 0, SUN_OVER_THE_EAST, &camera);
        assert!(first.shadow_cascades_drawn >= 1, "{name}");
        let light = light_at(&on, &off, 0, &camera, cells, in_the_shadow).unwrap();
        assert!((0.97..1.03).contains(&light), "{name}: {light}");
        // The block is placed: the next picture has its shadow (the maps that serve this scene are of the finest, and are drawn again as soon as the terrain changes).
        upload_shadow_scene(&mut renderer, ShadowScene::Block, 0);
        let (on, off, placed) = on_and_off(&mut renderer, 0, SUN_OVER_THE_EAST, &camera);
        assert!(
            placed.shadow_cascades_drawn >= 1,
            "{name}: the maps were not drawn again"
        );
        let light = light_at(&on, &off, 0, &camera, cells, in_the_shadow).unwrap();
        assert!((0.07..0.15).contains(&light), "{name}: placed: {light}");
        // And nothing changed in the picture after: the maps are kept.
        let (_, _, kept) = on_and_off(&mut renderer, 0, SUN_OVER_THE_EAST, &camera);
        assert_eq!(kept.shadow_cascades_drawn, 0, "{name}");
        // The block is taken away again: the shadow goes.
        place_blocks(&mut renderer, ORIGIN_BRICK, &floor_voxels(), Some(0));
        let (on, off, taken) = on_and_off(&mut renderer, 0, SUN_OVER_THE_EAST, &camera);
        assert!(taken.shadow_cascades_drawn >= 1, "{name}");
        let light = light_at(&on, &off, 0, &camera, cells, in_the_shadow).unwrap();
        assert!((0.97..1.03).contains(&light), "{name}: taken away: {light}");
    }
}

#[test]
fn a_sun_that_moves_a_little_keeps_the_maps_a_little_more_draws_them_again_and_one_that_jumps_puts_the_shadow_where_the_new_sun_does()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        upload_shadow_scene(&mut renderer, ShadowScene::Block, 0);
        let (camera, cells, _) = shadow_scene_setup(ShadowScene::Block, 0);
        let cells = cells.map(|cell| cell as f64);
        let turned = |angle: f32| {
            // The sun turned about the z axis from over the east by `angle` radians: toward the vertical.
            let (sine, cosine) = (libm::sinf(angle), libm::cosf(angle));
            let (x, y) = (SUN_OVER_THE_EAST[0], SUN_OVER_THE_EAST[1]);
            [x * cosine - y * sine, x * sine + y * cosine, 0.0]
        };
        let (_, _, first) = on_and_off(&mut renderer, 0, SUN_OVER_THE_EAST, &camera);
        assert!(first.shadow_cascades_drawn >= 1, "{name}");
        // Two thousandths of a radian: kept. (The sine of the angle the maps allow is 0.005.)
        let (_, _, a_little) = on_and_off(&mut renderer, 0, turned(0.002), &camera);
        assert_eq!(a_little.shadow_cascades_drawn, 0, "{name}: 0.002 rad");
        // Seven thousandths from where the maps were drawn: drawn again.
        let (_, _, more) = on_and_off(&mut renderer, 0, turned(0.007), &camera);
        assert!(more.shadow_cascades_drawn >= 1, "{name}: 0.007 rad");
        let (_, _, kept) = on_and_off(&mut renderer, 0, turned(0.0072), &camera);
        assert_eq!(kept.shadow_cascades_drawn, 0, "{name}: 0.0072 rad");
        // A sun that has jumped (a quarter of a radian: a player's `.time`): drawn again at once, and the shadow of the block, four cells high, lies where the new sun puts
        // it: the block's top edge has a shadow at 14 - 4 * x / y, and the sun is now (0.8, 0.6): at 8.67, the shadow from there to 14.
        let sun = [0.8, 0.6, 0.0];
        let (on, off, jumped) = on_and_off(&mut renderer, 0, sun, &camera);
        assert!(jumped.shadow_cascades_drawn >= 1, "{name}: the jump");
        for (x, shadowed) in [
            (10.0, true),
            (11.5, true),
            (13.0, true),
            (6.5, false),
            (7.5, false),
            (22.0, false),
        ] {
            let light = light_at(&on, &off, 0, &camera, cells, [x, 1.0, 16.0]).unwrap();
            let right = if shadowed {
                (0.07..0.15).contains(&light)
            } else {
                (0.97..1.03).contains(&light)
            };
            assert!(right, "{name}: x = {x}: {light}");
        }
    }
}

#[test]
fn a_player_standing_on_the_floor_shades_it_and_moving_changes_the_shadow_in_the_next_picture() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        place_blocks(&mut renderer, ORIGIN_BRICK, &floor_voxels(), Some(0));
        let (camera, cells, _) = shadow_scene_setup(ShadowScene::Block, 0);
        let cells = cells.map(|cell| cell as f64);
        let (on, off, first) = on_and_off(&mut renderer, 0, SUN_OVER_THE_EAST, &camera);
        assert!(first.shadow_cascades_drawn >= 1, "{name}");
        // The floor is lit at x = 20 and x = 12.
        for x in [12.0, 20.0] {
            let light = light_at(&on, &off, 0, &camera, cells, [x, 1.0, 16.0]).unwrap();
            assert!((0.97..1.03).contains(&light), "{name}: x = {x}: {light}");
        }
        // A player whose eyes are 1.5 voxels over the floor at x = 22: the figure is two voxels high, so the sun at 53 degrees puts its shadow at about 20.5.
        let at = |x: f32| Avatar {
            position: WorldPosition::new(ORIGIN_BRICK, [x, 2.5, 16.5]),
            color: 3,
        };
        renderer.set_avatars(&[at(22.0)]);
        let (on, off, with_the_player) = on_and_off(&mut renderer, 0, SUN_OVER_THE_EAST, &camera);
        assert!(
            with_the_player.shadow_cascades_drawn >= 1,
            "{name}: the player is a caster of the fine maps"
        );
        assert!(
            with_the_player.shadow_casters_drawn > first.shadow_casters_drawn,
            "{name}"
        );
        let in_the_players_shadow =
            light_at(&on, &off, 0, &camera, cells, [20.8, 1.0, 16.5]).unwrap();
        assert!(
            in_the_players_shadow < 0.5,
            "{name}: {in_the_players_shadow}"
        );
        let beside_it = light_at(&on, &off, 0, &camera, cells, [20.8, 1.0, 20.0]).unwrap();
        assert!((0.97..1.03).contains(&beside_it), "{name}: {beside_it}");
        // The player walks: the shadow follows in the next picture.
        renderer.set_avatars(&[at(18.0)]);
        let (on, off, walked) = on_and_off(&mut renderer, 0, SUN_OVER_THE_EAST, &camera);
        assert!(
            walked.shadow_cascades_drawn >= 1,
            "{name}: the maps were not drawn again"
        );
        let old_place = light_at(&on, &off, 0, &camera, cells, [20.8, 1.0, 16.5]).unwrap();
        assert!(
            (0.97..1.03).contains(&old_place),
            "{name}: the shadow stayed: {old_place}"
        );
        let new_place = light_at(&on, &off, 0, &camera, cells, [16.8, 1.0, 16.5]).unwrap();
        assert!(new_place < 0.5, "{name}: {new_place}");
        // Taken away: no more shadow.
        renderer.set_avatars(&[]);
        let (on, off, gone) = on_and_off(&mut renderer, 0, SUN_OVER_THE_EAST, &camera);
        assert!(gone.shadow_cascades_drawn >= 1, "{name}");
        let light = light_at(&on, &off, 0, &camera, cells, [16.8, 1.0, 16.5]).unwrap();
        assert!((0.97..1.03).contains(&light), "{name}: {light}");
    }
}

/// A place given in voxels, as a brick and an offset in it.
fn place_in_voxels(voxels: [f64; 3]) -> WorldPosition {
    let brick = voxels.map(|part| (part / 32.0).floor());
    WorldPosition::new(
        BrickCoordinate::new(brick[0] as i128, brick[1] as i128, brick[2] as i128),
        std::array::from_fn(|axis| (voxels[axis] - brick[axis] * 32.0) as f32),
    )
}

/// A green cube at (16, 16, 16) with no light of its own, seen from the side it can be seen from with its -x face in the middle of the picture, and the brightness of that
/// pixel with the shadows on over what it is with them off, for this atmosphere (which has the sun's light and the sky's).
fn side_face_light(
    context: &GraphicsContext,
    sun: [f32; 3],
    sun_light: u8,
    sky_light: u8,
    ball: Option<SunOccluder>,
) -> f64 {
    let mut renderer = new_renderer(context);
    place_blocks(
        &mut renderer,
        ORIGIN_BRICK,
        &[([16, 16, 16], 2, 0)],
        Some(0),
    );
    if let Some(ball) = ball {
        renderer.set_sun_occluders(&[ball]);
    }
    let mut camera = camera_at(ORIGIN_BRICK, [10.0, 16.5, 16.5]);
    camera.yaw = -std::f32::consts::FRAC_PI_2;
    let atmosphere = |shadows| Atmosphere {
        sun_light,
        sky_light,
        ..shadow_atmosphere(0, sun, shadows)
    };
    renderer.set_atmosphere(atmosphere(false));
    let off = render(&mut renderer, &camera);
    renderer.set_atmosphere(atmosphere(true));
    let on = render(&mut renderer, &camera);
    let at = |image: &[u8]| linear_brightness(pixel(image, SIZE / 2, SIZE / 2));
    assert!(at(&off) > 0.0, "the face is black with the shadows off");
    at(&on) / at(&off)
}

#[test]
fn faces_turned_from_the_sun_have_the_shadowed_share_of_the_sky_and_gain_all_of_it_as_they_turn_to_it()
 {
    // The sky alone (the sun has no light), so the share is the whole of the change: a face looking along -x, and the sun at an angle from it whose cosine is `cosine`.
    let share = |cosine: f32| [-cosine, (1.0 - cosine * cosine).sqrt(), 0.0];
    let fill = f64::from(SHADOWED_ATMOSPHERE_FILL) / 255.0;
    for context in contexts() {
        let name = adapter_name(&context);
        // Looking straight away from the sun, and edge on: the shadowed share (the face is in the shade of its own body).
        for cosine in [-0.8f32, -0.1, 0.0] {
            let light = side_face_light(&context, share(cosine), 0, 255, None);
            assert!(
                (light - fill).abs() < 0.02,
                "{name}: cosine {cosine}: {light} against {fill}"
            );
        }
        // Half way to facing it (0.075 of the 0.15 it takes): half way between.
        let half = side_face_light(&context, share(0.075), 0, 255, None);
        let expected = fill + (1.0 - fill) * 0.5;
        assert!(
            (half - expected).abs() < 0.03,
            "{name}: {half} against {expected}"
        );
        // From 0.15 on: all of the sky, as with no shadows.
        for cosine in [0.15f32, 0.5, 1.0] {
            let light = side_face_light(&context, share(cosine), 0, 255, None);
            assert!(
                (light - 1.0).abs() < 0.02,
                "{name}: cosine {cosine}: {light}"
            );
        }
    }
}

#[test]
fn a_face_of_a_planets_ground_that_looks_away_from_the_sun_but_is_lit_by_the_turn_is_not_shaded_by_its_own_body()
 {
    // The cube's -x face is a face of the ground of a planet whose way out there is (-0.5, 0, 0.866) (a ball of radius 1,000 under the face, a cube-sphere's patch that is not
    // level): the sun is nearly along +z, a little toward +x (0.3), so the face looks away from it by its own normal (the cosine is -0.29), and by the turned one (which points
    // half way to the way out, up and to -x) it is lit by 0.23. The shadow map holds the back of the cube, which is this face, at the depth of the face: only a place moved
    // off the face and eased toward the sun is lit by it. With the shadows on the face is as bright as with them off.
    let face = [16.0, 16.5, 16.5];
    let way_out = [-0.5, 0.0, 0.866_025_403_784_438_6];
    let radius = 1000.0;
    let centre: [f64; 3] = std::array::from_fn(|axis| face[axis] - radius * way_out[axis]);
    let ball = SunOccluder::Ball {
        centre: place_in_voxels(centre),
        radius: radius as f32,
    };
    for context in contexts() {
        let name = adapter_name(&context);
        let light = side_face_light(&context, [0.3, 0.0, 1.0], 255, 0, Some(ball));
        // (Not exactly all of it: a face this slanted to the light (73 degrees) has a few texels of the filter deeper than the bias, a few in a hundred of the light. A face
        // in its own shadow has none.)
        assert!(
            (0.9..1.03).contains(&light),
            "{name}: the face of the ground is {light} of what it is with no shadows"
        );
    }
}

#[test]
fn the_shadow_of_the_last_cascade_fades_out_over_the_last_of_its_reach_as_the_rule_says() {
    // With no fog there are eleven cascades, and the last reaches 58.7 million voxels. A scene of cells of 2^21 voxels (two million) seen from 26 cells above its floor has a
    // place in a shadow 55 million voxels away: in the part of the last cascade's reach where it gives way to no shadow at all (from 85% of the reach). The share of the shadow
    // left is the rule's (`cascade_for`), and the light is the sun's and the sky's by it, as the shader combines them.
    let level = 21;
    let reach = renderer::reach_of(67_108_864.0);
    let (probe, camera_cells) = ([12.5, 1.0, 16.0], [16.0, 27.0, 16.0]);
    let squares: f64 = (0..3)
        .map(|axis| {
            let delta = (probe[axis] - camera_cells[axis]) * scale_of(level);
            delta * delta
        })
        .sum();
    let distance = squares.sqrt() as f32;
    let (index, blend) = renderer::cascade_for(&[reach], distance);
    assert_eq!(index, 0);
    assert!((0.2..0.8).contains(&blend), "{distance}: {blend}");
    // The light of a floor that faces the sun at 53 degrees, by how much of the sun reaches it: the sun's (0.8 of a full 1), the sky's (110 of 255, and of that the shadowed
    // share of 64 of 255 when it is not reached), shaded by the side (0.4 + 0.6 * 0.8) and combined like a screen blend.
    let light = |reached: f64| {
        let sky = 110.0 / 255.0 * (64.0 / 255.0 + (1.0 - 64.0 / 255.0) * reached);
        let shaded = (0.4 + 0.6 * 0.8) * sky;
        let sun = 0.8 * reached;
        shaded + sun * (1.0 - shaded)
    };
    let expected = light(f64::from(blend)) / light(1.0);
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        upload_shadow_scene(&mut renderer, ShadowScene::Block, level);
        let camera = camera_in_cells(level, [16, 27, 16], 0.0, -Camera::MAXIMUM_PITCH);
        let atmosphere = |shadows| Atmosphere {
            fog_start: 1.0e15,
            fog_end: 2.0e15,
            ..shadow_atmosphere(level, SUN_OVER_THE_EAST, shadows)
        };
        renderer.set_atmosphere(atmosphere(false));
        let off = render(&mut renderer, &camera);
        renderer.set_atmosphere(atmosphere(true));
        let on = render(&mut renderer, &camera);
        let seen = light_at(&on, &off, level, &camera, camera_cells, probe)
            .expect("the place is in the picture");
        assert!(
            (seen - expected).abs() < 0.03,
            "{name}: the light there is {seen} of full, the rule says {expected} (a share of {blend} of the sun)"
        );
        // And it is not the shadow whole (0.11) nor none of it (1.0): the test is of the fade.
        assert!(seen > 0.2 && seen < 0.9, "{name}: {seen}");
    }
}
