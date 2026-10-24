use std::fmt::Debug;

use voxel_space::{LevelOfDetail, VoxelBox, VoxelCoordinate};

use crate::interval::Interval;
use crate::noise::FractalNoise;

/// A function of position that is positive where the world is solid and zero or negative where it
/// is air.
///
/// Implementations must be deterministic and must keep `bounds` sound: for every voxel inside
/// `region`, `sample` at the same level of detail lies inside `bounds`.
pub trait DensityField: Debug + Send + Sync {
    fn sample(&self, position: VoxelCoordinate, footprint: LevelOfDetail) -> f64;

    fn bounds(&self, region: &VoxelBox, footprint: LevelOfDetail) -> Interval;

    /// Bounds on `sample` over `region` at `footprint` and at every finer footprint (a smaller level of detail). For asking whether a whole cube, and everything
    /// drawn inside it however finely, is air or ground. A field that cannot say gives no bound at all, which is the default.
    fn bounds_through_finer(&self, region: &VoxelBox, footprint: LevelOfDetail) -> Interval {
        let _ = (region, footprint);
        Interval::new(f64::NEG_INFINITY, f64::INFINITY)
    }
}

/// Ground whose surface height varies with x and z: solid below `base_height + hills(x, z)`.
///
/// The density changes by exactly 1 per voxel of height, so it also approximates the vertical
/// distance to the surface. A voxel is solid when its centre is below the surface.
#[derive(Clone, Debug, PartialEq)]
pub struct Heightfield {
    base_height: f64,
    hills: FractalNoise,
}

impl Heightfield {
    pub fn new(base_height: f64, hills: FractalNoise) -> Self {
        Self { base_height, hills }
    }
}

impl DensityField for Heightfield {
    fn sample(&self, position: VoxelCoordinate, footprint: LevelOfDetail) -> f64 {
        self.base_height + self.hills.sample(position, footprint) - (position.y() as f64 + 0.5)
    }

    fn bounds(&self, region: &VoxelBox, footprint: LevelOfDetail) -> Interval {
        let height = Interval::new(region.min().y() as f64 + 0.5, region.max().y() as f64 + 0.5);
        self.hills
            .bounds(region, footprint)
            .shifted(self.base_height)
            .minus(height)
    }

    fn bounds_through_finer(&self, region: &VoxelBox, footprint: LevelOfDetail) -> Interval {
        let height = Interval::new(region.min().y() as f64 + 0.5, region.max().y() as f64 + 0.5);
        self.hills
            .bounds_through_finer(region, footprint)
            .shifted(self.base_height)
            .minus(height)
    }
}

/// Thin sheets of air where a noise passes through zero: `strength * (|noise| - half_thickness)`
/// is negative (air) inside a sheet and positive (solid) elsewhere. Combine with ground using
/// [`Minimum`] to carve caves.
#[derive(Clone, Debug, PartialEq)]
pub struct ShellCaves {
    noise: FractalNoise,
    half_thickness: f64,
    strength: f64,
}

impl ShellCaves {
    /// Panics unless `strength` is positive.
    pub fn new(noise: FractalNoise, half_thickness: f64, strength: f64) -> Self {
        assert!(strength > 0.0, "cave strength must be positive");
        Self {
            noise,
            half_thickness,
            strength,
        }
    }
}

/// What a field whose features are all finer than the footprint is: no feature, which for caves is solid everywhere. (Not the value the noise has
/// when it is all dropped, which is 0, the middle of a sheet of air: that would make a world with caves air everywhere from far away.)
const NO_CAVES: f64 = 1.0e12;

impl DensityField for ShellCaves {
    fn sample(&self, position: VoxelCoordinate, footprint: LevelOfDetail) -> f64 {
        if self.noise.is_entirely_finer_than(footprint) {
            return NO_CAVES;
        }
        self.strength * (self.noise.sample(position, footprint).abs() - self.half_thickness)
    }

    fn bounds(&self, region: &VoxelBox, footprint: LevelOfDetail) -> Interval {
        if self.noise.is_entirely_finer_than(footprint) {
            return Interval::point(NO_CAVES);
        }
        self.noise
            .bounds(region, footprint)
            .absolute()
            .shifted(-self.half_thickness)
            .scaled(self.strength)
    }

    fn bounds_through_finer(&self, region: &VoxelBox, footprint: LevelOfDetail) -> Interval {
        let caves = self
            .noise
            .bounds_through_finer(region, footprint)
            .absolute()
            .shifted(-self.half_thickness)
            .scaled(self.strength);
        // At a footprint where every octave is dropped there are no caves at all, and at a finer one there may be.
        if self.noise.is_entirely_finer_than(footprint) {
            caves.hull(Interval::point(NO_CAVES))
        } else {
            caves
        }
    }
}

/// Solid only where both fields are solid: the intersection of two shapes.
#[derive(Clone, Debug, PartialEq)]
pub struct Minimum<A, B> {
    pub first: A,
    pub second: B,
}

impl<A: DensityField, B: DensityField> DensityField for Minimum<A, B> {
    fn sample(&self, position: VoxelCoordinate, footprint: LevelOfDetail) -> f64 {
        self.first
            .sample(position, footprint)
            .min(self.second.sample(position, footprint))
    }

    fn bounds(&self, region: &VoxelBox, footprint: LevelOfDetail) -> Interval {
        self.first
            .bounds(region, footprint)
            .minimum(self.second.bounds(region, footprint))
    }

    fn bounds_through_finer(&self, region: &VoxelBox, footprint: LevelOfDetail) -> Interval {
        self.first
            .bounds_through_finer(region, footprint)
            .minimum(self.second.bounds_through_finer(region, footprint))
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::noise::Dimensionality;

    fn voxel(x: i64, y: i64, z: i64) -> VoxelCoordinate {
        VoxelCoordinate::new(x, y, z)
    }

    const FULL: LevelOfDetail = LevelOfDetail::FULL_DETAIL;

    fn flat_ground(height: f64) -> Heightfield {
        Heightfield::new(height, FractalNoise::none())
    }

    #[test]
    fn flat_ground_is_solid_exactly_below_its_height() {
        let ground = flat_ground(10.0);
        assert!(ground.sample(voxel(0, 9, 0), FULL) > 0.0);
        assert!(ground.sample(voxel(0, 10, 0), FULL) <= 0.0);
        assert_eq!(ground.sample(voxel(0, 9, 0), FULL), 0.5);
        assert_eq!(ground.sample(voxel(77, 0, -3), FULL), 9.5);
        assert_eq!(ground.sample(voxel(0, -100, 0), FULL), 109.5);
    }

    #[test]
    fn flat_ground_bounds_are_exact() {
        let region = VoxelBox::new(voxel(0, 5, 0), voxel(3, 14, 3)).unwrap();
        assert_eq!(
            flat_ground(10.0).bounds(&region, FULL),
            Interval::new(10.0 - 14.5, 10.0 - 5.5)
        );
    }

    #[test]
    fn hills_raise_and_lower_the_surface_around_the_base_height() {
        let hills = FractalNoise::standard(5, Dimensionality::Planar, 6, 3, 20.0, 0.5);
        let ground = Heightfield::new(50.0, hills);
        let surface_heights: Vec<f64> = (0..200)
            .map(|x| 50.0 + ground.hills.sample(voxel(x * 3, 0, 0), FULL))
            .collect();
        assert!(surface_heights.iter().any(|&height| height > 55.0));
        assert!(surface_heights.iter().any(|&height| height < 45.0));
    }

    #[test]
    fn caves_are_air_inside_sheets_and_solid_elsewhere() {
        let noise = FractalNoise::standard(9, Dimensionality::Volumetric, 4, 1, 1.0, 0.5);
        let caves = ShellCaves::new(noise, 0.1, 16.0);
        let values: Vec<f64> = (0..4000)
            .map(|i| caves.sample(voxel(i, 7, -i), FULL))
            .collect();
        assert!(
            values.iter().any(|&value| value <= 0.0),
            "no cave voxels found"
        );
        assert!(
            values.iter().any(|&value| value > 0.0),
            "no solid voxels found"
        );
        assert!(values.iter().all(|&value| value >= -16.0 * 0.1 - 1e-12));
    }

    #[test]
    fn caves_that_are_all_finer_than_the_footprint_are_no_caves_and_not_air_everywhere() {
        // The noise of caves at footprint 6 has every octave dropped (wavelengths 2^5 and 2^4), and then it is 0, which is the middle of a sheet of air:
        // the world from far away must not be air. Found by the coarse cells of the rolling hills, which were all air from detail level 6.
        let noise = FractalNoise::standard(9, Dimensionality::Volumetric, 5, 2, 1.0, 0.5);
        let caves = ShellCaves::new(noise, 0.08, 16.0);
        let coarse = LevelOfDetail::new(6).unwrap();
        for i in 0..200 {
            assert!(caves.sample(voxel(i * 37, i * 11, -i * 5), coarse) > 0.0);
        }
        let region = VoxelBox::new(voxel(0, 0, 0), voxel(1000, 1000, 1000)).unwrap();
        assert!(caves.bounds(&region, coarse).lower() > 0.0);
        // Where an octave is kept (footprint 5 keeps the first), the sheets are there as they were, and at full detail nothing is changed.
        let kept = LevelOfDetail::new(5).unwrap();
        let values: Vec<f64> = (0..4000)
            .map(|i| caves.sample(voxel(i, 7, -i), kept))
            .collect();
        assert!(values.iter().any(|&value| value <= 0.0));
        // Ground with such caves, from far away, is the ground.
        let world = Minimum {
            first: flat_ground(10.0),
            second: caves,
        };
        assert!(world.sample(voxel(5, 3, 5), coarse) > 0.0);
        assert!(world.sample(voxel(5, 30, 5), coarse) <= 0.0);
        // And noise with no octaves at all is not "dropped": it is 0 at every footprint, as it always was.
        let nothing = ShellCaves::new(FractalNoise::none(), 0.1, 16.0);
        assert!(
            nothing.sample(voxel(1, 2, 3), FULL) < 0.0
                && nothing.sample(voxel(1, 2, 3), coarse) < 0.0
        );
    }

    #[test]
    fn the_minimum_is_solid_only_where_both_fields_are() {
        let combined = Minimum {
            first: flat_ground(10.0),
            second: flat_ground(20.0),
        };
        // Between heights 10 and 20 only the second field is solid.
        assert!(combined.sample(voxel(0, 15, 0), FULL) <= 0.0);
        assert!(combined.sample(voxel(0, 5, 0), FULL) > 0.0);
        let region = VoxelBox::new(voxel(0, 0, 0), voxel(0, 30, 0)).unwrap();
        assert_eq!(
            combined.bounds(&region, FULL),
            Interval::new(10.0 - 30.5, 10.0 - 0.5)
        );
    }

    #[test]
    fn the_minimum_is_bounded_by_the_lower_of_the_two_whichever_comes_first() {
        let region = VoxelBox::new(voxel(0, 0, 0), voxel(0, 30, 0)).unwrap();
        for (first, second) in [(10.0, 20.0), (20.0, 10.0)] {
            let combined = Minimum {
                first: flat_ground(first),
                second: flat_ground(second),
            };
            let lower_ground = 10.0;
            let expected = Interval::new(lower_ground - 30.5, lower_ground - 0.5);
            assert_eq!(combined.bounds(&region, FULL), expected);
            assert_eq!(combined.bounds_through_finer(&region, FULL), expected);
        }
    }

    #[test]
    fn cave_bounds_through_finer_hold_every_sample_at_every_finer_level_where_the_footprint_drops_an_octave()
     {
        // Octaves of 32 and 16 voxels, the second half as strong: a footprint of 5 drops the second, and it is there again at every finer level.
        let caves = ShellCaves::new(
            FractalNoise::standard(5, Dimensionality::Volumetric, 5, 2, 1.0, 0.5),
            0.08,
            16.0,
        );
        let level = |level| LevelOfDetail::new(level).unwrap();
        let mut closer_than_without_the_octave = 0;
        for x in (0..60).step_by(7) {
            for y in (0..60).step_by(7) {
                for z in (0..60).step_by(7) {
                    let region = VoxelBox::new(voxel(x, y, z), voxel(x + 3, y + 3, z + 3)).unwrap();
                    let bounds = caves.bounds_through_finer(&region, level(5));
                    for finer in 0..=5 {
                        for sx in x..=x + 3 {
                            for sy in y..=y + 3 {
                                for sz in z..=z + 3 {
                                    let value = caves.sample(voxel(sx, sy, sz), level(finer));
                                    assert!(
                                        value >= bounds.lower() - 1e-9
                                            && value <= bounds.upper() + 1e-9,
                                        "{value} at {finer} outside {bounds:?} at {sx} {sy} {sz}"
                                    );
                                }
                            }
                        }
                    }
                    // (The octave that comes back widens the bound: the one that leaves it out is narrower.)
                    closer_than_without_the_octave += usize::from(
                        caves.bounds(&region, level(5)).lower() > bounds.lower() + 1e-6,
                    );
                    // Where every octave is kept it is just the bounds, and where one is dropped it is not the value of no caves at all.
                    assert_eq!(
                        caves.bounds_through_finer(&region, level(4)),
                        caves.bounds(&region, level(4))
                    );
                    assert!(bounds.upper() < 100.0, "{bounds:?}");
                    // With every octave dropped there may be caves at a finer level or none.
                    let none = caves.bounds_through_finer(&region, level(6));
                    assert_eq!(none.upper(), NO_CAVES);
                    assert!(none.lower() <= bounds.lower());
                }
            }
        }
        assert!(
            closer_than_without_the_octave > 20,
            "the dropped octave widens the bounds of {closer_than_without_the_octave} regions"
        );
    }

    fn component() -> impl Strategy<Value = i64> {
        prop_oneof![-500i64..500, ((1i64 << 40) - 300)..((1i64 << 40) + 300)]
    }

    proptest! {
        /// Cave bounds on their own, where nothing masks an error in the upper bound: with a
        /// single coarse octave a small region sees values of one sign, so the absolute value
        /// in the bound matters.
        #[test]
        fn cave_bounds_contain_samples_and_use_the_absolute_value(
            seed in any::<u64>(),
            base in [component(), component(), component()],
            size in [0i64..40, 0..40, 0..40],
        ) {
            let caves = ShellCaves::new(
                FractalNoise::standard(seed, Dimensionality::Volumetric, 9, 1, 1.0, 0.5),
                0.1,
                16.0,
            );
            let region = VoxelBox::new(
                voxel(base[0], base[1], base[2]),
                voxel(base[0] + size[0], base[1] + size[1], base[2] + size[2]),
            ).unwrap();
            let bounds = caves.bounds(&region, FULL);
            for corner in [region.min(), region.max()] {
                let value = caves.sample(corner, FULL);
                prop_assert!(
                    value >= bounds.lower() - 1e-9 && value <= bounds.upper() + 1e-9,
                    "{value} outside [{}, {}] at {corner}", bounds.lower(), bounds.upper()
                );
            }
            // Cave density is never below -strength * half_thickness.
            prop_assert!(bounds.lower() >= -16.0 * 0.1 - 1e-9);
        }

        /// Bounds of a composed field (ground carved by caves) must contain every sample.
        #[test]
        fn composed_field_bounds_contain_samples(
            seed in any::<u64>(),
            base in [component(), component(), component()],
            size in [0i64..40, 0..40, 0..40],
            offsets in proptest::collection::vec([0.0f64..=1.0, 0.0..=1.0, 0.0..=1.0], 16),
            detail in 0u8..6,
        ) {
            let field = Minimum {
                first: Heightfield::new(
                    base[1] as f64,
                    FractalNoise::standard(seed, Dimensionality::Planar, 7, 4, 30.0, 0.5),
                ),
                second: ShellCaves::new(
                    FractalNoise::standard(seed ^ 1, Dimensionality::Volumetric, 5, 2, 1.0, 0.5),
                    0.08,
                    16.0,
                ),
            };
            let region = VoxelBox::new(
                voxel(base[0], base[1], base[2]),
                voxel(base[0] + size[0], base[1] + size[1], base[2] + size[2]),
            ).unwrap();
            let detail = LevelOfDetail::new(detail).unwrap();
            let bounds = field.bounds(&region, detail);
            for fraction in offsets {
                let point = voxel(
                    base[0] + (fraction[0] * size[0] as f64) as i64,
                    base[1] + (fraction[1] * size[1] as f64) as i64,
                    base[2] + (fraction[2] * size[2] as f64) as i64,
                );
                let value = field.sample(point, detail);
                prop_assert!(
                    value >= bounds.lower() - 1e-9 && value <= bounds.upper() + 1e-9,
                    "{value} outside [{}, {}] at {point}", bounds.lower(), bounds.upper()
                );
            }
        }

        /// The bounds through finer levels of a composed field hold the samples at every level from the footprint down.
        #[test]
        fn composed_field_bounds_through_finer_contain_samples_at_every_finer_level(
            seed in any::<u64>(),
            base in [component(), component(), component()],
            size in [0i64..40, 0..40, 0..40],
            offsets in proptest::collection::vec([0.0f64..=1.0, 0.0..=1.0, 0.0..=1.0], 8),
            detail in 0u8..8,
        ) {
            let field = Minimum {
                first: Heightfield::new(
                    base[1] as f64,
                    FractalNoise::standard(seed, Dimensionality::Planar, 7, 4, 30.0, 0.5),
                ),
                second: ShellCaves::new(
                    FractalNoise::standard(seed ^ 1, Dimensionality::Volumetric, 5, 2, 1.0, 0.5),
                    0.08,
                    16.0,
                ),
            };
            let region = VoxelBox::new(
                voxel(base[0], base[1], base[2]),
                voxel(base[0] + size[0], base[1] + size[1], base[2] + size[2]),
            ).unwrap();
            let bounds = field.bounds_through_finer(&region, LevelOfDetail::new(detail).unwrap());
            for finer in 0..=detail {
                let finer = LevelOfDetail::new(finer).unwrap();
                for fraction in &offsets {
                    let point = voxel(
                        base[0] + (fraction[0] * size[0] as f64) as i64,
                        base[1] + (fraction[1] * size[1] as f64) as i64,
                        base[2] + (fraction[2] * size[2] as f64) as i64,
                    );
                    let value = field.sample(point, finer);
                    prop_assert!(
                        value >= bounds.lower() - 1e-9 && value <= bounds.upper() + 1e-9,
                        "{value} at {finer:?} outside [{}, {}] at {point}", bounds.lower(), bounds.upper()
                    );
                }
            }
        }
    }
}
