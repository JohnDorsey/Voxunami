//! A planet: ground that is a ball, whose outside is shaped by 2D heightmaps bent round it (`docs/specs/planet-spec.md`, section 3).
//!
//! **The shape.** A voxel is ground when `radius + height(direction) - distance > 0`, where `distance` is how far the middle of the voxel is from the planet's
//! centre (the corner where the eight voxels at the origin meet) and `height` depends only on the direction. The value is about how many voxels below the
//! surface the voxel is, so the layers of the surface (grass, dirt, stone) are read from it as the overworld's are read from its height.
//!
//! **The heightmaps.** The planet is a cube-sphere: each of the six faces of a cube has a heightmap (the overworld's fractal noise, planar), read at the
//! position on the face that is `radius` times the angle across it, so features have the same size over the whole face. Where two or three faces meet, their
//! heights are blended: the weight of a face is smooth in the direction, nothing for a face that is not near the greatest, and the weights are scaled so that
//! their squares add to one, which keeps the spread of the independent noises of the faces (a plain average of two would have half the variance, and the seams
//! would show as flat bands).
//!
//! **Bit for bit the same everywhere.** Only `+ - * /`, `sqrt`, `abs`, `min`, `max` and `floor` are used on floating point, and the angle across a face is a
//! polynomial, not `atan`.

use std::sync::Arc;

use coarse_cells::CoarseNode;
use region_octree::BrickCube;
use voxel_grid::{Brick, ChannelIndex, ChannelStorage, GridLayout};
use voxel_space::{
    BRICK_EDGE_LENGTH, BRICK_VOXEL_COUNT, BrickCoordinate, LevelOfDetail, LocalVoxelIndex,
    VoxelBox, VoxelCoordinate,
};

use terrain::Interval;
use terrain::{DensityField, ShellCaves};
use terrain::{Dimensionality, FractalNoise, mix};
use terrain::{SURFACE_LAYER_DEPTH, SolidBall, SurfaceLayers, TerrainGenerator};

/// How far below the greatest a face's share of the direction can be and still count, as a share of the distance from the centre: the width of the seam.
const BLEND_BAND: f64 = 0.15;

/// The greatest ratio of the distance across a face to the distance along its axis that a face is read at. A face counts where its share of the direction is
/// within [`BLEND_BAND`] of the greatest, and the greatest share is at least 1 / sqrt(3), so the ratio is under (1 / sqrt(3)) / (1 / sqrt(3) - 0.15), which is
/// 1.36; a test checks that. (Bounds over a box ask no further than this.)
const LARGEST_RATIO_READ: f64 = 2.0;

/// Room for the rounding of the arithmetic of a bound, so that a bound is never a hair under a value.
const BOUND_ALLOWANCE: f64 = 1.0e-6;

/// The coefficients of the odd polynomial `x * (c0 + c1 x^2 + c2 x^4 ...)` that is within 1e-5 of the arctangent for `|x| <= 1` (Abramowitz and Stegun 4.4.49).
const ARCTANGENT_COEFFICIENTS: [f64; 9] = [
    1.0,
    -0.333_331_452_8,
    0.199_935_508_5,
    -0.142_088_994_4,
    0.106_562_639_3,
    -0.075_289_640_0,
    0.042_909_613_8,
    -0.016_165_736_7,
    0.002_866_225_7,
];

/// A function that grows like the arctangent, so that a distance along a face is about the angle across it, built from nothing but `+ - *`: the
/// polynomial up to 1 and, beyond it (where a face is read a little past its edge to blend with the next), the straight line that touches it at 1. Odd,
/// increasing, and continuous with a continuous slope.
fn arctangent(x: f64) -> f64 {
    let magnitude = x.abs();
    let squared = magnitude.min(1.0) * magnitude.min(1.0);
    let mut sum = 0.0;
    let mut slope = 0.0;
    // Horner's rule from the highest coefficient down; the slope sums c_k (2k + 1) x^2k at the same time.
    for (index, &coefficient) in ARCTANGENT_COEFFICIENTS.iter().enumerate().rev() {
        sum = sum * squared + coefficient;
        slope = slope * squared + coefficient * (2 * index + 1) as f64;
    }
    let inside = magnitude.min(1.0) * sum;
    let value = inside + (magnitude - magnitude.min(1.0)) * slope;
    value.copysign(x)
}

/// The shape of a planet. All lengths are in voxels. The output for given settings is part of a world's file format, as for every generator (see
/// [`TerrainGenerator`]): a changed design is a new generator.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanetSettings {
    pub seed: u64,
    /// The radius where the hills contribute nothing.
    pub radius: f64,
    /// Height contributed by the largest hills (the sum over all octaves can reach about twice this).
    pub hill_height: f64,
    pub largest_hill_wavelength_log2: u8,
    pub hill_octave_count: u8,
    pub cave_wavelength_log2: u8,
    /// Half the thickness of cave sheets, in noise units (the noise ranges over `[-1, 1]`). Zero for no caves.
    pub cave_half_thickness: f64,
}

impl PlanetSettings {
    /// A planet with one hundredth of the radius of the Earth (a voxel is a metre): 63,710 voxels, 127 kilometres across, with hills a kilometre wide
    /// and a few hundred metres tall, and the sheet caves of the overworld.
    pub fn one_percent_of_earth(seed: u64) -> Self {
        Self {
            seed,
            radius: 63_710.0,
            hill_height: 160.0,
            largest_hill_wavelength_log2: 10,
            hill_octave_count: 9,
            cave_wavelength_log2: 5,
            cave_half_thickness: 0.08,
        }
    }
}

/// One side of the cube whose heightmap a direction is read from: the axis it is across, and which way along that axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Face {
    axis: usize,
    negative: bool,
}

impl Face {
    fn index(self) -> usize {
        self.axis * 2 + usize::from(self.negative)
    }

    /// The two axes across the face.
    fn lateral_axes(self) -> [usize; 2] {
        [(self.axis + 1) % 3, (self.axis + 2) % 3]
    }

    /// The least and the greatest distance along the face's axis, on the face's side of the planet, of the points of a box with these corners (negative or zero
    /// where the box reaches across the plane through the centre).
    fn distances_along(self, low: [f64; 3], high: [f64; 3]) -> (f64, f64) {
        if self.negative {
            (-high[self.axis], -low[self.axis])
        } else {
            (low[self.axis], high[self.axis])
        }
    }
}

/// The ground of a planet, without caves: positive below the surface, and about the number of voxels below it.
#[derive(Clone, Debug)]
pub struct PlanetGround {
    radius: f64,
    faces: [FractalNoise; 6],
}

impl PlanetGround {
    pub fn new(settings: &PlanetSettings) -> Self {
        let faces = std::array::from_fn(|face| {
            let face_seed =
                mix(settings.seed ^ (face as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            FractalNoise::standard(
                face_seed,
                Dimensionality::Planar,
                settings.largest_hill_wavelength_log2,
                settings.hill_octave_count,
                settings.hill_height,
                0.5,
            )
        });
        Self {
            radius: settings.radius,
            faces,
        }
    }

    pub fn radius(&self) -> f64 {
        self.radius
    }

    /// The radius of the ball about the centre that is ground all through: the radius less the most the hills can raise or lower it (the blend of the faces can reach
    /// the square root of three times the greatest height of a face alone, by the Cauchy-Schwarz inequality, for weights whose squares add to one) and a voxel,
    /// for the middle of a voxel being what the field is asked at.
    pub fn core_radius(&self) -> f64 {
        let greatest = self
            .faces
            .iter()
            .map(FractalNoise::largest_magnitude)
            .fold(0.0, f64::max);
        self.radius - 3.0f64.sqrt() * greatest - 1.0
    }

    /// Where on the heightmap of a face a point at `ratio` times the distance along the face's axis across the face is read: in whole voxels.
    fn heightmap_coordinate(&self, ratio: f64) -> i128 {
        (self.radius * arctangent(ratio)).floor() as i128
    }

    /// The share of each axis's face in the height at `position` (a point, not a direction), `weights[axis]`, with the squares adding to one.
    fn blend_weights(position: [f64; 3], distance: f64) -> [f64; 3] {
        let shares = position.map(|component| component.abs() / distance);
        let greatest = shares[0].max(shares[1]).max(shares[2]);
        let weights = shares.map(|share| {
            let nearness = ((share - (greatest - BLEND_BAND)) / BLEND_BAND).clamp(0.0, 1.0);
            nearness * nearness * (3.0 - 2.0 * nearness)
        });
        let norm =
            (weights[0] * weights[0] + weights[1] * weights[1] + weights[2] * weights[2]).sqrt();
        weights.map(|weight| weight / norm)
    }

    /// Where on the heightmap of `face` the point at `position` is read, in whole voxels.
    fn heightmap_position(&self, face: Face, position: [f64; 3]) -> VoxelCoordinate {
        let [first, second] = face.lateral_axes();
        let along = position[face.axis].abs();
        let x = self.heightmap_coordinate(position[first] / along);
        let z = self.heightmap_coordinate(position[second] / along);
        VoxelCoordinate::new(x, 0, z)
    }

    fn face_height(&self, face: Face, position: [f64; 3], footprint: LevelOfDetail) -> f64 {
        self.faces[face.index()].sample(self.heightmap_position(face, position), footprint)
    }

    /// The height of the ground over the radius, in the direction of `position`.
    pub(crate) fn height(
        &self,
        position: [f64; 3],
        distance: f64,
        footprint: LevelOfDetail,
    ) -> f64 {
        let weights = Self::blend_weights(position, distance);
        let mut total = 0.0;
        for (axis, &weight) in weights.iter().enumerate() {
            if weight > 0.0 {
                let face = Face {
                    axis,
                    negative: position[axis] < 0.0,
                };
                total += weight * self.face_height(face, position, footprint);
            }
        }
        total
    }

    /// The middle of a voxel, which is where the field is asked.
    fn middle(position: VoxelCoordinate) -> [f64; 3] {
        [
            position.x() as f64 + 0.5,
            position.y() as f64 + 0.5,
            position.z() as f64 + 0.5,
        ]
    }

    /// Bounds on the field over `region`, given how to bound a face's noise over a box of its heightmap.
    fn bounds_by(
        &self,
        region: &VoxelBox,
        face_bounds: impl Fn(&FractalNoise, &VoxelBox) -> Interval,
    ) -> Interval {
        let low = Self::middle(region.min());
        let high = Self::middle(region.max());
        // How near and how far the box is from the centre, in all, and along each axis.
        let mut nearest_squared = 0.0;
        let mut farthest_squared = 0.0;
        let mut nearest_along = [0.0f64; 3];
        for axis in 0..3 {
            let near = if low[axis] <= 0.0 && high[axis] >= 0.0 {
                0.0
            } else {
                low[axis].abs().min(high[axis].abs())
            };
            let far = low[axis].abs().max(high[axis].abs());
            nearest_along[axis] = near;
            nearest_squared += near * near;
            farthest_squared += far * far;
        }
        let nearest = nearest_squared.sqrt();
        let farthest = farthest_squared.sqrt();
        // A face counts only where its share is within the width of the seam of the greatest, which is at least the greatest of the nearest.
        let greatest_along = nearest_along.iter().copied().fold(0.0, f64::max);

        let mut upper_squared = 0.0;
        let mut lower_squared = 0.0;
        let mut single: Option<Interval> = None;
        let mut faces_counting = 0;
        for axis in 0..3 {
            for negative in [false, true] {
                let face = Face { axis, negative };
                // The distances along the face's axis, on its side of the planet.
                let (along_low, along_high) = face.distances_along(low, high);
                if along_high <= 0.0 || along_high + BLEND_BAND * farthest <= greatest_along {
                    continue;
                }
                let range = self.face_range(face, (low, high), along_low, along_high, &face_bounds);
                faces_counting += 1;
                single = Some(range);
                upper_squared += range.upper().max(0.0) * range.upper().max(0.0);
                lower_squared += (-range.lower()).max(0.0) * (-range.lower()).max(0.0);
            }
        }
        // With one face the height is that face's; with several it is a combination with weights whose squares add to one, which by the Cauchy-Schwarz
        // inequality is no greater than the length of the vector of the faces' greatest values.
        let height = match (faces_counting, single) {
            (1, Some(range)) => range,
            _ => Interval::new(-lower_squared.sqrt(), upper_squared.sqrt()),
        };
        Interval::new(
            self.radius + height.lower() - farthest - BOUND_ALLOWANCE,
            self.radius + height.upper() - nearest + BOUND_ALLOWANCE,
        )
    }

    /// The part of the heightmap of `face` that the points of the box with these `corners` are read from, if their distance along the face's axis is at
    /// least `along_low` and at most `along_high`; `None` if `along_low` is not above zero (the box reaches across, and the part is not bounded) or the part is
    /// beyond the room the world has.
    fn heightmap_region(
        &self,
        face: Face,
        corners: ([f64; 3], [f64; 3]),
        along_low: f64,
        along_high: f64,
    ) -> Option<VoxelBox> {
        if along_low <= 0.0 {
            return None;
        }
        let (low, high) = corners;
        let mut heightmap_low = [0i128; 2];
        let mut heightmap_high = [0i128; 2];
        for (slot, axis) in face.lateral_axes().into_iter().enumerate() {
            // The ratio is least where a negative number is divided by the least distance and a positive one by the greatest, and greatest the other way.
            let least = low[axis]
                / if low[axis] >= 0.0 {
                    along_high
                } else {
                    along_low
                };
            let greatest = high[axis]
                / if high[axis] >= 0.0 {
                    along_low
                } else {
                    along_high
                };
            // Past the ratios a face is read at nothing is read, so a box that reaches far across a face (a cube as wide as the planet's distance from it, beside
            // the plane through its centre) asks about the ratios there are, not about the million-fold ones that would not fit a coordinate.
            let (least, greatest) = (
                least.clamp(-LARGEST_RATIO_READ, LARGEST_RATIO_READ),
                greatest.clamp(-LARGEST_RATIO_READ, LARGEST_RATIO_READ),
            );
            // A voxel more to either side, for rounding.
            heightmap_low[slot] = self.heightmap_coordinate(least).saturating_sub(1);
            heightmap_high[slot] = self.heightmap_coordinate(greatest).saturating_add(1);
        }
        // (A planet so wide that its heightmap does not fit the coordinates has no region to bound; the whole range of its noise stands for it.)
        VoxelBox::new(
            VoxelCoordinate::try_new(heightmap_low[0], 0, heightmap_low[1]).ok()?,
            VoxelCoordinate::try_new(heightmap_high[0], 0, heightmap_high[1]).ok()?,
        )
    }

    /// Bounds on the noise of `face` over the directions of the box `corners` where the face counts, whose distance along the face's axis is at least
    /// `along_low` and at most `along_high`.
    fn face_range(
        &self,
        face: Face,
        corners: ([f64; 3], [f64; 3]),
        along_low: f64,
        along_high: f64,
        face_bounds: &impl Fn(&FractalNoise, &VoxelBox) -> Interval,
    ) -> Interval {
        let noise = &self.faces[face.index()];
        match self.heightmap_region(face, corners, along_low, along_high) {
            Some(region) => face_bounds(noise, &region),
            None => Interval::new(-noise.largest_magnitude(), noise.largest_magnitude()),
        }
    }
}

impl DensityField for PlanetGround {
    fn sample(&self, position: VoxelCoordinate, footprint: LevelOfDetail) -> f64 {
        let middle = Self::middle(position);
        let distance =
            (middle[0] * middle[0] + middle[1] * middle[1] + middle[2] * middle[2]).sqrt();
        self.radius + self.height(middle, distance, footprint) - distance
    }

    fn bounds(&self, region: &VoxelBox, footprint: LevelOfDetail) -> Interval {
        self.bounds_by(region, |noise, heightmap| {
            noise.bounds(heightmap, footprint)
        })
    }

    fn bounds_through_finer(&self, region: &VoxelBox, footprint: LevelOfDetail) -> Interval {
        self.bounds_by(region, |noise, heightmap| {
            noise.bounds_through_finer(heightmap, footprint)
        })
    }
}

/// A planet's bricks: the ground of [`PlanetGround`] with grass on the outside, then dirt, then stone, and the sheet caves of the overworld in it.
///
/// The layers are read from the depth under the surface (the value of the ground), not from the voxels above, so a cave under the surface does not turn
/// the ground beside it to grass as it would in the overworld. The levels of detail are made from the ground without the caves.
#[derive(Clone, Debug)]
pub struct PlanetGenerator {
    ground: Arc<PlanetGround>,
    caves: ShellCaves,
    material_channel: ChannelIndex,
    materials: SurfaceLayers,
}

/// The planet of `settings`: its generator.
pub fn planet(
    settings: PlanetSettings,
    material_channel: ChannelIndex,
    materials: SurfaceLayers,
) -> PlanetGenerator {
    let caves = ShellCaves::new(
        FractalNoise::standard(
            settings.seed ^ 0xCA7E_5CA7_E5CA_7E5C,
            Dimensionality::Volumetric,
            settings.cave_wavelength_log2,
            2,
            1.0,
            0.5,
        ),
        settings.cave_half_thickness,
        16.0,
    );
    PlanetGenerator {
        ground: Arc::new(PlanetGround::new(&settings)),
        caves,
        material_channel,
        materials,
    }
}

impl PlanetGenerator {
    pub fn ground(&self) -> &PlanetGround {
        &self.ground
    }

    /// The material of a voxel that is `depth` voxels under the surface (the value of the ground), when it is ground.
    fn layer(&self, depth: f64) -> u32 {
        if depth <= 1.0 {
            self.materials.surface
        } else if depth <= SURFACE_LAYER_DEPTH as f64 {
            self.materials.subsurface
        } else {
            self.materials.deep
        }
    }
}

impl TerrainGenerator for PlanetGenerator {
    fn generate_brick(&self, layout: &GridLayout, coordinate: BrickCoordinate) -> Brick {
        let detail = LevelOfDetail::FULL_DETAIL;
        let mut brick = Brick::new(layout);
        let region = VoxelBox::of_brick(coordinate);
        let uniform = |brick: &mut Brick, material: u32| {
            brick.channel_mut(self.material_channel).fill(material);
        };

        let ground = self.ground.bounds(&region, detail);
        if ground.upper() <= 0.0 {
            uniform(&mut brick, self.materials.air);
            return brick;
        }
        let caves = self.caves.bounds(&region, detail);
        let below_the_layers = ground.lower() > SURFACE_LAYER_DEPTH as f64;
        if below_the_layers && caves.lower() > 0.0 {
            uniform(&mut brick, self.materials.deep);
            return brick;
        }
        let may_have_caves = caves.lower() <= 0.0;

        let origin = coordinate.origin();
        let mut materials = vec![self.materials.air; BRICK_VOXEL_COUNT];
        for z in 0..BRICK_EDGE_LENGTH {
            for y in 0..BRICK_EDGE_LENGTH {
                for x in 0..BRICK_EDGE_LENGTH {
                    let Some(position) = origin.checked_offset(x as i128, y as i128, z as i128)
                    else {
                        continue;
                    };
                    if may_have_caves && self.caves.sample(position, detail) <= 0.0 {
                        continue;
                    }
                    let local = LocalVoxelIndex::new(x as u8, y as u8, z as u8);
                    if below_the_layers {
                        materials[local.index()] = self.materials.deep;
                        continue;
                    }
                    let depth = self.ground.sample(position, detail);
                    if depth > 0.0 {
                        materials[local.index()] = self.layer(depth);
                    }
                }
            }
        }
        *brick.channel_mut(self.material_channel) = ChannelStorage::from_values(&materials);
        brick
    }

    fn coarse_node(&self, cube: BrickCube, unlit: u8) -> Option<CoarseNode> {
        crate::generators::planet_node::coarse_node_of_planet(
            &self.ground,
            self.materials,
            cube,
            unlit,
        )
    }

    fn may_show(&self, cube: BrickCube) -> bool {
        terrain::cube_may_show(&*self.ground, cube)
    }

    fn solid_core(&self) -> Option<SolidBall> {
        let radius = self.ground.core_radius();
        (radius > 0.0).then_some(SolidBall {
            centre: [0.0; 3],
            radius,
        })
    }
}

/// Planets and materials that the tests of the planet's generator and of its coarse nodes share.
#[cfg(test)]
pub(crate) mod test_fixtures {
    use super::*;

    pub(crate) const MATERIALS: SurfaceLayers = SurfaceLayers {
        air: 0,
        deep: 1,
        subsurface: 2,
        surface: 3,
    };

    /// A small planet, so that tests reach every seam and corner with few samples: a radius of 500 voxels, hills 20 voxels tall over 64 voxels at the most.
    pub(crate) fn small_planet(seed: u64) -> PlanetSettings {
        PlanetSettings {
            seed,
            radius: 500.0,
            hill_height: 20.0,
            largest_hill_wavelength_log2: 6,
            hill_octave_count: 4,
            cave_wavelength_log2: 4,
            cave_half_thickness: 0.08,
        }
    }

    /// A planet whose hills are nothing and whose caves are nothing: a ball.
    pub(crate) fn ball(radius: f64) -> PlanetSettings {
        PlanetSettings {
            radius,
            hill_octave_count: 0,
            cave_half_thickness: 0.0,
            ..small_planet(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use voxel_grid::test_support::StandardTestLayout;
    use voxel_space::test_support::local_voxel_index;

    use super::test_fixtures::{MATERIALS, ball, small_planet};
    use super::*;

    const FULL: LevelOfDetail = LevelOfDetail::FULL_DETAIL;

    fn voxel(x: i128, y: i128, z: i128) -> VoxelCoordinate {
        VoxelCoordinate::new(x, y, z)
    }

    fn generator_of(settings: PlanetSettings, fixture: &StandardTestLayout) -> PlanetGenerator {
        planet(settings, fixture.material, MATERIALS)
    }

    // ---- the angle across a face ----

    #[test]
    fn the_polynomial_arctangent_is_close_to_the_real_one() {
        // Values of the arctangent from a table, not from the standard library (whose result may differ between platforms, which is why it is not used).
        let table = [
            (0.0, 0.0),
            (0.1, 0.099_668_652_491_162_04),
            (0.25, 0.244_978_663_126_864_4),
            (0.5, 0.463_647_609_000_806_1),
            (0.75, 0.643_501_108_793_284_4),
            (1.0, std::f64::consts::FRAC_PI_4),
        ];
        for (x, expected) in table {
            assert!((arctangent(x) - expected).abs() < 2.0e-5, "at {x}");
            assert_eq!(arctangent(-x), -arctangent(x), "odd at {x}");
        }
    }

    #[test]
    fn beyond_one_the_arctangent_goes_on_in_a_straight_line_that_touches_it_at_one() {
        let step = 1.0e-6;
        let slope_inside = (arctangent(1.0) - arctangent(1.0 - step)) / step;
        let slope_outside = (arctangent(1.0 + step) - arctangent(1.0)) / step;
        assert!(
            (slope_inside - slope_outside).abs() < 1.0e-4,
            "{slope_inside} against {slope_outside}"
        );
        // Straight: the same slope a third of the way and two thirds of the way to 1.3.
        let first = arctangent(1.1) - arctangent(1.0);
        let second = arctangent(1.2) - arctangent(1.1);
        let third = arctangent(1.3) - arctangent(1.2);
        assert!((first - second).abs() < 1.0e-12 && (second - third).abs() < 1.0e-12);
    }

    #[test]
    fn a_face_is_never_read_at_a_ratio_past_the_largest_that_bounds_ask_about() {
        // The least the greatest share can be is the direction along a diagonal of the cube, 1 / sqrt(3); a face counts down to BLEND_BAND under it.
        let least_greatest = 1.0 / 3.0f64.sqrt();
        let most = least_greatest / (least_greatest - BLEND_BAND);
        assert!(most < LARGEST_RATIO_READ, "{most}");
        // And a scan of directions agrees: every face with weight has the ratio of its lateral distances to its own under the bound.
        let steps = 40;
        let mut counted = 0;
        for i in 0..=steps {
            for j in 0..=steps {
                for k in 0..=steps {
                    let at = |n: i32| f64::from(n) / f64::from(steps) * 2.0 - 1.0;
                    let position = [at(i), at(j), at(k)];
                    let distance = (position[0] * position[0]
                        + position[1] * position[1]
                        + position[2] * position[2])
                        .sqrt();
                    if distance == 0.0 {
                        continue;
                    }
                    let weights = PlanetGround::blend_weights(position, distance);
                    for axis in 0..3 {
                        if weights[axis] > 0.0 {
                            for lateral in [(axis + 1) % 3, (axis + 2) % 3] {
                                let ratio = position[lateral].abs() / position[axis].abs();
                                assert!(ratio <= most + 1e-9, "{ratio} at {position:?}");
                                counted += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(counted > 10_000, "{counted}");
    }

    #[test]
    fn the_arctangent_only_increases() {
        let mut previous = arctangent(-1.5);
        for step in 1..=3000 {
            let value = arctangent(-1.5 + step as f64 * 0.001);
            assert!(value > previous, "step {step}");
            previous = value;
        }
    }

    // ---- the shape ----

    #[test]
    fn without_hills_the_ground_is_the_ball_of_voxels_whose_middles_are_inside_the_radius() {
        let radius = 60;
        let ground = PlanetGround::new(&ball(radius as f64));
        for z in -66i128..66 {
            for y in -66i128..66 {
                for x in -66i128..66 {
                    // The middle of a voxel is `coordinate + 1/2`, so twice its distance squared is an integer.
                    let twice_squared =
                        (2 * x + 1).pow(2) + (2 * y + 1).pow(2) + (2 * z + 1).pow(2);
                    let inside = twice_squared < 4 * radius * radius;
                    assert_eq!(
                        ground.sample(voxel(x, y, z), FULL) > 0.0,
                        inside,
                        "at {x}, {y}, {z}"
                    );
                }
            }
        }
    }

    #[test]
    fn without_hills_the_value_is_the_depth_under_the_surface() {
        let ground = PlanetGround::new(&ball(100.0));
        // Along an axis the middle of the voxel at x is x + 1/2 away, give or take the half voxel across.
        let depth = ground.sample(voxel(79, 0, 0), FULL);
        assert!((depth - (100.0 - 79.5)).abs() < 0.01, "{depth}");
        let depth = ground.sample(voxel(-1, -61, -1), FULL);
        assert!((depth - (100.0 - 60.5)).abs() < 0.01, "{depth}");
    }

    #[test]
    fn the_hills_move_the_surface_both_ways_and_by_at_most_what_they_can() {
        let settings = small_planet(3);
        let ground = PlanetGround::new(&settings);
        // The amplitudes are 20, 10, 5 and 2.5, so a face reaches 37.5 either way, and the blend of three faces (squares of weights adding to one)
        // reaches the square root of three times that.
        let (mut highest, mut lowest) = (f64::MIN, f64::MAX);
        for step in 0..6000i128 {
            // Points on a spiral of directions, on the radius where the hills contribute nothing.
            let t = step as f64 / 6000.0;
            let direction = [
                1.0 - 2.0 * t,
                ((step * 7919) % 2000) as f64 / 1000.0 - 1.0,
                ((step * 104_729) % 2000) as f64 / 1000.0 - 1.0,
            ];
            let length = (direction[0] * direction[0]
                + direction[1] * direction[1]
                + direction[2] * direction[2])
                .sqrt()
                .max(0.01);
            let position = direction.map(|component| component / length * 500.0);
            let height = ground.height(position, 500.0, FULL);
            highest = highest.max(height);
            lowest = lowest.min(height);
        }
        assert!(highest > 10.0 && lowest < -10.0, "{lowest} to {highest}");
        let reach = 37.5 * 3.0f64.sqrt() + 1e-9;
        assert!(highest <= reach && lowest >= -reach);
    }

    /// The change of the height along a path on the sphere between two points, in steps of about half a voxel along the sphere.
    fn greatest_step(
        ground: &PlanetGround,
        radius: f64,
        from: [f64; 3],
        to: [f64; 3],
        height: impl Fn(&PlanetGround, [f64; 3], f64) -> f64,
    ) -> f64 {
        let unit = |v: [f64; 3]| {
            let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            v.map(|component| component / length)
        };
        let (from, to) = (unit(from), unit(to));
        let chord = {
            let d = [from[0] - to[0], from[1] - to[1], from[2] - to[2]];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        };
        let steps = (chord * radius * 2.0) as usize;
        let mut previous: Option<f64> = None;
        let mut greatest = 0.0f64;
        for step in 0..=steps {
            let t = step as f64 / steps as f64;
            let point = unit([
                from[0] + (to[0] - from[0]) * t,
                from[1] + (to[1] - from[1]) * t,
                from[2] + (to[2] - from[2]) * t,
            ])
            .map(|component| component * radius);
            let value = height(ground, point, radius);
            if let Some(previous) = previous {
                greatest = greatest.max((value - previous).abs());
            }
            previous = Some(value);
        }
        greatest
    }

    /// The height from the face with the greatest share alone, which is what the planet would be without the blend.
    fn height_of_the_greatest_face_alone(ground: &PlanetGround, position: [f64; 3], _: f64) -> f64 {
        let axis = (0..3)
            .max_by(|&a, &b| position[a].abs().total_cmp(&position[b].abs()))
            .unwrap();
        let face = Face {
            axis,
            negative: position[axis] < 0.0,
        };
        ground.face_height(face, position, FULL)
    }

    #[test]
    fn the_blend_leaves_no_step_where_faces_meet_that_the_faces_alone_would_have() {
        let ground = PlanetGround::new(&small_planet(11));
        let blended = |ground: &PlanetGround, position: [f64; 3], radius: f64| {
            ground.height(position, radius, FULL)
        };
        // Paths that cross an edge between two faces, an edge a little off the middle of it, and the corner where three meet, and paths that cross no seam.
        let crossings = [
            ([1.0, 0.8, 0.1], [0.8, 1.0, 0.1]),
            ([1.0, -0.6, 0.45], [0.6, -1.0, 0.43]),
            ([1.0, 0.9, 0.85], [0.85, 1.0, 0.9]),
            ([0.7, 0.9, 1.0], [1.0, 0.9, 0.7]),
            ([-1.0, 0.3, -0.7], [-0.7, 0.3, -1.0]),
        ];
        let within_a_face = [
            ([1.0, -0.3, 0.2], [1.0, 0.3, -0.2]),
            ([0.1, 1.0, 0.2], [-0.2, 1.0, -0.3]),
        ];
        let calm = within_a_face
            .iter()
            .map(|&(from, to)| greatest_step(&ground, 500.0, from, to, blended))
            .fold(0.0, f64::max);
        assert!(calm > 0.0);
        let mut greatest_jump_without_the_blend = 0.0f64;
        for (from, to) in crossings {
            let with_blend = greatest_step(&ground, 500.0, from, to, blended);
            // The blend changes the height no faster than the noise does within a face (a little more is allowed for the weights changing).
            assert!(
                with_blend < 2.0 * calm,
                "{from:?} to {to:?}: step {with_blend} against {calm} within a face"
            );
            greatest_jump_without_the_blend = greatest_jump_without_the_blend.max(greatest_step(
                &ground,
                500.0,
                from,
                to,
                height_of_the_greatest_face_alone,
            ));
        }
        // And the test can tell: without the blend the faces disagree where they meet (by how much depends on where; at some crossing it is a lot).
        assert!(
            greatest_jump_without_the_blend > 4.0 * calm,
            "the faces alone jump {greatest_jump_without_the_blend} at the worst of the seams, against {calm} within a face"
        );
    }

    #[test]
    fn the_blend_keeps_the_spread_of_the_noise_where_faces_meet() {
        // Small features (wavelengths 16, 8 and 4) on a big sphere, so that a few thousand points are many independent values.
        let settings = PlanetSettings {
            radius: 2000.0,
            hill_height: 10.0,
            largest_hill_wavelength_log2: 4,
            hill_octave_count: 3,
            ..small_planet(21)
        };
        let ground = PlanetGround::new(&settings);
        let (mut near_seam, mut far_from_seam) = (Vec::new(), Vec::new());
        let grid = 500i128;
        for face_axis in 0..3 {
            for s in 0..grid {
                for t in 0..grid {
                    let on_face = (
                        2.0 * (s as f64 + 0.37) / grid as f64 - 1.0,
                        2.0 * (t as f64 + 0.61) / grid as f64 - 1.0,
                    );
                    let mut direction = [0.0; 3];
                    direction[face_axis] = 1.0;
                    direction[(face_axis + 1) % 3] = on_face.0;
                    direction[(face_axis + 2) % 3] = on_face.1;
                    let length = (direction[0] * direction[0]
                        + direction[1] * direction[1]
                        + direction[2] * direction[2])
                        .sqrt();
                    let position = direction.map(|component| component / length * 2000.0);
                    let mut shares = position.map(f64::abs).map(|component| component / 2000.0);
                    shares.sort_by(f64::total_cmp);
                    let gap = shares[2] - shares[1];
                    let height = ground.height(position, 2000.0, FULL);
                    if gap < BLEND_BAND / 3.0 {
                        near_seam.push(height);
                    } else if gap > BLEND_BAND {
                        far_from_seam.push(height);
                    }
                }
            }
        }
        let variance = |values: &[f64]| {
            let mean = values.iter().sum::<f64>() / values.len() as f64;
            values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / values.len() as f64
        };
        assert!(near_seam.len() > 10_000 && far_from_seam.len() > 10_000);
        let ratio = variance(&near_seam) / variance(&far_from_seam);
        // A plain average of two independent faces would have half the variance.
        assert!(
            (0.85..1.15).contains(&ratio),
            "the spread near seams is {ratio} times the spread elsewhere"
        );
    }

    // ---- bounds ----

    /// A box about `center` with the given edges, if the world has the room.
    fn box_around(center: [i128; 3], edges: [i128; 3]) -> VoxelBox {
        let low = voxel(
            center[0] - edges[0] / 2,
            center[1] - edges[1] / 2,
            center[2] - edges[2] / 2,
        );
        let high = voxel(
            low.x() + edges[0] - 1,
            low.y() + edges[1] - 1,
            low.z() + edges[2] - 1,
        );
        VoxelBox::new(low, high).unwrap()
    }

    /// The voxels to ask a box's bounds about: its eight corners, where a bound is most likely to be a hair short, and the ones in between that `probes` choose.
    fn probe_positions(region: &VoxelBox, probes: &[[f64; 3]]) -> Vec<VoxelCoordinate> {
        let (low, high) = (region.min(), region.max());
        let mut positions = Vec::new();
        for corner in 0..8 {
            positions.push(voxel(
                if corner & 1 == 0 { low.x() } else { high.x() },
                if corner & 2 == 0 { low.y() } else { high.y() },
                if corner & 4 == 0 { low.z() } else { high.z() },
            ));
        }
        for probe in probes {
            let at = |axis: usize, low: i128, high: i128| {
                low + ((high - low) as f64 * probe[axis]) as i128
            };
            positions.push(voxel(
                at(0, low.x(), high.x()),
                at(1, low.y(), high.y()),
                at(2, low.z(), high.z()),
            ));
        }
        positions
    }

    /// A centre near a seam between two faces of the cube, or the corner where three meet, within 70 voxels of the surface of the small planet: the share of
    /// one axis is within 0.12 of the other's.
    fn near_a_seam() -> impl Strategy<Value = [i128; 3]> {
        (
            0usize..3,
            [any::<bool>(), any::<bool>()],
            0.3f64..1.0,
            -0.12f64..0.12,
            -1.0f64..1.0,
            -70i128..70,
        )
            .prop_map(|(axis, negative, magnitude, difference, third, depth)| {
                let mut direction = [0.0; 3];
                direction[axis] = if negative[0] { -magnitude } else { magnitude };
                direction[(axis + 1) % 3] = if negative[1] {
                    -(magnitude + difference)
                } else {
                    magnitude + difference
                };
                direction[(axis + 2) % 3] = third * magnitude;
                let length = (direction[0] * direction[0]
                    + direction[1] * direction[1]
                    + direction[2] * direction[2])
                    .sqrt();
                direction.map(|component| (component / length * (500.0 + depth as f64)) as i128)
            })
    }

    /// A centre near the surface of the small planet, in any direction.
    fn near_the_surface() -> impl Strategy<Value = [i128; 3]> {
        (
            [-1000i128..1000, -1000i128..1000, -1000i128..1000],
            -70i128..70,
        )
            .prop_filter_map("a direction", |(direction, depth)| {
                let length = ((direction[0].pow(2) + direction[1].pow(2) + direction[2].pow(2))
                    as f64)
                    .sqrt();
                (length > 50.0).then(|| {
                    direction.map(|c| (c as f64 / length * (500.0 + depth as f64)) as i128)
                })
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(300))]

        #[test]
        fn the_bounds_hold_every_sample_in_the_box_at_every_detail(
            seed in any::<u64>(),
            center in near_the_surface(),
            edges in [1i128..120, 1i128..120, 1i128..120],
            probes in proptest::collection::vec([0.0f64..1.0, 0.0f64..1.0, 0.0f64..1.0], 24),
            level in 0u8..5,
        ) {
            let ground = PlanetGround::new(&small_planet(seed));
            let region = box_around(center, edges);
            let detail = LevelOfDetail::new(level).unwrap();
            let bounds = ground.bounds(&region, detail);
            let through_finer = ground.bounds_through_finer(&region, detail);
            for position in probe_positions(&region, &probes) {
                let value = ground.sample(position, detail);
                prop_assert!(bounds.contains(value), "{value} at {position:?} outside {bounds:?} of {region:?}");
                for finer in 0..=level {
                    let value = ground.sample(position, LevelOfDetail::new(finer).unwrap());
                    prop_assert!(
                        through_finer.contains(value),
                        "{value} at detail {finer} at {position:?} outside {through_finer:?} of {region:?}"
                    );
                }
            }
        }

        /// Boxes of a few voxels at the seams, where the bound is a combination of faces' and there is almost no slack from the size of the box to hide a bound
        /// that is a little short; and at coarse detail, where the finer octaves are dropped from the bound but are in the sample at the finer detail.
        #[test]
        fn the_bounds_of_tiny_boxes_at_the_seams_hold_the_samples_at_every_detail(
            seed in any::<u64>(),
            center in near_a_seam(),
            edges in [1i128..7, 1i128..7, 1i128..7],
            level in 0u8..8,
        ) {
            let ground = PlanetGround::new(&small_planet(seed));
            let region = box_around(center, edges);
            let detail = LevelOfDetail::new(level).unwrap();
            let bounds = ground.bounds(&region, detail);
            let through_finer = ground.bounds_through_finer(&region, detail);
            for position in probe_positions(&region, &[[0.5, 0.5, 0.5]]) {
                let value = ground.sample(position, detail);
                prop_assert!(bounds.contains(value), "{value} at {position:?} outside {bounds:?} of {region:?}");
                for finer in 0..=level {
                    let value = ground.sample(position, LevelOfDetail::new(finer).unwrap());
                    prop_assert!(
                        through_finer.contains(value),
                        "{value} at detail {finer} at {position:?} outside {through_finer:?} of {region:?}"
                    );
                }
            }
        }

        #[test]
        fn the_bounds_of_boxes_that_hold_the_centre_or_stretch_far_still_hold(
            seed in any::<u64>(),
            center in [-3000i128..3000, -3000i128..3000, -3000i128..3000],
            edges in [1i128..4000, 1i128..4000, 1i128..4000],
            probes in proptest::collection::vec([0.0f64..1.0, 0.0f64..1.0, 0.0f64..1.0], 24),
        ) {
            let ground = PlanetGround::new(&small_planet(seed));
            let region = box_around(center, edges);
            let bounds = ground.bounds(&region, FULL);
            for position in probe_positions(&region, &probes) {
                let value = ground.sample(position, FULL);
                prop_assert!(bounds.contains(value), "{value} at {position:?} outside {bounds:?} of {region:?}");
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(300))]

        /// The part of a heightmap that a bound looks at holds the place on the heightmap of every point of the box where the face counts. (The bounds of the
        /// noise over a part that is a little short stay inside the slack of the lattice, so the property tests of the bounds do not see it; this does.)
        #[test]
        fn the_part_of_the_heightmap_a_box_is_read_from_holds_every_point_of_it(
            seed in any::<u64>(),
            center in near_the_surface(),
            edges in [1i128..200, 1i128..200, 1i128..200],
            probes in proptest::collection::vec([0.0f64..1.0, 0.0f64..1.0, 0.0f64..1.0], 24),
        ) {
            let ground = PlanetGround::new(&small_planet(seed));
            let region = box_around(center, edges);
            let (low, high) = (PlanetGround::middle(region.min()), PlanetGround::middle(region.max()));
            let mut faces_counted = 0;
            for position in probe_positions(&region, &probes) {
                let middle = PlanetGround::middle(position);
                let distance = (middle[0] * middle[0] + middle[1] * middle[1] + middle[2] * middle[2]).sqrt();
                let weights = PlanetGround::blend_weights(middle, distance);
                for (axis, &weight) in weights.iter().enumerate() {
                    if weight <= 0.0 {
                        continue;
                    }
                    let face = Face { axis, negative: middle[axis] < 0.0 };
                    let (along_low, along_high) = face.distances_along(low, high);
                    let Some(part) = ground.heightmap_region(face, (low, high), along_low, along_high) else {
                        // Only a box that reaches across the plane through the centre has none.
                        prop_assert!(along_low <= 0.0, "{face:?} of {region:?}");
                        continue;
                    };
                    faces_counted += 1;
                    let at = ground.heightmap_position(face, middle);
                    prop_assert!(part.contains(at), "{at:?} is not in {part:?}, from {position:?} in {region:?} on {face:?}");
                }
            }
            prop_assert!(faces_counted > 0);
        }
    }

    #[test]
    fn a_brick_at_the_surface_of_the_planet_has_bounds_that_say_something() {
        let ground = PlanetGround::new(&PlanetSettings::one_percent_of_earth(2024));
        // Bricks on the surface along the axes and out near a corner, where three faces blend.
        for brick in [
            BrickCoordinate::new(0, 1990, 0),
            BrickCoordinate::new(-1992, 0, -3),
            BrickCoordinate::new(1150, 1150, 1150),
            BrickCoordinate::new(-1147, 1149, 1153),
        ] {
            let bounds = ground.bounds(&VoxelBox::of_brick(brick), FULL);
            // Where two or three faces blend the bound is the root of the sum of the squares of their bounds (the weights' squares add to one), so wider.
            let limit = if brick.x().abs() == 1150 || brick.x().abs() == 1147 {
                450.0
            } else {
                160.0
            };
            assert!(
                bounds.upper() - bounds.lower() < limit,
                "{brick:?}: {bounds:?} is too wide to say if a brick has the surface in it"
            );
        }
    }

    #[test]
    fn far_away_and_deep_inside_boxes_are_classified_by_the_bounds_alone() {
        let ground = PlanetGround::new(&PlanetSettings::one_percent_of_earth(7));
        let sky = ground.bounds(&box_around([0, 70_000, 0], [64, 64, 64]), FULL);
        assert!(sky.upper() <= 0.0);
        let sky_far = ground.bounds(
            &box_around([200_000, -90_000, 31], [5000, 5000, 5000]),
            FULL,
        );
        assert!(sky_far.upper() <= 0.0);
        let core = ground.bounds(&box_around([100, -300, 20], [2000, 2000, 2000]), FULL);
        assert!(core.lower() > 0.0);
        let everything = ground.bounds(&box_around([0, 0, 0], [300_000, 300_000, 300_000]), FULL);
        assert!(everything.lower() < 0.0 && everything.upper() > 0.0);
    }

    // ---- bricks ----

    /// The material of a voxel worked out on its own from the samples.
    fn reference_material(generator: &PlanetGenerator, position: VoxelCoordinate) -> u32 {
        let depth = generator.ground.sample(position, FULL);
        if depth <= 0.0 || generator.caves.sample(position, FULL) <= 0.0 {
            MATERIALS.air
        } else if depth <= 1.0 {
            MATERIALS.surface
        } else if depth <= 4.0 {
            MATERIALS.subsurface
        } else {
            MATERIALS.deep
        }
    }

    fn brick_of(center: [i128; 3]) -> BrickCoordinate {
        BrickCoordinate::new(
            center[0].div_euclid(32),
            center[1].div_euclid(32),
            center[2].div_euclid(32),
        )
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(40))]

        #[test]
        fn generated_bricks_match_the_voxel_by_voxel_reference(
            seed in any::<u64>(),
            center in near_the_surface(),
            probes in proptest::collection::vec(local_voxel_index(), 300),
        ) {
            let fixture = StandardTestLayout::default();
            let generator = generator_of(small_planet(seed), &fixture);
            let coordinate = brick_of(center);
            let brick = generator.generate_brick(&fixture.layout, coordinate);
            for local in probes {
                let expected = reference_material(&generator, coordinate.voxel_at(local));
                prop_assert_eq!(brick.get(fixture.material, local), expected);
            }
        }
    }

    #[test]
    fn every_brick_of_a_ball_with_its_far_corner_just_inside_the_surface_has_its_layers() {
        // A brick that is all under the surface but for a few voxels of its layers is not all stone: the bounds say "ground" there (the far corner is under
        // the surface), but the voxels within four of the surface are grass and dirt. Every voxel of every such brick, against the reference.
        let fixture = StandardTestLayout::default();
        let generator = generator_of(ball(300.0), &fixture);
        let mut checked = 0;
        for z in -11i128..11 {
            for y in -11i128..11 {
                for x in -11i128..11 {
                    let far = |brick: i128| {
                        (32.0 * brick as f64 + 0.5)
                            .abs()
                            .max((32.0 * brick as f64 + 31.5).abs())
                    };
                    let far_corner = (far(x) * far(x) + far(y) * far(y) + far(z) * far(z)).sqrt();
                    if !(-1.0..5.0).contains(&(300.0 - far_corner)) {
                        continue;
                    }
                    checked += 1;
                    let coordinate = BrickCoordinate::new(x, y, z);
                    let brick = generator.generate_brick(&fixture.layout, coordinate);
                    for index in 0..BRICK_VOXEL_COUNT {
                        let local = LocalVoxelIndex::from_index(index).unwrap();
                        assert_eq!(
                            brick.get(fixture.material, local),
                            reference_material(&generator, coordinate.voxel_at(local)),
                            "{coordinate:?} at {local:?}"
                        );
                    }
                }
            }
        }
        assert!(checked > 100, "{checked}");
    }

    #[test]
    fn the_layers_are_grass_then_three_of_dirt_then_stone_looking_down_each_axis() {
        let fixture = StandardTestLayout::default();
        let generator = generator_of(ball(300.0), &fixture);
        // Straight down from the top of the ball along +y: the middles are at 299.5 and so on from the surface at 300.
        let column = |y: i128, x: i128, z: i128| {
            let coordinate = brick_of([x, y, z]);
            let local = LocalVoxelIndex::new(
                x.rem_euclid(32) as u8,
                y.rem_euclid(32) as u8,
                z.rem_euclid(32) as u8,
            );
            generator
                .generate_brick(&fixture.layout, coordinate)
                .get(fixture.material, local)
        };
        let expected = [
            (300, MATERIALS.air),
            (299, MATERIALS.surface),
            (298, MATERIALS.subsurface),
            (297, MATERIALS.subsurface),
            (296, MATERIALS.subsurface),
            (295, MATERIALS.deep),
            (200, MATERIALS.deep),
            (0, MATERIALS.deep),
        ];
        for (depth_from_zero, material) in expected {
            // +y, and the other five ways, which are the same by the symmetry of the ball (a negative coordinate n is the mirror of -n - 1).
            assert_eq!(
                column(depth_from_zero, 0, 0),
                material,
                "+y at {depth_from_zero}"
            );
            assert_eq!(
                column(-depth_from_zero - 1, 0, 0),
                material,
                "-y at {depth_from_zero}"
            );
            let along_x = {
                let coordinate = brick_of([depth_from_zero, 0, 0]);
                let local = LocalVoxelIndex::new(depth_from_zero.rem_euclid(32) as u8, 0, 0);
                generator
                    .generate_brick(&fixture.layout, coordinate)
                    .get(fixture.material, local)
            };
            assert_eq!(along_x, material, "+x at {depth_from_zero}");
        }
    }

    #[test]
    fn the_sky_and_the_deep_are_uniform_bricks() {
        let fixture = StandardTestLayout::default();
        let generator = generator_of(ball(300.0), &fixture);
        let uniform = |coordinate| {
            generator
                .generate_brick(&fixture.layout, coordinate)
                .channel(fixture.material)
                .uniform_value()
        };
        assert_eq!(uniform(BrickCoordinate::new(0, 12, 0)), Some(MATERIALS.air));
        assert_eq!(
            uniform(BrickCoordinate::new(-30, -30, 30)),
            Some(MATERIALS.air)
        );
        assert_eq!(uniform(BrickCoordinate::new(0, 0, 0)), Some(MATERIALS.deep));
        assert_eq!(
            uniform(BrickCoordinate::new(-3, 2, 4)),
            Some(MATERIALS.deep)
        );
        assert_eq!(
            uniform(BrickCoordinate::new(0, 9, 0)),
            None,
            "the surface is in this brick"
        );
    }

    #[test]
    fn a_planet_has_air_grass_dirt_stone_and_caves_near_the_top() {
        let fixture = StandardTestLayout::default();
        let generator = generator_of(small_planet(1), &fixture);
        let mut seen = [false; 4];
        let mut caves = 0usize;
        for x in -3..3 {
            for y in 10..17 {
                for z in -3..3 {
                    let brick =
                        generator.generate_brick(&fixture.layout, BrickCoordinate::new(x, y, z));
                    for value in brick.channel(fixture.material).iter() {
                        seen[value as usize] = true;
                    }
                }
            }
        }
        for x in -2..2 {
            for y in 6..12 {
                for z in -2..2 {
                    let brick =
                        generator.generate_brick(&fixture.layout, BrickCoordinate::new(x, y, z));
                    // A brick deep in the stone (all of it below the layers) that is not all stone has a cave in it.
                    let air = brick
                        .channel(fixture.material)
                        .iter()
                        .filter(|&value| value == MATERIALS.air)
                        .count();
                    caves += air;
                }
            }
        }
        assert_eq!(
            seen, [true; 4],
            "air, stone, dirt and grass should all occur"
        );
        assert!(caves > 1000, "{caves} voxels of caves deep down");
    }

    #[test]
    fn generation_is_independent_of_thread_and_repetition() {
        let fixture = StandardTestLayout::default();
        let generator = Arc::new(generator_of(small_planet(5), &fixture));
        let coordinate = BrickCoordinate::new(2, 15, -3);
        let reference = generator.generate_brick(&fixture.layout, coordinate);
        let from_threads: Vec<Brick> = std::thread::scope(|scope| {
            (0..4)
                .map(|_| scope.spawn(|| generator.generate_brick(&fixture.layout, coordinate)))
                .collect::<Vec<_>>()
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });
        for brick in from_threads {
            assert_eq!(brick, reference);
        }
    }

    // ---- what need not be drawn ----

    /// A direction from a number, spread over the sphere well enough to meet the seams and corners of the cube: the integers `0..` mixed into three components.
    fn direction_number(number: u64) -> [f64; 3] {
        let component = |salt: u64| {
            let bits = mix(number.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ salt) >> 11;
            bits as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
        };
        let direction = [component(1), component(2), component(3)];
        let length = (direction[0] * direction[0]
            + direction[1] * direction[1]
            + direction[2] * direction[2])
            .sqrt();
        direction.map(|axis| axis / length)
    }

    #[test]
    fn a_planet_says_it_has_a_core_that_is_ground_at_every_place_in_it_and_most_of_the_planet() {
        for settings in [small_planet(5), PlanetSettings::one_percent_of_earth(6)] {
            let fixture = StandardTestLayout::default();
            let radius = settings.radius;
            let generator = generator_of(settings, &fixture);
            let core = generator
                .solid_core()
                .expect("a planet says where its core is");
            assert_eq!(core.centre, [0.0; 3]);
            // Most of the planet: the hills reach no more than a few times their height down, and the hills of the planet are a few hundredths of its size at the most.
            assert!(core.radius > radius * 0.8 && core.radius < radius);
            // Ground at every sampled place in it: at its own radius, where the surface is nearest, and at fractions of it. (The directions meet seams and corners.)
            for number in 0..4000 {
                let direction = direction_number(number);
                for share in [1.0, 0.999, 0.9, 0.5, 0.0] {
                    let at = direction.map(|axis| axis * core.radius * share);
                    let value = generator.ground().sample(
                        voxel(
                            at[0].floor() as i128,
                            at[1].floor() as i128,
                            at[2].floor() as i128,
                        ),
                        FULL,
                    );
                    assert!(value > 0.0, "{value} at {at:?} ({number}, {share})");
                }
            }
        }
    }

    #[test]
    fn the_core_of_a_planet_is_not_a_great_deal_smaller_than_it_has_to_be() {
        // The hills of the small planet are 20 voxels tall at the most, 40 with all the octaves, and the blend can reach about 1.7 times a face's (the squares of its weights add
        // to one): the surface is nowhere more than 70 voxels below the radius, and the core is not much deeper than that.
        let fixture = StandardTestLayout::default();
        let generator = generator_of(small_planet(5), &fixture);
        let core = generator.solid_core().unwrap();
        assert!(core.radius >= 500.0 - 80.0, "{}", core.radius);
        // And a ball, with no hills, has its whole radius but the one voxel that keeps the rounding of the middle of a voxel out of it.
        let whole_ball = generator_of(ball(300.0), &fixture).solid_core().unwrap();
        assert_eq!(whole_ball.radius, 299.0);
        // A planet so small that the hills could take its ground to nothing has no core to speak of.
        assert_eq!(generator_of(ball(1.0), &fixture).solid_core(), None);
        assert_eq!(generator_of(ball(0.5), &fixture).solid_core(), None);
        assert!(generator_of(ball(2.0), &fixture).solid_core().is_some());
    }

    #[test]
    fn a_cube_that_may_not_show_has_only_air_or_only_ground_all_round_it() {
        let fixture = StandardTestLayout::default();
        let generator = generator_of(small_planet(9), &fixture);
        let (mut hidden, mut shown) = (0, 0);
        for level in 1..=3u8 {
            let edge = 1i128 << level;
            let range = (-20i128).div_euclid(edge)..=(20i128).div_euclid(edge);
            for x in range.clone() {
                for y in range.clone() {
                    for z in range.clone() {
                        let cube = BrickCube::new(
                            BrickCoordinate::new(x * edge, y * edge, z * edge),
                            level,
                        )
                        .unwrap();
                        let sampled = |box_edge_in_voxels: i128, low: i128| {
                            // A lattice of 5 x 5 x 5 voxels over the box from `low` bricks on from the cube's, at every detail from the cube's down to full.
                            let first = cube.origin().origin();
                            let step = (box_edge_in_voxels - 1) / 4;
                            let mut values = Vec::new();
                            for i in 0..5 {
                                for j in 0..5 {
                                    for k in 0..5 {
                                        let position = first
                                            .checked_offset(
                                                low + i * step,
                                                low + j * step,
                                                low + k * step,
                                            )
                                            .unwrap();
                                        for finer in 0..=level {
                                            values.push(generator.ground.sample(
                                                position,
                                                LevelOfDetail::new(finer).unwrap(),
                                            ));
                                        }
                                    }
                                }
                            }
                            values
                        };
                        if generator.may_show(cube) {
                            shown += 1;
                            continue;
                        }
                        hidden += 1;
                        let edge_in_voxels = edge * 32;
                        let inside = sampled(edge_in_voxels, 0);
                        let all_air = inside.iter().all(|&value| value <= 0.0);
                        // Ground through the cube and the cubes round it.
                        let around = sampled(3 * edge_in_voxels, -edge_in_voxels);
                        let all_ground = around.iter().all(|&value| value > 0.0);
                        assert!(
                            all_air || all_ground,
                            "{cube:?} is not drawn but is neither air nor buried"
                        );
                    }
                }
            }
        }
        assert!(
            hidden > 3000 && shown > 150,
            "{hidden} hidden, {shown} shown"
        );
        assert!(generator.may_show(BrickCube::new(BrickCoordinate::new(0, 15, 0), 0).unwrap()));
    }

    // ---- the file format ----

    /// Where the pins look: on the axes, near the seams between faces and the corners where three meet, at negative coordinates, and at random.
    const PINNED_VOXELS: [[i128; 3]; 9] = [
        [0, 63_700, 0],
        [63_650, 100, -40],
        [-20, -63_720, 15],
        [45_000, 44_990, 3_000],
        [-36_800, 36_790, 36_810],
        [36_820, -36_815, -36_818],
        [12_345, 54_321, -23_456],
        [-50_000, -2_000, -39_000],
        [1, 1, 63_709],
    ];

    /// The field of the planet at the voxels above, as the bits of the numbers (at full detail and at detail 5). These values are the file format: if one
    /// changes, existing planets change under their players; add a new generator instead. They were produced by this code, so they pin behaviour; they are
    /// not an independent check of it. Windows and Linux agree.
    #[test]
    fn the_ground_of_the_planet_is_pinned() {
        let ground = PlanetGround::new(&PlanetSettings::one_percent_of_earth(2024));
        let observed: Vec<(u64, u64)> = PINNED_VOXELS
            .iter()
            .map(|&[x, y, z]| {
                (
                    ground.sample(voxel(x, y, z), FULL).to_bits(),
                    ground
                        .sample(voxel(x, y, z), LevelOfDetail::new(5).unwrap())
                        .to_bits(),
                )
            })
            .collect();
        eprintln!("OBSERVED {observed:?}");
        assert_eq!(observed, PINNED_GROUND);
    }

    const PINNED_GROUND: [(u64, u64); 9] = [
        (13849323064644685824, 13850166124800102400),
        (4640028671270735616, 4639965080029867520),
        (4630442861240614912, 4630571345967558656),
        (13843251951081226240, 13843456351094308864),
        (13862368130900105472, 13862361040871846144),
        (13847897247879270400, 13847458366631870464),
        (4659418075021440160, 4659416128116420496),
        (4644059207971371264, 4644040704279515904),
        (4643560923340650368, 4643532313077981824),
    ];

    /// The bricks of the planet at places with surface, caves and seams: a digest of each. Pinned like the field, and it also pins the layers.
    #[test]
    fn the_bricks_of_the_planet_are_pinned() {
        let fixture = StandardTestLayout::default();
        let generator = generator_of(PlanetSettings::one_percent_of_earth(2024), &fixture);
        let bricks = [
            BrickCoordinate::new(0, 1990, 0),
            BrickCoordinate::new(1989, 3, -2),
            BrickCoordinate::new(1406, 1388, 94),
            BrickCoordinate::new(-1147, 1147, 1147),
            BrickCoordinate::new(-12, -1988, 5),
            BrickCoordinate::new(500, 1700, -400),
        ];
        let observed: Vec<u64> = bricks
            .iter()
            .map(|&coordinate| {
                let brick = generator.generate_brick(&fixture.layout, coordinate);
                assert_eq!(
                    brick.channel(fixture.material).uniform_value(),
                    None,
                    "{coordinate} should contain terrain detail"
                );
                brick.content_digest()
            })
            .collect();
        eprintln!("OBSERVED {observed:?}");
        assert_eq!(observed, PINNED_BRICKS);
    }

    const PINNED_BRICKS: [u64; 6] = [
        15048038413391879901,
        13937117706274179919,
        6363084911726167297,
        9257172923548608195,
        11762191239010736638,
        16216277259062425412,
    ];
}
