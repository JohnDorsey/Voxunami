//! The cubes of a ship that is far away: which are drawn, and which are sent to a client so that they can be.

use std::collections::HashSet;

use coarse_cells::{
    DetailSettings, ViewSelection, far_ship_empty_cubes, far_ship_nodes_to_send,
    select_around_brick, select_far_ship,
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

/// A ship of two blocks of bricks, far from each other: its box is much bigger than what is in it.
fn two_blocks() -> (BrickBox, Vec<BrickCoordinate>) {
    let bricks = vec![brick(30, 0, 0), brick(37, 7, 7)];
    (BrickBox::new(bricks[0], bricks[1]).unwrap(), bricks)
}

fn holds(bricks: &[BrickCoordinate], cube: BrickCube) -> bool {
    bricks.iter().any(|brick| cube.contains(*brick))
}

/// Whether the cube is one of `empty` or inside one.
fn is_inside_one_of(cube: BrickCube, empty: &[BrickCube]) -> bool {
    empty
        .iter()
        .any(|e| e.level() >= cube.level() && BrickCube::containing(cube.origin(), e.level()) == *e)
}

#[test]
fn the_server_says_empty_every_cube_the_client_may_draw_or_go_on_to_draw_that_it_does_not_send_and_that_has_no_brick()
 {
    let (bounds, bricks) = two_blocks();
    let holds = |cube: BrickCube| holds(&bricks, cube);
    // A client knows where a ship may have something by its box alone.
    let by_box = |cube: BrickCube| cube.meets(bounds);
    let mut some_empty_cube_was_needed = false;
    for viewer in [
        brick(0, 0, 0),
        brick(33, 3, 3),
        brick(34, 30, 3),
        brick(200, -40, 5),
    ] {
        let sent: HashSet<BrickCube> = far_ship_nodes_to_send(viewer, &view(), &holds, COARSEST)
            .into_iter()
            .collect();
        let empty = far_ship_empty_cubes(viewer, &view(), bounds, &holds, COARSEST);
        let drawn_by_the_client = select_far_ship(viewer, &view(), &by_box);
        for cube in &drawn_by_the_client {
            if holds(*cube) {
                assert!(
                    sent.contains(cube),
                    "{viewer:?}: {cube:?} is drawn and held and not sent"
                );
                // The halves it may be split into when the viewer comes nearer.
                if cube.level() >= 2 {
                    let half = 1i128 << (cube.level() - 1);
                    for octant in 0..8i128 {
                        let origin = brick(
                            cube.origin().x() + (octant & 1) * half,
                            cube.origin().y() + (octant >> 1 & 1) * half,
                            cube.origin().z() + (octant >> 2) * half,
                        );
                        let child = BrickCube::new(origin, cube.level() - 1).unwrap();
                        if child.meets(bounds) {
                            assert!(
                                sent.contains(&child) || is_inside_one_of(child, &empty),
                                "{viewer:?}: {child:?}, a half of {cube:?}, is neither sent nor said to be empty"
                            );
                        }
                    }
                }
            } else {
                some_empty_cube_was_needed = true;
                assert!(
                    is_inside_one_of(*cube, &empty),
                    "{viewer:?}: {cube:?} is drawn by a client, holds no brick and is not said to be empty ({empty:?})"
                );
            }
        }
    }
    assert!(
        some_empty_cube_was_needed,
        "the viewers do not test anything"
    );
}

#[test]
fn the_empty_cubes_are_the_biggest_that_hold_no_brick_each_once_coarsest_first_and_none_is_sent_or_holds_a_sent_one()
 {
    let (bounds, bricks) = two_blocks();
    let holds = |cube: BrickCube| holds(&bricks, cube);
    for viewer in [brick(0, 0, 0), brick(33, 3, 3), brick(34, 30, 3)] {
        let empty = far_ship_empty_cubes(viewer, &view(), bounds, &holds, COARSEST);
        let sent = far_ship_nodes_to_send(viewer, &view(), &holds, COARSEST);
        assert_eq!(
            empty.iter().collect::<HashSet<_>>().len(),
            empty.len(),
            "once each"
        );
        let levels: Vec<u8> = empty.iter().map(|cube| cube.level()).collect();
        assert!(
            levels.windows(2).all(|pair| pair[0] >= pair[1]),
            "{levels:?}"
        );
        assert_eq!(
            empty,
            far_ship_empty_cubes(viewer, &view(), bounds, &holds, COARSEST),
            "the same every time"
        );
        for cube in &empty {
            assert!(!holds(*cube), "{cube:?} holds a brick");
            assert!(cube.meets(bounds), "{cube:?} is outside the box");
            // The biggest: what holds it holds a brick (or is the coarsest there is).
            if cube.level() < COARSEST {
                let parent = BrickCube::containing(cube.origin(), cube.level() + 1);
                assert!(
                    holds(parent),
                    "{cube:?} is inside {parent:?}, which is empty too"
                );
            }
            assert!(
                sent.iter().all(|node| !is_inside_one_of(*node, &[*cube])),
                "{cube:?} is empty and a node inside it is sent"
            );
        }
    }
}

#[test]
fn a_ship_that_fills_its_box_has_no_empty_cube_to_say() {
    let bounds = BrickBox::new(brick(30, 0, 0), brick(37, 7, 7)).unwrap();
    let holds = |cube: BrickCube| cube.meets(bounds);
    for viewer in [brick(0, 0, 0), brick(33, 3, 3)] {
        assert_eq!(
            far_ship_empty_cubes(viewer, &view(), bounds, &holds, COARSEST),
            Vec::<BrickCube>::new(),
            "{viewer:?}"
        );
    }
}
