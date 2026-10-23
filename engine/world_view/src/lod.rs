//! Bricks and coarse nodes drawn together (`docs/lod-spec.md`, 3.3 and 3.6): what a viewer sees of the terrain of a level, near as bricks and far as nodes.
//!
//! [`LodView`] decides, from where the viewer is, which cubes are to be drawn ([`coarse_cells::select_around_brick`]: bricks inside the viewer's
//! [`coarse_cells::FineRegion`], nodes of growing size outside), has them made (the bricks by the [`WorldMesher`], held back; the nodes on threads of its own,
//! from the terrain generators or from what the server has said differs, see [`LodView::set_override`]) and then swaps what is on the screen for what is
//! wanted in groups ([`coarse_cells::plan_swaps`]), so that a cube and what replaces it change in one frame and are never both on the screen or neither.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;

use coarse_cells::{
    CoarseNode, DetailSettings, ViewSelection, Zoom, plan_swaps, select_around_brick,
};
use region_octree::BrickCube;
use terrain::TerrainGenerator;
use voxel_meshing::{PackedBrickMesh, mesh_coarse_node};
use voxel_space::{BrickCoordinate, GridId, LevelIndex, WorldPosition};
use world_storage::LevelData;

use crate::mesher::{MeshUpdate, WorldMesher};

/// How much a picture is asked to show.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodSettings {
    pub detail: DetailSettings,
    pub zoom: Option<Zoom>,
    /// Nodes whose nearest point is farther than this many voxels are not drawn.
    pub maximum_distance: f64,
}

/// What the view needs to know of the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LodConfig {
    /// How bright terrain is that no light reaches, which is what a generator is told its cells are lit by.
    pub unlit: u8,
    /// How many threads make nodes.
    pub worker_count: usize,
}

/// How many cubes that are to come may be put on the screen in one update: each is a buffer made on the GPU.
const SHOWN_PER_UPDATE: usize = 64;

/// How many nodes are being made at once for each thread.
const JOBS_PER_WORKER: usize = 4;

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

#[derive(Default)]
struct NodeState {
    /// The version of the picture on the screen, if the node is shown.
    drawn: Option<u64>,
    /// A picture that is up to date and not on the screen yet, with its version.
    ready: Option<(u64, PackedBrickMesh)>,
    /// The version being made.
    requested: Option<u64>,
}

pub struct LodView {
    mesher: WorldMesher,
    grid: GridId,
    /// What the server has said differs from the generators', by grid and cube.
    overrides: HashMap<(GridId, BrickCube), Override>,
    next_version: u64,
    /// What is to be drawn, bricks (cubes of level 0) and nodes, and for what it was worked out.
    wanted: HashSet<BrickCube>,
    wanted_for: Option<(BrickCoordinate, LodSettings)>,
    /// What is drawn: bricks that were shown (some with nothing to see) and nodes.
    shown: HashSet<BrickCube>,
    /// The shown nodes that have something on the screen.
    uploaded: HashSet<BrickCube>,
    nodes: HashMap<BrickCube, NodeState>,
    /// Nodes to be made, the most wanted last.
    queue: Vec<BrickCube>,
    plan_needed: bool,
    seen_ready_serial: u64,
    epoch: u64,
    in_flight: usize,
    jobs: Option<mpsc::Sender<Job>>,
    results: mpsc::Receiver<Done>,
    workers: Vec<JoinHandle<()>>,
}

impl std::fmt::Debug for LodView {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LodView")
            .field("wanted", &self.wanted.len())
            .field("shown", &self.shown.len())
            .field("in_flight", &self.in_flight)
            .finish_non_exhaustive()
    }
}

impl LodView {
    /// A view whose bricks are made by `mesher`, which is held back from now on.
    pub fn new(mesher: WorldMesher, config: LodConfig) -> Self {
        let mesher = mesher.with_held_back_display();
        let (job_sender, job_receiver) = mpsc::channel::<Job>();
        let (result_sender, results) = mpsc::channel::<Done>();
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
                        let done = Done {
                            epoch: job.epoch,
                            cube: job.cube,
                            version: job.version,
                            mesh,
                        };
                        if result_sender.send(done).is_err() {
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
            next_version: 0,
            wanted: HashSet::new(),
            wanted_for: None,
            shown: HashSet::new(),
            uploaded: HashSet::new(),
            nodes: HashMap::new(),
            queue: Vec::new(),
            plan_needed: false,
            seen_ready_serial: 0,
            epoch: 0,
            in_flight: 0,
            jobs: Some(job_sender),
            results,
            workers,
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
        &self.shown
    }

    /// The cubes that are to be drawn, as far as the last update worked it out.
    pub fn wanted(&self) -> &HashSet<BrickCube> {
        &self.wanted
    }

    /// Work in progress: bricks and nodes being made.
    pub fn pending_jobs(&self) -> usize {
        self.mesher.pending_jobs() + self.in_flight
    }

    /// Whether anything is waiting to be made or swapped: the view is not yet what it is to be.
    pub fn is_settled(&self) -> bool {
        self.pending_jobs() == 0 && self.queue.is_empty() && !self.plan_needed
    }

    /// The node the server says is the node of a cube instead of what the generators give (a `None` takes back what was said). Bricks have none, and
    /// what is said of one is ignored.
    pub fn set_override(&mut self, grid: GridId, cube: BrickCube, node: Option<CoarseNode>) {
        if cube.level() == 0 {
            return;
        }
        match node {
            Some(node) => {
                self.next_version += 1;
                self.overrides.insert(
                    (grid, cube),
                    Override {
                        node: Arc::new(node),
                        version: self.next_version,
                    },
                );
            }
            None => {
                self.overrides.remove(&(grid, cube));
            }
        }
        if grid == self.grid && self.nodes.contains_key(&cube) {
            self.queue.push(cube);
        }
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
        let mut uploaded: Vec<BrickCube> = self.uploaded.drain().collect();
        uploaded.sort_by_key(|cube| {
            let origin = cube.origin();
            (cube.level(), origin.x(), origin.y(), origin.z())
        });
        updates.extend(uploaded.into_iter().map(MeshUpdate::RemoveCoarse));
        self.wanted.clear();
        self.wanted_for = None;
        self.shown.clear();
        self.nodes.clear();
        self.queue.clear();
        self.plan_needed = false;
        self.epoch += 1;
        self.in_flight = 0;
        updates
    }

    /// Collects what the threads have finished, works out what is to be drawn from where the viewer is, and puts on the screen what can be. Returns what
    /// the renderer must change. `level` is the level the viewer is in (and `level_index` its number); the viewer is `viewer`, in its bricks.
    pub fn update(
        &mut self,
        level_index: LevelIndex,
        level: &LevelData,
        viewer: WorldPosition,
        settings: &LodSettings,
    ) -> Vec<MeshUpdate> {
        self.grid = GridId::root(level_index);
        let centre = viewer.brick();
        let mut updates = self.mesher.update(level, centre);
        self.collect_nodes(&mut updates);
        if self.wanted_for != Some((centre, *settings)) {
            self.reselect(centre, settings);
        }
        self.start_jobs(level);
        let serial = self.mesher.ready_serial();
        if self.plan_needed || serial != self.seen_ready_serial {
            self.seen_ready_serial = serial;
            self.swap(&mut updates);
        }
        updates
    }

    /// Updates until nothing is left to make or swap, returning every change. For headless rendering and tests.
    pub fn settle(
        &mut self,
        level_index: LevelIndex,
        level: &LevelData,
        viewer: WorldPosition,
        settings: &LodSettings,
    ) -> Vec<MeshUpdate> {
        let mut updates = Vec::new();
        loop {
            updates.extend(self.update(level_index, level, viewer, settings));
            if self.is_settled() {
                return updates;
            }
            std::thread::sleep(std::time::Duration::from_micros(200));
        }
    }

    fn collect_nodes(&mut self, updates: &mut Vec<MeshUpdate>) {
        while let Ok(done) = self.results.try_recv() {
            if done.epoch != self.epoch {
                continue;
            }
            self.in_flight = self.in_flight.saturating_sub(1);
            let current = self.data_version(done.cube);
            let Some(state) = self.nodes.get_mut(&done.cube) else {
                continue;
            };
            if state.requested == Some(done.version) {
                state.requested = None;
            }
            if done.version != current {
                // Made from data that has been replaced since: the new data is queued already.
                continue;
            }
            if self.shown.contains(&done.cube) {
                // On the screen already: the picture is replaced where it is.
                state.drawn = Some(done.version);
                if !done.mesh.is_empty() {
                    self.uploaded.insert(done.cube);
                    updates.push(MeshUpdate::UploadCoarse {
                        cube: done.cube,
                        mesh: done.mesh,
                    });
                } else if self.uploaded.remove(&done.cube) {
                    updates.push(MeshUpdate::RemoveCoarse(done.cube));
                }
            } else {
                state.ready = Some((done.version, done.mesh));
                self.plan_needed = true;
            }
        }
    }

    fn reselect(&mut self, centre: BrickCoordinate, settings: &LodSettings) {
        let cubes = select_around_brick(
            centre,
            &ViewSelection {
                detail: settings.detail,
                zoom: settings.zoom,
                fine_radius: u32::try_from(self.mesher.view_radius_bricks()).unwrap_or(0),
                maximum_distance: settings.maximum_distance,
            },
        );
        self.wanted = cubes.iter().copied().collect();
        self.wanted_for = Some((centre, *settings));
        for cube in cubes.iter().filter(|cube| cube.level() > 0) {
            self.nodes.entry(*cube).or_default();
        }
        let (wanted, shown) = (&self.wanted, &self.shown);
        self.nodes
            .retain(|cube, _| wanted.contains(cube) || shown.contains(cube));
        // The nearest are made first: they are the last in the queue.
        let nearest_last = |cube: &BrickCube| {
            let origin = cube.origin();
            let half = (cube.edge_length() / 2) as i64;
            let distance = |own: i64, at: i64| (own + half - at).unsigned_abs();
            std::cmp::Reverse(
                u128::from(distance(origin.x(), centre.x())).pow(2)
                    + u128::from(distance(origin.y(), centre.y())).pow(2)
                    + u128::from(distance(origin.z(), centre.z())).pow(2),
            )
        };
        self.queue = cubes.into_iter().filter(|cube| cube.level() > 0).collect();
        self.queue.sort_by_key(nearest_last);
        self.plan_needed = true;
    }

    /// Whether the picture of a node that is wanted or shown has to be made (again).
    fn needs_making(&self, cube: BrickCube) -> bool {
        let Some(state) = self.nodes.get(&cube) else {
            return false;
        };
        let version = self.data_version(cube);
        if state.requested == Some(version) {
            return false;
        }
        if self.shown.contains(&cube) {
            state.drawn != Some(version)
        } else {
            self.wanted.contains(&cube)
                && state.ready.as_ref().map(|(ready, _)| *ready) != Some(version)
        }
    }

    fn start_jobs(&mut self, level: &LevelData) {
        let limit = self.workers.len() * JOBS_PER_WORKER;
        while self.in_flight < limit {
            let Some(cube) = self.queue.pop() else {
                break;
            };
            if !self.needs_making(cube) {
                continue;
            }
            let version = self.data_version(cube);
            let source = match self.overrides.get(&(self.grid, cube)) {
                Some(held) => Source::Override(Arc::clone(&held.node)),
                None => match level.terrain.coarse_generator(cube) {
                    Some(generator) => Source::Generator(generator),
                    None => Source::Nothing,
                },
            };
            self.nodes
                .get_mut(&cube)
                .expect("it needs making, so it has a state")
                .requested = Some(version);
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
        let version = self.data_version(cube);
        self.nodes
            .get(&cube)
            .and_then(|state| state.ready.as_ref())
            .is_some_and(|(ready, _)| *ready == version)
    }

    fn swap(&mut self, updates: &mut Vec<MeshUpdate>) {
        let swaps = plan_swaps(&self.shown, &self.wanted, |cube| self.is_ready(cube));
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
                self.show(cube, updates);
                shown_now += 1;
            }
        }
        // What was left waits for readiness (a result will say) or for the next update (the limit).
        self.plan_needed = !complete;
    }

    fn hide(&mut self, cube: BrickCube, updates: &mut Vec<MeshUpdate>) {
        self.shown.remove(&cube);
        if cube.level() == 0 {
            updates.extend(self.mesher.hide(cube.origin()));
            return;
        }
        if self.uploaded.remove(&cube) {
            updates.push(MeshUpdate::RemoveCoarse(cube));
        }
        if let Some(state) = self.nodes.get_mut(&cube) {
            state.drawn = None;
        }
        if !self.wanted.contains(&cube) {
            self.nodes.remove(&cube);
        }
    }

    fn show(&mut self, cube: BrickCube, updates: &mut Vec<MeshUpdate>) {
        if cube.level() == 0 {
            self.shown.insert(cube);
            updates.extend(self.mesher.show(cube.origin()));
            return;
        }
        let state = self
            .nodes
            .get_mut(&cube)
            .expect("a node that is ready has a state");
        let (version, mesh) = state
            .ready
            .take()
            .expect("the planner only adds what is ready");
        state.drawn = Some(version);
        self.shown.insert(cube);
        if !mesh.is_empty() {
            self.uploaded.insert(cube);
            updates.push(MeshUpdate::UploadCoarse { cube, mesh });
        }
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
