//! Drawing only the faces of a brick that can look at the camera (`docs/plan.md`, "Draw only the faces of a brick that can look at the camera").
//!
//! The rasteriser throws away a quad that looks away from the camera, after the vertex shader has run on all four of its corners; a brick's quads are in the order of the faces they
//! are on, so the renderer sends only the faces the camera is on the right side of. A picture must not change by a pixel. These tests draw the same scene both ways, from cameras
//! inside a brick, beside it, far off and exactly on the planes of its edges, and compare the pictures:
//!
//! * bricks of the terrain, with slabs among the cubes (the quads of the second pipeline);
//! * bricks of ships turned every way, which are placed by a turn and a corner and not by the corner alone;
//! * coarse nodes, whose cells are two, four and eight voxels wide.
//!
//! Each also checks that the faces were left out in some of the pictures: a comparison of two pictures that both draw everything proves nothing.

use super::contact_trace::Random;
use super::*;

/// A camera at this place in voxels (anywhere: the brick and the offset in it are worked out), turned toward `target` and then by `wobble` (yaw, pitch) more.
fn camera_looking(from: [f64; 3], target: [f64; 3], wobble: [f64; 2]) -> Camera {
    let brick = BrickCoordinate::new(
        (from[0] / 32.0).floor() as i128,
        (from[1] / 32.0).floor() as i128,
        (from[2] / 32.0).floor() as i128,
    );
    let mut camera = camera_at(brick, from.map(|component| component.rem_euclid(32.0)));
    let (dx, dy, dz) = (
        target[0] - from[0],
        target[1] - from[1],
        target[2] - from[2],
    );
    // (Yaw 0 looks along -z and turns toward -x; pitch looks up.)
    camera.yaw = libm::atan2(-dx, -dz) + wobble[0];
    camera.pitch = libm::atan2(dy, libm::hypot(dx, dz)) + wobble[1];
    camera
}

/// A place in a cube of voxels `around` (a middle), `reach` voxels to each side of it.
fn random_place(random: &mut Random, around: [f64; 3], reach: f64) -> [f64; 3] {
    around.map(|middle| middle + (random.unit() * 2.0 - 1.0) * reach)
}

/// Cameras that look at `target` from random places up to `reach` voxels from it, with the aim off by up to half a radian each way.
fn random_cameras(random: &mut Random, target: [f64; 3], reach: f64, count: usize) -> Vec<Camera> {
    (0..count)
        .map(|_| {
            let from = random_place(random, target, reach);
            let wobble = [random.unit() - 0.5, random.unit() - 0.5];
            camera_looking(from, target, wobble)
        })
        .collect()
}

/// The pictures of one scene from one camera: with all six faces of each brick drawn, with only those that can look at the camera, and how many quads the second left out (the
/// first leaves out none, which is checked).
fn whole_and_culled(renderer: &mut VoxelRenderer, camera: &Camera) -> (Vec<u8>, Vec<u8>, usize) {
    renderer.set_skip_faces_looking_away(false);
    let whole = settled(renderer, camera);
    let (_, statistics) = renderer.render_to_rgba(camera, SIZE, SIZE);
    assert_eq!(statistics.quads_looking_away_skipped, 0);
    renderer.set_skip_faces_looking_away(true);
    let culled = settled(renderer, camera);
    let (_, statistics) = renderer.render_to_rgba(camera, SIZE, SIZE);
    (whole, culled, statistics.quads_looking_away_skipped)
}

/// Checks that every camera's two pictures are the same to the pixel, that the scene is in view in them, and that faces were left out.
fn same_pictures_from_every_camera(
    name: &str,
    scene: &str,
    renderer: &mut VoxelRenderer,
    cameras: &[Camera],
) {
    let (mut skipped, mut drawn) = (0, 0);
    for (index, camera) in cameras.iter().enumerate() {
        let (whole, culled, left_out) = whole_and_culled(renderer, camera);
        let differing = whole
            .chunks(4)
            .zip(culled.chunks(4))
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(
            differing, 0,
            "{name}, {scene}: camera {index} at {:?} differs in {differing} pixels",
            camera.position,
        );
        skipped += left_out;
        drawn += whole
            .chunks(4)
            .filter(|chunk| !is_sky((*chunk).try_into().unwrap()))
            .count();
    }
    assert!(
        drawn > cameras.len() * 150,
        "{name}, {scene}: the scenes are almost empty ({drawn} pixels in {} pictures)",
        cameras.len()
    );
    assert!(
        skipped > 0,
        "{name}, {scene}: no face was left out of any picture, so nothing was compared"
    );
}

/// Blocks of red (whole, a bottom slab, a top slab) and green at random in a brick, about `density` of its voxels.
fn random_blocks(random: &mut Random, density: f64) -> Vec<([u8; 3], u32, u32)> {
    let mut blocks = Vec::new();
    for z in 0..32u8 {
        for y in 0..32u8 {
            for x in 0..32u8 {
                if random.unit() < density {
                    let block = 1 + random.below(2) as u32;
                    let state = if block == 1 {
                        random.below(3) as u32
                    } else {
                        0
                    };
                    blocks.push(([x, y, z], block, state));
                }
            }
        }
    }
    blocks
}

/// The sun at an angle that gives every direction a face looks in a different light, so that a face missing from a picture shows.
fn lit_from_a_corner() -> Atmosphere {
    Atmosphere {
        sky_color: SKY,
        fog_start: 1.0e7,
        fog_end: 2.0e7,
        sun_direction: [0.4, 0.8, 0.45],
        sun_light: 255,
        sky_light: 120,
        ..Atmosphere::default()
    }
}

#[test]
fn the_pictures_of_terrain_are_the_same_to_the_pixel_whether_or_not_the_faces_that_look_away_are_drawn()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut random = Random(1);
        let mut renderer = new_renderer(&context);
        renderer.set_atmosphere(lit_from_a_corner());
        // Eight bricks in a block, of cubes and slabs.
        for z in 0..2 {
            for y in 0..2 {
                for x in 0..2 {
                    let blocks = random_blocks(&mut random, 0.03);
                    place_blocks(&mut renderer, BrickCoordinate::new(x, y, z), &blocks, None);
                }
            }
        }
        let middle = [32.0, 32.0, 32.0];
        // Far off, beside the block, and over and under it.
        let mut cameras = random_cameras(&mut random, middle, 120.0, 40);
        // Inside the bricks: the camera is in the middle of the brick it is in, and all six faces can look at it.
        for inside in [[16.5, 16.5, 16.5], [48.5, 16.5, 48.5], [16.5, 48.5, 16.5]] {
            cameras.push(camera_looking(inside, middle, [0.3, 0.1]));
            cameras.push(camera_looking(
                inside,
                [inside[0], inside[1], inside[2] - 50.0],
                [0.0, 0.0],
            ));
        }
        // On the planes of the edges of a brick, where a face is edge on: 0, 32 and 64 along each axis in turn.
        for axis in 0..3 {
            for plane in [0.0, 32.0, 64.0] {
                let mut from = [20.5, 20.5, 20.5];
                from[axis] = plane;
                cameras.push(camera_looking(from, middle, [0.2, 0.2]));
                cameras.push(camera_looking(from, [60.5, 5.5, 33.5], [0.0, 0.0]));
            }
        }
        same_pictures_from_every_camera(&name, "terrain", &mut renderer, &cameras);
    }
}

#[test]
fn the_pictures_of_ships_turned_every_way_are_the_same_to_the_pixel_whether_or_not_the_faces_that_look_away_are_drawn()
 {
    // (A unit quaternion about no axis of the grid, the fourth component from the other three.)
    let w = (1.0f64 - (0.18f64 * 0.18 + 0.36 * 0.36 + 0.54 * 0.54)).sqrt();
    let turns: [(&str, Orientation); 5] = [
        ("not turned", Orientation::IDENTITY),
        // The ship's x onto y, y onto z, z onto x: every entry of the matrix is used, in the place it is in.
        (
            "tumbled about a diagonal",
            Orientation::from_components([0.5, 0.5, 0.5, 0.5]).unwrap(),
        ),
        ("half way round about x", half_turn_about_x()),
        ("yawed and tipped", Orientation::from_yaw_pitch(0.7, 0.4)),
        (
            "turned about no axis of the grid",
            Orientation::from_components([0.18, -0.36, 0.54, w]).unwrap(),
        ),
    ];
    for context in contexts() {
        let name = adapter_name(&context);
        let mut random = Random(2);
        for (label, orientation) in turns {
            let mut renderer = new_renderer(&context);
            renderer.set_atmosphere(lit_from_a_corner());
            // The ship's origin is the world's, and two of its bricks side by side.
            let placed = pose([0, 0, 0], [0.0; 3], orientation);
            renderer.set_grid_pose(a_ship(), placed);
            for x in 0..2 {
                let blocks = random_blocks(&mut random, 0.03);
                renderer.upload_grid_brick(
                    a_ship(),
                    BrickCoordinate::new(x, 0, 0),
                    &mesh_of_blocks(&shaped_table(), &blocks, None),
                );
            }
            // The middle of the two bricks (32 by 16 by 16 voxels from the origin of the ship), turned and placed.
            let turn = placed.rotation_matrix();
            let local = [32.0, 16.0, 16.0];
            let middle: [f64; 3] = std::array::from_fn(|row| {
                (0..3).map(|column| turn[row][column] * local[column]).sum()
            });
            let mut cameras = random_cameras(&mut random, middle, 110.0, 30);
            // Inside the first brick, and on the planes of the first brick's edges in the ship's own axes.
            for ship_place in [[16.5, 16.5, 16.5], [0.0, 16.5, 16.5], [32.0, 16.5, 16.5]] {
                let from: [f64; 3] = std::array::from_fn(|row| {
                    (0..3)
                        .map(|column| turn[row][column] * ship_place[column])
                        .sum()
                });
                cameras.push(camera_looking(from, middle, [0.25, -0.1]));
            }
            same_pictures_from_every_camera(&name, label, &mut renderer, &cameras);
        }
    }
}

#[test]
fn the_pictures_of_coarse_nodes_are_the_same_to_the_pixel_whether_or_not_the_faces_that_look_away_are_drawn()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut random = Random(3);
        for level in 1u8..=3 {
            let width = 32.0 * f64::from(1u32 << level);
            let cells: Vec<Option<u32>> = (0..32 * 32 * 32)
                .map(|_| (random.unit() < 0.04).then(|| 1 + random.below(2) as u32))
                .collect();
            let mut renderer = new_renderer(&context);
            renderer.set_atmosphere(lit_from_a_corner());
            renderer.upload_coarse_node(
                cube_of_bricks([0, 0, 0], level),
                &coarse_mesh(|x, y, z| cells[(z * 32 + y) * 32 + x]),
            );
            let middle = [width / 2.0; 3];
            let mut cameras = random_cameras(&mut random, middle, 1.5 * width, 24);
            // Inside the node, and on the planes of its edges.
            cameras.push(camera_looking([width / 4.0; 3], middle, [0.2, 0.1]));
            for axis in 0..3 {
                for plane in [0.0, width] {
                    let mut from = [width / 3.0; 3];
                    from[axis] = plane;
                    cameras.push(camera_looking(from, middle, [0.2, 0.2]));
                }
            }
            same_pictures_from_every_camera(
                &name,
                &format!("level {level}"),
                &mut renderer,
                &cameras,
            );
        }
    }
}

#[test]
fn the_faces_left_out_are_counted_exactly_and_none_are_while_the_camera_is_inside_or_the_switch_is_off()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        // A box of 20 voxels each way in the middle of a brick: the same number of quads on each of its six faces (one each if the mesher joins the voxels of a face, as it does).
        let mut renderer = new_renderer(&context);
        let mut voxels = Vec::new();
        for z in 6..26 {
            for y in 6..26 {
                for x in 6..26 {
                    voxels.push(([x, y, z], RED));
                }
            }
        }
        place_voxels(&mut renderer, ORIGIN_BRICK, &voxels);
        let target = [16.0, 16.0, 16.0];
        let statistics_from = |renderer: &mut VoxelRenderer, from: [f64; 3]| {
            renderer
                .render_to_rgba(&camera_looking(from, target, [0.0, 0.0]), SIZE, SIZE)
                .1
        };
        // Far along +z, looking at it: the brick's -z face looks away from every place in the camera's side of the brick, the other five can look at it.
        let behind = statistics_from(&mut renderer, [16.0, 16.0, 100.0]);
        let per_face = behind.quads_drawn / 6;
        assert!(
            per_face >= 1 && behind.quads_drawn == per_face * 6,
            "{name}"
        );
        assert_eq!(behind.quads_looking_away_skipped, per_face, "{name}");
        assert_eq!(behind.bricks_drawn, 1, "{name}");
        // Over a corner (+x, +y, +z): the three faces on the far sides are left out.
        let corner = statistics_from(&mut renderer, [100.0, 100.0, 100.0]);
        assert_eq!(corner.quads_looking_away_skipped, 3 * per_face, "{name}");
        // Under the opposite corner too.
        let under = statistics_from(&mut renderer, [-60.0, -60.0, -60.0]);
        assert_eq!(under.quads_looking_away_skipped, 3 * per_face, "{name}");
        // Inside the brick (inside the box, even) every face can look at the camera.
        let inside = statistics_from(&mut renderer, [10.5, 10.5, 10.5]);
        assert_eq!(inside.quads_looking_away_skipped, 0, "{name}");
        // With the switch off nothing is left out, and the box is the same.
        let seen = camera_looking([100.0, 100.0, 100.0], target, [0.0, 0.0]);
        let with_the_cull = settled(&mut renderer, &seen);
        assert!(
            !is_sky(pixel(&with_the_cull, SIZE / 2, SIZE / 2)),
            "{name}: the box is in the middle of the picture"
        );
        renderer.set_skip_faces_looking_away(false);
        let without = renderer.render_to_rgba(&seen, SIZE, SIZE).1;
        assert_eq!(without.quads_looking_away_skipped, 0, "{name}");
        assert_eq!(settled(&mut renderer, &seen), with_the_cull, "{name}");
    }
}
