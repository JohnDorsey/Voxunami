//! The cubes of a ship that is far away: which are drawn, and which are sent to a client so that they can be.

use std::collections::HashSet;

use coarse_cells::{
    DetailSettings, ViewSelection, far_ship_nodes_to_send, select_around_brick, select_far_ship,
};
use region_octree::BrickCube;
use voxel_space::{BrickBox, BrickCoordinate};

/// The coarsest level a server keeps nodes of.
const COARSEST: u8 = 12;

fn view() -> ViewSelection {
    ViewSelection {
        detail: DetailSettings {
            cell_pixels: 3.0,
            pixel_size: 2.0 * 0.700_207_538_209_062_3 / 1080.0,
        },
        zoom: None,
        // (A client that holds the bricks around it: the server of a far ship asks for the same selection with none.)
        fine_radius: 12,
        maximum_distance: 20_000.0,
    }
}

fn brick(x: i128, y: i128, z: i128) -> BrickCoordinate {
    BrickCoordinate::new(x, y, z)
}

#[test]
fn the_cubes_of_a_far_ship_are_those_of_the_selection_with_no_fine_region_and_none_is_a_brick() {
    let bounds = BrickBox::new(brick(30, 0, 0), brick(37, 7, 7)).unwrap();
    let may_hold = |cube: BrickCube| cube.meets(bounds);
    // The viewer is among the ship's bricks as well as beside them: a ship that has no bricks here has none to draw whoever is looking.
    for viewer in [brick(0, 0, 0), brick(33, 3, 3), brick(200, -40, 5)] {
        let drawn = select_far_ship(viewer, &view(), &may_hold);
        assert!(drawn.iter().all(|cube| cube.level() >= 1), "{viewer:?}");
        assert!(!drawn.is_empty(), "{viewer:?}");
        let no_fine_region = ViewSelection {
            fine_radius: 0,
            ..view()
        };
        let mut expected: Vec<BrickCube> = select_around_brick(viewer, &no_fine_region, &may_hold)
            .into_iter()
            .filter(|cube| cube.level() >= 1)
            .collect();
        let mut drawn = drawn;
        expected.sort_by_key(|cube| (cube.level(), cube.origin()));
        drawn.sort_by_key(|cube| (cube.level(), cube.origin()));
        assert_eq!(drawn, expected, "{viewer:?}");
    }
}

#[test]
fn what_is_sent_is_what_is_drawn_and_every_cube_that_holds_it_and_the_cubes_a_level_finer() {
    let bounds = BrickBox::new(brick(30, 0, 0), brick(37, 7, 7)).unwrap();
    let may_hold = |cube: BrickCube| cube.meets(bounds);
    let viewer = brick(0, 0, 0);
    let drawn = select_far_ship(viewer, &view(), &may_hold);
    let sent = far_ship_nodes_to_send(viewer, &view(), &may_hold, COARSEST);
    let sent_set: HashSet<BrickCube> = sent.iter().copied().collect();
    assert_eq!(sent_set.len(), sent.len(), "no cube is sent twice");
    for cube in &drawn {
        assert!(sent_set.contains(cube), "{cube:?} is drawn and not sent");
        // Every cube that holds it, up to the coarsest.
        for level in cube.level() + 1..=COARSEST {
            let holder = BrickCube::containing(cube.origin(), level);
            assert!(sent_set.contains(&holder), "{holder:?} holds {cube:?}");
        }
        // Each of its eight halves, if it has a finer level and the half may hold anything.
        if cube.level() >= 2 {
            let half = 1i128 << (cube.level() - 1);
            for (dx, dy, dz) in [
                (0, 0, 0),
                (1, 0, 0),
                (0, 1, 0),
                (1, 1, 0),
                (0, 0, 1),
                (1, 0, 1),
                (0, 1, 1),
                (1, 1, 1),
            ] {
                let origin = brick(
                    cube.origin().x() + dx * half,
                    cube.origin().y() + dy * half,
                    cube.origin().z() + dz * half,
                );
                let child = BrickCube::new(origin, cube.level() - 1).unwrap();
                assert_eq!(sent_set.contains(&child), may_hold(child), "{child:?}");
            }
        }
    }
    // Nothing else: every cube sent is one of the three kinds.
    for cube in &sent {
        let is_drawn = drawn.contains(cube);
        let holds_a_drawn = drawn.iter().any(|d| {
            d.level() < cube.level() && BrickCube::containing(d.origin(), cube.level()) == *cube
        });
        let half_of_a_drawn = drawn.iter().any(|d| {
            d.level() == cube.level() + 1 && BrickCube::containing(cube.origin(), d.level()) == *d
        });
        assert!(
            is_drawn || holds_a_drawn || half_of_a_drawn,
            "{cube:?} is none of them"
        );
    }
}

#[test]
fn what_is_sent_is_coarsest_first_and_the_same_every_time() {
    let bounds = BrickBox::new(brick(30, 0, 0), brick(37, 7, 7)).unwrap();
    let may_hold = |cube: BrickCube| cube.meets(bounds);
    let first = far_ship_nodes_to_send(brick(0, 0, 0), &view(), &may_hold, COARSEST);
    let second = far_ship_nodes_to_send(brick(0, 0, 0), &view(), &may_hold, COARSEST);
    assert_eq!(first, second);
    let levels: Vec<u8> = first.iter().map(|cube| cube.level()).collect();
    assert!(
        levels.windows(2).all(|pair| pair[0] >= pair[1]),
        "{levels:?}"
    );
}

#[test]
fn a_cube_that_may_hold_nothing_is_neither_drawn_nor_sent_nor_are_the_cubes_inside_it() {
    let nowhere = |_: BrickCube| false;
    assert!(select_far_ship(brick(0, 0, 0), &view(), &nowhere).is_empty());
    assert!(far_ship_nodes_to_send(brick(0, 0, 0), &view(), &nowhere, COARSEST).is_empty());
}
