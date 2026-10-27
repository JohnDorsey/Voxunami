//! Gravity as a field: the acceleration at a place in the world, from the pull of the level and of whatever is massive enough to pull.
//!
//! The level's own pull is one vector everywhere (down, today), and each massive body adds a pull toward its centre of mass, of the
//! softened Newtonian strength `G * mass / (distance^2 + softening^2)`, so the field is finite at the centre of a body and falls off as
//! the square of the distance. Everything is `f64` summed in the order the pulls are given, with only products, quotients and square roots, so
//! the field is the same bit for bit on every machine.
//!
//! The pull of a body is a point mass at its centre of mass: exact far from it, and wrong near the surface of a long or flat one, where the
//! pull points at the centre and not at the nearest hull. (The better answer is a sum over coarse cells of mass; this has the interface for it.)

use crate::{BRICK_EDGE_LENGTH, WorldPosition};

/// What gravity is in a world: a server setting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GravitySettings {
    /// The acceleration of the level itself, in voxels per second per second, the same everywhere: 5 downward is what characters fall in (a metric number for a planet that is not the Earth; owner, 2026-10-22).
    pub base: [f32; 3],
    /// The `G` of a body's pull, in voxels cubed per kilogram per second squared. The real one for metres is 6.674e-11, at which only a
    /// body the size of a moon pulls noticeably.
    pub pull_strength: f32,
    /// The least mass, in kilograms, a body must have to pull anything at all: `None` for no body to. This is what "sufficiently huge" is.
    pub pull_mass_threshold: Option<f32>,
    /// How far from a centre of mass, in voxels, the pull stops growing as the distance shrinks.
    pub softening: f32,
}

impl Default for GravitySettings {
    /// Down at 5 metres a second a second, and nothing pulls (no threshold is set).
    fn default() -> Self {
        Self {
            base: [0.0, -5.0, 0.0],
            pull_strength: 6.674e-11,
            pull_mass_threshold: None,
            softening: 1.0,
        }
    }
}

/// A body that may pull: where its centre of mass is in the world and its mass in kilograms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MassPull {
    pub position: WorldPosition,
    pub mass: f32,
}

/// The acceleration at a place, in the world's axes, in voxels per second per second: the level's and the sum of the pulls of the bodies
/// that have at least the threshold mass, in the order given.
pub fn gravity_at(
    settings: &GravitySettings,
    pulls: impl IntoIterator<Item = MassPull>,
    at: WorldPosition,
) -> [f32; 3] {
    let mut total = settings.base.map(f64::from);
    let Some(threshold) = settings.pull_mass_threshold else {
        return settings.base;
    };
    let strength = f64::from(settings.pull_strength);
    let softening = f64::from(settings.softening);
    for pull in pulls {
        if pull.mass < threshold || pull.mass <= 0.0 {
            continue;
        }
        let toward = vector_between(at, pull.position);
        let square: f64 = toward.iter().map(|component| component * component).sum();
        let softened = square + softening * softening;
        // G m r / (r^2 + s^2)^(3/2): the unit vector toward the body, over the softened square of the distance.
        let scale = strength * f64::from(pull.mass) / (softened * softened.sqrt());
        for axis in 0..3 {
            total[axis] += scale * toward[axis];
        }
    }
    total.map(|component| component as f32)
}

/// The vector from one position to another, in voxels, with the bricks differenced first so that it is exact for positions near each other
/// however far both are from the origin.
fn vector_between(from: WorldPosition, to: WorldPosition) -> [f64; 3] {
    let (a, b) = (from.brick(), to.brick());
    let (from_offset, to_offset) = (from.offset(), to.offset());
    let along = |to: i128, from: i128, to_offset: f32, from_offset: f32| {
        (to - from) as f64 * BRICK_EDGE_LENGTH as f64
            + (f64::from(to_offset) - f64::from(from_offset))
    };
    [
        along(b.x(), a.x(), to_offset[0], from_offset[0]),
        along(b.y(), a.y(), to_offset[1], from_offset[1]),
        along(b.z(), a.z(), to_offset[2], from_offset[2]),
    ]
}

#[cfg(test)]
mod tests {
    use crate::BrickCoordinate;

    use super::*;

    fn at(x: f32, y: f32, z: f32) -> WorldPosition {
        WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z])
    }

    fn pulling(threshold: Option<f32>) -> GravitySettings {
        GravitySettings {
            base: [0.0, -32.0, 0.0],
            pull_strength: 1e-3,
            pull_mass_threshold: threshold,
            softening: 1.0,
        }
    }

    #[test]
    fn with_no_threshold_nothing_pulls_and_the_level_is_all_there_is() {
        let moon = MassPull {
            position: at(0.0, 0.0, 0.0),
            mass: 1e30,
        };
        assert_eq!(
            gravity_at(&pulling(None), [moon], at(5.0, 5.0, 5.0)),
            [0.0, -32.0, 0.0]
        );
        assert_eq!(
            gravity_at(&GravitySettings::default(), [], at(1.0, 2.0, 3.0)),
            [0.0, -5.0, 0.0]
        );
    }

    #[test]
    fn a_body_pulls_toward_its_centre_as_the_inverse_square_of_the_distance_and_the_level_is_added()
    {
        let settings = GravitySettings {
            base: [0.0; 3],
            ..pulling(Some(1.0))
        };
        let body = MassPull {
            position: at(0.0, 0.0, 0.0),
            mass: 1e6,
        };
        // G m / d^2 at 100 and at 200: a quarter as much at twice the distance (the softening of one voxel is a ten thousandth).
        let near = gravity_at(&settings, [body], at(100.0, 0.0, 0.0));
        let far = gravity_at(&settings, [body], at(200.0, 0.0, 0.0));
        assert!((near[0] + 1e-3 * 1e6 / 1e4).abs() < 1e-3, "{near:?}");
        assert!((far[0] / near[0] - 0.25).abs() < 1e-3, "{near:?} {far:?}");
        assert!(near[1].abs() < 1e-6 && near[2].abs() < 1e-6);
        // From any side it is toward the body, and the level's gravity is added to it.
        let below = gravity_at(&pulling(Some(1.0)), [body], at(0.0, -100.0, 0.0));
        assert!((below[1] - (-32.0 + 0.1)).abs() < 1e-3, "{below:?}");
        let beside = gravity_at(&pulling(Some(1.0)), [body], at(0.0, 0.0, 100.0));
        assert!((beside[2] + 0.1).abs() < 1e-3 && (beside[1] + 32.0).abs() < 1e-3);
    }

    #[test]
    fn the_pull_is_finite_and_zero_at_the_centre_and_small_bodies_do_not_pull() {
        let settings = GravitySettings {
            base: [0.0; 3],
            ..pulling(Some(1000.0))
        };
        let body = MassPull {
            position: at(7.0, 7.0, 7.0),
            mass: 1e9,
        };
        assert_eq!(gravity_at(&settings, [body], at(7.0, 7.0, 7.0)), [0.0; 3]);
        let near = gravity_at(&settings, [body], at(8.0, 7.0, 7.0));
        // At the softening distance: G m r / (2 r^2)^(3/2) with r = 1.
        assert!(
            (near[0] + 1e-3 * 1e9 / (2.0f32 * 2.0f32.sqrt())).abs() < 1.0,
            "{near:?}"
        );
        let light = MassPull {
            position: at(7.0, 7.0, 7.0),
            mass: 999.0,
        };
        assert_eq!(gravity_at(&settings, [light], at(8.0, 7.0, 7.0)), [0.0; 3]);
        // A body with exactly the threshold mass pulls.
        let exact = MassPull {
            position: at(7.0, 7.0, 7.0),
            mass: 1000.0,
        };
        assert!(gravity_at(&settings, [exact], at(8.0, 7.0, 7.0))[0] < 0.0);
        let none = MassPull {
            position: at(7.0, 7.0, 7.0),
            mass: 0.0,
        };
        assert_eq!(
            gravity_at(&pulling(Some(0.0)), [none], at(8.0, 7.0, 7.0)),
            [0.0, -32.0, 0.0]
        );
    }

    #[test]
    fn pulls_add_and_the_sum_is_in_the_order_given_and_exact_far_from_the_origin() {
        let settings = GravitySettings {
            base: [0.0; 3],
            ..pulling(Some(1.0))
        };
        let (left, right) = (
            MassPull {
                position: at(-50.0, 0.0, 0.0),
                mass: 1e6,
            },
            MassPull {
                position: at(50.0, 0.0, 0.0),
                mass: 3e6,
            },
        );
        // Midway, the heavier one wins by the difference.
        let net = gravity_at(&settings, [left, right], at(0.0, 0.0, 0.0));
        assert!(
            (net[0] - 1e-3 * (3e6 - 1e6) / 2500.0).abs() < 1e-3,
            "{net:?}"
        );
        // The same two bodies and place a billion voxels along: the same answer to the precision of the offsets.
        let shifted = |position: WorldPosition| {
            WorldPosition::new(BrickCoordinate::new(31_250_000, 0, 0), position.offset())
                .translated([position.brick().x() as f32 * 32.0, 0.0, 0.0])
        };
        let far = gravity_at(
            &settings,
            [
                MassPull {
                    position: shifted(left.position),
                    ..left
                },
                MassPull {
                    position: shifted(right.position),
                    ..right
                },
            ],
            shifted(at(0.0, 0.0, 0.0)),
        );
        assert!((far[0] - net[0]).abs() < 1e-4, "{far:?} {net:?}");
    }

    #[test]
    fn the_same_inputs_give_the_same_bits() {
        let settings = pulling(Some(1.0));
        let pulls = [
            MassPull {
                position: at(3.3, -4.4, 5.5),
                mass: 123_456.7,
            },
            MassPull {
                position: at(-9.1, 8.2, 0.3),
                mass: 7_654_321.0,
            },
        ];
        let first = gravity_at(&settings, pulls, at(1.25, 2.5, -3.75));
        let second = gravity_at(&settings, pulls, at(1.25, 2.5, -3.75));
        assert_eq!(first.map(f32::to_bits), second.map(f32::to_bits));
        // Written down once; a change in it is a change in the simulation.
        assert_eq!(
            first.map(f32::to_bits),
            [3_256_905_950, 3_240_464_763, 1_099_147_622]
        );
    }
}
