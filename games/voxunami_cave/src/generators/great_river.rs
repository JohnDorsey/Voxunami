//! The great rivers: tubes of water that span the universe (`docs/specs/great-rivers-spec.md`).
//!
//! A river is a **path** of knots with whole-voxel positions, joined by straight **courses** (a small whole-number direction and a number of steps along it, so
//! that a knot is exact at any distance), and a **radius** at each knot that goes from knot to knot as a power of two: the log of the radius changes in a
//! straight line along a course. The water is every voxel within the radius of the path, where the radius is that of the nearest point of the course (the
//! tube is a solid of revolution about each course) and a ball of the radius of the knot closes the gap at every bend. The whole is the union of those pieces.
//!
//! **Why whole directions.** A course is `start + steps * direction` with a direction of at most 5 along an axis. The cross product of a voxel's offset from the
//! start with the direction, and their dot product, are then exact in an `i128` for any voxel of the world (a coordinate is at most 2^120; the products stay under
//! 2^125), so a tube a hundred metres across is as sharp 4 x 10^26 voxels out as at the origin. The comparison with the radius is in `f64` (IEEE operations and
//! `libm::exp2`, the same on every machine), and the bounds that classify a box are made loose by a margin of 10^-12 of the sizes they work with, which is more than
//! the rounding of the sample they bound.
//!
//! **Bounds.** The box is replaced by a ball around its middle. The distance of the box from a course is within the radius of that ball of the distance of the
//! middle, the position along the course within it too, and the radius of the tube is monotone in the position, so the value of a piece over the box lies between
//! two numbers that are cheap to find. A box that does not reach the part of the course between the ends is not touched by the piece; one that is inside the
//! ends and inside the tube at its narrowest is all water.

use coarse_cells::CoarseNode;
use region_octree::BrickCube;
use voxel_grid::{Brick, ChannelIndex, ChannelStorage, GridLayout};
use voxel_space::{
    BRICK_EDGE_LENGTH, BRICK_VOXEL_COUNT, BrickCoordinate, LevelOfDetail, LocalVoxelIndex,
    VoxelBox, VoxelCoordinate,
};

use terrain::DensityField;
use terrain::Interval;
use terrain::mix;
use terrain::{SurfaceLayers, TerrainGenerator};

/// A light year in metres, which are voxels.
pub const LIGHT_YEAR: i128 = 9_460_730_472_580_800;

/// The widest a river is: one billion light years across.
pub const WIDEST_DIAMETER: i128 = 1_000_000_000 * LIGHT_YEAR;

/// The narrowest a river is: eighty metres across.
pub const NARROWEST_DIAMETER: i128 = 80;

/// The radius of the universe, 46.5 billion light years (the owner's universe is 93 billion across).
pub const UNIVERSE_RADIUS: i128 = 46_500_000_000 * LIGHT_YEAR;

/// How far a river keeps from the origin, where the first planet is: its path stays above this height (5 billion light years), and its widest tube is half a billion
/// light years in radius.
pub const MINIMUM_HEIGHT: i128 = 5_000_000_000 * LIGHT_YEAR;

/// A course advances along x by at least this far, and at most [`LONGEST_ADVANCE`].
const SHORTEST_ADVANCE: i128 = 800_000_000 * LIGHT_YEAR;
const LONGEST_ADVANCE: i128 = 3_500_000_000 * LIGHT_YEAR;

/// The most the path may be above [`MINIMUM_HEIGHT`] where it starts.
const START_HEIGHT_RANGE: i128 = 3_000_000_000 * LIGHT_YEAR;

/// What the numbers in the bounds are made looser by, as a share of the sizes they work with, so that a sample (which is rounded) is always inside the bounds of the
/// box it is in. The rounding of a sample is about 10^-15 of those sizes.
const MARGIN_SHARE: f64 = 1.0e-12;

/// The largest a component of the direction of a course is. (With it, the products in the sums stay inside an `i128` for any place in the world.)
const LARGEST_DIRECTION_COMPONENT: i128 = 5;

fn margin(size: f64) -> f64 {
    MARGIN_SHARE * size + 1.0
}

fn unit(seed: u64, salt: u64, index: u64) -> f64 {
    (mix(mix(seed ^ salt) ^ index) >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
}

fn between(low: i128, high: i128, unit: f64) -> i128 {
    low + ((high - low) as f64 * unit) as i128
}

fn length(vector: [f64; 3]) -> f64 {
    (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt()
}

fn as_floats(vector: [i128; 3]) -> [f64; 3] {
    vector.map(|component| component as f64)
}

fn dot(a: [i128; 3], b: [i128; 3]) -> i128 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [i128; 3], b: [i128; 3]) -> [i128; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn difference(a: [i128; 3], b: [i128; 3]) -> [i128; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// A place on a river where its radius is set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiverKnot {
    pub position: [i128; 3],
    /// The base-2 logarithm of the radius, in voxels.
    pub radius_log2: f64,
    /// The direction of the course that leaves the knot (the one that arrives at the last).
    pub direction: [i128; 3],
}

impl RiverKnot {
    /// The radius of the river here, in voxels.
    pub fn radius(&self) -> f64 {
        libm::exp2(self.radius_log2)
    }

    /// A place outside the river from which this knot is in view: three radii and two hundred metres from it, at right angles to its course. The player who is
    /// put there faces the knot along the vector from the viewpoint to it.
    pub fn viewpoint(&self) -> [i128; 3] {
        // The axis along which the course goes least, crossed with the course, is at right angles to it.
        let axis = (0..3)
            .min_by_key(|&axis| self.direction[axis].abs())
            .expect("there are three axes");
        let mut unit_axis = [0i128; 3];
        unit_axis[axis] = 1;
        let sideways = as_floats(cross(self.direction, unit_axis));
        let distance = 3.0 * self.radius() + 200.0;
        let scale = distance / length(sideways);
        [
            self.position[0] + (sideways[0] * scale) as i128,
            self.position[1] + (sideways[1] * scale) as i128,
            self.position[2] + (sideways[2] * scale) as i128,
        ]
    }
}

/// A straight run between two knots: `steps` steps of `direction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Course {
    direction: [i128; 3],
    steps: i128,
}

/// One river: a chain of knots joined by courses.
#[derive(Clone, Debug, PartialEq)]
pub struct RiverPath {
    knots: Vec<RiverKnot>,
    courses: Vec<Course>,
}

impl RiverPath {
    /// The one path of the first rivers: from one edge of the universe to the other, wandering sideways but never closer to the origin than
    /// [`MINIMUM_HEIGHT`], with a radius at each knot drawn so that every power of two between the narrowest and the widest is as likely as any other, and the
    /// widest and the narrowest set exactly.
    fn across_the_universe(seed: u64) -> Self {
        const SALT_START: u64 = 0x0102_0304_0506_0708;
        const SALT_COURSE: u64 = 0x1112_1314_1516_1718;
        const SALT_RADIUS: u64 = 0x2122_2324_2526_2728;
        const SALT_EXTREMES: u64 = 0x3132_3334_3536_3738;

        let mut position = [
            -UNIVERSE_RADIUS,
            MINIMUM_HEIGHT + between(0, START_HEIGHT_RANGE, unit(seed, SALT_START, 0)),
            between(
                -UNIVERSE_RADIUS / 2,
                UNIVERSE_RADIUS / 2,
                unit(seed, SALT_START, 1),
            ),
        ];
        let mut positions = vec![position];
        let mut courses = Vec::new();
        while position[0] < UNIVERSE_RADIUS {
            let index = courses.len() as u64;
            // Along x by 3 to 5 for each step, and sideways by up to 2.
            let along = 3 + (unit(seed, SALT_COURSE, 4 * index) * 3.0) as i128;
            let mut sideways = [0, 1]
                .map(|axis| (unit(seed, SALT_COURSE, 4 * index + 1 + axis) * 5.0) as i128 - 2);
            let advance = between(
                SHORTEST_ADVANCE,
                LONGEST_ADVANCE,
                unit(seed, SALT_COURSE, 4 * index + 3),
            );
            let steps = advance / along;
            let end = |sideways: [i128; 2]| {
                [
                    position[0] + steps * along,
                    position[1] + steps * sideways[0],
                    position[2] + steps * sideways[1],
                ]
            };
            // A course that would go below the lowest height or past the edge of the universe turns the other way instead (it cannot then go past the other
            // limit: a course is shorter than the room on either side).
            let mut next = end(sideways);
            if next[1] < MINIMUM_HEIGHT || next[1] > UNIVERSE_RADIUS {
                sideways[0] = -sideways[0];
                next = end(sideways);
            }
            if next[2].abs() > UNIVERSE_RADIUS {
                sideways[1] = -sideways[1];
                next = end(sideways);
            }
            courses.push(Course {
                direction: [along, sideways[0], sideways[1]],
                steps,
            });
            position = next;
            positions.push(position);
        }

        // The radii: a power of two drawn evenly between the narrowest and the widest, except at the two knots chosen to be exactly the widest and exactly the
        // narrowest (not at an end of the path, which is past the edge of the universe).
        let widest = libm::log2((WIDEST_DIAMETER / 2) as f64);
        let narrowest = libm::log2((NARROWEST_DIAMETER / 2) as f64);
        let count = positions.len();
        let widest_at = 1 + (mix(seed ^ SALT_EXTREMES) % (count as u64 - 2)) as usize;
        let mut narrowest_at = 1 + (mix(seed ^ SALT_EXTREMES ^ 1) % (count as u64 - 2)) as usize;
        if narrowest_at == widest_at {
            narrowest_at = narrowest_at % (count - 2) + 1;
        }
        let knots = positions
            .iter()
            .enumerate()
            .map(|(index, &position)| RiverKnot {
                position,
                radius_log2: if index == widest_at {
                    widest
                } else if index == narrowest_at {
                    narrowest
                } else {
                    narrowest + (widest - narrowest) * unit(seed, SALT_RADIUS, index as u64)
                },
                direction: courses[index.min(courses.len() - 1)].direction,
            })
            .collect();
        Self { knots, courses }
    }

    pub fn knots(&self) -> &[RiverKnot] {
        &self.knots
    }

    /// The pieces the water is the union of: a ball at each knot, and a tube along each course.
    fn pieces(&self) -> impl Iterator<Item = Piece> + '_ {
        let balls = self.knots.iter().map(|knot| Piece::Ball {
            centre: knot.position,
            radius: knot.radius(),
        });
        let tubes = self
            .courses
            .iter()
            .zip(self.knots.windows(2))
            .map(|(course, ends)| {
                Piece::Tube(Tube::new(
                    ends[0].position,
                    course.direction,
                    course.steps,
                    [ends[0].radius_log2, ends[1].radius_log2],
                ))
            });
        balls.chain(tubes)
    }
}

/// A piece of a river: all the voxels within a radius of a point, or within the radius of a course at each place along it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Piece {
    Ball { centre: [i128; 3], radius: f64 },
    Tube(Tube),
}

/// A course with a radius that goes from one end to the other as a power of two.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Tube {
    start: [i128; 3],
    direction: [i128; 3],
    /// The squared length of the direction, and the length.
    direction_squared: i128,
    direction_length: f64,
    /// The dot product of the direction with the offset of the far end from the start: the largest dot product a voxel of the tube has.
    extent: i128,
    /// The base-2 logarithm of the radius at each end.
    radius_log2: [f64; 2],
}

impl Tube {
    fn new(start: [i128; 3], direction: [i128; 3], steps: i128, radius_log2: [f64; 2]) -> Self {
        assert!(
            direction
                .iter()
                .all(|component| component.abs() <= LARGEST_DIRECTION_COMPONENT),
            "the direction of a course is small"
        );
        assert!(steps > 0, "a course has steps");
        let direction_squared = dot(direction, direction);
        Self {
            start,
            direction,
            direction_squared,
            direction_length: (direction_squared as f64).sqrt(),
            extent: steps * direction_squared,
            radius_log2,
        }
    }

    /// The radius a fraction `along` of the way from one end to the other (0 to 1).
    fn radius_at(&self, along: f64) -> f64 {
        libm::exp2(self.radius_log2[0] + (self.radius_log2[1] - self.radius_log2[0]) * along)
    }
}

impl Piece {
    /// How far inside the piece a voxel is (negative outside), or `None` if the piece does not reach it.
    fn value(&self, position: [i128; 3]) -> Option<f64> {
        match self {
            Self::Ball { centre, radius } => {
                Some(radius - length(as_floats(difference(position, *centre))))
            }
            Self::Tube(tube) => {
                let offset = difference(position, tube.start);
                let along = dot(offset, tube.direction);
                if along < 0 || along > tube.extent {
                    return None;
                }
                let perpendicular =
                    length(as_floats(cross(offset, tube.direction))) / tube.direction_length;
                Some(tube.radius_at(along as f64 / tube.extent as f64) - perpendicular)
            }
        }
    }

    /// The least and greatest value over a box, loosened by the margin; `NEG_INFINITY` for both if the piece does not touch the box, and the least is
    /// `NEG_INFINITY` if the box is not all inside the part of the piece that has a value.
    fn bounds(&self, low: [i128; 3], high: [i128; 3]) -> (f64, f64) {
        // Twice the middle of the box (whole numbers), and the radius of a ball that holds the box.
        let middle_twice = [0, 1, 2].map(|axis| low[axis] + high[axis]);
        let half_diagonal = length([0, 1, 2].map(|axis| (high[axis] - low[axis]) as f64)) / 2.0;
        match self {
            Self::Ball { centre, radius } => {
                let doubled =
                    difference(middle_twice, [centre[0] * 2, centre[1] * 2, centre[2] * 2]);
                let distance = length(as_floats(doubled)) / 2.0;
                let slack = margin(radius + distance + half_diagonal);
                (
                    radius - distance - half_diagonal - slack,
                    radius - (distance - half_diagonal).max(0.0) + slack,
                )
            }
            Self::Tube(tube) => {
                let doubled = difference(
                    middle_twice,
                    [tube.start[0] * 2, tube.start[1] * 2, tube.start[2] * 2],
                );
                let along_middle = dot(doubled, tube.direction) as f64 / 2.0;
                let distance_middle = length(as_floats(cross(doubled, tube.direction)))
                    / (2.0 * tube.direction_length);
                // The dot product changes by at most the length of the direction for each voxel the box reaches.
                let reach = half_diagonal * tube.direction_length;
                let extent = tube.extent as f64;
                let slack_along = margin(along_middle.abs() + reach + extent);
                let (first, last) = (along_middle - reach, along_middle + reach);
                if last + slack_along < 0.0 || first - slack_along > extent {
                    return (f64::NEG_INFINITY, f64::NEG_INFINITY);
                }
                let fraction = |along: f64| (along / extent).clamp(0.0, 1.0);
                let (near_radius, far_radius) = (
                    tube.radius_at(fraction(first)),
                    tube.radius_at(fraction(last)),
                );
                let (smallest, largest) =
                    (near_radius.min(far_radius), near_radius.max(far_radius));
                let slack = margin(largest + distance_middle + half_diagonal);
                let upper = largest - (distance_middle - half_diagonal).max(0.0) + slack;
                let lower = if first - slack_along >= 0.0 && last + slack_along <= extent {
                    smallest - distance_middle - half_diagonal - slack
                } else {
                    f64::NEG_INFINITY
                };
                (lower, upper)
            }
        }
    }
}

/// The union of some pieces of rivers, as a density field: positive where there is water.
#[derive(Clone, Debug, PartialEq)]
pub struct RiverField {
    pieces: Vec<Piece>,
}

fn corners(region: &VoxelBox) -> ([i128; 3], [i128; 3]) {
    let (low, high) = (region.min(), region.max());
    ([low.x(), low.y(), low.z()], [high.x(), high.y(), high.z()])
}

impl RiverField {
    /// The pieces that can reach a box: the field over a box that is inside it is the same as the field of these, so a part of the work of a brick or a node is
    /// done on the few pieces near it and not on all of a river.
    fn near(&self, low: [i128; 3], high: [i128; 3]) -> Self {
        Self {
            pieces: self
                .pieces
                .iter()
                .copied()
                .filter(|piece| piece.bounds(low, high).1 > 0.0)
                .collect(),
        }
    }

    /// The pieces that can reach the cube or the cubes beside it, or all of them if the world has no room for the cubes beside it.
    fn near_cube(&self, cube: BrickCube) -> Self {
        let edge = (cube.edge_length() * BRICK_EDGE_LENGTH as u128) as i128;
        let origin = cube.origin().origin();
        let around = origin
            .checked_offset(-edge, -edge, -edge)
            .zip(origin.checked_offset(2 * edge - 1, 2 * edge - 1, 2 * edge - 1));
        match around {
            Some((low, high)) => {
                self.near([low.x(), low.y(), low.z()], [high.x(), high.y(), high.z()])
            }
            None => self.clone(),
        }
    }
}

impl DensityField for RiverField {
    fn sample(&self, position: VoxelCoordinate, _: LevelOfDetail) -> f64 {
        let position = [position.x(), position.y(), position.z()];
        self.pieces
            .iter()
            .filter_map(|piece| piece.value(position))
            .fold(f64::MIN, f64::max)
    }

    fn bounds(&self, region: &VoxelBox, _: LevelOfDetail) -> Interval {
        let (low, high) = corners(region);
        let (lower, upper) = self
            .pieces
            .iter()
            .map(|piece| piece.bounds(low, high))
            .fold(
                (f64::NEG_INFINITY, f64::NEG_INFINITY),
                |(lower, upper), (l, u)| (lower.max(l), upper.max(u)),
            );
        Interval::new(lower, upper)
    }

    /// The rivers have no features finer than any other: what is water at a level of detail is water at every one.
    fn bounds_through_finer(&self, region: &VoxelBox, footprint: LevelOfDetail) -> Interval {
        self.bounds(region, footprint)
    }
}

/// The great rivers of a world: for now one path across the universe.
#[derive(Clone, Debug, PartialEq)]
pub struct GreatRivers {
    paths: Vec<RiverPath>,
    field: RiverField,
}

impl GreatRivers {
    /// The rivers of the world with this seed.
    pub fn new(seed: u64) -> Self {
        let paths = vec![RiverPath::across_the_universe(seed)];
        let field = RiverField {
            pieces: paths.iter().flat_map(RiverPath::pieces).collect(),
        };
        Self { paths, field }
    }

    pub fn paths(&self) -> &[RiverPath] {
        &self.paths
    }

    /// Every knot of every path.
    pub fn knots(&self) -> impl Iterator<Item = &RiverKnot> {
        self.paths.iter().flat_map(|path| path.knots.iter())
    }

    /// The knot where a river is widest.
    pub fn widest_knot(&self) -> RiverKnot {
        *self
            .knots()
            .max_by(|a, b| a.radius_log2.total_cmp(&b.radius_log2))
            .expect("a river has knots")
    }

    /// The knot where a river is narrowest.
    pub fn narrowest_knot(&self) -> RiverKnot {
        *self
            .knots()
            .min_by(|a, b| a.radius_log2.total_cmp(&b.radius_log2))
            .expect("a river has knots")
    }

    /// The water of the rivers as a density field.
    pub fn field(&self) -> &RiverField {
        &self.field
    }
}

/// Makes the bricks of a level whose only content is the great rivers: water where a river is and air everywhere else. Like every generator, its output for a seed
/// is part of the file format of the worlds that use it.
#[derive(Clone, Debug)]
pub struct GreatRiversGenerator {
    rivers: GreatRivers,
    material_channel: ChannelIndex,
    air: u32,
    water: u32,
}

/// The generator of the great rivers of the world with this seed: `water` in `material_channel` where there is a river and `air` elsewhere.
pub fn great_rivers(
    seed: u64,
    material_channel: ChannelIndex,
    air: u32,
    water: u32,
) -> GreatRiversGenerator {
    GreatRiversGenerator {
        rivers: GreatRivers::new(seed),
        material_channel,
        air,
        water,
    }
}

impl GreatRiversGenerator {
    pub fn rivers(&self) -> &GreatRivers {
        &self.rivers
    }

    /// Water for every layer of the ground: a river is made of one block.
    fn materials(&self) -> SurfaceLayers {
        SurfaceLayers {
            air: self.air,
            deep: self.water,
            subsurface: self.water,
            surface: self.water,
        }
    }
}

impl TerrainGenerator for GreatRiversGenerator {
    fn generate_brick(&self, layout: &GridLayout, coordinate: BrickCoordinate) -> Brick {
        let detail = LevelOfDetail::FULL_DETAIL;
        let mut brick = Brick::new(layout);
        let region = VoxelBox::of_brick(coordinate);
        let (low, high) = corners(&region);
        let near = self.rivers.field.near(low, high);
        let bounds = near.bounds(&region, detail);
        if bounds.upper() <= 0.0 {
            brick.channel_mut(self.material_channel).fill(self.air);
            return brick;
        }
        if bounds.lower() > 0.0 {
            brick.channel_mut(self.material_channel).fill(self.water);
            return brick;
        }
        let origin = coordinate.origin();
        let mut materials = vec![self.air; BRICK_VOXEL_COUNT];
        for z in 0..BRICK_EDGE_LENGTH {
            for y in 0..BRICK_EDGE_LENGTH {
                for x in 0..BRICK_EDGE_LENGTH {
                    let position = origin
                        .checked_offset(x as i128, y as i128, z as i128)
                        .expect("a brick is inside the world");
                    if near.sample(position, detail) > 0.0 {
                        materials[LocalVoxelIndex::new(x as u8, y as u8, z as u8).index()] =
                            self.water;
                    }
                }
            }
        }
        *brick.channel_mut(self.material_channel) = ChannelStorage::from_values(&materials);
        brick
    }

    fn coarse_node(&self, cube: BrickCube, unlit: u8) -> Option<CoarseNode> {
        let near = self.rivers.field.near_cube(cube);
        terrain::coarse_node_of_field(&near, self.materials(), cube, unlit)
    }

    fn may_show(&self, cube: BrickCube) -> bool {
        let near = self.rivers.field.near_cube(cube);
        terrain::cube_may_show(&near, cube)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use voxel_grid::test_support::StandardTestLayout;
    use voxel_space::VOXEL_COORDINATE_MAXIMUM;

    use super::*;

    const AIR: u32 = 0;
    const WATER: u32 = 7;
    const DETAIL: LevelOfDetail = LevelOfDetail::FULL_DETAIL;

    fn voxel(position: [i128; 3]) -> VoxelCoordinate {
        VoxelCoordinate::new(position[0], position[1], position[2])
    }

    fn generator(seed: u64) -> (GreatRiversGenerator, StandardTestLayout) {
        let fixture = StandardTestLayout::default();
        (great_rivers(seed, fixture.material, AIR, WATER), fixture)
    }

    fn is_water(field: &RiverField, position: [i128; 3]) -> bool {
        field.sample(voxel(position), DETAIL) > 0.0
    }

    /// A field of one tube, for the tests that compare with exact arithmetic.
    fn one_tube(direction: [i128; 3], steps: i128, radius: f64) -> RiverField {
        let log2 = libm::log2(radius);
        RiverField {
            pieces: vec![Piece::Tube(Tube::new(
                [0, 0, 0],
                direction,
                steps,
                [log2, log2],
            ))],
        }
    }

    #[test]
    fn the_river_is_one_billion_light_years_across_at_its_widest_and_eighty_metres_at_its_narrowest()
     {
        for seed in [0, 1, 2024, u64::MAX] {
            let rivers = GreatRivers::new(seed);
            let widest = rivers.widest_knot().radius() * 2.0;
            let narrowest = rivers.narrowest_knot().radius() * 2.0;
            assert!(
                (widest / WIDEST_DIAMETER as f64 - 1.0).abs() < 1.0e-12,
                "seed {seed}: {widest}"
            );
            assert!(
                (narrowest - 80.0).abs() < 1.0e-9,
                "seed {seed}: {narrowest}"
            );
            assert!(narrowest < 100.0);
            for knot in rivers.knots() {
                let diameter = knot.radius() * 2.0;
                assert!(diameter >= narrowest * (1.0 - 1.0e-12));
                assert!(diameter <= widest * (1.0 + 1.0e-12));
            }
        }
    }

    #[test]
    fn the_diameters_are_spread_over_every_power_of_two_between_the_two() {
        // Eighty-three octaves from 80 metres to a billion light years: the knots of ten worlds fall in most of them (a knot is in the octave of its log).
        let mut octaves = [false; 84];
        for seed in 0..10 {
            for knot in GreatRivers::new(seed).knots() {
                octaves[(knot.radius_log2 * 2.0) as usize / 2] = true;
            }
        }
        let missing: Vec<usize> = (5..83).filter(|&octave| !octaves[octave]).collect();
        assert!(
            missing.len() <= 8,
            "octaves of the radius with no knot: {missing:?}"
        );
    }

    #[test]
    fn the_path_goes_from_one_edge_of_the_universe_to_the_other_and_stays_in_it() {
        for seed in [0, 7, 2024, 99_999] {
            let rivers = GreatRivers::new(seed);
            let path = &rivers.paths()[0];
            let knots = path.knots();
            assert_eq!(knots[0].position[0], -UNIVERSE_RADIUS);
            assert!(knots.last().unwrap().position[0] >= UNIVERSE_RADIUS);
            assert!((20..=80).contains(&knots.len()), "{} knots", knots.len());
            for knot in knots {
                assert!(knot.position[0].abs() <= 2 * UNIVERSE_RADIUS);
                assert!(
                    (MINIMUM_HEIGHT..=UNIVERSE_RADIUS).contains(&knot.position[1]),
                    "seed {seed}: {:?}",
                    knot.position
                );
                assert!(knot.position[2].abs() <= UNIVERSE_RADIUS);
            }
            // The path is a chain: each knot is where the course from the one before ends.
            for (course, ends) in path.courses.iter().zip(knots.windows(2)) {
                for axis in 0..3 {
                    assert_eq!(
                        ends[0].position[axis] + course.steps * course.direction[axis],
                        ends[1].position[axis]
                    );
                }
            }
        }
    }

    #[test]
    fn no_water_is_near_the_first_planet() {
        // Everything within four billion light years of the origin is air, by the bounds (which are sound): a box that size has no water in it, and so has none
        // of the planet's neighbourhood.
        let four_billion = 4_000_000_000 * LIGHT_YEAR;
        for seed in [0, 5, 2024, 31_337] {
            let rivers = GreatRivers::new(seed);
            let region = VoxelBox::new(
                VoxelCoordinate::new(-four_billion, -four_billion, -four_billion),
                VoxelCoordinate::new(four_billion, four_billion, four_billion),
            )
            .unwrap();
            assert!(
                rivers.field().bounds(&region, DETAIL).upper() <= 0.0,
                "seed {seed}"
            );
        }
    }

    #[test]
    fn a_river_is_a_ball_at_every_knot_and_a_tube_along_every_course() {
        let rivers = GreatRivers::new(2024);
        let path = &rivers.paths()[0];
        let balls = rivers
            .field()
            .pieces
            .iter()
            .filter(|piece| matches!(piece, Piece::Ball { .. }))
            .count();
        let tubes = rivers
            .field()
            .pieces
            .iter()
            .filter(|piece| matches!(piece, Piece::Tube(_)))
            .count();
        assert_eq!((balls, tubes), (path.knots().len(), path.knots().len() - 1));
        // A ball is the knot's place and radius.
        for knot in path.knots() {
            assert!(rivers.field().pieces.iter().any(|piece| matches!(piece,
                Piece::Ball { centre, radius } if *centre == knot.position && *radius == knot.radius())));
        }
    }

    #[test]
    fn a_seed_makes_one_river_and_another_seed_another() {
        assert_eq!(GreatRivers::new(9), GreatRivers::new(9));
        assert_ne!(GreatRivers::new(9), GreatRivers::new(10));
    }

    #[test]
    fn a_tube_along_an_axis_is_the_voxels_whose_distance_from_the_axis_is_under_the_radius() {
        // Exact arithmetic: a voxel is water when y^2 + z^2 < 5.5^2 = 30.25, i.e. when it is at most 30, and its x is from 0 to the length of the tube.
        let field = one_tube([1, 0, 0], 40, 5.5);
        for x in -3..45 {
            for y in -7..8 {
                for z in -7..8 {
                    let expected = (0..=40).contains(&x) && y * y + z * z <= 30;
                    assert_eq!(is_water(&field, [x, y, z]), expected, "({x}, {y}, {z})");
                }
            }
        }
    }

    #[test]
    fn a_tube_along_a_slanting_direction_is_exact_by_integer_arithmetic() {
        // Direction (3, 1, 2), 100 steps: along = offset . d from 0 to 100 * 14, and the squared distance from the axis is |offset x d|^2 / 14, under
        // 5.5^2 = 121/4 when 4 |offset x d|^2 < 121 * 14.
        let direction = [3i128, 1, 2];
        let field = one_tube(direction, 100, 5.5);
        let mut wet = 0;
        for x in -4..320 {
            for y in -8..110 {
                for z in -8..215 {
                    let offset = [x, y, z];
                    let along = dot(offset, direction);
                    let squared_cross: i128 = cross(offset, direction).iter().map(|c| c * c).sum();
                    let expected = (0..=1400).contains(&along) && 4 * squared_cross < 121 * 14;
                    if expected {
                        wet += 1;
                    }
                    assert_eq!(is_water(&field, offset), expected, "{offset:?}");
                }
            }
        }
        assert!(wet > 1000, "the tube has water in the box ({wet})");
    }

    #[test]
    fn a_tube_is_sharp_at_the_far_end_of_the_world() {
        // The same tube moved to 2^119 voxels out in every direction: the voxels either side of its surface are still told apart, and the products stay in range.
        let far = 1i128 << 119;
        let log2 = libm::log2(40.0);
        let field = RiverField {
            pieces: vec![Piece::Tube(Tube::new(
                [far, far, far],
                [5, -3, 4],
                1_000_000,
                [log2, log2],
            ))],
        };
        // On the axis, 7,500 steps along.
        let on_axis = [far + 5 * 7500, far - 3 * 7500, far + 4 * 7500];
        assert!(is_water(&field, on_axis));
        // The unit step along x (1, 0, 0) is 5/sqrt(50) of a metre off the axis per voxel, so thirty voxels from it along x is well outside 40, and a few are inside.
        let nudged = |amount: i128| [on_axis[0] + amount, on_axis[1], on_axis[2]];
        assert!(is_water(&field, nudged(20)));
        assert!(is_water(&field, nudged(-20)));
        assert!(!is_water(&field, nudged(100)));
        assert!(!is_water(&field, nudged(-100)));
        // The very ends of the range.
        let top = VOXEL_COORDINATE_MAXIMUM;
        for corner in [[top, top, top], [-top, -top, -top], [top, -top, top]] {
            let _ = field.sample(voxel(corner), DETAIL);
            let region = VoxelBox::new(
                voxel([-top, -top, -top]),
                voxel(corner.map(|c| c.max(-top))),
            );
            if let Some(region) = region {
                let _ = field.bounds(&region, DETAIL);
            }
        }
    }

    #[test]
    fn the_radius_changes_as_a_power_of_two_along_a_tube() {
        // From 8 to 1024 over 100 voxels along x: at a quarter of the way the radius is 8 * 2^(7/4)... exactly 2^(3 + 7/4), and the water is as wide as that.
        let field = RiverField {
            pieces: vec![Piece::Tube(Tube::new(
                [0, 0, 0],
                [1, 0, 0],
                1000,
                [3.0, 10.0],
            ))],
        };
        // (The log of the radius is 3 + 7 x / 1000.)
        for x in [0i128, 250, 500, 750, 1000] {
            let radius = libm::exp2(3.0 + 7.0 * x as f64 / 1000.0);
            let inside = (radius - 1.0).floor() as i128;
            let outside = (radius + 1.0).ceil() as i128;
            assert!(is_water(&field, [x, inside, 0]), "x = {x}, inside {inside}");
            assert!(
                !is_water(&field, [x, outside, 0]),
                "x = {x}, outside {outside}"
            );
        }
    }

    #[test]
    fn a_box_across_a_taper_is_not_called_water_by_the_wide_end() {
        // The log of the radius goes from 3 to 20 over 2,000 voxels along x: the radius is about 890 at x = 800 and about 9,400 at x = 1,200. The box from x = 800
        // to 1,200 and y = 1,000 to 1,010 is well inside the ends of the tube and has a voxel outside the tube at its narrow end (890 - 1,005 < 0), so its lower
        // bound must be at most that and it must not be called all water for the radius at the other end.
        let field = RiverField {
            pieces: vec![Piece::Tube(Tube::new(
                [0, 0, 0],
                [1, 0, 0],
                2000,
                [3.0, 20.0],
            ))],
        };
        let region = VoxelBox::new(voxel([800, 1000, 0]), voxel([1200, 1010, 0])).unwrap();
        let bounds = field.bounds(&region, DETAIL);
        let narrow_end = field.sample(voxel([800, 1005, 0]), DETAIL);
        assert!(narrow_end < 0.0);
        assert!(
            bounds.lower() <= narrow_end,
            "{} against {narrow_end}",
            bounds.lower()
        );
        // The wide end of the box is water, so the upper bound is positive.
        assert!(field.sample(voxel([1200, 1005, 0]), DETAIL) > 0.0);
        assert!(bounds.upper() > 0.0);
    }

    #[test]
    fn a_bend_has_no_gap_because_of_the_ball_at_the_knot() {
        // Two tubes of radius 20 that meet at a right angle at the origin (one along +x ending there, one along +y leaving it): the corner of the outer side is
        // water because of the ball, and the pieces alone leave it dry.
        let log2 = libm::log2(20.0);
        let arriving = Piece::Tube(Tube::new([-100, 0, 0], [1, 0, 0], 100, [log2, log2]));
        let leaving = Piece::Tube(Tube::new([0, 0, 0], [0, 1, 0], 100, [log2, log2]));
        let ball = Piece::Ball {
            centre: [0, 0, 0],
            radius: 20.0,
        };
        let tubes = RiverField {
            pieces: vec![arriving, leaving],
        };
        let joined = RiverField {
            pieces: vec![arriving, leaving, ball],
        };
        // The outer corner: x past the end of the arriving tube and y below the start of the leaving one.
        let corner = [10, -10, 0];
        assert!(!is_water(&tubes, corner));
        assert!(is_water(&joined, corner));
    }

    /// A field and a box, both made of random choices at random scales near the knots of the river of a seed, with points in the box.
    fn region_near_a_knot() -> impl Strategy<Value = (u64, usize, [i128; 3], u32, Vec<[f64; 3]>)> {
        (
            0u64..8,
            0usize..80,
            [-1i128..=1, -1..=1, -1..=1],
            0u32..90,
            proptest::collection::vec([0.0f64..1.0, 0.0..1.0, 0.0..1.0], 12),
        )
            .prop_map(|(seed, knot, signs, size_log2, points)| {
                (seed, knot, signs, size_log2, points)
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(400))]

        /// The bounds over a box hold every sample in it, at every scale from one voxel to the universe, near every kind of place on the river.
        #[test]
        fn bounds_hold_every_sample_in_the_box(
            (seed, knot, signs, size_log2, points) in region_near_a_knot(),
            offset_log2 in 0u32..90,
        ) {
            let rivers = GreatRivers::new(seed);
            let knots: Vec<RiverKnot> = rivers.knots().copied().collect();
            let knot = knots[knot % knots.len()];
            // A box of edge 2^size_log2 whose corner is a signed distance 2^offset_log2 from the knot, or exactly at it.
            let shift = |axis: usize| signs[axis] * (1i128 << offset_log2);
            let low = [0, 1, 2].map(|axis| knot.position[axis] + shift(axis));
            let edge = 1i128 << size_log2;
            let high = low.map(|corner| corner + edge - 1);
            let (Some(low_voxel), Some(high_voxel)) = (
                VoxelCoordinate::try_new(low[0], low[1], low[2]).ok(),
                VoxelCoordinate::try_new(high[0], high[1], high[2]).ok(),
            ) else {
                return Ok(());
            };
            let region = VoxelBox::new(low_voxel, high_voxel).unwrap();
            let bounds = rivers.field().bounds(&region, DETAIL);
            for point in &points {
                let at = [0, 1, 2].map(|axis| low[axis] + (point[axis] * (edge - 1) as f64) as i128);
                let value = rivers.field().sample(voxel(at), DETAIL);
                prop_assert!(
                    bounds.lower() <= value && value <= bounds.upper(),
                    "{value} not in [{}, {}] at {at:?} in the box from {low:?} of edge 2^{size_log2}",
                    bounds.lower(),
                    bounds.upper()
                );
            }
        }
    }

    #[test]
    fn bounds_say_something_at_every_scale_of_the_river() {
        // They are no use if they never say: a box a tenth of the radius across at the middle of the river is all water by the bounds, and one the same size
        // twice the radius off is all air, wherever the river is wide or narrow.
        let rivers = GreatRivers::new(2024);
        let mut said = 0;
        for knot in rivers.knots() {
            let radius = knot.radius();
            let edge = ((radius / 10.0) as i128).max(1);
            let at = |position: [i128; 3]| {
                VoxelBox::new(
                    voxel(position),
                    voxel(position.map(|component| component + edge - 1)),
                )
                .unwrap()
            };
            let inside = rivers.field().bounds(&at(knot.position), DETAIL);
            let away = knot.viewpoint();
            let outside = rivers.field().bounds(&at(away), DETAIL);
            if inside.lower() > 0.0 && outside.upper() <= 0.0 {
                said += 1;
            }
        }
        // (A knot next to a wide one has the wide one's water near its viewpoint, and says less.)
        assert!(
            said >= rivers.knots().count() * 3 / 4,
            "{said} of {}",
            rivers.knots().count()
        );
    }

    #[test]
    fn making_the_field_near_a_box_changes_no_sample_in_the_box() {
        let rivers = GreatRivers::new(2024);
        let narrow = rivers.narrowest_knot();
        let low = narrow.position.map(|component| component - 300);
        let high = narrow.position.map(|component| component + 300);
        let near = rivers.field().near(low, high);
        assert!(near.pieces.len() < rivers.field().pieces.len() / 4);
        assert!(!near.pieces.is_empty());
        for x in (-300..=300).step_by(13) {
            for y in (-300..=300).step_by(13) {
                for z in (-300..=300).step_by(13) {
                    let at = [
                        narrow.position[0] + x,
                        narrow.position[1] + y,
                        narrow.position[2] + z,
                    ];
                    assert_eq!(is_water(&near, at), is_water(rivers.field(), at), "{at:?}");
                }
            }
        }
    }

    /// The water in a line of voxels from `start`, one voxel at a time in `direction`, read from the bricks the generator makes.
    fn voxels_along(
        generator: &GreatRiversGenerator,
        fixture: &StandardTestLayout,
        start: [i128; 3],
        direction: [i128; 3],
        count: i128,
    ) -> Vec<bool> {
        (0..count)
            .map(|step| {
                let position = voxel([0, 1, 2].map(|axis| start[axis] + step * direction[axis]));
                let (brick, local) = position.split();
                let made = generator.generate_brick(&fixture.layout, brick);
                made.get(fixture.material, local) == WATER
            })
            .collect()
    }

    /// A voxel of water and one of air next to it (to the voxel, as far as `f64` places the surface), on the line from a knot toward its viewpoint: found by going
    /// out from the knot until the water ends, and then halving. The surface of a union of tubes at a bend is not where one tube's radius puts it, so the field is
    /// asked.
    fn surface_beside(field: &RiverField, knot: &RiverKnot) -> Option<([i128; 3], [i128; 3])> {
        let toward = as_floats(difference(knot.viewpoint(), knot.position));
        let norm = length(toward);
        let at = |distance: f64| {
            [0, 1, 2].map(|axis| knot.position[axis] + (toward[axis] / norm * distance) as i128)
        };
        if !is_water(field, knot.position) {
            return None;
        }
        let mut outside = 1.0;
        while is_water(field, at(outside)) {
            outside *= 2.0;
            if outside > 1.0e27 {
                return None;
            }
        }
        let mut inside = outside / 2.0;
        for _ in 0..200 {
            let middle = (inside + outside) / 2.0;
            if is_water(field, at(middle)) {
                inside = middle;
            } else {
                outside = middle;
            }
        }
        Some((at(inside), at(outside)))
    }

    #[test]
    fn the_narrowest_part_is_eighty_voxels_across_in_the_bricks() {
        let (generator, fixture) = generator(2024);
        let narrow = generator.rivers().narrowest_knot();
        // A line through the knot at right angles to its course: the viewpoint is on one, so walk from there past the knot, one voxel along the axis it is
        // furthest along.
        let view = narrow.viewpoint();
        let axis = (0..3)
            .max_by_key(|&axis| (narrow.position[axis] - view[axis]).abs())
            .unwrap();
        let mut step = [0i128; 3];
        step[axis] = (narrow.position[axis] - view[axis]).signum();
        let start = [0, 1, 2].map(|a| narrow.position[a] - 150 * step[a]);
        let line = voxels_along(&generator, &fixture, start, step, 300);
        let wet = line.iter().filter(|&&water| water).count();
        // The line goes at a slant across the tube, so it is a little longer than the diameter.
        assert!(
            (80..=100).contains(&wet),
            "{wet} voxels of water in the line across the narrowest part"
        );
        // It is one run of water with air either side.
        let first = line.iter().position(|&water| water).unwrap();
        let last = line.iter().rposition(|&water| water).unwrap();
        assert!(line[first..=last].iter().all(|&water| water));
        assert!(first > 0 && last < 299);
    }

    #[test]
    fn a_brick_in_the_middle_of_the_widest_part_is_water_and_one_far_from_everything_is_air() {
        let (generator, fixture) = generator(2024);
        let widest = generator.rivers().widest_knot();
        let (brick, _) = voxel(widest.position).split();
        let inside = generator.generate_brick(&fixture.layout, brick);
        assert_eq!(
            inside.channel(fixture.material).uniform_value(),
            Some(WATER)
        );
        let nowhere = generator.generate_brick(&fixture.layout, BrickCoordinate::new(0, 0, 0));
        assert_eq!(nowhere.channel(fixture.material).uniform_value(), Some(AIR));
        let far = generator.generate_brick(
            &fixture.layout,
            BrickCoordinate::new(-(1i128 << 114), 1 << 113, 1 << 112),
        );
        assert_eq!(far.channel(fixture.material).uniform_value(), Some(AIR));
    }

    #[test]
    fn the_surface_is_in_the_bricks_wherever_the_voxels_can_place_it() {
        // At each knot where the radius is under 2^40 voxels, the last voxel of water on the way out and the first of air are next to each other: a brick that
        // holds both is part water and part air (so the surface is sharp at the voxels and not only in the coarse cells), and two bricks hold one each (the wall
        // is then exactly between two bricks, which a wall that is flat along the grid does one time in thirty-two).
        let (generator, fixture) = generator(2024);
        let (mut together, mut apart) = (0, 0);
        for knot in generator
            .rivers()
            .knots()
            .filter(|knot| knot.radius_log2 < 40.0)
        {
            let (inside, outside) = surface_beside(generator.rivers().field(), knot)
                .expect("the water ends on the way out");
            let (water_brick, air_brick) = (voxel(inside).split().0, voxel(outside).split().0);
            let made = |brick| generator.generate_brick(&fixture.layout, brick);
            if water_brick == air_brick {
                together += 1;
                assert_eq!(
                    made(water_brick).channel(fixture.material).uniform_value(),
                    None,
                    "water and air next to each other in one brick that is not mixed, by the knot {knot:?}"
                );
            } else {
                apart += 1;
                assert!(
                    made(water_brick)
                        .channel(fixture.material)
                        .iter()
                        .any(|v| v == WATER)
                );
                assert!(
                    made(air_brick)
                        .channel(fixture.material)
                        .iter()
                        .any(|v| v == AIR)
                );
            }
        }
        assert!(
            together >= 5,
            "{together} knots under 2^40 with the surface in one brick ({apart} between two)"
        );
    }

    #[test]
    fn deep_in_the_water_nothing_shows_and_far_from_it_nothing_shows_and_on_the_surface_something_does()
     {
        let (generator, _) = generator(2024);
        let widest = generator.rivers().widest_knot();
        // A cube of 2^70 bricks (2^75 voxels) at the middle of a river that is 2^82 in radius, with all the cubes beside it.
        let deep = BrickCube::containing(voxel(widest.position).split().0, 70);
        assert!(!generator.may_show(deep));
        let node = generator.coarse_node(deep, 3).unwrap();
        assert_eq!(
            node.uniform_cell().map(|cell| (cell.block, cell.fill)),
            Some((WATER, 255))
        );
        // Far from every river.
        let nowhere = BrickCube::containing(BrickCoordinate::new(0, 0, 0), 60);
        assert!(!generator.may_show(nowhere));
        let node = generator.coarse_node(nowhere, 3).unwrap();
        assert_eq!(
            node.uniform_cell().map(|cell| (cell.block, cell.fill)),
            Some((AIR, 0))
        );
        // On the surface of the widest part, a cube at each level from 10 to 78 has it in it and may show.
        let (inside, _) = surface_beside(generator.rivers().field(), &widest).unwrap();
        let brick = voxel(inside).split().0;
        for level in (10..=78).step_by(4) {
            let cube = BrickCube::containing(brick, level);
            assert!(generator.may_show(cube), "level {level}");
        }
    }

    #[test]
    fn a_node_on_the_surface_has_water_in_some_cells_and_air_in_others_at_every_scale_where_it_is_resolved()
     {
        let (generator, _) = generator(2024);
        let mut checked = 0;
        for knot in generator.rivers().knots() {
            let Some((inside, outside)) = surface_beside(generator.rivers().field(), knot) else {
                continue;
            };
            let (brick, other) = (voxel(inside).split().0, voxel(outside).split().0);
            // From cells of one metre (or, where `f64` cannot place the surface closer than 2^(log2 r - 45), cells as big as that) to cells of a hundredth of the
            // radius.
            let radius_level = knot.radius_log2 as u8;
            let first = radius_level.saturating_sub(45).max(1);
            for level in (first..radius_level.saturating_sub(6).max(first + 1)).step_by(7) {
                let cube = BrickCube::containing(brick, level);
                // (A wall exactly between two nodes has water in one and air in the other.)
                if cube != BrickCube::containing(other, level) {
                    continue;
                }
                let node = generator.coarse_node(cube, 3).unwrap();
                let fills: Vec<u8> = node.cells().map(|cell| cell.fill).collect();
                assert!(
                    fills.iter().any(|&fill| fill >= 128) && fills.iter().any(|&fill| fill < 128),
                    "level {level} at the knot with radius 2^{}",
                    knot.radius_log2
                );
                for cell in node.cells().filter(|cell| cell.fill > 0) {
                    assert_eq!(cell.block, WATER);
                }
                checked += 1;
            }
        }
        assert!(checked >= 100, "{checked} nodes checked");
    }

    /// FNV-1a over everything that places a river: each knot's position, the log of its radius and its direction.
    fn path_digest(rivers: &GreatRivers) -> u64 {
        let mut state: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |bytes: &[u8]| {
            for &byte in bytes {
                state ^= u64::from(byte);
                state = state.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        for knot in rivers.knots() {
            for component in knot.position.iter().chain(&knot.direction) {
                feed(&component.to_le_bytes());
            }
            feed(&knot.radius_log2.to_bits().to_le_bytes());
        }
        state
    }

    /// The river of the world of seed 2024 and the bricks on its surface are the file format of the worlds that have them: if one of these changes, add a new
    /// generator version instead. They were produced by this code, so they pin behaviour and are not an independent check of it (the tests above are).
    #[test]
    fn the_river_of_the_world_of_seed_2024_is_pinned() {
        let (generator, fixture) = generator(2024);
        let rivers = generator.rivers();
        assert_eq!(rivers.knots().count(), PINNED_KNOT_COUNT);
        assert_eq!(path_digest(rivers), PINNED_PATH_DIGEST);
        // A brick on the surface at the narrowest part, at the knot nearest 2^20 voxels in radius, at the one nearest 2^40, and at the widest.
        let nearest = |log2: f64| {
            *rivers
                .knots()
                .min_by(|a, b| {
                    (a.radius_log2 - log2)
                        .abs()
                        .total_cmp(&(b.radius_log2 - log2).abs())
                })
                .unwrap()
        };
        let knots = [
            rivers.narrowest_knot(),
            nearest(20.0),
            nearest(40.0),
            rivers.widest_knot(),
        ];
        let digests: Vec<u64> = knots
            .iter()
            .map(|knot| {
                let (inside, _) = surface_beside(rivers.field(), knot).unwrap();
                generator
                    .generate_brick(&fixture.layout, voxel(inside).split().0)
                    .content_digest()
            })
            .collect();
        assert_eq!(digests, PINNED_SURFACE_BRICK_DIGESTS);
    }

    const PINNED_KNOT_COUNT: usize = 42;
    const PINNED_PATH_DIGEST: u64 = 12_461_684_530_946_746_804;
    const PINNED_SURFACE_BRICK_DIGESTS: [u64; 4] = [
        11_212_326_591_546_245_444,
        267_590_855_975_239_444,
        14_488_390_183_957_742_531,
        12_622_403_330_340_407_157,
    ];

    #[test]
    fn the_whole_world_can_be_asked_about() {
        // The root of the octree, and cubes at the edges of the world, give an answer and do not overflow.
        let (generator, _) = generator(2024);
        let root = BrickCube::containing(BrickCoordinate::new(0, 0, 0), 116);
        let _ = generator.may_show(root);
        let _ = generator.coarse_node(root, 3);
        for level in [100u8, 110, 115] {
            for corner in [
                BrickCoordinate::new(-(1i128 << 115), -(1i128 << 115), -(1i128 << 115)),
                BrickCoordinate::new((1i128 << 115) - 1, (1i128 << 115) - 1, (1i128 << 115) - 1),
            ] {
                let cube = BrickCube::containing(corner, level);
                let _ = generator.may_show(cube);
                let _ = generator.coarse_node(cube, 3);
            }
        }
    }

    #[test]
    fn generation_is_independent_of_thread_and_repetition() {
        let (generator, fixture) = generator(2024);
        let narrow = generator.rivers().narrowest_knot();
        let (inside, _) = surface_beside(generator.rivers().field(), &narrow).unwrap();
        let brick = voxel(inside).split().0;
        let reference = generator.generate_brick(&fixture.layout, brick);
        assert_eq!(reference.channel(fixture.material).uniform_value(), None);
        let from_threads: Vec<Brick> = std::thread::scope(|scope| {
            (0..4)
                .map(|_| scope.spawn(|| generator.generate_brick(&fixture.layout, brick)))
                .collect::<Vec<_>>()
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });
        for made in from_threads {
            assert_eq!(made, reference);
        }
    }
}
