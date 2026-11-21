//! The shadow a picture's own relief casts (`docs/specs/block-textures-spec.md` section 4.4): the alpha of a normal picture (LabPBR `_n`) is a height, 255 the top and 0 the
//! lowest, and a pixel that a higher one toward the sun stands over, in the sun's ray, is in the shade of it.
//!
//! The scene is the one of `bumps.rs` (a face of one block seen straight on, a picture 8 pixels across, the sun the only light), but looked at in one pixel of the picture at a time,
//! and the picture has a ridge: the two rows or columns of pixels nearest the sun are at the top and the rest at the bottom, 0.1 voxel lower (the depth of the relief, a number of the
//! shader's). The sun is 0.6 along the normal and 0.8 along the face toward the ridge, so its ray climbs 0.75 of a voxel for each voxel along the face: it clears the top of the ridge
//! 0.133 of a voxel from a pixel at the bottom. The pixel next to the ridge (0.0625 from it) is in its shade; the pixel after the next (0.1875) is not, and neither is the ridge, which
//! is the top. What a pixel is lit as is compared with what it is lit as with no relief (every height 255): the shade takes the sun's light, and the ambient and the sky's are 0
//! here, so the shade is black.

use super::bumps::{Face, PICTURE_PIXELS, PLUS_Z, Scene, sun_of};
use super::*;

const MINUS_X: Face = Face {
    name: "-x",
    camera: [0.0; 3],
    aim: [0.0; 3],
    normal: [-1.0, 0.0, 0.0],
    right: [0.0, 0.0, 1.0],
    up: [0.0, 1.0, 0.0],
};

const PLUS_Y: Face = Face {
    name: "+y",
    camera: [0.0; 3],
    aim: [0.0; 3],
    normal: [0.0, 1.0, 0.0],
    right: [1.0, 0.0, 0.0],
    up: [0.0, 0.0, -1.0],
};

/// Where the picture's ridge is: the side it is on, which is the side the sun is on.
#[derive(Clone, Copy, Debug)]
enum Side {
    Right,
    Left,
    Top,
    Bottom,
}

impl Side {
    const ALL: [Side; 4] = [Side::Right, Side::Left, Side::Top, Side::Bottom];

    /// The direction on the face toward the ridge.
    fn toward(self, face: &Face) -> [f64; 3] {
        match self {
            Side::Right => face.right,
            Side::Left => face.right.map(|part| -part),
            Side::Top => face.up,
            Side::Bottom => face.up.map(|part| -part),
        }
    }

    /// Whether the pixel at `(column, row)` is on the ridge (the two rows or columns at this side).
    fn ridge(self, column: usize, row: usize) -> bool {
        match self {
            Side::Right => column >= PICTURE_PIXELS - 2,
            Side::Left => column < 2,
            Side::Top => row < 2,
            Side::Bottom => row >= PICTURE_PIXELS - 2,
        }
    }

    /// The pixel in the middle of its row or column of the picture that is `steps` pixels from the ridge's edge: 0 is the one on the ridge's edge, 1 the one next to the ridge.
    fn pixel_from_ridge(self, steps: usize) -> (usize, usize) {
        let middle = PICTURE_PIXELS / 2;
        match self {
            Side::Right => (PICTURE_PIXELS - 2 - steps, middle),
            Side::Left => (1 + steps, middle),
            Side::Top => (middle, 1 + steps),
            Side::Bottom => (middle, PICTURE_PIXELS - 2 - steps),
        }
    }
}

/// A normal picture with no tilt, no occlusion, and the heights `height(column, row)`.
fn heights(height: impl Fn(usize, usize) -> u8) -> Vec<u8> {
    (0..PICTURE_PIXELS * PICTURE_PIXELS)
        .flat_map(|index| {
            let (column, row) = (index % PICTURE_PIXELS, index / PICTURE_PIXELS);
            [128, 128, 255, height(column, row)]
        })
        .collect()
}

fn ridge_picture(side: Side) -> Vec<u8> {
    heights(|column, row| if side.ridge(column, row) { 255 } else { 0 })
}

/// How the scene is drawn: with the walk of the sun's ray or not, and the pictures drawn as far as `picture_distance`.
#[derive(Clone, Copy)]
struct Drawn {
    traced: bool,
    picture_distance: f32,
}

const NEAR: Drawn = Drawn {
    traced: false,
    picture_distance: 100.0,
};

/// A face of the block, on an adapter, to take pictures of.
struct Taker<'a> {
    context: &'a GraphicsContext,
    name: &'a str,
    face: &'a Face,
}

impl Taker<'_> {
    /// A face seen straight on and so narrow that the middle of the picture is the middle of the pixel `(column, row)` of its picture (rows count from the top).
    fn looking_at(&self, column: usize, row: usize) -> Face {
        let face = self.face;
        let across = (column as f64 + 0.5) / PICTURE_PIXELS as f64 - 0.5;
        let down = (row as f64 + 0.5) / PICTURE_PIXELS as f64 - 0.5;
        let point: [f64; 3] = std::array::from_fn(|axis| {
            16.5 + face.normal[axis] * 0.5 + face.right[axis] * across - face.up[axis] * down
        });
        Face {
            name: face.name,
            camera: std::array::from_fn(|axis| point[axis] + face.normal[axis] * 13.0),
            aim: point,
            normal: face.normal,
            right: face.right,
            up: face.up,
        }
    }

    /// How bright the pixel `(column, row)` of the face is with the normal picture `picture` (or none), in a sun in direction `sun`.
    fn lit(
        &self,
        pixel: (usize, usize),
        picture: Option<&[u8]>,
        sun: [f64; 3],
        drawn: Drawn,
        what: &str,
    ) -> f64 {
        let looked = self.looking_at(pixel.0, pixel.1);
        Scene {
            traced: drawn.traced,
            picture_distance: drawn.picture_distance,
            ..Scene::new(&looked, picture, sun)
        }
        .brightness(
            self.context,
            self.name,
            &format!("relief_{}_{what}", self.face.name),
        )
    }
}

fn check_every_side(traced: bool) {
    let drawn = Drawn { traced, ..NEAR };
    for context in contexts() {
        let name = adapter_name(&context);
        for face in [&PLUS_Z, &MINUS_X, &PLUS_Y] {
            let taker = Taker {
                context: &context,
                name: &name,
                face,
            };
            for side in Side::ALL {
                let sun = sun_of(face, side.toward(face));
                let (ridge, no_relief) = (ridge_picture(side), heights(|_, _| 255));
                let next_to_it = side.pixel_from_ridge(1);
                let open = taker.lit(next_to_it, Some(&no_relief), sun, drawn, "open");
                assert!(
                    open > 0.2,
                    "{name}: face {} ridge {side:?}: the face with no relief is lit {open:.3}, which is too little to say anything by",
                    face.name
                );
                let shaded = taker.lit(next_to_it, Some(&ridge), sun, drawn, "shaded");
                assert!(
                    shaded < 0.05 * open,
                    "{name}: face {} ridge {side:?}: the pixel next to the ridge is lit {shaded:.3} of {open:.3}, wanted none",
                    face.name
                );
                // The pixel 0.1875 from the ridge, past where the ray has cleared the top, and the ridge itself (the top: nothing is higher).
                for (pixel, what) in [
                    (side.pixel_from_ridge(2), "clear"),
                    (side.pixel_from_ridge(0), "edge"),
                ] {
                    let in_the_open = taker.lit(pixel, Some(&ridge), sun, drawn, what);
                    let wanted = taker.lit(pixel, Some(&no_relief), sun, drawn, what);
                    assert!(
                        (in_the_open / wanted - 1.0).abs() < 0.02,
                        "{name}: face {} ridge {side:?}: the pixel {what} of the ridge is lit {in_the_open:.3}, wanted {wanted:.3}",
                        face.name
                    );
                }
            }
        }
    }
}

#[test]
fn a_pixel_the_ridge_of_its_own_picture_stands_over_toward_the_sun_is_in_its_shade_on_any_face_and_from_any_side()
 {
    check_every_side(false);
}

#[test]
fn the_same_holds_in_the_shader_that_walks_the_suns_ray() {
    check_every_side(true);
}

#[test]
fn the_shade_is_not_there_when_the_sun_is_overhead_the_picture_is_far_or_the_relief_is_flat_or_none()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        let face = &PLUS_Z;
        let taker = Taker {
            context: &context,
            name: &name,
            face,
        };
        let side = Side::Right;
        let pixel = side.pixel_from_ridge(1);
        let ridge = ridge_picture(side);
        let flat_low = heights(|_, _| 0);
        let no_relief = heights(|_, _| 255);
        let grazing = sun_of(face, side.toward(face));
        // The sun along the normal: its ray goes straight up and clears every ridge.
        let overhead = taker.lit(pixel, Some(&ridge), face.normal, NEAR, "overhead");
        let overhead_open = taker.lit(pixel, Some(&no_relief), face.normal, NEAR, "overhead_open");
        assert!(
            (overhead / overhead_open - 1.0).abs() < 0.02,
            "{name}: with the sun along the normal the ridge's shade shows: {overhead:.3} over {overhead_open:.3}"
        );
        // Every pixel as low as the others: nothing is higher, whatever the sun.
        let open = taker.lit(pixel, Some(&no_relief), grazing, NEAR, "flat_open");
        let low = taker.lit(pixel, Some(&flat_low), grazing, NEAR, "flat_low");
        assert!(
            (low / open - 1.0).abs() < 0.02,
            "{name}: a picture of one low height has shade: {low:.3} over {open:.3}"
        );
        // A picture with no normal companion has none either.
        let none = taker.lit(pixel, None, grazing, NEAR, "none");
        assert!(
            (none / open - 1.0).abs() < 0.02,
            "{name}: a picture with no normal companion has shade: {none:.3} over {open:.3}"
        );
        // Beyond the picture distance (here 6 voxels, and the camera is 13 from the face) the face is flat and so is its shade.
        let far = Drawn {
            picture_distance: 6.0,
            ..NEAR
        };
        let far_ridge = taker.lit(pixel, Some(&ridge), grazing, far, "far");
        let far_open = taker.lit(pixel, Some(&no_relief), grazing, far, "far_open");
        assert!(
            (far_ridge / far_open - 1.0).abs() < 0.02,
            "{name}: beyond the picture distance the ridge's shade shows: {far_ridge:.3} over {far_open:.3}"
        );
        // And the shade did show where the picture is drawn.
        let near = taker.lit(pixel, Some(&ridge), grazing, NEAR, "near");
        assert!(
            near < 0.05 * open,
            "{name}: the ridge's shade shows nothing: {near:.3} against {open:.3}"
        );
    }
}
