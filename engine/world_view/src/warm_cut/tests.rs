use region_octree::BrickCube;
use voxel_space::BrickCoordinate;

use super::*;

fn cube(x: i128, y: i128, z: i128, level: u8) -> BrickCube {
    BrickCube::new(BrickCoordinate::new(x, y, z), level).expect("an aligned cube")
}

#[test]
fn nodes_are_queued_so_that_the_coarsest_are_made_first_and_the_nearest_of_those_first_and_bricks_are_left_out()
 {
    let centre = BrickCoordinate::new(0, 0, 0);
    // (Given nearest first at each level, which is the wrong way round for the queue, and mixed.)
    let cubes = vec![
        cube(0, 0, 0, 0),
        cube(2, 0, 0, 1),
        cube(4, 0, 0, 2),
        cube(0, 8, 0, 3),
        cube(8, 0, 0, 1),
        cube(16, 0, 0, 2),
        cube(-32, 0, 0, 3),
        cube(1, 1, 1, 0),
    ];
    let mut queue = warm_queue_of(&cubes, centre);
    // The queue is made from its end.
    queue.reverse();
    assert_eq!(
        queue,
        vec![
            cube(0, 8, 0, 3),
            cube(-32, 0, 0, 3),
            cube(4, 0, 0, 2),
            cube(16, 0, 0, 2),
            cube(2, 0, 0, 1),
            cube(8, 0, 0, 1),
        ]
    );
}

#[test]
fn cubes_at_the_same_level_and_distance_keep_the_order_they_were_given_in() {
    let centre = BrickCoordinate::new(0, 0, 0);
    // Two cubes the same distance from the centre.
    let first = cube(8, 0, 0, 1);
    let second = cube(-10, 0, 0, 1);
    let queue = warm_queue_of(&[first, second], centre);
    assert_eq!(queue, vec![first, second]);
    assert_eq!(warm_queue_of(&[second, first], centre), vec![second, first]);
}
