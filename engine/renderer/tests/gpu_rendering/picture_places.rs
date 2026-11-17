//! Which pixel of a picture is drawn at a place on a face: the oracle for `FacePicture::place_of` (`docs/world-painting-options.md`, section E, P3).
//!
//! The CPU says which pixel of a block's picture a point of a face is (`BlockTable::face_picture` and `FacePicture::place_of`), without the graphics card, so that a tool can
//! paint there and a server can use the same answer. Here the card is asked the same question: one picture whose every pixel has a colour of its own is put on every face
//! of a block in each of its 24 orientations, which shines by itself (so that no light or shading changes a colour), and the pixel drawn where the card is aimed at a point of
//! the face must be the pixel the CPU names.
//!
//! What a colour is is read from the screen and not from the code under test: a picture of the block turned as it is made is seen head on from the +z side, where red grows
//! to the screen's right and green down it (the picture's own layout), and the 64 colours of that view are the key to every other.

use block_models::{BlockTable, ORIENTATION_COUNT};
use voxel_grid::ChannelStorage;
use voxel_meshing::{BrickChannels, BrickNeighbourhood, mesh_brick};

use super::emission::table;
use super::*;

const PIXELS: usize = 8;

/// A picture whose pixel in column `c` and row `r` is `[20 + 28 c, 20 + 28 r, 140]`.
fn coded_picture() -> Vec<u8> {
    (0..PIXELS * PIXELS)
        .flat_map(|index| {
            let (column, row) = (index % PIXELS, index / PIXELS);
            [20 + 28 * column as u8, 20 + 28 * row as u8, 140, 255]
        })
        .collect()
}

/// A specular companion in which every pixel shines wholly (alpha 254), whatever the light.
fn shining() -> Vec<u8> {
    (0..PIXELS * PIXELS)
        .flat_map(|_| [200, 150, 100, 254])
        .collect()
}

/// The mesh of a brick holding one block, a whole cube of block 1, at the middle, turned as `orientation` says.
fn mesh_of_one_block(table: &BlockTable, orientation: u32) -> voxel_meshing::PackedBrickMesh {
    let index = LocalVoxelIndex::new(16, 16, 16).index();
    let mut blocks = vec![0u32; BRICK_VOXEL_COUNT];
    let mut orientations = vec![0u32; BRICK_VOXEL_COUNT];
    blocks[index] = 1;
    orientations[index] = orientation;
    let (blocks, orientations) = (
        ChannelStorage::from_values(&blocks),
        ChannelStorage::from_values(&orientations),
    );
    mesh_brick(
        &BrickNeighbourhood {
            center: BrickChannels {
                blocks: &blocks,
                states: None,
                orientations: Some(&orientations),
                light: None,
            },
            neighbours: [None; 6],
        },
        table,
    )
    .pack()
}

/// The renderer, with the coded picture on the block's faces, shining, in the dark.
fn renderer_with_the_picture(context: &GraphicsContext, table: &BlockTable) -> VoxelRenderer {
    let mut renderer = new_renderer(context);
    let (picture, specular) = (coded_picture(), shining());
    renderer.set_palette_with_pictures(
        &table.palette(),
        &table.palette_pictures(),
        &[PictureSize {
            size: PIXELS as u32,
            tiles: vec![&picture],
            specular: vec![Some(&specular)],
            ..PictureSize::default()
        }],
    );
    renderer.set_picture_distance(100.0);
    renderer.set_atmosphere(Atmosphere {
        sky_color: SKY,
        sun_light: 0,
        sky_light: 0,
        ambient_light: 0,
        fog_start: 1000.0,
        fog_end: 2000.0,
        ..Atmosphere::default()
    });
    renderer
}

/// The colour at the middle of the picture when the eye at `eye` is aimed at `place` (voxels of the brick).
fn colour_aimed_at(renderer: &mut VoxelRenderer, eye: [f64; 3], place: [f64; 3]) -> [u8; 3] {
    let image = settled(renderer, &camera_aimed_at(eye, place));
    let [red, green, blue, _] = pixel(&image, SIZE / 2, SIZE / 2);
    [red, green, blue]
}

/// What colour the card draws for each of the picture's pixels: seen head on, the block as it is made, from the +z side.
fn key(renderer: &mut VoxelRenderer) -> Vec<([u8; 3], [usize; 2])> {
    let mut key = Vec::new();
    for row in 0..PIXELS {
        for column in 0..PIXELS {
            // Red grows to the right (+x), green downward, as the picture is laid out.
            let place = [
                16.0 + (column as f64 + 0.5) / PIXELS as f64,
                17.0 - (row as f64 + 0.5) / PIXELS as f64,
                17.0,
            ];
            let colour = colour_aimed_at(renderer, [place[0], place[1], 21.0], place);
            key.push((colour, [column, row]));
        }
    }
    key
}

/// The pixel the key says a colour is, and how far the colour is from the key's (the sum of the channels' differences).
fn pixel_of_colour(key: &[([u8; 3], [usize; 2])], colour: [u8; 3]) -> ([usize; 2], u32) {
    key.iter()
        .map(|(known, pixel)| {
            let distance: u32 = (0..3)
                .map(|channel| u32::from(known[channel].abs_diff(colour[channel])))
                .sum();
            (*pixel, distance)
        })
        .min_by_key(|(_, distance)| *distance)
        .expect("a key of 64 colours")
}

#[test]
fn the_pixel_the_card_draws_at_a_place_on_a_face_is_the_pixel_the_cpu_names_in_every_orientation() {
    for context in contexts() {
        let name = adapter_name(&context);
        let table = table();
        let mut renderer = renderer_with_the_picture(&context, &table);
        renderer.upload_brick(ORIGIN_BRICK, &mesh_of_one_block(&table, 0));
        let key = key(&mut renderer);
        // The key is sound: 64 colours, each far from every other, and red grows with the column and green with the row.
        for (index, (colour, pixel)) in key.iter().enumerate() {
            for (other, other_pixel) in key.iter().skip(index + 1) {
                let apart: u32 = (0..3)
                    .map(|channel| u32::from(colour[channel].abs_diff(other[channel])))
                    .sum();
                assert!(
                    apart >= 20,
                    "{name}: pixels {pixel:?} and {other_pixel:?} look alike"
                );
            }
            let neighbour = |column: usize, row: usize| {
                key.iter()
                    .find(|(_, at)| *at == [column, row])
                    .map(|(colour, _)| *colour)
                    .unwrap()
            };
            if pixel[0] + 1 < PIXELS {
                assert!(
                    neighbour(pixel[0] + 1, pixel[1])[0] > colour[0],
                    "{name}: {pixel:?}"
                );
            }
            if pixel[1] + 1 < PIXELS {
                assert!(
                    neighbour(pixel[0], pixel[1] + 1)[1] > colour[1],
                    "{name}: {pixel:?}"
                );
            }
        }

        let mut probes = 0;
        let mut worst_colour_distance = 0;
        for orientation in 0..ORIENTATION_COUNT as u32 {
            renderer.upload_brick(ORIGIN_BRICK, &mesh_of_one_block(&table, orientation));
            for face in FaceDirection::ALL {
                let picture = table
                    .face_picture(1, 0, orientation, face)
                    .unwrap_or_else(|| panic!("{name}: no picture on {face:?} at {orientation}"));
                let (along, across_axis, down_axis) = {
                    let axis = face.axis();
                    (axis, (axis + 1) % 3, (axis + 2) % 3)
                };
                // Eight places on the face, at the middles of eighths: each tangent coordinate takes every eighth once, in an order that differs between the two.
                for step in 0..PIXELS {
                    let tangents = [
                        (step as f64 + 0.5) / PIXELS as f64,
                        ((step * 3 + 1) % PIXELS) as f64 / PIXELS as f64 + 0.5 / PIXELS as f64,
                    ];
                    let mut local = [0.0; 3];
                    local[along] = if face.is_positive() { 1.0 } else { 0.0 };
                    local[across_axis] = tangents[0];
                    local[down_axis] = tangents[1];
                    let place = [16.0 + local[0], 16.0 + local[1], 16.0 + local[2]];
                    // The eye is out from the face, and off to the side along each of its axes, so that the face is seen at a slant.
                    let mut eye = place;
                    eye[along] += if face.is_positive() { 3.0 } else { -3.0 };
                    eye[across_axis] += if step % 2 == 0 { 1.2 } else { -1.2 };
                    eye[down_axis] += if step % 3 == 0 { 0.7 } else { -0.9 };
                    let colour = colour_aimed_at(&mut renderer, eye, place);
                    let (drawn, distance) = pixel_of_colour(&key, colour);
                    worst_colour_distance = worst_colour_distance.max(distance);
                    let wanted = picture.place_of(local).pixel(PIXELS);
                    assert_eq!(
                        drawn, wanted,
                        "{name}: orientation {orientation}, {face:?}, at {local:?}: the card draws pixel {drawn:?} (colour {colour:?}, {distance} from the key's), the CPU names {wanted:?}"
                    );
                    probes += 1;
                }
            }
        }
        assert_eq!(probes, ORIENTATION_COUNT * 6 * PIXELS);
        // The colours read are the key's, not something between two of them (the key's neighbours are at least 56 apart in a channel).
        assert!(
            worst_colour_distance <= 12,
            "{name}: a colour was {worst_colour_distance} from the key's"
        );
    }
}
