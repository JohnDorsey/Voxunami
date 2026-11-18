//! The shade of the corners of a face (ambient occlusion), worked out by the fragment shader from the voxels that block the sun (`volume_shade_level` in `contact_on.wgsl`).
//!
//! The rule is written here a second time, in plain code (`corner_levels`), and the shader is held to it by pictures: a place on a face is as dark as the rule's four corner levels
//! blended across the voxel say. The rule's own tests are at the top (what each combination of cells gives a corner; that faces on one plane agree about the corner they share).
//! The scenes are lit by the ambient light alone (the sun is under the ground, so the faces turn from it), so a face's brightness is its open brightness times one minus 0.2 a
//! level (`SHADE_PER_LEVEL` in `voxel.wgsl`).

use std::collections::HashSet;

use voxel_meshing::quad_axes;

use super::contact_trace::{occupancy_where, settled_at};
use super::*;

/// The share of its light a face loses for each level of shade (`SHADE_PER_LEVEL` in `voxel.wgsl`).
const SHADE_PER_LEVEL: f64 = 0.2;

// ---- the rule ----

/// The level (0 to 3) of each corner of the face of the voxel at `voxel` that looks `face`, in the quad's corner order (`u` fastest, in `voxel_meshing::quad_axes`): the number of
/// cubes that block among the two cells beside the cell in front of the face and the one diagonal to it at that corner, or 3 when both beside it do (nothing gets round a corner that
/// is shut on both sides). `blocks` says whether the cube at a cell does.
fn corner_levels(
    blocks: impl Fn([i64; 3]) -> bool,
    voxel: [i64; 3],
    face: FaceDirection,
) -> [u8; 4] {
    let (u_axis, v_axis) = quad_axes(face);
    let mut front = voxel;
    front[face.axis()] += if face.is_positive() { 1 } else { -1 };
    std::array::from_fn(|corner| {
        let toward_u = if corner & 1 == 1 { 1 } else { -1 };
        let toward_v = if corner & 2 == 2 { 1 } else { -1 };
        let mut beside_in_u = front;
        beside_in_u[u_axis] += toward_u;
        let mut beside_in_v = front;
        beside_in_v[v_axis] += toward_v;
        let mut diagonal = beside_in_u;
        diagonal[v_axis] += toward_v;
        let (blocked_in_u, blocked_in_v) = (blocks(beside_in_u), blocks(beside_in_v));
        if blocked_in_u && blocked_in_v {
            3
        } else {
            u8::from(blocked_in_u) + u8::from(blocked_in_v) + u8::from(blocks(diagonal))
        }
    })
}

/// The level at a place on a face: the four corner levels blended across the voxel (`across_u` and `across_v` are 0 to 1 along the quad's two axes).
fn blended(levels: [u8; 4], across_u: f64, across_v: f64) -> f64 {
    let level = |corner: usize| f64::from(levels[corner]);
    let low = level(0) + (level(1) - level(0)) * across_u;
    let high = level(2) + (level(3) - level(2)) * across_u;
    low + (high - low) * across_v
}

fn step(cell: [i64; 3], face: FaceDirection) -> [i64; 3] {
    let mut next = cell;
    next[face.axis()] += if face.is_positive() { 1 } else { -1 };
    next
}

/// A world of cubes that block, given by the set of cells that do.
struct Cells(HashSet<[i64; 3]>);

impl Cells {
    fn blocks(&self, cell: [i64; 3]) -> bool {
        self.0.contains(&cell)
    }

    fn levels(&self, voxel: [i64; 3], face: FaceDirection) -> [u8; 4] {
        corner_levels(|cell| self.blocks(cell), voxel, face)
    }

    /// Where corner `corner` of the face of `voxel` is in the world, in whole voxels from the origin.
    fn vertex(voxel: [i64; 3], face: FaceDirection, corner: usize) -> [i64; 3] {
        let (u_axis, v_axis) = quad_axes(face);
        let mut place = voxel;
        place[face.axis()] += i64::from(face.is_positive());
        place[u_axis] += (corner & 1) as i64;
        place[v_axis] += (corner >> 1) as i64;
        place
    }
}

/// A floor of stone up to and including y = 0 (as far as the tests look), and the given cells above it.
fn floor_with(above: &[[i64; 3]]) -> Cells {
    let mut cells = HashSet::new();
    for x in -6..=6 {
        for z in -6..=6 {
            for y in -2..=0 {
                cells.insert([x, y, z]);
            }
        }
    }
    cells.extend(above.iter().copied());
    Cells(cells)
}

#[test]
fn open_ground_is_not_shaded_and_a_block_shades_the_corners_that_touch_it() {
    let ground = floor_with(&[]);
    for x in -3..=3 {
        for z in -3..=3 {
            assert_eq!(ground.levels([x, 0, z], FaceDirection::PositiveY), [0; 4]);
        }
    }
    // The block is the cube from (0, 1, 0) to (1, 2, 1); the floor's top faces are at y = 1. A face that touches it only at a corner has that one corner at level 1; one that
    // shares an edge with it has the two corners of the edge at level 1.
    let world = floor_with(&[[0, 1, 0]]);
    for (side_x, side_z) in [(1, 1), (-1, -1), (1, -1), (-1, 1)] {
        let levels = world.levels([side_x, 0, side_z], FaceDirection::PositiveY);
        assert_eq!(levels.iter().map(|&level| u32::from(level)).sum::<u32>(), 1);
        let dark = levels.iter().position(|&level| level == 1).unwrap();
        let place = Cells::vertex([side_x, 0, side_z], FaceDirection::PositiveY, dark);
        // (The corner that is dark is the corner of the block's footprint: x and z of the block's own corner.)
        assert!(
            [0, 1].contains(&place[0]) && [0, 1].contains(&place[2]),
            "{place:?}"
        );
    }
    for (side_x, side_z) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let levels = world.levels([side_x, 0, side_z], FaceDirection::PositiveY);
        assert_eq!(levels.iter().filter(|&&level| level == 1).count(), 2);
        assert_eq!(levels.iter().filter(|&&level| level == 0).count(), 2);
    }
}

#[test]
fn what_each_combination_of_side_and_corner_cells_gives_one_corner() {
    // The face of the stone at the origin that looks up; its corner 3 is at (1, 1, 1), with the cell it looks at at (0, 1, 0), its sides (0, 1, 1) and (1, 1, 0), and the
    // diagonal cell (1, 1, 1).
    let side_along_u = [0, 1, 1];
    let side_along_v = [1, 1, 0];
    let diagonal = [1, 1, 1];
    let cases: [(&[[i64; 3]], u8); 8] = [
        (&[], 0),
        (&[side_along_u], 1),
        (&[side_along_v], 1),
        (&[diagonal], 1),
        (&[side_along_u, diagonal], 2),
        (&[side_along_v, diagonal], 2),
        // Both sides closed: nothing gets round the corner, so it is as dark as it goes whether or not the diagonal is.
        (&[side_along_u, side_along_v], 3),
        (&[side_along_u, side_along_v, diagonal], 3),
    ];
    for (solid, expected) in cases {
        let levels = floor_with(solid).levels([0, 0, 0], FaceDirection::PositiveY);
        assert_eq!(levels[3], expected, "cells {solid:?}");
    }
    // The corner on the far side of the face looks at none of these cells.
    let world = floor_with(&[side_along_u, side_along_v, diagonal]);
    assert_eq!(world.levels([0, 0, 0], FaceDirection::PositiveY)[0], 0);
}

/// A tunnel one cell wide in a wall, going down one cell for each cell along x and `height` cells tall: the cell at (x, y, 0) is air when `1 <= x + y <= height`.
fn staircase(height: i64) -> Cells {
    let mut cells = HashSet::new();
    for x in -12..=12 {
        for y in -14..=14 {
            for z in -2..=0 {
                if !(z == 0 && (1..=height).contains(&(x + y))) {
                    cells.insert([x, y, z]);
                }
            }
        }
    }
    Cells(cells)
}

#[test]
fn the_wall_of_a_three_tall_staircase_has_a_saddle_in_its_middle_row_and_a_two_tall_one_has_none() {
    // The wall cell at (x, y, -1) looks along +z at the air cell (x, y, 0); the face's axes are x then y, so its corners are lower left, lower right, upper left, upper right.
    for (row, expected) in [
        (1, [3, 1, 1, 0]),
        // Two opposite corners are dark and the other two are not: no way of cutting the face in two triangles shades it right.
        (2, [1, 0, 0, 1]),
        (3, [0, 1, 1, 3]),
    ] {
        for x in -3..=3 {
            assert_eq!(
                staircase(3).levels([x, row - x, -1], FaceDirection::PositiveZ),
                expected,
                "row {row}, x {x}"
            );
        }
    }
    for x in -3..=3 {
        for row in [1, 2] {
            let levels = staircase(2).levels([x, row - x, -1], FaceDirection::PositiveZ);
            assert!(
                !(levels[0] == levels[3] && levels[1] == levels[2] && levels[0] != levels[1]),
                "row {row}: {levels:?}"
            );
        }
    }
}

fn hash(seed: u64, place: [i64; 3]) -> u64 {
    let mut value = seed ^ 0x9E37_79B9_7F4A_7C15;
    for component in place {
        value = (value ^ component as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value ^= value >> 29;
    }
    value
}

#[test]
fn faces_on_one_plane_that_share_a_vertex_agree_about_its_level() {
    // The level belongs to the corner, not to the face: two faces that look the same way from the same plane and meet at a corner look at the same four cells and give it the
    // same level, so the shade has no seam where one meets the next. (A rule that counted a face's own two edges would have seams.)
    for seed in 0..6u64 {
        let world = Cells(
            (-4..=4)
                .flat_map(|x| (-4..=4).flat_map(move |y| (-4..=4).map(move |z| [x, y, z])))
                .filter(|&cell| hash(seed, cell) % 100 < 40)
                .collect(),
        );
        let mut faces: Vec<([i64; 3], FaceDirection)> = Vec::new();
        for &voxel in &world.0 {
            for face in FaceDirection::ALL {
                if !world.blocks(step(voxel, face)) {
                    faces.push((voxel, face));
                }
            }
        }
        let mut compared = 0;
        for (index, &(voxel, face)) in faces.iter().enumerate() {
            for &(other, other_face) in &faces[index + 1..] {
                if face != other_face || voxel[face.axis()] != other[face.axis()] {
                    continue;
                }
                for corner in 0..4 {
                    for other_corner in 0..4 {
                        if Cells::vertex(voxel, face, corner)
                            == Cells::vertex(other, face, other_corner)
                        {
                            compared += 1;
                            assert_eq!(
                                world.levels(voxel, face)[corner],
                                world.levels(other, face)[other_corner],
                                "seed {seed}: {voxel:?} and {other:?} on {face:?}"
                            );
                        }
                    }
                }
            }
        }
        assert!(compared > 100, "seed {seed}: {compared} corners compared");
    }
}

// ---- the shader ----

/// The size of the pictures: a place on a face is a few pixels across, so the blend is told from the corners'.
const BIG: u32 = 256;

/// The camera narrowed to a field of view of 8 degrees: from about 45 voxels off it sees about 6 voxels across, so a place on a face is a few pixels across and the blend is told
/// from the corners'.
fn narrow(mut camera: Camera) -> Camera {
    camera.vertical_field_of_view = 8.0f32.to_radians();
    camera
}

fn brightness_at(image: &[u8], x: u32, y: u32) -> f64 {
    let start = ((y * BIG + x) * 4) as usize;
    linear_brightness([
        image[start],
        image[start + 1],
        image[start + 2],
        image[start + 3],
    ])
}

/// What of the world is solid (blocks the sun and is drawn), in the voxels of the first brick.
type Solid<'a> = &'a dyn Fn(usize, usize, usize) -> bool;

/// How the scene is lit. The shade is the same in each: it does not depend on the sun or on the shadows being drawn.
#[derive(Clone, Copy, Debug)]
enum Lighting {
    /// A sun under the ground (the faces that look up are turned from it, and the walls are edge on) with its shadows drawn: the faces are drawn by the shader that walks the sun's
    /// ray.
    ShadowsOn,
    /// The same sun with no shadows.
    ShadowsOff,
    /// No sun at all.
    NoSun,
}

const LIGHTINGS: [Lighting; 3] = [Lighting::ShadowsOn, Lighting::ShadowsOff, Lighting::NoSun];

/// The scene drawn twice from `camera` under the ambient light alone: with the voxels that block the sun told to the renderer (so the shader shades the corners) and without
/// (so nothing is shaded, and every face has its open brightness).
fn pictures(
    context: &GraphicsContext,
    name: &str,
    solid: Solid,
    camera: &Camera,
    lighting: Lighting,
) -> (Vec<u8>, Vec<u8>) {
    pictures_of_bricks(
        context,
        name,
        &[ORIGIN_BRICK],
        &|place| {
            place.iter().all(|&component| (0..32).contains(&component))
                && solid(place[0] as usize, place[1] as usize, place[2] as usize)
        },
        camera,
        lighting,
    )
}

/// As `pictures`, for a world of the bricks given (each meshed alone, with sky all round it), `solid` saying whether the voxel at a place in the world (in voxels from the origin)
/// is a cube.
fn pictures_of_bricks(
    context: &GraphicsContext,
    name: &str,
    bricks: &[BrickCoordinate],
    solid: &dyn Fn([i64; 3]) -> bool,
    camera: &Camera,
    lighting: Lighting,
) -> (Vec<u8>, Vec<u8>) {
    let mut opaque = vec![true; 256];
    opaque[0] = false;
    let table = BlockTable::of_cubes(&opaque);
    let atmosphere = Atmosphere {
        sun_direction: [0.0, -1.0, 0.0],
        sun_light: if matches!(lighting, Lighting::NoSun) {
            0
        } else {
            120
        },
        ambient_light: 120,
        shadows: matches!(lighting, Lighting::ShadowsOn),
        ..Atmosphere::default()
    };
    let mut shaded = new_renderer(context);
    let mut open = new_renderer(context);
    for &brick in bricks {
        let corner = [brick.x(), brick.y(), brick.z()].map(|component| component as i64 * 32);
        let at = |voxel: LocalVoxelIndex| {
            [voxel.x(), voxel.y(), voxel.z()]
                .iter()
                .zip(corner)
                .map(|(&local, base)| base + i64::from(local))
                .collect::<Vec<_>>()
                .try_into()
                .unwrap()
        };
        let mut values = vec![0u32; BRICK_VOXEL_COUNT];
        for voxel in LocalVoxelIndex::all() {
            if solid(at(voxel)) {
                values[voxel.index()] = RED;
            }
        }
        let storage = ChannelStorage::from_values(&values);
        let mesh = mesh_brick(
            &BrickNeighbourhood {
                center: BrickChannels::blocks_only(&storage),
                neighbours: [None; 6],
            },
            &table,
        )
        .pack();
        shaded.upload_brick(brick, &mesh);
        open.upload_brick(brick, &mesh);
        shaded.set_occupancy(
            brick,
            &occupancy_where(|x, y, z| {
                solid([
                    corner[0] + x as i64,
                    corner[1] + y as i64,
                    corner[2] + z as i64,
                ])
            }),
        );
    }
    shaded.set_atmosphere(atmosphere);
    let shaded = settled_at(&mut shaded, camera, BIG);
    save_big_image("shaded", name, &shaded);
    open.set_atmosphere(atmosphere);
    let open = settled_at(&mut open, camera, BIG);
    save_big_image("open", name, &open);
    (shaded, open)
}

fn save_big_image(kind: &str, adapter: &str, image: &[u8]) {
    let Some(directory) = std::env::var_os("VOX_SAVE_IMAGES").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&directory).expect("image directory can be created");
    let adapter: String = adapter
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let file = std::fs::File::create(directory.join(format!("volume_shade_{kind}_{adapter}.png")))
        .expect("image file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), BIG, BIG);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(image))
        .expect("the PNG is written");
}

/// How much of its open brightness the face is, at `place` (in voxels of the brick) as seen from `camera`: the shaded picture's pixel there over the open picture's.
fn share_at(shaded: &[u8], open: &[u8], camera: &Camera, place: [f64; 3]) -> f64 {
    let relative = std::array::from_fn(|axis| place[axis] - camera.position.in_voxels()[axis]);
    let (x, y) = pixel_of_sized(camera, relative, BIG).expect("the place is in the picture");
    brightness_at(shaded, x, y) / brightness_at(open, x, y)
}

/// The tops of columns of two heights, seen from above: every place on a top is as dark as the corner levels of its face, blended, say.
#[test]
fn the_shader_shades_a_place_on_a_face_as_the_rule_blended_across_the_voxel_says() {
    // Columns of height 13 or 14 over the whole of the floor of the brick, by a hash (two heights, so a column's neighbours reach at most one voxel over it).
    for seed in 0..3u64 {
        let height = |x: i64, z: i64| 13 + (hash(seed, [x, 0, z]) % 2) as i64;
        let solid = |x: usize, y: usize, z: usize| (y as i64) < height(x as i64, z as i64);
        let blocks = |cell: [i64; 3]| {
            (0..32).contains(&cell[0])
                && (0..32).contains(&cell[2])
                && cell[1] < height(cell[0], cell[2])
        };
        for context in contexts() {
            let name = adapter_name(&context);
            for lighting in LIGHTINGS {
                let mut camera = narrow(camera_at(ORIGIN_BRICK, [16.0, 60.0, 16.0]));
                camera.pitch = -Camera::MAXIMUM_PITCH;
                let (shaded, open) = pictures(&context, &name, &solid, &camera, lighting);
                let mut darkest: f64 = 1.0;
                let mut worst = 0.0f64;
                for x in 14..18i64 {
                    for z in 14..18i64 {
                        let top = height(x, z);
                        let levels =
                            corner_levels(blocks, [x, top - 1, z], FaceDirection::PositiveY);
                        for fraction_x in [0.25, 0.5, 0.75] {
                            for fraction_z in [0.25, 0.5, 0.75] {
                                // (The first axis of a face looking up is z and the second is x.)
                                let level = blended(levels, fraction_z, fraction_x);
                                let expected = 1.0 - SHADE_PER_LEVEL * level;
                                let place =
                                    [x as f64 + fraction_x, top as f64, z as f64 + fraction_z];
                                let share = share_at(&shaded, &open, &camera, place);
                                darkest = darkest.min(share);
                                worst = worst.max((share - expected).abs());
                            }
                        }
                    }
                }
                assert!(
                    darkest < 0.85,
                    "{name}, seed {seed}: some of the scene is shaded (the darkest place is {darkest} of its open brightness)"
                );
                assert!(
                    worst < 0.02,
                    "{name}, seed {seed}: a place is {worst} of its open brightness from what the rule says"
                );
            }
        }
    }
}

#[test]
fn the_floor_of_a_pit_one_voxel_wide_is_at_level_three_all_over() {
    for context in contexts() {
        let name = adapter_name(&context);
        for lighting in LIGHTINGS {
            let solid = |x: usize, y: usize, z: usize| y < 14 && (x, y, z) != (16, 13, 16);
            let mut camera = narrow(camera_at(ORIGIN_BRICK, [16.5, 60.0, 16.5]));
            camera.pitch = -Camera::MAXIMUM_PITCH;
            let (shaded, open) = pictures(&context, &name, &solid, &camera, lighting);
            // The bottom of the pit is the top of the voxel at y = 12, 13 voxels deep: look at its middle, and at open floor beside.
            let middle = share_at(&shaded, &open, &camera, [16.5, 13.0, 16.5]);
            assert!(
                (middle - 0.4).abs() < 0.02,
                "{name}: the middle of the pit's floor is {middle} of its open brightness, not 0.4"
            );
            let open_floor = share_at(&shaded, &open, &camera, [13.5, 14.0, 13.5]);
            assert!(
                (open_floor - 1.0).abs() < 0.01,
                "{name}: open floor is {open_floor}"
            );
        }
    }
}

#[test]
fn a_block_on_a_floor_darkens_the_four_diagonal_floor_squares_alike_and_the_four_beside_it_alike() {
    // The owner's first bug with the shade of Minecraft: of the floor squares that touch a block only at a corner, two opposite ones were dark over half their area and the
    // other two over all of it, because of how the quad's two triangles lie. The shade is blended across the voxel, so they are the same.
    for context in contexts() {
        let name = adapter_name(&context);
        let solid = |x: usize, y: usize, z: usize| y < 16 || (x, y, z) == (16, 16, 16);
        let mut camera = narrow(camera_at(ORIGIN_BRICK, [16.5, 60.0, 16.5]));
        camera.pitch = -Camera::MAXIMUM_PITCH;
        let (shaded, open) = pictures(&context, &name, &solid, &camera, Lighting::ShadowsOn);
        // Places on the floor's top (y = 16) a quarter of the way from the corner of the block: one in each of the four diagonal squares, and the same for the four squares
        // beside the block (at the middle of the edge it shares).
        let at = |x: f64, z: f64| share_at(&shaded, &open, &camera, [x, 16.0, z]);
        let diagonals = [
            at(15.75, 15.75),
            at(17.25, 15.75),
            at(15.75, 17.25),
            at(17.25, 17.25),
        ];
        let sides = [
            at(15.75, 16.5),
            at(17.25, 16.5),
            at(16.5, 15.75),
            at(16.5, 17.25),
        ];
        for group in [diagonals, sides] {
            let (lowest, highest) = group
                .iter()
                .fold((f64::MAX, f64::MIN), |(l, h), &v| (l.min(v), h.max(v)));
            assert!(
                highest - lowest < 0.02,
                "{name}: the four are as dark as each other: {group:?}"
            );
        }
        // The rule: the place is a quarter of the way from the block's corner (or edge), so a diagonal square is at level 1 * 0.75 * 0.75 there (one dark corner) and a side
        // square at 1 * 0.75 (two dark corners along the edge it shares).
        for (share, level) in [(diagonals[0], 0.5625), (sides[0], 0.75)] {
            assert!(
                (share - (1.0 - SHADE_PER_LEVEL * level)).abs() < 0.02,
                "{name}: {share} for level {level}"
            );
        }
        assert!(
            (at(14.0, 14.0) - 1.0).abs() < 0.01,
            "{name}: open floor is not shaded"
        );
    }
}

#[test]
fn the_wall_of_a_three_tall_staircase_is_shaded_as_its_saddle_says_and_without_a_sharp_edge() {
    // The tunnel is in the layer z = 15 of a wall (the layers z <= 14 are solid, and nothing in front of it), the cell at (x, y, 15) air when
    // `1 <= (x - 16) + (y - 16) <= 3`; the back of the tunnel is the face looking +z of the voxel behind each cell. Seen from the front, 40 voxels off.
    for context in contexts() {
        let name = adapter_name(&context);
        let tunnel = |x: i64, y: i64| (1..=3).contains(&((x - 16) + (y - 16)));
        let solid =
            |x: usize, y: usize, z: usize| z <= 14 || (z == 15 && !tunnel(x as i64, y as i64));
        let blocks = |cell: [i64; 3]| cell[2] <= 14 || (cell[2] == 15 && !tunnel(cell[0], cell[1]));
        let camera = narrow(camera_at(ORIGIN_BRICK, [16.0, 16.0, 56.0]));
        let (shaded, open) = pictures(&context, &name, &solid, &camera, Lighting::ShadowsOn);
        let mut worst = 0.0f64;
        let mut saddle = [f64::MAX; 2];
        for x in 14..19i64 {
            for y in 14..19i64 {
                if !tunnel(x, y) {
                    continue;
                }
                let levels = corner_levels(blocks, [x, y, 14], FaceDirection::PositiveZ);
                for fraction_x in [0.25, 0.5, 0.75] {
                    for fraction_y in [0.25, 0.5, 0.75] {
                        let expected =
                            1.0 - SHADE_PER_LEVEL * blended(levels, fraction_x, fraction_y);
                        let share = share_at(
                            &shaded,
                            &open,
                            &camera,
                            [x as f64 + fraction_x, y as f64 + fraction_y, 15.0],
                        );
                        worst = worst.max((share - expected).abs());
                    }
                }
                // The middle row (x + y = 32 + 2) has the saddle: the middle of its face is the mean of [1, 0, 0, 1], a level a half, whichever way its triangles lie.
                if (x - 16) + (y - 16) == 2 {
                    saddle[0] = saddle[0].min(share_at(
                        &shaded,
                        &open,
                        &camera,
                        [x as f64 + 0.5, y as f64 + 0.5, 15.0],
                    ));
                    saddle[1] = saddle[1].min(share_at(
                        &shaded,
                        &open,
                        &camera,
                        [x as f64 + 0.25, y as f64 + 0.75, 15.0],
                    ));
                }
            }
        }
        assert!(
            worst < 0.02,
            "{name}: a place on the wall is {worst} of its open brightness from what the rule says"
        );
        assert!(
            (saddle[0] - 0.9).abs() < 0.02,
            "{name}: the middle of the saddle face is level 0.5 ({})",
            saddle[0]
        );
        // A quarter along and three quarters up the saddle face: the two dark corners are the lower left and the upper right, so it is level 0.25 * 0.25 + 0.75 * 0.75 = 0.625
        // a quarter in from each... (the rule's own blend, not a guess).
        let expected = 1.0 - SHADE_PER_LEVEL * blended([1, 0, 0, 1], 0.25, 0.75);
        assert!(
            (saddle[1] - expected).abs() < 0.02,
            "{name}: {} against {expected}",
            saddle[1]
        );
    }
}

#[test]
fn the_shade_is_the_same_across_the_edges_and_corners_of_bricks() {
    // Columns of height 13 or 14 over four bricks that meet in a line of corners (x = 32, z = 32), the camera over it: the cells beside a face are in the next brick, across an
    // edge of it or in a brick that touches only at a corner, and the shader reads them from the table of bricks.
    for seed in 0..3u64 {
        let height = |x: i64, z: i64| 13 + (hash(seed, [x, 0, z]) % 2) as i64;
        let blocks = |cell: [i64; 3]| {
            (0..64).contains(&cell[0])
                && (0..64).contains(&cell[2])
                && cell[1] < height(cell[0], cell[2])
        };
        let bricks = [
            BrickCoordinate::new(0, 0, 0),
            BrickCoordinate::new(1, 0, 0),
            BrickCoordinate::new(0, 0, 1),
            BrickCoordinate::new(1, 0, 1),
        ];
        for context in contexts() {
            let name = adapter_name(&context);
            for lighting in LIGHTINGS {
                let mut camera = narrow(camera_at(ORIGIN_BRICK, [32.0, 60.0, 32.0]));
                camera.pitch = -Camera::MAXIMUM_PITCH;
                let (shaded, open) =
                    pictures_of_bricks(&context, &name, &bricks, &blocks, &camera, lighting);
                let mut darkest: f64 = 1.0;
                let mut worst = 0.0f64;
                for x in 30..34i64 {
                    for z in 30..34i64 {
                        let top = height(x, z);
                        let levels =
                            corner_levels(blocks, [x, top - 1, z], FaceDirection::PositiveY);
                        for fraction_x in [0.25, 0.5, 0.75] {
                            for fraction_z in [0.25, 0.5, 0.75] {
                                let expected =
                                    1.0 - SHADE_PER_LEVEL * blended(levels, fraction_z, fraction_x);
                                let place =
                                    [x as f64 + fraction_x, top as f64, z as f64 + fraction_z];
                                let share = share_at(&shaded, &open, &camera, place);
                                darkest = darkest.min(share);
                                worst = worst.max((share - expected).abs());
                            }
                        }
                    }
                }
                assert!(
                    darkest < 0.85,
                    "{name}, seed {seed}: some of it is shaded ({darkest})"
                );
                assert!(
                    worst < 0.02,
                    "{name}, seed {seed}: a place is {worst} of its open brightness from what the rule says"
                );
            }
        }
    }
}
