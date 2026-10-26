use crate::{BRICK_EDGE_LENGTH, BrickCoordinate};

const EDGE: f32 = BRICK_EDGE_LENGTH as f32;

/// The vector from one position to another, as whole bricks and an `f32` rest: exact in bricks however far apart the positions are and however far both are from
/// the origin, which an `f32` vector of voxels cannot be. A thing that moves with something else (a frame that follows a planet) is moved by one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Displacement {
    pub bricks: [i64; 3],
    /// In voxels; any size (the difference of two offsets is less than a brick, and the sum of several may be more).
    pub offset: [f32; 3],
}

impl Displacement {
    /// Going nowhere.
    pub const NONE: Self = Self {
        bricks: [0; 3],
        offset: [0.0; 3],
    };

    /// The displacement the other way.
    pub fn negated(self) -> Self {
        Self {
            bricks: self.bricks.map(|bricks| -bricks),
            offset: self.offset.map(|component| -component),
        }
    }

    /// As plain numbers, in voxels.
    pub fn in_voxels(self) -> [f64; 3] {
        let edge = f64::from(EDGE);
        [0, 1, 2].map(|axis| self.bricks[axis] as f64 * edge + f64::from(self.offset[axis]))
    }
}

/// A continuous position in a grid: a whole brick plus an `f32` offset inside it.
///
/// A single `f32` loses precision far from the origin (a quarter-millimetre at 1 km, a whole metre
/// at 10 000 km). Keeping the large part as an exact integer brick and only the small part as a
/// float keeps full precision at any distance. Offsets are in voxel units from the brick's lowest
/// corner and are normalized into `[0, 32)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldPosition {
    brick: BrickCoordinate,
    offset: [f32; 3],
}

// A position is always finite (the constructor insists), so it equals itself.
impl Eq for WorldPosition {}

impl WorldPosition {
    /// Carries whole bricks out of `offset`, so any finite offset is accepted. Panics if the
    /// offset is not finite or the position would leave the supported coordinate range.
    pub fn new(brick: BrickCoordinate, offset: [f32; 3]) -> Self {
        assert!(
            offset.iter().all(|component| component.is_finite()),
            "position offset must be finite"
        );
        Self::try_new(brick, offset).expect("position is outside the supported coordinate range")
    }

    /// Like [`WorldPosition::new`], but `None` for an offset that is not finite or a position that would leave the supported coordinate
    /// range, which `new` panics at.
    pub fn try_new(brick: BrickCoordinate, offset: [f32; 3]) -> Option<Self> {
        let mut carries = [0i64; 3];
        let mut normalized = offset;
        for axis in 0..3 {
            if !offset[axis].is_finite() {
                return None;
            }
            let mut carry = (offset[axis] / EDGE).floor();
            let mut remainder = offset[axis] - carry * EDGE;
            // Rounding can leave the remainder exactly at the edge length.
            if remainder >= EDGE {
                remainder -= EDGE;
                carry += 1.0;
            }
            normalized[axis] = remainder;
            carries[axis] = carry as i64;
        }
        let brick = brick.checked_offset(carries[0], carries[1], carries[2])?;
        Some(Self {
            brick,
            offset: normalized,
        })
    }

    pub fn brick(self) -> BrickCoordinate {
        self.brick
    }

    /// Voxel units from the brick's lowest corner, each in `[0, 32)`.
    pub fn offset(self) -> [f32; 3] {
        self.offset
    }

    /// This position as plain numbers, in voxels from the origin of its grid: exact to a bit of the sum (the brick is exact, and an `f32` offset in an `f64`).
    pub fn in_voxels(self) -> [f64; 3] {
        let edge = f64::from(EDGE);
        [0, 1, 2].map(|axis| {
            let brick = [self.brick.x(), self.brick.y(), self.brick.z()][axis];
            brick as f64 * edge + f64::from(self.offset[axis])
        })
    }

    /// The position at these voxels from the origin of the grid: the brick they are in, and the `f32` rest. `None` for numbers that are not finite or that leave the
    /// supported coordinate range.
    pub fn try_from_voxels(position: [f64; 3]) -> Option<Self> {
        let edge = f64::from(EDGE);
        let brick = position.map(|component| (component / edge).floor());
        let offset = [0, 1, 2].map(|axis| (position[axis] - brick[axis] * edge) as f32);
        // (A cast to an integer saturates, so a number past the end of the world is a brick past it, which the coordinate refuses; a number that is not one has an
        // offset that is not, which `try_new` refuses.)
        let brick =
            BrickCoordinate::try_new(brick[0] as i64, brick[1] as i64, brick[2] as i64).ok()?;
        Self::try_new(brick, offset)
    }

    /// The position at these voxels, as [`WorldPosition::try_from_voxels`]; panics where that is `None`, as [`WorldPosition::new`] does.
    pub fn from_voxels(position: [f64; 3]) -> Self {
        assert!(
            position.iter().all(|component| component.is_finite()),
            "position must be finite"
        );
        Self::try_from_voxels(position).expect("position is outside the supported coordinate range")
    }

    /// What it takes to go from this position to `other`.
    pub fn displacement_to(self, other: Self) -> Displacement {
        Displacement {
            bricks: [
                other.brick.x() - self.brick.x(),
                other.brick.y() - self.brick.y(),
                other.brick.z() - self.brick.z(),
            ],
            offset: [0, 1, 2].map(|axis| other.offset[axis] - self.offset[axis]),
        }
    }

    /// This position moved by a displacement; `None` where that leaves the supported coordinate range (or the displacement is not a number).
    pub fn displaced(self, displacement: Displacement) -> Option<Self> {
        let brick = self.brick.checked_offset(
            displacement.bricks[0],
            displacement.bricks[1],
            displacement.bricks[2],
        )?;
        Self::try_new(
            brick,
            [0, 1, 2].map(|axis| self.offset[axis] + displacement.offset[axis]),
        )
    }

    /// This position moved by `delta` voxels.
    pub fn translated(self, delta: [f32; 3]) -> Self {
        Self::new(
            self.brick,
            [
                self.offset[0] + delta[0],
                self.offset[1] + delta[1],
                self.offset[2] + delta[2],
            ],
        )
    }

    /// The vector from `origin` to this position, in voxels. Exact for nearby positions however far
    /// both are from the grid origin, which is what camera-relative rendering relies on.
    pub fn relative_to(self, origin: Self) -> [f32; 3] {
        let component = |this: i64, other: i64, this_offset: f32, other_offset: f32| {
            ((this - other) as f64 * f64::from(EDGE) + f64::from(this_offset - other_offset)) as f32
        };
        [
            component(
                self.brick.x(),
                origin.brick.x(),
                self.offset[0],
                origin.offset[0],
            ),
            component(
                self.brick.y(),
                origin.brick.y(),
                self.offset[1],
                origin.offset[1],
            ),
            component(
                self.brick.z(),
                origin.brick.z(),
                self.offset[2],
                origin.offset[2],
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn brick(x: i64, y: i64, z: i64) -> BrickCoordinate {
        BrickCoordinate::new(x, y, z)
    }

    #[test]
    fn a_displacement_is_the_whole_bricks_between_two_positions_and_the_rest_and_takes_the_one_to_the_other()
     {
        let a = WorldPosition::new(brick(3, 4, 5), [1.0, 2.0, 3.0]);
        let b = WorldPosition::new(brick(-9, 0, 5), [30.0, 2.5, 0.0]);
        let d = a.displacement_to(b);
        assert_eq!(d.bricks, [-12, -4, 0]);
        assert_eq!(d.offset, [29.0, 0.5, -3.0]);
        assert_eq!(
            d.in_voxels(),
            [-12.0 * 32.0 + 29.0, -4.0 * 32.0 + 0.5, -3.0]
        );
        assert_eq!(a.displaced(d), Some(b));
        // And back the other way.
        assert_eq!(b.displaced(d.negated()), Some(a));
        assert_eq!(d.negated().bricks, [12, 4, 0]);
        assert_eq!(d.negated().offset, [-29.0, -0.5, 3.0]);
        // No displacement leaves a position where it is.
        assert_eq!(a.displaced(Displacement::NONE), Some(a));
        assert_eq!(a.displacement_to(a), Displacement::NONE);
    }

    #[test]
    fn a_displacement_is_exact_in_bricks_however_far_apart_the_positions_are() {
        // A hundred billion bricks and a thousand voxels: the bricks are exact and the rest is the rest.
        let near = WorldPosition::new(brick(0, 0, 0), [0.5, 0.5, 0.5]);
        let far = WorldPosition::new(
            brick(100_000_000_000, -100_000_000_000, 7),
            [31.5, 0.25, 0.5],
        );
        let d = near.displacement_to(far);
        assert_eq!(d.bricks, [100_000_000_000, -100_000_000_000, 7]);
        assert_eq!(d.offset, [31.0, -0.25, 0.0]);
        assert_eq!(near.displaced(d), Some(far));
    }

    #[test]
    fn a_displacement_past_the_end_of_the_world_gives_no_position() {
        use crate::BRICK_COORDINATE_MAXIMUM;
        let last = WorldPosition::new(brick(BRICK_COORDINATE_MAXIMUM, 0, 0), [0.0; 3]);
        let one_brick = Displacement {
            bricks: [1, 0, 0],
            offset: [0.0; 3],
        };
        assert_eq!(last.displaced(one_brick), None);
        let a_voxel_short = Displacement {
            bricks: [0, 0, 0],
            offset: [31.0, 0.0, 0.0],
        };
        assert!(last.displaced(a_voxel_short).is_some());
        let too_many_bricks = Displacement {
            bricks: [i64::MAX, 0, 0],
            offset: [0.0; 3],
        };
        assert_eq!(last.displaced(too_many_bricks), None);
        let not_a_number = Displacement {
            bricks: [0; 3],
            offset: [f32::NAN, 0.0, 0.0],
        };
        assert_eq!(last.displaced(not_a_number), None);
    }

    #[test]
    fn a_position_in_voxels_is_the_brick_and_the_offset_and_comes_back_from_them() {
        let position = WorldPosition::new(brick(12_500, -3, 7), [4.5, 0.0, 31.75]);
        assert_eq!(position.in_voxels(), [400_004.5, -96.0, 255.75]);
        assert_eq!(WorldPosition::from_voxels(position.in_voxels()), position);
        // Below zero the brick is the one below: -40 voxels is brick -2, 24 in.
        let below = WorldPosition::from_voxels([100.0, 0.0, -40.0]);
        assert_eq!(below.brick(), brick(3, 0, -2));
        assert_eq!(below.offset(), [4.0, 0.0, 24.0]);
        // The edge of a brick belongs to the brick above it.
        assert_eq!(
            WorldPosition::from_voxels([32.0, -32.0, 0.0]).brick(),
            brick(1, -1, 0)
        );
        // Far from the origin the brick is exact: a billion voxels and a quarter.
        let far = WorldPosition::from_voxels([1.0e9 + 0.25, 0.0, 0.0]);
        assert_eq!(far.brick(), brick(31_250_000, 0, 0));
        assert_eq!(far.offset()[0], 0.25);
    }

    #[test]
    fn voxels_that_are_past_the_end_of_the_world_or_not_numbers_are_no_position_to_try() {
        assert_eq!(
            WorldPosition::try_from_voxels([100.0, 0.0, -40.0]),
            Some(WorldPosition::from_voxels([100.0, 0.0, -40.0]))
        );
        assert_eq!(WorldPosition::try_from_voxels([f64::NAN, 0.0, 0.0]), None);
        assert_eq!(
            WorldPosition::try_from_voxels([0.0, f64::INFINITY, 0.0]),
            None
        );
        assert_eq!(WorldPosition::try_from_voxels([0.0, 0.0, 1.0e300]), None);
        assert_eq!(WorldPosition::try_from_voxels([-1.0e300, 0.0, 0.0]), None);
    }

    #[test]
    #[should_panic(expected = "position must be finite")]
    fn voxels_that_are_not_numbers_are_not_a_position() {
        let _ = WorldPosition::from_voxels([f64::NAN, 0.0, 0.0]);
    }

    #[test]
    fn a_position_that_cannot_be_made_is_none_from_try_new_and_the_same_as_new_when_it_can() {
        use crate::BRICK_COORDINATE_MAXIMUM;
        let last = brick(BRICK_COORDINATE_MAXIMUM, 0, 0);
        // Past the end of the world, by a carry; or not a number at all.
        assert_eq!(WorldPosition::try_new(last, [32.0, 0.0, 0.0]), None);
        assert_eq!(
            WorldPosition::try_new(last, [31.0, 0.0, 0.0])
                .unwrap()
                .brick(),
            last
        );
        assert_eq!(
            WorldPosition::try_new(brick(0, 0, 0), [f32::NAN, 0.0, 0.0]),
            None
        );
        assert_eq!(
            WorldPosition::try_new(brick(0, 0, 0), [0.0, f32::INFINITY, 0.0]),
            None
        );
        // Where it can be made it is what `new` makes, carries and all.
        assert_eq!(
            WorldPosition::try_new(brick(1, 2, 3), [40.0, -1.0, 64.5]),
            Some(WorldPosition::new(brick(1, 2, 3), [40.0, -1.0, 64.5]))
        );
    }

    #[test]
    fn offsets_are_carried_into_neighbouring_bricks() {
        let cases = [
            ([5.0, 6.0, 7.0], (0, 0, 0), [5.0, 6.0, 7.0]),
            ([32.0, 0.0, 0.0], (1, 0, 0), [0.0, 0.0, 0.0]),
            ([-1.0, 33.5, 64.0], (-1, 1, 2), [31.0, 1.5, 0.0]),
            ([-32.0, -33.0, 95.5], (-1, -2, 2), [0.0, 31.0, 31.5]),
        ];
        for (offset, (x, y, z), expected_offset) in cases {
            let position = WorldPosition::new(brick(0, 0, 0), offset);
            assert_eq!(position.brick(), brick(x, y, z), "{offset:?}");
            assert_eq!(position.offset(), expected_offset, "{offset:?}");
        }
    }

    #[test]
    fn a_tiny_negative_offset_never_leaves_the_remainder_at_the_edge_length() {
        let position = WorldPosition::new(brick(5, 5, 5), [-1e-9, 0.0, 0.0]);
        assert!(position.offset()[0] < EDGE);
        assert!(position.offset()[0] >= 0.0);
    }

    #[test]
    fn translating_across_a_boundary_moves_to_the_next_brick() {
        let start = WorldPosition::new(brick(0, 0, 0), [31.5, 0.0, 0.0]);
        let moved = start.translated([1.0, -0.5, 0.0]);
        assert_eq!(moved.brick(), brick(1, -1, 0));
        assert_eq!(moved.offset(), [0.5, 31.5, 0.0]);
    }

    #[test]
    #[should_panic(expected = "must be finite")]
    fn non_finite_offsets_are_rejected() {
        WorldPosition::new(brick(0, 0, 0), [f32::NAN, 0.0, 0.0]);
    }

    #[test]
    fn relative_vectors_are_exact_even_a_hundred_billion_voxels_from_the_origin() {
        let near_origin = WorldPosition::new(brick(0, 0, 0), [3.25, 7.5, 1.125]);
        let near_origin_other = WorldPosition::new(brick(2, -1, 0), [10.0, 2.0, 30.5]);
        let far = |position: WorldPosition| {
            WorldPosition::new(
                brick(
                    position.brick().x() + 100_000_000_000,
                    position.brick().y() - 100_000_000_000,
                    position.brick().z() + 5,
                ),
                position.offset(),
            )
        };
        assert_eq!(
            far(near_origin_other).relative_to(far(near_origin)),
            near_origin_other.relative_to(near_origin)
        );
        assert_eq!(
            near_origin_other.relative_to(near_origin),
            [64.0 + 6.75, -32.0 - 5.5, 29.375]
        );
    }

    #[test]
    fn a_position_is_zero_distance_from_itself_and_antisymmetric() {
        let a = WorldPosition::new(brick(3, 4, 5), [1.0, 2.0, 3.0]);
        let b = WorldPosition::new(brick(-9, 0, 5), [30.0, 2.5, 0.0]);
        assert_eq!(a.relative_to(a), [0.0; 3]);
        let forward = a.relative_to(b);
        let backward = b.relative_to(a);
        assert_eq!([-forward[0], -forward[1], -forward[2]], backward);
    }

    proptest! {
        #[test]
        fn a_position_displaced_by_the_displacement_to_another_is_that_other_to_within_a_hair(
            base in [-1000i64..1000, -1000..1000, -1000..1000],
            offset in [0.0f32..32.0, 0.0..32.0, 0.0..32.0],
            other_base in [-1000i64..1000, -1000..1000, -1000..1000],
            other_offset in [0.0f32..32.0, 0.0..32.0, 0.0..32.0],
        ) {
            let a = WorldPosition::new(brick(base[0], base[1], base[2]), offset);
            let b = WorldPosition::new(brick(other_base[0], other_base[1], other_base[2]), other_offset);
            let there = a.displaced(a.displacement_to(b)).expect("the world is large");
            let miss = there.relative_to(b);
            prop_assert!(miss.iter().all(|component| component.abs() < 1e-4), "{miss:?}");
        }
    }

    proptest! {
        #[test]
        fn normalized_offsets_stay_in_range_and_preserve_the_position(
            offset in [-5000.0f32..5000.0, -5000.0..5000.0, -5000.0..5000.0],
            base in [-1000i64..1000, -1000..1000, -1000..1000],
        ) {
            let origin = WorldPosition::new(brick(base[0], base[1], base[2]), [0.0; 3]);
            let position = WorldPosition::new(origin.brick(), offset);
            for axis in 0..3 {
                prop_assert!((0.0..EDGE).contains(&position.offset()[axis]));
            }
            let recovered = position.relative_to(origin);
            for axis in 0..3 {
                prop_assert!((recovered[axis] - offset[axis]).abs() <= 1e-3, "{recovered:?} vs {offset:?}");
            }
        }
    }
}
