//! Bricks and coarse nodes drawn together: what is on the screen of a viewer on flat ground, standing and walking.

use std::collections::{HashMap, HashSet};

use std::cell::Cell;
use std::sync::Arc;

use coarse_cells::{CoarseCell, CoarseNode, DetailSettings, ViewSelection, select_around_brick};
use proptest::prelude::*;
use region_octree::BrickCube;
use terrain::TerrainGenerator;
use voxel_grid::{Brick, GridLayout};
use voxel_meshing::{BrickOccupancy, PackedBrickMesh, mesh_coarse_node};
use voxel_space::{BrickCoordinate, FaceDirection, GridId, LevelIndex, WorldPosition};
use world_storage::LevelData;
use world_storage::test_support::{Plugin, brick};
use world_view::{
    CoarseTerrainSource, LodConfig, LodSettings, LodView, MeshUpdate, OccupancyChange,
    TerrainSource,
};

mod common;

use common::{Scene, scene, scene_of};

/// What the renderer holds, as the updates say.
#[derive(Default)]
struct Screen {
    bricks: HashMap<BrickCoordinate, PackedBrickMesh>,
    /// The bricks the renderer holds and does not draw.
    hidden_bricks: HashMap<BrickCoordinate, PackedBrickMesh>,
    /// What the renderer has been told of which voxels block the sun, if the mesher was asked to say.
    occupancy: HashMap<BrickCoordinate, BrickOccupancy>,
    /// The nodes that are drawn.
    nodes: HashMap<BrickCube, PackedBrickMesh>,
    /// The nodes the renderer holds and does not draw.
    hidden: HashMap<BrickCube, PackedBrickMesh>,
}

impl Screen {
    /// What the mesher has said of the occupancy of its bricks (nothing unless it was asked to).
    fn apply_occupancy(&mut self, changes: Vec<(BrickCoordinate, OccupancyChange)>) {
        for (coordinate, change) in changes {
            match change {
                OccupancyChange::Set(occupancy) => {
                    self.occupancy.insert(coordinate, occupancy);
                }
                OccupancyChange::Clear => {
                    self.occupancy.remove(&coordinate);
                }
            }
        }
    }

    fn apply(&mut self, updates: Vec<MeshUpdate>) {
        for update in updates {
            match update {
                MeshUpdate::Upload { coordinate, mesh } if mesh.is_empty() => {
                    self.bricks.remove(&coordinate);
                }
                MeshUpdate::Upload { coordinate, mesh } => {
                    assert!(
                        !self.hidden_bricks.contains_key(&coordinate),
                        "{coordinate:?} is uploaded where the renderer holds a hidden mesh of it"
                    );
                    self.bricks.insert(coordinate, mesh);
                }
                MeshUpdate::Remove(coordinate) => {
                    self.bricks.remove(&coordinate);
                    self.hidden_bricks.remove(&coordinate);
                }
                MeshUpdate::HideBrick(coordinate) => {
                    let mesh = self
                        .bricks
                        .remove(&coordinate)
                        .unwrap_or_else(|| panic!("{coordinate:?} is hidden and was not drawn"));
                    self.hidden_bricks.insert(coordinate, mesh);
                }
                MeshUpdate::ShowBrick(coordinate) => {
                    let mesh = self
                        .hidden_bricks
                        .remove(&coordinate)
                        .unwrap_or_else(|| panic!("{coordinate:?} is shown and was not hidden"));
                    self.bricks.insert(coordinate, mesh);
                }
                MeshUpdate::UploadCoarse { cube, mesh } if mesh.is_empty() => {
                    self.nodes.remove(&cube);
                    self.hidden.remove(&cube);
                }
                // (As the renderer does it: a mesh that is held hidden is replaced where it is, and stays hidden.)
                MeshUpdate::UploadCoarse { cube, mesh } => {
                    if let Some(held) = self.hidden.get_mut(&cube) {
                        *held = mesh;
                    } else {
                        self.nodes.insert(cube, mesh);
                    }
                }
                MeshUpdate::RemoveCoarse(cube) => {
                    self.nodes.remove(&cube);
                    self.hidden.remove(&cube);
                }
                MeshUpdate::HideCoarse(cube) => {
                    let mesh = self
                        .nodes
                        .remove(&cube)
                        .unwrap_or_else(|| panic!("{cube:?} is hidden and was not drawn"));
                    self.hidden.insert(cube, mesh);
                }
                MeshUpdate::ShowCoarse(cube) => {
                    let mesh = self
                        .hidden
                        .remove(&cube)
                        .unwrap_or_else(|| panic!("{cube:?} is shown and was not hidden"));
                    self.nodes.insert(cube, mesh);
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

fn view_of(scene: &Scene, radius: i128) -> LodView {
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
        GridId::MAIN,
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
        GridId::MAIN,
        scene.world.main(),
        viewer_in(brick(0, 0, 0)),
        &settings(),
    );
    assert!(view.is_settled());
    assert_eq!(view.pending_jobs(), 0);
    assert!(
        view.update(
            GridId::MAIN,
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
            screen.apply(view.update(GridId::MAIN, scene.world.main(), viewer, &nearer()));
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
            &|cube| scene.world.main().terrain.may_show(cube),
        )
        .into_iter()
        .collect();
        assert_eq!(view.wanted(), &wanted, "step {step}");
    }
    assert!(checked > 8, "{checked} states were looked at");
}

/// The first few of some cubes, in an order that does not change, for a message.
fn first_of(cubes: impl Iterator<Item = BrickCube>) -> Vec<(u8, [i128; 3])> {
    let mut found: Vec<(u8, [i128; 3])> = cubes
        .map(|cube| {
            let origin = cube.origin();
            (cube.level(), [origin.x(), origin.y(), origin.z()])
        })
        .collect();
    found.sort();
    found.truncate(6);
    found
}

/// What is drawn is what is wanted, or says what differs (as detail level and brick coordinates, the first few of each).
fn assert_converged(view: &LodView, context: &str) {
    let missing = view.wanted().difference(view.shown()).count();
    let extra = view.shown().difference(view.wanted()).count();
    assert!(
        missing == 0 && extra == 0,
        "{context}: {missing} cubes are wanted and not drawn, {extra} are drawn and not wanted; first of each: {:?} and {:?}",
        first_of(view.wanted().difference(view.shown()).copied()),
        first_of(view.shown().difference(view.wanted()).copied()),
    );
}

/// Updates until the view has nothing more to do, as `LodView::settle` does, but with a deadline and the mesher's bookkeeping checked at every update (a view
/// that waits for ever is a failure, not a test that never ends).
fn settle_checked(
    view: &mut LodView,
    screen: &mut Screen,
    scene: &Scene,
    viewer: WorldPosition,
    settings: &LodSettings,
    context: &str,
) {
    settle_checked_in(view, screen, scene.world.main(), viewer, settings, context);
}

/// The same, for any terrain.
fn settle_checked_in(
    view: &mut LodView,
    screen: &mut Screen,
    terrain: &impl CoarseTerrainSource,
    viewer: WorldPosition,
    settings: &LodSettings,
    context: &str,
) {
    let started = std::time::Instant::now();
    loop {
        screen.apply(view.update(GridId::MAIN, terrain, viewer, settings));
        screen.apply_occupancy(view.mesher_mut().take_occupancy_updates());
        if let Err(problem) = view.check_consistency() {
            panic!("{context}: the view's records disagree: {problem}");
        }
        if view.is_settled() {
            return;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "{context}: does not settle"
        );
        std::thread::sleep(std::time::Duration::from_micros(300));
    }
}

#[test]
fn walking_away_and_back_draws_what_is_wanted_again_and_does_not_stay_a_node_among_bricks() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 40);
    let mut view = view_of(&scene, 3);
    let mut screen = Screen::default();
    // Out and back over bricks that were drawn as bricks, a few bricks at a time (within what the mesher keeps loaded), several times.
    for (round, at) in [0, 3, 0, 2, 0, 4, 1, 0].into_iter().enumerate() {
        let context = format!("round {round}, at {at}");
        settle_checked(
            &mut view,
            &mut screen,
            &scene,
            viewer_in(brick(at, 0, 0)),
            &nearer(),
            &context,
        );
        assert_converged(&view, &context);
        assert_nothing_overlaps(view.shown());
        assert_screen_matches(&screen, &view);
    }
}

/// A view that jumps from far away to the ground is refined a level at a time (`docs/specs/viewpoints-spec.md` 6.2, item 11): the picture it had stays only until the
/// halves of it are made, and so on down, and not until everything that replaces it is. Found in phase 0: from 264 km up back to the surface the old giant nodes stayed
/// for the whole of the build.
#[test]
fn a_view_that_jumps_from_far_away_to_the_ground_is_refined_a_level_at_a_time_and_never_has_a_hole()
{
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 40);
    let mut view = view_of(&scene, 3);
    let mut screen = Screen::default();
    let far_settings = LodSettings {
        maximum_distance: 4.0e4,
        ..settings()
    };
    // From 100,000 voxels up: a few big nodes.
    settle_checked(
        &mut view,
        &mut screen,
        &scene,
        viewer_in(brick(0, 400, 0)),
        &far_settings,
        "far away",
    );
    let before: HashSet<BrickCube> = view.shown().clone();
    assert!(
        before.iter().all(|cube| cube.level() >= 5),
        "the far picture is of big nodes: {:?}",
        first_of(before.iter().copied())
    );
    // Down to the ground.
    let viewer = viewer_in(brick(0, 0, 0));
    let started = std::time::Instant::now();
    let mut previous = before.clone();
    let mut between_seen = 0;
    let mut states = 0;
    loop {
        screen.apply(view.update(GridId::MAIN, scene.world.main(), viewer, &far_settings));
        if view.shown() != &previous {
            states += 1;
            assert_nothing_overlaps(view.shown());
            // (What the renderer holds is drawn. A big node of the ground has nothing to see in it, so not every shown cube has a mesh, which `assert_screen_matches` asks.)
            for cube in screen.nodes.keys() {
                assert!(
                    view.shown().contains(cube),
                    "{cube:?} is on the screen and not shown"
                );
            }
            if let Err(problem) = view.check_consistency() {
                panic!("the view's records disagree: {problem}");
            }
            // A cube that is neither what was drawn before nor what is wanted: a step on the way.
            if view
                .shown()
                .iter()
                .any(|cube| !before.contains(cube) && !view.wanted().contains(cube))
            {
                between_seen += 1;
            }
            // Where something was drawn and something is wanted, something is drawn still: the ground is never bare.
            let holders = holders_of(view.shown());
            for cube in &previous {
                let wanted_inside = view.wanted().iter().any(|wanted| {
                    wanted.level() <= cube.level()
                        && BrickCube::containing(wanted.origin(), cube.level()) == *cube
                });
                let wanted_over = view.wanted().contains(cube)
                    || (cube.level() + 1..=30).any(|level| {
                        view.wanted()
                            .contains(&BrickCube::containing(cube.origin(), level))
                    });
                if wanted_inside || wanted_over {
                    assert!(
                        covered(*cube, view.shown(), &holders),
                        "{cube:?} was drawn and has something wanted in it, and is not covered now"
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
            "does not settle"
        );
        std::thread::sleep(std::time::Duration::from_micros(50));
    }
    assert_converged(&view, "on the ground");
    assert!(
        between_seen >= 2,
        "the picture went from the far one to the near one with {between_seen} steps on the way ({states} states): it waited for everything"
    );
}

/// How many nodes a view hands its threads at a time sets how fast it builds at the rate the screen is updated, because it hands out more only when it is updated: at 4 for
/// each thread, 11 threads were given 44 nodes a frame, and the 15,000 nodes of a view from 264 km up back to the ground took 7.5 s at 60 frames a second where the threads
/// can do it in 3.2 s (`job_feed.rs`; measured 2026-11-21, `docs/history/build-log.md`). So a view whose threads finished everything they were handed before the next update
/// hands them more, and never the whole queue: what is handed cannot be taken back when the view moves on.
#[test]
fn a_view_whose_threads_run_dry_hands_them_more_and_never_the_whole_queue() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 40);
    let workers = 2;
    let mut view = LodView::new(
        scene.mesher(3),
        LodConfig {
            unlit: 0,
            worker_count: workers,
        },
    );
    let viewer = viewer_in(brick(0, 0, 0));
    view.update(GridId::MAIN, scene.world.main(), viewer, &settings());
    assert!(view.wanted().len() > 1000, "{} cubes", view.wanted().len());
    // The threads have finished nothing yet: the least is handed out, 4 for each.
    assert_eq!(view.nodes_being_made(), workers * 4);
    // Given time to finish them (the condition is that they have, which the view says by handing out more), the threads are handed more, up to a most.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut most = 0;
    while most <= workers * 4 {
        assert!(
            std::time::Instant::now() < deadline,
            "never handed more than {most}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
        view.update(GridId::MAIN, scene.world.main(), viewer, &settings());
        most = most.max(view.nodes_being_made());
        assert!(
            view.nodes_being_made() <= workers * 64,
            "{} nodes handed out to {workers} threads",
            view.nodes_being_made()
        );
    }
}

/// One thing a player does.
#[derive(Clone, Debug)]
enum Move {
    /// A step or a few along one axis: what walking does.
    Step { axis: usize, bricks: i128 },
    /// Back to a place already been to (which one, counted from the first, round the places there are).
    Return { which: usize },
    /// Somewhere else on the ground.
    Jump { x: i128, z: i128 },
    /// A voxel dug in the ground under the viewer, which the mesher is told of.
    Dig,
    /// The server says a node that is drawn is another (one of stone, or of nothing).
    Override { which: usize, stone: bool },
    /// The server takes back something it said.
    TakeBack { which: usize },
    /// The player asks for a finer or a coarser picture (the cubes are worked out again though the viewer is where it was).
    Detail { cell_pixels: u8 },
    /// The viewer goes to another level and comes back: everything is taken down and built again.
    Reset,
}

fn moves() -> impl Strategy<Value = Vec<Move>> {
    prop::collection::vec(
        prop_oneof![
            4 => (0usize..3, -3i128..=3).prop_map(|(axis, bricks)| Move::Step { axis, bricks }),
            3 => (0usize..100).prop_map(|which| Move::Return { which }),
            1 => (-20i128..=20, -20i128..=20).prop_map(|(x, z)| Move::Jump { x, z }),
            1 => Just(Move::Dig),
            1 => (0usize..1000, any::<bool>()).prop_map(|(which, stone)| Move::Override { which, stone }),
            1 => (0usize..100).prop_map(|which| Move::TakeBack { which }),
            1 => (2u8..=6).prop_map(|cell_pixels| Move::Detail { cell_pixels }),
            1 => Just(Move::Reset),
        ],
        2..10,
    )
}

/// Drawn out to fewer voxels than `nearer`: the paths are many, and what they check does not depend on how far the nodes reach.
fn steady() -> LodSettings {
    LodSettings {
        maximum_distance: 350.0,
        ..settings()
    }
}

proptest! {
    // Eight paths by default, which is a few seconds; `PROPTEST_CASES=500` searches further.
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|cases| cases.parse().ok()).unwrap_or(8),
        ..ProptestConfig::default()
    })]

    /// The view is drawing what is wanted whenever it has nothing more to do, wherever the viewer has been and however it got there: back to what it has left, to somewhere
    /// else, with the ground dug under it. (A cube that is wanted and is not drawn, with nothing being made that would bring it, is a view that waits for ever.)
    #[test]
    fn whatever_path_the_viewer_takes_the_view_ends_up_drawing_what_is_wanted(path in moves()) {
        follow_path(&path, 0);
    }

    /// The same with the pictures that leave the screen held, under a budget small enough that some are let go of (an eighth of a megabyte is a few hundred nodes).
    #[test]
    fn whatever_path_the_viewer_takes_the_view_ends_up_drawing_what_is_wanted_with_pictures_held(path in moves()) {
        follow_path(&path, 100_000);
    }
}

/// A viewer follows `path`, and the view is drawing what is wanted each time it comes to rest; with `held_budget` bytes of pictures held if it is more than 0.
fn follow_path(path: &[Move], held_budget: usize) {
    {
        let mut scene = scene();
        scene.assign_around(brick(0, 0, 0), 60);
        let mut view = view_of(&scene, 3);
        view.hold_hidden_pictures(held_budget);
        if held_budget > 0 {
            // (What the sun is told is looked at too, with the pictures: a brick that is shown from hold is told again.)
            view.mesher_mut().enable_occupancy();
        }
        let mut screen = Screen::default();
        let mut places = vec![[0i128, 0, 0]];
        let mut detail = steady();
        // What the server has said, to take back.
        let mut said: Vec<BrickCube> = Vec::new();
        let stone = scene.game.raw_block("base_blocks:stone");
        settle_checked(
            &mut view,
            &mut screen,
            &scene,
            viewer_in(brick(0, 0, 0)),
            &detail,
            "the start",
        );
        assert_converged(&view, "the start");
        for (index, step) in path.iter().enumerate() {
            let mut at = *places.last().expect("it starts somewhere");
            match step {
                Move::Step { axis, bricks } => at[*axis] += bricks,
                Move::Return { which } => at = places[which % places.len()],
                Move::Jump { x, z } => at = [*x, 0, *z],
                Move::Dig => {
                    let edited = scene.edit([at[0] * 32 + 5, 9, at[2] * 32 + 5], "base_blocks:air");
                    view.mesher_mut().brick_changed(edited);
                }
                Move::Override {
                    which,
                    stone: is_stone,
                } => {
                    let mut nodes: Vec<BrickCube> = view
                        .shown()
                        .iter()
                        .copied()
                        .filter(|cube| cube.level() > 0)
                        .collect();
                    nodes.sort_by_key(|cube| {
                        (
                            cube.level(),
                            cube.origin().x(),
                            cube.origin().y(),
                            cube.origin().z(),
                        )
                    });
                    if !nodes.is_empty() {
                        let cube = nodes[which % nodes.len()];
                        let node = if *is_stone {
                            node_of(stone)
                        } else {
                            CoarseNode::uniform(CoarseCell::empty(scene.air, 0))
                        };
                        view.set_override(GridId::MAIN, cube, Some(node));
                        said.push(cube);
                    }
                }
                Move::TakeBack { which } => {
                    if !said.is_empty() {
                        let cube = said.remove(which % said.len());
                        view.set_override(GridId::MAIN, cube, None);
                    }
                }
                Move::Detail { cell_pixels } => detail.detail.cell_pixels = f64::from(*cell_pixels),
                Move::Reset => screen.apply(view.reset()),
            }
            // Within the ground that has generators, and not far above or below it.
            at = [
                at[0].clamp(-30, 30),
                at[1].clamp(-2, 2),
                at[2].clamp(-30, 30),
            ];
            places.push(at);
            let context = format!("move {index} {step:?}, at {at:?}, after {places:?}");
            settle_checked(
                &mut view,
                &mut screen,
                &scene,
                viewer_in(brick(at[0], at[1], at[2])),
                &detail,
                &context,
            );
            assert_converged(&view, &context);
            assert_nothing_overlaps(view.shown());
            // (The ground is flat and the pictures are what the generators make of it, until the server has said otherwise.)
            if said.is_empty() {
                assert_screen_matches(&screen, &view);
            }
            assert_held_pictures_match(&screen, &view, held_budget, &context);
            if held_budget > 0 {
                // The bricks are those of a view that has been nowhere else, whatever was held on the way, and edited while it was held.
                assert_held_bricks_match(&screen, &view, held_budget, &context);
                let fresh = what_a_fresh_view_draws(&scene, brick(at[0], at[1], at[2]), &detail);
                assert_bricks_are_what_a_fresh_view_draws(&screen, &fresh, &context);
            }
        }
    }
}

/// What the renderer holds hidden is what the view holds, within the budget, and none of it is drawn or shown.
fn assert_held_pictures_match(screen: &Screen, view: &LodView, budget: usize, context: &str) {
    let held = view.held_pictures();
    assert_eq!(
        screen.hidden.len(),
        held.with_mesh,
        "{context}: the renderer holds {} hidden meshes and the view says it holds {}",
        screen.hidden.len(),
        held.with_mesh
    );
    for cube in screen.hidden.keys() {
        assert!(
            !screen.nodes.contains_key(cube) && !view.shown().contains(cube),
            "{context}: {cube:?} is hidden and drawn or shown"
        );
    }
    assert!(
        held.bytes <= budget,
        "{context}: {} bytes are held of {budget}",
        held.bytes
    );
    if budget == 0 {
        assert!(screen.hidden.is_empty(), "{context}: nothing is to be held");
    }
}

/// The terrain of a level, that counts how many nodes the view asks it to make.
struct Making<'a> {
    level: &'a LevelData,
    made: &'a Cell<u64>,
}

impl TerrainSource for Making<'_> {
    fn known(&self, coordinate: BrickCoordinate) -> Option<Arc<Brick>> {
        self.level.known(coordinate)
    }

    fn generator_for(&self, coordinate: BrickCoordinate) -> Option<Arc<dyn TerrainGenerator>> {
        self.level.generator_for(coordinate)
    }

    fn layout(&self) -> &Arc<GridLayout> {
        self.level.layout()
    }
}

impl CoarseTerrainSource for Making<'_> {
    fn version(&self) -> u64 {
        self.level.version()
    }

    fn may_show(&self, cube: BrickCube) -> bool {
        self.level.may_show(cube)
    }

    fn coarse_generator(&self, cube: BrickCube) -> Option<Arc<dyn TerrainGenerator>> {
        self.made.set(self.made.get() + 1);
        self.level.coarse_generator(cube)
    }
}

/// What a view that goes from the ground to far above it and back is given to do, for the tests of held pictures: the view, what is on the screen at the ground, how many
/// nodes were made to draw it, and the settings of a view that reaches far.
struct Trip {
    scene: Scene,
    far: LodSettings,
}

impl Trip {
    fn new() -> Self {
        let mut scene = scene();
        scene.assign_around(brick(0, 0, 0), 40);
        let far = LodSettings {
            maximum_distance: 4.0e4,
            ..settings()
        };
        Self { scene, far }
    }

    fn ground() -> WorldPosition {
        viewer_in(brick(0, 0, 0))
    }

    fn sky() -> WorldPosition {
        viewer_in(brick(0, 400, 0))
    }
}

#[test]
fn a_view_that_goes_up_and_comes_back_makes_no_node_again_and_draws_what_it_drew() {
    let trip = Trip::new();
    let made = Cell::new(0);
    let terrain = Making {
        level: trip.scene.world.main(),
        made: &made,
    };
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    let mut screen = Screen::default();

    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        Trip::ground(),
        &trip.far,
        "the ground",
    );
    let (ground_nodes, ground_shown) = (screen.nodes.clone(), view.shown().clone());
    let made_for_the_ground = made.get();
    assert!(ground_nodes.len() > 100 && made_for_the_ground > 100);
    assert!(screen.hidden.is_empty(), "nothing has been taken down yet");

    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        Trip::sky(),
        &trip.far,
        "the sky",
    );
    // Each node of the ground is drawn from the sky, if the sky wants it too (the biggest are in both), and held, hidden, if not.
    let mut held_from_the_ground = 0;
    for cube in ground_nodes.keys() {
        let drawn = screen.nodes.contains_key(cube);
        let hidden = screen.hidden.contains_key(cube);
        assert_eq!(
            drawn,
            view.wanted().contains(cube),
            "{cube:?} is drawn from the sky if the sky wants it"
        );
        assert_eq!(hidden, !drawn, "{cube:?} is drawn or held");
        held_from_the_ground += usize::from(hidden);
    }
    assert!(
        held_from_the_ground * 4 >= ground_nodes.len() * 3,
        "{held_from_the_ground} of {} nodes of the ground are held",
        ground_nodes.len()
    );
    assert_held_pictures_match(&screen, &view, 256 << 20, "the sky");
    let made_before_coming_back = made.get();

    // Back: what was drawn is drawn again, and what is made is a few steps on the way, not the nodes that were held.
    let mut uploads = 0;
    let mut shows = 0;
    let started = std::time::Instant::now();
    loop {
        let updates = view.update(GridId::MAIN, &terrain, Trip::ground(), &trip.far);
        uploads += updates
            .iter()
            .filter(|update| matches!(update, MeshUpdate::UploadCoarse { .. }))
            .count();
        shows += updates
            .iter()
            .filter(|update| matches!(update, MeshUpdate::ShowCoarse(_)))
            .count();
        screen.apply(updates);
        view.check_consistency().expect("the records agree");
        if view.is_settled() {
            break;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "does not settle"
        );
        std::thread::sleep(std::time::Duration::from_micros(300));
    }
    let made_coming_back = made.get() - made_before_coming_back;
    println!(
        "ground: {} nodes shown, {made_for_the_ground} made; back from the sky: {made_coming_back} made, {uploads} uploaded, {shows} shown from hold",
        ground_nodes.len()
    );
    assert_eq!(view.shown(), &ground_shown, "the same cubes are drawn");
    assert_eq!(screen.nodes, ground_nodes, "with the same pictures");
    assert_converged(&view, "back on the ground");
    assert!(
        shows >= ground_nodes.len() / 2,
        "{shows} of {} nodes were shown from hold",
        ground_nodes.len()
    );
    assert!(
        made_coming_back * 50 <= made_for_the_ground,
        "{made_coming_back} nodes were made coming back, {made_for_the_ground} for the ground: it is built again"
    );
    assert!(
        uploads * 10 <= ground_nodes.len(),
        "{uploads} pictures were uploaded coming back, of {} nodes",
        ground_nodes.len()
    );
}

#[test]
fn the_cap_of_pictures_shown_in_an_update_does_not_hold_back_pictures_that_are_held() {
    let trip = Trip::new();
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    let mut screen = Screen::default();
    let terrain = trip.scene.world.main();
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::ground(),
        &trip.far,
        "the ground",
    );
    let ground_nodes = screen.nodes.len();
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::sky(),
        &trip.far,
        "the sky",
    );

    // In one update after the viewer is back, more nodes are shown than the cap of an update allows to be put on the screen (64): they are moves.
    let updates = view.update(GridId::MAIN, terrain, Trip::ground(), &trip.far);
    let shows = updates
        .iter()
        .filter(|update| matches!(update, MeshUpdate::ShowCoarse(_)))
        .count();
    assert!(
        shows > 64,
        "{shows} nodes were shown in the first update of {ground_nodes}"
    );
}

#[test]
fn the_budget_of_held_pictures_lets_go_of_the_oldest_and_what_is_let_go_of_is_made_again() {
    let trip = Trip::new();
    let terrain = trip.scene.world.main();
    // How much the ground takes, held.
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::ground(),
        &trip.far,
        "the ground",
    );
    let ground_nodes = screen.nodes.clone();
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::sky(),
        &trip.far,
        "the sky",
    );
    let all = view.held_pictures();
    assert!(all.with_mesh > 100, "{all:?}");

    // A budget of a third of that: the rest is let go of, and the ground is made again where it was.
    let budget = all.bytes / 3;
    let made = Cell::new(0);
    let counted = Making {
        level: terrain,
        made: &made,
    };
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(budget);
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        &counted,
        Trip::ground(),
        &trip.far,
        "the ground",
    );
    let made_for_the_ground = made.get();
    settle_checked_in(
        &mut view,
        &mut screen,
        &counted,
        Trip::sky(),
        &trip.far,
        "the sky",
    );
    assert_held_pictures_match(&screen, &view, budget, "the sky");
    let held = view.held_pictures();
    assert!(
        held.bytes > budget / 2 && held.with_mesh < all.with_mesh,
        "{held:?} of {all:?} held for {budget}"
    );
    let before = made.get();
    settle_checked_in(
        &mut view,
        &mut screen,
        &counted,
        Trip::ground(),
        &trip.far,
        "back",
    );
    let made_again = made.get() - before;
    assert_eq!(screen.nodes, ground_nodes, "the same pictures");
    assert!(
        made_again > 0 && made_again < made_for_the_ground,
        "{made_again} made again of {made_for_the_ground}: some were held and some were not"
    );
}

#[test]
fn a_held_picture_of_a_node_the_server_has_changed_since_is_not_shown_and_the_node_is_made_again() {
    let trip = Trip::new();
    let terrain = trip.scene.world.main();
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::ground(),
        &trip.far,
        "the ground",
    );
    let ground_nodes = screen.nodes.clone();
    let cube = a_drawn_node(&view, &screen);
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::sky(),
        &trip.far,
        "the sky",
    );
    assert!(screen.hidden.contains_key(&cube), "{cube:?} is held");

    // The server says what is in it while it is held: nothing is done until it is wanted.
    let stone = trip.scene.game.raw_block("base_blocks:stone");
    view.set_override(GridId::MAIN, cube, Some(node_of(stone)));
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::sky(),
        &trip.far,
        "the sky, told",
    );
    assert!(
        screen.hidden.contains_key(&cube),
        "still held, and no work was done for it"
    );

    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::ground(),
        &trip.far,
        "back",
    );
    assert_eq!(
        screen.nodes[&cube],
        mesh_coarse_node(&node_of(stone), &[None; 6]).pack(),
        "the picture of what the server said, not the one that was held"
    );
    assert!(!screen.hidden.contains_key(&cube));
    // The others are what they were (the cubes round an edit are drawn that were left out, so more may be drawn).
    for (other, mesh) in &ground_nodes {
        if *other != cube {
            assert_eq!(screen.nodes.get(other), Some(mesh), "{other:?}");
        }
    }
}

#[test]
fn a_reset_takes_down_the_pictures_that_are_held_too() {
    let trip = Trip::new();
    let terrain = trip.scene.world.main();
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::ground(),
        &trip.far,
        "the ground",
    );
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::sky(),
        &trip.far,
        "the sky",
    );
    assert!(!screen.hidden.is_empty());
    screen.apply(view.reset());
    assert!(screen.nodes.is_empty() && screen.hidden.is_empty());
    assert_eq!(view.held_pictures(), world_view::HeldPictures::default());
    view.check_consistency().expect("the records agree");
}

#[test]
fn without_a_budget_nothing_is_hidden_and_the_updates_are_what_they_were() {
    let trip = Trip::new();
    let terrain = trip.scene.world.main();
    let mut view = view_of(&trip.scene, 3);
    let mut screen = Screen::default();
    for (viewer, context) in [
        (Trip::ground(), "ground"),
        (Trip::sky(), "sky"),
        (Trip::ground(), "back"),
    ] {
        let started = std::time::Instant::now();
        loop {
            let updates = view.update(GridId::MAIN, terrain, viewer, &trip.far);
            assert!(
                !updates.iter().any(|update| matches!(
                    update,
                    MeshUpdate::HideCoarse(_) | MeshUpdate::ShowCoarse(_)
                )),
                "{context}: a picture was hidden or shown"
            );
            screen.apply(updates);
            if view.is_settled() {
                break;
            }
            assert!(
                started.elapsed() < std::time::Duration::from_secs(60),
                "does not settle"
            );
            std::thread::sleep(std::time::Duration::from_micros(300));
        }
        assert!(screen.hidden.is_empty());
        assert_eq!(view.held_pictures(), world_view::HeldPictures::default());
    }
}

/// Finds a node of the ground that is drawn.
/// What the renderer holds hidden of the bricks is what the view holds of them, within the budget, and none of it is drawn or shown.
fn assert_held_bricks_match(screen: &Screen, view: &LodView, budget: usize, context: &str) {
    let held = view.held_brick_pictures();
    assert_eq!(
        screen.hidden_bricks.len(),
        held.with_mesh,
        "{context}: the renderer holds {} hidden meshes of bricks and the view says it holds {}",
        screen.hidden_bricks.len(),
        held.with_mesh
    );
    for coordinate in screen.hidden_bricks.keys() {
        assert!(
            !screen.bricks.contains_key(coordinate)
                && !view
                    .shown()
                    .contains(&BrickCube::new(*coordinate, 0).unwrap()),
            "{context}: {coordinate:?} is hidden and drawn or shown"
        );
    }
    assert!(
        held.bytes <= budget,
        "{context}: {} bytes of bricks are held of {budget}",
        held.bytes
    );
    if budget == 0 {
        assert!(
            screen.hidden_bricks.is_empty(),
            "{context}: nothing is to be held"
        );
    }
}

/// The bricks drawn and the occupancy told, by a view that has never been anywhere but at `at` (nothing held), to compare a view that has with.
fn what_a_fresh_view_draws(scene: &Scene, at: BrickCoordinate, detail: &LodSettings) -> Screen {
    let mut view = view_of(scene, 3);
    view.mesher_mut().enable_occupancy();
    let mut screen = Screen::default();
    settle_checked(
        &mut view,
        &mut screen,
        scene,
        viewer_in(at),
        detail,
        "a fresh view",
    );
    screen
}

/// The bricks that are drawn are those a fresh view draws, picture for picture, and what the sun was told of each of the bricks a fresh view tells of is the same.
fn assert_bricks_are_what_a_fresh_view_draws(screen: &Screen, fresh: &Screen, context: &str) {
    if screen.bricks != fresh.bricks {
        let mut differ: Vec<BrickCoordinate> = screen
            .bricks
            .keys()
            .chain(fresh.bricks.keys())
            .filter(|coordinate| screen.bricks.get(coordinate) != fresh.bricks.get(coordinate))
            .copied()
            .collect();
        differ.sort();
        differ.dedup();
        differ.truncate(6);
        panic!(
            "{context}: {} bricks are drawn and a fresh view draws {}; first that differ: {differ:?}",
            screen.bricks.len(),
            fresh.bricks.len()
        );
    }
    let mut wrong: Vec<BrickCoordinate> = fresh
        .occupancy
        .iter()
        .filter(|(coordinate, occupancy)| screen.occupancy.get(coordinate) != Some(occupancy))
        .map(|(coordinate, _)| *coordinate)
        .collect();
    wrong.sort();
    wrong.truncate(6);
    assert!(
        wrong.is_empty(),
        "{context}: what the sun is told of these bricks is not what a fresh view tells: {wrong:?}"
    );
}

/// A view that goes from the ground to the sky and back, in the loop of an update at a time, and what that did.
struct Return {
    /// The pictures of bricks put up (not shown from hold).
    brick_uploads: usize,
    /// The bricks shown from hold.
    brick_shows: usize,
    /// The updates until the view had nothing more to do.
    updates: usize,
    /// The bricks that were shown after the first update.
    shown_after_the_first_update: HashSet<BrickCube>,
}

fn come_back(
    view: &mut LodView,
    screen: &mut Screen,
    terrain: &impl CoarseTerrainSource,
    settings: &LodSettings,
) -> Return {
    let mut result = Return {
        brick_uploads: 0,
        brick_shows: 0,
        updates: 0,
        shown_after_the_first_update: HashSet::new(),
    };
    let started = std::time::Instant::now();
    loop {
        let updates = view.update(GridId::MAIN, terrain, Trip::ground(), settings);
        result.brick_uploads += updates
            .iter()
            .filter(|update| matches!(update, MeshUpdate::Upload { mesh, .. } if !mesh.is_empty()))
            .count();
        result.brick_shows += updates
            .iter()
            .filter(|update| matches!(update, MeshUpdate::ShowBrick(_)))
            .count();
        screen.apply(updates);
        screen.apply_occupancy(view.mesher_mut().take_occupancy_updates());
        view.check_consistency().expect("the records agree");
        result.updates += 1;
        if result.updates == 1 {
            result.shown_after_the_first_update = view.shown().clone();
        }
        if view.is_settled() {
            return result;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "does not settle"
        );
        std::thread::sleep(std::time::Duration::from_micros(300));
    }
}

#[test]
fn a_view_that_goes_up_and_comes_back_makes_no_brick_again_and_draws_the_bricks_it_drew() {
    let trip = Trip::new();
    let terrain = trip.scene.world.main();
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::ground(),
        &trip.far,
        "the ground",
    );
    let (ground_bricks, ground_shown) = (screen.bricks.clone(), view.shown().clone());
    assert!(
        ground_bricks.len() > 20,
        "{} bricks are drawn on the ground",
        ground_bricks.len()
    );
    assert!(
        screen.hidden_bricks.is_empty(),
        "nothing has been taken down yet"
    );

    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::sky(),
        &trip.far,
        "the sky",
    );
    assert!(
        screen.bricks.is_empty(),
        "no brick of the ground is drawn from the sky"
    );
    assert_eq!(
        screen.hidden_bricks, ground_bricks,
        "every brick of the ground is held, hidden, with the picture it had"
    );
    assert_held_bricks_match(&screen, &view, 256 << 20, "the sky");
    assert!(view.held_brick_pictures().bricks >= ground_bricks.len());
    let pictures_made_before = view.mesher().ready_serial();

    let back = come_back(&mut view, &mut screen, terrain, &trip.far);
    println!(
        "back from the sky: {} updates, {} bricks shown from hold, {} uploaded; shown after the first update: {} of {} cubes of the ground",
        back.updates,
        back.brick_shows,
        back.brick_uploads,
        back.shown_after_the_first_update.len(),
        ground_shown.len()
    );
    assert_eq!(view.shown(), &ground_shown, "the same cubes are drawn");
    assert_eq!(screen.bricks, ground_bricks, "with the same pictures");
    assert!(screen.hidden_bricks.is_empty());
    assert_eq!(back.brick_uploads, 0, "no picture of a brick was uploaded");
    assert_eq!(
        back.brick_shows,
        ground_bricks.len(),
        "each brick was shown from hold"
    );
    assert_eq!(
        view.mesher().ready_serial(),
        pictures_made_before,
        "no brick was meshed again"
    );
    assert_held_bricks_match(&screen, &view, 256 << 20, "back");
    assert_converged(&view, "back on the ground");
}

#[test]
fn what_the_sun_is_told_of_the_bricks_is_told_again_when_they_are_shown_from_hold() {
    let trip = Trip::new();
    let terrain = trip.scene.world.main();
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    view.mesher_mut().enable_occupancy();
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::ground(),
        &trip.far,
        "the ground",
    );
    let ground_occupancy = screen.occupancy.clone();
    assert!(
        ground_occupancy.len() > 100,
        "{} bricks",
        ground_occupancy.len()
    );
    assert!(
        ground_occupancy
            .values()
            .any(|occupancy| matches!(occupancy, BrickOccupancy::Full))
            && ground_occupancy
                .values()
                .any(|occupancy| matches!(occupancy, BrickOccupancy::Mixed(_))),
        "the ground has bricks that are full and bricks that are part stone"
    );

    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::sky(),
        &trip.far,
        "the sky",
    );
    // (The bricks of the ground are out of range, and the renderer is told it does not know them; the picture is held and what the sun is told of it with it.)
    assert!(
        ground_occupancy
            .keys()
            .all(|coordinate| !screen.occupancy.contains_key(coordinate)),
        "the renderer is told nothing of the bricks of the ground from the sky"
    );

    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::ground(),
        &trip.far,
        "back",
    );
    for (coordinate, occupancy) in &ground_occupancy {
        assert_eq!(
            screen.occupancy.get(coordinate),
            Some(occupancy),
            "{coordinate:?}"
        );
    }
}

#[test]
fn an_edit_to_a_brick_while_its_picture_is_held_is_never_put_up_and_the_bricks_are_what_a_fresh_view_draws()
 {
    let mut trip = Trip::new();
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    view.mesher_mut().enable_occupancy();
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        Trip::ground(),
        &trip.far,
        "the ground",
    );
    let ground_bricks = screen.bricks.clone();
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        Trip::sky(),
        &trip.far,
        "the sky",
    );
    let held_before = screen.hidden_bricks.len();

    // A hole in the grass at the edge of the first brick, said while it is up: the pictures of the brick and of the bricks beside it are out of date (the brick beside
    // it has a face against the hole now).
    let edited = trip.scene.edit([0, 9, 5], "base_blocks:air");
    assert_eq!(edited, brick(0, 0, 0));
    view.mesher_mut().brick_changed(edited);
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        Trip::sky(),
        &trip.far,
        "the sky, told",
    );
    let affected: Vec<BrickCoordinate> = std::iter::once(edited)
        .chain(
            FaceDirection::ALL
                .iter()
                .filter_map(|&face| edited.neighbour(face)),
        )
        .collect();
    for coordinate in &affected {
        assert!(
            !screen.hidden_bricks.contains_key(coordinate),
            "{coordinate:?} is held and its picture is out of date"
        );
    }
    assert!(
        screen.hidden_bricks.len() < held_before && screen.hidden_bricks.len() > held_before / 2,
        "{} of {held_before} are still held: the rest were not touched",
        screen.hidden_bricks.len()
    );
    assert_held_bricks_match(&screen, &view, 256 << 20, "the sky, told");

    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        Trip::ground(),
        &trip.far,
        "back",
    );
    let fresh = what_a_fresh_view_draws(&trip.scene, brick(0, 0, 0), &trip.far);
    assert_bricks_are_what_a_fresh_view_draws(&screen, &fresh, "back, after the edit");
    assert_ne!(
        screen.bricks[&edited], ground_bricks[&edited],
        "the hole is in the picture"
    );
    assert_ne!(
        screen.bricks[&brick(-1, 0, 0)],
        ground_bricks[&brick(-1, 0, 0)],
        "and the face of the brick beside it"
    );
}

#[test]
fn a_change_of_what_is_drawn_from_lets_go_of_every_held_picture_and_the_bricks_are_made_from_the_new()
 {
    let trip = Trip::new();
    let terrain = trip.scene.world.main();
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::ground(),
        &trip.far,
        "the ground",
    );
    let ground_bricks = screen.bricks.clone();
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::sky(),
        &trip.far,
        "the sky",
    );
    assert!(!screen.hidden_bricks.is_empty());

    // A game changes its light: what the bricks are drawn from is not what the held pictures were made from.
    let hook = Arc::new(common::LitBrick {
        lit: brick(0, 0, 0),
        brightness: 200,
    });
    view.mesher_mut()
        .set_channels(trip.scene.channels_lit_by(&hook));
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::sky(),
        &trip.far,
        "the sky, changed",
    );
    assert!(
        screen.hidden_bricks.is_empty(),
        "{} pictures are held that were made from what was drawn from before",
        screen.hidden_bricks.len()
    );
    assert_eq!(
        view.held_brick_pictures(),
        world_view::HeldBricks::default()
    );

    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::ground(),
        &trip.far,
        "back",
    );
    let mut fresh_view = LodView::new(
        trip.scene
            .mesher_seeing(3, trip.scene.channels_lit_by(&hook)),
        config(),
    );
    let mut fresh = Screen::default();
    settle_checked_in(
        &mut fresh_view,
        &mut fresh,
        terrain,
        Trip::ground(),
        &trip.far,
        "a fresh view",
    );
    assert_eq!(
        screen.bricks, fresh.bricks,
        "the bricks are drawn from the new light"
    );
    assert_ne!(screen.bricks, ground_bricks, "and it is not the old one");
}

#[test]
fn a_reset_takes_down_the_hidden_meshes_of_pictures_that_went_out_of_date_before_the_view_was_updated_too()
 {
    let mut trip = Trip::new();
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        Trip::ground(),
        &trip.far,
        "the ground",
    );
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        Trip::sky(),
        &trip.far,
        "the sky",
    );
    let edited = trip.scene.edit([0, 9, 5], "base_blocks:air");
    view.mesher_mut().brick_changed(edited);
    // (The renderer still holds the hidden meshes of the pictures that are out of date, and nothing has been said of them yet.)
    screen.apply(view.reset());
    assert!(
        screen.bricks.is_empty() && screen.hidden_bricks.is_empty(),
        "{} drawn and {} hidden",
        screen.bricks.len(),
        screen.hidden_bricks.len()
    );
}

#[test]
fn the_budget_of_held_bricks_lets_go_of_the_oldest_and_what_is_let_go_of_is_meshed_again() {
    let trip = Trip::new();
    let terrain = trip.scene.world.main();
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::ground(),
        &trip.far,
        "the ground",
    );
    let ground_bricks = screen.bricks.clone();
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::sky(),
        &trip.far,
        "the sky",
    );
    let all = view.held_brick_pictures();
    assert!(all.with_mesh > 20, "{all:?}");

    // A budget of a third of that: the rest is let go of, and the bricks are meshed again where they were.
    let budget = all.bytes / 3;
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(budget);
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::ground(),
        &trip.far,
        "the ground",
    );
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::sky(),
        &trip.far,
        "the sky",
    );
    assert_held_bricks_match(&screen, &view, budget, "the sky");
    let held = view.held_brick_pictures();
    assert!(
        held.bytes > budget / 2 && held.bricks < all.bricks,
        "{held:?} of {all:?} held for {budget}"
    );
    let pictures_made_before = view.mesher().ready_serial();
    let back = come_back(&mut view, &mut screen, terrain, &trip.far);
    let meshed_again = view.mesher().ready_serial() - pictures_made_before;
    assert_eq!(screen.bricks, ground_bricks, "the same pictures");
    // Exactly the bricks that were let go of are meshed again: every one that was held is shown as it is (the ones that have something to see are shown from hold, which
    // of them are kept being a matter of the order they were taken down in).
    assert_eq!(
        meshed_again,
        (all.bricks - held.bricks) as u64,
        "{} bricks were shown from hold, of {} held with room for all and {} held for the budget",
        back.brick_shows,
        all.bricks,
        held.bricks
    );
    assert!(meshed_again > 0 && held.bricks > 0);
}

#[test]
fn the_cap_of_pictures_shown_in_an_update_does_not_hold_back_bricks_that_are_held() {
    let trip = Trip::new();
    let terrain = trip.scene.world.main();
    // A ball of bricks that is more than the cap of an update (64) of them with something to draw.
    let mut view = view_of(&trip.scene, 6);
    view.hold_hidden_pictures(256 << 20);
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::ground(),
        &trip.far,
        "the ground",
    );
    let ground_bricks = screen.bricks.clone();
    assert!(
        ground_bricks.len() > 100,
        "{} bricks are drawn",
        ground_bricks.len()
    );
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::sky(),
        &trip.far,
        "the sky",
    );
    // In one update after the viewer is back, every brick is shown: they are moves, and the cap is of pictures put on the graphics card.
    screen.apply(view.update(GridId::MAIN, terrain, Trip::ground(), &trip.far));
    assert_eq!(
        screen.bricks,
        ground_bricks,
        "{} of {} bricks are drawn after the first update",
        screen.bricks.len(),
        ground_bricks.len()
    );
}

#[test]
fn a_reset_takes_down_the_held_bricks_too() {
    let trip = Trip::new();
    let terrain = trip.scene.world.main();
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    view.mesher_mut().enable_occupancy();
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::ground(),
        &trip.far,
        "the ground",
    );
    settle_checked_in(
        &mut view,
        &mut screen,
        terrain,
        Trip::sky(),
        &trip.far,
        "the sky",
    );
    assert!(!screen.hidden_bricks.is_empty());
    screen.apply(view.reset());
    screen.apply_occupancy(view.mesher_mut().take_occupancy_updates());
    assert!(screen.bricks.is_empty() && screen.hidden_bricks.is_empty());
    assert!(screen.occupancy.is_empty(), "the sun is told of no brick");
    assert_eq!(
        view.held_brick_pictures(),
        world_view::HeldBricks::default()
    );
    view.check_consistency().expect("the records agree");
}

#[test]
fn without_a_budget_no_brick_is_hidden_or_shown_and_the_updates_are_what_they_were() {
    let trip = Trip::new();
    let terrain = trip.scene.world.main();
    let mut view = view_of(&trip.scene, 3);
    let mut screen = Screen::default();
    for (viewer, context) in [
        (Trip::ground(), "ground"),
        (Trip::sky(), "sky"),
        (Trip::ground(), "back"),
    ] {
        let started = std::time::Instant::now();
        loop {
            let updates = view.update(GridId::MAIN, terrain, viewer, &trip.far);
            assert!(
                !updates.iter().any(|update| matches!(
                    update,
                    MeshUpdate::HideBrick(_) | MeshUpdate::ShowBrick(_)
                )),
                "{context}: a brick was hidden or shown"
            );
            screen.apply(updates);
            if view.is_settled() {
                break;
            }
            assert!(
                started.elapsed() < std::time::Duration::from_secs(60),
                "does not settle"
            );
            std::thread::sleep(std::time::Duration::from_micros(300));
        }
        assert!(screen.hidden_bricks.is_empty());
        assert_eq!(
            view.held_brick_pictures(),
            world_view::HeldBricks::default()
        );
    }
}

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
    screen.apply(view.settle(GridId::MAIN, scene.world.main(), viewer, &settings()));
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
    screen.apply(view.settle(GridId::MAIN, scene.world.main(), viewer, &settings()));
    assert!(!screen.nodes.contains_key(&cube));
    // (The cubes beside an edit are not left out as buried, so more may be drawn than were; none that were is gone.)
    assert!(shown_before.is_subset(view.shown()));
    assert!(view.shown().contains(&cube));

    // A node of one block: the picture of a cube of that block.
    let stone = scene.game.raw_block("base_blocks:stone");
    view.set_override(GridId::MAIN, cube, Some(node_of(stone)));
    screen.apply(view.settle(GridId::MAIN, scene.world.main(), viewer, &settings()));
    assert_eq!(
        screen.nodes[&cube],
        mesh_coarse_node(&node_of(stone), &[None; 6]).pack()
    );
    // (The cubes beside an edit are not left out as buried, so more may be drawn than were; none that were is gone.)
    assert!(shown_before.is_subset(view.shown()));
    assert!(view.shown().contains(&cube));

    // Taken back: what the generators give.
    view.set_override(GridId::MAIN, cube, None);
    screen.apply(view.settle(GridId::MAIN, scene.world.main(), viewer, &settings()));
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
    screen.apply(view.settle(GridId::MAIN, scene.world.main(), viewer, &settings()));
    let cube = a_drawn_node(&view, &screen);
    let stone = scene.game.raw_block("base_blocks:stone");
    let dirt = scene.game.raw_block("base_blocks:dirt");
    let of_stone = mesh_coarse_node(&node_of(stone), &[None; 6]).pack();
    let of_dirt = mesh_coarse_node(&node_of(dirt), &[None; 6]).pack();
    for _ in 0..3 {
        // The picture of the first is being made when the second is said: whenever it is done, it is for something that is no longer so.
        view.set_override(GridId::MAIN, cube, Some(node_of(stone)));
        let started = view.update(GridId::MAIN, scene.world.main(), viewer, &settings());
        view.set_override(GridId::MAIN, cube, Some(node_of(dirt)));
        let later = view.settle(GridId::MAIN, scene.world.main(), viewer, &settings());
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
        screen.apply(view.settle(GridId::MAIN, scene.world.main(), viewer, &settings()));
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
    screen.apply(view.settle(GridId::MAIN, scene.world.main(), viewer, &settings()));
    let cube = a_drawn_node(&view, &screen);
    let before: HashMap<BrickCube, PackedBrickMesh> = screen.nodes.clone();
    let stone = scene.game.raw_block("base_blocks:stone");
    view.set_override(GridId::root(LevelIndex::new(1)), cube, Some(node_of(stone)));
    let far = BrickCube::containing(brick(1_000_000, 0, 0), 3);
    view.set_override(GridId::MAIN, far, Some(node_of(stone)));
    let updates = view.settle(GridId::MAIN, scene.world.main(), viewer, &settings());
    assert!(updates.is_empty(), "{updates:?}");
    assert_eq!(screen.nodes, before);

    // Nor does it when the view is worked out again from a brick over: the cube is drawn as it was, and no picture of it is made again.
    let moved = view.settle(
        GridId::MAIN,
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
fn a_view_of_the_terrain_of_another_grid_takes_what_is_said_of_that_grid_and_not_of_the_main_one() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 10);
    let mut view = view_of(&scene, 3);
    let mut screen = Screen::default();
    let viewer = viewer_in(brick(0, 0, 0));
    let second = GridId::root(LevelIndex::new(1));
    screen.apply(view.settle(second, scene.world.main(), viewer, &settings()));
    let cube = a_drawn_node(&view, &screen);
    let before = screen.nodes[&cube].clone();
    let stone = scene.game.raw_block("base_blocks:stone");

    // What is said of the main level is not for this one.
    view.set_override(GridId::MAIN, cube, Some(node_of(stone)));
    screen.apply(view.settle(second, scene.world.main(), viewer, &settings()));
    assert_eq!(screen.nodes[&cube], before);
    // What is said of its own grid is.
    view.set_override(second, cube, Some(node_of(stone)));
    screen.apply(view.settle(second, scene.world.main(), viewer, &settings()));
    assert_ne!(screen.nodes[&cube], before);
}

/// The terrain of a level that can be made to say that nothing may show, and that says what version of itself that is.
struct Dimmable<'a> {
    level: &'a LevelData,
    dark: Cell<bool>,
    version: Cell<u64>,
}

impl TerrainSource for Dimmable<'_> {
    fn known(&self, coordinate: BrickCoordinate) -> Option<Arc<Brick>> {
        self.level.known(coordinate)
    }

    fn generator_for(&self, coordinate: BrickCoordinate) -> Option<Arc<dyn TerrainGenerator>> {
        self.level.generator_for(coordinate)
    }

    fn layout(&self) -> &Arc<GridLayout> {
        self.level.layout()
    }
}

impl CoarseTerrainSource for Dimmable<'_> {
    fn version(&self) -> u64 {
        self.version.get()
    }

    fn may_show(&self, cube: BrickCube) -> bool {
        !self.dark.get() && self.level.may_show(cube)
    }

    fn coarse_generator(&self, cube: BrickCube) -> Option<Arc<dyn TerrainGenerator>> {
        self.level.coarse_generator(cube)
    }
}

#[test]
fn what_a_terrain_said_may_show_is_asked_again_when_its_version_changes_and_not_before() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 10);
    let terrain = Dimmable {
        level: scene.world.main(),
        dark: Cell::new(false),
        version: Cell::new(0),
    };
    let mut view = view_of(&scene, 3);
    let mut screen = Screen::default();
    screen.apply(view.settle(
        GridId::MAIN,
        &terrain,
        viewer_in(brick(0, 0, 0)),
        &settings(),
    ));
    assert!(!screen.nodes.is_empty());

    // The terrain now says nothing may show, but not that it is another terrain: the answers the view has are kept, and the view is worked out from them for the
    // next place the viewer is at.
    terrain.dark.set(true);
    screen.apply(view.settle(
        GridId::MAIN,
        &terrain,
        viewer_in(brick(0, 0, 1)),
        &settings(),
    ));
    assert!(!screen.nodes.is_empty());

    // Once it says it is another, they are asked again.
    terrain.version.set(1);
    screen.apply(view.settle(
        GridId::MAIN,
        &terrain,
        viewer_in(brick(0, 0, 2)),
        &settings(),
    ));
    assert!(screen.nodes.is_empty());
}

/// The terrain of a level, that counts how many times the view asks whether a cube may show anything.
struct Counting<'a> {
    level: &'a LevelData,
    asked: &'a Cell<u64>,
}

impl TerrainSource for Counting<'_> {
    fn known(&self, coordinate: BrickCoordinate) -> Option<Arc<Brick>> {
        self.level.known(coordinate)
    }

    fn generator_for(&self, coordinate: BrickCoordinate) -> Option<Arc<dyn TerrainGenerator>> {
        self.level.generator_for(coordinate)
    }

    fn layout(&self) -> &Arc<GridLayout> {
        self.level.layout()
    }
}

impl CoarseTerrainSource for Counting<'_> {
    fn version(&self) -> u64 {
        self.level.version()
    }

    fn may_show(&self, cube: BrickCube) -> bool {
        self.asked.set(self.asked.get() + 1);
        self.level.may_show(cube)
    }

    fn coarse_generator(&self, cube: BrickCube) -> Option<Arc<dyn TerrainGenerator>> {
        self.level.coarse_generator(cube)
    }
}

#[test]
fn a_view_at_rest_does_not_ask_the_terrain_again_when_a_watch_only_tells_it_the_default_generator_of_cubes()
 {
    // The view is worked out from the answers of `may_show`, kept for one version of the terrain. A client is told, at every watch, the generator of each cube that
    // covers the region (the default, nearly always), including the cubes it knew: if that is a new version each time, a view that moves a brick at a time has
    // nothing kept at any brick, and at 444 Mm along its trail of cubes each answer for a big cube is a walk through all of them.
    let mut scene = scene();
    scene.tell_assignments_around(brick(0, 0, 0), 10);
    let asked = Cell::new(0);
    let at = viewer_in(brick(0, 0, 0));
    let mut view = view_of(&scene, 3);
    let mut screen = Screen::default();
    let source = Counting {
        level: scene.world.main(),
        asked: &asked,
    };
    screen.apply(view.settle(GridId::MAIN, &source, at, &settings()));
    assert!(asked.get() > 0 && !screen.nodes.is_empty());
    let shown = view.shown().clone();

    // Another watch: cubes of the same region again, and cubes beyond it.
    scene.tell_assignments_around(brick(0, 0, 4), 10);
    scene.tell_assignments_around(brick(0, 8, 8), 10);
    asked.set(0);
    let source = Counting {
        level: scene.world.main(),
        asked: &asked,
    };
    screen.apply(view.settle(GridId::MAIN, &source, at, &settings()));
    assert_eq!(asked.get(), 0, "the view asked the terrain again");
    assert_eq!(view.shown(), &shown);
}

#[test]
fn a_view_that_goes_up_and_comes_back_does_not_ask_the_terrain_again_about_the_cubes_it_worked_out_on_the_ground()
 {
    // What the terrain says of a cube is kept for more than the last two workings out of the cubes: a view that leaves in several moves, and comes back, asks about
    // the cubes of the ground again only if it asked about none of them on the way (measured, from 264 km up: 32,338 of the 33,030 cubes asked again).
    let trip = Trip::new();
    let asked = Cell::new(0);
    let terrain = Counting {
        level: trip.scene.world.main(),
        asked: &asked,
    };
    let mut view = view_of(&trip.scene, 3);
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        Trip::ground(),
        &trip.far,
        "the ground",
    );
    let asked_for_the_ground = asked.get();
    let ground_shown = view.shown().clone();
    assert!(asked_for_the_ground > 100 && ground_shown.len() > 100);

    // Up in six moves, each its own working out of the cubes (a view that is flown up asks again at each brick it passes), as far as a map.
    for (step, height) in [30, 60, 120, 200, 300, 400].into_iter().enumerate() {
        settle_checked_in(
            &mut view,
            &mut screen,
            &terrain,
            viewer_in(brick(0, height, 0)),
            &trip.far,
            &format!("up, step {step}"),
        );
    }
    assert_ne!(view.shown(), &ground_shown);

    asked.set(0);
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        Trip::ground(),
        &trip.far,
        "the ground again",
    );
    assert_eq!(view.shown(), &ground_shown, "the ground is what it was");
    assert_eq!(
        asked.get(),
        0,
        "the terrain was asked again about {} cubes of the {asked_for_the_ground} it was asked about for the ground",
        asked.get()
    );
}

#[test]
fn a_view_at_rest_works_out_its_cubes_again_when_a_watch_tells_it_another_generator_than_the_one_that_made_them()
 {
    let mut scene = scene_of(&[Plugin::BaseBlocks, Plugin::FancyTerrain]);
    scene.tell_assignments_around(brick(0, 0, 0), 10);
    let asked = Cell::new(0);
    let at = viewer_in(brick(0, 0, 0));
    let mut view = view_of(&scene, 3);
    let mut screen = Screen::default();
    let source = Counting {
        level: scene.world.main(),
        asked: &asked,
    };
    screen.apply(view.settle(GridId::MAIN, &source, at, &settings()));
    assert!(asked.get() > 0);

    // The ground there is higher (40, not 10) by the generator the server names for a cube: what was kept of the cubes round it is not so any more.
    let higher = scene
        .world
        .main()
        .terrain
        .generators()
        .runtime_id(&world_storage::test_support::id("fancy_terrain:cliffs_v1"))
        .unwrap();
    scene
        .world
        .main_mut()
        .terrain
        .set_assignment(BrickCube::containing(brick(0, 0, 0), 3), Some(higher));
    asked.set(0);
    let source = Counting {
        level: scene.world.main(),
        asked: &asked,
    };
    screen.apply(view.settle(GridId::MAIN, &source, at, &settings()));
    assert!(asked.get() > 0, "the view did not ask again");
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
        GridId::MAIN,
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
    screen.apply(view.settle(GridId::MAIN, scene.world.main(), viewer, &settings()));
    assert!(!screen.nodes.is_empty());
    screen.apply(view.reset());
    assert!(screen.nodes.is_empty());
    assert!(screen.bricks.is_empty());
    assert!(view.shown().is_empty());
    screen.apply(view.settle(GridId::MAIN, scene.world.main(), viewer, &settings()));
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
    screen.apply(view.settle(GridId::MAIN, scene.world.main(), viewer, &settings()));
    let before = screen.bricks[&brick(0, 0, 0)].clone();
    let edited = scene.edit([5, 9, 5], "base_blocks:air");
    view.mesher_mut().brick_changed(edited);
    screen.apply(view.settle(GridId::MAIN, scene.world.main(), viewer, &settings()));
    assert_ne!(screen.bricks[&brick(0, 0, 0)], before);
    assert_nothing_overlaps(view.shown());
}

/// The cubes the rule gives for a viewer in `centre`, with or without leaving out those that hold nothing to see, in an order that does not change.
fn selected(scene: &Scene, centre: BrickCoordinate, leave_out_empty: bool) -> Vec<BrickCube> {
    let view = ViewSelection {
        detail: settings().detail,
        zoom: None,
        fine_radius: 3,
        maximum_distance: settings().maximum_distance,
    };
    let terrain = &scene.world.main().terrain;
    let mut cubes = select_around_brick(centre, &view, &|cube| {
        !leave_out_empty || terrain.may_show(cube)
    });
    cubes.sort_by_key(|cube| {
        let origin = cube.origin();
        (cube.level(), origin.x(), origin.y(), origin.z())
    });
    cubes
}

/// The 26 cubes of the size of `cube` that touch it.
fn touching(cube: BrickCube) -> Vec<BrickCube> {
    let edge = cube.edge_length() as i128;
    let origin = cube.origin();
    let mut touching = Vec::new();
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                if (dx, dy, dz) != (0, 0, 0) {
                    let at = brick(
                        origin.x() + dx * edge,
                        origin.y() + dy * edge,
                        origin.z() + dz * edge,
                    );
                    touching.push(BrickCube::new(at, cube.level()).unwrap());
                }
            }
        }
    }
    touching
}

/// Whether a cube is the cube that holds an edit or touches it, as far as cubes of the size of the cube go.
fn is_near(cube: BrickCube, edit: BrickCube) -> bool {
    let holder = BrickCube::containing(edit.origin(), cube.level());
    holder == cube || touching(cube).contains(&holder)
}

#[test]
fn cubes_that_are_all_air_or_buried_in_ground_are_not_drawn_and_what_is_left_is_a_fraction() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 10);
    let pruned = selected(&scene, brick(0, 0, 0), true);
    let all = selected(&scene, brick(0, 0, 0), false);
    assert!(
        pruned.len() * 5 < all.len(),
        "{} of {} cubes",
        pruned.len(),
        all.len()
    );

    let mut view = view_of(&scene, 3);
    let mut screen = Screen::default();
    screen.apply(view.settle(
        GridId::MAIN,
        scene.world.main(),
        viewer_in(brick(0, 0, 0)),
        &settings(),
    ));
    assert_eq!(view.wanted().len(), pruned.len());
    assert_eq!(view.shown(), view.wanted());
    for cube in view.shown().iter().filter(|cube| cube.level() > 0) {
        assert!(
            scene.world.main().terrain.may_show(*cube),
            "{cube:?} was drawn though it holds nothing to see"
        );
    }
    // What is drawn is where the ground is and next to it: no node is wholly under the ground, and none wholly over it.
    for cube in screen.nodes.keys() {
        let low = cube.origin().y() * 32;
        let high = low + cube.edge_length() as i128 * 32;
        assert!(
            low < 10 && high > -(cube.edge_length() as i128 * 32),
            "{cube:?}"
        );
    }
    assert!(!screen.nodes.is_empty());
    // Bricks are never left out: all of them are drawn, as for a view that leaves nothing out.
    for cube in all.iter().filter(|cube| cube.level() == 0) {
        assert!(view.wanted().contains(cube), "{cube:?}");
    }
}

#[test]
fn a_block_placed_high_in_the_air_among_the_bricks_is_drawn_though_the_nodes_round_it_are_air() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 10);
    let mut view = view_of(&scene, 3);
    let mut screen = Screen::default();
    let viewer = viewer_in(brick(0, 0, 0));
    screen.apply(view.settle(GridId::MAIN, scene.world.main(), viewer, &settings()));

    // Three bricks up, the farthest the bricks go: there is only air there, and the cubes that hold it are air all through.
    let edited = scene.edit([5, 3 * 32 + 5, 5], "base_blocks:stone");
    assert_eq!(edited, brick(0, 3, 0));
    view.mesher_mut().brick_changed(edited);
    screen.apply(view.settle(GridId::MAIN, scene.world.main(), viewer, &settings()));
    assert!(
        view.shown()
            .contains(&BrickCube::new(brick(0, 3, 0), 0).unwrap())
    );
    assert!(!screen.bricks[&brick(0, 3, 0)].is_empty());
    assert_nothing_overlaps(view.shown());
}

#[test]
fn a_flood_of_edits_in_new_places_is_one_reselect_after_a_few_updates_and_not_one_each() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 10);
    let mut view = view_of(&scene, 3);
    let mut screen = Screen::default();
    let viewer = viewer_in(brick(0, 0, 0));
    screen.apply(view.settle(GridId::MAIN, scene.world.main(), viewer, &settings()));
    let before = view.wanted().clone();
    let stone = scene.game.raw_block("base_blocks:stone");
    let all = selected(&scene, brick(0, 0, 0), false);
    // Three cubes far enough from one another that none is beside another (a cube beside an edit is kept with it).
    let mut in_the_sky: Vec<BrickCube> = Vec::new();
    for cube in all
        .iter()
        .copied()
        .filter(|cube| cube.level() == 1 && cube.origin().y() >= 8 && !before.contains(cube))
    {
        let apart = |other: &BrickCube| {
            let (a, b) = (cube.origin(), other.origin());
            (a.x() - b.x()).abs() >= 8 || (a.y() - b.y()).abs() >= 8 || (a.z() - b.z()).abs() >= 8
        };
        if in_the_sky.iter().all(apart) {
            in_the_sky.push(cube);
        }
        if in_the_sky.len() == 3 {
            break;
        }
    }
    assert_eq!(in_the_sky.len(), 3);

    // The first, after a quiet time, is taken in at once.
    view.set_override(GridId::MAIN, in_the_sky[0], Some(node_of(stone)));
    screen.apply(view.update(GridId::MAIN, scene.world.main(), viewer, &settings()));
    assert!(view.wanted().contains(&in_the_sky[0]));
    // The next is not: the cubes are worked out again at most every few updates, and the view knows there is more to do.
    view.set_override(GridId::MAIN, in_the_sky[1], Some(node_of(stone)));
    screen.apply(view.update(GridId::MAIN, scene.world.main(), viewer, &settings()));
    assert!(!view.wanted().contains(&in_the_sky[1]), "not at once");
    assert!(!view.is_settled());
    // A third comes meanwhile. Some updates later both are in, from the one working out.
    view.set_override(GridId::MAIN, in_the_sky[2], Some(node_of(stone)));
    let mut updates = 2;
    while !view.wanted().contains(&in_the_sky[1]) {
        screen.apply(view.update(GridId::MAIN, scene.world.main(), viewer, &settings()));
        updates += 1;
        assert!(updates <= 30, "never worked out");
    }
    assert!((5..=15).contains(&updates), "after {updates} updates");
    for cube in &in_the_sky {
        assert!(view.wanted().contains(cube), "{cube:?}");
    }
    screen.apply(view.settle(GridId::MAIN, scene.world.main(), viewer, &settings()));
    for cube in &in_the_sky {
        assert_eq!(
            screen.nodes[cube],
            mesh_coarse_node(&node_of(stone), &[None; 6]).pack()
        );
    }
}

#[test]
fn an_edit_in_the_sky_or_under_the_ground_brings_in_the_cubes_round_it_and_taking_it_back_lets_them_go()
 {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 10);
    let mut view = view_of(&scene, 3);
    let mut screen = Screen::default();
    let viewer = viewer_in(brick(0, 0, 0));
    screen.apply(view.settle(GridId::MAIN, scene.world.main(), viewer, &settings()));
    let before = view.wanted().clone();

    // A cube of the finest detail high over the ground and one deep under it, each with cubes of the same detail all round it: the rule would give them all, and
    // only the terrain leaves them out.
    let all = selected(&scene, brick(0, 0, 0), false);
    let all_set: HashSet<BrickCube> = all.iter().copied().collect();
    let surrounded = |cube: &&BrickCube| {
        cube.level() == 1
            && touching(**cube)
                .iter()
                .all(|beside| all_set.contains(beside))
    };
    let sky = *all
        .iter()
        .filter(surrounded)
        .find(|cube| cube.origin().y() >= 8)
        .expect("a cube in the sky");
    let deep = *all
        .iter()
        .filter(surrounded)
        .find(|cube| cube.origin().y() <= -8)
        .expect("a cube deep in the ground");
    assert!(!before.contains(&sky) && !before.contains(&deep));
    for beside in touching(sky).into_iter().chain(touching(deep)) {
        assert!(!before.contains(&beside), "{beside:?} is not left out");
    }

    // A block of stone floating in the sky is drawn, and so are the cubes round it, which have nothing in them. (It is said twice, the first time of a block
    // of something else: a place is edited when it gets its first override and unedited when it loses its only one, however many times it was said.)
    let stone = scene.game.raw_block("base_blocks:stone");
    view.set_override(
        GridId::MAIN,
        sky,
        Some(node_of(scene.game.raw_block("base_blocks:dirt"))),
    );
    view.set_override(GridId::MAIN, sky, Some(node_of(stone)));
    screen.apply(view.settle(GridId::MAIN, scene.world.main(), viewer, &settings()));
    assert_eq!(
        screen.nodes[&sky],
        mesh_coarse_node(&node_of(stone), &[None; 6]).pack()
    );
    for beside in touching(sky) {
        assert!(view.wanted().contains(&beside), "{beside:?}");
        assert!(!screen.nodes.contains_key(&beside), "{beside:?} is air");
    }
    assert!(before.is_subset(view.wanted()));
    assert!(
        view.wanted()
            .difference(&before)
            .all(|cube| is_near(*cube, sky))
    );
    assert_eq!(view.shown(), view.wanted());

    // A hole dug deep under the ground: the cubes of stone round it are drawn, with the walls that face the hole, and the hole is not.
    view.set_override(
        GridId::MAIN,
        deep,
        Some(CoarseNode::uniform(CoarseCell::empty(scene.air, 0))),
    );
    screen.apply(view.settle(GridId::MAIN, scene.world.main(), viewer, &settings()));
    assert!(view.wanted().contains(&deep));
    assert!(!screen.nodes.contains_key(&deep));
    for beside in touching(deep) {
        assert!(view.wanted().contains(&beside), "{beside:?}");
        assert!(screen.nodes.contains_key(&beside), "{beside:?} has no wall");
    }
    assert!(
        view.wanted()
            .difference(&before)
            .all(|cube| is_near(*cube, sky) || is_near(*cube, deep))
    );
    assert_eq!(view.shown(), view.wanted());
    assert_nothing_overlaps(view.shown());

    // Both taken back: what was left out is left out again, once the cubes are worked out again.
    view.set_override(GridId::MAIN, sky, None);
    view.set_override(GridId::MAIN, deep, None);
    screen.apply(view.settle(GridId::MAIN, scene.world.main(), viewer, &settings()));
    assert_eq!(view.wanted(), &before);
    assert_eq!(view.shown(), &before);
    for cube in screen.nodes.keys() {
        assert!(before.contains(cube), "{cube:?} is still drawn");
    }
}
