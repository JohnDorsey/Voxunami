use crate::{BRICK_EDGE_LENGTH, BrickCoordinate};

const EDGE: f32 = BRICK_EDGE_LENGTH as f32;

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

impl WorldPosition {
    /// Carries whole bricks out of `offset`, so any finite offset is accepted. Panics if the
    /// offset is not finite or the position would leave the supported coordinate range.
    pub fn new(brick: BrickCoordinate, offset: [f32; 3]) -> Self {
        let mut carries = [0i64; 3];
        let mut normalized = offset;
        for axis in 0..3 {
            assert!(offset[axis].is_finite(), "position offset must be finite");
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
        let brick = brick
            .checked_offset(carries[0], carries[1], carries[2])
            .expect("position is outside the supported coordinate range");
        Self {
            brick,
            offset: normalized,
        }
    }

    pub fn brick(self) -> BrickCoordinate {
        self.brick
    }

    /// Voxel units from the brick's lowest corner, each in `[0, 32)`.
    pub fn offset(self) -> [f32; 3] {
        self.offset
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
