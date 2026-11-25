//! Bricks and coarse nodes drawn together: what is on the screen of a viewer on flat ground, standing and walking.

use std::collections::{HashMap, HashSet};

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

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
    CoarseTerrainSource, LodConfig, LodSettings, LodView, MayShowOracle, MeshUpdate,
    OccupancyChange, TerrainSource, Viewpoint,
};

mod common;

/// How long a test waits for a view to come to rest before it says the view does not: the tests of this file run together, a dozen at a time, each with threads of its own, and a
/// view that is made from nothing in the dev profile takes a minute of that machine's time when the others are busy (a run of the whole file took 205 s, and the 60 s deadline
/// this was before was passed by two tests that pass alone in 40 s). It is for a view that is stuck, not for a slow one.
const DEADLINE: std::time::Duration = std::time::Duration::from_secs(300);

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
                // (As the renderer does it: a mesh that is drawn is replaced where it is, one that is held hidden is, and a new one is held hidden.)
                MeshUpdate::UploadHiddenBrick { coordinate, mesh } if mesh.is_empty() => {
                    self.bricks.remove(&coordinate);
                    self.hidden_bricks.remove(&coordinate);
                }
                MeshUpdate::UploadHiddenBrick { coordinate, mesh } => {
                    if let Some(drawn) = self.bricks.get_mut(&coordinate) {
                        *drawn = mesh;
                    } else {
                        self.hidden_bricks.insert(coordinate, mesh);
                    }
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
                // (As the renderer does it: a mesh that is drawn is replaced where it is, one that is held hidden is, and a new one is held hidden.)
                MeshUpdate::UploadHiddenCoarse { cube, mesh } if mesh.is_empty() => {
                    self.nodes.remove(&cube);
                    self.hidden.remove(&cube);
                }
                MeshUpdate::UploadHiddenCoarse { cube, mesh } => {
                    if let Some(drawn) = self.nodes.get_mut(&cube) {
                        *drawn = mesh;
                    } else {
                        self.hidden.insert(cube, mesh);
                    }
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
            assert!(started.elapsed() < DEADLINE, "step {step} does not settle");
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
        assert!(started.elapsed() < DEADLINE, "{context}: does not settle");
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
        assert!(started.elapsed() < DEADLINE, "does not settle");
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
    let deadline = std::time::Instant::now() + DEADLINE;
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
        assert!(started.elapsed() < DEADLINE, "does not settle");
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
            assert!(started.elapsed() < DEADLINE, "does not settle");
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
        assert!(started.elapsed() < DEADLINE, "does not settle");
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
            assert!(started.elapsed() < DEADLINE, "does not settle");
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

// ---- A viewpoint kept warm (docs/specs/viewpoints-spec.md, phase 3b) ----

/// How far the warm views reach: less than the trips, so that the tests are not so slow that they miss their deadlines when the binary runs as a whole.
fn far() -> LodSettings {
    LodSettings {
        maximum_distance: 1.2e4,
        ..settings()
    }
}

/// A place above the ground that is far enough for a coarser picture, and near enough to be cheap.
fn sky() -> WorldPosition {
    viewer_in(brick(0, 100, 0))
}

fn warm(viewer: WorldPosition, settings: &LodSettings) -> Option<Viewpoint> {
    Some(Viewpoint {
        viewer,
        settings: *settings,
    })
}

/// Every node a fresh view at a place draws is, in a view that keeps the place warm, either drawn or held hidden, as the same picture.
fn assert_nodes_are_drawn_or_held(screen: &Screen, fresh: &Screen, context: &str) {
    let mut missing = Vec::new();
    for (cube, mesh) in &fresh.nodes {
        match (screen.nodes.get(cube), screen.hidden.get(cube)) {
            (Some(held), None) | (None, Some(held)) if held == mesh => {}
            _ => missing.push(*cube),
        }
    }
    missing.sort_by_key(|cube| (cube.level(), cube.origin().x(), cube.origin().y()));
    assert!(
        missing.is_empty(),
        "{context}: {} of the {} nodes a fresh view draws are neither drawn nor held as it draws them; first {:?}",
        missing.len(),
        fresh.nodes.len(),
        &missing[..missing.len().min(5)]
    );
}

#[test]
fn a_viewpoint_that_is_kept_warm_is_made_and_held_hidden_while_another_is_drawn_and_what_is_drawn_is_what_it_was()
 {
    let trip = Trip::new();
    let fresh_sky = what_a_fresh_view_draws(&trip.scene, brick(0, 100, 0), &far());
    let fresh_ground = what_a_fresh_view_draws(&trip.scene, brick(0, 0, 0), &far());
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "the sky, the ground warm",
    );
    assert_eq!(
        screen.nodes, fresh_sky.nodes,
        "what is drawn is what a view that is only at the sky draws"
    );
    assert_nodes_are_drawn_or_held(&screen, &fresh_ground, "the ground, warm");
    assert!(
        screen.hidden.len() * 2 > fresh_ground.nodes.len(),
        "most of the ground is held hidden: {} of {}",
        screen.hidden.len(),
        fresh_ground.nodes.len()
    );
    assert_eq!(screen.hidden.len(), view.held_pictures().with_mesh);
    for cube in screen.hidden.keys() {
        assert!(
            !screen.nodes.contains_key(cube),
            "{cube:?} is held and drawn"
        );
    }
}

#[test]
fn a_view_at_a_place_that_is_warm_does_not_ask_the_terrain_and_makes_next_to_nothing_and_draws_what_a_fresh_view_does()
 {
    let trip = Trip::new();
    let fresh_ground = what_a_fresh_view_draws(&trip.scene, brick(0, 0, 0), &far());
    let asked = Cell::new(0);
    let terrain = Counting {
        level: trip.scene.world.main(),
        asked: &asked,
    };
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        &far(),
        "the sky, the ground warm",
    );

    // Switch: the ground is drawn and the sky is warm.
    asked.set(0);
    let selections = view.selections();
    view.set_warm_viewpoint(warm(sky(), &far()));
    let mut uploads = 0;
    let mut shows = 0;
    loop {
        let updates = view.update(GridId::MAIN, &terrain, Trip::ground(), &far());
        uploads += updates
            .iter()
            .filter(|update| matches!(update, MeshUpdate::UploadCoarse { mesh, .. } if !mesh.is_empty()))
            .count();
        shows += updates
            .iter()
            .filter(|update| matches!(update, MeshUpdate::ShowCoarse(_)))
            .count();
        screen.apply(updates);
        screen.apply_occupancy(view.mesher_mut().take_occupancy_updates());
        view.check_consistency().expect("the records agree");
        if view.is_settled() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(300));
    }
    assert_eq!(
        asked.get(),
        0,
        "the cuts of both places were kept: nothing is asked of the terrain"
    );
    assert_eq!(
        view.selections(),
        selections,
        "neither the place that is drawn now nor the one that was left was worked out again"
    );
    assert!(
        shows * 2 > fresh_ground.nodes.len(),
        "most of the ground is shown from hold: {shows} of {}",
        fresh_ground.nodes.len()
    );
    assert!(
        uploads * 20 < fresh_ground.nodes.len(),
        "{uploads} nodes were put up for the ground, which has {}",
        fresh_ground.nodes.len()
    );
    assert_eq!(
        screen.nodes, fresh_ground.nodes,
        "what is drawn is what a view that is only at the ground draws"
    );
}

#[test]
fn a_warm_viewpoint_that_is_the_one_that_is_drawn_changes_nothing_and_none_is_made_twice() {
    let trip = Trip::new();
    let fresh_ground = what_a_fresh_view_draws(&trip.scene, brick(0, 0, 0), &far());
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        Trip::ground(),
        &far(),
        "the ground, warm too",
    );
    assert_eq!(screen.nodes, fresh_ground.nodes);
    assert!(
        screen.hidden.is_empty(),
        "what is drawn is not held as well: {} held",
        screen.hidden.len()
    );
}

/// The terrain of a level that says, in order, of which cubes a node was asked for.
struct Recording<'a> {
    level: &'a LevelData,
    made: &'a std::cell::RefCell<Vec<BrickCube>>,
    /// The bricks it was asked for a generator of, in order (a brick is asked about when a job to generate it is started).
    generated: &'a std::cell::RefCell<Vec<BrickCoordinate>>,
}

impl TerrainSource for Recording<'_> {
    fn known(&self, coordinate: BrickCoordinate) -> Option<Arc<Brick>> {
        self.level.known(coordinate)
    }

    fn generator_for(&self, coordinate: BrickCoordinate) -> Option<Arc<dyn TerrainGenerator>> {
        self.generated.borrow_mut().push(coordinate);
        self.level.generator_for(coordinate)
    }

    fn layout(&self) -> &Arc<GridLayout> {
        self.level.layout()
    }
}

impl CoarseTerrainSource for Recording<'_> {
    fn version(&self) -> u64 {
        self.level.version()
    }

    fn may_show(&self, cube: BrickCube) -> bool {
        self.level.may_show(cube)
    }

    fn coarse_generator(&self, cube: BrickCube) -> Option<Arc<dyn TerrainGenerator>> {
        self.made.borrow_mut().push(cube);
        self.level.coarse_generator(cube)
    }
}

/// What the drawn viewpoint needs is made before what the warm one waits to have made: the threads take a drawn job before any warm one that waits, whichever was handed to
/// them first. (Before the share of the threads, the view handed out nothing for the warm viewpoint until everything drawn was handed out, and this was a test of the order of
/// the handing out; the warm nodes are handed out under a limit of their own now, and the order that matters is the one the threads take them in.)
#[test]
fn what_the_drawn_viewpoint_needs_is_made_before_the_warm_nodes_that_wait_though_they_were_handed_out_first()
 {
    let trip = Trip::new();
    let along = viewer_in(brick(60, 0, 0));
    let up_there = viewer_in(brick(0, 80, 0));

    // The nodes a view with nothing warm makes for the move along the ground.
    let terrain = Counted::new(trip.scene.world.main(), true, std::time::Duration::ZERO);
    let mut view = view_with(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        Trip::ground(),
        &far(),
        "the ground",
    );
    terrain.started();
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        along,
        &far(),
        "along the ground",
    );
    let needed: HashSet<BrickCube> = terrain.started().into_iter().collect();
    assert!(
        needed.len() > 100,
        "{} nodes to make for the move",
        needed.len()
    );

    // The same with a warm viewpoint whose nodes are handed out, and wait, before the move: the 3 threads are held at a gate by the first nodes they take, and the viewer moves.
    let terrain = Counted::new(trip.scene.world.main(), true, std::time::Duration::ZERO);
    let mut view = view_with(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        Trip::ground(),
        &far(),
        "the ground",
    );
    terrain.gate.close();
    let _opens = OpensWhenDropped(Arc::clone(&terrain.gate));
    view.set_warm_viewpoint(warm(up_there, &far()));
    let started = std::time::Instant::now();
    while view.warm_nodes_being_made() < 4 {
        screen.apply(view.update(GridId::MAIN, &terrain, Trip::ground(), &far()));
        assert!(
            started.elapsed() < DEADLINE,
            "no nodes were handed out for the sky"
        );
        std::thread::sleep(std::time::Duration::from_micros(300));
    }
    terrain.started();
    // The move: what it needs is handed out (4 for each of 3 threads) behind the warm nodes that were handed out first, which wait or are held at the gate.
    while view.nodes_being_made() - view.warm_nodes_being_made() < 12 {
        screen.apply(view.update(GridId::MAIN, &terrain, along, &far()));
        assert!(
            started.elapsed() < DEADLINE,
            "nothing was handed out for the move"
        );
        std::thread::sleep(std::time::Duration::from_micros(300));
    }
    terrain.gate.open();
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        along,
        &far(),
        "along the ground, the sky warm",
    );
    let order = terrain.started();
    let first_for_the_warm_one = order
        .iter()
        .position(|cube| !needed.contains(cube))
        .expect("the warm viewpoint has nodes to make too");
    // (The threads write what they start a moment after they take it, so a few of those that were taken together may be written in another order.)
    assert!(
        first_for_the_warm_one >= 9,
        "only {first_for_the_warm_one} nodes were started for the move before one for the warm viewpoint: {:?}",
        &order[..order.len().min(16)]
    );
}

#[test]
fn what_is_held_for_a_warm_viewpoint_is_not_let_go_of_by_the_budget_until_it_is_not_warm_any_more()
{
    let trip = Trip::new();
    let fresh_ground = what_a_fresh_view_draws(&trip.scene, brick(0, 0, 0), &far());
    let budget = 64 << 10;
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(budget);
    // (What is held for the viewpoint is left to the budget when it is taken away, which is what this is about.)
    view.set_release_warm_pictures(false);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "the sky, the ground warm",
    );
    assert!(
        view.held_pictures().bytes > budget,
        "the ground is more than the budget ({} bytes held)",
        view.held_pictures().bytes
    );
    assert_nodes_are_drawn_or_held(&screen, &fresh_ground, "the ground, warm, over the budget");

    view.set_warm_viewpoint(None);
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "the sky, nothing warm",
    );
    assert_held_pictures_match(&screen, &view, budget, "nothing warm any more");
}

#[test]
fn an_edit_in_the_warm_cut_makes_its_held_picture_again_and_it_is_not_drawn() {
    let trip = Trip::new();
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "the sky, the ground warm",
    );
    let mut held: Vec<BrickCube> = screen.hidden.keys().copied().collect();
    held.sort_by_key(|cube| {
        (
            cube.level(),
            cube.origin().x(),
            cube.origin().y(),
            cube.origin().z(),
        )
    });
    let cube = held[held.len() / 2];
    let stone = trip.scene.game.raw_block("base_blocks:stone");
    let of_stone = mesh_coarse_node(&node_of(stone), &[None; 6]).pack();
    assert_ne!(screen.hidden[&cube], of_stone);
    view.set_override(GridId::MAIN, cube, Some(node_of(stone)));
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "an edit of a held node",
    );
    assert_eq!(screen.hidden[&cube], of_stone, "held, and made again");
    assert!(!screen.nodes.contains_key(&cube), "and not drawn");
}

#[test]
fn a_reset_takes_down_what_was_held_for_the_warm_viewpoint_too() {
    let trip = Trip::new();
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "the sky, the ground warm",
    );
    assert!(!screen.hidden.is_empty());
    screen.apply(view.reset());
    assert!(screen.hidden.is_empty() && screen.nodes.is_empty() && screen.bricks.is_empty());
    assert!(screen.hidden_bricks.is_empty());
    // And it is made again from the start.
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "after the reset",
    );
    assert!(!screen.hidden.is_empty());
}

#[test]
fn a_warm_viewpoint_that_moves_is_worked_out_again_after_a_while_and_what_it_wants_is_held() {
    let trip = Trip::new();
    let along = brick(48, 0, 0);
    let fresh_along = what_a_fresh_view_draws(&trip.scene, along, &far());
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "the sky, the ground warm",
    );
    view.set_warm_viewpoint(warm(viewer_in(along), &far()));
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "the warm viewpoint moved",
    );
    assert_nodes_are_drawn_or_held(&screen, &fresh_along, "the ground along, warm");
}

#[test]
fn without_a_budget_a_warm_viewpoint_is_not_made_and_changes_nothing() {
    let trip = Trip::new();
    let fresh_sky = what_a_fresh_view_draws(&trip.scene, brick(0, 100, 0), &far());
    let mut view = view_of(&trip.scene, 3);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    let mut hidden_uploads = 0;
    let started = std::time::Instant::now();
    loop {
        let updates = view.update(GridId::MAIN, trip.scene.world.main(), sky(), &far());
        hidden_uploads += updates
            .iter()
            .filter(|update| matches!(update, MeshUpdate::UploadHiddenCoarse { .. }))
            .count();
        screen.apply(updates);
        if view.is_settled() {
            break;
        }
        assert!(started.elapsed() < DEADLINE, "does not settle");
        std::thread::sleep(std::time::Duration::from_micros(300));
    }
    assert_eq!(hidden_uploads, 0);
    assert_eq!(screen.nodes, fresh_sky.nodes);
    assert!(screen.hidden.is_empty());
}

// ---- The bricks of a viewpoint kept warm ----

/// A view that draws `sky()` and keeps the ground warm, the sun told of the bricks it draws, settled.
fn view_with_the_ground_warm(trip: &Trip, screen: &mut Screen) -> LodView {
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    view.mesher_mut().enable_occupancy();
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    settle_checked_in(
        &mut view,
        screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "the sky, the ground warm",
    );
    view
}

#[test]
fn the_bricks_of_a_warm_viewpoint_are_made_and_held_hidden_and_the_sun_is_told_nothing_of_them_until_they_are_drawn()
 {
    let trip = Trip::new();
    let fresh_ground = what_a_fresh_view_draws(&trip.scene, brick(0, 0, 0), &far());
    let mut screen = Screen::default();
    let view = view_with_the_ground_warm(&trip, &mut screen);
    assert!(
        fresh_ground.bricks.len() > 20,
        "{} bricks have something to see on the ground",
        fresh_ground.bricks.len()
    );
    assert!(screen.bricks.is_empty(), "no brick is drawn at the sky");
    for (coordinate, mesh) in &fresh_ground.bricks {
        assert_eq!(
            screen.hidden_bricks.get(coordinate),
            Some(mesh),
            "{coordinate:?} is held hidden as a fresh view draws it"
        );
    }
    assert_eq!(
        screen.hidden_bricks.len(),
        view.held_brick_pictures().with_mesh
    );
    // A brick the camera is far from would take the place of one it is near in the renderer's tables of the sun: it is told when it is drawn.
    for coordinate in fresh_ground.occupancy.keys() {
        assert!(
            !screen.occupancy.contains_key(coordinate),
            "{coordinate:?} was told to the sun while only held"
        );
    }
    view.check_consistency().expect("the records agree");
}

#[test]
fn a_switch_to_the_warm_viewpoint_is_one_update_that_asks_nothing_makes_nothing_and_draws_what_a_fresh_view_does()
 {
    let trip = Trip::new();
    let fresh_ground = what_a_fresh_view_draws(&trip.scene, brick(0, 0, 0), &far());
    let asked = Cell::new(0);
    let terrain = Counting {
        level: trip.scene.world.main(),
        asked: &asked,
    };
    let mut screen = Screen::default();
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    view.mesher_mut().enable_occupancy();
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        &far(),
        "the sky, the ground warm",
    );

    asked.set(0);
    let selections = view.selections();
    view.set_warm_viewpoint(warm(sky(), &far()));
    let updates = view.update(GridId::MAIN, &terrain, Trip::ground(), &far());
    assert_eq!(view.selections(), selections, "no cut was worked out again");
    let made = updates
        .iter()
        .filter(|update| {
            matches!(
                update,
                MeshUpdate::Upload { .. }
                    | MeshUpdate::UploadCoarse { .. }
                    | MeshUpdate::UploadHiddenCoarse { .. }
                    | MeshUpdate::UploadHiddenBrick { .. }
            )
        })
        .count();
    screen.apply(updates);
    screen.apply_occupancy(view.mesher_mut().take_occupancy_updates());
    assert_eq!(asked.get(), 0, "nothing is asked of the terrain");
    assert_eq!(made, 0, "no picture is made or put up: they are moved");
    assert_eq!(
        screen.nodes, fresh_ground.nodes,
        "the nodes are what a fresh view draws, after the first update"
    );
    assert_bricks_are_what_a_fresh_view_draws(&screen, &fresh_ground, "the first update");
    view.check_consistency().expect("the records agree");
}

#[test]
fn an_edit_to_a_brick_of_the_warm_viewpoint_makes_its_held_picture_again_and_the_ground_is_what_a_fresh_view_draws()
 {
    let mut trip = Trip::new();
    let mut screen = Screen::default();
    let mut view = view_with_the_ground_warm(&trip, &mut screen);
    let held_before = screen.hidden_bricks.len();
    let edited = trip.scene.edit([0, 9, 5], "base_blocks:air");
    assert_eq!(edited, brick(0, 0, 0));
    view.mesher_mut().brick_changed(edited);
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "the sky, told",
    );
    assert_eq!(
        screen.hidden_bricks.len(),
        held_before,
        "the bricks that were held are held again"
    );
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        Trip::ground(),
        &far(),
        "the ground",
    );
    let fresh = what_a_fresh_view_draws(&trip.scene, brick(0, 0, 0), &far());
    assert_bricks_are_what_a_fresh_view_draws(&screen, &fresh, "the ground, after the edit");
}

#[test]
fn a_brick_that_is_wanted_by_the_drawn_viewpoint_is_shown_and_not_held_and_the_warm_one_does_not_take_its_picture()
 {
    let trip = Trip::new();
    let fresh_ground = what_a_fresh_view_draws(&trip.scene, brick(0, 0, 0), &far());
    let mut screen = Screen::default();
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    view.mesher_mut().enable_occupancy();
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        Trip::ground(),
        &far(),
        "the ground, warm too",
    );
    assert_bricks_are_what_a_fresh_view_draws(&screen, &fresh_ground, "the ground, warm too");
    assert!(screen.hidden_bricks.is_empty());
    assert_eq!(view.held_brick_pictures().bricks, 0);
}

#[test]
fn a_warm_viewpoint_that_moves_at_every_update_is_worked_out_again_once_in_a_while_and_not_at_every_update()
 {
    let trip = Trip::new();
    let mut screen = Screen::default();
    let mut view = view_with_the_ground_warm(&trip, &mut screen);
    let selections = view.selections();
    // (A terrain whose answers cannot go to another thread: the cubes are worked out on the thread of the update, at the update that asks.)
    let terrain = OnThisThread(trip.scene.world.main());
    // The warm viewpoint is a brick further along at every update for 40 updates: the first is worked out at once, and the next not for a while.
    for step in 1..=40 {
        view.set_warm_viewpoint(warm(viewer_in(brick(step, 0, 0)), &far()));
        screen.apply(view.update(GridId::MAIN, &terrain, sky(), &far()));
    }
    assert_eq!(
        view.selections() - selections,
        1,
        "worked out once in 40 updates of a viewpoint that moves at every one"
    );
    for step in 41..=110 {
        view.set_warm_viewpoint(warm(viewer_in(brick(step, 0, 0)), &far()));
        screen.apply(view.update(GridId::MAIN, &terrain, sky(), &far()));
    }
    assert_eq!(
        view.selections() - selections,
        2,
        "and once more in the 70 updates after"
    );
}

/// (This was a test that no brick round the warm viewpoint was handed out in the first update, while the drawn viewpoint had bricks to make: the two shared one limit on the
/// bricks in flight, and the drawn ones used it all. The warm ones are held under a limit of their own now, for their share of the threads, so that order is not the rule;
/// what is kept is that the drawn ones are handed out first, with all the room they had, and what is new is the warm room, exactly. The order that matters is the one the
/// threads take them in, which is `the_bricks_that_are_drawn_are_made_before_the_warm_ones_that_wait_though_those_were_handed_out_first`, further down.)
#[test]
fn the_bricks_round_the_drawn_viewpoint_are_handed_out_first_with_all_the_room_they_had_and_those_round_the_warm_one_get_a_room_of_their_own()
 {
    let trip = Trip::new();
    let made = std::cell::RefCell::new(Vec::new());
    let generated = std::cell::RefCell::new(Vec::new());
    let terrain = Recording {
        level: trip.scene.world.main(),
        made: &made,
        generated: &generated,
    };
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(viewer_in(brick(30, 0, 0)), &far()));
    screen_of_one_update(&mut view, &terrain);
    let asked = generated.borrow().clone();
    // The mesher has 3 threads and 1 of them is for warm work: 4 jobs for each thread are held for what is drawn (12), and 4 apart for what is only kept ready.
    assert_eq!(
        asked.len(),
        12 + 4,
        "{} bricks were handed out in the first update",
        asked.len()
    );
    for coordinate in &asked[..12] {
        assert!(
            coordinate.x().abs() <= 8 && coordinate.y().abs() <= 8 && coordinate.z().abs() <= 8,
            "{coordinate:?}, round the warm viewpoint, was handed out before the bricks of the drawn viewpoint"
        );
    }
    for coordinate in &asked[12..] {
        assert!(
            (coordinate.x() - 30).abs() <= 8
                && coordinate.y().abs() <= 8
                && coordinate.z().abs() <= 8,
            "{coordinate:?} is not round the warm viewpoint"
        );
    }
}

fn screen_of_one_update(view: &mut LodView, terrain: &impl CoarseTerrainSource) {
    let mut screen = Screen::default();
    screen.apply(view.update(GridId::MAIN, terrain, Trip::ground(), &far()));
}

/// The terrain of a level that cannot hand what it says to another thread: whatever works out cubes from it does so where it is asked to.
struct OnThisThread<'a>(&'a LevelData);

impl TerrainSource for OnThisThread<'_> {
    fn known(&self, coordinate: BrickCoordinate) -> Option<Arc<Brick>> {
        self.0.known(coordinate)
    }

    fn generator_for(&self, coordinate: BrickCoordinate) -> Option<Arc<dyn TerrainGenerator>> {
        self.0.generator_for(coordinate)
    }

    fn layout(&self) -> &Arc<GridLayout> {
        self.0.layout()
    }
}

impl CoarseTerrainSource for OnThisThread<'_> {
    fn version(&self) -> u64 {
        self.0.version()
    }

    fn may_show(&self, cube: BrickCube) -> bool {
        self.0.may_show(cube)
    }

    fn coarse_generator(&self, cube: BrickCube) -> Option<Arc<dyn TerrainGenerator>> {
        self.0.coarse_generator(cube)
    }
}

// ---- The cubes of the warm viewpoint are worked out on a thread of their own ----

/// A gate that the answers asked for on another thread wait at until a test opens it, and a count of the answers that were given through it.
#[derive(Debug)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
    asked: AtomicU64,
    /// How many of the copies of the terrain's answers that were handed to the thread have been let go of: how many workings out are over.
    finished: AtomicU64,
}

impl Gate {
    fn new(open: bool) -> Arc<Self> {
        Arc::new(Self {
            open: Mutex::new(open),
            changed: Condvar::new(),
            asked: AtomicU64::new(0),
            finished: AtomicU64::new(0),
        })
    }

    fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }

    fn close(&self) {
        *self.open.lock().unwrap() = false;
    }

    /// How many workings out the thread has done, as far as the copy of the answers it was given is concerned (the result is sent a moment after).
    fn finished(&self) -> u64 {
        self.finished.load(Ordering::Relaxed)
    }

    /// How many answers have been given through the gate.
    fn asked(&self) -> u64 {
        self.asked.load(Ordering::Relaxed)
    }

    /// Waits for the gate to be open, and counts the answer. A gate that nobody opens fails the thread that waits at it, and does not hold the test for ever.
    fn pass(&self) {
        let mut open = self.open.lock().unwrap();
        while !*open {
            let (guard, timeout) = self
                .changed
                .wait_timeout(open, std::time::Duration::from_secs(60))
                .unwrap();
            assert!(!timeout.timed_out(), "nobody opened the gate");
            open = guard;
        }
        drop(open);
        self.asked.fetch_add(1, Ordering::Relaxed);
    }
}

/// Counts a working out as over when the answers it was given are let go of.
struct CountsWhenDropped(Arc<Gate>);

impl Drop for CountsWhenDropped {
    fn drop(&mut self) {
        self.0.finished.fetch_add(1, Ordering::Relaxed);
    }
}

/// Opens a gate when it is dropped, so that a test that fails with the gate shut does not wait for the view that it leaves behind to give up (declare it after the view).
struct OpensWhenDropped(Arc<Gate>);

impl Drop for OpensWhenDropped {
    fn drop(&mut self) {
        self.0.open();
    }
}

/// The terrain of a level whose answers to another thread go through a gate, that can be made to say nothing may show, to say it is another version of itself, and to
/// have a bug in what it says to another thread.
struct Gated<'a> {
    level: &'a LevelData,
    gate: Arc<Gate>,
    dark: Cell<bool>,
    version: Cell<u64>,
    /// How many times the thread of the update was asked whether a cube may show anything.
    asked_here: Cell<u64>,
    bug: Cell<bool>,
}

impl<'a> Gated<'a> {
    fn new(level: &'a LevelData, gate: &Arc<Gate>) -> Self {
        Self {
            level,
            gate: Arc::clone(gate),
            dark: Cell::new(false),
            version: Cell::new(0),
            asked_here: Cell::new(0),
            bug: Cell::new(false),
        }
    }
}

impl TerrainSource for Gated<'_> {
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

impl CoarseTerrainSource for Gated<'_> {
    fn version(&self) -> u64 {
        self.version.get()
    }

    fn may_show(&self, cube: BrickCube) -> bool {
        self.asked_here.set(self.asked_here.get() + 1);
        !self.dark.get() && self.level.may_show(cube)
    }

    fn detached_may_show(&self) -> Option<MayShowOracle> {
        let terrain = self.level.terrain.clone();
        let (gate, dark, bug) = (Arc::clone(&self.gate), self.dark.get(), self.bug.get());
        let counted = CountsWhenDropped(Arc::clone(&self.gate));
        Some(Arc::new(move |cube| {
            let _ = &counted;
            gate.pass();
            assert!(!bug, "a bug in the answers of the terrain");
            !dark && terrain.may_show(cube)
        }))
    }

    fn coarse_generator(&self, cube: BrickCube) -> Option<Arc<dyn TerrainGenerator>> {
        self.level.coarse_generator(cube)
    }
}

/// What a view at the sky holds hidden when it keeps `place` warm, with an edit of `edit` told it first if there is one, as a reference: nothing goes wrong in it.
fn held_for_a_warm_place(
    trip: &Trip,
    edit: Option<BrickCube>,
    place: WorldPosition,
) -> HashSet<BrickCube> {
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    if let Some(cube) = edit {
        let stone = trip.scene.game.raw_block("base_blocks:stone");
        view.set_override(GridId::MAIN, cube, Some(node_of(stone)));
    }
    view.set_warm_viewpoint(warm(place, &far()));
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "a reference view",
    );
    screen.hidden.keys().copied().collect()
}

/// Updates the view at the sky until what is drawn is what a fresh view at the sky draws.
fn draw_the_sky(
    view: &mut LodView,
    screen: &mut Screen,
    terrain: &impl CoarseTerrainSource,
    fresh_sky: &Screen,
) {
    let started = std::time::Instant::now();
    loop {
        screen.apply(view.update(GridId::MAIN, terrain, sky(), &far()));
        if screen.nodes == fresh_sky.nodes {
            return;
        }
        assert!(started.elapsed() < DEADLINE, "the sky is not drawn");
        std::thread::sleep(std::time::Duration::from_micros(300));
    }
}

#[test]
fn the_cubes_of_the_warm_viewpoint_are_worked_out_on_a_thread_of_their_own_and_what_is_drawn_does_not_wait_for_them()
 {
    let trip = Trip::new();
    let fresh_sky = what_a_fresh_view_draws(&trip.scene, brick(0, 100, 0), &far());
    let fresh_ground = what_a_fresh_view_draws(&trip.scene, brick(0, 0, 0), &far());
    let gate = Gate::new(false);
    let terrain = Gated::new(trip.scene.world.main(), &gate);
    let mut view = view_of(&trip.scene, 3);
    let _opens = OpensWhenDropped(Arc::clone(&gate));
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();

    // The sky is drawn while the cubes of the ground are being worked out, and the thread that works them out has not been given an answer.
    draw_the_sky(&mut view, &mut screen, &terrain, &fresh_sky);
    assert!(
        view.warm_selection_pending(),
        "the cubes of the ground are still being worked out"
    );
    assert!(!view.is_settled(), "and the view has more to do");
    assert_eq!(gate.asked(), 0);
    assert!(
        screen.hidden.is_empty(),
        "nothing is held for the ground yet: {} pictures",
        screen.hidden.len()
    );
    assert_eq!(
        view.selections(),
        2,
        "the cubes of the sky and of the ground"
    );

    // The answers come, and the ground is made and held as it is when nothing waits.
    gate.open();
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        &far(),
        "the gate open",
    );
    assert!(!view.warm_selection_pending());
    assert_eq!(view.selections(), 2, "worked out once");
    assert!(gate.asked() > 1000, "{} answers", gate.asked());
    assert_eq!(screen.nodes, fresh_sky.nodes);
    assert_nodes_are_drawn_or_held(&screen, &fresh_ground, "the ground, warm");
}

#[test]
fn a_switch_to_a_place_whose_cubes_were_worked_out_on_the_thread_asks_the_terrain_nothing_and_works_nothing_out()
 {
    let trip = Trip::new();
    let fresh_ground = what_a_fresh_view_draws(&trip.scene, brick(0, 0, 0), &far());
    let gate = Gate::new(true);
    let terrain = Gated::new(trip.scene.world.main(), &gate);
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        &far(),
        "the sky, the ground warm",
    );
    assert!(
        gate.asked() > 1000,
        "the ground was worked out on the thread"
    );

    let (asked_there, asked_here, selections) =
        (gate.asked(), terrain.asked_here.get(), view.selections());
    view.set_warm_viewpoint(warm(sky(), &far()));
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        Trip::ground(),
        &far(),
        "the ground, the sky warm",
    );
    assert_eq!(gate.asked(), asked_there, "the thread was asked nothing");
    assert_eq!(terrain.asked_here.get(), asked_here, "nor was the update");
    assert_eq!(view.selections(), selections, "no cube was worked out");
    assert_eq!(screen.nodes, fresh_ground.nodes);
}

#[test]
fn cubes_worked_out_for_a_terrain_that_has_changed_since_are_not_used_and_are_worked_out_again() {
    let trip = Trip::new();
    let gate = Gate::new(false);
    let terrain = Gated::new(trip.scene.world.main(), &gate);
    let mut view = view_of(&trip.scene, 3);
    let _opens = OpensWhenDropped(Arc::clone(&gate));
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    screen.apply(view.update(GridId::MAIN, &terrain, sky(), &far()));
    assert!(view.warm_selection_pending());

    // The terrain is another while the cubes are being worked out for the one it was, and says nothing may show.
    terrain.version.set(1);
    terrain.dark.set(true);
    for _ in 0..5 {
        screen.apply(view.update(GridId::MAIN, &terrain, sky(), &far()));
    }
    assert!(view.warm_selection_pending(), "the old working out goes on");
    assert_eq!(
        view.selections(),
        3,
        "the cubes of the sky for the other terrain"
    );
    gate.open();
    // The cubes that come in are of a terrain that is not this one: they are thrown away, and another working out is started at that update.
    let started = std::time::Instant::now();
    while view.selections() < 4 && view.warm_selection_pending() {
        screen.apply(view.update(GridId::MAIN, &terrain, sky(), &far()));
        assert!(
            started.elapsed() < DEADLINE,
            "the working out does not come in"
        );
        std::thread::sleep(std::time::Duration::from_micros(300));
    }
    assert_eq!(
        view.selections(),
        4,
        "the cubes were not used, and not waited for"
    );
    assert!(view.warm_selection_pending());
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        &far(),
        "the terrain changed",
    );
    // The sky for the one terrain and the other, the ground for the one and the other.
    assert_eq!(view.selections(), 4);
    assert!(
        screen.hidden.is_empty(),
        "{} held for a ground that may show nothing",
        screen.hidden.len()
    );
}

#[test]
fn an_edit_told_while_the_cubes_are_being_worked_out_is_in_the_cubes_that_are_held() {
    let trip = Trip::new();
    // A cube of the finest detail in the sky over the ground, with cubes of its size all round it: the cubes of the ground leave it out, and an edit in it brings it in.
    let selection = ViewSelection {
        detail: far().detail,
        zoom: None,
        fine_radius: 3,
        maximum_distance: far().maximum_distance,
    };
    let all = select_around_brick(brick(0, 0, 0), &selection, &|_| true);
    let all_set: HashSet<BrickCube> = all.iter().copied().collect();
    let mut in_the_sky: Vec<BrickCube> = all
        .iter()
        .copied()
        .filter(|cube| {
            cube.level() == 1
                && cube.origin().y() >= 8
                && touching(*cube)
                    .iter()
                    .all(|beside| all_set.contains(beside))
        })
        .collect();
    in_the_sky.sort_by_key(|cube| (cube.origin().y(), cube.origin().x(), cube.origin().z()));
    let cube = in_the_sky[0];
    let plain = held_for_a_warm_place(&trip, None, Trip::ground());
    let reference = held_for_a_warm_place(&trip, Some(cube), Trip::ground());
    assert!(
        reference.contains(&cube) && !plain.contains(&cube),
        "the edit brings its cube in"
    );

    let gate = Gate::new(false);
    let terrain = Gated::new(trip.scene.world.main(), &gate);
    let mut view = view_of(&trip.scene, 3);
    let _opens = OpensWhenDropped(Arc::clone(&gate));
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    screen.apply(view.update(GridId::MAIN, &terrain, sky(), &far()));
    assert!(view.warm_selection_pending());
    let stone = trip.scene.game.raw_block("base_blocks:stone");
    view.set_override(GridId::MAIN, cube, Some(node_of(stone)));
    // (The cubes of the sky are worked out again for the edit, once it has been long enough since they were.)
    for _ in 0..20 {
        screen.apply(view.update(GridId::MAIN, &terrain, sky(), &far()));
    }
    assert!(view.warm_selection_pending());
    assert_eq!(view.selections(), 3);
    gate.open();
    // The cubes that come in were worked out without the edit: they are thrown away, and another working out is started at that update.
    let started = std::time::Instant::now();
    while view.selections() < 4 && view.warm_selection_pending() {
        screen.apply(view.update(GridId::MAIN, &terrain, sky(), &far()));
        assert!(
            started.elapsed() < DEADLINE,
            "the working out does not come in"
        );
        std::thread::sleep(std::time::Duration::from_micros(300));
    }
    assert_eq!(
        view.selections(),
        4,
        "the cubes were not used, and not waited for"
    );
    assert!(view.warm_selection_pending());
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        &far(),
        "an edit told in the middle",
    );
    let held: HashSet<BrickCube> = screen.hidden.keys().copied().collect();
    assert_eq!(held, reference);
}

#[test]
fn a_reset_while_the_cubes_are_being_worked_out_drops_what_comes_of_it() {
    let trip = Trip::new();
    let gate = Gate::new(false);
    let terrain = Gated::new(trip.scene.world.main(), &gate);
    let mut view = view_of(&trip.scene, 3);
    let _opens = OpensWhenDropped(Arc::clone(&gate));
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    screen.apply(view.update(GridId::MAIN, &terrain, sky(), &far()));
    assert!(view.warm_selection_pending());

    // The viewer is in another level now, where nothing may show; what the thread says of the level it left is of no use.
    screen.apply(view.reset());
    assert!(!view.warm_selection_pending(), "nothing is waited for");
    terrain.dark.set(true);
    gate.open();
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        &far(),
        "after the reset",
    );
    assert!(screen.hidden.is_empty(), "{} held", screen.hidden.len());
    assert!(screen.nodes.is_empty());
}

#[test]
fn a_reset_forgets_what_the_thread_was_told_by_the_terrain() {
    let trip = Trip::new();
    let gate = Gate::new(true);
    let terrain = Gated::new(trip.scene.world.main(), &gate);
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        &far(),
        "the sky, the ground warm",
    );
    assert!(!screen.hidden.is_empty());

    // Another level, that has the same version and says nothing may show.
    screen.apply(view.reset());
    terrain.dark.set(true);
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        &far(),
        "another level",
    );
    assert!(screen.hidden.is_empty(), "{} held", screen.hidden.len());
}

#[test]
fn a_warm_viewpoint_taken_away_while_the_cubes_are_being_worked_out_holds_nothing_for_them() {
    let trip = Trip::new();
    let up_there = viewer_in(brick(0, 300, 0));
    let reference = held_for_a_warm_place(&trip, None, up_there);
    let on_the_ground = held_for_a_warm_place(&trip, None, Trip::ground());
    assert_ne!(reference, on_the_ground, "the two places are not alike");

    let gate = Gate::new(false);
    let terrain = Gated::new(trip.scene.world.main(), &gate);
    let mut view = view_of(&trip.scene, 3);
    let _opens = OpensWhenDropped(Arc::clone(&gate));
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    screen.apply(view.update(GridId::MAIN, &terrain, sky(), &far()));
    assert!(view.warm_selection_pending());

    view.set_warm_viewpoint(None);
    screen.apply(view.update(GridId::MAIN, &terrain, sky(), &far()));
    assert!(
        !view.warm_selection_pending(),
        "nothing is waited for any more"
    );
    // The thread finishes, and a viewpoint is kept warm somewhere else: only that one is held.
    gate.open();
    while gate.asked() < 1000 {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    view.set_warm_viewpoint(warm(up_there, &far()));
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        &far(),
        "kept warm somewhere else",
    );
    let held: HashSet<BrickCube> = screen.hidden.keys().copied().collect();
    assert_eq!(held, reference);
}

#[test]
#[should_panic(expected = "a bug in the answers of the terrain")]
fn a_bug_in_what_the_terrain_says_on_the_thread_is_a_panic_of_the_update() {
    let trip = Trip::new();
    let gate = Gate::new(true);
    let terrain = Gated::new(trip.scene.world.main(), &gate);
    terrain.bug.set(true);
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let started = std::time::Instant::now();
    loop {
        view.update(GridId::MAIN, &terrain, sky(), &far());
        assert!(started.elapsed() < DEADLINE, "the bug was never said");
        std::thread::sleep(std::time::Duration::from_micros(300));
    }
}

#[test]
fn a_warm_viewpoint_that_moves_at_every_update_is_not_worked_out_at_every_update_on_the_thread_either()
 {
    let trip = Trip::new();
    let mut screen = Screen::default();
    let mut view = view_with_the_ground_warm(&trip, &mut screen);
    let selections = view.selections();
    for step in 1..=130 {
        view.set_warm_viewpoint(warm(viewer_in(brick(step, 0, 0)), &far()));
        screen.apply(view.update(GridId::MAIN, trip.scene.world.main(), sky(), &far()));
        std::thread::sleep(std::time::Duration::from_micros(200));
    }
    let worked_out = view.selections() - selections;
    assert!(
        (1..=3).contains(&worked_out),
        "{worked_out} workings out in 130 updates"
    );
    // And what is asked for last is what is worked out in the end.
    let fresh_there = held_for_a_warm_place(&trip, None, viewer_in(brick(130, 0, 0)));
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "the warm viewpoint stopped",
    );
    let held: HashSet<BrickCube> = screen.hidden.keys().copied().collect();
    assert!(
        fresh_there.is_subset(&held),
        "{} of the {} cubes held for the place it ended at are held",
        fresh_there.intersection(&held).count(),
        fresh_there.len()
    );
}

#[test]
fn what_a_terrain_hands_to_another_thread_says_what_the_terrain_says() {
    let trip = Trip::new();
    let level = trip.scene.world.main();
    let oracle = level.detached_may_show().expect("a level has a copy");
    let cubes = select_around_brick(
        brick(0, 0, 0),
        &ViewSelection {
            detail: far().detail,
            zoom: None,
            fine_radius: 3,
            maximum_distance: far().maximum_distance,
        },
        &|_| true,
    );
    let (mut yes, mut no) = (0, 0);
    for cube in &cubes {
        let said = oracle(*cube);
        assert_eq!(said, level.may_show(*cube), "{cube:?}");
        if said {
            yes += 1;
        } else {
            no += 1;
        }
    }
    assert!(yes > 100 && no > 100, "{yes} may show and {no} may not");
}

#[test]
fn a_working_out_that_is_not_done_is_not_started_again_whatever_the_updates_and_the_place() {
    let trip = Trip::new();
    let gate = Gate::new(false);
    let terrain = Gated::new(trip.scene.world.main(), &gate);
    let mut view = view_of(&trip.scene, 3);
    let _opens = OpensWhenDropped(Arc::clone(&gate));
    view.hold_hidden_pictures(256 << 20);
    let mut screen = Screen::default();
    // More than the least number of updates between two workings out, and the place moves at every one of them.
    for step in 0..200 {
        view.set_warm_viewpoint(warm(viewer_in(brick(step, 0, 0)), &far()));
        screen.apply(view.update(GridId::MAIN, &terrain, sky(), &far()));
    }
    assert!(view.warm_selection_pending());
    assert_eq!(
        view.selections(),
        2,
        "the cubes of the sky, and of the first place of the warm viewpoint, once"
    );
}

#[test]
fn the_thread_goes_on_from_what_it_learnt_of_the_terrain_and_does_not_ask_again_what_it_asked() {
    let trip = Trip::new();
    let gate = Gate::new(true);
    let terrain = Gated::new(trip.scene.world.main(), &gate);
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        &far(),
        "the sky, the ground warm",
    );
    let first = gate.asked();
    assert!(first > 1000, "{first} answers for the ground");

    // The place moves a brick: nearly every cube that is asked about is one that was asked about.
    view.set_warm_viewpoint(warm(viewer_in(brick(1, 0, 0)), &far()));
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        &far(),
        "the place moved a brick",
    );
    let second = gate.asked() - first;
    assert!(
        second * 4 < first,
        "{second} answers for the next place, {first} for the first"
    );
}

#[test]
fn a_view_that_is_working_out_the_cubes_of_its_warm_viewpoint_again_has_not_settled() {
    let trip = Trip::new();
    let gate = Gate::new(true);
    let terrain = Gated::new(trip.scene.world.main(), &gate);
    let mut view = view_of(&trip.scene, 3);
    let _opens = OpensWhenDropped(Arc::clone(&gate));
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        &far(),
        "the sky, the ground warm",
    );
    assert!(view.is_settled());

    // The terrain is another version of itself, which makes the cubes of the ground out of date though they are for the place that it is: they are worked out again.
    gate.close();
    terrain.version.set(1);
    let started = std::time::Instant::now();
    while !view.warm_selection_pending() {
        screen.apply(view.update(GridId::MAIN, &terrain, sky(), &far()));
        assert!(started.elapsed() < DEADLINE, "never worked out again");
        std::thread::sleep(std::time::Duration::from_micros(300));
    }
    assert!(
        !view.is_settled(),
        "the cubes are being worked out and the view says it has nothing more to do"
    );
    gate.open();
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        &far(),
        "worked out again",
    );
}

#[test]
fn cubes_that_come_in_after_the_warm_viewpoint_was_taken_away_are_not_used_when_it_is_set_again() {
    let trip = Trip::new();
    let gate = Gate::new(false);
    let terrain = Gated::new(trip.scene.world.main(), &gate);
    let mut view = view_of(&trip.scene, 3);
    let _opens = OpensWhenDropped(Arc::clone(&gate));
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    screen.apply(view.update(GridId::MAIN, &terrain, sky(), &far()));
    assert!(view.warm_selection_pending());
    view.set_warm_viewpoint(None);
    screen.apply(view.update(GridId::MAIN, &terrain, sky(), &far()));

    // The thread finishes what it was at, and the cubes wait to be taken in. Then the viewpoint is set again, where it was, for a terrain that says nothing may show (the
    // same version of it: that is what the terrain of another level may say).
    gate.open();
    let started = std::time::Instant::now();
    while gate.finished() < 1 {
        assert!(started.elapsed() < DEADLINE, "the working out does not end");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    std::thread::sleep(std::time::Duration::from_millis(30));
    terrain.dark.set(true);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    screen.apply(view.update(GridId::MAIN, &terrain, sky(), &far()));
    assert!(
        view.warm_selection_pending(),
        "the cubes of the time before were used, and nothing was asked of the terrain as it is now"
    );
    settle_checked_in(&mut view, &mut screen, &terrain, sky(), &far(), "set again");
    assert!(screen.hidden.is_empty(), "{} held", screen.hidden.len());
}

#[test]
fn bricks_that_are_made_for_the_warm_viewpoint_alone_do_not_ask_the_planner_to_look_again() {
    let trip = Trip::new();
    let mut screen = Screen::default();
    let mut view = view_with_the_ground_warm(&trip, &mut screen);
    let serial = view.mesher().ready_serial();
    let held = view.held_brick_pictures();

    // The warm viewpoint moves a few bricks along: bricks that are new to its ball are made, and held. What is drawn is not touched, and the planner of the swaps, which
    // looks at every cube that is drawn and every one that is wanted when something that may be drawn is ready, has nothing new to look at.
    view.set_warm_viewpoint(warm(viewer_in(brick(6, 0, 0)), &far()));
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "the warm viewpoint moved",
    );
    let later = view.held_brick_pictures();
    assert!(
        later.with_mesh > held.with_mesh,
        "bricks were made for the new place: {} held before, {} after",
        held.with_mesh,
        later.with_mesh
    );
    assert_eq!(
        view.mesher().ready_serial(),
        serial,
        "a brick that only the warm viewpoint wants was made, and the planner was asked to look again"
    );
}

// ---- A warm viewpoint taken away ----

/// A view that has drawn the ground and then the sky, so that the pictures of the ground are held because they were shown and taken down: the view's own.
fn view_with_pictures_of_its_own(trip: &Trip, screen: &mut Screen) -> LodView {
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    for (viewer, context) in [(Trip::ground(), "the ground"), (sky(), "the sky")] {
        settle_checked_in(
            &mut view,
            screen,
            trip.scene.world.main(),
            viewer,
            &far(),
            context,
        );
    }
    view
}

#[test]
fn a_warm_viewpoint_taken_away_lets_go_of_what_was_made_for_it_alone_and_keeps_what_the_view_held_of_its_own()
 {
    let trip = Trip::new();
    let fresh_sky = what_a_fresh_view_draws(&trip.scene, brick(0, 100, 0), &far());
    let mut screen = Screen::default();
    let mut view = view_with_pictures_of_its_own(&trip, &mut screen);
    let own_nodes: HashSet<BrickCube> = screen.hidden.keys().copied().collect();
    let own_bricks: HashSet<BrickCoordinate> = screen.hidden_bricks.keys().copied().collect();
    let own_held = (view.held_pictures(), view.held_brick_pictures());
    assert!(
        own_nodes.len() > 100 && own_bricks.len() > 20,
        "the ground is held: {} nodes and {} bricks",
        own_nodes.len(),
        own_bricks.len()
    );

    // A place that was never drawn is kept warm: pictures are made for it.
    let along = viewer_in(brick(30, 0, 0));
    view.set_warm_viewpoint(warm(along, &far()));
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "a place kept warm",
    );
    assert!(
        screen.hidden.len() > own_nodes.len() + 100
            && screen.hidden_bricks.len() > own_bricks.len() + 20,
        "pictures were made for the warm place: {} nodes and {} bricks held",
        screen.hidden.len(),
        screen.hidden_bricks.len()
    );

    // Taken away: what was made for it is let go of at once, and what the ground left is not.
    view.set_warm_viewpoint(None);
    let updates = view.update(GridId::MAIN, trip.scene.world.main(), sky(), &far());
    screen.apply(updates);
    let held_nodes: HashSet<BrickCube> = screen.hidden.keys().copied().collect();
    let held_bricks: HashSet<BrickCoordinate> = screen.hidden_bricks.keys().copied().collect();
    assert_eq!(held_nodes, own_nodes);
    assert_eq!(held_bricks, own_bricks);
    assert_eq!((view.held_pictures(), view.held_brick_pictures()), own_held);
    view.check_consistency().expect("the records agree");
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "taken away",
    );
    assert_eq!(screen.nodes, fresh_sky.nodes, "what is drawn is untouched");
    assert_eq!(screen.hidden.len(), own_nodes.len());
    assert_eq!(screen.hidden_bricks.len(), own_bricks.len());
}

#[test]
fn without_the_release_a_warm_viewpoint_taken_away_leaves_what_was_made_for_it_to_the_budget() {
    let trip = Trip::new();
    let mut screen = Screen::default();
    let mut view = view_with_pictures_of_its_own(&trip, &mut screen);
    view.set_release_warm_pictures(false);
    view.set_warm_viewpoint(warm(viewer_in(brick(30, 0, 0)), &far()));
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "a place kept warm",
    );
    let (nodes, bricks) = (screen.hidden.len(), screen.hidden_bricks.len());
    view.set_warm_viewpoint(None);
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "taken away",
    );
    assert_eq!(
        (screen.hidden.len(), screen.hidden_bricks.len()),
        (nodes, bricks),
        "nothing was let go of: the budget is large"
    );
}

#[test]
fn what_was_made_for_the_warm_viewpoint_is_not_let_go_of_when_it_is_taken_away_as_the_viewer_goes_there()
 {
    let trip = Trip::new();
    let fresh_ground = what_a_fresh_view_draws(&trip.scene, brick(0, 0, 0), &far());
    let mut screen = Screen::default();
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "the sky, the ground warm",
    );
    // The viewer goes to the ground and the warm viewpoint is taken away in the same update: what was made for it is wanted now, and is shown by moves.
    view.set_warm_viewpoint(None);
    let (mut uploads, mut brick_uploads) = (0, 0);
    let started = std::time::Instant::now();
    loop {
        let updates = view.update(
            GridId::MAIN,
            trip.scene.world.main(),
            Trip::ground(),
            &far(),
        );
        uploads += updates
            .iter()
            .filter(|update| matches!(update, MeshUpdate::UploadCoarse { mesh, .. } if !mesh.is_empty()))
            .count();
        brick_uploads += updates
            .iter()
            .filter(|update| matches!(update, MeshUpdate::Upload { mesh, .. } if !mesh.is_empty()))
            .count();
        screen.apply(updates);
        screen.apply_occupancy(view.mesher_mut().take_occupancy_updates());
        view.check_consistency().expect("the records agree");
        if view.is_settled() {
            break;
        }
        assert!(started.elapsed() < DEADLINE, "does not settle");
        std::thread::sleep(std::time::Duration::from_micros(300));
    }
    assert_eq!(screen.nodes, fresh_ground.nodes);
    assert_eq!(screen.bricks, fresh_ground.bricks);
    assert!(
        uploads * 20 < fresh_ground.nodes.len(),
        "{uploads} nodes were put up for the ground, which has {}",
        fresh_ground.nodes.len()
    );
    assert_eq!(
        brick_uploads, 0,
        "every brick of the ground was made for the warm viewpoint, and is shown by a move"
    );
}

#[test]
fn a_warm_viewpoint_that_moves_on_leaves_what_was_made_for_the_place_it_left_to_the_budget() {
    let trip = Trip::new();
    let mut screen = Screen::default();
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "the sky, the ground warm",
    );
    let first: HashSet<BrickCube> = screen.hidden.keys().copied().collect();
    view.set_warm_viewpoint(warm(viewer_in(brick(150, 0, 0)), &far()));
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "the warm viewpoint moved on",
    );
    for cube in &first {
        assert!(screen.hidden.contains_key(cube), "{cube:?} was let go of");
    }
}

#[test]
fn a_warm_viewpoint_taken_away_as_the_viewer_goes_there_leaves_nothing_pinned_for_the_budget_to_spare()
 {
    let trip = Trip::new();
    let budget = 64 << 10;
    let mut screen = Screen::default();
    let mut view = view_of(&trip.scene, 3);
    view.hold_hidden_pictures(budget);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "the sky, the ground warm",
    );
    // The viewer goes where the warm viewpoint is, and it is taken away: the cubes of the place were its, and are the viewer's now.
    view.set_warm_viewpoint(None);
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        Trip::ground(),
        &far(),
        "the ground",
    );
    // And away again: what was drawn is taken down and held, within the budget, as any picture the view's own.
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        sky(),
        &far(),
        "the sky again",
    );
    assert_held_pictures_match(&screen, &view, budget, "the sky again");
    assert!(
        view.held_brick_pictures().bytes <= budget,
        "{} bytes of bricks are held of {budget}",
        view.held_brick_pictures().bytes
    );
}

// ---- A share of the threads for the warm viewpoint (docs/specs/viewpoints-spec.md, section 9l) ----

/// A generator whose nodes (or whose bricks, for [`Counted::of_bricks`]) go through a gate, take a moment, and are counted as they are made: how many at the same time, at most.
#[derive(Debug)]
struct Counting1 {
    inner: Arc<dyn TerrainGenerator>,
    gate: Arc<Gate>,
    now: Arc<std::sync::atomic::AtomicUsize>,
    most: Arc<std::sync::atomic::AtomicUsize>,
    pause: std::time::Duration,
    /// The cubes whose nodes (or bricks) were started, in the order the threads started them.
    started: Arc<Mutex<Vec<BrickCube>>>,
    /// Whether the bricks are what is counted, and the nodes are made as they are.
    bricks: bool,
}

impl Counting1 {
    /// Counts one made: records the cube, waits at the gate, takes a moment, then does `make`.
    fn counted<T>(&self, cube: BrickCube, make: impl FnOnce() -> T) -> T {
        self.started.lock().unwrap().push(cube);
        let now = self.now.fetch_add(1, Ordering::SeqCst) + 1;
        self.most.fetch_max(now, Ordering::SeqCst);
        self.gate.pass();
        std::thread::sleep(self.pause);
        let made = make();
        self.now.fetch_sub(1, Ordering::SeqCst);
        made
    }
}

impl TerrainGenerator for Counting1 {
    fn generate_brick(&self, layout: &GridLayout, coordinate: BrickCoordinate) -> Brick {
        if !self.bricks {
            return self.inner.generate_brick(layout, coordinate);
        }
        let cube = BrickCube::new(coordinate, 0).expect("a brick is a cube");
        self.counted(cube, || self.inner.generate_brick(layout, coordinate))
    }

    fn coarse_node(&self, cube: BrickCube, unlit: u8) -> Option<CoarseNode> {
        if self.bricks {
            return self.inner.coarse_node(cube, unlit);
        }
        self.counted(cube, || self.inner.coarse_node(cube, unlit))
    }
}

/// The terrain of a level whose nodes are made through a gate and counted ([`Counting1`]), or whose bricks are ([`Counted::of_bricks`]).
struct Counted<'a> {
    level: &'a LevelData,
    gate: Arc<Gate>,
    now: Arc<std::sync::atomic::AtomicUsize>,
    most: Arc<std::sync::atomic::AtomicUsize>,
    pause: std::time::Duration,
    started: Arc<Mutex<Vec<BrickCube>>>,
    bricks: bool,
    /// The gate that what is handed out from now on goes through, once [`Counted::use_another_gate`] has put one in place of `gate`.
    later_gate: std::cell::RefCell<Option<Arc<Gate>>>,
}

impl<'a> Counted<'a> {
    fn new(level: &'a LevelData, open: bool, pause: std::time::Duration) -> Self {
        Self {
            level,
            gate: Gate::new(open),
            now: Arc::default(),
            most: Arc::default(),
            pause,
            started: Arc::default(),
            bricks: false,
            later_gate: std::cell::RefCell::new(None),
        }
    }

    /// The same, for the bricks: it is their generating that goes through the gate and is counted, and the nodes are made as they are.
    fn of_bricks(level: &'a LevelData, open: bool, pause: std::time::Duration) -> Self {
        Self {
            bricks: true,
            ..Self::new(level, open, pause)
        }
    }

    /// A new gate, shut, that what is handed out from now on goes through: what was handed out before stays at the gate it had.
    fn use_another_gate(&self) -> Arc<Gate> {
        let gate = Gate::new(false);
        *self.later_gate.borrow_mut() = Some(Arc::clone(&gate));
        gate
    }

    fn wrapped(&self, inner: Arc<dyn TerrainGenerator>) -> Arc<dyn TerrainGenerator> {
        Arc::new(Counting1 {
            inner,
            gate: self
                .later_gate
                .borrow()
                .clone()
                .unwrap_or_else(|| Arc::clone(&self.gate)),
            now: Arc::clone(&self.now),
            most: Arc::clone(&self.most),
            pause: self.pause,
            started: Arc::clone(&self.started),
            bricks: self.bricks,
        })
    }

    /// The cubes whose nodes were started since this was last asked, in the order the threads started them.
    fn started(&self) -> Vec<BrickCube> {
        std::mem::take(&mut *self.started.lock().unwrap())
    }

    /// How many have been started since this was last asked for them, without taking them.
    fn started_count(&self) -> usize {
        self.started.lock().unwrap().len()
    }

    /// The most nodes that were being made at the same time since this was last asked.
    fn most_at_once(&self) -> usize {
        self.most
            .swap(self.now.load(Ordering::SeqCst), Ordering::SeqCst)
    }
}

impl TerrainSource for Counted<'_> {
    fn known(&self, coordinate: BrickCoordinate) -> Option<Arc<Brick>> {
        self.level.known(coordinate)
    }

    fn generator_for(&self, coordinate: BrickCoordinate) -> Option<Arc<dyn TerrainGenerator>> {
        let inner = self.level.generator_for(coordinate)?;
        Some(if self.bricks {
            self.wrapped(inner)
        } else {
            inner
        })
    }

    fn layout(&self) -> &Arc<GridLayout> {
        self.level.layout()
    }
}

impl CoarseTerrainSource for Counted<'_> {
    fn version(&self) -> u64 {
        self.level.version()
    }

    fn may_show(&self, cube: BrickCube) -> bool {
        self.level.may_show(cube)
    }

    fn coarse_generator(&self, cube: BrickCube) -> Option<Arc<dyn TerrainGenerator>> {
        let inner = self.level.coarse_generator(cube)?;
        Some(self.wrapped(inner))
    }
}

fn view_with(scene: &Scene, workers: usize) -> LodView {
    LodView::new(
        scene.mesher(3),
        LodConfig {
            unlit: 0,
            worker_count: workers,
        },
    )
}

#[test]
fn a_view_makes_the_nodes_of_its_warm_place_on_half_its_threads_at_most_and_all_of_them() {
    let trip = Trip::new();
    let fresh_ground = what_a_fresh_view_draws(&trip.scene, brick(0, 0, 0), &far());
    let terrain = Counted::new(
        trip.scene.world.main(),
        true,
        std::time::Duration::from_millis(1),
    );
    let mut view = view_with(&trip.scene, 6);
    assert_eq!(view.warm_worker_count(), 3, "half of 6");
    assert_eq!(
        view_with(&trip.scene, 1).warm_worker_count(),
        1,
        "at least one"
    );
    assert_eq!(
        view_with(&trip.scene, 11).warm_worker_count(),
        5,
        "half of 11, down"
    );
    view.hold_hidden_pictures(256 << 20);
    let mut screen = Screen::default();
    // The sky is drawn alone first: what it takes is not what is looked at.
    settle_checked_in(&mut view, &mut screen, &terrain, sky(), &far(), "the sky");
    terrain.most_at_once();
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        &far(),
        "the sky, the ground warm",
    );
    let most = terrain.most_at_once();
    assert!(
        most <= 3,
        "{most} nodes were made at the same time for a place that is only kept ready, by 6 threads"
    );
    assert_nodes_are_drawn_or_held(&screen, &fresh_ground, "the ground, warm");

    // A view that is told to use more threads for it does.
    view.set_warm_worker_count(6);
    assert_eq!(view.warm_worker_count(), 6);
}

#[test]
fn what_is_held_for_the_warm_place_is_apart_from_what_is_held_for_the_drawn_one_and_neither_takes_the_others()
 {
    let trip = Trip::new();
    let terrain = Counted::new(trip.scene.world.main(), false, std::time::Duration::ZERO);
    let mut view = view_with(&trip.scene, 2);
    let _opens = OpensWhenDropped(Arc::clone(&terrain.gate));
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let started = std::time::Instant::now();
    while view.warm_nodes_being_made() < 4 {
        view.update(GridId::MAIN, &terrain, sky(), &far());
        assert!(
            started.elapsed() < DEADLINE,
            "no nodes were handed out for the ground"
        );
        std::thread::sleep(std::time::Duration::from_micros(300));
    }
    // 2 threads hold 4 each of what is drawn, and 1 thread (half of 2) holds 4 of what is only kept ready: the drawn jobs are not fewer for the warm ones, and
    // the warm ones are not more than the share holds.
    view.update(GridId::MAIN, &terrain, sky(), &far());
    assert_eq!(view.warm_nodes_being_made(), 4);
    assert_eq!(view.nodes_being_made() - view.warm_nodes_being_made(), 8);
}

#[test]
fn a_warm_job_that_waits_when_its_place_is_the_one_that_is_drawn_is_made_as_a_drawn_one_and_a_reset_forgets_what_was_held()
 {
    let trip = Trip::new();
    let fresh_ground = what_a_fresh_view_draws(&trip.scene, brick(0, 0, 0), &far());
    let terrain = Counted::new(trip.scene.world.main(), false, std::time::Duration::ZERO);
    // One thread, held at the gate by a drawn job: what the view hands out for the ground waits behind it.
    let mut view = view_with(&trip.scene, 1);
    let _opens = OpensWhenDropped(Arc::clone(&terrain.gate));
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let started = std::time::Instant::now();
    while view.warm_nodes_being_made() < 4 {
        view.update(GridId::MAIN, &terrain, sky(), &far());
        assert!(
            started.elapsed() < DEADLINE,
            "no nodes were handed out for the ground"
        );
        std::thread::sleep(std::time::Duration::from_micros(300));
    }
    let drawn_before = view.nodes_being_made() - view.warm_nodes_being_made();
    assert_eq!(view.warm_nodes_being_made(), 4);

    // The viewer goes to the ground: the nodes that were handed out for it are its own now, and are counted so.
    view.set_warm_viewpoint(None);
    view.update(GridId::MAIN, &terrain, Trip::ground(), &far());
    assert_eq!(
        view.warm_nodes_being_made(),
        0,
        "they wait, and are drawn ones now"
    );
    assert_eq!(view.nodes_being_made(), drawn_before + 4);

    // The gate opens: the view is what a fresh view at the ground is, and nothing is left in flight.
    let mut screen = Screen::default();
    terrain.gate.open();
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        Trip::ground(),
        &far(),
        "the ground",
    );
    assert_eq!(screen.nodes, fresh_ground.nodes);
    assert_eq!(view.nodes_being_made(), 0);
    assert_eq!(view.warm_nodes_being_made(), 0);

    // A reset forgets what was held for the warm place, as what was held for the drawn one.
    terrain.gate.close();
    view.set_warm_viewpoint(warm(sky(), &far()));
    let started = std::time::Instant::now();
    while view.warm_nodes_being_made() == 0 {
        view.update(GridId::MAIN, &terrain, Trip::ground(), &far());
        assert!(
            started.elapsed() < DEADLINE,
            "no nodes were handed out for the sky"
        );
        std::thread::sleep(std::time::Duration::from_micros(300));
    }
    let _ = view.reset();
    assert_eq!(view.warm_nodes_being_made(), 0);
    assert_eq!(view.nodes_being_made(), 0);
}

// ---- A share of the threads for the bricks of the warm viewpoint too (docs/specs/viewpoints-spec.md, section 9o) ----

/// How long the tests of the threads that hold at a gate wait for what they are waiting for: a few jobs that take no time, so this is for a view that is stuck. It is shorter
/// than the minute a thread waits at a gate for somebody to open it ([`Gate::pass`]), so that a test that fails does so by its own assertion, and not with a thread that gave up
/// at the gate at the same moment (which ended the whole test binary).
const GATED_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

fn view_with_brick_threads(scene: &Scene, node_workers: usize, brick_workers: usize) -> LodView {
    LodView::new(
        scene.mesher_with_workers(3, brick_workers),
        LodConfig {
            unlit: 0,
            worker_count: node_workers,
        },
    )
}

/// Updates the view, at `viewer`, and puts what it says on the screen, until `condition` holds of it.
fn update_until(
    view: &mut LodView,
    screen: &mut Screen,
    terrain: &impl CoarseTerrainSource,
    viewer: WorldPosition,
    what: &str,
    mut condition: impl FnMut(&LodView) -> bool,
) {
    let started = std::time::Instant::now();
    while !condition(view) {
        screen.apply(view.update(GridId::MAIN, terrain, viewer, &far()));
        assert!(started.elapsed() < GATED_DEADLINE, "{what}");
        std::thread::sleep(std::time::Duration::from_micros(300));
    }
}

/// The bricks that were handed to the threads for what is drawn, as the mesher counts them.
fn drawn_bricks_being_made(view: &LodView) -> usize {
    view.mesher().pending_jobs() - view.mesher().warm_bricks_being_made()
}

#[test]
fn a_view_makes_the_bricks_of_its_warm_place_on_half_the_threads_of_its_mesher_at_most_and_all_of_them()
 {
    let trip = Trip::new();
    let fresh_ground = what_a_fresh_view_draws(&trip.scene, brick(0, 0, 0), &far());
    let terrain = Counted::of_bricks(trip.scene.world.main(), false, std::time::Duration::ZERO);
    let mut view = view_with_brick_threads(&trip.scene, 2, 6);
    let _opens = OpensWhenDropped(Arc::clone(&terrain.gate));
    view.hold_hidden_pictures(256 << 20);
    let mut screen = Screen::default();
    // The sky alone first: nothing is assigned there, so no brick is made, and the closed gate holds nothing.
    settle_checked_in(&mut view, &mut screen, &terrain, sky(), &far(), "the sky");
    assert_eq!(terrain.started_count(), 0);

    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    update_until(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        "no brick of the ground was started",
        |_| terrain.started_count() >= 3,
    );
    // A while with the gate shut: the threads that are not for warm work take no brick (of 6 threads, 3 are), though more wait than the 3 that are held.
    for _ in 0..100 {
        screen.apply(view.update(GridId::MAIN, &terrain, sky(), &far()));
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(
        terrain.started_count(),
        3,
        "bricks that are only kept ready were started on more threads than the share"
    );
    assert_eq!(terrain.most_at_once(), 3);

    // A view that is told to use more threads for it does, at once: the bricks that wait are taken by the threads that were not allowed to.
    view.set_warm_worker_count(6);
    update_until(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        "the threads that were let into warm work took no brick",
        |_| terrain.started_count() >= 6,
    );

    terrain.gate.open();
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        &far(),
        "the sky, the ground warm",
    );
    assert!(screen.bricks.is_empty(), "no brick is drawn at the sky");
    for (coordinate, mesh) in &fresh_ground.bricks {
        assert_eq!(
            screen.hidden_bricks.get(coordinate),
            Some(mesh),
            "{coordinate:?} is held hidden as a fresh view draws it"
        );
    }
    assert_eq!(view.mesher().warm_bricks_being_made(), 0);
    assert_eq!(view.mesher().pending_jobs(), 0);
}

#[test]
fn what_is_handed_out_for_the_warm_bricks_is_apart_from_what_is_handed_out_for_the_drawn_ones_and_neither_takes_the_others_room()
 {
    let trip = Trip::new();
    let terrain = Counted::of_bricks(trip.scene.world.main(), false, std::time::Duration::ZERO);
    let mut view = view_with_brick_threads(&trip.scene, 1, 2);
    let _opens = OpensWhenDropped(Arc::clone(&terrain.gate));
    view.hold_hidden_pictures(256 << 20);
    // The ground is drawn, and a place 20 bricks along is only kept ready.
    view.set_warm_viewpoint(warm(viewer_in(brick(20, 0, 0)), &far()));
    let mut screen = Screen::default();
    update_until(
        &mut view,
        &mut screen,
        &terrain,
        Trip::ground(),
        "bricks were not handed out for both",
        |view| view.mesher().warm_bricks_being_made() >= 4 && drawn_bricks_being_made(view) >= 8,
    );
    screen.apply(view.update(GridId::MAIN, &terrain, Trip::ground(), &far()));
    // 2 threads hold 4 each of what is drawn, and 1 thread (half of 2) holds 4 of what is only kept ready: the drawn bricks are not fewer for the warm ones, and the warm
    // ones are not more than the share holds.
    assert_eq!(view.mesher().warm_bricks_being_made(), 4);
    assert_eq!(drawn_bricks_being_made(&view), 8);
}

#[test]
fn a_warm_brick_that_waits_when_its_place_is_the_one_that_is_drawn_is_made_as_a_drawn_one_and_a_reset_forgets_what_was_held()
 {
    let trip = Trip::new();
    let fresh_ground = what_a_fresh_view_draws(&trip.scene, brick(0, 0, 0), &far());
    let terrain = Counted::of_bricks(trip.scene.world.main(), false, std::time::Duration::ZERO);
    // One thread, held at the gate by a brick of the ground that is only kept ready: what is handed out for it after waits behind it.
    let mut view = view_with_brick_threads(&trip.scene, 1, 1);
    let _opens = OpensWhenDropped(Arc::clone(&terrain.gate));
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    update_until(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        "no brick of the ground was handed out and started",
        |view| view.mesher().warm_bricks_being_made() >= 4 && terrain.started_count() >= 1,
    );
    assert_eq!(view.mesher().warm_bricks_being_made(), 4);

    // The viewer goes to the ground: the bricks that wait are its own now, and are counted so. The one the thread has taken stays what it was handed out as.
    view.set_warm_viewpoint(None);
    screen.apply(view.update(GridId::MAIN, &terrain, Trip::ground(), &far()));
    assert_eq!(
        view.mesher().warm_bricks_being_made(),
        1,
        "the ones that wait are drawn ones now"
    );

    // The gate opens: the view is what a fresh view at the ground is, and nothing is left in flight.
    terrain.gate.open();
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        Trip::ground(),
        &far(),
        "the ground",
    );
    assert_eq!(screen.bricks, fresh_ground.bricks);
    assert_eq!(view.mesher().warm_bricks_being_made(), 0);
    assert_eq!(view.mesher().pending_jobs(), 0);

    // A reset forgets what was held for the warm place, as what was held for the drawn one.
    terrain.gate.close();
    view.set_warm_viewpoint(warm(viewer_in(brick(20, 0, 0)), &far()));
    update_until(
        &mut view,
        &mut screen,
        &terrain,
        Trip::ground(),
        "no brick was handed out for the place 20 bricks along",
        |view| view.mesher().warm_bricks_being_made() > 0,
    );
    let _ = view.reset();
    assert_eq!(view.mesher().warm_bricks_being_made(), 0);
    assert_eq!(view.mesher().pending_jobs(), 0);
}

#[test]
fn what_was_handed_out_for_the_warm_place_before_a_reset_is_not_counted_with_what_is_handed_out_after_it_when_it_is_over()
 {
    let trip = Trip::new();
    let terrain = Counted::of_bricks(trip.scene.world.main(), false, std::time::Duration::ZERO);
    // One thread for warm work, held at the first gate by a brick that was handed out before the reset, with 3 more that wait behind it.
    let mut view = view_with_brick_threads(&trip.scene, 1, 2);
    let _opens = OpensWhenDropped(Arc::clone(&terrain.gate));
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    update_until(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        "no brick of the ground was handed out and started",
        |view| view.mesher().warm_bricks_being_made() >= 4 && terrain.started_count() >= 1,
    );
    screen.apply(view.reset());
    assert_eq!(view.mesher().warm_bricks_being_made(), 0);

    // What is handed out after the reset goes through a gate of its own, behind the 4 that were: 4 are counted again.
    let second_gate = terrain.use_another_gate();
    let _opens_second = OpensWhenDropped(Arc::clone(&second_gate));
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    update_until(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        "no brick was handed out after the reset",
        |view| view.mesher().warm_bricks_being_made() >= 4,
    );
    // The first gate opens: the 4 bricks of before the reset are made, and then the thread starts one of those of after it, which waits at the second gate.
    terrain.gate.open();
    let began = std::time::Instant::now();
    while terrain.started_count() < 5 {
        assert!(
            began.elapsed() < GATED_DEADLINE,
            "{} of 5 bricks were started",
            terrain.started_count()
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    // What the view is told now is the end of 4 jobs of before the reset, and nothing of those of after it: they are not taken off the count of the 4 that wait.
    screen.apply(view.update(GridId::MAIN, &terrain, sky(), &far()));
    // (A count that was taken down is made up by what the same update hands out, up to the room there is: what shows it is how many bricks are being made.)
    assert_eq!(
        view.mesher().warm_bricks_being_made(),
        4,
        "jobs that were handed out before the reset were taken off the count of those handed out after it"
    );
    assert_eq!(
        view.mesher().pending_jobs(),
        4,
        "more bricks were handed out than the room there is: the count was taken down by jobs that were not its own"
    );

    // The second gate opens, and all that was handed out is made.
    second_gate.open();
    settle_checked_in(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        &far(),
        "the sky, the ground warm",
    );
    assert_eq!(view.mesher().warm_bricks_being_made(), 0);
    assert_eq!(view.mesher().pending_jobs(), 0);
}

#[test]
fn the_bricks_that_are_drawn_are_made_before_the_warm_ones_that_wait_though_those_were_handed_out_first()
 {
    let trip = Trip::new();
    let terrain = Counted::of_bricks(trip.scene.world.main(), false, std::time::Duration::ZERO);
    // 3 threads, 1 of them for warm work.
    let mut view = view_with_brick_threads(&trip.scene, 1, 3);
    let _opens = OpensWhenDropped(Arc::clone(&terrain.gate));
    view.hold_hidden_pictures(256 << 20);
    view.set_warm_viewpoint(warm(Trip::ground(), &far()));
    let mut screen = Screen::default();
    // The warm bricks come first: the thread for them takes one and is held at the gate, and the others that were handed out wait.
    update_until(
        &mut view,
        &mut screen,
        &terrain,
        sky(),
        "no brick of the ground was handed out and started",
        |view| view.mesher().pending_jobs() >= 4 && terrain.started_count() >= 1,
    );
    // Then the viewer goes 20 bricks along, to a place that has bricks to draw: the 2 threads that are free take the first two, held at the gate, and the rest wait.
    let along = viewer_in(brick(20, 0, 0));
    update_until(
        &mut view,
        &mut screen,
        &terrain,
        along,
        "the bricks of the new place were not handed out and started",
        |view| view.mesher().pending_jobs() >= 4 + 12 && terrain.started_count() >= 3,
    );
    // The gate opens, and the threads make what was handed out and nothing else (no update is made, so nothing is handed out after).
    terrain.gate.open();
    let began = std::time::Instant::now();
    while terrain.started_count() < 16 {
        assert!(
            began.elapsed() < GATED_DEADLINE,
            "{} of 16 bricks were started",
            terrain.started_count()
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let order = terrain.started();
    let is_warm = |cube: &BrickCube| cube.origin().x() < 10;
    let second_warm = order
        .iter()
        .enumerate()
        .filter(|(_, cube)| is_warm(cube))
        .nth(1)
        .map(|(position, _)| position)
        .expect("a second brick of the warm place was started");
    let drawn_before = order[..second_warm]
        .iter()
        .filter(|cube| !is_warm(cube))
        .count();
    // (The 3 threads write what they start a moment after they take it, so up to 3 of the 12 may be written out of order.)
    assert!(
        drawn_before >= 12 - 3,
        "only {drawn_before} of the 12 bricks that are drawn were started before the second brick that is only kept ready"
    );
}

#[test]
fn a_mesher_makes_the_bricks_of_a_place_kept_ready_on_half_its_threads_unless_told_otherwise_and_a_view_that_is_told_a_share_tells_its_mesher_as_well()
 {
    let scene = scene();
    assert_eq!(scene.mesher_with_workers(3, 6).warm_worker_count(), 3);
    assert_eq!(
        scene.mesher_with_workers(3, 1).warm_worker_count(),
        1,
        "at least one"
    );
    assert_eq!(
        scene.mesher_with_workers(3, 11).warm_worker_count(),
        5,
        "half of 11, down"
    );
    let mut mesher = scene.mesher_with_workers(3, 6);
    mesher.set_warm_worker_count(2);
    assert_eq!(mesher.warm_worker_count(), 2);
    mesher.set_warm_worker_count(0);
    assert_eq!(mesher.warm_worker_count(), 1, "at least one");
    mesher.set_warm_worker_count(100);
    assert_eq!(
        mesher.warm_worker_count(),
        6,
        "at most the threads there are"
    );

    // A view's share is for its nodes and its bricks, each at most the threads they have.
    let mut view = view_with_brick_threads(&scene, 4, 6);
    assert_eq!(view.warm_worker_count(), 2);
    assert_eq!(view.mesher().warm_worker_count(), 3);
    view.set_warm_worker_count(5);
    assert_eq!(view.warm_worker_count(), 4);
    assert_eq!(view.mesher().warm_worker_count(), 5);
}

// ---- A camera that moves a long way a frame keeps its cut while it is far from what it draws (docs/specs/viewpoints-spec.md, phase 5b) ----

/// How many bricks a viewer at `sky()` moves along x, to go from one place to another.
fn at_the_sky_along(bricks: i128) -> WorldPosition {
    viewer_in(brick(bricks, 100, 0))
}

#[test]
fn a_view_that_is_told_a_tolerance_keeps_its_cut_while_the_viewer_stays_a_share_of_the_distance_to_what_it_draws_from_the_place_the_cut_was_chosen_for()
 {
    let trip = Trip::new();
    let terrain = trip.scene.world.main();
    let mut view = view_of(&trip.scene, 3);
    view.set_reselect_tolerance(0.1);
    let first = at_the_sky_along(0);
    view.update(GridId::MAIN, terrain, first, &far());
    let first_cut = view.wanted().clone();
    let selections = view.selections();
    assert!(!first_cut.is_empty());
    assert_eq!(view.wanted_centre(), Some(first.brick()));

    // The ground is 100 bricks below: a tenth of that is about 10 bricks. A few bricks along keep the cut, and the place it was chosen for.
    for bricks in [1, 3, 5] {
        view.update(GridId::MAIN, terrain, at_the_sky_along(bricks), &far());
        assert_eq!(view.selections(), selections, "{bricks} bricks along");
        assert_eq!(view.wanted(), &first_cut);
        assert_eq!(view.wanted_centre(), Some(first.brick()));
    }
    // Coming back is no change either.
    view.update(GridId::MAIN, terrain, first, &far());
    assert_eq!(view.selections(), selections);

    // Past it the cut is chosen again, for where the viewer is, and what is drawn goes on being what the cut says.
    let far_along = at_the_sky_along(30);
    view.update(GridId::MAIN, terrain, far_along, &far());
    assert_eq!(view.selections(), selections + 1);
    assert_eq!(view.wanted_centre(), Some(far_along.brick()));
    // Then a few bricks from there keep that cut.
    view.update(GridId::MAIN, terrain, at_the_sky_along(33), &far());
    assert_eq!(view.selections(), selections + 1);
}

#[test]
fn without_a_tolerance_a_view_chooses_its_cut_again_whenever_the_viewer_is_in_another_brick() {
    let trip = Trip::new();
    let terrain = trip.scene.world.main();
    let mut view = view_of(&trip.scene, 3);
    view.update(GridId::MAIN, terrain, at_the_sky_along(0), &far());
    let selections = view.selections();
    for bricks in 1..=4 {
        view.update(GridId::MAIN, terrain, at_the_sky_along(bricks), &far());
        assert_eq!(view.selections(), selections + bricks as u64);
        assert_eq!(view.wanted_centre(), Some(at_the_sky_along(bricks).brick()));
    }
}

#[test]
fn a_viewer_next_to_what_it_draws_gets_the_cut_of_the_brick_it_is_in_whatever_the_tolerance() {
    // On the ground the nearest thing drawn is the ground under the feet: a tenth of nothing is nothing.
    let trip = Trip::new();
    let terrain = trip.scene.world.main();
    let mut view = view_of(&trip.scene, 3);
    view.set_reselect_tolerance(0.1);
    view.update(GridId::MAIN, terrain, viewer_in(brick(0, 0, 0)), &far());
    let selections = view.selections();
    view.update(GridId::MAIN, terrain, viewer_in(brick(1, 0, 0)), &far());
    assert_eq!(view.selections(), selections + 1);
    assert_eq!(view.wanted_centre(), Some(brick(1, 0, 0)));
}

#[test]
fn a_view_that_keeps_its_cut_for_a_viewer_that_moves_settles_to_what_it_wants_and_draws_what_a_fresh_view_at_the_place_of_the_cut_draws()
 {
    let trip = Trip::new();
    let fresh = what_a_fresh_view_draws(&trip.scene, brick(0, 100, 0), &far());
    let mut view = view_of(&trip.scene, 3);
    view.set_reselect_tolerance(0.1);
    let mut screen = Screen::default();
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        at_the_sky_along(0),
        &far(),
        "the sky",
    );
    // The viewer goes a few bricks along: the picture is the one of the place the cut was chosen for, whole.
    settle_checked_in(
        &mut view,
        &mut screen,
        trip.scene.world.main(),
        at_the_sky_along(4),
        &far(),
        "a few bricks along",
    );
    // (The empty bricks of the fine ball are wanted and not drawn at the sky, with a tolerance or without; the nodes are what the picture is.)
    assert_eq!(view.wanted_centre(), Some(at_the_sky_along(0).brick()));
    assert_eq!(screen.nodes, fresh.nodes);
}

#[test]
fn a_change_of_what_is_asked_of_the_picture_or_of_the_terrain_chooses_the_cut_again_whatever_the_tolerance_and_a_view_with_no_node_to_measure_by_stays_exact()
 {
    let trip = Trip::new();
    let terrain = Dimmable {
        level: trip.scene.world.main(),
        dark: Cell::new(false),
        version: Cell::new(0),
    };
    let mut view = view_of(&trip.scene, 3);
    view.set_reselect_tolerance(0.1);
    view.update(GridId::MAIN, &terrain, at_the_sky_along(0), &far());
    let selections = view.selections();

    // A finer picture is asked for from the same place: the cut is chosen again.
    let finer = LodSettings {
        detail: DetailSettings {
            cell_pixels: 1.5,
            ..far().detail
        },
        ..far()
    };
    view.update(GridId::MAIN, &terrain, at_the_sky_along(1), &finer);
    assert_eq!(view.selections(), selections + 1, "another setting");
    // And it is chosen for the place the viewer is at then, and kept from there.
    assert_eq!(view.wanted_centre(), Some(at_the_sky_along(1).brick()));
    view.update(GridId::MAIN, &terrain, at_the_sky_along(2), &finer);
    assert_eq!(view.selections(), selections + 1);

    // The terrain says it is another one: chosen again though the viewer is where it was.
    terrain.version.set(1);
    view.update(GridId::MAIN, &terrain, at_the_sky_along(2), &finer);
    assert_eq!(
        view.selections(),
        selections + 2,
        "another version of the terrain"
    );
    // (For the brick the viewer is in now, and not the one it was in when the cut was last chosen, which it is within the share of.)
    assert_eq!(view.wanted_centre(), Some(at_the_sky_along(2).brick()));

    // Nothing is drawn within reach: there is nothing to measure the distance to, so a tolerance is no tolerance.
    let near = LodSettings {
        maximum_distance: 100.0,
        ..far()
    };
    let mut view = view_of(&trip.scene, 3);
    view.set_reselect_tolerance(0.1);
    view.update(GridId::MAIN, &terrain, at_the_sky_along(0), &near);
    let selections = view.selections();
    view.update(GridId::MAIN, &terrain, at_the_sky_along(1), &near);
    assert_eq!(view.selections(), selections + 1);
    assert_eq!(view.wanted_centre(), Some(at_the_sky_along(1).brick()));
}

#[test]
fn a_negative_or_unreal_tolerance_is_none() {
    let trip = Trip::new();
    let terrain = trip.scene.world.main();
    for bad in [-1.0, f64::NAN, f64::INFINITY, 0.0] {
        let mut view = view_of(&trip.scene, 3);
        view.set_reselect_tolerance(bad);
        view.update(GridId::MAIN, terrain, at_the_sky_along(0), &far());
        let selections = view.selections();
        view.update(GridId::MAIN, terrain, at_the_sky_along(1), &far());
        assert_eq!(view.selections(), selections + 1, "{bad}");
    }
}
