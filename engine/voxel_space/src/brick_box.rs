use crate::{BRICK_COORDINATE_MAXIMUM, BRICK_COORDINATE_MINIMUM, BrickCoordinate};

/// An axis-aligned box of bricks: the region a client watches. Both corners are inclusive, so a
/// box is never empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BrickBox {
    min: BrickCoordinate,
    max: BrickCoordinate,
}

impl BrickBox {
    /// `None` if `max` is below `min` on any axis.
    pub fn new(min: BrickCoordinate, max: BrickCoordinate) -> Option<Self> {
        (min.x() <= max.x() && min.y() <= max.y() && min.z() <= max.z())
            .then_some(Self { min, max })
    }

    /// The cube of bricks within `radius` of `center` along every axis, cut off at the limits of the
    /// coordinate range.
    pub fn around(center: BrickCoordinate, radius: u32) -> Self {
        let radius = i64::from(radius);
        let low = |value: i64| value.saturating_sub(radius).max(BRICK_COORDINATE_MINIMUM);
        let high = |value: i64| value.saturating_add(radius).min(BRICK_COORDINATE_MAXIMUM);
        Self {
            min: BrickCoordinate::new(low(center.x()), low(center.y()), low(center.z())),
            max: BrickCoordinate::new(high(center.x()), high(center.y()), high(center.z())),
        }
    }

    pub fn min(self) -> BrickCoordinate {
        self.min
    }

    pub fn max(self) -> BrickCoordinate {
        self.max
    }

    pub fn contains(self, brick: BrickCoordinate) -> bool {
        (self.min.x()..=self.max.x()).contains(&brick.x())
            && (self.min.y()..=self.max.y()).contains(&brick.y())
            && (self.min.z()..=self.max.z()).contains(&brick.z())
    }

    /// The bricks both boxes contain, if any.
    pub fn intersection(self, other: Self) -> Option<Self> {
        Self::new(
            BrickCoordinate::new(
                self.min.x().max(other.min.x()),
                self.min.y().max(other.min.y()),
                self.min.z().max(other.min.z()),
            ),
            BrickCoordinate::new(
                self.max.x().min(other.max.x()),
                self.max.y().min(other.max.y()),
                self.max.z().min(other.max.z()),
            ),
        )
    }

    /// How many bricks the box holds, saturating at `u128::MAX` for boxes too large to count.
    pub fn brick_count(self) -> u128 {
        let edge = |low: i64, high: i64| (high - low) as u128 + 1;
        edge(self.min.x(), self.max.x())
            .saturating_mul(edge(self.min.y(), self.max.y()))
            .saturating_mul(edge(self.min.z(), self.max.z()))
    }

    /// Every brick in the box, x varying fastest, then y, then z.
    pub fn bricks(self) -> impl Iterator<Item = BrickCoordinate> {
        (self.min.z()..=self.max.z()).flat_map(move |z| {
            (self.min.y()..=self.max.y()).flat_map(move |y| {
                (self.min.x()..=self.max.x()).map(move |x| BrickCoordinate::new(x, y, z))
            })
        })
    }

    /// The bricks of this box that `other` does not contain, as at most six boxes that do not
    /// overlap each other. Moving a watched region needs only the newly covered part, and this finds
    /// it without visiting the bricks both regions share.
    pub fn without(self, other: Self) -> Vec<Self> {
        let Some(shared) = self.intersection(other) else {
            return vec![self];
        };
        let mut remaining = Vec::with_capacity(6);
        let mut slab = |min: [i64; 3], max: [i64; 3]| {
            // Checked before building coordinates: an empty slab beside a box touching the limit of
            // the coordinate range would have a corner outside it.
            if (0..3).all(|axis| min[axis] <= max[axis]) {
                remaining.push(Self {
                    min: BrickCoordinate::new(min[0], min[1], min[2]),
                    max: BrickCoordinate::new(max[0], max[1], max[2]),
                });
            }
        };
        let (low, high) = (self.min, self.max);
        // Cut the box into slabs: first the x ranges beside the shared part, then, within the
        // shared x range, the y ranges beside it, then the z ranges beside the shared part.
        slab(
            [low.x(), low.y(), low.z()],
            [shared.min.x() - 1, high.y(), high.z()],
        );
        slab(
            [shared.max.x() + 1, low.y(), low.z()],
            [high.x(), high.y(), high.z()],
        );
        slab(
            [shared.min.x(), low.y(), low.z()],
            [shared.max.x(), shared.min.y() - 1, high.z()],
        );
        slab(
            [shared.min.x(), shared.max.y() + 1, low.z()],
            [shared.max.x(), high.y(), high.z()],
        );
        slab(
            [shared.min.x(), shared.min.y(), low.z()],
            [shared.max.x(), shared.max.y(), shared.min.z() - 1],
        );
        slab(
            [shared.min.x(), shared.min.y(), shared.max.z() + 1],
            [shared.max.x(), shared.max.y(), high.z()],
        );
        remaining
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::test_support::brick_coordinate;

    fn brick(x: i64, y: i64, z: i64) -> BrickCoordinate {
        BrickCoordinate::new(x, y, z)
    }

    fn boxed(min: [i64; 3], max: [i64; 3]) -> BrickBox {
        BrickBox::new(brick(min[0], min[1], min[2]), brick(max[0], max[1], max[2])).unwrap()
    }

    #[test]
    fn a_box_needs_its_minimum_corner_below_its_maximum() {
        assert!(BrickBox::new(brick(0, 0, 0), brick(0, 0, 0)).is_some());
        assert!(BrickBox::new(brick(1, 0, 0), brick(0, 5, 5)).is_none());
        assert!(BrickBox::new(brick(0, 0, 1), brick(5, 5, 0)).is_none());
    }

    #[test]
    fn around_is_a_centered_cube_cut_off_at_the_coordinate_limits() {
        let around = BrickBox::around(brick(10, -3, 0), 2);
        assert_eq!(around, boxed([8, -5, -2], [12, -1, 2]));
        assert_eq!(around.brick_count(), 125);
        let at_the_edge = BrickBox::around(brick(BRICK_COORDINATE_MAXIMUM, 0, 0), 5);
        assert_eq!(at_the_edge.max().x(), BRICK_COORDINATE_MAXIMUM);
        assert_eq!(at_the_edge.min().x(), BRICK_COORDINATE_MAXIMUM - 5);
        assert_eq!(
            BrickBox::around(brick(BRICK_COORDINATE_MINIMUM, 0, 0), u32::MAX)
                .min()
                .x(),
            BRICK_COORDINATE_MINIMUM
        );
    }

    #[test]
    fn contains_and_intersection_include_the_boundaries() {
        let a = boxed([0, 0, 0], [4, 4, 4]);
        assert!(a.contains(brick(0, 4, 2)));
        assert!(!a.contains(brick(5, 0, 0)));
        assert!(!a.contains(brick(0, 0, -1)));
        let b = boxed([4, 4, 4], [9, 9, 9]);
        assert_eq!(a.intersection(b), Some(boxed([4, 4, 4], [4, 4, 4])));
        assert_eq!(a.intersection(boxed([5, 0, 0], [9, 4, 4])), None);
    }

    #[test]
    fn bricks_lists_every_brick_with_x_varying_fastest() {
        let listed: Vec<_> = boxed([0, 0, 0], [1, 1, 0]).bricks().collect();
        assert_eq!(
            listed,
            [
                brick(0, 0, 0),
                brick(1, 0, 0),
                brick(0, 1, 0),
                brick(1, 1, 0)
            ]
        );
        assert_eq!(boxed([-2, 5, 0], [3, 7, 1]).bricks().count(), 6 * 3 * 2);
    }

    #[test]
    fn the_count_of_a_huge_box_saturates_instead_of_overflowing() {
        let everything = BrickBox::new(
            brick(
                BRICK_COORDINATE_MINIMUM,
                BRICK_COORDINATE_MINIMUM,
                BRICK_COORDINATE_MINIMUM,
            ),
            brick(
                BRICK_COORDINATE_MAXIMUM,
                BRICK_COORDINATE_MAXIMUM,
                BRICK_COORDINATE_MAXIMUM,
            ),
        )
        .unwrap();
        assert_eq!(everything.brick_count(), u128::MAX);
    }

    #[test]
    fn without_a_box_inside_leaves_six_slabs_and_without_a_disjoint_box_leaves_everything() {
        let outer = boxed([0, 0, 0], [9, 9, 9]);
        assert_eq!(outer.without(boxed([3, 3, 3], [5, 5, 5])).len(), 6);
        assert_eq!(outer.without(boxed([20, 0, 0], [30, 9, 9])), [outer]);
        assert_eq!(outer.without(outer), []);
        // Boxes touching the limits of the coordinate range have no room for a slab beyond them.
        let at_minimum = BrickBox::around(brick(BRICK_COORDINATE_MINIMUM, 0, 0), 3);
        assert_eq!(at_minimum.without(at_minimum), []);
        let shifted = BrickBox::around(brick(BRICK_COORDINATE_MINIMUM + 1, 0, 0), 3);
        assert_eq!(shifted.without(at_minimum).len(), 1);
        assert_eq!(outer.without(boxed([-5, -5, -5], [20, 20, 20])), []);
        // A region moved one brick along x gains exactly one slab of 10 x 10.
        let moved = boxed([1, 0, 0], [10, 9, 9]);
        let gained = moved.without(outer);
        assert_eq!(gained, [boxed([10, 0, 0], [10, 9, 9])]);
    }

    fn small_box() -> impl Strategy<Value = BrickBox> {
        (brick_coordinate(), (0u32..6, 0u32..6, 0u32..6)).prop_map(|(corner, (dx, dy, dz))| {
            let high = |value: i64, extra: u32| {
                value
                    .saturating_add(i64::from(extra))
                    .min(BRICK_COORDINATE_MAXIMUM)
            };
            BrickBox::new(
                corner,
                brick(
                    high(corner.x(), dx),
                    high(corner.y(), dy),
                    high(corner.z(), dz),
                ),
            )
            .unwrap()
        })
    }

    /// A box near a given brick, so two random boxes overlap often enough to be interesting.
    fn nearby_box() -> impl Strategy<Value = BrickBox> {
        ((-6i64..6, -6i64..6, -6i64..6), (0i64..7, 0i64..7, 0i64..7)).prop_map(
            |(corner, extent)| {
                boxed(
                    [corner.0, corner.1, corner.2],
                    [
                        corner.0 + extent.0,
                        corner.1 + extent.1,
                        corner.2 + extent.2,
                    ],
                )
            },
        )
    }

    proptest! {
        #[test]
        fn without_partitions_exactly_the_bricks_not_in_the_other_box(
            a in nearby_box(),
            b in nearby_box(),
        ) {
            let pieces = a.without(b);
            prop_assert!(pieces.len() <= 6);
            for candidate in nearby_box_bricks() {
                let in_pieces = pieces.iter().filter(|piece| piece.contains(candidate)).count();
                let expected = usize::from(a.contains(candidate) && !b.contains(candidate));
                prop_assert_eq!(in_pieces, expected, "{:?}", candidate);
            }
        }

        #[test]
        fn counting_matches_iterating(region in small_box()) {
            prop_assert_eq!(region.brick_count(), region.bricks().count() as u128);
        }

        #[test]
        fn an_intersection_is_inside_both_boxes(a in small_box(), b in small_box()) {
            if let Some(shared) = a.intersection(b) {
                for corner in [shared.min(), shared.max()] {
                    prop_assert!(a.contains(corner) && b.contains(corner));
                }
            } else {
                prop_assert!(a.bricks().all(|candidate| !b.contains(candidate)));
            }
        }
    }

    /// Every brick in the range the nearby-box strategy can reach.
    fn nearby_box_bricks() -> impl Iterator<Item = BrickCoordinate> {
        boxed([-8, -8, -8], [20, 20, 20]).bricks()
    }
}
