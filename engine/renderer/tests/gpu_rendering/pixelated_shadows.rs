//! Pixelated shadows (`VoxelRenderer::set_pixelated_shadows`, `docs/history/pixelated-shadows-study.md`): every pixel of a block's picture is in the light, or in the shade, as a
//! whole, so that the edge of a shadow steps along the picture's pixels.
//!
//! The scene is the roof of the shadow tests (a roof eight cells up over the first ten along x, a sun that puts the edge of its shadow at x = 5.725 on the floor, a quarter of the way
//! through a picture pixel), seen from just over the floor through a narrow view, so that a pixel of the floor's picture (a sixteenth of a voxel) is nine pixels of the screen
//! across. For each picture pixel the brightness of the screen pixels that are well inside it is read: with the setting on they must all be the same.

use block_models::BlockTable;

use super::*;

const BLOCK: u32 = 1;
/// The pixels of a picture along a side of it, which is also how many to a voxel (a picture covers a voxel's face).
const PICTURE_PIXELS: usize = 16;
/// The sun of the scene: 54 degrees up, over the +x side and a little to +z. The edge of the roof's shadow on the floor (seven cells under it) falls at x = 10 - 6 * 0.57 / 0.8 = 5.725.
const SUN: [f32; 3] = [0.57, 0.8, 0.1];
/// Where the camera is, over the floor beside the edge of the shadow, and what it looks at.
const EYE: [f64; 3] = [5.7, 3.5, 16.3];
const LOOKED_AT: [f64; 3] = [5.7, 1.0, 16.0];

/// A picture that is white and opaque everywhere, so that only the light changes a pixel's brightness.
fn white_picture() -> Vec<u8> {
    (0..PICTURE_PIXELS * PICTURE_PIXELS)
        .flat_map(|_| [255, 255, 255, 255])
        .collect()
}

/// Air and a grey block with the white picture on every face.
fn table() -> BlockTable {
    let tile = block_models::TileRef::new(0, 0).expect("a place for the picture");
    let grey = BlockDefinition::cube([0.7, 0.7, 0.7])
        .with_texture(namespaced_id::NamespacedId::new("test", "picture").unwrap())
        .with_pictures(block_models::FacePictures {
            top: tile,
            side: tile,
            bottom: tile,
        });
    BlockTable::from_definitions(&[Some(&BlockDefinition::air()), Some(&grey)])
}

/// Where the ship of the second test has its origin, and how far it is turned about the vertical axis (radians).
const SHIP_ORIGIN: [f64; 3] = [8.0, 0.0, 8.0];
const SHIP_TURN: f64 = std::f64::consts::FRAC_PI_6;

/// A place of the scene (terrain: where it is; a ship: its own, turned and placed) as a place in the world, and a direction of it as a direction in the world.
#[derive(Clone, Copy)]
struct Placement {
    turn: Option<f64>,
}

impl Placement {
    fn rotation(self) -> glam::Quat {
        let [x, y, z, w] = Orientation::from_yaw_pitch(self.turn.unwrap_or(0.0), 0.0)
            .components()
            .map(|component| component as f32);
        glam::Quat::from_xyzw(x, y, z, w)
    }

    fn place(self, local: [f64; 3]) -> [f64; 3] {
        if self.turn.is_none() {
            return local;
        }
        let turned = self.rotation() * Vec3::new(local[0] as f32, local[1] as f32, local[2] as f32);
        [
            SHIP_ORIGIN[0] + f64::from(turned.x),
            SHIP_ORIGIN[1] + f64::from(turned.y),
            SHIP_ORIGIN[2] + f64::from(turned.z),
        ]
    }

    fn direction(self, local: [f32; 3]) -> [f32; 3] {
        (self.rotation() * Vec3::from(local)).to_array()
    }
}

/// The floor and the roof, drawn with pictures (as terrain, or as a ship turned about the vertical axis), in a renderer with the shadows on and the setting as given.
fn roof_scene(context: &GraphicsContext, pixelated: bool, placement: Placement) -> VoxelRenderer {
    let table = table();
    let mut renderer = new_renderer(context);
    let picture = white_picture();
    renderer.set_palette_with_pictures(
        &table.palette(),
        &table.palette_pictures(),
        &[PictureSize {
            size: PICTURE_PIXELS as u32,
            tiles: vec![&picture],
            ..PictureSize::default()
        }],
    );
    renderer.set_picture_distance(100.0);
    let mut blocks = Vec::new();
    for z in 0..32u8 {
        for x in 0..32u8 {
            blocks.push(([x, 0, z], BLOCK, 0));
            if x < 10 {
                blocks.push(([x, 7, z], BLOCK, 0));
            }
        }
    }
    match placement.turn {
        None => place_blocks_in(&mut renderer, &table, ORIGIN_BRICK, &blocks, Some(0)),
        Some(turn) => {
            renderer.set_grid_pose(
                a_ship(),
                pose(
                    [0, 0, 0],
                    SHIP_ORIGIN,
                    Orientation::from_yaw_pitch(turn, 0.0),
                ),
            );
            renderer.upload_grid_brick(
                a_ship(),
                ORIGIN_BRICK,
                &mesh_of_blocks(&table, &blocks, Some(0)),
            );
        }
    }
    renderer.set_atmosphere(shadow_atmosphere(0, placement.direction(SUN), true));
    renderer.set_pixelated_shadows(pixelated);
    renderer
}

/// For each pixel of the floor's picture in the part of the floor the camera looks at, from its x and z: the dimmest and the brightest of nine screen pixels spread well inside it.
fn picture_pixels_read(image: &[u8], camera: &Camera, placement: Placement) -> Vec<(f64, f64)> {
    let eye = placement.place(EYE);
    let mut readings = Vec::new();
    for across_z in 252..260 {
        for across_x in 87..96 {
            let (mut dimmest, mut brightest, mut seen) = (f64::MAX, f64::MIN, 0);
            for share_x in [0.2, 0.5, 0.8] {
                for share_z in [0.2, 0.5, 0.8] {
                    let place = placement.place([
                        (f64::from(across_x) + share_x) / PICTURE_PIXELS as f64,
                        1.0,
                        (f64::from(across_z) + share_z) / PICTURE_PIXELS as f64,
                    ]);
                    let relative = std::array::from_fn(|axis| place[axis] - eye[axis]);
                    let Some((x, y)) = pixel_of(camera, relative) else {
                        continue;
                    };
                    let light = linear_brightness(pixel(image, x, y));
                    dimmest = dimmest.min(light);
                    brightest = brightest.max(light);
                    seen += 1;
                }
            }
            if seen == 9 {
                readings.push((dimmest, brightest));
            }
        }
    }
    readings
}

/// How many picture pixels are read, how many of them are not one brightness all through (the dimmest of a pixel's samples is under 98 percent of the brightest), how many are
/// as bright as the brightest anywhere (in the light) and how many under a quarter of that (in a shadow).
fn counts(readings: &[(f64, f64)]) -> (usize, usize, usize, usize) {
    let brightest = readings
        .iter()
        .map(|&(_, brightest)| brightest)
        .fold(0.0, f64::max);
    let mixed = readings
        .iter()
        .filter(|&&(dimmest, brightest)| dimmest < brightest * 0.98)
        .count();
    let lit = readings
        .iter()
        .filter(|&&(dimmest, _)| dimmest > brightest * 0.98)
        .count();
    let shaded = readings
        .iter()
        .filter(|&&(_, most)| most < brightest * 0.25)
        .count();
    (readings.len(), mixed, lit, shaded)
}

/// With the setting on, every picture pixel of the floor that is read is one brightness, the edge of the shadow is in the picture, and with it off the edge cuts some in two.
fn check_the_floor(context: &GraphicsContext, placement: Placement, what: &str) {
    let name = adapter_name(context);
    let camera = camera_aimed_at(placement.place(EYE), placement.place(LOOKED_AT));
    for pixelated in [false, true] {
        let mut renderer = roof_scene(context, pixelated, placement);
        let image = settled(&mut renderer, &camera);
        save_image(
            &format!(
                "pixelated_shadow_{what}_{}",
                if pixelated { "on" } else { "off" }
            ),
            &name,
            &image,
        );
        let (read, mixed, lit, shaded) = counts(&picture_pixels_read(&image, &camera, placement));
        assert!(
            read >= 24 && lit >= 5 && shaded >= 5,
            "{name}, {what}, pixelated {pixelated}: of {read} picture pixels {lit} are lit and {shaded} are in the shadow: the test could not see the edge"
        );
        if pixelated {
            assert_eq!(
                mixed, 0,
                "{name}, {what}: {mixed} of {read} picture pixels are partly in the light and partly in a shadow with the setting on"
            );
        } else {
            assert!(
                mixed >= 4,
                "{name}, {what}: only {mixed} of {read} picture pixels are cut by the edge of the shadow with the setting off: the test could not see the difference"
            );
        }
    }
}

#[test]
fn with_the_setting_on_each_pixel_of_a_picture_on_the_floor_is_one_brightness_and_with_it_off_the_edge_of_a_shadow_cuts_pixels_in_two()
 {
    for context in contexts() {
        check_the_floor(&context, Placement { turn: None }, "terrain");
    }
}

#[test]
fn the_same_holds_for_a_ship_turned_30_degrees_whose_pictures_are_turned_with_it() {
    for context in contexts() {
        check_the_floor(
            &context,
            Placement {
                turn: Some(SHIP_TURN),
            },
            "ship",
        );
    }
}
