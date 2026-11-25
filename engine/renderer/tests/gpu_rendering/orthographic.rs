//! The orthographic camera (`Projection::Orthographic`, `docs/specs/viewpoints-spec.md` 6.6 B): a thing is as large far away as near, the far plane cuts off what is farther than
//! its depth, and every face is drawn that looks along the way the camera looks, wherever the camera is.

use super::*;

use renderer::Projection;

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
