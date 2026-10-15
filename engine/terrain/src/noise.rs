//! Value noise built for exact reproducibility and cheap, tight bounds.
//!
//! Each octave places pseudo-random values on a lattice whose spacing (the octave's wavelength)
//! is a power of two voxels, and blends them smoothly. Power-of-two spacing makes the lattice
//! cell and the position inside it exact integer operations, so there is no floating-point
//! error to differ between machines, and lattice hashing works at any coordinate.

use voxel_space::{LevelOfDetail, VoxelBox, VoxelCoordinate};

use crate::interval::Interval;

/// The most octave wavelength supported (2^56 voxels).
const MAXIMUM_WAVELENGTH_LOG2: u8 = 56;

/// Most lattice points per axis a box may touch before bounds fall back to the full range.
const MAXIMUM_BOUNDED_LATTICE_POINTS_PER_AXIS: i64 = 4;

const GOLDEN_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// The splitmix64 output function applied to `value`: a fast, well-mixed 64-bit hash.
/// `mix(0)` is the published first output of splitmix64 seeded with 0, `0xE220A8397B1DCDAF`.
pub fn mix(value: u64) -> u64 {
    let mut z = value.wrapping_add(GOLDEN_GAMMA);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A 64-bit hash of a seed and a lattice point. Negative coordinates hash as their two's
/// complement bit pattern.
pub fn lattice_hash(seed: u64, x: i64, y: i64, z: i64) -> u64 {
    let mut hash = mix(seed);
    for component in [x, y, z] {
        hash = mix(hash ^ component as u64);
    }
    hash
}

/// The pseudo-random value at a lattice point, in `[-1, 1)`. Exact: 53 hash bits scaled by a
/// power of two.
pub fn lattice_value(seed: u64, x: i64, y: i64, z: i64) -> f64 {
    const SCALE: f64 = 1.0 / 4_503_599_627_370_496.0; // 2^-52
    (lattice_hash(seed, x, y, z) >> 11) as f64 * SCALE - 1.0
}

/// Which axes a noise varies along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dimensionality {
    /// Depends only on x and z; constant along y. For heightfields.
    Planar,
    /// Depends on all three coordinates. For caves and overhangs.
    Volumetric,
}

/// One layer of a [`FractalNoise`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Octave {
    /// The lattice spacing is `2^wavelength_log2` voxels.
    pub wavelength_log2: u8,
    /// The octave contributes values in `[-amplitude, amplitude]`. Must not be negative.
    pub amplitude: f64,
}

/// A sum of octaves of value noise.
#[derive(Clone, Debug, PartialEq)]
pub struct FractalNoise {
    seed: u64,
    dimensionality: Dimensionality,
    octaves: Vec<Octave>,
}

impl FractalNoise {
    /// Panics if an octave's wavelength exceeds 2^56 or its amplitude is negative or NaN.
    pub fn new(seed: u64, dimensionality: Dimensionality, octaves: Vec<Octave>) -> Self {
        for octave in &octaves {
            assert!(
                octave.wavelength_log2 <= MAXIMUM_WAVELENGTH_LOG2,
                "octave wavelength 2^{} is too large",
                octave.wavelength_log2
            );
            assert!(
                octave.amplitude >= 0.0,
                "octave amplitude must not be negative"
            );
        }
        Self {
            seed,
            dimensionality,
            octaves,
        }
    }

    /// Octaves whose wavelength halves each step from `largest_wavelength_log2` while the
    /// amplitude is multiplied by `persistence`. Stops early at wavelength 1 voxel.
    pub fn standard(
        seed: u64,
        dimensionality: Dimensionality,
        largest_wavelength_log2: u8,
        octave_count: u8,
        largest_amplitude: f64,
        persistence: f64,
    ) -> Self {
        let mut octaves = Vec::new();
        let mut amplitude = largest_amplitude;
        for step in 0..octave_count {
            let Some(wavelength_log2) = largest_wavelength_log2.checked_sub(step) else {
                break;
            };
            octaves.push(Octave {
                wavelength_log2,
                amplitude,
            });
            amplitude *= persistence;
        }
        Self::new(seed, dimensionality, octaves)
    }

    /// Always zero.
    pub fn none() -> Self {
        Self::new(0, Dimensionality::Planar, Vec::new())
    }

    /// An octave is kept at a level of detail only if its wavelength is at least the footprint.
    fn keeps(octave: &Octave, footprint: LevelOfDetail) -> bool {
        octave.wavelength_log2 >= footprint.level()
    }

    fn octave_seed(&self, index: usize) -> u64 {
        mix(self.seed ^ (index as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93))
    }

    pub fn sample(&self, position: VoxelCoordinate, footprint: LevelOfDetail) -> f64 {
        let mut total = 0.0;
        for (index, octave) in self.octaves.iter().enumerate() {
            if Self::keeps(octave, footprint) {
                total += octave.amplitude
                    * sample_octave(
                        self.octave_seed(index),
                        octave.wavelength_log2,
                        self.dimensionality,
                        position,
                    );
            }
        }
        total
    }

    /// Bounds on [`FractalNoise::sample`] for every voxel of `region`. Where an octave's lattice is
    /// coarse compared with the region, the bound is tight; otherwise it is the octave's full range.
    pub fn bounds(&self, region: &VoxelBox, footprint: LevelOfDetail) -> Interval {
        let mut total = Interval::point(0.0);
        for (index, octave) in self.octaves.iter().enumerate() {
            if Self::keeps(octave, footprint) {
                let unit = octave_bounds(
                    self.octave_seed(index),
                    octave.wavelength_log2,
                    self.dimensionality,
                    region,
                );
                total = total.plus(unit.scaled(octave.amplitude));
            }
        }
        total
    }

    /// The most the sampled value can differ between full detail and `footprint`: the combined
    /// amplitude of the octaves dropped at that footprint.
    pub fn omitted_amplitude(&self, footprint: LevelOfDetail) -> f64 {
        self.octaves
            .iter()
            .filter(|octave| !Self::keeps(octave, footprint))
            .map(|octave| octave.amplitude)
            .sum()
    }
}

/// The lattice cell of a coordinate and the position within it, strictly between 0 and 1 (the
/// centre of the voxel).
fn split_axis(coordinate: i64, wavelength_log2: u8) -> (i64, f64) {
    let cell = coordinate >> wavelength_log2;
    let within = coordinate & ((1i64 << wavelength_log2) - 1);
    let fraction = (within as f64 + 0.5) / (1u64 << wavelength_log2) as f64;
    (cell, fraction)
}

/// Quintic smoothstep: zero slope at both ends, so cell borders show no creases.
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn sample_octave(
    seed: u64,
    wavelength_log2: u8,
    dimensionality: Dimensionality,
    position: VoxelCoordinate,
) -> f64 {
    let (cell_x, fraction_x) = split_axis(position.x(), wavelength_log2);
    let (cell_z, fraction_z) = split_axis(position.z(), wavelength_log2);
    let (weight_x, weight_z) = (fade(fraction_x), fade(fraction_z));
    match dimensionality {
        Dimensionality::Planar => {
            let corner = |dx, dz| lattice_value(seed, cell_x + dx, 0, cell_z + dz);
            lerp(
                lerp(corner(0, 0), corner(1, 0), weight_x),
                lerp(corner(0, 1), corner(1, 1), weight_x),
                weight_z,
            )
        }
        Dimensionality::Volumetric => {
            let (cell_y, fraction_y) = split_axis(position.y(), wavelength_log2);
            let weight_y = fade(fraction_y);
            let corner = |dx, dy, dz| lattice_value(seed, cell_x + dx, cell_y + dy, cell_z + dz);
            let along_x = |dy, dz| lerp(corner(0, dy, dz), corner(1, dy, dz), weight_x);
            lerp(
                lerp(along_x(0, 0), along_x(1, 0), weight_y),
                lerp(along_x(0, 1), along_x(1, 1), weight_y),
                weight_z,
            )
        }
    }
}

/// Bounds of one octave's unit-amplitude value over `region`. A sample is a weighted average of
/// the lattice values at the corners of its cell, so it lies between the smallest and largest
/// lattice value among the cells the region touches.
fn octave_bounds(
    seed: u64,
    wavelength_log2: u8,
    dimensionality: Dimensionality,
    region: &VoxelBox,
) -> Interval {
    let lattice_range =
        |low: i64, high: i64| (low >> wavelength_log2, (high >> wavelength_log2) + 1);
    let (x_first, x_last) = lattice_range(region.min().x(), region.max().x());
    let (z_first, z_last) = lattice_range(region.min().z(), region.max().z());
    let (y_first, y_last) = match dimensionality {
        Dimensionality::Planar => (0, 0),
        Dimensionality::Volumetric => lattice_range(region.min().y(), region.max().y()),
    };
    let too_wide =
        |first: i64, last: i64| last - first + 1 > MAXIMUM_BOUNDED_LATTICE_POINTS_PER_AXIS;
    if too_wide(x_first, x_last) || too_wide(y_first, y_last) || too_wide(z_first, z_last) {
        return Interval::new(-1.0, 1.0);
    }
    let mut lowest = f64::INFINITY;
    let mut highest = f64::NEG_INFINITY;
    for x in x_first..=x_last {
        for y in y_first..=y_last {
            for z in z_first..=z_last {
                let value = lattice_value(seed, x, y, z);
                lowest = lowest.min(value);
                highest = highest.max(value);
            }
        }
    }
    Interval::new(lowest, highest)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn voxel(x: i64, y: i64, z: i64) -> VoxelCoordinate {
        VoxelCoordinate::new(x, y, z)
    }

    fn level(level: u8) -> LevelOfDetail {
        LevelOfDetail::new(level).unwrap()
    }

    /// Float rounding in the blend may exceed a bound by a few ulps.
    const TOLERANCE: f64 = 1e-9;

    #[test]
    fn mix_matches_the_published_splitmix64_first_output() {
        assert_eq!(mix(0), 0xE220_A839_7B1D_CDAF);
    }

    /// Reference values computed independently with arbitrary-precision integer arithmetic.
    #[test]
    fn lattice_hashes_match_an_independent_computation() {
        let cases = [
            (0x1234, (0, 0, 0), 0x3BBA_DF25_DD9F_FF02),
            (0x1234, (-1, 5, 100), 0xFAAE_A47A_79CB_B608),
            (0x1234, (1 << 40, -7, 3), 0xB80F_7186_7652_6698),
            (0, (-(1 << 55), (1 << 55) - 1, 0), 0x6DA1_EA35_1BFE_BE11),
        ];
        for (seed, (x, y, z), expected) in cases {
            assert_eq!(
                lattice_hash(seed, x, y, z),
                expected,
                "seed {seed:#x} at {x},{y},{z}"
            );
        }
    }

    #[test]
    fn lattice_values_are_the_top_53_hash_bits_scaled_into_minus_one_to_one() {
        let hash = lattice_hash(0x1234, 0, 0, 0);
        let expected = (hash >> 11) as f64 / 4_503_599_627_370_496.0 - 1.0;
        assert_eq!(lattice_value(0x1234, 0, 0, 0), expected);
    }

    #[test]
    fn different_seeds_and_positions_give_different_values() {
        let reference = lattice_value(1, 3, 4, 5);
        assert_ne!(reference, lattice_value(2, 3, 4, 5));
        assert_ne!(reference, lattice_value(1, 4, 4, 5));
        assert_ne!(reference, lattice_value(1, 3, 5, 5));
        assert_ne!(reference, lattice_value(1, 3, 4, 6));
    }

    #[test]
    fn a_voxel_splits_into_cell_and_centred_fraction() {
        assert_eq!(split_axis(0, 2), (0, 0.125));
        assert_eq!(split_axis(3, 2), (0, 0.875));
        assert_eq!(split_axis(4, 2), (1, 0.125));
        assert_eq!(split_axis(-1, 2), (-1, 0.875));
        assert_eq!(split_axis(-4, 2), (-1, 0.125));
        assert_eq!(split_axis(-5, 2), (-2, 0.875));
    }

    #[test]
    fn planar_noise_ignores_height() {
        let noise = FractalNoise::standard(7, Dimensionality::Planar, 5, 3, 1.0, 0.5);
        let at = |y| noise.sample(voxel(10, y, -20), LevelOfDetail::FULL_DETAIL);
        assert_eq!(at(0), at(1000));
        assert_eq!(at(0), at(-99_999));
    }

    #[test]
    fn volumetric_noise_varies_with_height() {
        let noise = FractalNoise::standard(7, Dimensionality::Volumetric, 5, 3, 1.0, 0.5);
        let values: Vec<f64> = (0..64)
            .map(|y| noise.sample(voxel(10, y, -20), LevelOfDetail::FULL_DETAIL))
            .collect();
        assert!(values.windows(2).any(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn standard_octaves_halve_wavelength_and_scale_amplitude_and_stop_at_one_voxel() {
        let noise = FractalNoise::standard(0, Dimensionality::Planar, 2, 6, 8.0, 0.5);
        assert_eq!(
            noise.octaves,
            [
                Octave {
                    wavelength_log2: 2,
                    amplitude: 8.0
                },
                Octave {
                    wavelength_log2: 1,
                    amplitude: 4.0
                },
                Octave {
                    wavelength_log2: 0,
                    amplitude: 2.0
                },
            ]
        );
    }

    #[test]
    fn no_noise_is_zero_with_point_bounds() {
        let none = FractalNoise::none();
        assert_eq!(none.sample(voxel(5, 5, 5), LevelOfDetail::FULL_DETAIL), 0.0);
        let region = VoxelBox::new(voxel(0, 0, 0), voxel(9, 9, 9)).unwrap();
        assert_eq!(
            none.bounds(&region, LevelOfDetail::FULL_DETAIL),
            Interval::point(0.0)
        );
    }

    #[test]
    fn dropped_octaves_are_exactly_those_finer_than_the_footprint() {
        let noise = FractalNoise::standard(3, Dimensionality::Volumetric, 6, 7, 1.0, 0.5);
        // Octaves have wavelengths 2^6 .. 2^0 with amplitudes 1, 1/2, ... 1/64.
        assert_eq!(noise.omitted_amplitude(level(0)), 0.0);
        assert_eq!(noise.omitted_amplitude(level(1)), 1.0 / 64.0);
        assert_eq!(
            noise.omitted_amplitude(level(3)),
            1.0 / 64.0 + 1.0 / 32.0 + 1.0 / 16.0
        );
        assert!((noise.omitted_amplitude(level(7)) - 127.0 / 64.0).abs() < 1e-12);
        // With every octave dropped the value is exactly zero.
        assert_eq!(noise.sample(voxel(123, -45, 6), level(7)), 0.0);
    }

    #[test]
    fn a_region_inside_one_coarse_cell_gets_bounds_much_tighter_than_the_full_range() {
        let noise = FractalNoise::standard(11, Dimensionality::Volumetric, 12, 1, 1.0, 0.5);
        let region = VoxelBox::new(voxel(100, 100, 100), voxel(131, 131, 131)).unwrap();
        let bounds = noise.bounds(&region, LevelOfDetail::FULL_DETAIL);
        // Eight lattice values at spacing 4096 span far less than the full [-1, 1).
        assert!(bounds.upper() - bounds.lower() < 2.0);
        let wide = VoxelBox::new(voxel(0, 0, 0), voxel(100_000, 100_000, 100_000)).unwrap();
        assert_eq!(
            noise.bounds(&wide, LevelOfDetail::FULL_DETAIL),
            Interval::new(-1.0, 1.0)
        );
    }

    #[test]
    fn a_region_inside_one_voxel_cell_pair_collapses_the_bound_to_the_lattice_extremes() {
        // With 2^20-voxel cells a 32-voxel box lies in a single cell, so its bound is the range
        // of that cell's corner values, which the samples inside must straddle tightly.
        let noise = FractalNoise::standard(5, Dimensionality::Planar, 20, 1, 1.0, 0.5);
        let region = VoxelBox::new(voxel(1000, 0, 1000), voxel(1031, 31, 1031)).unwrap();
        let bounds = noise.bounds(&region, LevelOfDetail::FULL_DETAIL);
        let corners = [
            lattice_value(noise.octave_seed(0), 0, 0, 0),
            lattice_value(noise.octave_seed(0), 1, 0, 0),
            lattice_value(noise.octave_seed(0), 0, 0, 1),
            lattice_value(noise.octave_seed(0), 1, 0, 1),
        ];
        let lowest = corners.iter().copied().fold(f64::INFINITY, f64::min);
        let highest = corners.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert_eq!(bounds, Interval::new(lowest, highest));
    }

    /// Pseudo-random positions inside a box, derived from the hash so tests need no extra state.
    fn points_in(region: &VoxelBox, count: u64) -> Vec<VoxelCoordinate> {
        let span = |low: i64, high: i64, salt: u64, index: u64| {
            low + (lattice_hash(99, salt as i64, index as i64, 0) % ((high - low) as u64 + 1))
                as i64
        };
        let (min, max) = (region.min(), region.max());
        (0..count)
            .map(|index| {
                voxel(
                    span(min.x(), max.x(), 1, index),
                    span(min.y(), max.y(), 2, index),
                    span(min.z(), max.z(), 3, index),
                )
            })
            .chain([min, max])
            .collect()
    }

    fn base_component() -> impl Strategy<Value = i64> {
        prop_oneof![
            -300i64..300,
            ((1i64 << 40) - 100)..((1i64 << 40) + 100),
            (-(1i64 << 50))..(-(1i64 << 50) + 100),
        ]
    }

    fn noises() -> impl Strategy<Value = FractalNoise> {
        (
            any::<u64>(),
            prop_oneof![
                Just(Dimensionality::Planar),
                Just(Dimensionality::Volumetric)
            ],
            0u8..14,
            1u8..7,
            0.1f64..50.0,
        )
            .prop_map(|(seed, dimensionality, wavelength, count, amplitude)| {
                FractalNoise::standard(seed, dimensionality, wavelength, count, amplitude, 0.5)
            })
    }

    fn regions() -> impl Strategy<Value = VoxelBox> {
        (
            [base_component(), base_component(), base_component()],
            [0i64..70, 0..70, 0..70],
        )
            .prop_map(|(base, size)| {
                VoxelBox::new(
                    voxel(base[0], base[1], base[2]),
                    voxel(base[0] + size[0], base[1] + size[1], base[2] + size[2]),
                )
                .unwrap()
            })
    }

    proptest! {
        #[test]
        fn lattice_values_stay_in_range(seed in any::<u64>(), x in any::<i64>(), y in any::<i64>(), z in any::<i64>()) {
            let value = lattice_value(seed, x, y, z);
            prop_assert!((-1.0..1.0).contains(&value));
        }

        /// Noise must be smooth: one voxel step changes a unit octave by at most the steepest
        /// slope of the blend (1.875 per cell, times the value range 2) divided by the cell size.
        #[test]
        fn neighbouring_voxels_differ_by_a_bounded_amount(
            seed in any::<u64>(),
            wavelength_log2 in 1u8..10,
            x in base_component(), y in base_component(), z in base_component(),
            axis in 0usize..3,
            dimensionality in prop_oneof![Just(Dimensionality::Planar), Just(Dimensionality::Volumetric)],
        ) {
            let step = [(axis == 0) as i64, (axis == 1) as i64, (axis == 2) as i64];
            let at = |offset: i64| sample_octave(
                seed, wavelength_log2, dimensionality,
                voxel(x + offset * step[0], y + offset * step[1], z + offset * step[2]),
            );
            let difference = (at(1) - at(0)).abs();
            let limit = 3.75 / f64::from(1u32 << wavelength_log2);
            prop_assert!(difference <= limit + TOLERANCE, "{difference} > {limit}");
        }

        #[test]
        fn bounds_contain_every_sample_inside_the_region_at_every_level(
            noise in noises(),
            region in regions(),
            detail in 0u8..10,
        ) {
            let detail = level(detail);
            let bounds = noise.bounds(&region, detail);
            for point in points_in(&region, 24) {
                let value = noise.sample(point, detail);
                prop_assert!(
                    value >= bounds.lower() - TOLERANCE && value <= bounds.upper() + TOLERANCE,
                    "{value} outside [{}, {}] at {point}", bounds.lower(), bounds.upper()
                );
            }
        }

        /// Coarser sampling may only differ from full detail by the amplitude of the octaves it
        /// drops, which is what lets distant terrain be trusted as a stand-in for the real thing.
        #[test]
        fn coarser_levels_differ_from_full_detail_by_at_most_the_omitted_amplitude(
            noise in noises(),
            region in regions(),
            detail in 0u8..14,
        ) {
            let detail = level(detail);
            let allowance = noise.omitted_amplitude(detail) + TOLERANCE;
            for point in points_in(&region, 12) {
                let difference = (noise.sample(point, detail) - noise.sample(point, LevelOfDetail::FULL_DETAIL)).abs();
                prop_assert!(difference <= allowance, "{difference} > {allowance} at {point}");
            }
        }
    }
}
