//! Bricks and coarse nodes drawn together: what is on the screen of a viewer on flat ground, standing and walking.

use std::collections::{HashMap, HashSet};

use coarse_cells::{CoarseCell, CoarseNode, DetailSettings};
use region_octree::BrickCube;
use voxel_meshing::{PackedBrickMesh, mesh_coarse_node};
use voxel_space::{BrickCoordinate, GridId, LevelIndex, WorldPosition};
use world_storage::test_support::brick;
use world_view::{LodConfig, LodSettings, LodView, MeshUpdate};

mod common;

use common::{Scene, scene};

/// What the renderer holds, as the updates say.
#[derive(Default)]
struct Screen {
    bricks: HashMap<BrickCoordinate, PackedBrickMesh>,
    nodes: HashMap<BrickCube, PackedBrickMesh>,
}

impl Screen {
    fn apply(&mut self, updates: Vec<MeshUpdate>) {
        for update in updates {
            match update {
                MeshUpdate::Upload { coordinate, mesh } if mesh.is_empty() => {
                    self.bricks.remove(&coordinate);
                }
                MeshUpdate::Upload { coordinate, mesh } => {
                    self.bricks.insert(coordinate, mesh);
                }
                MeshUpdate::Remove(coordinate) => {
                    self.bricks.remove(&coordinate);
                }
                MeshUpdate::UploadCoarse { cube, mesh } if mesh.is_empty() => {
                    self.nodes.remove(&cube);
                }
                MeshUpdate::UploadCoarse { cube, mesh } => {
                    self.nodes.insert(cube, mesh);
                }
                MeshUpdate::RemoveCoarse(cube) => {
                    self.nodes.remove(&cube);
                }
                other => panic!("an update for a ship: {other:?}"),
            }
        }
    }
}

fn settings() -> LodSettings {
    LodSettings {
        detail: DetailSettings {
            cell_pixels: 3.0,
            pixel_size: 2.0 * 0.700_207_538_209_062_3 / 1080.0,
        },
        zoom: None,
        maximum_distance: 1500.0,
    }
}

/// The same, drawn out to fewer voxels, for the tests that look at every state on the way.
fn nearer() -> LodSettings {
    LodSettings {
        maximum_distance: 700.0,
        ..settings()
    }
}

fn config() -> LodConfig {
    LodConfig {
        unlit: 0,
        worker_count: 3,
    }
}

fn viewer_in(at: BrickCoordinate) -> WorldPosition {
    WorldPosition::new(at, [16.0, 20.0, 16.0])
}

fn view_of(scene: &Scene, radius: i64) -> LodView {
    LodView::new(scene.mesher(radius), config())
}

/// The cubes and every cube that holds one of them, so that "something shown is inside this cube" is a lookup.
fn holders_of(cubes: &HashSet<BrickCube>) -> HashSet<BrickCube> {
    let mut holders = HashSet::new();
    for cube in cubes {
        for level in cube.level() + 1..=30 {
            holders.insert(BrickCube::containing(cube.origin(), level));
        }
    }
    holders
}

/// Whether anything shown covers a brick of the cube.
fn covered(cube: BrickCube, shown: &HashSet<BrickCube>, holders: &HashSet<BrickCube>) -> bool {
    shown.contains(&cube)
        || holders.contains(&cube)
        || (cube.level() + 1..=30)
            .any(|level| shown.contains(&BrickCube::containing(cube.origin(), level)))
}

fn assert_nothing_overlaps(shown: &HashSet<BrickCube>) {
    for cube in shown {
        for level in cube.level() + 1..=30 {
            let holder = BrickCube::containing(cube.origin(), level);
            assert!(
                !shown.contains(&holder),
                "{cube:?} is drawn inside {holder:?}, which is drawn"
            );
        }
    }
}

/// Everything on the screen is something shown, and everything shown that has something to see is on the screen.
fn assert_screen_matches(screen: &Screen, view: &LodView) {
    for coordinate in screen.bricks.keys() {
        assert!(
            view.shown()
                .contains(&BrickCube::new(*coordinate, 0).unwrap()),
            "{coordinate:?}"
        );
    }
    for cube in screen.nodes.keys() {
        assert!(view.shown().contains(cube), "{cube:?}");
    }
    for cube in view.shown() {
        // The ground is the bricks with y < 0 and, in the bricks with y = 0, up to 10: the bricks with y = 0 have its surface and those under it have no
        // surface to draw; a node has its sides to show, if it has any ground in it.
        let (drawn, has_something) = if cube.level() == 0 {
            (
                screen.bricks.contains_key(&cube.origin()),
                cube.origin().y() == 0,
            )
        } else {
            (screen.nodes.contains_key(cube), cube.origin().y() <= 0)
        };
        assert_eq!(drawn, has_something, "{cube:?}");
    }
}

#[test]
fn bricks_near_and_nodes_far_are_drawn_together_with_no_gap_and_no_overlap() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 10);
    let mut view = view_of(&scene, 3);
    let mut screen = Screen::default();
    screen.apply(view.settle(
        LevelIndex::MAIN,
        scene.world.main(),
        viewer_in(brick(0, 0, 0)),
        &settings(),
    ));

    assert!(!view.wanted().is_empty());
    assert_eq!(view.shown(), view.wanted(), "everything wanted is drawn");
    assert_nothing_overlaps(view.shown());
    assert_screen_matches(&screen, &view);
    let bricks = view.shown().iter().filter(|cube| cube.level() == 0).count();
    let nodes = view.shown().len() - bricks;
    assert!(bricks > 100, "{bricks} bricks");
    assert!(nodes > 100, "{nodes} nodes");
    // The bricks are those the region of 3 wants and no others; nothing beyond is a brick.
    let region = coarse_cells::FineRegion::new(brick(0, 0, 0), 3);
    for cube in view.shown().iter().filter(|cube| cube.level() == 0) {
        assert!(region.wants_brick(cube.origin()), "{cube:?}");
    }
    // Nodes of the ground have something to draw: grass on top.
    assert!(screen.nodes.values().any(|mesh| !mesh.quads.is_empty()));
}

#[test]
fn a_settled_view_has_nothing_more_to_do() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 10);
    let mut view = view_of(&scene, 3);
    view.settle(
        LevelIndex::MAIN,
        scene.world.main(),
        viewer_in(brick(0, 0, 0)),
        &settings(),
    );
    assert!(view.is_settled());
    assert_eq!(view.pending_jobs(), 0);
    assert!(
        view.update(
            LevelIndex::MAIN,
            scene.world.main(),
            viewer_in(brick(0, 0, 0)),
            &settings()
        )
        .is_empty()
    );
}

#[test]
fn walking_never_leaves_a_hole_where_something_was_and_is_wanted_and_never_draws_twice() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 40);
    let mut view = view_of(&scene, 3);
    let mut screen = Screen::default();
    let mut previous: HashSet<BrickCube> = HashSet::new();
    let mut checked = 0;
    for step in 0..8 {
        let viewer = viewer_in(brick(step, 0, 0));
        let started = std::time::Instant::now();
        loop {
            screen.apply(view.update(LevelIndex::MAIN, scene.world.main(), viewer, &nearer()));
            if view.shown() != &previous {
                checked += 1;
                assert_nothing_overlaps(view.shown());
                assert_screen_matches(&screen, &view);
                // What was drawn and is wanted now stays covered, by what was there or what replaced it.
                let holders = holders_of(view.shown());
                let wanted_holders = holders_of(view.wanted());
                for cube in &previous {
                    let still_wanted = view.wanted().contains(cube)
                        || wanted_holders.contains(cube)
                        || (cube.level() + 1..=30).any(|level| {
                            view.wanted()
                                .contains(&BrickCube::containing(cube.origin(), level))
                        });
                    if still_wanted {
                        assert!(
                            covered(*cube, view.shown(), &holders),
                            "step {step}: {cube:?} was drawn and is wanted still, and is not covered"
                        );
                    }
                }
                previous = view.shown().clone();
            }
            if view.is_settled() {
                break;
            }
            assert!(
                started.elapsed() < std::time::Duration::from_secs(120),
                "step {step} does not settle"
            );
            std::thread::sleep(std::time::Duration::from_micros(300));
        }
        assert_eq!(view.shown(), view.wanted(), "step {step}");
        // And what is wanted is what is wanted for the brick the viewer is in.
        let wanted: HashSet<BrickCube> = coarse_cells::select_around_brick(
            brick(step, 0, 0),
            &coarse_cells::ViewSelection {
                detail: nearer().detail,
                zoom: None,
                fine_radius: 3,
                maximum_distance: nearer().maximum_distance,
            },
        )
        .into_iter()
        .collect();
        assert_eq!(view.wanted(), &wanted, "step {step}");
    }
    assert!(checked > 8, "{checked} states were looked at");
}

/// Finds a node of the ground that is drawn.
fn a_drawn_node(view: &LodView, screen: &Screen) -> BrickCube {
    let mut nodes: Vec<BrickCube> = screen.nodes.keys().copied().collect();
    nodes.sort_by_key(|cube| {
        let origin = cube.origin();
        (cube.level(), origin.x(), origin.y(), origin.z())
    });
    let cube = nodes[nodes.len() / 2];
    assert!(view.shown().contains(&cube));
    cube
}

fn node_of(block: u32) -> CoarseNode {
    CoarseNode::uniform(CoarseCell {
        block,
        fill: 255,
        light: 255,
        exposure: 0,
    })
}

#[test]
fn what_the_server_says_replaces_the_picture_of_a_node_where_it_is_and_taking_it_back_restores_it()
{
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 10);
    let mut view = view_of(&scene, 3);
    let mut screen = Screen::default();
    let viewer = viewer_in(brick(0, 0, 0));
    screen.apply(view.settle(LevelIndex::MAIN, scene.world.main(), viewer, &settings()));
    let cube = a_drawn_node(&view, &screen);
    let original = screen.nodes[&cube].clone();
    let shown_before = view.shown().clone();

    // A node of nothing: the picture is taken down and the node is still the one that is drawn.
    let air = scene.air;
    view.set_override(
        GridId::MAIN,
        cube,
        Some(CoarseNode::uniform(CoarseCell::empty(air, 0))),
    );
    screen.apply(view.settle(LevelIndex::MAIN, scene.world.main(), viewer, &settings()));
    assert!(!screen.nodes.contains_key(&cube));
    assert_eq!(view.shown(), &shown_before);

    // A node of one block: the picture of a cube of that block.
    let stone = scene.game.raw_block("base_blocks:stone");
    view.set_override(GridId::MAIN, cube, Some(node_of(stone)));
    screen.apply(view.settle(LevelIndex::MAIN, scene.world.main(), viewer, &settings()));
    assert_eq!(
        screen.nodes[&cube],
        mesh_coarse_node(&node_of(stone), &[None; 6]).pack()
    );
    assert_eq!(view.shown(), &shown_before);

    // Taken back: what the generators give.
    view.set_override(GridId::MAIN, cube, None);
    screen.apply(view.settle(LevelIndex::MAIN, scene.world.main(), viewer, &settings()));
    assert_eq!(screen.nodes[&cube], original);
}

#[test]
fn the_last_thing_said_is_what_is_drawn_and_a_picture_made_from_what_was_said_before_is_never_put_up()
 {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 10);
    let mut view = view_of(&scene, 3);
    let mut screen = Screen::default();
    let viewer = viewer_in(brick(0, 0, 0));
    screen.apply(view.settle(LevelIndex::MAIN, scene.world.main(), viewer, &settings()));
    let cube = a_drawn_node(&view, &screen);
    let stone = scene.game.raw_block("base_blocks:stone");
    let dirt = scene.game.raw_block("base_blocks:dirt");
    let of_stone = mesh_coarse_node(&node_of(stone), &[None; 6]).pack();
    let of_dirt = mesh_coarse_node(&node_of(dirt), &[None; 6]).pack();
    for _ in 0..3 {
        // The picture of the first is being made when the second is said: whenever it is done, it is for something that is no longer so.
        view.set_override(GridId::MAIN, cube, Some(node_of(stone)));
        let started = view.update(LevelIndex::MAIN, scene.world.main(), viewer, &settings());
        view.set_override(GridId::MAIN, cube, Some(node_of(dirt)));
        let later = view.settle(LevelIndex::MAIN, scene.world.main(), viewer, &settings());
        for update in started.iter().chain(&later) {
            assert!(
                !matches!(update, MeshUpdate::UploadCoarse { cube: found, mesh } if *found == cube && *mesh == of_stone),
                "a picture of the first thing said was put up after the second"
            );
        }
        screen.apply(started);
        screen.apply(later);
        assert_eq!(screen.nodes[&cube], of_dirt);
        // And back to stone for the next round.
        view.set_override(GridId::MAIN, cube, Some(node_of(stone)));
        screen.apply(view.settle(LevelIndex::MAIN, scene.world.main(), viewer, &settings()));
        assert_eq!(screen.nodes[&cube], of_stone);
    }
}

#[test]
fn what_is_said_of_another_level_or_of_a_cube_that_is_not_drawn_changes_nothing_here() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 10);
    let mut view = view_of(&scene, 3);
    let mut screen = Screen::default();
    let viewer = viewer_in(brick(0, 0, 0));
    screen.apply(view.settle(LevelIndex::MAIN, scene.world.main(), viewer, &settings()));
    let cube = a_drawn_node(&view, &screen);
    let before: HashMap<BrickCube, PackedBrickMesh> = screen.nodes.clone();
    let stone = scene.game.raw_block("base_blocks:stone");
    view.set_override(GridId::root(LevelIndex::new(1)), cube, Some(node_of(stone)));
    let far = BrickCube::containing(brick(1_000_000, 0, 0), 3);
    view.set_override(GridId::MAIN, far, Some(node_of(stone)));
    let updates = view.settle(LevelIndex::MAIN, scene.world.main(), viewer, &settings());
    assert!(updates.is_empty(), "{updates:?}");
    assert_eq!(screen.nodes, before);

    // Nor does it when the view is worked out again from a brick over: the cube is drawn as it was, and no picture of it is made again.
    let moved = view.settle(
        LevelIndex::MAIN,
        scene.world.main(),
        viewer_in(brick(0, 0, 1)),
        &settings(),
    );
    assert!(
        !moved.iter().any(
            |update| matches!(update, MeshUpdate::UploadCoarse { cube: found, .. } if *found == cube)
        ),
        "{moved:?}"
    );
}

#[test]
fn what_is_said_of_a_brick_is_ignored() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 10);
    let mut view = view_of(&scene, 3);
    let stone = scene.game.raw_block("base_blocks:stone");
    view.set_override(
        GridId::MAIN,
        BrickCube::containing(brick(0, 0, 0), 0),
        Some(node_of(stone)),
    );
    let updates = view.settle(
        LevelIndex::MAIN,
        scene.world.main(),
        viewer_in(brick(0, 0, 0)),
        &settings(),
    );
    assert!(!updates.is_empty());
}

#[test]
fn a_reset_takes_everything_down_and_the_view_is_built_again_for_the_new_level() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 10);
    let mut view = view_of(&scene, 3);
    let mut screen = Screen::default();
    let viewer = viewer_in(brick(0, 0, 0));
    screen.apply(view.settle(LevelIndex::MAIN, scene.world.main(), viewer, &settings()));
    assert!(!screen.nodes.is_empty());
    screen.apply(view.reset());
    assert!(screen.nodes.is_empty());
    assert!(screen.bricks.is_empty());
    assert!(view.shown().is_empty());
    screen.apply(view.settle(LevelIndex::MAIN, scene.world.main(), viewer, &settings()));
    assert_eq!(view.shown(), view.wanted());
    assert_screen_matches(&screen, &view);
}

#[test]
fn an_edit_to_a_brick_that_is_drawn_changes_its_picture_as_before() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 10);
    let mut view = view_of(&scene, 3);
    let mut screen = Screen::default();
    let viewer = viewer_in(brick(0, 0, 0));
    screen.apply(view.settle(LevelIndex::MAIN, scene.world.main(), viewer, &settings()));
    let before = screen.bricks[&brick(0, 0, 0)].clone();
    let edited = scene.edit([5, 9, 5], "base_blocks:air");
    view.mesher_mut().brick_changed(edited);
    screen.apply(view.settle(LevelIndex::MAIN, scene.world.main(), viewer, &settings()));
    assert_ne!(screen.bricks[&brick(0, 0, 0)], before);
    assert_nothing_overlaps(view.shown());
}
