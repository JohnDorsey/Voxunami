//! The sun of a game: where it is at a time of the world's clock, what it lights, and what the sky looks like because of it.
//!
//! A game that has a sun says so for a level ([`crate::GameDefinition::sun`]); the shell works the rest out for every picture from the world's clock (which the server
//! sends and every client counts, so everyone has the same sun and a recording shows the same day), the camera and the planets the client knows. None of it is simulation:
//! it is how the world looks, so a recording holds nothing of it.
//!
//! *The sun's path* is a circle on the sky: the direction at noon turned about an axis, once round in a day. *Hours* are the time of day (0 to 24): 12 is noon, when the sun
//! is highest on its path. *Daylight* at a place is how much of the sun reaches it past the planets and the ground ([`renderer::sun_visibility`]), and the sky is the colour
//! the game gives it, darker the less daylight there is at the camera and as it goes up out of the air of a planet (a sky that goes black in space).

use libm::{cos, sin};
use renderer::{SunOccluder, balls_and_ground, daylight_at};
use voxel_space::{BrickCoordinate, WorldPosition};

/// The sky of a level with a sun.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sky {
    /// Its colour in full daylight, in the air, as sRGB.
    pub color: [f32; 3],
    /// How high, in voxels, over the surface of a planet there is air: the sky is its colour up to half of this, fades from there, and is black above it. A level with
    /// no planet in it (flat ground that goes on) has all the air there is.
    pub thickness: f32,
}

/// A sun that moves with the world's clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sun {
    /// Seconds of the world's clock in a day (at the server's ticks per second, whole ticks).
    pub day_seconds: u32,
    /// The time of day, in hours, when the world's clock is 0: where a new world begins.
    pub start_hour: f64,
    /// The direction to the sun at noon (any length).
    pub noon: [f64; 3],
    /// The axis the sun turns about, in the way that takes it from the morning to the afternoon (any length, and perpendicular to `noon` if the path is to be a great circle
    /// of the sky, as it is for a sun that rises and sets as far from the pole of its path as it is high at noon).
    pub axis: [f64; 3],
    /// How bright, 0 to 255, the sun lights a face that looks straight at it (`renderer::Atmosphere::sun_light`).
    pub light: u8,
    /// How bright, 0 to 255, the sky lights the faces the sun reaches (`renderer::Atmosphere::sky_light`), in the air.
    pub sky_light: u8,
    /// How much, 0 to 255, of the sky's light reaches a place the sun does not (`renderer::Atmosphere::sky_light_in_shadow`): one in the shadow of a hill, or a face turned
    /// from the sun. The sky is scattered light, so a shadow is not black; 255 is no shadow of the sky at all.
    pub sky_light_in_shadow: u8,
    /// Whether the level's ground is flat and level, with `y` up, so that the sun is hidden whenever it is below the horizon (the sun cannot light a place from under the
    /// ground). A level with planets in it is not flat: the planets hide the sun.
    pub flat_ground: bool,
    pub sky: Sky,
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

fn cross(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ]
}

fn unit(vector: [f64; 3]) -> [f64; 3] {
    let length = dot(vector, vector).sqrt();
    assert!(length > 0.0, "a direction needs a length");
    vector.map(|part| part / length)
}

impl Sun {
    /// The time of day, in hours from 0 up to (not including) 24, when the world's clock is `clock` ticks and the server ticks `ticks_per_second` times a second.
    pub fn hours_at(&self, clock: u64, ticks_per_second: u32) -> f64 {
        let day_ticks = u64::from(self.day_seconds) * u64::from(ticks_per_second);
        if day_ticks == 0 {
            return self.start_hour.rem_euclid(24.0);
        }
        // (Whole ticks of the day, so the time is exact however old the world is.)
        let into_the_day = (clock % day_ticks) as f64 / day_ticks as f64;
        (self.start_hour + 24.0 * into_the_day).rem_euclid(24.0)
    }

    /// The unit vector towards the sun at this time of day: the direction of noon turned about the axis a twenty-fourth of a turn for each hour from noon. Worked out with
    /// `libm`, so the same on every machine.
    pub fn direction_at(&self, hours: f64) -> [f32; 3] {
        let (noon, axis) = (unit(self.noon), unit(self.axis));
        let angle = (hours - 12.0) / 24.0 * std::f64::consts::TAU;
        let (sine, cosine) = (sin(angle), cos(angle));
        let sideways = cross(axis, noon);
        let along = dot(axis, noon) * (1.0 - cosine);
        std::array::from_fn(|part| {
            (noon[part] * cosine + sideways[part] * sine + axis[part] * along) as f32
        })
    }
}

/// What the shell does for a picture because of the sun.
#[derive(Clone, Debug, PartialEq)]
pub struct SunView {
    /// The time of day this is for, in hours.
    pub hours: f64,
    /// The unit vector towards the sun.
    pub direction: [f32; 3],
    /// `renderer::Atmosphere::sun_light`.
    pub sun_light: u8,
    /// `renderer::Atmosphere::sky_light`, less in thin air.
    pub sky_light: u8,
    /// `renderer::Atmosphere::sky_light_in_shadow`.
    pub sky_light_in_shadow: u8,
    /// The colour of the sky and the fog, as sRGB.
    pub sky_color: [f32; 3],
    /// How much of the sun reaches the camera: 0 to 1.
    pub daylight: f32,
    /// What can hide the sun.
    pub occluders: Vec<SunOccluder>,
}

fn smoothstep(edge_start: f32, edge_end: f32, value: f32) -> f32 {
    let fraction = ((value - edge_start) / (edge_end - edge_start)).clamp(0.0, 1.0);
    fraction * fraction * (3.0 - 2.0 * fraction)
}

/// How much air there is where the camera is: 1 up to half of `thickness` over the nearest ball (a planet's solid core, so a little under its ground), falling to 0 at
/// `thickness` and above. Where there is no ball there is all the air there is.
fn air_at(camera: WorldPosition, occluders: &[SunOccluder], thickness: f32) -> f32 {
    let (balls, _) = balls_and_ground(occluders, camera);
    let nearest = balls
        .iter()
        .map(|ball| {
            let centre = ball.centre.to_array().map(f64::from);
            dot(centre, centre).sqrt() - f64::from(ball.radius)
        })
        .fold(f64::INFINITY, f64::min);
    if nearest.is_infinite() {
        return 1.0;
    }
    1.0 - smoothstep(thickness * 0.5, thickness, nearest as f32)
}

/// The sun for a picture taken from `camera` at this time of day, with these planets (the balls that are ground all through in them) in its way.
pub fn sun_view(
    sun: &Sun,
    hours: f64,
    camera: WorldPosition,
    planets: Vec<SunOccluder>,
) -> SunView {
    let direction = sun.direction_at(hours);
    let mut occluders = planets;
    if sun.flat_ground {
        occluders.push(SunOccluder::FlatGround);
    }
    let daylight = daylight_at(camera, glam::Vec3::from_array(direction), &occluders);
    let air = air_at(camera, &occluders, sun.sky.thickness);
    SunView {
        hours,
        direction,
        sun_light: sun.light,
        sky_light: (f32::from(sun.sky_light) * air).round() as u8,
        sky_light_in_shadow: sun.sky_light_in_shadow,
        sky_color: sun.sky.color.map(|part| part * daylight * air),
        daylight,
        occluders,
    }
}

/// A position from a place in voxels (a brick and the offset in it, as everything that is drawn is placed).
pub fn position_of_voxels(voxels: [f64; 3]) -> WorldPosition {
    let edge = voxel_space::BRICK_EDGE_LENGTH as f64;
    let whole = voxels.map(|part| (part / edge).floor());
    // (An offset a hair under a whole brick rounds to the whole brick as an `f32`: the next brick takes it.)
    WorldPosition::try_new(
        BrickCoordinate::new(whole[0] as i128, whole[1] as i128, whole[2] as i128),
        std::array::from_fn(|axis| (voxels[axis] - whole[axis] * edge) as f32),
    )
    .expect("a place in the world")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sun that goes round in 1,200 seconds from 6 in the morning, over a pole at the top of the world, and lights all it can.
    fn sun() -> Sun {
        Sun {
            day_seconds: 1_200,
            start_hour: 6.0,
            noon: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 2.0],
            light: 255,
            sky_light: 100,
            sky_light_in_shadow: 64,
            flat_ground: false,
            sky: Sky {
                color: [0.4, 0.6, 0.9],
                thickness: 30_000.0,
            },
        }
    }

    fn near(first: [f32; 3], second: [f32; 3]) -> bool {
        first.iter().zip(second).all(|(a, b)| (a - b).abs() < 1e-6)
    }

    #[test]
    fn the_time_of_day_is_the_clock_over_the_length_of_a_day_from_the_hour_the_world_began_at() {
        let sun = sun();
        // 20 ticks a second and 1,200 seconds a day: a day is 24,000 ticks, an hour 1,000.
        assert_eq!(sun.hours_at(0, 20), 6.0);
        assert_eq!(sun.hours_at(1_000, 20), 7.0);
        assert_eq!(sun.hours_at(6_000, 20), 12.0);
        assert_eq!(sun.hours_at(18_000, 20), 0.0, "midnight is 0, not 24");
        assert_eq!(sun.hours_at(24_000, 20), 6.0, "a whole day later");
        assert_eq!(sun.hours_at(24_500, 20), 6.5);
        // A world a hundred billion days old is as exact as a new one.
        assert_eq!(sun.hours_at(24_000 * 100_000_000_000 + 3_000, 20), 9.0);
        // The rate of ticks matters: at 10 a second a day is 12,000 ticks.
        assert_eq!(sun.hours_at(3_000, 10), 12.0);
    }

    #[test]
    fn a_sun_with_a_day_of_no_length_stands_at_its_start_hour() {
        let mut sun = sun();
        sun.day_seconds = 0;
        assert_eq!(sun.hours_at(123_456, 20), 6.0);
        sun.start_hour = 30.0;
        assert_eq!(sun.hours_at(0, 20), 6.0, "an hour past 24 is the morning");
    }

    #[test]
    fn the_sun_is_at_noon_in_its_direction_and_goes_round_the_axis_from_the_morning_to_the_afternoon()
     {
        let sun = sun();
        assert!(near(sun.direction_at(12.0), [0.0, 1.0, 0.0]));
        // The axis is +z and the noon sun +y: the afternoon is towards the cross product (z x y = -x).
        assert!(near(sun.direction_at(18.0), [-1.0, 0.0, 0.0]));
        assert!(near(sun.direction_at(6.0), [1.0, 0.0, 0.0]));
        assert!(near(sun.direction_at(0.0), [0.0, -1.0, 0.0]));
        assert!(near(sun.direction_at(24.0), sun.direction_at(0.0)));
        // Three hours after noon: an eighth of a turn.
        let h = std::f32::consts::FRAC_1_SQRT_2;
        assert!(near(sun.direction_at(15.0), [-h, h, 0.0]));
    }

    #[test]
    fn the_direction_is_a_unit_vector_at_every_time_and_the_path_is_a_circle_about_the_axis() {
        // A tilted sun: noon is not along y, and the axis is across it.
        let sun = Sun {
            noon: [0.0, 0.94, 0.342],
            axis: [0.0, -0.342, 0.94],
            ..sun()
        };
        let axis = unit(sun.axis);
        for step in 0..96 {
            let direction = sun.direction_at(f64::from(step) * 0.25);
            let length = direction.iter().map(|part| part * part).sum::<f32>().sqrt();
            assert!((length - 1.0).abs() < 1e-6, "{step}: {length}");
            let along: f64 = (0..3)
                .map(|part| f64::from(direction[part]) * axis[part])
                .sum();
            assert!(
                along.abs() < 1e-6,
                "{step}: the path leaves its plane by {along}"
            );
        }
    }

    #[test]
    fn a_noon_that_is_not_across_the_axis_gives_a_small_circle_of_the_sky() {
        // Noon along +y and an axis that is 45 degrees to it: the sun is never as far from the axis as at noon, and its height changes over the day by less than a
        // great circle's.
        let sun = Sun {
            axis: [0.0, 1.0, 1.0],
            ..sun()
        };
        let axis = unit(sun.axis);
        let along_at = |hours: f64| {
            let direction = sun.direction_at(hours);
            (0..3)
                .map(|part| f64::from(direction[part]) * axis[part])
                .sum::<f64>()
        };
        for hours in [0.0, 3.0, 9.0, 12.0, 17.5] {
            assert!((along_at(hours) - along_at(12.0)).abs() < 1e-6, "{hours}");
        }
    }

    #[test]
    #[should_panic(expected = "a direction needs a length")]
    fn a_sun_whose_axis_has_no_length_is_a_mistake_and_says_so() {
        let sun = Sun {
            axis: [0.0; 3],
            ..sun()
        };
        let _ = sun.direction_at(12.0);
    }

    fn at_height(height: i128) -> WorldPosition {
        position_of_voxels([0.0, height as f64, 0.0])
    }

    fn planet_below(depth: i128, radius: f32) -> SunOccluder {
        SunOccluder::Ball {
            centre: position_of_voxels([0.0, (-depth) as f64, 0.0]),
            radius,
        }
    }

    #[test]
    fn at_noon_over_a_planet_the_day_is_full_and_the_sky_has_its_colour() {
        let view = sun_view(
            &sun(),
            12.0,
            at_height(10),
            vec![planet_below(100_000, 99_900.0)],
        );
        assert_eq!(view.daylight, 1.0);
        assert_eq!(view.sky_color, [0.4, 0.6, 0.9]);
        assert_eq!((view.sun_light, view.sky_light), (255, 100));
        assert_eq!(view.hours, 12.0);
        assert!(near(view.direction, [0.0, 1.0, 0.0]));
        assert_eq!(
            view.occluders.len(),
            1,
            "flat ground is not added to a sun without it"
        );
    }

    #[test]
    fn at_midnight_over_a_planet_the_sky_is_black_and_the_sun_is_hidden() {
        let view = sun_view(
            &sun(),
            0.0,
            at_height(10),
            vec![planet_below(100_000, 99_900.0)],
        );
        assert_eq!(view.daylight, 0.0);
        assert_eq!(view.sky_color, [0.0; 3]);
        // (The sky's light is the share of the sun that reaches each face, worked out for each by the renderer: the setting is the same.)
        assert_eq!(view.sky_light, 100);
    }

    #[test]
    fn the_sky_goes_dark_as_the_camera_goes_up_out_of_the_air_and_is_black_in_space() {
        let planet = planet_below(100_000, 99_900.0);
        let colour_at =
            |height: i128| sun_view(&sun(), 12.0, at_height(height), vec![planet]).sky_color;
        // The ground is 100 voxels over the core: the air is full up to 15,000 over it and gone at 30,000.
        assert_eq!(colour_at(100), [0.4, 0.6, 0.9]);
        assert_eq!(colour_at(14_000), [0.4, 0.6, 0.9]);
        let middle = colour_at(22_000)[2];
        assert!(0.1 < middle && middle < 0.8, "{middle}");
        assert_eq!(colour_at(40_000), [0.0; 3]);
        assert_eq!(colour_at(5_000_000), [0.0; 3]);
        // And it never gets brighter as it goes up.
        let mut last = f32::MAX;
        for height in (0..40_000).step_by(500) {
            let blue = colour_at(height)[2];
            assert!(blue <= last, "{height}: {blue} after {last}");
            last = blue;
        }
        // The sky's light fades with the air too.
        let light_at =
            |height: i128| sun_view(&sun(), 12.0, at_height(height), vec![planet]).sky_light;
        assert_eq!((light_at(100), light_at(40_000)), (100, 0));
    }

    #[test]
    fn on_flat_ground_the_day_ends_when_the_sun_sets_whatever_the_height() {
        let flat = Sun {
            flat_ground: true,
            ..sun()
        };
        for height in [10, 100_000, 1_000_000_000] {
            let noon = sun_view(&flat, 12.0, at_height(height), Vec::new());
            assert_eq!(noon.daylight, 1.0, "{height}");
            assert_eq!(noon.sky_color, [0.4, 0.6, 0.9], "all the air there is");
            let midnight = sun_view(&flat, 0.0, at_height(height), Vec::new());
            assert_eq!(midnight.daylight, 0.0, "{height}");
            assert_eq!(midnight.sky_color, [0.0; 3]);
        }
        assert_eq!(
            sun_view(&flat, 12.0, at_height(0), Vec::new()).occluders,
            [SunOccluder::FlatGround]
        );
    }

    #[test]
    fn sunset_darkens_the_sky_a_little_at_a_time() {
        let flat = Sun {
            flat_ground: true,
            ..sun()
        };
        let mut last = f32::MAX;
        let mut dimmer = 0;
        for step in 0..=120 {
            let hours = 12.0 + f64::from(step) * 0.1;
            let blue = sun_view(&flat, hours, at_height(10), Vec::new()).sky_color[2];
            assert!(blue <= last + 1e-6, "{hours}");
            dimmer += usize::from(blue < last);
            last = blue;
        }
        assert_eq!(last, 0.0);
        assert!(dimmer >= 5, "the sunset takes some time: {dimmer} steps");
    }

    #[test]
    fn a_place_in_voxels_is_a_brick_and_the_offset_in_it() {
        let position = position_of_voxels([33.5, -0.25, 64.0]);
        assert_eq!(position.brick(), BrickCoordinate::new(1, -1, 2));
        assert_eq!(position.offset(), [1.5, 31.75, 0.0]);
    }
}
