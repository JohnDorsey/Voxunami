//! How far a ship is from a player, as the lifecycle measures it. The squares are compared with the squares of the distances, so that no
//! square root is taken: nothing here but adding and multiplying numbers, which every machine does the same.

use voxel_space::{BRICK_EDGE_LENGTH, BrickBox, WorldPosition};

/// A position as plain numbers, in voxels from the corner of the brick (0, 0, 0). Exact for any brick the world has, since a brick number
/// times 32 is far below what a `f64` holds whole.
fn voxels(position: WorldPosition) -> [f64; 3] {
    let brick = position.brick();
    let offset = position.offset();
    [
        (brick.x() * BRICK_EDGE_LENGTH as i64) as f64 + f64::from(offset[0]),
        (brick.y() * BRICK_EDGE_LENGTH as i64) as f64 + f64::from(offset[1]),
        (brick.z() * BRICK_EDGE_LENGTH as i64) as f64 + f64::from(offset[2]),
    ]
}

/// The square of the distance, in voxels, between two places.
pub fn squared_distance(a: WorldPosition, b: WorldPosition) -> f64 {
    let (a, b) = (voxels(a), voxels(b));
    (0..3)
        .map(|axis| {
            let gap = a[axis] - b[axis];
            gap * gap
        })
        .sum()
}

/// The square of the distance, in voxels, from a place to a box of bricks (nothing, inside it).
pub fn squared_distance_to_bricks(place: WorldPosition, bricks: BrickBox) -> f64 {
    let place = voxels(place);
    let (low, high) = (bricks.min(), bricks.max());
    let edge = BRICK_EDGE_LENGTH as i64;
    let corners = [
        (low.x() * edge, (high.x() + 1) * edge),
        (low.y() * edge, (high.y() + 1) * edge),
        (low.z() * edge, (high.z() + 1) * edge),
    ];
    (0..3)
        .map(|axis| {
            let (near, far) = (corners[axis].0 as f64, corners[axis].1 as f64);
            let gap = (near - place[axis]).max(place[axis] - far).max(0.0);
            gap * gap
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use voxel_space::BrickCoordinate;

    use super::*;

    fn at(brick: [i64; 3], offset: [f32; 3]) -> WorldPosition {
        WorldPosition::new(BrickCoordinate::new(brick[0], brick[1], brick[2]), offset)
    }

    fn bricks(low: [i64; 3], high: [i64; 3]) -> BrickBox {
        BrickBox::new(
            BrickCoordinate::new(low[0], low[1], low[2]),
            BrickCoordinate::new(high[0], high[1], high[2]),
        )
        .unwrap()
    }

    #[test]
    fn two_places_are_as_far_as_the_voxels_between_them_across_bricks_too() {
        assert_eq!(
            squared_distance(at([0; 3], [0.0; 3]), at([0; 3], [3.0, 4.0, 0.0])),
            25.0
        );
        // Ten bricks along x and a voxel back is 319 voxels.
        assert_eq!(
            squared_distance(at([0; 3], [1.0, 0.0, 0.0]), at([10, 0, 0], [0.0; 3])),
            319.0 * 319.0
        );
        assert_eq!(
            squared_distance(at([-3, 2, 1], [5.0; 3]), at([-3, 2, 1], [5.0; 3])),
            0.0
        );
    }

    #[test]
    fn the_distance_to_a_box_of_bricks_is_nothing_inside_and_to_the_nearest_face_edge_or_corner_outside()
     {
        let ship = bricks([2, 0, 0], [3, 0, 0]);
        // The box is from 64 to 128 along x, and from 0 to 32 along y and z.
        for inside in [
            at([2, 0, 0], [0.0; 3]),
            at([3, 0, 0], [31.5; 3]),
            at([3, 0, 0], [0.0, 10.0, 20.0]),
        ] {
            assert_eq!(squared_distance_to_bricks(inside, ship), 0.0);
        }
        // Beside a face: 10 voxels from the low x face, and 6 from the high one.
        assert_eq!(
            squared_distance_to_bricks(at([1, 0, 0], [22.0, 5.0, 5.0]), ship),
            100.0
        );
        assert_eq!(
            squared_distance_to_bricks(at([4, 0, 0], [6.0, 5.0, 5.0]), ship),
            36.0
        );
        // Past an edge: 3 along x and 4 along y make 5.
        assert_eq!(
            squared_distance_to_bricks(at([1, 0, 0], [29.0, 36.0, 5.0]), ship),
            25.0
        );
        // Past a corner: 1, 2 and 2 make 3.
        assert_eq!(
            squared_distance_to_bricks(at([1, 0, 0], [31.0, -2.0, 34.0]), ship),
            9.0
        );
        // Below, with the box a long way up: bricks are a whole 32 each.
        assert_eq!(
            squared_distance_to_bricks(at([2, 0, 0], [0.0; 3]), bricks([2, 5, 0], [2, 5, 0])),
            (5.0 * 32.0f64) * (5.0 * 32.0f64)
        );
    }
}
