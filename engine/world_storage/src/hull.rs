//! The box that a ship's blocks fill: the hull of a ship, worked out from its voxels (step 4c-5-1 of `docs/decisions/joints-and-cars.md`).
//!
//! What the world knows of how far a ship reaches is the sphere about its origin through the far corner of its bricks (a whole brick on the far side of each axis: 55 voxels for a
//! ship of one brick, whatever is in it), which makes two ships of a few blocks "in the way" of each other with a hundred voxels between their hulls. The box of the blocks is the
//! measure that a room for a hit to come can be made of. It is worked out where the bodies weigh the ship (`grid_bodies`), from the same look at each brick, and kept on the ship
//! ([`MovingGrid::hull`](crate::MovingGrid::hull)); it is not saved, for it is worked out again from the voxels when the ship is next weighed.

use voxel_space::VoxelCoordinate;

/// The smallest box of voxels that holds every block of a ship, in the ship's own coordinates: the lowest and the highest voxel along each axis that has one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HullBox {
    lowest: [i128; 3],
    highest: [i128; 3],
}

impl HullBox {
    /// The box of a single voxel.
    pub fn of_voxel(voxel: VoxelCoordinate) -> Self {
        let at = [voxel.x(), voxel.y(), voxel.z()];
        Self {
            lowest: at,
            highest: at,
        }
    }

    /// The smallest box that holds this one and a voxel.
    pub fn including(self, voxel: VoxelCoordinate) -> Self {
        self.union(Self::of_voxel(voxel))
    }

    /// The smallest box that holds both.
    pub fn union(self, other: Self) -> Self {
        Self {
            lowest: [0, 1, 2].map(|axis| self.lowest[axis].min(other.lowest[axis])),
            highest: [0, 1, 2].map(|axis| self.highest[axis].max(other.highest[axis])),
        }
    }

    /// The box of the voxels, or `None` if there are none.
    pub fn of_voxels(voxels: impl IntoIterator<Item = VoxelCoordinate>) -> Option<Self> {
        voxels
            .into_iter()
            .map(Self::of_voxel)
            .reduce(|first, second| first.union(second))
    }

    /// The lowest voxel of the box along each axis (a voxel that is in it).
    pub fn lowest(&self) -> VoxelCoordinate {
        VoxelCoordinate::new(self.lowest[0], self.lowest[1], self.lowest[2])
    }

    /// The highest voxel of the box along each axis (a voxel that is in it).
    pub fn highest(&self) -> VoxelCoordinate {
        VoxelCoordinate::new(self.highest[0], self.highest[1], self.highest[2])
    }

    /// Whether a voxel is in the box.
    pub fn contains(&self, voxel: VoxelCoordinate) -> bool {
        let at = [voxel.x(), voxel.y(), voxel.z()];
        (0..3).all(|axis| self.lowest[axis] <= at[axis] && at[axis] <= self.highest[axis])
    }

    /// The middle of the box, in voxels: the far faces of the highest voxels are one more than their coordinates.
    pub fn middle(&self) -> [f64; 3] {
        [0, 1, 2].map(|axis| (self.lowest[axis] as f64 + self.highest[axis] as f64 + 1.0) * 0.5)
    }

    /// How far the farthest corner of the box is from a point, in voxels: a sphere about the point with this radius holds every block of the ship.
    pub fn reach_from(&self, point: [f64; 3]) -> f64 {
        let squared: f64 = (0..3)
            .map(|axis| {
                let near = self.lowest[axis] as f64 - point[axis];
                let far = self.highest[axis] as f64 + 1.0 - point[axis];
                let farthest = near.abs().max(far.abs());
                farthest * farthest
            })
            .sum();
        libm::sqrt(squared)
    }

    /// How far the corners of the box are from its middle, in voxels: half its diagonal.
    pub fn radius(&self) -> f64 {
        self.reach_from(self.middle())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::*;

    fn voxel(x: i128, y: i128, z: i128) -> VoxelCoordinate {
        VoxelCoordinate::new(x, y, z)
    }

    #[test]
    fn no_voxels_have_no_box() {
        assert_eq!(HullBox::of_voxels([]), None);
    }

    #[test]
    fn one_voxel_is_a_box_of_one_voxel_whose_corners_are_half_a_diagonal_from_its_middle() {
        let hull = HullBox::of_voxel(voxel(3, -2, 7));
        assert_eq!(hull.lowest(), voxel(3, -2, 7));
        assert_eq!(hull.highest(), voxel(3, -2, 7));
        assert_eq!(hull.middle(), [3.5, -1.5, 7.5]);
        assert_eq!(hull.radius(), libm::sqrt(0.75));
        assert!(hull.contains(voxel(3, -2, 7)));
        assert!(!hull.contains(voxel(4, -2, 7)));
    }

    #[test]
    fn two_voxels_a_hundred_apart_are_a_box_between_them_and_not_the_bricks_they_are_in() {
        let hull = HullBox::of_voxels([voxel(5, 5, 5), voxel(105, 40, 3)]).unwrap();
        assert_eq!(hull.lowest(), voxel(5, 5, 3));
        assert_eq!(hull.highest(), voxel(105, 40, 5));
        // The reach about the origin of the ship: to the far corner (106, 41, 6), not to the far corner of its bricks (128, 64, 32).
        assert_eq!(
            hull.reach_from([0.0; 3]),
            libm::sqrt(106.0 * 106.0 + 41.0 * 41.0 + 6.0 * 6.0)
        );
    }

    #[test]
    fn the_reach_is_to_the_farthest_corner_from_either_side_of_a_point() {
        let hull = HullBox::of_voxels([voxel(0, 0, 0), voxel(9, 9, 9)]).unwrap();
        // A point near the low corner is far from the high one, and the other way about.
        assert_eq!(hull.reach_from([0.0; 3]), libm::sqrt(300.0));
        assert_eq!(hull.reach_from([10.0; 3]), libm::sqrt(300.0));
        assert_eq!(hull.reach_from([5.0; 3]), libm::sqrt(75.0));
        assert_eq!(hull.radius(), libm::sqrt(75.0));
    }

    fn voxel_sets() -> impl Strategy<Value = Vec<(i128, i128, i128)>> {
        proptest::collection::vec((-300i128..300, -300i128..300, -300i128..300), 1..40)
    }

    proptest! {
        /// The box is what a look at every voxel finds: the lowest and highest along each axis, every voxel is in it, and a face of it has a voxel on it.
        #[test]
        fn the_box_is_what_a_look_at_every_voxel_finds(points in voxel_sets()) {
            let voxels: Vec<VoxelCoordinate> = points.iter().map(|&(x, y, z)| voxel(x, y, z)).collect();
            let hull = HullBox::of_voxels(voxels.iter().copied()).unwrap();
            let low = [0, 1, 2].map(|axis| points.iter().map(|p| [p.0, p.1, p.2][axis]).min().unwrap());
            let high = [0, 1, 2].map(|axis| points.iter().map(|p| [p.0, p.1, p.2][axis]).max().unwrap());
            prop_assert_eq!(hull.lowest(), voxel(low[0], low[1], low[2]));
            prop_assert_eq!(hull.highest(), voxel(high[0], high[1], high[2]));
            for found in &voxels {
                prop_assert!(hull.contains(*found));
            }
        }

        /// A sphere about a point with the reach holds every corner of every voxel, and the reach is the distance to the farthest corner of the box.
        #[test]
        fn the_reach_holds_every_corner_of_every_voxel_and_is_the_box_s_own_farthest_corner(
            points in voxel_sets(),
            at in (-400.0f64..400.0, -400.0f64..400.0, -400.0f64..400.0),
        ) {
            let point = [at.0, at.1, at.2];
            let voxels: Vec<VoxelCoordinate> = points.iter().map(|&(x, y, z)| voxel(x, y, z)).collect();
            let hull = HullBox::of_voxels(voxels.iter().copied()).unwrap();
            let reach = hull.reach_from(point);
            for found in &voxels {
                for corner in 0..8 {
                    let corner = [
                        found.x() + (corner & 1),
                        found.y() + ((corner >> 1) & 1),
                        found.z() + ((corner >> 2) & 1),
                    ];
                    let apart: f64 = (0..3)
                        .map(|axis| {
                            let gap = corner[axis] as f64 - point[axis];
                            gap * gap
                        })
                        .sum();
                    prop_assert!(libm::sqrt(apart) <= reach + 1e-9, "a corner of {found:?} is {} from the point, past the reach {reach}", libm::sqrt(apart));
                }
            }
            // The farthest corner of the box (a corner of the box need not be a corner of a voxel: the faces are, the corners are not), by a look at all eight.
            let corners: BTreeSet<[i128; 3]> = (0..8)
                .map(|corner| {
                    [
                        if corner & 1 == 0 { hull.lowest().x() } else { hull.highest().x() + 1 },
                        if corner & 2 == 0 { hull.lowest().y() } else { hull.highest().y() + 1 },
                        if corner & 4 == 0 { hull.lowest().z() } else { hull.highest().z() + 1 },
                    ]
                })
                .collect();
            let farthest = corners
                .iter()
                .map(|corner| {
                    libm::sqrt(
                        (0..3)
                            .map(|axis| {
                                let gap = corner[axis] as f64 - point[axis];
                                gap * gap
                            })
                            .sum(),
                    )
                })
                .fold(0.0, f64::max);
            prop_assert!((reach - farthest).abs() <= 1e-9);
        }

        /// Putting voxels in any order and in any grouping gives the same box.
        #[test]
        fn the_union_does_not_depend_on_the_order_or_the_grouping(points in voxel_sets()) {
            let voxels: Vec<VoxelCoordinate> = points.iter().map(|&(x, y, z)| voxel(x, y, z)).collect();
            let all = HullBox::of_voxels(voxels.iter().copied());
            let backwards = HullBox::of_voxels(voxels.iter().rev().copied());
            let middle = voxels.len() / 2;
            let halves = match (
                HullBox::of_voxels(voxels[..middle].iter().copied()),
                HullBox::of_voxels(voxels[middle..].iter().copied()),
            ) {
                (Some(first), Some(second)) => Some(first.union(second)),
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            };
            prop_assert_eq!(all, backwards);
            prop_assert_eq!(all, halves);
        }
    }
}
