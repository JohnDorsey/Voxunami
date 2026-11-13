//! Bumps from a picture's normal companion (`PictureSize::normal`, `docs/specs/block-textures-spec.md` section 4.4): the normal picture (LabPBR `_n`) tilts the normal the
//! sun's light is the cosine of, so that a pixel tilted toward the sun is brighter than a flat one and a pixel tilted away is darker.
//!
//! The scene is one block, a face of it seen straight on, with a white picture of 8 pixels across that is tilted the same everywhere, in no light but a sun's whose rays graze
//! the face at a known angle. The brightness of the face with the tilted picture over its brightness with a flat one is the cosine the tilted normal makes with the sun over the
//! cosine the face's own normal makes: a ratio that does not depend on how bright the sun is made, and that is worked out here from the rule of the format.

use super::contact_trace::occupancy_where;
use super::emission::{table, white_picture};
use super::*;

pub(super) const PICTURE_PIXELS: usize = 8;
/// The byte of a normal picture that is no tilt in that direction (the one `NO_NORMAL` is made of): 0 is -1 and 255 is 1 on either side of it.
const FLAT: i32 = 128;
/// How far from `FLAT` the tilted pictures go: a tilt of 38/127, about 0.3 (17 degrees).
const TILT: i32 = 38;
/// The cosine the sun makes with the face's own normal: the sun is 0.6 of the way along the normal and 0.8 along the direction the picture is tilted.
const FLAT_COSINE: f64 = 0.6;

/// A face of the block, and a camera that looks at it straight on from outside: the face's normal, and the directions of the picture's right and of its top as they are in the
/// world (the picture is seen upright and the right way round from outside: `a_face_with_a_picture_shows_it_upright_and_the_right_way_round_from_outside_on_a_side_and_on_top`).
pub(super) struct Face {
    pub(super) name: &'static str,
    pub(super) camera: [f64; 3],
    pub(super) aim: [f64; 3],
    pub(super) normal: [f64; 3],
    pub(super) right: [f64; 3],
    pub(super) up: [f64; 3],
}

pub(super) const PLUS_Z: Face = Face {
    name: "+z",
    camera: [16.5, 16.5, 30.0],
    aim: [16.5, 16.5, 17.0],
    normal: [0.0, 0.0, 1.0],
    right: [1.0, 0.0, 0.0],
    up: [0.0, 1.0, 0.0],
};
const MINUS_X: Face = Face {
    name: "-x",
    camera: [2.0, 16.5, 16.5],
    aim: [15.0, 16.5, 16.5],
    normal: [-1.0, 0.0, 0.0],
    right: [0.0, 0.0, 1.0],
    up: [0.0, 1.0, 0.0],
};

/// The top, seen from above with north (-z) up the picture: its top is north and its right is east.
const PLUS_Y: Face = Face {
    name: "+y",
    camera: [16.5, 30.0, 16.5],
    aim: [16.5, 17.0, 16.5],
    normal: [0.0, 1.0, 0.0],
    right: [1.0, 0.0, 0.0],
    up: [0.0, 0.0, -1.0],
};

/// A normal picture whose every pixel is tilted by `(right, up)` bytes from flat, with the ambient occlusion and the height of LabPBR (255, 255) which are not read.
fn tilted(right: i32, up: i32) -> Vec<u8> {
    (0..PICTURE_PIXELS * PICTURE_PIXELS)
        .flat_map(|_| [(FLAT + right) as u8, (FLAT + up) as u8, 255, 255])
        .collect()
}

/// A normal picture with no tilt and the ambient occlusion byte `occlusion` (LabPBR's blue: 255 is nothing in the way of the ambient light, 0 is all of it) everywhere.
pub(super) fn occluded(occlusion: u8) -> Vec<u8> {
    (0..PICTURE_PIXELS * PICTURE_PIXELS)
        .flat_map(|_| [FLAT as u8, FLAT as u8, occlusion, 255])
        .collect()
}

fn scaled(vector: [f64; 3], by: f64) -> [f64; 3] {
    vector.map(|part| part * by)
}

fn sum(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// One picture to take: a face of the block with the normal picture `normal` (or none), in the light of a sun in direction `sun`, with the pictures drawn up to
/// `picture_distance`, and with the shader that walks the sun's ray (and the shadows on) or without it.
#[derive(Clone, Copy)]
pub(super) struct Scene<'a> {
    pub(super) face: &'a Face,
    pub(super) normal: Option<&'a [u8]>,
    pub(super) sun: [f64; 3],
    pub(super) picture_distance: f32,
    pub(super) traced: bool,
    /// The lights of the atmosphere, 0 to 255: the sun's, the sky's and the ambient light's.
    pub(super) sun_light: u8,
    pub(super) sky_light: u8,
    pub(super) ambient_light: u8,
}

impl<'a> Scene<'a> {
    /// The scene with the pictures drawn far (100 voxels), the walk of the sun's ray off, and the sun the only light.
    pub(super) fn new(face: &'a Face, normal: Option<&'a [u8]>, sun: [f64; 3]) -> Self {
        Scene {
            face,
            normal,
            sun,
            picture_distance: 100.0,
            traced: false,
            sun_light: 255,
            sky_light: 0,
            ambient_light: 0,
        }
    }

    /// The brightness of the face in the middle of the picture of the scene, on the adapter `name`; `what` names the saved picture.
    pub(super) fn brightness(&self, context: &GraphicsContext, name: &str, what: &str) -> f64 {
        let table = table();
        let mut renderer = new_renderer(context);
        let picture = white_picture();
        renderer.set_palette_with_pictures(
            &table.palette(),
            &table.palette_pictures(),
            &[PictureSize {
                size: PICTURE_PIXELS as u32,
                tiles: vec![&picture],
                normal: vec![self.normal],
                ..PictureSize::default()
            }],
        );
        renderer.set_picture_distance(self.picture_distance);
        place_blocks_in(
            &mut renderer,
            &table,
            ORIGIN_BRICK,
            &[([16, 16, 16], 1, 0)],
            Some(0),
        );
        if self.traced {
            renderer.set_occupancy(
                ORIGIN_BRICK,
                &occupancy_where(|x, y, z| [x, y, z] == [16, 16, 16]),
            );
        }
        renderer.set_atmosphere(Atmosphere {
            sun_direction: self.sun.map(|part| part as f32),
            sun_light: self.sun_light,
            sky_light: self.sky_light,
            ambient_light: self.ambient_light,
            shadows: self.traced,
            ..Atmosphere::default()
        });
        let camera = camera_aimed_at(self.face.camera, self.face.aim);
        if self.traced {
            let statistics = renderer.render_to_rgba(&camera, SIZE, SIZE).1;
            assert_eq!(
                statistics.traced_windows, 1,
                "{name}: the scene is not drawn with the walk of the sun's ray: {statistics:?}"
            );
        }
        let image = settled(&mut renderer, &camera);
        save_image(&format!("bumps_{what}"), name, &image);
        linear_brightness(pixel(&image, SIZE / 2, SIZE / 2))
    }
}

/// What a pixel tilted by `tilt` toward `toward` (a direction in the face's plane, a unit vector) is lit as, over what a flat one is, in the light of the sun that is 0.6 along
/// the normal and 0.8 along `toward`: the cosine of the tilted normal with the sun, over the flat cosine. The tilt is the byte's: x and y are
/// `(byte - 128) / 127`, and the normal's third part is what makes the length 1.
fn expected_ratio(face: &Face, toward: [f64; 3], tilt_bytes: i32) -> f64 {
    let tilt = f64::from(tilt_bytes) / 127.0;
    let normal = sum(
        scaled(face.normal, (1.0 - tilt * tilt).sqrt()),
        scaled(toward, tilt),
    );
    let sun = sum(scaled(face.normal, FLAT_COSINE), scaled(toward, 0.8));
    dot(normal, sun) / FLAT_COSINE
}

/// The four tilts of a picture on a face and the sun that makes each of them show: toward and away from the sun along the picture's top, and along its right.
fn cases(face: &Face) -> Vec<(&'static str, [f64; 3], i32, i32, i32)> {
    let negative = |vector: [f64; 3]| scaled(vector, -1.0);
    vec![
        ("up", face.up, 0, TILT, TILT),
        ("down", face.up, 0, -TILT, -TILT),
        ("right", face.right, TILT, 0, TILT),
        ("left", face.right, -TILT, 0, -TILT),
        // (Toward the sun, which is below the face's middle: the same pictures, with the sun on the other side.)
        ("up, the sun below", negative(face.up), 0, TILT, -TILT),
        (
            "right, the sun to the left",
            negative(face.right),
            TILT,
            0,
            -TILT,
        ),
    ]
}

pub(super) fn sun_of(face: &Face, toward: [f64; 3]) -> [f64; 3] {
    sum(scaled(face.normal, FLAT_COSINE), scaled(toward, 0.8))
}

fn check_every_tilt(traced: bool) {
    for context in contexts() {
        let name = adapter_name(&context);
        for face in [&PLUS_Z, &MINUS_X, &PLUS_Y] {
            for (what, toward, right, up, along) in cases(face) {
                let sun = sun_of(face, toward);
                let (flat_picture, bumpy_picture) = (tilted(0, 0), tilted(right, up));
                let flat = Scene {
                    traced,
                    ..Scene::new(face, Some(&flat_picture), sun)
                }
                .brightness(&context, &name, "flat");
                let bumpy = Scene {
                    traced,
                    ..Scene::new(face, Some(&bumpy_picture), sun)
                }
                .brightness(&context, &name, &format!("{}_{what}", face.name));
                let wanted = expected_ratio(face, toward, along);
                assert!(
                    flat > 0.2,
                    "{name}: face {}: the flat face is lit {flat:.3}, which is too little to say anything by",
                    face.name
                );
                assert!(
                    (bumpy / flat - wanted).abs() < 0.03,
                    "{name}: face {} tilted {what}: {bumpy:.3} over {flat:.3} is {:.3}, wanted {wanted:.3}",
                    face.name,
                    bumpy / flat
                );
            }
        }
    }
}

#[test]
fn a_pixel_tilted_toward_the_sun_is_lit_by_the_cosine_of_its_tilted_normal_on_any_face_and_in_any_direction()
 {
    check_every_tilt(false);
}

#[test]
fn the_same_holds_in_the_shader_that_walks_the_suns_ray() {
    check_every_tilt(true);
}

#[test]
fn a_flat_normal_picture_a_picture_with_none_and_a_tilt_beyond_the_picture_distance_all_leave_the_face_as_it_was()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        let face = &PLUS_Z;
        let sun = sun_of(face, face.up);
        let (flat_picture, tilted_picture) = (tilted(0, 0), tilted(0, TILT));
        let none = Scene::new(face, None, sun).brightness(&context, &name, "none");
        let flat =
            Scene::new(face, Some(&flat_picture), sun).brightness(&context, &name, "flat_again");
        assert!(
            (flat / none - 1.0).abs() < 0.005,
            "{name}: a flat normal picture lights the face {flat:.4}, none {none:.4}"
        );
        // The picture is gone beyond its distance (here 6 voxels, and the camera is 13 from the face), and the tilt with it.
        let far = Scene {
            picture_distance: 6.0,
            ..Scene::new(face, Some(&tilted_picture), sun)
        }
        .brightness(&context, &name, "beyond");
        let far_flat = Scene {
            picture_distance: 6.0,
            ..Scene::new(face, Some(&flat_picture), sun)
        }
        .brightness(&context, &name, "beyond_flat");
        assert!(
            (far / far_flat - 1.0).abs() < 0.005,
            "{name}: beyond the picture distance the tilt still shows: {far:.4} over {far_flat:.4}"
        );
        // And the tilt did show where the picture is drawn.
        let near = Scene::new(face, Some(&tilted_picture), sun).brightness(&context, &name, "near");
        assert!(
            near > flat * 1.2,
            "{name}: the tilt toward the sun shows nothing: {near:.4} against {flat:.4}"
        );
    }
}
