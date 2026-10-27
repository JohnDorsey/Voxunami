use crate::{
    BRICK_EDGE_LENGTH, BrickCoordinate, LocalVoxelIndex, VOXEL_COORDINATE_MAXIMUM, VoxelCoordinate,
};

/// An axis-aligned box of voxels. Both corners are inclusive, so a box is never empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VoxelBox {
    min: VoxelCoordinate,
    max: VoxelCoordinate,
}

impl VoxelBox {
    /// `None` if `max` is below `min` on any axis.
    pub fn new(min: VoxelCoordinate, max: VoxelCoordinate) -> Option<Self> {
        (min.x() <= max.x() && min.y() <= max.y() && min.z() <= max.z())
            .then_some(Self { min, max })
    }

    /// The voxels of one brick.
    pub fn of_brick(brick: BrickCoordinate) -> Self {
        let last = (BRICK_EDGE_LENGTH - 1) as u8;
        Self {
            min: brick.origin(),
            max: brick.voxel_at(LocalVoxelIndex::new(last, last, last)),
        }
    }

    pub fn min(self) -> VoxelCoordinate {
        self.min
    }

    pub fn max(self) -> VoxelCoordinate {
        self.max
    }

    pub fn contains(self, coordinate: VoxelCoordinate) -> bool {
        (self.min.x()..=self.max.x()).contains(&coordinate.x())
            && (self.min.y()..=self.max.y()).contains(&coordinate.y())
            && (self.min.z()..=self.max.z()).contains(&coordinate.z())
    }

    /// True if the boxes share at least one voxel.
    pub fn intersects(self, other: Self) -> bool {
        self.min.x() <= other.max.x()
            && other.min.x() <= self.max.x()
            && self.min.y() <= other.max.y()
            && other.min.y() <= self.max.y()
            && self.min.z() <= other.max.z()
            && other.min.z() <= self.max.z()
    }

    /// Moves the upper corner outward by non-negative amounts, stopping at the coordinate range
    /// limit. Panics if an amount is negative.
    pub fn extended_upper(self, dx: i128, dy: i128, dz: i128) -> Self {
        assert!(
            dx >= 0 && dy >= 0 && dz >= 0,
            "extension must not be negative"
        );
        let extend =
            |value: i128, amount: i128| value.saturating_add(amount).min(VOXEL_COORDINATE_MAXIMUM);
        Self {
            min: self.min,
            max: VoxelCoordinate::new(
                extend(self.max.x(), dx),
                extend(self.max.y(), dy),
                extend(self.max.z(), dz),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::test_support::{brick_coordinate, voxel_coordinate};

    fn voxel(x: i128, y: i128, z: i128) -> VoxelCoordinate {
        VoxelCoordinate::new(x, y, z)
    }

    #[test]
    fn new_rejects_inverted_boxes_on_any_axis() {
        assert!(VoxelBox::new(voxel(0, 0, 0), voxel(0, 0, 0)).is_some());
        assert!(VoxelBox::new(voxel(1, 0, 0), voxel(0, 5, 5)).is_none());
        assert!(VoxelBox::new(voxel(0, 1, 0), voxel(5, 0, 5)).is_none());
        assert!(VoxelBox::new(voxel(0, 0, 1), voxel(5, 5, 0)).is_none());
    }

    #[test]
    fn a_brick_box_spans_exactly_its_32_voxels_per_axis() {
        let voxel_box = VoxelBox::of_brick(BrickCoordinate::new(-1, 0, 2));
        assert_eq!(voxel_box.min(), voxel(-32, 0, 64));
        assert_eq!(voxel_box.max(), voxel(-1, 31, 95));
    }

    #[test]
    fn contains_includes_both_corners_and_nothing_beyond() {
        let voxel_box = VoxelBox::new(voxel(-2, 0, 5), voxel(3, 4, 5)).unwrap();
        assert!(voxel_box.contains(voxel(-2, 0, 5)));
        assert!(voxel_box.contains(voxel(3, 4, 5)));
        assert!(!voxel_box.contains(voxel(-3, 0, 5)));
        assert!(!voxel_box.contains(voxel(4, 4, 5)));
        assert!(!voxel_box.contains(voxel(0, 0, 6)));
    }

    #[test]
    fn boxes_sharing_a_boundary_voxel_intersect_but_adjacent_bricks_do_not() {
        let left = VoxelBox::new(voxel(0, 0, 0), voxel(4, 4, 4)).unwrap();
        let touching = VoxelBox::new(voxel(4, 4, 4), voxel(8, 8, 8)).unwrap();
        let apart = VoxelBox::new(voxel(5, 0, 0), voxel(8, 4, 4)).unwrap();
        assert!(left.intersects(touching) && touching.intersects(left));
        assert!(!left.intersects(apart) && !apart.intersects(left));

        let brick = VoxelBox::of_brick(BrickCoordinate::new(0, 0, 0));
        let neighbour = VoxelBox::of_brick(BrickCoordinate::new(1, 0, 0));
        assert!(!brick.intersects(neighbour));
    }

    #[test]
    fn extended_upper_moves_only_the_upper_corner_and_saturates() {
        let voxel_box = VoxelBox::of_brick(BrickCoordinate::new(0, 0, 0)).extended_upper(0, 4, 0);
        assert_eq!(voxel_box.min(), voxel(0, 0, 0));
        assert_eq!(voxel_box.max(), voxel(31, 35, 31));

        let near_limit =
            VoxelBox::new(voxel(0, 0, 0), voxel(0, VOXEL_COORDINATE_MAXIMUM - 1, 0)).unwrap();
        assert_eq!(
            near_limit.extended_upper(0, 10, 0).max().y(),
            VOXEL_COORDINATE_MAXIMUM
        );
        assert_eq!(
            near_limit.extended_upper(0, i128::MAX, 0).max().y(),
            VOXEL_COORDINATE_MAXIMUM
        );
    }

    #[test]
    #[should_panic(expected = "must not be negative")]
    fn extended_upper_rejects_negative_amounts() {
        VoxelBox::of_brick(BrickCoordinate::ORIGIN).extended_upper(0, -1, 0);
    }

    proptest! {
        #[test]
        fn a_brick_box_contains_exactly_the_voxels_that_split_into_that_brick(
            brick in brick_coordinate(),
            probe in voxel_coordinate(),
        ) {
            prop_assert_eq!(
                VoxelBox::of_brick(brick).contains(probe),
                probe.split().0 == brick
            );
        }
    }
}
