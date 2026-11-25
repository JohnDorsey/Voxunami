//! A view that cannot finish says so: a generator that panics on a worker thread reaches the caller as it would if the work were done in place, and a view that
//! is idle without drawing what is wanted says which cubes and why (`docs/specs/lod-spec.md`, 3.6a).

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::{Duration, Instant};

use coarse_cells::{CoarseNode, DetailSettings};
use region_octree::BrickCube;
use terrain::TerrainGenerator;
use voxel_grid::{Brick, GridLayout};
use voxel_space::{BrickCoordinate, GridId, WorldPosition};
use world_storage::LevelData;
use world_storage::test_support::brick;
use world_view::{
    CoarseTerrainSource, LodConfig, LodSettings, LodView, TerrainSource, WorldMesher,
};

mod common;

use common::{Scene, scene};

/// What a generator with a bug does, and where.
#[derive(Clone, Copy, Debug)]
enum Fault {
    /// Panics making this brick.
    Brick(BrickCoordinate),
    /// Panics making the node of any cube of two bricks or more that holds this brick.
    NodeHolding(BrickCoordinate),
}

#[derive(Debug)]
struct Faulty {
    inner: Arc<dyn TerrainGenerator>,
    fault: Fault,
}

impl TerrainGenerator for Faulty {
    fn generate_brick(&self, layout: &GridLayout, coordinate: BrickCoordinate) -> Brick {
        if matches!(self.fault, Fault::Brick(bad) if bad == coordinate) {
            panic!("a generator with a bug in a brick");
        }
        self.inner.generate_brick(layout, coordinate)
    }

    fn coarse_node(&self, cube: BrickCube, unlit: u8) -> Option<CoarseNode> {
        if matches!(self.fault, Fault::NodeHolding(bad) if cube.level() >= 1 && cube.contains(bad))
        {
            panic!("a generator with a bug in a node");
        }
        self.inner.coarse_node(cube, unlit)
    }

    fn may_show(&self, cube: BrickCube) -> bool {
        self.inner.may_show(cube)
    }
}

/// The terrain of a level, as the program has it, with a generator that has a bug in it, and bricks that no generator here can make.
struct Source<'a> {
    level: &'a LevelData,
    fault: Option<Fault>,
    /// Bricks that nothing in this program can make (a region whose generator is of a plugin that is not loaded).
    missing: Vec<BrickCoordinate>,
}

impl<'a> Source<'a> {
    fn new(scene: &'a Scene) -> Self {
        Self {
            level: scene.world.main(),
            fault: None,
            missing: Vec::new(),
        }
    }

    fn with_fault(self, fault: Fault) -> Self {
        Self {
            fault: Some(fault),
            ..self
        }
    }

    fn without(self, brick: BrickCoordinate) -> Self {
        Self {
            missing: vec![brick],
            ..self
        }
    }

    fn faulty(&self, inner: Arc<dyn TerrainGenerator>) -> Arc<dyn TerrainGenerator> {
        match self.fault {
            Some(fault) => Arc::new(Faulty { inner, fault }),
            None => inner,
        }
    }
}

impl TerrainSource for Source<'_> {
    fn known(&self, coordinate: BrickCoordinate) -> Option<Arc<Brick>> {
        self.level.known(coordinate)
    }

    fn generator_for(&self, coordinate: BrickCoordinate) -> Option<Arc<dyn TerrainGenerator>> {
        if self.missing.contains(&coordinate) {
            return None;
        }
        self.level
            .generator_for(coordinate)
            .map(|inner| self.faulty(inner))
    }

    fn layout(&self) -> &Arc<GridLayout> {
        self.level.layout()
    }
}

impl CoarseTerrainSource for Source<'_> {
    fn version(&self) -> u64 {
        0
    }

    fn may_show(&self, cube: BrickCube) -> bool {
        self.level.may_show(cube)
    }

    fn coarse_generator(&self, cube: BrickCube) -> Option<Arc<dyn TerrainGenerator>> {
        self.level
            .coarse_generator(cube)
            .map(|inner| self.faulty(inner))
    }
}

fn settings() -> LodSettings {
    LodSettings {
        detail: DetailSettings {
            cell_pixels: 3.0,
            pixel_size: 2.0 * 0.700_207_538_209_062_3 / 1080.0,
        },
        zoom: None,
        maximum_distance: 350.0,
        orthographic: None,
    }
}

fn viewer() -> WorldPosition {
    WorldPosition::new(brick(0, 0, 0), [16.0, 20.0, 16.0])
}

fn view_of(scene: &Scene) -> LodView {
    LodView::new(
        scene.mesher(3),
        LodConfig {
            unlit: 0,
            worker_count: 3,
        },
    )
}

/// What a panic said.
fn message_of(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with no message".to_string())
}

/// Updates the view until it panics, which it is to do within a while; what the panic said.
fn what_the_view_panics_with(scene: &Scene, source: &Source<'_>) -> String {
    let mut view = view_of(scene);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(30) {
            view.update(GridId::MAIN, source, viewer(), &settings());
            std::thread::sleep(Duration::from_millis(2));
        }
    }));
    match outcome {
        Err(payload) => message_of(&*payload),
        Ok(()) => panic!(
            "the view went on for 30 seconds as if nothing had happened: settled {}, {} jobs pending, {} cubes wanted and not drawn",
            view.is_settled(),
            view.pending_jobs(),
            view.wanted().difference(view.shown()).count(),
        ),
    }
}

#[test]
fn a_generator_that_panics_making_a_brick_panics_the_view_and_does_not_leave_it_waiting_for_ever() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 20);
    let source = Source::new(&scene).with_fault(Fault::Brick(brick(1, 0, 0)));
    assert_eq!(
        what_the_view_panics_with(&scene, &source),
        "a generator with a bug in a brick"
    );
}

#[test]
fn a_generator_that_panics_making_a_node_panics_the_view_and_does_not_leave_it_waiting_for_ever() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 20);
    // A brick out where the cubes are nodes.
    let source = Source::new(&scene).with_fault(Fault::NodeHolding(brick(8, 0, 0)));
    assert_eq!(
        what_the_view_panics_with(&scene, &source),
        "a generator with a bug in a node"
    );
}

#[test]
fn a_mesher_whose_generator_panics_panics_when_it_is_updated() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 20);
    let source = Source::new(&scene).with_fault(Fault::Brick(brick(1, 0, 0)));
    let mut mesher: WorldMesher = scene.mesher(3);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(30) {
            mesher.update(&source, brick(0, 0, 0));
            std::thread::sleep(Duration::from_millis(2));
        }
    }));
    let payload = outcome.expect_err("the panic of the worker reaches the one who updates");
    assert_eq!(message_of(&*payload), "a generator with a bug in a brick");
}

/// Updates until the view has nothing more to do, within a while.
fn settle(view: &mut LodView, source: &Source<'_>) {
    let started = Instant::now();
    loop {
        view.update(GridId::MAIN, source, viewer(), &settings());
        if view.is_settled() {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "does not settle"
        );
        std::thread::sleep(Duration::from_micros(300));
    }
}

#[test]
fn a_view_that_is_drawing_what_is_wanted_has_not_stalled() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 20);
    let source = Source::new(&scene);
    let mut view = view_of(&scene);
    settle(&mut view, &source);
    assert_eq!(view.shown(), view.wanted());
    assert!(view.stalled().is_empty());
    assert_eq!(view.take_stall_report(), None);
}

#[test]
fn a_view_that_is_idle_and_cannot_draw_what_is_wanted_says_which_cubes_and_why_once() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 20);
    // Nothing here can make the brick, so the bricks beside it have a neighbour that is never there to mesh them against.
    let source = Source::new(&scene).without(brick(1, 0, 0));
    let mut view = view_of(&scene);
    settle(&mut view, &source);
    assert_ne!(
        view.shown(),
        view.wanted(),
        "the bricks round the hole wait"
    );

    let stalled = view.stalled();
    assert!(!stalled.is_empty());
    for stall in stalled {
        assert!(view.wanted().contains(&stall.cube));
        assert!(!view.shown().contains(&stall.cube));
    }
    // The brick itself, which nothing makes, and its neighbours, which wait for it.
    let itself = stalled
        .iter()
        .find(|stall| stall.cube.origin() == brick(1, 0, 0) && stall.cube.level() == 0)
        .expect("the brick that nothing makes is reported");
    assert!(
        itself.reason.contains("nothing here can make"),
        "{}",
        itself.reason
    );
    let beside = stalled
        .iter()
        .find(|stall| stall.cube.origin() == brick(0, 0, 0) && stall.cube.level() == 0)
        .expect("the brick beside it is reported");
    assert!(
        beside.reason.contains("neighbour") && beside.reason.contains("is not loaded"),
        "{}",
        beside.reason
    );

    // It is said once, in words, and not again for the same cubes however often the view settles.
    let report = view.take_stall_report().expect("it is said");
    assert!(report.contains("not drawn"), "{report}");
    assert_eq!(view.take_stall_report(), None);
    settle(&mut view, &source);
    assert_eq!(view.take_stall_report(), None);
}

#[test]
fn work_in_progress_is_not_a_stall() {
    let mut scene = scene();
    scene.assign_around(brick(0, 0, 0), 20);
    let source = Source::new(&scene);
    let mut view = view_of(&scene);
    // The first update has only started: much is wanted and nothing is drawn, and that is waiting.
    view.update(GridId::MAIN, &source, viewer(), &settings());
    assert!(!view.is_settled());
    assert!(!view.wanted().is_empty());
    assert!(view.stalled().is_empty());
}
