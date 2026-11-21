//! Digging at the edge of a brick changes the colliders of the brick beside it: its boxes are left out where they are buried in solid terrain, and a box of
//! solid terrain that nothing open touches has no collider at all, so what the brick is made of depends on the voxels of the bricks next to it.

mod common;

use std::collections::BTreeSet;

use common::{STILL, Scene, eye, run, spawn};
use physics::{CharacterBody, bricks_whose_colliders_read};
use voxel_space::{BrickCoordinate, VoxelCoordinate};

/// Solid ground to the top of voxel 39: the brick below the surface (`y` of 0) is solid and buried, the one with the surface in it (`y` of 1) is not. A character
/// standing on it, and a shaft dug under the character, from the surface down through the whole of the upper brick to the top of the lower one.
fn standing_over_a_shaft() -> (CharacterBody, Scene) {
    let mut scene = Scene::with_floor(39);
    let mut body = spawn(16.5, 45.0, 16.5);
    run(&mut body, &mut scene, STILL, 120);
    assert!(body.is_grounded());
    assert!((eye(&body)[1] - 41.62).abs() < 0.05, "{:?}", eye(&body));
    for x in 12..=20 {
        for z in 12..=20 {
            for y in 32..=39 {
                scene.remove(x, y, z);
            }
        }
    }
    // (The game tells the character of the brick the voxels are in, and not of the others: `run` would remake every brick of the scene.)
    scene.dirty = false;
    (body, scene)
}

/// Tells the character of every brick whose colliders read a voxel that was dug, as a game does when blocks change.
fn tell_of_the_dug_voxels(body: &mut CharacterBody, scene: &mut Scene) {
    let mut told = BTreeSet::new();
    for x in 12..=20 {
        for z in 12..=20 {
            for y in 32..=39 {
                told.extend(bricks_whose_colliders_read(VoxelCoordinate::new(x, y, z)));
            }
        }
    }
    for brick in told {
        body.brick_changed(scene, brick);
    }
}

#[test]
fn a_character_that_falls_down_a_shaft_dug_to_the_top_of_a_buried_brick_lands_on_it_when_it_is_told_of_the_bricks_the_dug_voxels_are_read_by()
 {
    let (mut body, mut scene) = standing_over_a_shaft();
    tell_of_the_dug_voxels(&mut body, &mut scene);
    run(&mut body, &mut scene, STILL, 240);
    // The floor of the shaft is the top of the brick below, at 32.
    assert!(
        (eye(&body)[1] - 33.62).abs() < 0.05,
        "the eyes are at {:?}, and should be 1.62 over the floor of the shaft",
        eye(&body)
    );
    assert!(body.is_grounded());
}

/// Why the bricks beside the edit are told of: the brick below the shaft is solid with solid beside it, so it had no collider at all, and it is not given one by what
/// is done to the brick above it.
#[test]
fn a_character_is_not_held_up_by_a_buried_brick_that_a_dig_exposed_when_only_the_brick_that_was_dug_is_made_again()
 {
    let (mut body, mut scene) = standing_over_a_shaft();
    body.brick_changed(&mut scene, BrickCoordinate::new(0, 1, 0));
    run(&mut body, &mut scene, STILL, 240);
    assert!(!body.is_grounded());
    assert!(eye(&body)[1] < 33.5, "{:?}", eye(&body));
}

mod bricks_made_from_a_voxel {
    use physics::bricks_whose_colliders_read;
    use voxel_space::{BrickCoordinate, VoxelCoordinate};

    fn brick(x: i128, y: i128, z: i128) -> BrickCoordinate {
        BrickCoordinate::new(x, y, z)
    }

    fn sorted(voxel: VoxelCoordinate) -> Vec<BrickCoordinate> {
        let mut bricks = bricks_whose_colliders_read(voxel);
        bricks.sort();
        bricks
    }

    #[test]
    fn a_voxel_away_from_the_faces_of_its_brick_changes_only_the_colliders_of_its_brick() {
        assert_eq!(sorted(VoxelCoordinate::new(5, 10, 20)), [brick(0, 0, 0)]);
        // (One in from a face is not on it.)
        assert_eq!(sorted(VoxelCoordinate::new(1, 30, 16)), [brick(0, 0, 0)]);
    }

    #[test]
    fn a_voxel_on_a_face_of_its_brick_changes_the_colliders_of_the_brick_across_that_face_too() {
        assert_eq!(
            sorted(VoxelCoordinate::new(0, 10, 10)),
            [brick(-1, 0, 0), brick(0, 0, 0)]
        );
        assert_eq!(
            sorted(VoxelCoordinate::new(31, 10, 10)),
            [brick(0, 0, 0), brick(1, 0, 0)]
        );
        assert_eq!(
            sorted(VoxelCoordinate::new(10, 32, 10)),
            [brick(0, 0, 0), brick(0, 1, 0)]
        );
        assert_eq!(
            sorted(VoxelCoordinate::new(10, 10, 63)),
            [brick(0, 0, 1), brick(0, 0, 2)]
        );
    }

    #[test]
    fn a_voxel_on_an_edge_or_a_corner_of_its_brick_changes_the_bricks_across_each_face_it_is_on_and_not_those_across_an_edge()
     {
        assert_eq!(
            sorted(VoxelCoordinate::new(0, 0, 10)),
            [brick(-1, 0, 0), brick(0, -1, 0), brick(0, 0, 0)]
        );
        assert_eq!(
            sorted(VoxelCoordinate::new(0, 0, 0)),
            [
                brick(-1, 0, 0),
                brick(0, -1, 0),
                brick(0, 0, -1),
                brick(0, 0, 0)
            ]
        );
        assert_eq!(
            sorted(VoxelCoordinate::new(31, 31, 31)),
            [
                brick(0, 0, 0),
                brick(0, 0, 1),
                brick(0, 1, 0),
                brick(1, 0, 0)
            ]
        );
    }

    #[test]
    fn bricks_below_the_origin_are_found_the_same_way() {
        // Voxel -1 is the last of brick -1: across its positive face is brick 0.
        assert_eq!(
            sorted(VoxelCoordinate::new(-1, 10, 10)),
            [brick(-1, 0, 0), brick(0, 0, 0)]
        );
        assert_eq!(
            sorted(VoxelCoordinate::new(-32, 10, 10)),
            [brick(-2, 0, 0), brick(-1, 0, 0)]
        );
    }
}
