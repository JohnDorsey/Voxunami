use std::fmt;

use crate::{
    BRICK_COORDINATE_MAXIMUM, BRICK_COORDINATE_MINIMUM, BRICK_EDGE_LENGTH, BRICK_EDGE_LENGTH_LOG2,
    FaceDirection, LocalVoxelIndex, VOXEL_COORDINATE_MAXIMUM, VOXEL_COORDINATE_MINIMUM,
};

/// A coordinate component outside the supported range. Only produced for untrusted input
/// (files, network); the panicking constructors treat it as a bug.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("coordinate component {value} is outside the supported range {minimum}..={maximum}")]
pub struct CoordinateOutOfRange {
    pub value: i64,
    pub minimum: i64,
    pub maximum: i64,
}

fn checked_component(value: i64, minimum: i64, maximum: i64) -> Result<i64, CoordinateOutOfRange> {
    if (minimum..=maximum).contains(&value) {
        Ok(value)
    } else {
        Err(CoordinateOutOfRange {
            value,
            minimum,
            maximum,
        })
    }
}

/// The position of one voxel within a grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VoxelCoordinate {
    x: i64,
    y: i64,
    z: i64,
}

impl VoxelCoordinate {
    pub const ORIGIN: Self = Self { x: 0, y: 0, z: 0 };

    /// Panics if a component is outside `VOXEL_COORDINATE_MINIMUM..=VOXEL_COORDINATE_MAXIMUM`.
    /// Use [`VoxelCoordinate::try_new`] for untrusted input.
    pub fn new(x: i64, y: i64, z: i64) -> Self {
        Self::try_new(x, y, z).unwrap_or_else(|error| panic!("invalid voxel coordinate: {error}"))
    }

    pub fn try_new(x: i64, y: i64, z: i64) -> Result<Self, CoordinateOutOfRange> {
        let check =
            |value| checked_component(value, VOXEL_COORDINATE_MINIMUM, VOXEL_COORDINATE_MAXIMUM);
        Ok(Self {
            x: check(x)?,
            y: check(y)?,
            z: check(z)?,
        })
    }

    pub fn x(self) -> i64 {
        self.x
    }

    pub fn y(self) -> i64 {
        self.y
    }

    pub fn z(self) -> i64 {
        self.z
    }

    /// The brick containing this voxel and the voxel's position inside it. Negative coordinates
    /// floor toward negative infinity, so voxel -1 is the last voxel of brick -1.
    pub fn split(self) -> (BrickCoordinate, LocalVoxelIndex) {
        let mask = BRICK_EDGE_LENGTH as i64 - 1;
        let brick = BrickCoordinate {
            x: self.x >> BRICK_EDGE_LENGTH_LOG2,
            y: self.y >> BRICK_EDGE_LENGTH_LOG2,
            z: self.z >> BRICK_EDGE_LENGTH_LOG2,
        };
        let local = LocalVoxelIndex::new(
            (self.x & mask) as u8,
            (self.y & mask) as u8,
            (self.z & mask) as u8,
        );
        (brick, local)
    }

    /// `None` if the result would leave the supported range.
    pub fn checked_offset(self, dx: i64, dy: i64, dz: i64) -> Option<Self> {
        Self::try_new(
            self.x.checked_add(dx)?,
            self.y.checked_add(dy)?,
            self.z.checked_add(dz)?,
        )
        .ok()
    }
}

impl fmt::Display for VoxelCoordinate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "({}, {}, {})", self.x, self.y, self.z)
    }
}

/// The position of one brick within a grid, in units of whole bricks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BrickCoordinate {
    x: i64,
    y: i64,
    z: i64,
}

impl BrickCoordinate {
    pub const ORIGIN: Self = Self { x: 0, y: 0, z: 0 };

    /// Panics if a component is outside `BRICK_COORDINATE_MINIMUM..=BRICK_COORDINATE_MAXIMUM`.
    /// Use [`BrickCoordinate::try_new`] for untrusted input.
    pub fn new(x: i64, y: i64, z: i64) -> Self {
        Self::try_new(x, y, z).unwrap_or_else(|error| panic!("invalid brick coordinate: {error}"))
    }

    pub fn try_new(x: i64, y: i64, z: i64) -> Result<Self, CoordinateOutOfRange> {
        let check =
            |value| checked_component(value, BRICK_COORDINATE_MINIMUM, BRICK_COORDINATE_MAXIMUM);
        Ok(Self {
            x: check(x)?,
            y: check(y)?,
            z: check(z)?,
        })
    }

    pub fn x(self) -> i64 {
        self.x
    }

    pub fn y(self) -> i64 {
        self.y
    }

    pub fn z(self) -> i64 {
        self.z
    }

    /// The voxel at this brick's lowest corner.
    pub fn origin(self) -> VoxelCoordinate {
        self.voxel_at(LocalVoxelIndex::new(0, 0, 0))
    }

    /// The grid-wide coordinate of the voxel at `local` inside this brick.
    pub fn voxel_at(self, local: LocalVoxelIndex) -> VoxelCoordinate {
        // In range by construction: brick components are at most 2^55 in magnitude.
        VoxelCoordinate {
            x: (self.x << BRICK_EDGE_LENGTH_LOG2) + i64::from(local.x()),
            y: (self.y << BRICK_EDGE_LENGTH_LOG2) + i64::from(local.y()),
            z: (self.z << BRICK_EDGE_LENGTH_LOG2) + i64::from(local.z()),
        }
    }

    /// `None` if the result would leave the supported range.
    pub fn checked_offset(self, dx: i64, dy: i64, dz: i64) -> Option<Self> {
        Self::try_new(
            self.x.checked_add(dx)?,
            self.y.checked_add(dy)?,
            self.z.checked_add(dz)?,
        )
        .ok()
    }

    /// The brick across `face`, or `None` at the edge of the supported range.
    pub fn neighbour(self, face: FaceDirection) -> Option<Self> {
        let [dx, dy, dz] = face.offset();
        self.checked_offset(dx, dy, dz)
    }
}

impl fmt::Display for BrickCoordinate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "brick ({}, {}, {})", self.x, self.y, self.z)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::test_support::{brick_coordinate, local_voxel_index, voxel_coordinate};

    #[test]
    fn split_floors_negative_coordinates_into_the_brick_below_zero() {
        // (voxel component, expected brick component, expected local component)
        let cases = [
            (-33, -2, 31),
            (-32, -1, 0),
            (-1, -1, 31),
            (0, 0, 0),
            (31, 0, 31),
            (32, 1, 0),
            (63, 1, 31),
            (64, 2, 0),
        ];
        for (voxel, expected_brick, expected_local) in cases {
            let (brick, local) = VoxelCoordinate::new(voxel, 0, 0).split();
            assert_eq!(
                (brick.x(), local.x()),
                (expected_brick, expected_local),
                "voxel x = {voxel}"
            );
            assert_eq!((brick.y(), local.y()), (0, 0));
        }
    }

    #[test]
    fn split_handles_all_three_axes_independently() {
        let (brick, local) = VoxelCoordinate::new(-1, 32, -33).split();
        assert_eq!(brick, BrickCoordinate::new(-1, 1, -2));
        assert_eq!(local, LocalVoxelIndex::new(31, 0, 31));
    }

    #[test]
    fn try_new_accepts_the_range_limits_and_rejects_one_beyond() {
        let (minimum, maximum) = (VOXEL_COORDINATE_MINIMUM, VOXEL_COORDINATE_MAXIMUM);
        assert!(VoxelCoordinate::try_new(minimum, maximum, 0).is_ok());
        assert_eq!(
            VoxelCoordinate::try_new(minimum - 1, 0, 0),
            Err(CoordinateOutOfRange {
                value: minimum - 1,
                minimum,
                maximum
            })
        );
        assert_eq!(
            VoxelCoordinate::try_new(0, maximum + 1, 0),
            Err(CoordinateOutOfRange {
                value: maximum + 1,
                minimum,
                maximum
            })
        );
        assert!(VoxelCoordinate::try_new(0, 0, i64::MAX).is_err());
        assert!(VoxelCoordinate::try_new(i64::MIN, 0, 0).is_err());

        let (brick_minimum, brick_maximum) = (BRICK_COORDINATE_MINIMUM, BRICK_COORDINATE_MAXIMUM);
        assert!(BrickCoordinate::try_new(brick_minimum, brick_maximum, 0).is_ok());
        assert!(BrickCoordinate::try_new(brick_minimum - 1, 0, 0).is_err());
        assert!(BrickCoordinate::try_new(0, 0, brick_maximum + 1).is_err());
    }

    #[test]
    #[should_panic(expected = "invalid voxel coordinate")]
    fn new_panics_outside_the_range() {
        VoxelCoordinate::new(VOXEL_COORDINATE_MAXIMUM + 1, 0, 0);
    }

    #[test]
    fn extreme_coordinates_split_and_recombine_without_overflow() {
        for component in [VOXEL_COORDINATE_MINIMUM, VOXEL_COORDINATE_MAXIMUM] {
            let coordinate = VoxelCoordinate::new(component, component, component);
            let (brick, local) = coordinate.split();
            assert_eq!(brick.voxel_at(local), coordinate);
        }
        // The extreme bricks start and end exactly on the extreme voxels.
        let lowest = BrickCoordinate::new(BRICK_COORDINATE_MINIMUM, 0, 0);
        assert_eq!(lowest.origin().x(), VOXEL_COORDINATE_MINIMUM);
        let highest = BrickCoordinate::new(BRICK_COORDINATE_MAXIMUM, 0, 0);
        let last_local = LocalVoxelIndex::new(31, 0, 0);
        assert_eq!(highest.voxel_at(last_local).x(), VOXEL_COORDINATE_MAXIMUM);
    }

    #[test]
    fn checked_offset_stays_in_range_or_reports_none() {
        let near_edge = VoxelCoordinate::new(VOXEL_COORDINATE_MAXIMUM - 1, 0, 0);
        assert_eq!(
            near_edge.checked_offset(1, -2, 3),
            Some(VoxelCoordinate::new(VOXEL_COORDINATE_MAXIMUM, -2, 3))
        );
        assert_eq!(near_edge.checked_offset(2, 0, 0), None);
        assert_eq!(near_edge.checked_offset(i64::MAX, 0, 0), None);

        let brick = BrickCoordinate::new(BRICK_COORDINATE_MINIMUM, 0, 0);
        assert_eq!(brick.checked_offset(-1, 0, 0), None);
        assert_eq!(
            brick.checked_offset(1, 0, 0),
            Some(BrickCoordinate::new(BRICK_COORDINATE_MINIMUM + 1, 0, 0))
        );
    }

    #[test]
    fn neighbours_step_one_brick_and_stop_at_the_range_edge() {
        let brick = BrickCoordinate::new(0, -3, 7);
        assert_eq!(
            brick.neighbour(FaceDirection::PositiveX),
            Some(BrickCoordinate::new(1, -3, 7))
        );
        assert_eq!(
            brick.neighbour(FaceDirection::NegativeY),
            Some(BrickCoordinate::new(0, -4, 7))
        );
        assert_eq!(
            brick.neighbour(FaceDirection::PositiveZ),
            Some(BrickCoordinate::new(0, -3, 8))
        );
        let edge = BrickCoordinate::new(BRICK_COORDINATE_MAXIMUM, 0, 0);
        assert_eq!(edge.neighbour(FaceDirection::PositiveX), None);
        assert!(edge.neighbour(FaceDirection::NegativeX).is_some());
    }

    #[test]
    fn display_formats_are_readable() {
        assert_eq!(VoxelCoordinate::new(1, -2, 3).to_string(), "(1, -2, 3)");
        assert_eq!(
            BrickCoordinate::new(-1, 0, 4).to_string(),
            "brick (-1, 0, 4)"
        );
    }

    proptest! {
        #[test]
        fn combining_a_split_coordinate_returns_the_original(coordinate in voxel_coordinate()) {
            let (brick, local) = coordinate.split();
            prop_assert_eq!(brick.voxel_at(local), coordinate);
        }

        #[test]
        fn a_brick_origin_is_aligned_and_never_above_the_voxels_inside(
            coordinate in voxel_coordinate()
        ) {
            let (brick, _) = coordinate.split();
            let origin = brick.origin();
            let edge = BRICK_EDGE_LENGTH as i64;
            prop_assert_eq!(origin.x().rem_euclid(edge), 0);
            prop_assert_eq!(origin.y().rem_euclid(edge), 0);
            prop_assert_eq!(origin.z().rem_euclid(edge), 0);
            prop_assert!(origin.x() <= coordinate.x() && coordinate.x() < origin.x() + edge);
            prop_assert!(origin.y() <= coordinate.y() && coordinate.y() < origin.y() + edge);
            prop_assert!(origin.z() <= coordinate.z() && coordinate.z() < origin.z() + edge);
        }

        #[test]
        fn splitting_a_voxel_built_from_brick_and_local_returns_them(
            brick in brick_coordinate(),
            local in local_voxel_index(),
        ) {
            prop_assert_eq!(brick.voxel_at(local).split(), (brick, local));
        }

        #[test]
        fn ordering_agrees_with_component_wise_comparison_x_first(
            a in voxel_coordinate(),
            b in voxel_coordinate(),
        ) {
            prop_assert_eq!(
                a.cmp(&b),
                (a.x(), a.y(), a.z()).cmp(&(b.x(), b.y(), b.z()))
            );
        }
    }
}
