//! The bricks a client draws as bricks (`docs/specs/lod-spec.md`, 3.3 and 3.5): a ball of bricks round the brick the viewer is in. Everything outside it is drawn as
//! coarse nodes, everything inside as bricks, and the two must meet without a gap or an overlap, on the client that draws them and on the server that sends
//! the nodes, which is why the rule for them is here and only here.
//!
//! The ball is of bricks: a brick is in it when its distance from the centre brick, counted in bricks, is at most the radius. The nodes are cubes of whole bricks,
//! so what is really drawn as bricks is every brick of every cube of 2 bricks (detail level 1, the smallest node) that has a brick in the ball: the ball with a
//! fringe of less than a brick and a half. [`FineRegion::wants_brick`] says it, and the selection ([`crate::select_nodes`]) splits exactly that far.

use region_octree::BrickCube;
use voxel_space::BrickCoordinate;

/// The bricks within `radius` bricks of `centre`, as a ball: those for which the sum of the squares of the differences of the coordinates is at most the
/// square of the radius.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FineRegion {
    pub centre: BrickCoordinate,
    pub radius: u32,
}

impl FineRegion {
    pub fn new(centre: BrickCoordinate, radius: u32) -> Self {
        Self { centre, radius }
    }

    /// Whether a ball of this radius holds the point at these distances along the three axes. A distance beyond the radius is out whatever the others are, which
    /// is checked first: distances are as large as the range of bricks (2^116), whose squares would not fit, and after the check each is at most the radius (a `u32`).
    fn holds_distances(&self, distances: [u128; 3]) -> bool {
        let radius = u128::from(self.radius);
        distances.iter().all(|&distance| distance <= radius)
            && distances
                .iter()
                .map(|&distance| distance * distance)
                .sum::<u128>()
                <= radius * radius
    }

    /// Whether the brick is in the ball.
    pub fn contains_brick(&self, brick: BrickCoordinate) -> bool {
        self.holds_distances([
            brick.x().abs_diff(self.centre.x()),
            brick.y().abs_diff(self.centre.y()),
            brick.z().abs_diff(self.centre.z()),
        ])
    }

    /// Whether the cube has a brick in the ball.
    pub fn touches_cube(&self, cube: BrickCube) -> bool {
        let origin = cube.origin();
        // Along each axis, the distance from the centre to the brick of the cube nearest it.
        let gap = |origin: i128, centre: i128| {
            // (The last brick is at most 2^116 past the origin of a cube, and the coordinates are within 2^115 of zero: no overflow.)
            let last = origin + cube.edge_length() as i128 - 1;
            if centre < origin {
                origin.abs_diff(centre)
            } else if centre > last {
                centre.abs_diff(last)
            } else {
                0
            }
        };
        self.holds_distances([
            gap(origin.x(), self.centre.x()),
            gap(origin.y(), self.centre.y()),
            gap(origin.z(), self.centre.z()),
        ])
    }

    /// Whether the brick is drawn as a brick: it is in a cube of 2 bricks (a node of detail level 1) that touches the ball.
    pub fn wants_brick(&self, brick: BrickCoordinate) -> bool {
        self.touches_cube(BrickCube::containing(brick, 1))
    }
}

#[cfg(test)]
mod tests {
    use voxel_space::{BRICK_COORDINATE_MAXIMUM, BRICK_COORDINATE_MINIMUM};

    use super::*;

    fn brick(x: i128, y: i128, z: i128) -> BrickCoordinate {
        BrickCoordinate::new(x, y, z)
    }

    #[test]
    fn a_brick_is_in_the_ball_when_the_squares_of_its_distances_add_to_at_most_the_square_of_the_radius()
     {
        let region = FineRegion::new(brick(10, -4, 7), 5);
        assert!(region.contains_brick(brick(10, -4, 7)));
        // 3, 4, 0 is exactly 5: in. 3, 4, 1 is not.
        assert!(region.contains_brick(brick(13, -8, 7)));
        assert!(!region.contains_brick(brick(13, -8, 8)));
        assert!(region.contains_brick(brick(15, -4, 7)));
        assert!(!region.contains_brick(brick(16, -4, 7)));
        assert!(region.contains_brick(brick(5, -4, 7)));
        assert!(!region.contains_brick(brick(4, -4, 7)));
        let alone = FineRegion::new(brick(0, 0, 0), 0);
        assert!(alone.contains_brick(brick(0, 0, 0)));
        assert!(!alone.contains_brick(brick(1, 0, 0)));
    }

    #[test]
    fn a_ball_at_one_end_of_the_range_does_not_reach_the_other_and_nothing_overflows() {
        // The squares of distances this large would not fit; the answer is still exact.
        let low = BRICK_COORDINATE_MINIMUM;
        let high = BRICK_COORDINATE_MAXIMUM;
        let region = FineRegion::new(brick(low, low, low), u32::MAX);
        assert!(region.contains_brick(brick(low, low, low)));
        assert!(region.contains_brick(brick(low + 1000, low, low)));
        assert!(!region.contains_brick(brick(high, high, high)));
        assert!(!region.contains_brick(brick(low + i128::from(u32::MAX) + 1, low, low)));
        assert!(region.contains_brick(brick(low + i128::from(u32::MAX), low, low)));
        let everything = BrickCube::containing(brick(0, 0, 0), region_octree::ROOT_LEVEL);
        assert!(region.touches_cube(everything));
        let far_cube = BrickCube::containing(brick(high, high, high), 100);
        assert!(!region.touches_cube(far_cube));
        // Two axes each at the radius are out together, as in any size of grid.
        let diagonal = FineRegion::new(brick(high, high, 0), 10);
        assert!(!diagonal.contains_brick(brick(high - 8, high - 8, 0)));
        assert!(diagonal.contains_brick(brick(high - 7, high - 7, 0)));
    }

    #[test]
    fn a_cube_touches_the_ball_exactly_when_one_of_its_bricks_is_in_it() {
        for radius in [0, 1, 2, 3, 5] {
            let region = FineRegion::new(brick(1, -2, 3), radius);
            for level in 0..=3u8 {
                let edge = 1i128 << level;
                for x in (-16..=16).step_by(edge as usize) {
                    for y in (-16..=16).step_by(edge as usize) {
                        for z in (-16..=16).step_by(edge as usize) {
                            let Ok(cube) = BrickCube::new(brick(x, y, z), level) else {
                                continue;
                            };
                            let any = (0..edge).any(|dx| {
                                (0..edge).any(|dy| {
                                    (0..edge).any(|dz| {
                                        region.contains_brick(brick(x + dx, y + dy, z + dz))
                                    })
                                })
                            });
                            assert_eq!(
                                region.touches_cube(cube),
                                any,
                                "{cube:?} and the ball of {radius}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_bricks_drawn_as_bricks_are_those_of_the_cubes_of_two_that_touch_the_ball() {
        let region = FineRegion::new(brick(0, 0, 0), 3);
        // The brick (3, 0, 0) is in the ball, so its cube of two, (2..3, 0..1, 0..1), is wanted whole.
        assert!(region.wants_brick(brick(2, 1, 1)));
        // The brick (4, 0, 0) is out of the ball, and so is its cube (4..5, ..): its nearest brick is (4, 0, 0) itself.
        assert!(!region.wants_brick(brick(4, 0, 0)));
        assert!(!region.wants_brick(brick(5, 1, 1)));
        // Every brick in the ball is wanted, and the wanted ones are no farther than the ball and one cube of two (less than 2 bricks) beyond.
        for x in -8..=8 {
            for y in -8..=8 {
                for z in -8..=8 {
                    let at = brick(x, y, z);
                    if region.contains_brick(at) {
                        assert!(region.wants_brick(at));
                    }
                    if region.wants_brick(at) {
                        let bigger = FineRegion::new(brick(0, 0, 0), 3 + 2);
                        assert!(bigger.contains_brick(at), "{at:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn coordinates_at_the_ends_of_the_range_do_not_overflow() {
        let region = FineRegion::new(
            brick(
                BRICK_COORDINATE_MAXIMUM - 2,
                BRICK_COORDINATE_MINIMUM + 2,
                0,
            ),
            4,
        );
        assert!(region.contains_brick(brick(
            BRICK_COORDINATE_MAXIMUM - 2,
            BRICK_COORDINATE_MINIMUM + 2,
            0
        )));
        assert!(!region.contains_brick(brick(
            BRICK_COORDINATE_MINIMUM,
            BRICK_COORDINATE_MAXIMUM,
            0
        )));
        let far = BrickCube::new(
            brick(
                BRICK_COORDINATE_MINIMUM,
                BRICK_COORDINATE_MINIMUM,
                BRICK_COORDINATE_MINIMUM,
            ),
            40,
        )
        .unwrap();
        assert!(!region.touches_cube(far));
        assert!(region.wants_brick(brick(
            BRICK_COORDINATE_MAXIMUM - 2,
            BRICK_COORDINATE_MINIMUM + 2,
            1
        )));
    }
}
