//! Value noise built for exact reproducibility and cheap, tight bounds.
//!
//! Each octave places pseudo-random values on a lattice whose spacing (the octave's wavelength)
//! is a power of two voxels, and blends them smoothly. Power-of-two spacing makes the lattice
//! cell and the position inside it exact integer operations, so there is no floating-point
//! error to differ between machines, and lattice hashing works at any coordinate.

use voxel_space::{LevelOfDetail, VoxelBox, VoxelCoordinate};

use crate::interval::Interval;

/// The most octave wavelength supported (2^56 voxels).
pub const MAXIMUM_WAVELENGTH_LOG2: u8 = 56;

/// Most lattice points per axis a box may touch before bounds fall back to the full range.
const MAXIMUM_BOUNDED_LATTICE_POINTS_PER_AXIS: i128 = 4;

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
/// complement bit pattern. A coordinate that fits an `i64` hashes as it always has (so terrain
/// near the origin is the same as before coordinates were 128 bits); one that does not has its
/// upper 64 bits mixed in as well, so that points 2^64 apart are not the same point.
pub fn lattice_hash(seed: u64, x: i128, y: i128, z: i128) -> u64 {
    if let (Ok(x), Ok(y), Ok(z)) = (i64::try_from(x), i64::try_from(y), i64::try_from(z)) {
        return lattice_hash_narrow(seed, x, y, z);
    }
    let mut hash = mix(seed);
    for component in [x, y, z] {
        let low = component as u64;
        hash = mix(hash ^ low);
        if i128::from(low as i64) != component {
            hash = mix(hash ^ (component >> 64) as u64);
        }
    }
    hash
}

/// [`lattice_hash`] for a point whose coordinates all fit an `i64`, which is nearly every point there is and every one of a world not astronomically large: the same
/// hash, without the wide arithmetic (which makes a hash three times as slow, and a voxel takes dozens).
#[inline]
pub fn lattice_hash_narrow(seed: u64, x: i64, y: i64, z: i64) -> u64 {
    let mut hash = mix(seed);
    for component in [x, y, z] {
        hash = mix(hash ^ component as u64);
    }
    hash
}

/// The pseudo-random value at a lattice point, in `[-1, 1)`. Exact: 53 hash bits scaled by a
/// power of two.
pub fn lattice_value(seed: u64, x: i128, y: i128, z: i128) -> f64 {
    value_of_hash(lattice_hash(seed, x, y, z))
}

/// [`lattice_value`] for a point whose coordinates all fit an `i64` (see [`lattice_hash_narrow`]).
#[inline]
pub fn lattice_value_narrow(seed: u64, x: i64, y: i64, z: i64) -> f64 {
    value_of_hash(lattice_hash_narrow(seed, x, y, z))
}

#[inline]
fn value_of_hash(hash: u64) -> f64 {
    const SCALE: f64 = 1.0 / 4_503_599_627_370_496.0; // 2^-52
    (hash >> 11) as f64 * SCALE - 1.0
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
        // Nearly every position there is has coordinates well within an `i64`, and then every octave is worked out in 64-bit arithmetic, as it was before coordinates were
        // 128 bits (the wide arithmetic makes a sample take twice as long). The one check is for all the octaves.
        let narrow = NarrowPosition::of(position);
        let mut total = 0.0;
        for (index, octave) in self.octaves.iter().enumerate() {
            if Self::keeps(octave, footprint) {
                let seed = self.octave_seed(index);
                let unit = match narrow {
                    Some(narrow) => {
                        narrow.octave(seed, octave.wavelength_log2, self.dimensionality)
                    }
                    None => {
                        sample_octave(seed, octave.wavelength_log2, self.dimensionality, position)
                    }
                };
                total += octave.amplitude * unit;
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

    /// Bounds on the samples of `region` at `footprint` and at every finer footprint (a smaller detail level): the octaves that `footprint` keeps are bounded as
    /// they are, and each that it drops may or may not be there at a finer one, so it adds what it can add or nothing. For asking whether a whole cube, and
    /// everything that will be drawn inside it however finely, holds nothing.
    pub fn bounds_through_finer(&self, region: &VoxelBox, footprint: LevelOfDetail) -> Interval {
        let mut total = Interval::point(0.0);
        for (index, octave) in self.octaves.iter().enumerate() {
            let unit = octave_bounds(
                self.octave_seed(index),
                octave.wavelength_log2,
                self.dimensionality,
                region,
            )
            .scaled(octave.amplitude);
            total = total.plus(if Self::keeps(octave, footprint) {
                unit
            } else {
                unit.hull(Interval::point(0.0))
            });
        }
        total
    }

    /// Whether the noise has octaves and every one of them is finer than `footprint`, so that it is dropped altogether (and the sample is 0 because there
    /// is nothing left, not because the noise happens to be zero there). Noise with no octaves at all is not "dropped".
    pub fn is_entirely_finer_than(&self, footprint: LevelOfDetail) -> bool {
        !self.octaves.is_empty()
            && self
                .octaves
                .iter()
                .all(|octave| !Self::keeps(octave, footprint))
    }

    /// The most the value can be away from zero at any position and any footprint: the combined amplitude of all the octaves. (The bound of a region too
    /// wide for the lattice to say anything about.)
    pub fn largest_magnitude(&self) -> f64 {
        self.octaves.iter().map(|octave| octave.amplitude).sum()
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
fn split_axis(coordinate: i128, wavelength_log2: u8) -> (i128, f64) {
    let cell = coordinate >> wavelength_log2;
    // (The part within the cell is below the wavelength, at most 2^63: through a `u64` it converts to `f64` exactly and fast, where a 128-bit integer
    // converts by a call to a library routine, which is the most of the cost of this in a loop over every voxel.)
    let within = (coordinate & ((1i128 << wavelength_log2) - 1)) as u64;
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

/// The value of an octave of noise at a position: the corner values of the lattice cell it is in, blended by where in the cell it is. `corner(dx, dy, dz)` is the value
/// at the corner `dx`, `dy`, `dz` (each 0 or 1) along from the cell's lowest.
#[inline]
fn blend_corners(
    dimensionality: Dimensionality,
    fractions: [f64; 3],
    corner: impl Fn(i64, i64, i64) -> f64,
) -> f64 {
    let [fraction_x, fraction_y, fraction_z] = fractions;
    let (weight_x, weight_z) = (fade(fraction_x), fade(fraction_z));
    match dimensionality {
        Dimensionality::Planar => lerp(
            lerp(corner(0, 0, 0), corner(1, 0, 0), weight_x),
            lerp(corner(0, 0, 1), corner(1, 0, 1), weight_x),
            weight_z,
        ),
        Dimensionality::Volumetric => {
            let weight_y = fade(fraction_y);
            let along_x = |dy, dz| lerp(corner(0, dy, dz), corner(1, dy, dz), weight_x);
            lerp(
                lerp(along_x(0, 0), along_x(1, 0), weight_y),
                lerp(along_x(0, 1), along_x(1, 1), weight_y),
                weight_z,
            )
        }
    }
}

/// A position whose three coordinates are within 2^62 of the origin, so that a lattice cell of it and the one beyond, whatever the wavelength, fit an `i64`.
#[derive(Clone, Copy)]
struct NarrowPosition {
    coordinates: [i64; 3],
}

impl NarrowPosition {
    const LIMIT: i64 = 1 << 62;

    fn of(position: VoxelCoordinate) -> Option<Self> {
        let narrow = |coordinate: i128| {
            i64::try_from(coordinate)
                .ok()
                .filter(|coordinate| (-Self::LIMIT..Self::LIMIT).contains(coordinate))
        };
        Some(Self {
            coordinates: [
                narrow(position.x())?,
                narrow(position.y())?,
                narrow(position.z())?,
            ],
        })
    }

    /// [`sample_octave`] of this position, in 64-bit arithmetic: the same value to the bit.
    #[inline]
    fn octave(self, seed: u64, wavelength_log2: u8, dimensionality: Dimensionality) -> f64 {
        let [x, y, z] = self.coordinates;
        let (cell_x, fraction_x) = split_axis_narrow(x, wavelength_log2);
        let (cell_z, fraction_z) = split_axis_narrow(z, wavelength_log2);
        let (cell_y, fraction_y) = match dimensionality {
            Dimensionality::Planar => (0, 0.0),
            Dimensionality::Volumetric => split_axis_narrow(y, wavelength_log2),
        };
        blend_corners(
            dimensionality,
            [fraction_x, fraction_y, fraction_z],
            |dx, dy, dz| lattice_value_narrow(seed, cell_x + dx, cell_y + dy, cell_z + dz),
        )
    }
}

/// [`split_axis`] for a coordinate that fits an `i64`.
#[inline]
fn split_axis_narrow(coordinate: i64, wavelength_log2: u8) -> (i64, f64) {
    let cell = coordinate >> wavelength_log2;
    let within = (coordinate & ((1i64 << wavelength_log2) - 1)) as u64;
    let fraction = (within as f64 + 0.5) / (1u64 << wavelength_log2) as f64;
    (cell, fraction)
}

fn sample_octave(
    seed: u64,
    wavelength_log2: u8,
    dimensionality: Dimensionality,
    position: VoxelCoordinate,
) -> f64 {
    let (cell_x, fraction_x) = split_axis(position.x(), wavelength_log2);
    let (cell_z, fraction_z) = split_axis(position.z(), wavelength_log2);
    // A planar noise does not look at y: its cell is 0, as the corners below have it.
    let (cell_y, fraction_y) = match dimensionality {
        Dimensionality::Planar => (0, 0.0),
        Dimensionality::Volumetric => split_axis(position.y(), wavelength_log2),
    };
    blend_corners(
        dimensionality,
        [fraction_x, fraction_y, fraction_z],
        |dx, dy, dz| {
            lattice_value(
                seed,
                cell_x + i128::from(dx),
                cell_y + i128::from(dy),
                cell_z + i128::from(dz),
            )
        },
    )
}

/// The cell of the lattice that a position holds and where in it, as `split_axis` says for a voxel, for a position given as twice its coordinate in voxels (so
/// that the middle of a region, which can be half a voxel off, is exact at any distance from the origin).
fn split_twice(twice: i128, wavelength_log2: u8) -> (i128, f64) {
    let shift = u32::from(wavelength_log2) + 1;
    let cell = twice >> shift;
    let within = twice - (cell << shift);
    (cell, within as f64 / (1u128 << shift) as f64)
}

/// The value of an octave at a position that is twice the given numbers in voxels (see [`split_twice`]).
fn sample_octave_between(
    seed: u64,
    wavelength_log2: u8,
    dimensionality: Dimensionality,
    twice: [i128; 3],
) -> f64 {
    let (cell_x, fraction_x) = split_twice(twice[0], wavelength_log2);
    let (cell_z, fraction_z) = split_twice(twice[2], wavelength_log2);
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
            let (cell_y, fraction_y) = split_twice(twice[1], wavelength_log2);
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

/// The steepest the quintic blend of [`fade`] gets: its slope is greatest in the middle, 30 * (1/2)^2 * (1/2)^2.
const STEEPEST_FADE: f64 = 1.875;

/// Room for the rounding of the blend, so that a bound is never a hair under a value.
const ROUNDING_ALLOWANCE: f64 = 1.0e-9;

/// Bounds of one octave's unit-amplitude value over `region`. A sample is a weighted average of the lattice values at the corners of its cell, so it lies between
/// the smallest and largest lattice value among the cells the region touches. Where the region is small beside the cell that is far too wide: the value cannot
/// move from its value at the middle of the region by more than the steepest slope of the blend (the largest difference of two neighbouring lattice values times
/// [`STEEPEST_FADE`], over the cell's size) times how far the region reaches from its middle, along each axis. The bound is the narrower of the two.
fn octave_bounds(
    seed: u64,
    wavelength_log2: u8,
    dimensionality: Dimensionality,
    region: &VoxelBox,
) -> Interval {
    let lattice_range =
        |low: i128, high: i128| (low >> wavelength_log2, (high >> wavelength_log2) + 1);
    let (x_first, x_last) = lattice_range(region.min().x(), region.max().x());
    let (z_first, z_last) = lattice_range(region.min().z(), region.max().z());
    let (y_first, y_last) = match dimensionality {
        Dimensionality::Planar => (0, 0),
        Dimensionality::Volumetric => lattice_range(region.min().y(), region.max().y()),
    };
    let too_wide =
        |first: i128, last: i128| last - first + 1 > MAXIMUM_BOUNDED_LATTICE_POINTS_PER_AXIS;
    if too_wide(x_first, x_last) || too_wide(y_first, y_last) || too_wide(z_first, z_last) {
        return Interval::new(-1.0, 1.0);
    }
    let mut lowest = f64::INFINITY;
    let mut highest = f64::NEG_INFINITY;
    // The lattice values of the corners, to find the largest step between neighbours along each axis.
    let mut values = [[[0.0f64; 4]; 4]; 4];
    for (i, x) in (x_first..=x_last).enumerate() {
        for (j, y) in (y_first..=y_last).enumerate() {
            for (k, z) in (z_first..=z_last).enumerate() {
                let value = lattice_value(seed, x, y, z);
                values[i][j][k] = value;
                lowest = lowest.min(value);
                highest = highest.max(value);
            }
        }
    }
    let counts = [
        (x_last - x_first + 1) as usize,
        (y_last - y_first + 1) as usize,
        (z_last - z_first + 1) as usize,
    ];
    let mut steepest = [0.0f64; 3];
    for i in 0..counts[0] {
        for j in 0..counts[1] {
            for k in 0..counts[2] {
                if i + 1 < counts[0] {
                    steepest[0] = steepest[0].max((values[i + 1][j][k] - values[i][j][k]).abs());
                }
                if j + 1 < counts[1] {
                    steepest[1] = steepest[1].max((values[i][j + 1][k] - values[i][j][k]).abs());
                }
                if k + 1 < counts[2] {
                    steepest[2] = steepest[2].max((values[i][j][k + 1] - values[i][j][k]).abs());
                }
            }
        }
    }
    let size = (1u128 << wavelength_log2) as f64;
    let reach = [
        (region.max().x() - region.min().x()) as f64 / 2.0,
        (region.max().y() - region.min().y()) as f64 / 2.0,
        (region.max().z() - region.min().z()) as f64 / 2.0,
    ];
    let mut spread = ROUNDING_ALLOWANCE;
    for axis in 0..3 {
        let varies = axis != 1 || dimensionality == Dimensionality::Volumetric;
        if varies {
            spread += STEEPEST_FADE * steepest[axis] / size * reach[axis];
        }
    }
    // Twice the position of the middle of the voxels of the region (a voxel's position is the middle of it, half a voxel in).
    let twice_middle = |low: i128, high: i128| low + high + 1;
    let centre = sample_octave_between(
        seed,
        wavelength_log2,
        dimensionality,
        [
            twice_middle(region.min().x(), region.max().x()),
            twice_middle(region.min().y(), region.max().y()),
            twice_middle(region.min().z(), region.max().z()),
        ],
    );
    let (lower, upper) = (lowest.max(centre - spread), highest.min(centre + spread));
    if lower <= upper {
        Interval::new(lower, upper)
    } else {
        Interval::new(lowest, highest)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn voxel(x: i128, y: i128, z: i128) -> VoxelCoordinate {
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

    /// A coordinate of any size the range allows, with the ends of what a narrow position holds, and of an `i64`, well represented.
    fn any_coordinate() -> impl Strategy<Value = i128> {
        prop_oneof![
            3 => -1000i128..1000,
            2 => any::<i128>().prop_map(|value| value >> 8),
            2 => (-3i128..3).prop_map(|delta| (1i128 << 62) + delta),
            2 => (-3i128..3).prop_map(|delta| -(1i128 << 62) + delta),
            1 => (-3i128..3).prop_map(|delta| (1i128 << 63) + delta),
            1 => (-3i128..3).prop_map(|delta| -(1i128 << 63) + delta),
            1 => (-3i128..3).prop_map(|delta| (1i128 << 119) + delta),
        ]
    }

    proptest! {
        /// The narrow path (64-bit arithmetic, taken for nearly every position) and the wide one give the same octave to the bit, at every wavelength and both
        /// dimensionalities, wherever a position is narrow.
        #[test]
        fn the_narrow_and_wide_octaves_agree_to_the_bit(
            seed in any::<u64>(),
            x in any_coordinate(),
            y in any_coordinate(),
            z in any_coordinate(),
            wavelength_log2 in 0u8..=40,
            volumetric in any::<bool>(),
        ) {
            let dimensionality = if volumetric { Dimensionality::Volumetric } else { Dimensionality::Planar };
            let position = VoxelCoordinate::new(x, y, z);
            if let Some(narrow) = NarrowPosition::of(position) {
                prop_assert_eq!(
                    narrow.octave(seed, wavelength_log2, dimensionality).to_bits(),
                    sample_octave(seed, wavelength_log2, dimensionality, position).to_bits()
                );
            } else {
                // Not narrow means one coordinate is at 2^62 or beyond in size.
                prop_assert!([x, y, z].iter().any(|c| c.unsigned_abs() >= 1 << 62));
            }
            // And whichever path a position takes, the sum of octaves is finite and within what the amplitudes allow.
            let noise = FractalNoise::standard(seed, dimensionality, wavelength_log2.max(3), 3, 1.0, 0.5);
            let value = noise.sample(position, LevelOfDetail::FULL_DETAIL);
            prop_assert!(value.abs() <= noise.largest_magnitude() + 1e-12, "{value}");
        }

        #[test]
        fn the_narrow_hash_is_the_wide_hash_wherever_the_point_fits_an_i64(
            seed in any::<u64>(),
            x in any::<i64>(),
            y in any::<i64>(),
            z in any::<i64>(),
        ) {
            prop_assert_eq!(
                lattice_hash_narrow(seed, x, y, z),
                lattice_hash(seed, i128::from(x), i128::from(y), i128::from(z))
            );
            prop_assert_eq!(
                lattice_value_narrow(seed, x, y, z).to_bits(),
                lattice_value(seed, i128::from(x), i128::from(y), i128::from(z)).to_bits()
            );
        }
    }

    #[test]
    fn a_position_is_narrow_up_to_2_to_the_62_and_not_from_it() {
        let at = |x: i128| VoxelCoordinate::new(x, 0, 0);
        assert!(NarrowPosition::of(at((1 << 62) - 1)).is_some());
        assert!(NarrowPosition::of(at(-(1 << 62))).is_some());
        assert!(NarrowPosition::of(at(1 << 62)).is_none());
        assert!(NarrowPosition::of(at(-(1 << 62) - 1)).is_none());
        assert!(NarrowPosition::of(at(1 << 100)).is_none());
        assert!(NarrowPosition::of(VoxelCoordinate::new(0, 1 << 70, 0)).is_none());
    }

    /// Reference values computed independently with arbitrary-precision integer arithmetic (the 128-bit rule: the upper 64 bits are mixed in
    /// only for a coordinate that does not fit an `i64`).
    #[test]
    fn lattice_hashes_of_coordinates_beyond_64_bits_match_an_independent_computation() {
        let cases = [
            (0x1234, (1i128 << 64, 0, 0), 0x3BE8_552E_1C68_4859),
            (0x1234, ((1 << 64) + 5, 0, 0), 0xFA04_8F3E_A3EB_6E85),
            (
                7,
                (-(1 << 100), (1 << 119) + 12345, -1),
                0x6D05_B9A5_54AD_1D86,
            ),
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
    fn points_that_differ_only_above_64_bits_are_different_points() {
        let near = lattice_hash(9, 5, 6, 7);
        for shift in [64, 65, 100, 126] {
            assert_ne!(
                lattice_hash(9, 5 + (1 << shift), 6, 7),
                near,
                "x + 2^{shift}"
            );
            assert_ne!(
                lattice_hash(9, 5, 6 - (1 << shift), 7),
                near,
                "y - 2^{shift}"
            );
            assert_ne!(
                lattice_hash(9, 5, 6, 7 + (1 << shift)),
                near,
                "z + 2^{shift}"
            );
        }
        // The edges of what an i64 holds are still hashed as they were.
        let edge = i128::from(i64::MAX);
        assert_ne!(lattice_hash(9, edge, 0, 0), lattice_hash(9, edge + 1, 0, 0));
        assert_ne!(
            lattice_hash(9, -edge - 1, 0, 0),
            lattice_hash(9, -edge - 2, 0, 0)
        );
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
    fn the_largest_magnitude_is_the_sum_of_the_amplitudes_and_no_sample_exceeds_it() {
        let noise = FractalNoise::standard(11, Dimensionality::Volumetric, 5, 4, 8.0, 0.5);
        assert_eq!(noise.largest_magnitude(), 8.0 + 4.0 + 2.0 + 1.0);
        assert_eq!(FractalNoise::none().largest_magnitude(), 0.0);
        for i in 0..2000 {
            let value = noise.sample(
                voxel(i * 7, i * 3 - 900, -i * 5),
                LevelOfDetail::FULL_DETAIL,
            );
            assert!(value.abs() <= noise.largest_magnitude(), "{value}");
        }
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

    /// The range of the lattice values of the corners of the cells a region touches: the bound before it was tightened.
    fn corner_range(noise: &FractalNoise, region: &VoxelBox, octave: usize) -> Interval {
        let wavelength_log2 = noise.octaves[octave].wavelength_log2;
        let range = |low: i128, high: i128| (low >> wavelength_log2, (high >> wavelength_log2) + 1);
        let (x_first, x_last) = range(region.min().x(), region.max().x());
        let (z_first, z_last) = range(region.min().z(), region.max().z());
        let (y_first, y_last) = match noise.dimensionality {
            Dimensionality::Planar => (0, 0),
            Dimensionality::Volumetric => range(region.min().y(), region.max().y()),
        };
        let mut found = Vec::new();
        for x in x_first..=x_last {
            for y in y_first..=y_last {
                for z in z_first..=z_last {
                    found.push(lattice_value(noise.octave_seed(octave), x, y, z));
                }
            }
        }
        Interval::new(
            found.iter().copied().fold(f64::INFINITY, f64::min),
            found.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        )
    }

    #[test]
    fn a_region_inside_one_cell_is_bounded_by_where_it_can_get_to_and_not_by_the_corners_of_the_cell()
     {
        // With 2^20-voxel cells a 32-voxel box lies in a single cell, so its value hardly moves: a few parts in a million, not the range of the corners.
        let noise = FractalNoise::standard(5, Dimensionality::Planar, 20, 1, 1.0, 0.5);
        let region = VoxelBox::new(voxel(1000, 0, 1000), voxel(1031, 31, 1031)).unwrap();
        let bounds = noise.bounds(&region, LevelOfDetail::FULL_DETAIL);
        let corners = corner_range(&noise, &region, 0);
        assert!(corners.upper() - corners.lower() > 0.3, "{corners:?}");
        assert!(bounds.upper() - bounds.lower() < 1e-3, "{bounds:?}");
        assert!(corners.lower() <= bounds.lower() && bounds.upper() <= corners.upper());
        for point in points_in(&region, 50) {
            assert!(bounds.contains(noise.sample(point, LevelOfDetail::FULL_DETAIL)));
        }
    }

    #[test]
    fn a_single_voxel_is_bounded_by_its_own_value() {
        for (wavelength, dimensionality) in [
            (2u8, Dimensionality::Planar),
            (7, Dimensionality::Volumetric),
            (30, Dimensionality::Planar),
        ] {
            let noise = FractalNoise::standard(77, dimensionality, wavelength, 1, 3.0, 0.5);
            for at in [
                voxel(5, 9, -3),
                voxel(-1_000_003, 4, 70_000),
                voxel(1 << 50, 3, -(1 << 40)),
            ] {
                let region = VoxelBox::new(at, at).unwrap();
                let bounds = noise.bounds(&region, LevelOfDetail::FULL_DETAIL);
                let value = noise.sample(at, LevelOfDetail::FULL_DETAIL);
                assert!(bounds.contains(value), "{bounds:?} and {value}");
                assert!(bounds.upper() - bounds.lower() < 1e-7, "{bounds:?}");
            }
        }
    }

    #[test]
    fn the_bounds_of_a_region_are_never_wider_than_the_corners_and_tighter_when_it_is_small() {
        let noise = FractalNoise::standard(2024, Dimensionality::Planar, 7, 6, 40.0, 0.5);
        let small = VoxelBox::new(voxel(100, 0, 200), voxel(101, 1, 201)).unwrap();
        let tight = noise.bounds(&small, level(1));
        let mut corners = Interval::point(0.0);
        for (index, octave) in noise.octaves.iter().enumerate() {
            if FractalNoise::keeps(octave, level(1)) {
                corners =
                    corners.plus(corner_range(&noise, &small, index).scaled(octave.amplitude));
            }
        }
        assert!(tight.lower() >= corners.lower() && tight.upper() <= corners.upper());
        assert!(
            tight.upper() - tight.lower() < (corners.upper() - corners.lower()) / 4.0,
            "{tight:?} against {corners:?}"
        );
    }

    #[test]
    fn bounds_through_finer_hold_what_the_octaves_a_footprint_drops_could_add() {
        let noise = FractalNoise::standard(5, Dimensionality::Volumetric, 6, 4, 8.0, 0.5);
        let region = VoxelBox::new(voxel(0, 0, 0), voxel(40, 40, 40)).unwrap();
        for footprint in 0..9 {
            let through = noise.bounds_through_finer(&region, level(footprint));
            for finer in 0..=footprint {
                let found = noise.bounds(&region, level(finer));
                assert!(
                    through.lower() <= found.lower() + TOLERANCE
                        && found.upper() <= through.upper() + TOLERANCE,
                    "{through:?} at {footprint} does not hold {found:?} at {finer}"
                );
            }
        }
        // Where nothing is dropped it is the bounds.
        assert_eq!(
            noise.bounds_through_finer(&region, level(0)),
            noise.bounds(&region, level(0))
        );
    }

    #[test]
    fn the_steepest_slope_of_the_blend_is_the_constant_the_bounds_are_made_with() {
        // The slope of the quintic blend is 30 t^2 (1 - t)^2, and it is greatest in the middle.
        let slope = |t: f64| 30.0 * t * t * (1.0 - t) * (1.0 - t);
        let mut steepest: f64 = 0.0;
        for step in 0..=1000 {
            let t = f64::from(step) / 1000.0;
            steepest = steepest.max(slope(t));
            if (0.001..=0.999).contains(&t) {
                let h = 1.0e-6;
                let measured = (fade(t + h) - fade(t - h)) / (2.0 * h);
                assert!((measured - slope(t)).abs() < 1.0e-5, "at {t}");
            }
        }
        // A smaller constant would not hold every region; a larger one would make every bound wider than it needs to be.
        assert_eq!(steepest, STEEPEST_FADE);
    }

    #[test]
    fn a_position_given_as_twice_its_value_splits_into_the_cell_it_is_in_and_where() {
        // A wavelength of 8 voxels: a cell is 16 of these units, and the middle of a voxel is an odd number of them.
        assert_eq!(split_twice(0, 3), (0, 0.0));
        assert_eq!(split_twice(1, 3), (0, 1.0 / 16.0));
        assert_eq!(split_twice(15, 3), (0, 15.0 / 16.0));
        assert_eq!(
            split_twice(16, 3),
            (1, 0.0),
            "a lattice line starts the cell after it"
        );
        assert_eq!(split_twice(-1, 3), (-1, 15.0 / 16.0));
        assert_eq!(split_twice(-16, 3), (-1, 0.0));
        assert_eq!(split_twice(-17, 3), (-2, 15.0 / 16.0));
        // For the middle of a voxel it is what `split_axis` says.
        for coordinate in [-17, -8, -1, 0, 1, 7, 8, 100] {
            assert_eq!(
                split_twice(2 * coordinate + 1, 3),
                split_axis(coordinate, 3),
                "voxel {coordinate}"
            );
        }
        // And far out, where a float could not hold the place exactly.
        assert_eq!(
            split_twice((1i128 << 63) + 11, 3),
            (1i128 << 59, 11.0 / 16.0)
        );
    }

    /// Every sample of every small box on a lattice with small cells lies in the bounds of its box: boxes across the borders of cells are where the steepest step
    /// between neighbours is a different one on each side, so a bound made from only some of the cells a box touches would fail here.
    #[test]
    fn the_tight_bounds_hold_every_sample_of_every_small_region_across_lattice_borders() {
        let check = |seed: u64,
                     dimensionality: Dimensionality,
                     lows: &[i128],
                     sizes: &[i128],
                     ys: &[i128]| {
            for &x in lows {
                for &y in ys {
                    for &z in lows {
                        for &size_x in sizes {
                            for &size_y in if ys.len() == 1 { &[0][..] } else { sizes } {
                                for &size_z in sizes {
                                    let region = VoxelBox::new(
                                        voxel(x, y, z),
                                        voxel(x + size_x, y + size_y, z + size_z),
                                    )
                                    .unwrap();
                                    let bounds = octave_bounds(seed, 3, dimensionality, &region);
                                    for sx in x..=x + size_x {
                                        for sy in y..=y + size_y {
                                            for sz in z..=z + size_z {
                                                let value = sample_octave(
                                                    seed,
                                                    3,
                                                    dimensionality,
                                                    voxel(sx, sy, sz),
                                                );
                                                assert!(
                                                    bounds.lower() <= value
                                                        && value <= bounds.upper(),
                                                    "{value} outside {bounds:?} of {region:?}, seed {seed}"
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        };
        let planar_lows: Vec<i128> = (-4..20).collect();
        for seed in [3, 99, 2024] {
            check(
                seed,
                Dimensionality::Planar,
                &planar_lows,
                &[0, 1, 2, 3, 5],
                &[0],
            );
        }
        let volumetric_lows: Vec<i128> = (-2..10).collect();
        for seed in [3, 2024] {
            check(
                seed,
                Dimensionality::Volumetric,
                &volumetric_lows,
                &[0, 1, 3],
                &volumetric_lows,
            );
        }
    }

    /// Pseudo-random positions inside a box, derived from the hash so tests need no extra state.
    fn points_in(region: &VoxelBox, count: u64) -> Vec<VoxelCoordinate> {
        let span = |low: i128, high: i128, salt: u64, index: u64| {
            low + (lattice_hash(99, salt as i128, index as i128, 0) % ((high - low) as u64 + 1))
                as i128
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

    fn base_component() -> impl Strategy<Value = i128> {
        prop_oneof![
            -300i128..300,
            ((1i128 << 40) - 100)..((1i128 << 40) + 100),
            (-(1i128 << 50))..(-(1i128 << 50) + 100),
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
            [0i128..70, 0..70, 0..70],
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
        fn lattice_values_stay_in_range(seed in any::<u64>(), x in any::<i128>(), y in any::<i128>(), z in any::<i128>()) {
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
            let step = [(axis == 0) as i128, (axis == 1) as i128, (axis == 2) as i128];
            let at = |offset: i128| sample_octave(
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

        #[test]
        fn bounds_through_finer_contain_every_sample_at_every_finer_level(
            noise in noises(),
            region in regions(),
            detail in 0u8..10,
        ) {
            let bounds = noise.bounds_through_finer(&region, level(detail));
            for finer in 0..=detail {
                for point in points_in(&region, 12) {
                    let value = noise.sample(point, level(finer));
                    prop_assert!(
                        value >= bounds.lower() - TOLERANCE && value <= bounds.upper() + TOLERANCE,
                        "{value} at {finer} outside [{}, {}] at {point}", bounds.lower(), bounds.upper()
                    );
                }
            }
        }

        /// Small regions are where the tightening works hardest, so they are tried by themselves, at every corner and at many places.
        #[test]
        fn the_tight_bounds_of_small_regions_contain_every_sample(
            noise in noises(),
            base in [base_component(), base_component(), base_component()],
            size in [0i128..6, 0..6, 0..6],
            detail in 0u8..10,
        ) {
            let region = VoxelBox::new(
                voxel(base[0], base[1], base[2]),
                voxel(base[0] + size[0], base[1] + size[1], base[2] + size[2]),
            ).unwrap();
            let bounds = noise.bounds(&region, level(detail));
            for x in base[0]..=base[0] + size[0] {
                for y in base[1]..=base[1] + size[1] {
                    for z in base[2]..=base[2] + size[2] {
                        let value = noise.sample(voxel(x, y, z), level(detail));
                        prop_assert!(
                            value >= bounds.lower() - TOLERANCE && value <= bounds.upper() + TOLERANCE,
                            "{value} outside [{}, {}] at {x} {y} {z}", bounds.lower(), bounds.upper()
                        );
                    }
                }
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
