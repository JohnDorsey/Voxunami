//! The geometry of shadow maps: where the sun's view of the world is drawn, and how a place finds its shadow in it.
//!
//! The sun's light is parallel, so a shadow map is a picture of the world seen from the sun, drawn without perspective: for each texel it keeps the surface nearest to the sun.
//! A place is in shadow if something is nearer to the sun than it is in its texel. One map is not enough, because a texel must be about as wide as a pixel of the picture,
//! and the picture shows things from a hand's breadth to the edge of the world: so there are **cascades**, maps of the same number of texels that cover squares of widths
//! doubling (or quadrupling, or growing eightfold) from one to the next, centred on the camera. The cascade a place looks in is the finest that holds it.
//!
//! Nothing here assumes a size of world. This is on purpose (the owner will make mountains of thousands to millions of voxels, and the planets are far larger):
//!
//! * The cascades are measured from the camera and from nothing else; every number is relative to the camera, as the renderer's positions are. How many there are, and how
//!   they grow, comes from how far the picture is drawn ([`half_widths`]), so the cost grows with the logarithm of the distance and not with the distance.
//! * A map is kept while the camera stays near the middle of it ([`SLACK`]) and the sun near where it was ([`SUN_DRIFT`]), and drawn again only when it is not: a cascade that
//!   covers a mountain range has all the terrain of the picture in it, and drawing that in every frame is what the renderer cannot afford. The middle of a map is on a grid
//!   of the world, so a map drawn again lies where it did, and what a place looks up in a kept map is found by moving the camera's place, not by drawing it again.
//! * A map covers the sphere round the camera that the cascade is for ([`Cascade::reach`]), and it is a picture of everything that can be between that sphere and the sun
//!   (the casters, [`Cascade::wants_caster`]), however far toward the sun they are. The depth range of the map is fitted to those casters ([`Cascade::fit`]): depth is measured
//!   from the far side of the sphere, so a place in the sphere has a depth near zero, where a float has the most precision, and a mountain a million voxels toward the sun is
//!   at depth 1 where a float has none to spare, which it does not need.
//! * Only the surfaces that look away from the sun are drawn into the map (the backs of things). A place that the sun lights is never on one, so a lit place cannot shadow
//!   itself, whatever the angle, and the bias is only for the texel's width.
//! * Biases are in texels of the cascade, so a shadow has the same look at every scale.
//!
//! The shader (`voxel.wgsl`, `shadow_of`) does the lookup for every pixel; the functions here are the same arithmetic, so that the renderer fits the maps by the rule the
//! pictures use, and so that a test can hold the two to each other.

use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
use voxel_space::WorldPosition;

/// How many cascades there are at most.
pub const MAXIMUM_CASCADES: usize = 11;

/// How many texels wide and high each cascade is.
pub const CASCADE_RESOLUTION: u32 = 1024;

/// Half the width, in voxels, of the finest cascade: a hand's breadth round the player, at a sixteenth of a voxel to a texel.
pub const FIRST_HALF_WIDTH: f32 = 32.0;

/// How many texels at the edge of a cascade are not used for looking up, so that a lookup and its filter stay inside the map.
pub const EDGE_TEXELS: f32 = 3.0;

/// How far, as a fraction of its half width, the camera may be from the middle of a cascade's map before the map is drawn again: the map is kept while the camera moves
/// about in a box of this size, and the middle of a map is on a grid of this size, so it is at most half of this from the camera when it is drawn. The cascade's reach is
/// what is left of the half width (and the texels at the edge) when the camera is as far from the middle as it may be.
pub const SLACK: f32 = 0.125;

/// The sine of the angle the sun may turn from where it was when a map was drawn before the map is drawn again (a shadow of something a quarter as high as the map is wide
/// moves by about a texel; at the pace of a day of twenty minutes it is a second). Ten times it, and the map is not a little out of date but wrong, and is drawn again
/// at once.
pub const SUN_DRIFT: f32 = 0.005;

/// Where, as a fraction of its reach, a cascade begins to give way to the next: the lookups of the two are mixed over the rest, so that the line between them does not show.
pub const BLEND_FROM: f32 = 0.85;

/// How much, in texels, the place's depth is eased toward the sun, and how much more for every unit of the tangent of the angle between the normal and the sun (a surface
/// seen at a slant changes depth across a texel by the texel's width times the tangent), up to [`SLOPE_BIAS_LIMIT`] of them. (A place is not moved along its normal as well:
/// it was, and the tests did not need it, since only a face turned from the sun by its own body is ever near what the map holds of it, and the slope covers that.)
pub const DEPTH_BIAS_TEXELS: f32 = 1.0;
pub const SLOPE_BIAS_LIMIT: f32 = 4.0;

/// The cosine of the angle between a face and the sun below which the face is, for the sky's light, turned away from the sun and in the shade: it fades in from 0 (edge on)
/// to this, so that a wall does not change from one to the other in a moment as the sun crosses its plane.
pub const FACING_FULL_FROM: f32 = 0.15;

/// The highest above the camera, toward the sun, that anything is drawn into a map. (A tenth of a billion voxels' worth of floats: depth is a fraction of the range, and
/// a range of 2^40 leaves a place at the camera more than 20 bits of depth.) What is higher is the business of the balls (`sun.rs`), not of terrain.
pub const HIGHEST_CASTER: f32 = (1u64 << 40) as f32;

/// What is held in a sphere of less than this many texels of a cascade in radius is not drawn into it: it would be a texel or two at most, and a cascade that covers a
/// mountain range has no use for a tree, nor for the bricks of the ground round the camera (a brick is a sphere of 28 voxels).
pub const SMALLEST_CASTER_TEXELS: f32 = 2.0;

fn smoothstep(edge_start: f32, edge_end: f32, value: f32) -> f32 {
    let fraction = ((value - edge_start) / (edge_end - edge_start)).clamp(0.0, 1.0);
    fraction * fraction * (3.0 - 2.0 * fraction)
}

/// The sun's frame: two directions across the light and the one toward the sun, at right angles to each other, with `right x up = toward_sun`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightBasis {
    pub right: Vec3,
    pub up: Vec3,
    pub toward_sun: Vec3,
}

impl LightBasis {
    /// The frame for a sun in this direction (not necessarily a unit vector; one that is zero or not a number is taken to be straight up): the frame for a sun straight up
    /// (`z` and `x` across the light), turned by the shortest turn that takes up to the sun. It changes continuously as the sun moves, except as the sun goes through
    /// straight down, where there is no shortest turn and the frame spins about the light: the sun is far from straight down by day, and by night (below a flat ground) no
    /// map is drawn. (A frame made from whichever axis of the world the sun is least along jumps by a quarter turn each time two axes change places: Cave's sun does it
    /// twice in a day, and its shadows' texels would change their direction in a moment.)
    pub fn new(sun: Vec3) -> Self {
        let toward_sun = sun.normalize_or(Vec3::Y);
        let turn = Quat::from_rotation_arc(Vec3::Y, toward_sun);
        Self {
            right: turn * Vec3::Z,
            up: turn * Vec3::X,
            toward_sun,
        }
    }

    /// The camera's place across the light, in voxels, as a pair of `f64`s: the map's texels are fixed in the world, and this is how the world is found under the camera.
    pub fn coordinates_of(&self, position: WorldPosition) -> [f64; 2] {
        let brick = position.brick();
        let offset = position.offset();
        let world = [
            brick.x() as f64 * 32.0 + f64::from(offset[0]),
            brick.y() as f64 * 32.0 + f64::from(offset[1]),
            brick.z() as f64 * 32.0 + f64::from(offset[2]),
        ];
        let along = |axis: Vec3| {
            world[0] * f64::from(axis.x)
                + world[1] * f64::from(axis.y)
                + world[2] * f64::from(axis.z)
        };
        [along(self.right), along(self.up)]
    }
}

/// The width of a texel, in voxels, of a cascade whose half width is this: a power of two when the half width is, so that the texels' places in the world are exact.
pub fn texel_of(half_width: f32) -> f32 {
    2.0 * half_width / CASCADE_RESOLUTION as f32
}

/// How far from the camera a place may be to be looked up in a cascade of this half width: the square it covers, less the [`SLACK`] the camera may have moved in, less the
/// texels kept at the edge.
pub fn reach_of(half_width: f32) -> f32 {
    half_width * (1.0 - SLACK) - EDGE_TEXELS * texel_of(half_width)
}

/// The half width at which the cascades stop doubling and begin to grow fourfold, and the one at which they begin to grow eightfold. Up to the first a texel is about a voxel or
/// less, which is as fine as the terrain the player is near, drawn in bricks; beyond it the terrain is drawn in coarse nodes of cells of many voxels, and a coarser map does
/// for it, and the further the coarser, so that a view of planets is within reach.
pub const LAST_DOUBLING_HALF_WIDTH: f32 = 512.0;
pub const LAST_QUADRUPLING_HALF_WIDTH: f32 = 2048.0;

/// The half widths of the cascades for a picture drawn to this distance (the distance where the fog is complete): the finest is [`FIRST_HALF_WIDTH`], each is double the one
/// before up to [`LAST_DOUBLING_HALF_WIDTH`], four times after that up to [`LAST_QUADRUPLING_HALF_WIDTH`] and eight times after that, and the last is the first whose reach
/// is the distance. At most [`MAXIMUM_CASCADES`] (the eleven reach 59 million voxels): further than that the last is the coarsest, and the rest of the picture is not shadowed.
pub fn half_widths(distance: f32) -> Vec<f32> {
    let wanted = if distance.is_nan() { 0.0 } else { distance };
    let mut widths = vec![FIRST_HALF_WIDTH];
    while reach_of(*widths.last().expect("there is a first")) < wanted
        && widths.len() < MAXIMUM_CASCADES
    {
        let last = *widths.last().expect("there is a first");
        widths.push(last * growth_after(last));
    }
    widths
}

/// How many times wider the cascade after one of this half width is.
fn growth_after(half_width: f32) -> f32 {
    if half_width < LAST_DOUBLING_HALF_WIDTH {
        2.0
    } else if half_width < LAST_QUADRUPLING_HALF_WIDTH {
        4.0
    } else {
        8.0
    }
}

/// Which cascade a place at this distance from the camera is looked up in, and how much of the next one is mixed with it: `(index, blend)`, `blend` going from 0 to 1 over
/// the last part of the cascade's reach ([`BLEND_FROM`]). The index of the last cascade means a place beyond all of them, which no shadow reaches: the index is then
/// `reaches.len()`. The shader does the same.
pub fn cascade_for(reaches: &[f32], distance: f32) -> (usize, f32) {
    for (index, &reach) in reaches.iter().enumerate() {
        if distance < reach {
            return (index, smoothstep(BLEND_FROM * reach, reach, distance));
        }
    }
    (reaches.len(), 0.0)
}

/// How much of a face counts as facing the sun, for the sky's light: 0 edge on or turned away, 1 from [`FACING_FULL_FROM`] on. The shader does the same.
pub fn facing(cosine: f32) -> f32 {
    smoothstep(0.0, FACING_FULL_FROM, cosine)
}

/// One cascade for one picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cascade {
    /// Half the width, in voxels, of the square across the light that the map covers.
    pub half_width: f32,
    /// The width of a texel, in voxels.
    pub texel: f32,
    /// How far from the camera a place may be to be looked up here ([`reach_of`]).
    pub reach: f32,
    /// The middle of the map across the light (along `right` and `up`), relative to the camera: the camera's place rounded to a step of the world ([`SLACK`] of the half width, a
    /// whole number of texels), so that a texel is always the same piece of the world and a shadow does not swim when the map is drawn again.
    pub centre: [f32; 2],
    /// The depth (distance toward the sun, from the camera) where depth is 0: below every place a lookup can be for.
    pub back: f32,
    /// The distance from depth 0 to depth 1.
    pub range: f32,
}

impl Cascade {
    /// The cascade of this half width for a camera at this place across the light ([`LightBasis::coordinates_of`]), with the smallest depth range there is
    /// (call [`Cascade::fit`] once the casters are known).
    pub fn new(half_width: f32, camera: [f64; 2]) -> Self {
        let texel = texel_of(half_width);
        let step = f64::from(half_width * SLACK);
        let snapped = |coordinate: f64| {
            let centre = (coordinate / step).round() * step - coordinate;
            (centre as f32).clamp(-(step as f32), step as f32)
        };
        let back = -(half_width + 4.0 * texel);
        Self {
            half_width,
            texel,
            reach: reach_of(half_width),
            centre: [snapped(camera[0]), snapped(camera[1])],
            back,
            range: -2.0 * back,
        }
    }

    /// Whether something whose bounding sphere has this middle (relative to the camera) and radius can put a place the cascade is for in shadow, so that it is drawn into the
    /// map: it is over the square the map covers (with a texel and its own radius to spare), it is not wholly below the map's depth 0, it is not out of the
    /// highest ([`HIGHEST_CASTER`]), and it is not too small to be a texel ([`SMALLEST_CASTER_TEXELS`]). What is not a number, or has no end, is not wanted.
    pub fn wants_caster(&self, basis: &LightBasis, middle: Vec3, radius: f32) -> bool {
        if !radius.is_finite() || radius < SMALLEST_CASTER_TEXELS * self.texel {
            return false;
        }
        let across = self.half_width + radius + self.texel;
        let height = middle.dot(basis.toward_sun);
        (middle.dot(basis.right) - self.centre[0]).abs() <= across
            && (middle.dot(basis.up) - self.centre[1]).abs() <= across
            && height + radius >= self.back
            && height - radius <= HIGHEST_CASTER
    }

    /// Whether the map, drawn for a camera that has since moved by `moved` (the camera's place now relative to where it was), still holds everything within its reach of the
    /// camera: the camera has not gone further than [`SLACK`] of the half width from where it was, along any of the three directions of the light.
    pub fn holds_after_moving(&self, basis: &LightBasis, moved: Vec3) -> bool {
        let slack = self.half_width * SLACK;
        moved.dot(basis.right).abs() <= slack
            && moved.dot(basis.up).abs() <= slack
            && moved.dot(basis.toward_sun).abs() <= slack
    }

    /// The matrix of the map for the camera `moved` from where it was when the map was drawn (what a place relative to the camera now is, is what it was and `moved`).
    pub fn matrix_after_moving(&self, basis: &LightBasis, moved: Vec3) -> Mat4 {
        self.matrix(basis) * Mat4::from_translation(moved)
    }

    /// How high, toward the sun, a wanted caster reaches: the top of its bounding sphere.
    pub fn top_of(basis: &LightBasis, middle: Vec3, radius: f32) -> f32 {
        middle.dot(basis.toward_sun) + radius
    }

    /// Sets the depth range so that it holds everything up to `highest` (the greatest [`Cascade::top_of`] of the casters wanted; none wanted is the map's own height):
    /// depth 0 is the cascade's `back`, and depth 1 is a sixty-fourth past the highest.
    pub fn fit(&mut self, highest: f32) {
        let own_top = self.half_width + 4.0 * self.texel;
        let front = if highest.is_nan() {
            own_top
        } else {
            highest.min(HIGHEST_CASTER).max(own_top)
        };
        self.range = (front - self.back) * (1.0 + 1.0 / 64.0);
    }

    /// Whether any place of the picture is looked up in this cascade, given the nearest and farthest that anything drawn is from the camera and the reach of the cascade before
    /// this one (0 for the first): a cascade that serves nothing is not drawn. A place in the blend zone of the cascade before is looked up here too.
    pub fn serves(&self, previous_reach: f32, nearest: f32, farthest: f32) -> bool {
        nearest < self.reach && farthest > BLEND_FROM * previous_reach
    }

    /// The matrix from a place relative to the camera to the map: `x` and `y` to -1..1 across the square, `z` the depth, 0 to 1 (toward the sun is deeper: the map keeps
    /// the greatest).
    pub fn matrix(&self, basis: &LightBasis) -> Mat4 {
        let across = 1.0 / self.half_width;
        let along = 1.0 / self.range;
        Mat4::from_cols(
            Vec4::new(
                basis.right.x * across,
                basis.up.x * across,
                basis.toward_sun.x * along,
                0.0,
            ),
            Vec4::new(
                basis.right.y * across,
                basis.up.y * across,
                basis.toward_sun.y * along,
                0.0,
            ),
            Vec4::new(
                basis.right.z * across,
                basis.up.z * across,
                basis.toward_sun.z * along,
                0.0,
            ),
            Vec4::new(
                -self.centre[0] * across,
                -self.centre[1] * across,
                -self.back * along,
                1.0,
            ),
        )
    }

    /// Where a place looks up its shadow, for a face whose normal makes an angle with the sun whose cosine is `cosine` (above 0): the texture coordinates in the map and the
    /// depth to compare with what the map holds. The shader's `shadow_in_cascade` does the same.
    pub fn lookup(&self, basis: &LightBasis, position: Vec3, cosine: f32) -> (Vec2, f32) {
        let clip = self.matrix(basis) * position.extend(1.0);
        let sine = (1.0 - cosine * cosine).max(0.0).sqrt();
        let slope = (sine / cosine.max(0.1)).min(SLOPE_BIAS_LIMIT);
        let reference = clip.z + (DEPTH_BIAS_TEXELS + slope) * self.texel / self.range;
        (Vec2::new(clip.x * 0.5 + 0.5, 0.5 - clip.y * 0.5), reference)
    }
}

#[cfg(test)]
mod tests {
    use voxel_space::BrickCoordinate;

    use super::*;

    fn sun_directions() -> Vec<Vec3> {
        let mut directions = vec![
            Vec3::X,
            Vec3::Y,
            Vec3::Z,
            -Vec3::X,
            -Vec3::Y,
            -Vec3::Z,
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::new(0.0, 0.94, 0.342),
            Vec3::new(0.001, 1.0, 0.0),
            Vec3::new(5.0, -0.2, 1.0e-4),
        ];
        // A scatter of directions over the sphere, from a fixed sequence.
        let mut state = 12345u32;
        for _ in 0..200 {
            let mut next = || {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0
            };
            directions.push(Vec3::new(next(), next(), next()));
        }
        directions
    }

    #[test]
    fn the_frame_of_the_light_is_at_right_angles_of_unit_length_and_right_handed_whatever_the_sun()
    {
        for sun in sun_directions() {
            if sun.length() < 1.0e-3 {
                continue;
            }
            let basis = LightBasis::new(sun);
            for axis in [basis.right, basis.up, basis.toward_sun] {
                assert!((axis.length() - 1.0).abs() < 1e-5, "{sun}: {axis}");
            }
            assert!(basis.right.dot(basis.up).abs() < 1e-5, "{sun}");
            assert!(basis.right.dot(basis.toward_sun).abs() < 1e-5, "{sun}");
            assert!(basis.up.dot(basis.toward_sun).abs() < 1e-5, "{sun}");
            assert!(
                (basis.right.cross(basis.up) - basis.toward_sun).length() < 1e-5,
                "{sun}"
            );
            assert!(
                (basis.toward_sun - sun.normalize()).length() < 1e-6,
                "{sun}"
            );
        }
    }

    #[test]
    fn a_sun_that_is_nothing_is_taken_to_be_straight_up_and_the_frame_is_still_a_frame() {
        for sun in [
            Vec3::ZERO,
            Vec3::splat(f32::NAN),
            Vec3::new(f32::INFINITY, 0.0, 0.0),
        ] {
            let basis = LightBasis::new(sun);
            assert_eq!(basis.toward_sun, Vec3::Y);
            assert!((basis.right.length() - 1.0).abs() < 1e-6);
            assert!((basis.right.cross(basis.up) - Vec3::Y).length() < 1e-5);
        }
    }

    #[test]
    fn the_frame_moves_smoothly_with_the_sun_across_the_day_while_it_is_up() {
        // Cave's sun goes round an axis; by day its frame must not jump from one step of a minute to the next. (By night the ground hides the sun and no map is drawn; the
        // sun is then nearest to straight down, where the frame spins about the light, which changes only how the texels lie.)
        let noon = Vec3::new(0.0, 0.94, 0.342);
        let axis = Vec3::new(0.0, -0.342, 0.94);
        let mut previous: Option<LightBasis> = None;
        let mut by_day = 0;
        for minute in 0..1440 {
            let angle = minute as f32 / 1440.0 * std::f32::consts::TAU;
            let sun = noon * libm::cosf(angle) + axis.cross(noon) * libm::sinf(angle);
            if sun.y < 0.0 {
                previous = None;
                continue;
            }
            by_day += 1;
            let basis = LightBasis::new(sun);
            if let Some(last) = previous {
                assert!(
                    (basis.right - last.right).length() < 0.02,
                    "minute {minute}"
                );
                assert!((basis.up - last.up).length() < 0.02, "minute {minute}");
            }
            previous = Some(basis);
        }
        assert!(by_day > 600, "{by_day}");
    }

    #[test]
    fn the_cascades_double_and_then_quadruple_and_then_grow_eightfold_and_reach_as_far_as_the_picture_is_drawn_if_there_are_enough()
     {
        for distance in [
            0.0f32, 1.0, 30.0, 33.0, 64.0, 384.0, 900.0, 4000.0, 5000.0, 1.0e5, 4.0e5, 1.0e6,
            1.0e7, 5.0e7, 6.0e7,
        ] {
            let widths = half_widths(distance);
            assert!(
                !widths.is_empty() && widths.len() <= MAXIMUM_CASCADES,
                "{distance}"
            );
            assert_eq!(widths[0], FIRST_HALF_WIDTH);
            // The last reaches the distance if any can; the one before does not (the last is the first that will do).
            if distance <= 5.0e7 {
                assert!(
                    reach_of(*widths.last().unwrap()) >= distance,
                    "{distance}: {widths:?}"
                );
            }
            if widths.len() > 1 && widths.len() < MAXIMUM_CASCADES {
                assert!(reach_of(widths[widths.len() - 2]) < distance, "{distance}");
            }
            for pair in widths.windows(2) {
                assert_eq!(
                    pair[1] / pair[0],
                    growth_after(pair[0]),
                    "{distance}: {widths:?}"
                );
            }
            // Every texel, and every step the middle of a map moves in, is a power of two, so that their places in the world are exact.
            for width in &widths {
                assert_eq!(texel_of(*width).to_bits() & 0x007F_FFFF, 0, "{width}");
                assert_eq!((width * SLACK).to_bits() & 0x007F_FFFF, 0, "{width}");
            }
        }
    }

    #[test]
    fn small_views_get_few_fine_cascades_and_views_of_mountains_and_planets_get_more_that_grow_faster()
     {
        assert_eq!(half_widths(20.0), [32.0]);
        // The fog of a view of twelve bricks is about 370 voxels: 32, 64, 128, 256, 512.
        assert_eq!(half_widths(370.0), [32.0, 64.0, 128.0, 256.0, 512.0]);
        // Beyond 450 voxels (the reach of the one of 512) they grow by four: 2,048 reaches 1,800.
        assert_eq!(
            half_widths(1_000.0),
            [32.0, 64.0, 128.0, 256.0, 512.0, 2048.0]
        );
        // A view of 400 thousand voxels (a mountain a million voxels high is seen from its foot): the last of nine is 1,048,576 and reaches 917 thousand.
        let mountain = half_widths(4.0e5);
        assert_eq!(mountain.len(), 9);
        assert_eq!(mountain[8], 1_048_576.0);
        assert!(reach_of(mountain[8]) >= 4.0e5);
        // A view of 50 million, as a planet's is from far: all eleven, 32 to 67 million.
        let planet = half_widths(5.0e7);
        assert_eq!(planet.len(), 11);
        assert_eq!(planet[10], 67_108_864.0);
        // Further than the coarsest cascade can reach, the coarsest is the last and there are eleven of them.
        assert_eq!(half_widths(1.0e12).len(), 11);
        assert_eq!(half_widths(f32::INFINITY).len(), 11);
        assert_eq!(half_widths(f32::NAN), [32.0]);
    }

    #[test]
    fn a_place_looks_in_the_finest_cascade_that_holds_it_and_mixes_in_the_next_near_the_edge() {
        let reaches = [31.0, 62.0, 124.0];
        assert_eq!(cascade_for(&reaches, 0.0), (0, 0.0));
        assert_eq!(cascade_for(&reaches, 10.0), (0, 0.0));
        // Past 85% of the reach the next is mixed in, to all of it at the reach.
        let (index, blend) = cascade_for(&reaches, 0.85 * 31.0 + 1.0);
        assert_eq!(index, 0);
        assert!(blend > 0.0 && blend < 0.2, "{blend}");
        let (index, blend) = cascade_for(&reaches, 30.99);
        assert_eq!(index, 0);
        assert!(blend > 0.999, "{blend}");
        assert_eq!(cascade_for(&reaches, 31.0).0, 1);
        assert_eq!(cascade_for(&reaches, 123.0).0, 2);
        // Past the last there is none.
        assert_eq!(cascade_for(&reaches, 124.0), (3, 0.0));
        assert_eq!(cascade_for(&reaches, 1.0e30), (3, 0.0));
        assert_eq!(cascade_for(&[], 5.0), (0, 0.0));
        // The blend only ever grows with the distance.
        let mut last = 0.0;
        for step in 0..=1000 {
            let (_, blend) = cascade_for(&reaches, 31.0 * step as f32 / 1000.0 - 1.0e-4);
            assert!(blend >= last - 1e-6);
            last = blend;
        }
    }

    #[test]
    fn a_face_counts_as_facing_the_sun_from_edge_on_to_a_little_past_it() {
        assert_eq!(facing(-1.0), 0.0);
        assert_eq!(facing(0.0), 0.0);
        assert_eq!(facing(FACING_FULL_FROM), 1.0);
        assert_eq!(facing(1.0), 1.0);
        let half = facing(FACING_FULL_FROM / 2.0);
        assert!((half - 0.5).abs() < 1e-6);
    }

    fn camera_light(x: i128, y: i128, z: i128, offset: [f32; 3], basis: &LightBasis) -> [f64; 2] {
        basis.coordinates_of(WorldPosition::new(BrickCoordinate::new(x, y, z), offset))
    }

    #[test]
    fn the_middle_of_a_map_is_always_a_whole_number_of_steps_from_the_world_origin_wherever_the_camera_is()
     {
        let basis = LightBasis::new(Vec3::new(0.3, 0.8, -0.2));
        for half_width in [32.0f32, 256.0, 4096.0, 1.0e6, 6.7e7] {
            let texel = f64::from(half_width * SLACK);
            let mut position = 0.0f32;
            for step in 0..400 {
                position += 0.0137 * half_width / 32.0;
                let camera = camera_light(
                    12_345 + step as i128,
                    -77,
                    5,
                    [position.rem_euclid(32.0), 3.0, 4.5],
                    &basis,
                );
                let cascade = Cascade::new(half_width, camera);
                // The middle in the world: the camera's place plus the middle relative to it.
                for (axis, (&camera, &centre)) in camera.iter().zip(&cascade.centre).enumerate() {
                    let world = camera + f64::from(centre);
                    let texels = world / texel;
                    assert!(
                        (texels - texels.round()).abs() < 1e-6,
                        "half width {half_width}, axis {axis}: {texels}"
                    );
                    assert!(centre.abs() <= texel as f32 / 2.0 * 1.0001);
                }
            }
        }
    }

    #[test]
    fn moving_the_camera_moves_the_middle_of_the_map_by_a_whole_step_or_not_at_all_and_a_step_is_a_whole_number_of_texels()
     {
        let half_width = 64.0f32;
        let texel = f64::from(half_width * SLACK);
        assert_eq!((texel / f64::from(texel_of(half_width))).fract(), 0.0);
        let mut last: Option<[f64; 2]> = None;
        let mut moves = 0;
        for step in 0..2000 {
            // Walking in the plane across the light, an eighth of a step at a time.
            let camera = [100.0 + step as f64 * texel / 8.0, -40.0];
            let cascade = Cascade::new(half_width, camera);
            let world = [
                camera[0] + f64::from(cascade.centre[0]),
                camera[1] + f64::from(cascade.centre[1]),
            ];
            if let Some(before) = last {
                for axis in 0..2 {
                    let moved = (world[axis] - before[axis]) / texel;
                    assert!((moved - moved.round()).abs() < 1e-6, "{moved}");
                    assert!(moved.abs() <= 1.0 + 1e-6, "{moved}");
                    moves += usize::from(moved.abs() > 0.5);
                }
            }
            last = Some(world);
        }
        // It moved once a step's worth of walking, no more and no less: 2000 / 8 steps.
        assert!((248..=252).contains(&moves), "{moves}");
    }

    #[test]
    fn the_matrix_maps_the_square_to_minus_one_to_one_and_depth_to_0_to_1_with_the_sun_deeper() {
        let basis = LightBasis::new(Vec3::new(0.2, 0.9, 0.4));
        let mut cascade = Cascade::new(64.0, [3.3, -1.7]);
        cascade.fit(5000.0);
        let matrix = cascade.matrix(&basis);
        let map = |position: Vec3| matrix * position.extend(1.0);
        // The middle of the map (at depth 0 of the camera).
        let middle = basis.right * cascade.centre[0] + basis.up * cascade.centre[1];
        let clip = map(middle);
        assert!(
            clip.x.abs() < 1e-5 && clip.y.abs() < 1e-5 && clip.w == 1.0,
            "{clip}"
        );
        // The edges of the square.
        let clip = map(middle + basis.right * 64.0 - basis.up * 64.0);
        assert!(
            (clip.x - 1.0).abs() < 1e-5 && (clip.y + 1.0).abs() < 1e-5,
            "{clip}"
        );
        // Depth: 0 at the back, 1 at the front, and growing toward the sun.
        let at_back = map(basis.toward_sun * cascade.back);
        assert!(at_back.z.abs() < 1e-6, "{at_back}");
        let at_front = map(basis.toward_sun * (cascade.back + cascade.range));
        assert!((at_front.z - 1.0).abs() < 1e-5, "{at_front}");
        let low = map(basis.toward_sun * 10.0).z;
        let high = map(basis.toward_sun * 11.0).z;
        assert!(high > low);
        // Sliding along the light does not change where in the square a place is.
        let here = map(Vec3::new(5.0, 6.0, 7.0));
        let nearer_the_sun = map(Vec3::new(5.0, 6.0, 7.0) + basis.toward_sun * 300.0);
        assert!(
            (here.x - nearer_the_sun.x).abs() < 1e-5 && (here.y - nearer_the_sun.y).abs() < 1e-5
        );
    }

    fn wants(cascade: &Cascade, basis: &LightBasis, middle: Vec3, radius: f32) -> bool {
        cascade.wants_caster(basis, middle, radius)
    }

    #[test]
    fn what_can_shade_the_map_is_wanted_however_far_toward_the_sun_and_what_cannot_is_not() {
        let basis = LightBasis::new(Vec3::new(0.0, 1.0, 0.0));
        let cascade = Cascade::new(32.0, [0.0, 0.0]);
        let across = |x: f32, height: f32, z: f32| {
            basis.right * x + basis.up * z + basis.toward_sun * height
        };
        // On the map, over it, and a mountain a million voxels up the light: all shade.
        assert!(wants(&cascade, &basis, across(0.0, 10.0, 0.0), 16.0));
        assert!(wants(&cascade, &basis, across(0.0, 1.0e6, 0.0), 1000.0));
        assert!(wants(&cascade, &basis, across(5.0, 1.0e11, -5.0), 1.0e9));
        // A node that reaches into the square from the side, and one that is beside it by more than its size.
        assert!(wants(&cascade, &basis, across(100.0, 50.0, 0.0), 80.0));
        assert!(!wants(&cascade, &basis, across(100.0, 50.0, 0.0), 60.0));
        assert!(!wants(&cascade, &basis, across(0.0, 50.0, -5000.0), 100.0));
        // Under the map's back: it cannot shade a place that is over it. Reaching up to it can.
        assert!(!wants(&cascade, &basis, across(0.0, -1000.0, 0.0), 100.0));
        assert!(wants(&cascade, &basis, across(0.0, -100.0, 0.0), 100.0));
        // Higher than anything is drawn.
        assert!(!wants(
            &cascade,
            &basis,
            across(0.0, HIGHEST_CASTER * 4.0, 0.0),
            10.0
        ));
        // A node that takes in the whole map and more, as a planet's does, however big.
        assert!(wants(&cascade, &basis, across(1.0e20, 0.0, 0.0), 1.0e30));
        assert!(wants(&cascade, &basis, Vec3::ZERO, 1.0e36));
        // Too small to be a texel or two of this cascade: not wanted; of a coarser one, a brick is not wanted.
        assert!(!wants(&cascade, &basis, across(0.0, 10.0, 0.0), 0.1));
        assert!(wants(&cascade, &basis, across(0.0, 10.0, 0.0), 0.15));
        let coarse = Cascade::new(1.0e6, [0.0, 0.0]);
        assert!(!wants(&coarse, &basis, across(0.0, 10.0, 0.0), 27.72));
        assert!(wants(
            &coarse,
            &basis,
            across(0.0, 10.0, 0.0),
            27.72 * 1024.0
        ));
        // A brick is drawn into the cascades whose texel is under 14 voxels (the sphere of a brick has a radius of 27.7) and not into a coarser one.
        assert!(wants(
            &Cascade::new(2048.0, [0.0, 0.0]),
            &basis,
            across(0.0, 10.0, 0.0),
            27.72
        ));
        assert!(!wants(
            &Cascade::new(8192.0, [0.0, 0.0]),
            &basis,
            across(0.0, 10.0, 0.0),
            27.72
        ));
        // Not numbers, and without end.
        assert!(!wants(&cascade, &basis, Vec3::splat(f32::NAN), 10.0));
        assert!(!wants(&cascade, &basis, Vec3::ZERO, f32::NAN));
        assert!(!wants(&cascade, &basis, Vec3::ZERO, f32::INFINITY));
        assert!(!wants(&cascade, &basis, Vec3::splat(f32::INFINITY), 10.0));
    }

    #[test]
    fn what_a_map_wants_follows_the_middle_of_the_map_and_the_light() {
        // The sun is low and toward +x: the square across the light is tilted, and what is wanted is what the light passes through, not what is straight up.
        let basis = LightBasis::new(Vec3::new(1.0, 0.2, 0.0));
        let cascade = Cascade::new(32.0, [0.0, 0.0]);
        let along_the_light = basis.toward_sun * 500.0;
        assert!(cascade.wants_caster(&basis, along_the_light, 10.0));
        let straight_up = Vec3::new(0.0, 500.0, 0.0);
        assert!(
            !cascade.wants_caster(&basis, straight_up, 10.0),
            "{}",
            straight_up.dot(basis.right)
        );
    }

    /// The depth of a place in the map, as the matrix gives it, in an `f32`.
    fn depth_of(cascade: &Cascade, basis: &LightBasis, height: f32) -> f32 {
        (cascade.matrix(basis) * (basis.toward_sun * height).extend(1.0)).z
    }

    #[test]
    fn a_range_that_holds_a_mountain_a_million_voxels_up_still_tells_apart_places_a_hundredth_of_a_voxel_apart()
     {
        let basis = LightBasis::new(Vec3::new(0.0, 1.0, 0.0));
        for (name, highest) in [
            ("a hill", 300.0f32),
            ("a mountain", 1.0e6),
            ("ten million", 1.0e7),
            ("at the limit", 1.0e12),
            ("beyond it", 1.0e30),
        ] {
            for half_width in [32.0f32, 2048.0] {
                let mut cascade = Cascade::new(half_width, [0.0, 0.0]);
                cascade.fit(highest);
                let top_depth = depth_of(&cascade, &basis, highest.min(HIGHEST_CASTER));
                assert!(top_depth <= 1.0, "{name}: {top_depth}");
                if highest > half_width + 4.0 * cascade.texel {
                    assert!(top_depth >= 0.98, "{name}: {top_depth}");
                }
                // Places in the map, a hundredth of the texel apart in height: each is deeper than the one before, the order is kept.
                let step = cascade.texel * 0.01;
                let mut last = depth_of(&cascade, &basis, 0.0);
                assert!(last > 0.0 && last < 1.0, "{name}: {last}");
                for index in 1..50 {
                    let depth = depth_of(&cascade, &basis, step * index as f32);
                    assert!(
                        depth > last,
                        "{name}, half width {half_width}: {index}: {depth} after {last}"
                    );
                    last = depth;
                }
            }
        }
    }

    #[test]
    fn the_depth_range_always_holds_the_map_itself_and_never_a_number_that_is_not() {
        let basis = LightBasis::new(Vec3::Y);
        for highest in [f32::NAN, -1.0e30, 0.0, 3.0, f32::INFINITY, f32::MAX] {
            let mut cascade = Cascade::new(32.0, [0.0, 0.0]);
            cascade.fit(highest);
            assert!(
                cascade.range.is_finite() && cascade.range > 0.0,
                "{highest}: {}",
                cascade.range
            );
            // A place at the far edge of the map is inside the depth range.
            let depth = depth_of(&cascade, &basis, cascade.half_width);
            assert!(depth > 0.0 && depth < 1.0, "{highest}: {depth}");
            // The range is at most the highest there is, plus the back and a little.
            assert!(
                cascade.range <= (HIGHEST_CASTER + 200.0) * 1.02,
                "{highest}"
            );
        }
    }

    #[test]
    fn a_cascade_is_served_when_a_place_in_its_reach_or_its_blend_into_it_can_be_drawn() {
        let first = Cascade::new(32.0, [0.0, 0.0]);
        let second = Cascade::new(64.0, [0.0, 0.0]);
        // Something 10 voxels away is in the first, not in the second (only in the blend, from 85% of the first's reach, 23 voxels).
        assert!(first.serves(0.0, 10.0, 10.0));
        assert!(!second.serves(first.reach, 10.0, 10.0));
        assert!(second.serves(first.reach, 10.0, 28.0));
        assert!(second.serves(first.reach, 40.0, 50.0));
        // Something wholly beyond the reach of the second is not its business.
        assert!(!second.serves(first.reach, 60.0, 90.0));
        // A big node that spans all of them serves every one.
        assert!(first.serves(0.0, 0.0, 1.0e9));
        assert!(second.serves(first.reach, 0.0, 1.0e9));
    }

    #[test]
    fn a_place_is_looked_up_where_it_is_and_eased_toward_the_sun_more_the_slantier_the_face() {
        let basis = LightBasis::new(Vec3::Y);
        let mut cascade = Cascade::new(32.0, [0.0, 0.0]);
        cascade.fit(1000.0);
        let place = Vec3::new(3.0, 4.0, 5.0);
        // A face looking at the sun squarely: where it is, eased by the one texel of bias.
        let (squarely_uv, squarely_reference) = cascade.lookup(&basis, place, 1.0);
        let plain = cascade.matrix(&basis) * place.extend(1.0);
        let expected_depth = plain.z + DEPTH_BIAS_TEXELS * cascade.texel / cascade.range;
        assert!(
            (squarely_reference - expected_depth).abs() < 1e-7,
            "{squarely_reference} {expected_depth}"
        );
        assert!((squarely_uv.x - (plain.x * 0.5 + 0.5)).abs() < 1e-6);
        assert!((squarely_uv.y - (0.5 - plain.y * 0.5)).abs() < 1e-6);
        // Slanted by 45 degrees: one more texel of bias (the tangent is 1). Edge on: capped.
        let cosine = std::f32::consts::FRAC_1_SQRT_2;
        let (_, slanted_reference) = cascade.lookup(&basis, place, cosine);
        let bias = slanted_reference - plain.z;
        assert!(
            (bias - 2.0 * cascade.texel / cascade.range).abs() < 1e-6,
            "{bias}"
        );
        let (_, grazing_reference) = cascade.lookup(&basis, place, 0.0);
        let grazing_bias = grazing_reference - plain.z;
        assert!(
            (grazing_bias - (DEPTH_BIAS_TEXELS + SLOPE_BIAS_LIMIT) * cascade.texel / cascade.range)
                .abs()
                < 1e-6,
            "{grazing_bias}"
        );
    }

    #[test]
    fn a_map_that_was_drawn_holds_while_the_camera_stays_within_its_slack_of_where_it_was_in_every_direction_of_the_light()
     {
        let basis = LightBasis::new(Vec3::new(0.3, 0.8, -0.2));
        let cascade = Cascade::new(64.0, [0.0, 0.0]);
        let slack = 64.0 * SLACK;
        for direction in [
            basis.right,
            basis.up,
            basis.toward_sun,
            -basis.right,
            -basis.up,
            -basis.toward_sun,
        ] {
            assert!(cascade.holds_after_moving(&basis, direction * (slack * 0.99)));
            assert!(!cascade.holds_after_moving(&basis, direction * (slack * 1.01)));
        }
        assert!(cascade.holds_after_moving(&basis, Vec3::ZERO));
        // The camera is as far as the slack from the middle of the map in each of the ways at once.
        assert!(cascade.holds_after_moving(
            &basis,
            (basis.right + basis.up + basis.toward_sun) * (slack * 0.99)
        ));
        // Not a number: does not hold.
        assert!(!cascade.holds_after_moving(&basis, Vec3::splat(f32::NAN)));
    }

    #[test]
    fn what_a_place_looks_up_after_the_camera_has_moved_is_what_it_looked_up_before_at_the_place_it_was()
     {
        // A place at a given spot of the world is `relative` to the camera; the camera moves by `moved`, and the place is now `relative - moved` from it. The kept map's matrix
        // after the move takes that to where the old matrix took `relative`.
        let basis = LightBasis::new(Vec3::new(0.2, 0.9, 0.4));
        let mut cascade = Cascade::new(64.0, [3.3, -1.7]);
        cascade.fit(900.0);
        let relative = Vec3::new(10.0, -4.0, 22.0);
        let moved = Vec3::new(3.0, 1.0, -2.5);
        let before = cascade.matrix(&basis) * relative.extend(1.0);
        let after = cascade.matrix_after_moving(&basis, moved) * (relative - moved).extend(1.0);
        assert!(
            (before - after).abs().max_element() < 1e-5,
            "{before} {after}"
        );
    }

    #[test]
    fn the_biases_are_in_texels_so_a_shadow_looks_the_same_at_every_scale() {
        let basis = LightBasis::new(Vec3::new(0.0, 0.9, 0.4));
        let cosine = basis.toward_sun.dot(Vec3::Y);
        let mut reference: Option<(f32, f32)> = None;
        // A place 8 texels from the middle of the map, 5 texels up: at every scale it is the same place in the map, to the bit of an f32.
        for scale in [1.0f32, 16.0, 4096.0, 1.0e6, 1.0e9] {
            let mut cascade = Cascade::new(32.0 * scale, [0.0, 0.0]);
            cascade.fit(cascade.half_width * 40.0);
            let place =
                basis.right * (8.0 * cascade.texel) + basis.toward_sun * (5.0 * cascade.texel);
            let (uv, depth) = cascade.lookup(&basis, place, cosine);
            let relative_depth = (depth - (cascade.matrix(&basis) * Vec3::ZERO.extend(1.0)).z)
                * cascade.range
                / cascade.texel;
            match reference {
                None => reference = Some((uv.x, relative_depth)),
                Some((x, expected)) => {
                    assert!((uv.x - x).abs() < 1e-5, "{scale}: {}", uv.x);
                    assert!(
                        (relative_depth - expected).abs() < 1e-2,
                        "{scale}: {relative_depth} against {expected}"
                    );
                }
            }
        }
    }
}
