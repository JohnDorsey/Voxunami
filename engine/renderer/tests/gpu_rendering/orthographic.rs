//! The orthographic camera (`Projection::Orthographic`, `docs/specs/viewpoints-spec.md` 6.6 B): a thing is as large far away as near, the far plane cuts off what is farther than
//! its depth, and every face is drawn that looks along the way the camera looks, wherever the camera is.

use super::*;

use renderer::{Projection, SunOccluder};

/// A camera `height` voxels over the middle (16, 16) of the brick at the origin, looking straight down: a picture of 64 pixels across `box_height` voxels, so that a pixel is
/// `box_height / 64` voxels (the top of the picture is toward -z).
fn looking_down(height: f64, box_height: f32, depth: f32) -> Camera {
    let mut camera = camera_at(ORIGIN_BRICK, [16.0, height, 16.0]);
    camera.pitch = -std::f64::consts::FRAC_PI_2;
    camera.projection = Projection::Orthographic {
        height: box_height,
        depth,
    };
    camera
}

/// The floor at y = 0 of the brick at the origin: red for x < 16 and green from 16 on.
fn red_and_green_floor(renderer: &mut VoxelRenderer) {
    let voxels: Vec<([u8; 3], u32)> = (0..32u8)
        .flat_map(|x| (0..32u8).map(move |z| ([x, 0, z], if x < 16 { RED } else { GREEN })))
        .collect();
    place_voxels(renderer, ORIGIN_BRICK, &voxels);
}

#[test]
fn a_floor_is_drawn_the_same_from_near_and_from_far_in_an_orthographic_view() {
    for context in contexts() {
        let mut renderer = new_renderer(&context);
        red_and_green_floor(&mut renderer);
        // A voxel a pixel across: the floor (32 voxels) covers the 32 pixels from 16 to 47 and the line between red and green is at 32.
        let near = render(&mut renderer, &looking_down(100.0, 64.0, 1_000.0));
        let far = render(&mut renderer, &looking_down(700.0, 64.0, 1_000.0));
        save_image("orthographic_floor", &adapter_name(&context), &far);
        assert_eq!(
            near,
            far,
            "{}: the picture does not depend on the distance",
            adapter_name(&context)
        );
        assert!(is_red(pixel(&far, 24, 32)), "{:?}", pixel(&far, 24, 32));
        assert!(is_green(pixel(&far, 40, 32)), "{:?}", pixel(&far, 40, 32));
        assert!(is_sky(pixel(&far, 8, 32)) && is_sky(pixel(&far, 56, 32)));
        assert!(is_sky(pixel(&far, 32, 8)) && is_sky(pixel(&far, 32, 56)));
        // The line is where it should be, to the pixel.
        assert!(is_red(pixel(&far, 31, 32)) && is_green(pixel(&far, 32, 32)));
        assert!(is_red(pixel(&far, 16, 32)) && is_green(pixel(&far, 47, 32)));
        assert!(is_sky(pixel(&far, 15, 32)) && is_sky(pixel(&far, 48, 32)));
    }
}

#[test]
fn what_is_farther_than_the_depth_of_an_orthographic_view_is_not_drawn() {
    for context in contexts() {
        let mut renderer = new_renderer(&context);
        red_and_green_floor(&mut renderer);
        // The floor is a little over 100 voxels from the camera: it is drawn with a depth of 200 and not with one of 50.
        let reaches = render(&mut renderer, &looking_down(100.0, 64.0, 200.0));
        assert!(is_red(pixel(&reaches, 24, 32)));
        let (short, statistics) =
            renderer.render_to_rgba(&looking_down(100.0, 64.0, 50.0), SIZE, SIZE);
        assert!(
            short
                .chunks(4)
                .all(|chunk| is_sky(chunk.try_into().unwrap())),
            "{}: all sky",
            adapter_name(&context)
        );
        assert_eq!(statistics.bricks_drawn, 0);
    }
}

#[test]
fn a_perspective_view_of_the_floor_still_gets_smaller_with_the_distance() {
    // (The other half of the first test: it is the projection that makes the pictures equal, and a perspective camera at the same two places does not.)
    for context in contexts() {
        let mut renderer = new_renderer(&context);
        red_and_green_floor(&mut renderer);
        let mut near = looking_down(100.0, 64.0, 1_000.0);
        let mut far = looking_down(300.0, 64.0, 1_000.0);
        near.projection = Projection::Perspective;
        far.projection = Projection::Perspective;
        let (near, far) = (render(&mut renderer, &near), render(&mut renderer, &far));
        assert_ne!(near, far, "{}", adapter_name(&context));
    }
}

/// A block of 32 voxels on every side, red.
fn solid_block(renderer: &mut VoxelRenderer, brick: BrickCoordinate) {
    let voxels: Vec<([u8; 3], u32)> = (0..32u8)
        .flat_map(|x| (0..32u8).flat_map(move |y| (0..32u8).map(move |z| ([x, y, z], RED))))
        .collect();
    place_voxels(renderer, brick, &voxels);
}

#[test]
fn every_face_that_looks_the_way_an_orthographic_camera_does_is_drawn_whichever_side_of_it_the_camera_is_on()
 {
    for context in contexts() {
        let mut renderer = new_renderer(&context);
        // The camera looks toward -x, down and toward -z (a yaw of 45 degrees and a pitch of 45 below), 300 voxels from the middle of the picture. The block is far to its right,
        // so far that the camera is at a lower x than the block's +x face although it is seen from the +x side: a face that is kept by where the camera is would be lost here.
        let mut camera = camera_at(BrickCoordinate::new(0, 0, 0), [0.0; 3]);
        camera.yaw = std::f64::consts::FRAC_PI_4;
        camera.pitch = -std::f64::consts::FRAC_PI_4;
        camera.projection = Projection::Orthographic {
            height: 400.0,
            depth: 2_000.0,
        };
        let forward = camera.forward();
        camera.move_in_world(glam::DVec3::new(
            -f64::from(forward.x) * 300.0,
            -f64::from(forward.y) * 300.0,
            -f64::from(forward.z) * 300.0,
        ));
        let cameras_x = camera.position.brick().x() as f64 * 32.0 + camera.position.offset()[0];
        // To the right of the middle of the picture by about 110 voxels and a little below it, with its +x face past the camera.
        let block = BrickCoordinate::new(5, -1, 0);
        assert!(
            f64::from(block.x() as i32 * 32 + 32) > cameras_x,
            "the +x face of the block is past the camera: {cameras_x}"
        );
        solid_block(&mut renderer, block);
        let with = render(&mut renderer, &camera);
        renderer.set_skip_faces_looking_away(false);
        let without = render(&mut renderer, &camera);
        save_image("orthographic_block", &adapter_name(&context), &with);
        assert_eq!(with, without, "{}", adapter_name(&context));
        // The block is there, and shows more than one shade (its top and its sides).
        let shades: std::collections::BTreeSet<u32> = with
            .chunks(4)
            .map(|chunk| <[u8; 4]>::try_from(chunk).unwrap())
            .filter(|&pixel| is_red(pixel))
            .map(brightness)
            .collect();
        assert!(shades.len() >= 2, "{}: {shades:?}", adapter_name(&context));
    }
}

/// A red floor at y = 0 of the brick at the origin with a green wall along z at x = 16, eight voxels high over the floor.
fn floor_with_a_wall(renderer: &mut VoxelRenderer) {
    // (With no lamp light on them, so that the sun is what lights them and a shadow shows.)
    let mut voxels: Vec<_> = (0..32u8)
        .flat_map(|x| (0..32u8).map(move |z| ([x, 0, z], RED, 0)))
        .collect();
    voxels.extend((1..=8u8).flat_map(|y| (0..32u8).map(move |z| ([16, y, z], GREEN, 0))));
    place_blocks(renderer, ORIGIN_BRICK, &voxels, Some(0));
}

/// A sun 45 degrees up toward +x, with shadows and no fog: the wall's shadow falls on the floor from x = 16 down to x = 8.
fn sun_from_the_side(renderer: &mut VoxelRenderer) {
    renderer.set_atmosphere(Atmosphere {
        sky_color: SKY,
        fog_start: 1.0e5,
        fog_end: 2.0e5,
        sun_direction: [1.0, 1.0, 0.0],
        sun_light: 255,
        ambient_light: 40,
        shadows: true,
        ..Atmosphere::default()
    });
}

/// The middle row of the picture of the floor and wall, from straight above, a voxel a pixel (the floor's x = 0 is at pixel 16): whether each part is in shadow where the sun
/// of [`sun_from_the_side`] puts it, `shift` pixels to the left of where it is when the camera is over the middle of the floor.
fn assert_the_walls_shadow(image: &[u8], shift: u32, name: &str) {
    let lit = brightness(pixel(image, 20 - shift, 32));
    let row: Vec<u32> = (16 - shift..48 - shift)
        .map(|x| brightness(pixel(image, x, 32)))
        .collect();
    // The shadow: from x = 8 to the wall (pixels 24 to 31), and not before nor past it.
    for x in 25..31u32 {
        let found = brightness(pixel(image, x - shift, 32));
        assert!(
            found * 10 < lit * 7,
            "{name}: pixel {x} is lit: {found} against {lit}, {row:?}"
        );
    }
    for x in (17..23u32).chain(34..46) {
        let found = brightness(pixel(image, x - shift, 32));
        assert!(
            found * 10 > lit * 9,
            "{name}: pixel {x} is dark: {found} against {lit}, {row:?}"
        );
    }
}

#[test]
fn an_orthographic_view_from_far_back_has_shadows_as_sharp_as_its_pixels() {
    for context in contexts() {
        let adapter = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        floor_with_a_wall(&mut renderer);
        sun_from_the_side(&mut renderer);
        for height in [100.0, 3_000.0, 40_000.0] {
            let image = render(&mut renderer, &looking_down(height, 64.0, 50_000.0));
            save_image(&format!("orthographic_shadow_{height}"), &adapter, &image);
            assert_the_walls_shadow(&image, 0, &format!("{adapter} from {height}"));
        }
    }
}

#[test]
fn the_shadow_of_an_orthographic_view_follows_it_when_it_moves_and_zooms() {
    for context in contexts() {
        let adapter = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        floor_with_a_wall(&mut renderer);
        sun_from_the_side(&mut renderer);
        let camera = looking_down(3_000.0, 64.0, 4_000.0);
        assert_the_walls_shadow(&render(&mut renderer, &camera), 0, &adapter);
        // Across by ten voxels: the shadow is ten pixels the other way, in the same maps or in maps drawn again.
        for step in [1.0, 4.0, 10.0] {
            let mut moved = camera;
            moved.move_in_world(glam::DVec3::new(step, 0.0, 0.0));
            assert_the_walls_shadow(
                &render(&mut renderer, &moved),
                step as u32,
                &format!("{adapter} moved {step}"),
            );
        }
        // Zoomed far out and back: the picture is the same as it was.
        let first = render(&mut renderer, &camera);
        render(&mut renderer, &looking_down(3_000.0, 2_000.0, 4_000.0));
        let again = render(&mut renderer, &camera);
        assert_eq!(first, again, "{adapter}");
    }
}

#[test]
fn a_perspective_view_has_the_shadows_it_had_after_an_orthographic_one() {
    for context in contexts() {
        let adapter = adapter_name(&context);
        let mut perspective = camera_at(ORIGIN_BRICK, [16.0, 60.0, 16.0]);
        perspective.pitch = -std::f64::consts::FRAC_PI_2;
        let scene = |context: &GraphicsContext| {
            let mut renderer = new_renderer(context);
            floor_with_a_wall(&mut renderer);
            sun_from_the_side(&mut renderer);
            renderer
        };
        let fresh = render(&mut scene(&context), &perspective);
        let mut renderer = scene(&context);
        render(&mut renderer, &looking_down(3_000.0, 64.0, 4_000.0));
        assert_eq!(render(&mut renderer, &perspective), fresh, "{adapter}");
    }
}

/// The sky's maps are squares round the camera, looked in by the distance from it, and say nothing of a view with no perspective, whose camera is far back from all it sees: such
/// a view has none, and the sky is open over everything in it, from any distance.
#[test]
fn an_orthographic_view_has_no_maps_of_the_sky_and_does_not_change_with_the_distance_when_cover_is_counted()
 {
    for context in contexts() {
        let adapter = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        floor_with_a_wall(&mut renderer);
        renderer.set_sun_occluders(&[SunOccluder::FlatGround]);
        renderer.set_atmosphere(Atmosphere {
            sky_color: SKY,
            fog_start: 1.0e5,
            fog_end: 2.0e5,
            sun_direction: [1.0, 1.0, 0.0],
            sun_light: 255,
            sky_light: 110,
            sky_light_under_cover: 0,
            shadows: true,
            ..Atmosphere::default()
        });
        let (near, statistics) =
            renderer.render_to_rgba(&looking_down(100.0, 64.0, 4_000.0), SIZE, SIZE);
        assert_eq!(statistics.sky_cascades_drawn, 0, "{adapter}");
        assert_eq!(
            statistics.shadow_cascades_drawn, 1,
            "{adapter}: one map of the sun"
        );
        let far = render(&mut renderer, &looking_down(3_000.0, 64.0, 4_000.0));
        assert_eq!(near, far, "{adapter}");
        // (A perspective camera at the nearer place does count the cover.)
        let mut perspective = looking_down(100.0, 64.0, 4_000.0);
        perspective.projection = Projection::Perspective;
        let (_, statistics) = renderer.render_to_rgba(&perspective, SIZE, SIZE);
        assert!(statistics.sky_cascades_drawn > 0, "{adapter}");
    }
}

#[test]
fn what_is_in_the_corner_of_a_wide_orthographic_view_has_its_shadow_too() {
    for context in contexts() {
        let adapter = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        // A view 512 voxels across, eight to a pixel, centred over the brick at the origin; six bricks right and six down the picture, a floor with a short wall: along x at 24,
        // 16 voxels high over the floor, and along z from 8 to 24 only, so that its shadow (from x = 8 to the wall) ends along both ways across the picture.
        let brick = BrickCoordinate::new(6, 0, 6);
        let mut voxels: Vec<_> = (0..32u8)
            .flat_map(|x| (0..32u8).map(move |z| ([x, 0, z], RED, 0)))
            .collect();
        voxels.extend((1..=16u8).flat_map(|y| (8..24u8).map(move |z| ([24, y, z], GREEN, 0))));
        place_blocks(&mut renderer, brick, &voxels, Some(0));
        sun_from_the_side(&mut renderer);
        let image = render(&mut renderer, &looking_down(3_000.0, 512.0, 4_000.0));
        save_image("orthographic_shadow_corner", &adapter, &image);
        // The floor is the pixels 54 to 57 both ways, at 4, 12, 20 and 28 voxels along it.
        let lit = brightness(pixel(&image, 54, 54));
        for (x, y) in [(55, 55), (56, 55), (55, 56), (56, 56)] {
            let found = brightness(pixel(&image, x, y));
            assert!(
                found * 10 < lit * 7,
                "{adapter} ({x}, {y}) is lit: {found} against {lit}"
            );
        }
        for (x, y) in [(54, 55), (54, 56), (55, 54), (56, 54), (55, 57), (56, 57)] {
            let found = brightness(pixel(&image, x, y));
            assert!(
                found * 10 > lit * 9,
                "{adapter} ({x}, {y}) is dark: {found} against {lit}"
            );
        }
    }
}

#[test]
fn an_orthographic_view_that_moves_along_the_way_it_looks_keeps_its_map() {
    for context in contexts() {
        let adapter = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        floor_with_a_wall(&mut renderer);
        sun_from_the_side(&mut renderer);
        let (first, statistics) =
            renderer.render_to_rgba(&looking_down(3_000.0, 64.0, 4_000.0), SIZE, SIZE);
        assert_eq!(statistics.shadow_cascades_drawn, 1, "{adapter}");
        // Higher, the picture sees the same box of the world: the map is looked in as it is.
        for height in [3_100.0, 3_500.0] {
            let (image, statistics) =
                renderer.render_to_rgba(&looking_down(height, 64.0, 4_000.0), SIZE, SIZE);
            assert_eq!(statistics.shadow_cascades_drawn, 0, "{adapter} {height}");
            assert_the_walls_shadow(&image, 0, &format!("{adapter} {height}"));
            assert_eq!(image, first, "{adapter} {height}");
        }
    }
}
