//! What is in the sky besides its colour: the stars, and the sun as one of them (a disc with a glow round it).
//!
//! The sky is drawn before the terrain, from nothing but the direction of each pixel, so it is the same from the ground of a planet and from space: what hides it is what is
//! drawn over it (a hill, a planet), and what dims it is the game (`Heavens`: the stars are as bright as the dark of the sky lets them be, the sun's disc and glow as the daylight
//! at the camera lets them be).
//!
//! *The stars* are a fixed field on the sphere, not a picture and not random: the sphere is the six faces of a cube, each a grid of cells (equal in angle, to within a tenth),
//! and a cell has a star or not, and where in the cell, how bright and what colour, from a hash of its face and place. The shader (`voxel.wgsl`, `star_colour`) does this for
//! every pixel; [`star_in_cell`] here is the same rule on the CPU, so that a test can aim a camera at a star that is there and one that is not. The field turns with the day:
//! [`Heavens::star_frame`] takes a direction in the world to a direction in the field, and the sun is at one place in the field for good, as the real one is among the real
//! stars on a day.
//!
//! *The sun* is a star like the others, near: a disc of [`SUN_DISC_RADIUS`] radians, with a glow round it that is the air's and so fades in thin air.

use glam::Vec3;
use libm::{atanf, tanf};

/// How many cells there are along an edge of each of the six faces of the field of stars. Cells are 0.0123 radians across (a degree is 1.4 of them).
pub const STAR_CELLS_ACROSS: u32 = 128;

/// How many of the cells of the field have a star, in parts of 256.
pub const STAR_CHANCE: u32 = 14;

/// The angle one cell takes up at the middle of a face, in radians: a face is a quarter turn (a warp makes the cells equal in angle) over its cells.
pub const STAR_CELL_ANGLE: f32 = std::f32::consts::FRAC_PI_2 / STAR_CELLS_ACROSS as f32;

/// The angular radius of the sun's disc, in radians. The real sun's is 0.0047; this is four times that, a number of mine, so that the sun is a disc and not a speck in a window
/// of a thousand pixels (it is 20 pixels across in one that is 1280 wide at the usual field of view).
pub const SUN_DISC_RADIUS: f32 = 0.02;

/// The sky that is not a colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Heavens {
    /// How brightly the stars shine, 0 to 1: 0 is no stars, and the sky is as it was.
    pub star_light: f32,
    /// The rows are the axes of the field of stars as directions in the world: a direction `d` of the world is `(row0 . d, row1 . d, row2 . d)` in the field. The identity is a
    /// field that does not turn.
    pub star_frame: [[f32; 3]; 3],
    /// The colour of the sun's disc, as linear light. Black is no sun in the sky (and no glow round it).
    pub sun_color: [f32; 3],
    /// How bright the glow round the sun is, 0 to 1: the air's, so it is less where there is less air.
    pub sun_glow: f32,
}

impl Default for Heavens {
    /// An empty sky: no stars, no sun in it.
    fn default() -> Self {
        Self {
            star_light: 0.0,
            star_frame: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            sun_color: [0.0; 3],
            sun_glow: 0.0,
        }
    }
}

impl Heavens {
    /// Whether anything is drawn in the sky: the renderer draws the pass for it only then, and a picture with nothing in the sky is as it was.
    pub fn is_empty(&self) -> bool {
        self.star_light <= 0.0 && self.sun_color.iter().all(|part| *part <= 0.0)
    }
}

/// A cell of the field of stars: the face of the cube (0 to 5: `2 * axis + 1` if the direction is along the axis, `2 * axis` if it is against) and the place on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StarCell {
    pub face: u32,
    pub across: u32,
    pub up: u32,
}

/// A star, as the field has it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Star {
    /// The unit direction to the star, in the field.
    pub direction: Vec3,
    /// How bright it is, 0.2 to 1 (a few are bright and most are faint).
    pub brightness: f32,
    /// Its linear colour at full brightness, from blue-white to orange.
    pub color: [f32; 3],
}

/// A hash of 32 bits (`lowbias32`): the same on every machine and in `voxel.wgsl` (`star_hash`).
pub fn star_hash(value: u32) -> u32 {
    let mut value = value;
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^ (value >> 16)
}

/// The two axes a face's cells are laid out along, as the shader has them: the ones after the face's own axis, round.
fn across_axes(axis: usize) -> (usize, usize) {
    ((axis + 1) % 3, (axis + 2) % 3)
}

/// The cell a direction (any length, not zero) is in, and where in it, as fractions from 0 to 1 along the cell's two axes.
pub fn star_cell_of(direction: Vec3) -> (StarCell, [f32; 2]) {
    let magnitude = direction.abs();
    let axis = if magnitude.x >= magnitude.y && magnitude.x >= magnitude.z {
        0
    } else if magnitude.y >= magnitude.z {
        1
    } else {
        2
    };
    let (first, second) = across_axes(axis);
    let major = magnitude[axis];
    let face = 2 * axis as u32 + u32::from(direction[axis] > 0.0);
    // The warp that makes cells equal in angle: the tangent plane's coordinate (-1 to 1) is a tangent of an angle (-45 to 45 degrees).
    let warp = |part: f32| atanf(part / major) * (4.0 / std::f32::consts::PI);
    let places = [warp(direction[first]), warp(direction[second])]
        .map(|part| (part * 0.5 + 0.5).clamp(0.0, 0.999_999) * STAR_CELLS_ACROSS as f32);
    (
        StarCell {
            face,
            across: places[0] as u32,
            up: places[1] as u32,
        },
        places.map(f32::fract),
    )
}

/// The star in a cell, if it has one.
pub fn star_in_cell(cell: StarCell) -> Option<Star> {
    let seed = star_hash(
        star_hash(cell.across + STAR_CELLS_ACROSS * cell.up) ^ cell.face.wrapping_mul(0x9e37_79b9),
    );
    if seed & 0xFF >= STAR_CHANCE {
        return None;
    }
    let other = star_hash(seed);
    let byte = |hash: u32, shift: u32| ((hash >> shift) & 0xFF) as f32 / 255.0;
    let brightness = 0.2 + 0.8 * byte(seed, 8) * byte(seed, 8) * byte(seed, 8);
    // Where in the cell, kept off its edges so that the dot is never cut by the next cell.
    let place = [0.2 + 0.6 * byte(seed, 16), 0.2 + 0.6 * byte(seed, 24)];
    let tint = byte(other, 0);
    let color = [0.7 + 0.3 * tint, 0.8 + 0.05 * tint, 1.0 - 0.35 * tint];
    let axis = (cell.face / 2) as usize;
    let (first, second) = across_axes(axis);
    let unwarp = |cell_number: u32, within: f32| {
        let across = (cell_number as f32 + within) / STAR_CELLS_ACROSS as f32 * 2.0 - 1.0;
        tanf(across * std::f32::consts::FRAC_PI_4)
    };
    let mut direction = Vec3::ZERO;
    direction[axis] = if cell.face % 2 == 1 { 1.0 } else { -1.0 };
    direction[first] = unwarp(cell.across, place[0]);
    direction[second] = unwarp(cell.up, place[1]);
    Some(Star {
        direction: direction.normalize(),
        brightness,
        color,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_cell() -> impl Iterator<Item = StarCell> {
        (0..6).flat_map(|face| {
            (0..STAR_CELLS_ACROSS).flat_map(move |up| {
                (0..STAR_CELLS_ACROSS).map(move |across| StarCell { face, across, up })
            })
        })
    }

    #[test]
    fn the_hash_is_the_one_the_shader_has_and_does_not_change() {
        // (Worked out apart, from the definition of `lowbias32`: a change here changes every star of every world.)
        assert_eq!(star_hash(0), 0);
        assert_eq!(star_hash(1), 0x6889_90c0);
        assert_eq!(star_hash(0xdead_beef), 0xe628_c683);
    }

    #[test]
    fn about_one_cell_in_eighteen_has_a_star_and_they_are_mostly_faint() {
        let stars: Vec<Star> = every_cell().filter_map(star_in_cell).collect();
        let cells = 6 * (STAR_CELLS_ACROSS * STAR_CELLS_ACROSS) as usize;
        let share = stars.len() as f32 / cells as f32;
        assert!(
            (share - STAR_CHANCE as f32 / 256.0).abs() < 0.005,
            "{} stars in {cells} cells",
            stars.len()
        );
        // Several thousand stars in all, as a clear sky has to the eye.
        assert!((4_000..7_000).contains(&stars.len()), "{}", stars.len());
        let faint = stars.iter().filter(|star| star.brightness < 0.5).count();
        let bright = stars.iter().filter(|star| star.brightness > 0.9).count();
        // (Brightness is 0.2 plus 0.8 of a cube of a byte: under a half for 72 stars in a hundred.)
        assert!(faint * 100 > stars.len() * 65, "{faint} faint");
        assert!((10..stars.len() / 10).contains(&bright), "{bright} bright");
        for star in &stars {
            assert!((0.2..=1.0).contains(&star.brightness));
            assert!((star.direction.length() - 1.0).abs() < 1e-5);
            assert!(star.color.iter().all(|part| (0.6..=1.0).contains(part)));
        }
    }

    #[test]
    fn a_star_is_in_the_cell_it_was_made_for_and_in_the_middle_of_it() {
        let mut checked = 0;
        for cell in every_cell() {
            let Some(star) = star_in_cell(cell) else {
                continue;
            };
            let (found, within) = star_cell_of(star.direction);
            assert_eq!(found, cell, "{star:?}");
            // (Kept off the edges: the dot is never cut by the next cell.)
            assert!(
                within.iter().all(|part| (0.15..=0.85).contains(part)),
                "{cell:?}: {within:?}"
            );
            checked += 1;
        }
        assert!(checked > 4_000);
    }

    #[test]
    fn the_cells_are_equal_in_angle_to_within_a_tenth_from_the_middle_of_a_face_to_its_corner() {
        // The angle between the middles of two cells that are next to each other, in the middle and in the corner of a face.
        let direction = |across: u32, up: u32| {
            let unwarp = |cell: u32| {
                tanf(
                    ((cell as f32 + 0.5) / STAR_CELLS_ACROSS as f32 * 2.0 - 1.0)
                        * std::f32::consts::FRAC_PI_4,
                )
            };
            Vec3::new(1.0, unwarp(across), unwarp(up)).normalize()
        };
        let angle = |first: Vec3, second: Vec3| libm::acosf(first.dot(second).clamp(-1.0, 1.0));
        let middle = STAR_CELLS_ACROSS / 2;
        let in_the_middle = angle(direction(middle, middle), direction(middle + 1, middle));
        let in_the_corner = angle(direction(0, 0), direction(1, 0));
        for angle in [in_the_middle, in_the_corner] {
            assert!(
                (angle / STAR_CELL_ANGLE - 1.0).abs() < 0.1,
                "{angle} against {STAR_CELL_ANGLE}"
            );
        }
    }

    #[test]
    fn the_field_has_stars_on_all_six_faces_and_none_is_denser_than_another() {
        let mut per_face = [0usize; 6];
        for cell in every_cell() {
            if star_in_cell(cell).is_some() {
                per_face[cell.face as usize] += 1;
            }
        }
        let mean = per_face.iter().sum::<usize>() as f32 / 6.0;
        for (face, count) in per_face.iter().enumerate() {
            assert!(
                (*count as f32 / mean - 1.0).abs() < 0.1,
                "face {face}: {count}"
            );
        }
    }

    #[test]
    fn an_empty_sky_is_the_default_and_a_sky_with_a_star_or_a_sun_is_not() {
        assert!(Heavens::default().is_empty());
        assert!(
            !Heavens {
                star_light: 0.1,
                ..Heavens::default()
            }
            .is_empty()
        );
        assert!(
            !Heavens {
                sun_color: [0.0, 0.0, 0.5],
                ..Heavens::default()
            }
            .is_empty()
        );
        // A glow with no sun is nothing to draw.
        assert!(
            Heavens {
                sun_glow: 1.0,
                ..Heavens::default()
            }
            .is_empty()
        );
    }
}
