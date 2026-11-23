//! Bricks and coarse nodes drawn together (`docs/specs/lod-spec.md`, 3.3 and 3.6): what a viewer sees of the terrain of a level, near as bricks and far as nodes.
//!
//! [`LodView`] decides, from where the viewer is, which cubes are to be drawn ([`coarse_cells::select_around_brick`]: bricks inside the viewer's
//! [`coarse_cells::FineRegion`], nodes of growing size outside), has them made (the bricks by the [`WorldMesher`], held back; the nodes on threads of its own,
//! from the terrain generators or from what the server has said differs, see [`LodView::set_override`]) and then swaps what is on the screen for what is
//! wanted in groups ([`coarse_cells::plan_swaps`]), so that a cube and what replaces it change in one frame and are never both on the screen or neither.

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;

use coarse_cells::{
    CoarseNode, DetailSettings, EditedCubes, Zoom, intermediate_cubes, plan_progressive_swaps,
    refinements,
};
use region_octree::BrickCube;
use terrain::{SolidBall, TerrainGenerator};
use voxel_meshing::{PackedBrickMesh, mesh_coarse_node};
use voxel_space::{BrickCoordinate, GridId, WorldPosition};
use world_storage::LevelData;

use crate::brick_states::HeldBricks;
use crate::cube_states::{Arrival, CubeStates, HeldPictures, Replacement, Taken};
use crate::job_feed::jobs_to_hold;
use crate::may_show_memo::MayShowMemo;
use crate::mesher::{MeshUpdate, TerrainSource, WorldMesher};
use crate::warm_cut::{KeptCut, Selection, WarmFinished, WarmJob, WarmWorker, warm_queue_of};

/// How much a picture is asked to show.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodSettings {
    pub detail: DetailSettings,
    pub zoom: Option<Zoom>,
    /// Nodes whose nearest point is farther than this many voxels are not drawn.
    pub maximum_distance: f64,
}

/// A place a view is to be ready to draw from: where the viewer is, in the grid's coordinates, and how much it is asked to show
/// ([`LodView::set_warm_viewpoint`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewpoint {
    pub viewer: WorldPosition,
    pub settings: LodSettings,
}

/// What a terrain says of whether a cube may show anything, as it said it when the answer was taken, in a form another thread can ask ([`CoarseTerrainSource::detached_may_show`]).
pub type MayShowOracle = Arc<dyn Fn(BrickCube) -> bool + Send + Sync>;

/// What the view needs to know of the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LodConfig {
    /// How bright terrain is that no light reaches, which is what a generator is told its cells are lit by.
    pub unlit: u8,
    /// How many threads make nodes.
    pub worker_count: usize,
}

/// What a view of levels of detail needs of the terrain it draws, besides its bricks ([`TerrainSource`]): which cubes may show anything, what makes the node of
/// one, and a version to forget answers by. The terrain of a level is one, and so is the terrain of a planet (a ship with terrain).
pub trait CoarseTerrainSource: TerrainSource {
    /// Changes whenever the answers of [`CoarseTerrainSource::may_show`] and [`CoarseTerrainSource::coarse_generator`] could.
    fn version(&self) -> u64;

    /// What [`CoarseTerrainSource::may_show`] says now, for a thread of its own to ask while the terrain goes on (it is a copy: what the terrain says later is not in it). The
    /// view that is to keep a place warm works out its cubes on such a thread, and on the thread of the update, as the cubes of the place that is drawn are, if the terrain
    /// has no such copy (`None`, the default).
    fn detached_may_show(&self) -> Option<MayShowOracle> {
        None
    }

    /// Whether drawing the cube could show anything (see [`TerrainGenerator::may_show`]).
    fn may_show(&self, cube: BrickCube) -> bool;

    /// What makes the node of the cube, if this program has it.
    fn coarse_generator(&self, cube: BrickCube) -> Option<Arc<dyn TerrainGenerator>>;

    /// A ball of the terrain that is ground all through, if it has one (a planet's core): a viewer outside it is not shown what is behind it. None, the default, for
    /// the terrain of a level.
    fn solid_core(&self) -> Option<SolidBall> {
        None
    }

    /// Whether the nodes the server sends are all there is of this terrain: no generator makes the rest, so a cube that has no node is empty only if the server has said
    /// so ([`LodView::set_override`] with `None`), and until it has the view waits to draw the cube (and what is to be drawn with it), and goes on drawing what it draws.
    /// False, the default: a cube with no node is made by the generator, or is empty if there is none.
    fn waits_for_nodes(&self) -> bool {
        false
    }
}

impl CoarseTerrainSource for LevelData {
    fn version(&self) -> u64 {
        self.terrain.version()
    }

    fn detached_may_show(&self) -> Option<MayShowOracle> {
        let terrain = self.terrain.clone();
        Some(Arc::new(move |cube| terrain.may_show(cube)))
    }

    fn may_show(&self, cube: BrickCube) -> bool {
        self.terrain.may_show(cube)
    }

    fn coarse_generator(&self, cube: BrickCube) -> Option<Arc<dyn TerrainGenerator>> {
        self.terrain.coarse_generator(cube)
    }
}

/// The fewest updates between two times the cubes are worked out again because the server has told of an edit somewhere new (the cubes round an edit are not
/// left out as empty or buried, so a new place can bring cubes in).
const RESELECT_MINIMUM_UPDATES: u32 = 10;

/// The fewest updates between the starts of two workings out of the cubes of the warm viewpoint when it moves (counted, never timed): about a second at 60 updates a second. A
/// working out asks about a good many cubes, and a place that moves all the time would keep a thread busy for ever.
const WARM_RESELECT_MINIMUM_UPDATES: u32 = 60;

/// How many cubes that are to come may be put on the screen in one update: each is a buffer made on the GPU.
const SHOWN_PER_UPDATE: usize = 64;

struct Override {
    node: Arc<CoarseNode>,
    /// Counts up as overrides are given, so that a node made from an older one is known for stale.
    version: u64,
}

enum Source {
    Override(Arc<CoarseNode>),
    Generator(Arc<dyn TerrainGenerator>),
    /// There is nothing to make it from: a region the program has no generator for.
    Nothing,
}

struct Job {
    epoch: u64,
    cube: BrickCube,
    version: u64,
    source: Source,
}

struct Done {
    epoch: u64,
    cube: BrickCube,
    version: u64,
    mesh: PackedBrickMesh,
}

/// What a thread that makes nodes says of a job.
enum Finished {
    Done(Done),
    /// The job panicked, with this payload.
    Panicked(Box<dyn Any + Send>),
}

/// A cube that is wanted and is not drawn, in a view that has nothing more to do: see [`LodView::stalled`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stalled {
    pub cube: BrickCube,
    /// Why nothing is being done to draw it, in words.
    pub reason: String,
}

/// How many stalled cubes a report names.
const STALL_REPORT_CUBES: usize = 5;

pub struct LodView {
    mesher: WorldMesher,
    grid: GridId,
    /// What the server has said differs from the generators', by grid and cube.
    overrides: HashMap<(GridId, BrickCube), Override>,
    /// The cubes the server has said are empty (a `None` for a cube that had no node), and the biggest level of them: a cube inside one of these is empty too.
    /// Only a terrain that [`waits_for_nodes`](CoarseTerrainSource::waits_for_nodes) asks.
    told_empty: HashSet<(GridId, BrickCube)>,
    told_empty_top: u8,
    /// The cubes that are wanted, have no node and have not been said to be empty, in a terrain that waits for nodes: they are made when the server says.
    waiting_for_the_server: HashSet<BrickCube>,
    next_version: u64,
    /// Where the overrides are, for leaving out only the cubes that hold no edit and have none beside them.
    edited: EditedCubes,
    /// Whether an override has come for a place that had none, since the cubes were last worked out; and how many updates ago that was.
    new_edits: bool,
    updates_since_selection: u32,
    /// What the terrain said of the cubes at the last working out.
    may_show: MayShowMemo,
    /// What is to be drawn and what is drawn, bricks (cubes of level 0) and nodes, and the records of the nodes (see [`CubeStates`]); and what the wanted cubes were
    /// worked out for.
    cubes: CubeStates,
    wanted_for: Option<(BrickCoordinate, LodSettings)>,
    /// How many times the edits have changed what the cubes are worked out from ([`LodView::set_override`]), and the number they were last worked out at.
    edit_generation: u64,
    selected_edits: u64,
    /// The place kept warm, as it was asked for ([`LodView::set_warm_viewpoint`]), the cubes it was worked out as, and the nodes of them still to make (the first made last).
    warm_wanted: Option<Viewpoint>,
    warm_cut: Option<KeptCut>,
    warm_queue: Vec<BrickCube>,
    updates_since_warm_selection: u32,
    /// The thread that works out the cubes of the warm viewpoint (made when there is first something for it to do), whether it has been handed a place and not given the
    /// cubes back, which of the lives of the view its work is for (what comes of another is thrown away), and what it has learnt of the terrain (with it while it works).
    warm_worker: Option<WarmWorker>,
    release_warm_pictures: bool,
    warm_in_flight: bool,
    warm_epoch: u64,
    warm_memo: Option<MayShowMemo>,
    /// How many times the cubes have been worked out ([`LodView::selections`]).
    selections: u64,
    /// The version of the terrain ([`CoarseTerrainSource::version`]) the cubes were last worked out for: when it is another, the answers that the cubes were chosen by are
    /// not the terrain's any more (a far ship's box changed), and they are worked out again though the viewer has not moved.
    selected_version: Option<u64>,
    /// Nodes to be made, the most wanted last.
    queue: Vec<BrickCube>,
    plan_needed: bool,
    seen_ready_serial: u64,
    epoch: u64,
    in_flight: usize,
    /// How many nodes the threads finished since the update before (what [`jobs_to_hold`] sizes what is handed to them by).
    finished_last_update: usize,
    jobs: Option<mpsc::Sender<Job>>,
    results: mpsc::Receiver<Finished>,
    workers: Vec<JoinHandle<()>>,
    /// Whether the view had nothing more to do at the end of the last update, so that it is looked at once when it comes to rest and not at every update.
    was_settled: bool,
    /// The cubes that are wanted and not drawn, as of the last time the view came to rest.
    stalls: Vec<Stalled>,
    /// What the report of a stall said it was about, so that the same stall is not reported again.
    stall_reported: HashSet<BrickCube>,
    stall_report: Option<String>,
}

impl std::fmt::Debug for LodView {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LodView")
            .field("wanted", &self.cubes.wanted().len())
            .field("shown", &self.cubes.shown().len())
            .field("in_flight", &self.in_flight)
            .finish_non_exhaustive()
    }
}

impl LodView {
    /// A view whose bricks are made by `mesher`, which is held back from now on.
    pub fn new(mesher: WorldMesher, config: LodConfig) -> Self {
        let mesher = mesher.with_held_back_display();
        let (job_sender, job_receiver) = mpsc::channel::<Job>();
        let (result_sender, results) = mpsc::channel::<Finished>();
        let job_receiver = Arc::new(Mutex::new(job_receiver));
        let workers = (0..config.worker_count.max(1))
            .map(|_| {
                let job_receiver = Arc::clone(&job_receiver);
                let result_sender = result_sender.clone();
                std::thread::spawn(move || {
                    loop {
                        // Hold the lock only while waiting for the next job, not while running it.
                        let job = job_receiver.lock().expect("job queue lock").recv();
                        let Ok(job) = job else { return };
                        // A generator with a bug panics here, on a thread nobody is watching: it is caught, and said to the view, which panics with it. (A thread that dies with
                        // its job leaves the job in flight for ever and the view waiting for it, with nothing to say why.)
                        let finished = catch_unwind(AssertUnwindSafe(|| {
                            let node = match job.source {
                                Source::Override(node) => Some(node),
                                Source::Generator(generator) => {
                                    generator.coarse_node(job.cube, config.unlit).map(Arc::new)
                                }
                                Source::Nothing => None,
                            };
                            let mesh = node
                                .map(|node| mesh_coarse_node(&node, &[None; 6]).pack())
                                .unwrap_or_default();
                            Done {
                                epoch: job.epoch,
                                cube: job.cube,
                                version: job.version,
                                mesh,
                            }
                        }))
                        .map_or_else(Finished::Panicked, Finished::Done);
                        if result_sender.send(finished).is_err() {
                            return;
                        }
                    }
                })
            })
            .collect();
        Self {
            mesher,
            grid: GridId::MAIN,
            overrides: HashMap::new(),
            told_empty: HashSet::new(),
            told_empty_top: 0,
            waiting_for_the_server: HashSet::new(),
            next_version: 0,
            edited: EditedCubes::new(),
            new_edits: false,
            updates_since_selection: 0,
            may_show: MayShowMemo::default(),
            cubes: CubeStates::new(),
            wanted_for: None,
            edit_generation: 0,
            selected_edits: 0,
            warm_wanted: None,
            warm_cut: None,
            warm_queue: Vec::new(),
            updates_since_warm_selection: 0,
            warm_worker: None,
            release_warm_pictures: true,
            warm_in_flight: false,
            warm_epoch: 0,
            warm_memo: None,
            selections: 0,
            selected_version: None,
            queue: Vec::new(),
            plan_needed: false,
            seen_ready_serial: 0,
            epoch: 0,
            in_flight: 0,
            finished_last_update: 0,
            jobs: Some(job_sender),
            results,
            workers,
            was_settled: false,
            stalls: Vec::new(),
            stall_reported: HashSet::new(),
            stall_report: None,
        }
    }

    /// The mesher of the bricks, for what is asked of the bricks themselves (which voxel is solid, what changed).
    pub fn mesher(&self) -> &WorldMesher {
        &self.mesher
    }

    pub fn mesher_mut(&mut self) -> &mut WorldMesher {
        &mut self.mesher
    }

    /// The cubes that are drawn now: bricks, including those with nothing to see, and nodes. They do not overlap.
    pub fn shown(&self) -> &HashSet<BrickCube> {
        self.cubes.shown()
    }

    /// The cubes that are to be drawn, as far as the last update worked it out.
    pub fn wanted(&self) -> &HashSet<BrickCube> {
        self.cubes.wanted()
    }

    /// Work in progress: bricks and nodes being made.
    pub fn pending_jobs(&self) -> usize {
        self.mesher.pending_jobs() + self.in_flight
    }

    /// How many coarse nodes the threads have been handed and the view has not collected yet (the part of [`LodView::pending_jobs`] that is not bricks).
    pub fn nodes_being_made(&self) -> usize {
        self.in_flight
    }

    /// Keeps the pictures of nodes that leave the screen, up to `budget_bytes` of them, instead of dropping them: a node that is taken down is **hidden** in the renderer
    /// ([`MeshUpdate::HideCoarse`]) and its record kept, and when it is wanted again it is shown by a move ([`MeshUpdate::ShowCoarse`]) and not made again. The
    /// oldest are let go of (a [`MeshUpdate::RemoveCoarse`]) when more is held than the budget, never one that is wanted. 0, the default, keeps nothing. The bricks are
    /// held the same way, the mesher's ([`WorldMesher::hold_hidden_pictures`]), under a budget of the same size of their own.
    pub fn hold_hidden_pictures(&mut self, budget_bytes: usize) {
        self.cubes.set_budget(budget_bytes);
        self.mesher.hold_hidden_pictures(budget_bytes);
    }

    /// What is held now of the nodes ([`LodView::hold_hidden_pictures`]).
    pub fn held_pictures(&self) -> HeldPictures {
        self.cubes.held_pictures()
    }

    /// What is held now of the bricks ([`LodView::hold_hidden_pictures`]).
    pub fn held_brick_pictures(&self) -> HeldBricks {
        self.mesher.held_pictures()
    }

    /// Whether anything is waiting to be made or swapped: the view is not yet what it is to be.
    pub fn is_settled(&self) -> bool {
        self.mesher.is_settled()
            && self.in_flight == 0
            && self.queue.is_empty()
            && self.warm_queue.is_empty()
            && !self.warm_in_flight
            && self.warm_is_worked_out()
            && !self.plan_needed
            && !self.new_edits
    }

    /// Keeps a second place ready: the cubes it would draw are worked out and their nodes made, after those of the place that is drawn, and each picture is held hidden in the
    /// renderer ([`MeshUpdate::UploadHiddenCoarse`]) and not let go of by the budget, so that drawing from there is a move of pictures and nothing is made
    /// ([`LodView::hold_hidden_pictures`] first: with no budget nothing is held, and the place is not kept). Worked out when it is set and, when it moves, at most about once a
    /// second (60 updates); a place that is the one that was drawn last, or is the one that is drawn next, is not worked out again. The working out is on a thread of its own
    /// when the terrain can give it what it says ([`CoarseTerrainSource::detached_may_show`]), and the cubes are used when it is done, and for the place it was started for
    /// (which may be a little behind a place that moves). `None`, the default, keeps nothing warm and lets the pictures that were kept for it go to the budget.
    pub fn set_warm_viewpoint(&mut self, viewpoint: Option<Viewpoint>) {
        self.warm_wanted = viewpoint;
    }

    /// Whether the cubes of the warm viewpoint are being worked out on the thread that does it, and are not given back yet.
    pub fn warm_selection_pending(&self) -> bool {
        self.warm_in_flight
    }

    /// Whether the pictures that were made for the warm viewpoint alone (held hidden, never shown) are let go of when it is taken away ([`LodView::set_warm_viewpoint`] with
    /// `None`), at that update; the pictures that were held because they were shown and taken down are the view's own, and stay either way. True, the default. False leaves
    /// them to the budget, the oldest going first, as any held picture.
    pub fn set_release_warm_pictures(&mut self, release: bool) {
        self.release_warm_pictures = release;
    }

    /// How many times the cubes of a place have been worked out from what the terrain says may show (the work that a place kept ready, or the one that was just left, saves
    /// when the view goes there): a count for the tests and for the curious.
    pub fn selections(&self) -> u64 {
        self.selections
    }

    /// Whether the warm viewpoint, if there is one, has had its cubes worked out for the place it is at.
    fn warm_is_worked_out(&self) -> bool {
        match (&self.warm_wanted, &self.warm_cut) {
            (Some(asked), cut) if self.cubes.holds_pictures() => cut.as_ref().is_some_and(|cut| {
                cut.centre == asked.viewer.brick() && cut.settings == asked.settings
            }),
            _ => true,
        }
    }

    /// The edits have changed what the cubes are worked out from.
    fn edits_changed(&mut self) {
        self.new_edits = true;
        self.edit_generation += 1;
    }

    /// The node the server says is the node of a cube instead of what the generators give (a `None` takes back what was said). Bricks have none, and
    /// what is said of one is ignored. For a terrain that waits for nodes ([`CoarseTerrainSource::waits_for_nodes`]) a `None` is the server saying the cube is
    /// empty, as is everything inside it.
    pub fn set_override(&mut self, grid: GridId, cube: BrickCube, node: Option<CoarseNode>) {
        if cube.level() == 0 {
            return;
        }
        match node {
            Some(node) => {
                self.told_empty.remove(&(grid, cube));
                self.next_version += 1;
                let replaced = self.overrides.insert(
                    (grid, cube),
                    Override {
                        node: Arc::new(node),
                        version: self.next_version,
                    },
                );
                if replaced.is_none() && self.edited.add(grid, cube) {
                    self.edits_changed();
                }
            }
            None => {
                if self.overrides.remove(&(grid, cube)).is_some() && self.edited.remove(grid, cube)
                {
                    // Cubes that were kept for being beside the edit may be left out again.
                    self.edits_changed();
                }
                self.told_empty.insert((grid, cube));
                self.told_empty_top = self.told_empty_top.max(cube.level());
            }
        }
        if grid == self.grid && self.cubes.has_node(cube) {
            self.queue.push(cube);
        }
        // What waited for the server to say what is in it, and is the cube or inside it, is made now.
        if grid == self.grid {
            let now_known: Vec<BrickCube> = self
                .waiting_for_the_server
                .iter()
                .copied()
                .filter(|waiting| {
                    waiting.level() <= cube.level()
                        && BrickCube::containing(waiting.origin(), cube.level()) == cube
                })
                .collect();
            for waiting in now_known {
                self.waiting_for_the_server.remove(&waiting);
                self.queue.push(waiting);
            }
        }
    }

    /// Whether the server has said that the cube, or a cube that holds it, is empty.
    fn is_told_empty(&self, cube: BrickCube) -> bool {
        (cube.level()..=self.told_empty_top).any(|level| {
            self.told_empty
                .contains(&(self.grid, BrickCube::containing(cube.origin(), level)))
        })
    }

    /// The version of the data a node is made from: that of its override, or 0 if the generators make it.
    fn data_version(&self, cube: BrickCube) -> u64 {
        self.overrides
            .get(&(self.grid, cube))
            .map_or(0, |held| held.version)
    }

    /// Forgets what is wanted and shown, as when the viewer moves to another level. Returns the removal of everything on the screen.
    pub fn reset(&mut self) -> Vec<MeshUpdate> {
        let mut updates = self.mesher.reset();
        let mut uploaded = self.cubes.uploaded_nodes();
        uploaded.sort_by_key(|cube| {
            let origin = cube.origin();
            (cube.level(), origin.x(), origin.y(), origin.z())
        });
        updates.extend(uploaded.into_iter().map(MeshUpdate::RemoveCoarse));
        self.cubes.clear();
        self.wanted_for = None;
        self.warm_cut = None;
        self.warm_queue.clear();
        self.forget_the_warm_selection();
        self.may_show.clear();
        self.queue.clear();
        self.waiting_for_the_server.clear();
        self.plan_needed = false;
        self.epoch += 1;
        self.in_flight = 0;
        self.finished_last_update = 0;
        self.was_settled = false;
        self.stalls.clear();
        self.stall_reported.clear();
        self.stall_report = None;
        updates
    }

    /// Collects what the threads have finished, works out what is to be drawn from where the viewer is, and puts on the screen what can be. Returns what
    /// the renderer must change. `grid` is the grid whose terrain is drawn (a level's root grid, or a planet) and `source` its terrain; the viewer is `viewer`,
    /// in the grid's coordinates.
    pub fn update(
        &mut self,
        grid: GridId,
        source: &impl CoarseTerrainSource,
        viewer: WorldPosition,
        settings: &LodSettings,
    ) -> Vec<MeshUpdate> {
        self.grid = grid;
        let centre = viewer.brick();
        // The bricks round the warm viewpoint are made after those round the viewer, whenever it is somewhere else.
        let warm_centre = self
            .warm_wanted
            .filter(|_| self.cubes.holds_pictures())
            .map(|warm| warm.viewer.brick());
        self.mesher.set_warm_centre(warm_centre);
        let mut updates = self.mesher.update(source, centre);
        self.collect_nodes(&mut updates);
        self.updates_since_selection = self.updates_since_selection.saturating_add(1);
        self.updates_since_warm_selection = self.updates_since_warm_selection.saturating_add(1);
        let moved = self.wanted_for != Some((centre, *settings))
            || self.selected_version != Some(source.version());
        let new_edits = self.new_edits && self.updates_since_selection >= RESELECT_MINIMUM_UPDATES;
        let mut left = None;
        if moved || new_edits {
            left = self.reselect(centre, viewer, settings, source);
        }
        self.update_warm(source, left, &mut updates);
        updates.extend(self.mesher.park_ready());
        self.start_jobs(source, &mut updates);
        let serial = self.mesher.ready_serial();
        if self.plan_needed || serial != self.seen_ready_serial {
            self.seen_ready_serial = serial;
            self.swap(&mut updates);
        }
        updates.extend(
            self.cubes
                .evict_over_budget()
                .into_iter()
                .map(MeshUpdate::RemoveCoarse),
        );
        // (A brick that is wanted is not let go of: it is about to be shown.)
        let cubes = &self.cubes;
        updates.extend(self.mesher.evict_held(|coordinate| {
            BrickCube::new(coordinate, 0).is_ok_and(|cube| cubes.is_to_be_made(&cube))
        }));
        // Looked at once, when the view comes to rest (not at every update: it compares the cubes that are drawn with the ones that are wanted, and they are many).
        let settled = self.is_settled();
        if !settled {
            self.stalls.clear();
        } else if !self.was_settled {
            self.audit(source);
        }
        self.was_settled = settled;
        updates
    }

    /// The cubes that are wanted and are not drawn, with nothing being done to draw them: what is left when the view has nothing more to do ([`LodView::is_settled`])
    /// and is not drawing what it wants. Empty while work is under way (waiting for a thread is not stalling), and empty for a view that has drawn everything.
    ///
    /// A view that settles with something in this list is stuck for good: it will not change until the viewer moves or the world does. Which cube and why are the first
    /// thing to know about it. As of the last time the view came to rest.
    pub fn stalled(&self) -> &[Stalled] {
        &self.stalls
    }

    /// What to say about the stall of the view, once: a sentence naming a few of the cubes and why they are not drawn, the first time the view comes to rest with
    /// these cubes stalled. `None` when there is nothing new to say.
    pub fn take_stall_report(&mut self) -> Option<String> {
        self.stall_report.take()
    }

    /// Checks what the view's records of its cubes cannot make impossible by their shape, and says what is wrong if it is not so (the first thing found), the
    /// mesher's own records included. Cheap enough for every update of a test.
    pub fn check_consistency(&self) -> Result<(), String> {
        self.mesher.check_consistency()?;
        self.cubes.check_consistency()?;
        // The bricks that are shown are the mesher's to hold the pictures of, and the view's to say are shown: the two must say the same.
        let shown_bricks = self.cubes.shown().iter().filter(|cube| cube.level() == 0);
        for cube in shown_bricks {
            if !self.mesher.is_shown(cube.origin()) {
                return Err(format!(
                    "{cube:?} is shown, and the mesher does not have it shown"
                ));
            }
        }
        for coordinate in self.mesher.shown_coordinates() {
            let cube = BrickCube::new(coordinate, 0).expect("a brick is a cube");
            if !self.cubes.shown().contains(&cube) {
                return Err(format!(
                    "the mesher has {coordinate:?} shown, and the view does not"
                ));
            }
        }
        let requested = self.cubes.requested_count();
        if requested > self.in_flight {
            return Err(format!(
                "{requested} pictures have been asked for and {} jobs are in flight",
                self.in_flight
            ));
        }
        Ok(())
    }

    /// Finds what is wanted and not drawn that nothing is going to draw, as the view comes to rest, and says it once.
    fn audit(&mut self, source: &impl TerrainSource) {
        let mut waiting: Vec<BrickCube> = self
            .cubes
            .wanted()
            .difference(self.cubes.shown())
            .copied()
            // (A cube that is ready waits for the one beside it that is not, and that one is the one to name.)
            .filter(|&cube| !self.is_ready(cube))
            .collect();
        waiting.sort_by_key(|cube| {
            let origin = cube.origin();
            (cube.level(), origin.x(), origin.y(), origin.z())
        });
        self.stalls = waiting
            .into_iter()
            .map(|cube| Stalled {
                cube,
                reason: self.why_not_ready(source, cube),
            })
            .collect();
        // (A cube that waits for the server to say what is in it is not stuck for good, and is not said: the stall of a view is a defect of the program's.)
        let stalled: HashSet<BrickCube> = self
            .stalls
            .iter()
            .map(|stall| stall.cube)
            .filter(|cube| !self.waiting_for_the_server.contains(cube))
            .collect();
        if stalled.is_empty() {
            self.stall_reported.clear();
        } else if stalled != self.stall_reported {
            let first: Vec<String> = self
                .stalls
                .iter()
                .filter(|stall| stalled.contains(&stall.cube))
                .take(STALL_REPORT_CUBES)
                .map(|stall| format!("{} ({})", describe(stall.cube), stall.reason))
                .collect();
            self.stall_report = Some(format!(
                "the view of the terrain has nothing more to do, and {} cubes that are wanted are not drawn, first {}: {}",
                stalled.len(),
                first.len(),
                first.join("; ")
            ));
            self.stall_reported = stalled;
        }
    }

    /// Why the picture of a cube that is wanted is not ready, nor on its way.
    fn why_not_ready(&self, source: &impl TerrainSource, cube: BrickCube) -> String {
        if cube.level() == 0 {
            return self
                .mesher
                .why_not_ready(source, cube.origin())
                .unwrap_or_else(|| "it is ready".to_string());
        }
        if self.waiting_for_the_server.contains(&cube) {
            return "the server has not said what is in it".to_string();
        }
        match self.cubes.node(cube) {
            None => "nothing asked for its picture".to_string(),
            Some(node) if node.is_requested() => {
                "its picture was asked for and never came".to_string()
            }
            Some(node) if node.has_waiting_picture() => {
                "its picture is of data that has changed since".to_string()
            }
            Some(_) => "nothing is making its picture".to_string(),
        }
    }

    /// Updates until nothing is left to make or swap, returning every change. For headless rendering and tests.
    pub fn settle(
        &mut self,
        grid: GridId,
        source: &impl CoarseTerrainSource,
        viewer: WorldPosition,
        settings: &LodSettings,
    ) -> Vec<MeshUpdate> {
        let mut updates = Vec::new();
        loop {
            updates.extend(self.update(grid, source, viewer, settings));
            if self.is_settled() {
                return updates;
            }
            std::thread::sleep(std::time::Duration::from_micros(200));
        }
    }

    fn collect_nodes(&mut self, updates: &mut Vec<MeshUpdate>) {
        let mut finished_now = 0;
        while let Ok(finished) = self.results.try_recv() {
            // As if the work had been done here: a bug in a generator is a panic of whoever asked for the node.
            let done = match finished {
                Finished::Done(done) => done,
                Finished::Panicked(payload) => resume_unwind(payload),
            };
            if done.epoch != self.epoch {
                continue;
            }
            self.in_flight = self.in_flight.saturating_sub(1);
            finished_now += 1;
            let current = self.data_version(done.cube);
            let cube = done.cube;
            match self
                .cubes
                .finish_making(cube, done.version, current, done.mesh)
            {
                Arrival::Ignored | Arrival::Replaced(Replacement::Unchanged) => {}
                // (A picture that is for the warm viewpoint only is held, hidden; one that is to be drawn waits for the planner.)
                Arrival::Waiting => match self.cubes.park(cube) {
                    Some(mesh) if !mesh.is_empty() => {
                        updates.push(MeshUpdate::UploadHiddenCoarse { cube, mesh });
                    }
                    Some(_) => {}
                    None => self.plan_needed = true,
                },
                Arrival::Replaced(Replacement::Upload(mesh)) => {
                    updates.push(MeshUpdate::UploadCoarse { cube, mesh });
                }
                Arrival::Replaced(Replacement::Remove) => {
                    updates.push(MeshUpdate::RemoveCoarse(cube));
                }
            }
        }
        self.finished_last_update = finished_now;
    }

    fn reselect(
        &mut self,
        centre: BrickCoordinate,
        viewer: WorldPosition,
        settings: &LodSettings,
        source: &impl CoarseTerrainSource,
    ) -> Option<KeptCut> {
        // The cubes that were drawn before, for the warm viewpoint to be worked out from if it is the place that was left.
        let left = (self.warm_wanted.is_some() && self.cubes.holds_pictures())
            .then(|| {
                let ((centre, settings), version) = self.wanted_for.zip(self.selected_version)?;
                Some(KeptCut {
                    centre,
                    settings,
                    version,
                    edits: self.selected_edits,
                    cubes: self.cubes.wanted().iter().copied().collect(),
                })
            })
            .flatten();
        // The cubes of the warm viewpoint are these if that is where the viewer is now (and nothing has changed since): nothing is asked of the terrain.
        let version = source.version();
        let kept = self
            .warm_cut
            .take_if(|cut| cut.is_for(centre, settings, version, self.edit_generation));
        let cubes = match kept {
            Some(kept) => kept.cubes,
            None => self.select_cut(centre, viewer, settings, source),
        };
        self.selected_edits = self.edit_generation;
        self.new_edits = false;
        self.updates_since_selection = 0;
        // What is shown is replaced a level at a time where what replaces it is much finer (a jump from far away): the cubes between are made too, first.
        // (A terrain that waits for the server's nodes has no node to make them from.)
        let wanted_set: HashSet<BrickCube> = cubes.iter().copied().collect();
        let (steps, hurried) = if source.waits_for_nodes() {
            (Vec::new(), HashSet::new())
        } else {
            let groups: Vec<_> = refinements(self.cubes.shown(), &wanted_set)
                .into_iter()
                .filter(|group| {
                    group
                        .coming
                        .iter()
                        .any(|cube| cube.level() + 1 < group.going.level())
                        // (What is to come is shown at once if it is all ready, held pictures among it: no step is made on the way.)
                        && !group.coming.iter().all(|cube| self.is_ready(*cube))
                })
                .collect();
            let hurried: HashSet<BrickCube> = groups
                .iter()
                .flat_map(|group| group.coming.iter().copied())
                .collect();
            (
                intermediate_cubes(&groups, |cube| self.is_ready(cube)),
                hurried,
            )
        };
        self.cubes.want_with_steps(&cubes, &steps);
        self.wanted_for = Some((centre, *settings));
        self.selected_version = Some(source.version());
        // The nearest are made first: they are the last in the queue.
        // (What was waited for is waited for again, by the cubes that are still wanted.)
        self.waiting_for_the_server.clear();
        self.queue = cubes
            .into_iter()
            .filter(|cube| cube.level() > 0 && !hurried.contains(cube))
            .collect();
        self.queue
            .sort_by_key(|cube| std::cmp::Reverse(squared_distance_to_middle(*cube, centre)));
        // The cubes of a jump are made coarsest first, the nearest first among those, and before the rest: the last in the queue are made first.
        let mut jump: Vec<BrickCube> = steps
            .into_iter()
            .chain(hurried.into_iter().filter(|cube| cube.level() > 0))
            .collect();
        jump.sort_by_key(|cube| {
            (
                cube.level(),
                std::cmp::Reverse(squared_distance_to_middle(*cube, centre)),
            )
        });
        self.queue.extend(jump);
        self.plan_needed = true;
        left
    }

    /// The cubes a view at `viewer` draws, worked out from what the terrain says may show.
    fn select_cut(
        &mut self,
        centre: BrickCoordinate,
        viewer: WorldPosition,
        settings: &LodSettings,
        source: &impl CoarseTerrainSource,
    ) -> Vec<BrickCube> {
        self.selections += 1;
        Selection {
            centre,
            viewer,
            settings,
            version: source.version(),
            grid: self.grid,
            edited: &self.edited,
            core: source.solid_core(),
            // (A terrain that has only the nodes the server sends has cubes beside a cube with a node that nobody says anything of: the server tells of the cubes of the box
            // of the ship and no more.)
            draws_beside_edits: !source.waits_for_nodes(),
            fine_radius: u32::try_from(self.mesher.view_radius_bricks()).unwrap_or(0),
        }
        .select(&mut self.may_show, &|cube| source.may_show(cube))
    }

    /// Keeps the warm viewpoint's cubes worked out, pinned and queued ([`LodView::set_warm_viewpoint`]). `left` is what was drawn before this update if that changed.
    fn update_warm(
        &mut self,
        source: &impl CoarseTerrainSource,
        left: Option<KeptCut>,
        updates: &mut Vec<MeshUpdate>,
    ) {
        let Some(asked) = self.warm_wanted.filter(|_| self.cubes.holds_pictures()) else {
            self.forget_the_warm_selection();
            self.warm_cut = None;
            // (The cubes are pinned though the cut they were pinned for is gone, if the viewer went to the place and the drawn cut took it.)
            if self.cubes.has_pins() {
                self.cubes.pin(&[]);
                self.warm_queue.clear();
                if self.release_warm_pictures {
                    self.release_what_was_made_for_the_warm_viewpoint(updates);
                }
            }
            return;
        };
        let centre = asked.viewer.brick();
        let (version, edits) = (source.version(), self.edit_generation);
        // (What the thread has finished is taken in either way: it is the thread's to be asked again.)
        let arrived = self.collect_warm_cut();
        if self
            .warm_cut
            .as_ref()
            .is_some_and(|cut| cut.is_for(centre, &asked.settings, version, edits))
        {
            return;
        }
        let kept = if let Some(left) =
            left.filter(|cut| cut.is_for(centre, &asked.settings, version, edits))
        {
            // The place that was drawn is the one kept warm now.
            Some(left)
        } else if self.wanted_for == Some((centre, asked.settings))
            && self.selected_version == Some(version)
            && self.selected_edits == edits
        {
            // The place that is drawn is the one kept warm too.
            Some(KeptCut {
                centre,
                settings: asked.settings,
                version,
                edits,
                cubes: self.cubes.wanted().iter().copied().collect(),
            })
        } else {
            None
        };
        if let Some(cut) = kept {
            self.updates_since_warm_selection = 0;
            let queue = warm_queue_of(&cut.cubes, cut.centre);
            self.keep_warm(cut, queue);
            return;
        }
        // The cubes the thread worked out, if they are of the terrain and the edits as they are now (the place may have moved on since it was handed over: a place kept warm is a
        // little behind where it is asked to be).
        if let Some((cut, queue)) = arrived.filter(|(cut, _)| {
            cut.settings == asked.settings && cut.version == version && cut.edits == edits
        }) {
            let is_where_it_is_asked_to_be = cut.centre == centre;
            self.keep_warm(cut, queue);
            if is_where_it_is_asked_to_be {
                return;
            }
        }
        if !self.warm_in_flight
            && (self.warm_cut.is_none()
                || self.updates_since_warm_selection >= WARM_RESELECT_MINIMUM_UPDATES)
        {
            self.updates_since_warm_selection = 0;
            self.work_out_warm_cut(source, &asked, version, edits);
        }
    }

    /// Lets go of the pictures that were made for the warm viewpoint alone and held hidden, and that nothing wants now (what the viewer draws, or is about to, stays).
    fn release_what_was_made_for_the_warm_viewpoint(&mut self, updates: &mut Vec<MeshUpdate>) {
        updates.extend(
            self.cubes
                .release_parked()
                .into_iter()
                .map(MeshUpdate::RemoveCoarse),
        );
        let cubes = &self.cubes;
        updates.extend(self.mesher.release_parked(|coordinate| {
            BrickCube::new(coordinate, 0).is_ok_and(|cube| cubes.is_to_be_made(&cube))
        }));
    }

    /// Pins the cubes of a cut, and queues the nodes of them to be made (`queue`: [`warm_queue_of`], which the thread that worked the cut out has made, and the others make).
    fn keep_warm(&mut self, cut: KeptCut, queue: Vec<BrickCube>) {
        self.cubes.pin(&cut.cubes);
        self.warm_queue = queue;
        self.warm_cut = Some(cut);
    }

    /// Starts working out the cubes of the warm viewpoint: on the thread that does it, from a copy of what the terrain says, if the terrain has one, and here otherwise.
    fn work_out_warm_cut(
        &mut self,
        source: &impl CoarseTerrainSource,
        asked: &Viewpoint,
        version: u64,
        edits: u64,
    ) {
        let centre = asked.viewer.brick();
        let Some(oracle) = source.detached_may_show() else {
            let cubes = self.select_cut(centre, asked.viewer, &asked.settings, source);
            let queue = warm_queue_of(&cubes, centre);
            self.keep_warm(
                KeptCut {
                    centre,
                    settings: asked.settings,
                    version,
                    edits,
                    cubes,
                },
                queue,
            );
            return;
        };
        self.selections += 1;
        self.warm_in_flight = true;
        let job = WarmJob {
            epoch: self.warm_epoch,
            centre,
            viewer: asked.viewer,
            settings: asked.settings,
            version,
            edits,
            grid: self.grid,
            fine_radius: u32::try_from(self.mesher.view_radius_bricks()).unwrap_or(0),
            core: source.solid_core(),
            draws_beside_edits: !source.waits_for_nodes(),
            edited: self.edited.clone(),
            oracle,
            memo: self.warm_memo.take().unwrap_or_default(),
        };
        self.warm_worker
            .get_or_insert_with(WarmWorker::start)
            .hand_over(job);
    }

    /// The cubes the thread has finished, if it has finished any that are for the life the view is in now (what it learnt of the terrain is kept for the next).
    fn collect_warm_cut(&mut self) -> Option<(KeptCut, Vec<BrickCube>)> {
        let worker = self.warm_worker.as_ref()?;
        let mut arrived = None;
        while let Some(finished) = worker.finished() {
            // As if the work had been done here: a bug in what the terrain says is a panic of whoever asked for the cubes.
            let done = match finished {
                WarmFinished::Done(done) => done,
                WarmFinished::Panicked(payload) => resume_unwind(payload),
            };
            if done.epoch != self.warm_epoch {
                continue;
            }
            self.warm_in_flight = false;
            self.warm_memo = Some(done.memo);
            arrived = Some((done.cut, done.queue));
        }
        arrived
    }

    /// Lets go of what the thread is doing or has done for the view as it was: what comes of it is thrown away, and what it learnt of the terrain with it.
    fn forget_the_warm_selection(&mut self) {
        if self.warm_in_flight || self.warm_memo.is_some() {
            self.warm_epoch += 1;
            self.warm_in_flight = false;
            self.warm_memo = None;
        }
    }

    fn start_jobs(&mut self, source: &impl CoarseTerrainSource, updates: &mut Vec<MeshUpdate>) {
        let source_waits = source.waits_for_nodes();
        let limit = jobs_to_hold(self.workers.len(), self.finished_last_update);
        while self.in_flight < limit {
            // (What the warm viewpoint needs waits for what is drawn.)
            let Some(cube) = self.queue.pop().or_else(|| self.warm_queue.pop()) else {
                break;
            };
            let version = self.data_version(cube);
            if !self.cubes.needs_making(cube, version) {
                continue;
            }
            // A picture that was held of data that has changed since is let go of, and the node is made again.
            if self.cubes.drop_stale_held(cube, version) == Taken::Removed {
                updates.push(MeshUpdate::RemoveCoarse(cube));
            }
            let source = match self.overrides.get(&(self.grid, cube)) {
                Some(held) => Source::Override(Arc::clone(&held.node)),
                None => match source.coarse_generator(cube) {
                    Some(generator) => Source::Generator(generator),
                    None if source_waits && !self.is_told_empty(cube) => {
                        // Nothing is made of it: it is not known to be empty, and what is drawn in its place stays until the server says.
                        self.waiting_for_the_server.insert(cube);
                        continue;
                    }
                    None => Source::Nothing,
                },
            };
            self.cubes.start_making(cube, version);
            self.in_flight += 1;
            self.jobs
                .as_ref()
                .expect("workers run until the view is dropped")
                .send(Job {
                    epoch: self.epoch,
                    cube,
                    version,
                    source,
                })
                .expect("workers are running");
        }
    }

    fn is_ready(&self, cube: BrickCube) -> bool {
        if cube.level() == 0 {
            return self.mesher.is_ready(cube.origin());
        }
        self.cubes.is_ready(cube, self.data_version(cube))
    }

    fn swap(&mut self, updates: &mut Vec<MeshUpdate>) {
        let swaps = plan_progressive_swaps(self.cubes.shown(), self.cubes.wanted(), |cube| {
            self.is_ready(cube)
        });
        let mut shown_now = 0;
        let mut complete = true;
        for swap in swaps {
            if shown_now >= SHOWN_PER_UPDATE {
                complete = false;
                break;
            }
            for cube in swap.remove {
                self.hide(cube, updates);
            }
            for cube in swap.add {
                // (Showing a picture that is held is a move and costs the graphics card nothing: it is not counted.)
                if self.show(cube, updates) {
                    shown_now += 1;
                }
            }
        }
        // What was left waits for readiness (a result will say) or for the next update (the limit).
        self.plan_needed = !complete;
    }

    fn hide(&mut self, cube: BrickCube, updates: &mut Vec<MeshUpdate>) {
        if cube.level() == 0 {
            self.cubes.hide_brick(cube);
            updates.extend(self.mesher.hide(cube.origin()));
        } else {
            match self.cubes.hold_node(cube, self.data_version(cube)) {
                Taken::Nothing => {}
                Taken::Removed => updates.push(MeshUpdate::RemoveCoarse(cube)),
                Taken::Hidden => updates.push(MeshUpdate::HideCoarse(cube)),
            }
        }
    }

    /// Shows a cube. Returns whether that cost the graphics card a picture to make (everything but showing one that was held).
    fn show(&mut self, cube: BrickCube, updates: &mut Vec<MeshUpdate>) -> bool {
        if cube.level() == 0 {
            // (Showing a brick that is held is a move as well.)
            let held = self.mesher.is_held(cube.origin());
            self.cubes.show_brick(cube);
            updates.extend(self.mesher.show(cube.origin()));
            return !held;
        } else if self.cubes.is_held(cube) {
            if self.cubes.reveal_node(cube) {
                updates.push(MeshUpdate::ShowCoarse(cube));
            }
            return false;
        } else {
            let mesh = self.cubes.show_node(cube);
            if !mesh.is_empty() {
                updates.push(MeshUpdate::UploadCoarse { cube, mesh });
            }
        }
        true
    }
}

impl Drop for LodView {
    fn drop(&mut self) {
        // Closing the job queue ends each worker's loop.
        self.jobs = None;
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

/// A cube in words: its detail level and where its lowest brick is.
fn describe(cube: BrickCube) -> String {
    let origin = cube.origin();
    format!(
        "level {} at ({}, {}, {})",
        cube.level(),
        origin.x(),
        origin.y(),
        origin.z()
    )
}

/// The square of the distance in bricks from `centre` to the middle of `cube` (the order in which cubes are made: nearest first). It saturates, since the square
/// of a distance across the range of coordinates (2^117) does not fit in 128 bits: exact for every cube nearer than 2^63 bricks, and for the farther ones only
/// as an order that puts them after.
pub(crate) fn squared_distance_to_middle(cube: BrickCube, centre: BrickCoordinate) -> u128 {
    let origin = cube.origin();
    let half = (cube.edge_length() / 2) as i128;
    let distance = |own: i128, at: i128| (own + half - at).unsigned_abs();
    [
        distance(origin.x(), centre.x()),
        distance(origin.y(), centre.y()),
        distance(origin.z(), centre.z()),
    ]
    .into_iter()
    .fold(0u128, |sum, distance| {
        sum.saturating_add(distance.saturating_mul(distance))
    })
}

#[cfg(test)]
mod tests {
    use voxel_space::BrickCoordinate;

    use super::*;

    #[test]
    fn cubes_are_made_nearest_first_by_the_square_of_the_distance_to_their_middle_and_a_far_one_does_not_overflow()
     {
        let centre = BrickCoordinate::new(0, 0, 0);
        let at = |x, y, z, level| BrickCube::new(BrickCoordinate::new(x, y, z), level).unwrap();
        // A cube of 2 bricks at (4, 0, 0) has its middle at (5, 1, 1): 25 + 1 + 1.
        assert_eq!(squared_distance_to_middle(at(4, 0, 0, 1), centre), 27);
        // A brick at (-3, -4, 0) has its middle at (-3 + 0, -4 + 0, 0), half being 0 for a brick: 9 + 16.
        assert_eq!(squared_distance_to_middle(at(-3, -4, 0, 0), centre), 25);
        assert!(
            squared_distance_to_middle(at(4, 0, 0, 1), centre)
                < squared_distance_to_middle(at(8, 0, 0, 1), centre)
        );
        // Across the whole range, the square is too large for 128 bits: it saturates rather than wrapping round to a small number that would sort as near.
        let far = BrickCoordinate::new(
            voxel_space::BRICK_COORDINATE_MAXIMUM,
            voxel_space::BRICK_COORDINATE_MAXIMUM,
            voxel_space::BRICK_COORDINATE_MAXIMUM,
        );
        let across = BrickCube::containing(far, 0);
        let from_the_other_end = BrickCoordinate::new(
            voxel_space::BRICK_COORDINATE_MINIMUM,
            voxel_space::BRICK_COORDINATE_MINIMUM,
            voxel_space::BRICK_COORDINATE_MINIMUM,
        );
        assert_eq!(
            squared_distance_to_middle(across, from_the_other_end),
            u128::MAX
        );
        assert!(
            squared_distance_to_middle(across, from_the_other_end)
                > squared_distance_to_middle(at(4, 0, 0, 1), centre)
        );
    }
}
