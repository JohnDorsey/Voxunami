//! What hides the sun from a place: the balls of planets and moons, and the ground of a level that is flat.
//!
//! The shader (`voxel.wgsl`, `sun_visibility`) does this for every pixel, and [`sun_visibility`] here is the same arithmetic on the CPU, so that the camera's own daylight (the
//! colour of the sky) is worked out by the rule the pictures are, and so that a test can hold the two to each other.
//!
//! A ball hides the sun from a place when the sun is in the disc the ball takes up in the sky of that place. The rule is in angles: the ball's half-width seen from the
//! place is `asin(radius / distance)`, the sun is at an angle from the ball's middle, and the sun is visible in proportion to how far that angle is past the ball's edge, over
//! a width on each side (a smoothstep), so that a terminator is a soft edge and not a step: the width is that of the sun's disc, [`SUN_DISC_SOFTNESS`], and, for a place
//! that is near the ball as the ground of a planet is, the twilight of its sky added to it ([`TWILIGHT_SOFTNESS`], fading with the eighth power of the distance in radii).
//! Every ball that hides the sun hides it, so the shares multiply.
//! The balls are conservative: a ball here is one that is all ground (a planet's core, [`terrain::SolidBall`]), so a place that is called lit may be in the shadow of a hill and
//! never the other way about.
//!
//! The ground of a flat level hides the sun when it is below the horizon, whatever the place (`y` is up): the same soft edge, in the sun's height.

use glam::Vec3;
use voxel_space::WorldPosition;

/// How wide, in radians, the soft edge of a shadow is on each side of where the sun's centre touches the edge of what hides it, for a ball far from the place: about the radius
/// of the real sun's disc (a hundredth of a radian across), a little more.
pub const SUN_DISC_SOFTNESS: f32 = 0.005;

/// What is added to that, in radians, for a place on the surface of a ball (on its ground), where the sun goes down through the air and the light fades over some degrees
/// (about 6, as at a real sunset's twilight). It is this times `(radius / distance)^8`, so it is nothing in the sky of a place a radius or more above the ball.
pub const TWILIGHT_SOFTNESS: f32 = 0.1;

/// The most the arcsine of a ball's share of the sky is asked about, `radius / distance`: a place on the ball or inside it is as near to its surface as this. (The answer of
/// `asin` at exactly 1 is not a number on the integrated AMD GPU of the development machine in DX12, found when the faces of a planet's far nodes, which are inside its core,
/// came out black there; the difference to the angle is a thousandth of a radian.)
pub const MOST_OF_A_BALL: f32 = 0.999999;

/// How many balls the shader looks at. The nearest to the camera are kept.
pub const MAXIMUM_SUN_BALLS: usize = 8;

/// Something that can be between a place and the sun.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SunOccluder {
    /// A ball that is ground all through: a planet's core.
    Ball {
        centre: WorldPosition,
        /// In voxels.
        radius: f32,
    },
    /// The ground of a level that is flat and level, with `y` up: the sun is hidden whenever it is below the horizon.
    FlatGround,
}

/// A ball as the shader has it: its middle relative to the camera, in voxels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunBall {
    pub centre: Vec3,
    pub radius: f32,
}

fn smoothstep(edge_start: f32, edge_end: f32, value: f32) -> f32 {
    let fraction = ((value - edge_start) / (edge_end - edge_start)).clamp(0.0, 1.0);
    fraction * fraction * (3.0 - 2.0 * fraction)
}

/// The vector's length, worked out from the vector scaled by its largest part, because the square of a part of 10^20 or more does not fit an `f32`. Returns the length and
/// the unit vector; a zero vector has length 0 and no direction (zero).
fn length_and_direction(vector: Vec3) -> (f32, Vec3) {
    let largest = vector.abs().max_element();
    if largest == 0.0 {
        return (0.0, Vec3::ZERO);
    }
    let scaled = vector / largest;
    let scaled_length = scaled.length();
    (largest * scaled_length, scaled / scaled_length)
}

/// How far over the surface of a ball, in radii of it, a face is still turned all the way to the way out of the ball (see [`sunlight`]), and how far it is no longer turned.
pub const TURNED_UP_TO: f32 = 0.01;
pub const TURNED_NO_MORE_FROM: f32 = 0.03;

/// What the sun does at a place: how much of it reaches the place, and the normal of a face there that its light is worked out by.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sunlight {
    /// 1 for all of the sun, 0 for none, and in between at a soft edge.
    pub visible: f32,
    /// The normal the cosine of the light is taken of: the face's own, turned towards the way out of the planet it is at the ground of, if it is. The ground of a planet made of
    /// voxels is faces that look along the axes of its cube, whatever way the ground goes, so far from the pole the tops of the hills look 45 degrees from the way up and
    /// the planet, seen from far, is patches of three kinds of light. A face at the ground (over it by a hundredth of the radius of the ball, or less, and anywhere inside)
    /// is lit as if it looked half-way between its own way and the way out of the ball; the turn is gone by three hundredths of the radius over it, so a ship in the air over
    /// a planet is not turned. It is one more step towards a lit globe, and the planet's own shadows (a hill's) are for shadow maps.
    pub normal: Vec3,
}

/// How much of the sun reaches `position` (relative to the camera, in voxels) when the sun is in the direction `sun` (a unit vector) and these things may be in the way:
/// 1 for all of it, 0 for none, and in between at a soft edge. The shader's `sunlight` does the same.
pub fn sun_visibility(position: Vec3, sun: Vec3, balls: &[SunBall], flat_ground: bool) -> f32 {
    sunlight(position, Vec3::Y, sun, balls, flat_ground).visible
}

/// The sun's light at `position` for a face that looks along `normal` (a unit vector): see [`Sunlight`]. The shader's `sunlight` does the same.
pub fn sunlight(
    position: Vec3,
    normal: Vec3,
    sun: Vec3,
    balls: &[SunBall],
    flat_ground: bool,
) -> Sunlight {
    let mut visible = 1.0;
    if flat_ground {
        let softness = SUN_DISC_SOFTNESS + TWILIGHT_SOFTNESS;
        visible = smoothstep(-softness, softness, sun.y);
    }
    // The ball whose surface the place is nearest (a place inside one is nearest it), how far over its surface the place is, in radii, and the way out of it there.
    let mut nearest: Option<(f32, Vec3)> = None;
    for ball in balls.iter().take(MAXIMUM_SUN_BALLS) {
        let (true_distance, direction) = length_and_direction(ball.centre - position);
        if true_distance == 0.0 {
            return Sunlight {
                visible: 0.0,
                normal,
            };
        }
        let over = (true_distance - ball.radius) / ball.radius;
        if nearest.is_none_or(|(least, _)| over < least) {
            nearest = Some((over, -direction));
        }
        // (A place inside the ball is taken to be on its surface, in its direction: see the shader.)
        let distance = true_distance.max(ball.radius);
        let share = ball.radius / distance;
        let reach = libm::asinf(share.min(MOST_OF_A_BALL));
        let separation = libm::acosf(direction.dot(sun).clamp(-1.0, 1.0));
        let share_squared = share * share;
        let share_to_the_fourth = share_squared * share_squared;
        let softness =
            SUN_DISC_SOFTNESS + TWILIGHT_SOFTNESS * share_to_the_fourth * share_to_the_fourth;
        // A ball that takes up less of the sky than the soft edge is wide hides only part of the sun's disc even at its middle, by the share of the area.
        let coverage = (reach / softness).min(1.0);
        let hidden = coverage
            * coverage
            * (1.0 - smoothstep(reach - softness, reach + softness, separation));
        visible *= 1.0 - hidden;
    }
    let normal = match nearest {
        Some((over, way_out)) => {
            let turn = 1.0 - smoothstep(TURNED_UP_TO, TURNED_NO_MORE_FROM, over);
            // (A face that looks exactly against the way out would be turned to nothing: it keeps its own.)
            let turned = normal + way_out * turn;
            if turned.length() > 1e-3 {
                turned.normalize()
            } else {
                normal
            }
        }
        None => normal,
    };
    Sunlight { visible, normal }
}

/// The balls of these occluders relative to a camera, the nearest [`MAXIMUM_SUN_BALLS`] of them, and whether any is a flat ground.
pub fn balls_and_ground(occluders: &[SunOccluder], camera: WorldPosition) -> (Vec<SunBall>, bool) {
    let mut balls: Vec<SunBall> = occluders
        .iter()
        .filter_map(|occluder| match *occluder {
            SunOccluder::Ball { centre, radius } => Some(SunBall {
                centre: Vec3::from_array(centre.relative_to(camera)),
                radius,
            }),
            SunOccluder::FlatGround => None,
        })
        .collect();
    // (Compared by the largest part, which is enough to keep the nearest, and does not overflow.)
    balls.sort_by(|first, second| {
        let nearness = |ball: &SunBall| (ball.centre.abs().max_element() - ball.radius).max(0.0);
        nearness(first).total_cmp(&nearness(second))
    });
    balls.truncate(MAXIMUM_SUN_BALLS);
    let flat = occluders.contains(&SunOccluder::FlatGround);
    (balls, flat)
}

/// How much of the sun reaches the camera, by the rule the pictures use.
pub fn daylight_at(camera: WorldPosition, sun: Vec3, occluders: &[SunOccluder]) -> f32 {
    let (balls, flat_ground) = balls_and_ground(occluders, camera);
    sun_visibility(Vec3::ZERO, sun, &balls, flat_ground)
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxel_space::BrickCoordinate;

    fn ball(centre: [f32; 3], radius: f32) -> SunBall {
        SunBall {
            centre: Vec3::from_array(centre),
            radius,
        }
    }

    /// A place a thousandth of a voxel over the surface of a ball of radius 1000 centred below the origin (a place exactly on it is inside it), where the sun is `elevation`
    /// radians over the horizon (the sun is in the plane of the place's up and the x axis; up is +y).
    fn surface_visibility(elevation: f32) -> f32 {
        let sun = Vec3::new(libm::cosf(elevation), libm::sinf(elevation), 0.0);
        sun_visibility(
            Vec3::ZERO,
            sun,
            &[ball([0.0, -1000.001, 0.0], 1000.0)],
            false,
        )
    }

    /// The effective normal of a face of a planet: a place `height` over the surface of a ball of radius 1,000 centred below the origin, on the ball's axis (so that the way
    /// out is +y), for a face that looks along `normal`.
    fn effective_normal(height: f32, normal: Vec3) -> Vec3 {
        let ball = ball([0.0, -1000.0, 0.0], 1000.0);
        let place = Vec3::new(0.0, height, 0.0);
        sunlight(place, normal, Vec3::Y, &[ball], false).normal
    }

    #[test]
    fn a_face_at_the_ground_of_a_planet_is_turned_to_the_way_out_of_the_planet_by_half_the_angle_it_is_from_it()
     {
        // A face that looks along +x, a metre over the ground of a planet whose way out here is +y: its normal is the mean of the two, 45 degrees up.
        let turned = effective_normal(1.0, Vec3::X);
        let h = std::f32::consts::FRAC_1_SQRT_2;
        assert!((turned - Vec3::new(h, h, 0.0)).length() < 1e-5, "{turned}");
        // One that already looks the way out is not turned, nor a unit vector made another length.
        assert!((effective_normal(1.0, Vec3::Y) - Vec3::Y).length() < 1e-6);
        // One that looks into the ground (the face of a ceiling of a cave, or a far node's underside) is turned too, and never to nothing.
        let below = effective_normal(1.0, -Vec3::Y);
        assert!(below.length() > 0.99 && below.length() < 1.01, "{below}");
        // Inside the ball, as the faces of far nodes may be, it is the same.
        assert!((effective_normal(-200.0, Vec3::X) - turned).length() < 1e-5);
    }

    #[test]
    fn the_turn_fades_out_over_the_first_kilometres_over_the_ground_so_that_a_ship_above_a_planet_is_not_turned()
     {
        // The ball has a radius of 1,000, so the turn is whole to 10 voxels over it and gone from 30.
        assert!((effective_normal(5.0, Vec3::X) - effective_normal(1.0, Vec3::X)).length() < 1e-5);
        let partly = effective_normal(20.0, Vec3::X);
        assert!(partly.y > 0.05 && partly.y < 0.7, "{partly}");
        assert!((effective_normal(31.0, Vec3::X) - Vec3::X).length() < 1e-6);
        assert!((effective_normal(100_000.0, Vec3::X) - Vec3::X).length() < 1e-6);
        // It only ever grows less the higher the place is.
        let mut last = 2.0;
        for height in 0..40 {
            let along = effective_normal(height as f32, Vec3::X).y;
            assert!(along <= last + 1e-6, "{height}");
            last = along;
        }
    }

    #[test]
    fn the_nearest_ball_turns_a_face_and_the_ground_and_a_sun_with_nothing_in_the_way_do_not() {
        // Two planets: a face on the second is turned by the second.
        let balls = [
            ball([0.0, -1000.0, 0.0], 1000.0),
            ball([5000.0, 0.0, 0.0], 100.0),
        ];
        let place = Vec3::new(5000.0, 100.5, 0.0);
        let turned = sunlight(place, Vec3::X, Vec3::Y, &balls, false).normal;
        let h = std::f32::consts::FRAC_1_SQRT_2;
        assert!((turned - Vec3::new(h, h, 0.0)).length() < 1e-4, "{turned}");
        // With no ball in the world a normal is what it was, and flat ground does not turn it.
        assert_eq!(
            sunlight(place, Vec3::X, Vec3::Y, &[], false).normal,
            Vec3::X
        );
        assert_eq!(sunlight(place, Vec3::X, Vec3::Y, &[], true).normal, Vec3::X);
    }

    #[test]
    fn a_face_looking_at_the_sun_across_a_planets_surface_is_lit_by_the_turned_normal() {
        // The sun is on the horizon of a place on a planet (along +x), and a face looks along +x: by its own normal it is lit fully, by the turned one at 45 degrees.
        let balls = [ball([0.0, -1000.001, 0.0], 1000.0)];
        let light = sunlight(Vec3::ZERO, Vec3::X, Vec3::X, &balls, false);
        let lambert = light.normal.dot(Vec3::X);
        assert!(
            (lambert - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3,
            "{lambert}"
        );
        assert!(
            (light.visible - 0.5).abs() < 0.02,
            "the sun is on the terminator: {}",
            light.visible
        );
    }

    #[test]
    fn nothing_in_the_way_is_all_of_the_sun() {
        assert_eq!(sun_visibility(Vec3::ZERO, Vec3::Y, &[], false), 1.0);
        assert_eq!(
            sun_visibility(
                Vec3::new(1e30, -5.0, 2.0),
                Vec3::new(0.6, 0.0, 0.8),
                &[],
                false
            ),
            1.0
        );
    }

    #[test]
    fn flat_ground_hides_the_sun_below_the_horizon_with_a_soft_edge() {
        let at_height = |height: f32| {
            let sun = Vec3::new(libm::cosf(height), libm::sinf(height), 0.0);
            sun_visibility(Vec3::new(0.0, 12345.0, 0.0), sun, &[], true)
        };
        assert_eq!(at_height(1.0), 1.0);
        assert_eq!(at_height(-1.0), 0.0);
        let edge = (SUN_DISC_SOFTNESS + TWILIGHT_SOFTNESS) * 1.01;
        assert_eq!(at_height(edge), 1.0);
        assert_eq!(at_height(-edge), 0.0);
        let at_the_horizon = at_height(0.0);
        assert!((at_the_horizon - 0.5).abs() < 1e-6, "{at_the_horizon}");
    }

    #[test]
    fn a_ball_hides_the_sun_from_its_night_side_and_not_from_its_day_side() {
        assert_eq!(surface_visibility(1.0), 1.0, "noon");
        assert_eq!(surface_visibility(-1.0), 0.0, "midnight");
        // The terminator, where the sun is on the horizon of a place on the surface, is the middle of the soft edge.
        let at_the_terminator = surface_visibility(0.0);
        assert!(
            (at_the_terminator - 0.5).abs() < 0.02,
            "{at_the_terminator}"
        );
        // The twilight is some degrees wide: a tenth of a radian either side of the terminator is nearly dark or nearly bright.
        assert!(surface_visibility(0.12) > 0.99 && surface_visibility(-0.12) < 0.01);
    }

    #[test]
    fn a_ball_gives_a_sunset_that_only_ever_darkens_as_the_sun_goes_down() {
        let mut last = 2.0;
        for step in 0..=160 {
            let elevation = 0.2 - step as f32 * 0.0025;
            let visible = surface_visibility(elevation);
            assert!(
                visible <= last + 1e-6,
                "the sun at {elevation} is brighter than higher up: {visible} after {last}"
            );
            assert!((0.0..=1.0).contains(&visible), "{visible}");
            last = visible;
        }
        assert_eq!(last, 0.0);
    }

    #[test]
    fn a_place_inside_a_ball_is_lit_as_if_it_were_on_the_balls_surface_in_its_direction() {
        // A face of a far node of a planet may be a little below the ground (a cell is hundreds of voxels across) and so inside the core: it is lit by the terminator of
        // the planet, which is the same as that of the ground over it, and not put in the dark because it is under the ball's skin.
        let balls = [ball([0.0, 0.0, 0.0], 50.0)];
        let inside = Vec3::new(10.0, 10.0, 10.0);
        // The sun on the side of the planet the place is on: lit. On the other: dark.
        assert_eq!(
            sun_visibility(inside, Vec3::new(1.0, 1.0, 1.0).normalize(), &balls, false),
            1.0
        );
        assert_eq!(
            sun_visibility(inside, -Vec3::new(1.0, 1.0, 1.0).normalize(), &balls, false),
            0.0
        );
        // At right angles to the way out from the middle it is the terminator, as on the surface.
        let terminator =
            sun_visibility(inside, Vec3::new(1.0, -1.0, 0.0).normalize(), &balls, false);
        assert!((terminator - 0.5).abs() < 0.02, "{terminator}");
        // The same place just outside the ball is lit alike.
        let outside = Vec3::splat(28.8676);
        for sun in [Vec3::Y, -Vec3::Y, Vec3::new(1.0, -1.0, 0.0).normalize()] {
            let (in_the_ball, on_the_ball) = (
                sun_visibility(inside, sun, &balls, false),
                sun_visibility(outside, sun, &balls, false),
            );
            assert!(
                (in_the_ball - on_the_ball).abs() < 0.2,
                "{sun}: {in_the_ball} and {on_the_ball}"
            );
        }
        // The exact middle has no direction, and no sun is the answer.
        assert_eq!(sun_visibility(Vec3::ZERO, Vec3::Y, &balls, false), 0.0);
    }

    #[test]
    fn a_ball_does_not_reach_across_to_the_sunlit_side_of_what_is_far_from_it() {
        // A place a million times the ball's radius away, with the sun not behind the ball, is in full sun, and the arithmetic is not spoiled by the size of the numbers.
        let balls = [ball([0.0, -1000.0, 0.0], 1000.0)];
        for distance in [1e6f32, 1e12, 1e20, 1e30] {
            let place = Vec3::new(distance, 0.0, 0.0);
            assert_eq!(
                sun_visibility(place, Vec3::new(1.0, 0.0, 0.0), &balls, false),
                1.0,
                "{distance}"
            );
            // And behind the ball as the sun is (the ball is between the place and the sun): hidden, at a distance where the ball takes up angle enough.
        }
        let behind = Vec3::new(5000.0, -1000.0, 0.0);
        let towards_the_ball = Vec3::new(-1.0, 0.0, 0.0);
        assert_eq!(sun_visibility(behind, towards_the_ball, &balls, false), 0.0);
    }

    #[test]
    fn a_ball_far_away_hides_nothing_it_is_not_in_front_of() {
        // A moon a million voxels away that takes up a few hundredths of a radian: the sun beside it is whole, and the sun behind its middle is hidden. (Its edge is the sun's
        // disc wide, not the twilight: the moon has no air.)
        let moon = [ball([1.0e6, 0.0, 0.0], 20_000.0)];
        let beside = Vec3::new(1.0, 0.1, 0.0).normalize();
        assert_eq!(sun_visibility(Vec3::ZERO, beside, &moon, false), 1.0);
        assert_eq!(
            sun_visibility(Vec3::ZERO, Vec3::X, &moon, false),
            0.0,
            "an eclipse"
        );
        // One smaller than the sun's disc is an annular eclipse: some of the sun, never all and never none, and a speck of it is nothing.
        let small = [ball([1.0e6, 0.0, 0.0], 3_000.0)];
        let partly = sun_visibility(Vec3::ZERO, Vec3::X, &small, false);
        assert!((0.5..0.95).contains(&partly), "{partly}");
        let speck = [ball([1.0e30, 0.0, 0.0], 1_000.0)];
        assert!(sun_visibility(Vec3::ZERO, Vec3::X, &speck, false) > 0.999);
    }

    #[test]
    fn balls_and_the_ground_each_take_their_share() {
        let place = Vec3::ZERO;
        let sun = Vec3::new(1.0, 0.0, 0.0);
        let first = ball([0.0, -1000.001, 0.0], 1000.0);
        let alone = sun_visibility(place, sun, &[first], false);
        let with_ground = sun_visibility(place, sun, &[first], true);
        assert!((alone - 0.5).abs() < 0.02, "{alone}");
        assert!((with_ground - alone * 0.5).abs() < 0.001, "{with_ground}");
        // The order of the balls does not matter.
        let second = ball([1.0e5, 0.0, 0.0], 100.0);
        assert_eq!(
            sun_visibility(place, sun, &[first, second], false),
            sun_visibility(place, sun, &[second, first], false)
        );
    }

    #[test]
    fn only_the_nearest_balls_are_kept_and_the_ground_is_noticed() {
        let camera = WorldPosition::new(BrickCoordinate::new(0, 0, 0), [0.0; 3]);
        let occluders: Vec<SunOccluder> = (0..20)
            .map(|index| SunOccluder::Ball {
                centre: WorldPosition::new(BrickCoordinate::new(100 + index, 0, 0), [0.0; 3]),
                radius: 10.0,
            })
            .chain([SunOccluder::FlatGround])
            .collect();
        let (balls, flat) = balls_and_ground(&occluders, camera);
        assert!(flat);
        assert_eq!(balls.len(), MAXIMUM_SUN_BALLS);
        let nearest = balls
            .iter()
            .map(|ball| ball.centre.x)
            .fold(f32::MAX, f32::min);
        assert_eq!(nearest, 3200.0);
        let farthest = balls.iter().map(|ball| ball.centre.x).fold(0.0, f32::max);
        assert_eq!(farthest, 3200.0 + 32.0 * 7.0, "the eight nearest");
        let (none, flat) = balls_and_ground(&[], camera);
        assert!(none.is_empty() && !flat);
    }

    #[test]
    fn the_cameras_daylight_is_the_rule_of_the_pictures_at_the_middle_of_the_view() {
        let planet = SunOccluder::Ball {
            centre: WorldPosition::new(BrickCoordinate::new(0, -32, 0), [0.0; 3]),
            radius: 1000.0,
        };
        let noon = daylight_at(
            WorldPosition::new(BrickCoordinate::new(0, 0, 0), [0.0; 3]),
            Vec3::Y,
            &[planet],
        );
        let night = daylight_at(
            WorldPosition::new(BrickCoordinate::new(0, 0, 0), [0.0; 3]),
            -Vec3::Y,
            &[planet],
        );
        assert_eq!((noon, night), (1.0, 0.0));
    }
}
