//! The server half of coarse levels of detail (`docs/specs/lod-spec.md`, 3.4 and 3.5): the coarse nodes that differ from what a client derives, kept up as the world
//! is edited, and sent to the clients that need them, coarsest first and no faster than they ask.
//!
//! A client works out the coarse cells of unedited terrain for itself from the terrain generators it was told of. Where bricks have been edited those cells are
//! wrong, so the server keeps, for each cube of bricks of detail level 1 to [`CoarseConfig::maximum_override_level`] that has an edited brick in it, an **override**:
//! the cells the generator gives for the cube, with the cells under each edited brick made from the brick itself and the cells under each half that has an override
//! made from that. The overrides are kept up by marking the cubes round an edit **dirty** and remaking a few of them each tick, finest first.
//!
//! Everything is keyed by the grid. The grids handled are the root grids of levels (terrain) and **planets** (ships with terrain generators of their own, `lod-spec.md`
//! 3.8): for a planet the cells the generator gives are patched with the exact cells of its stored bricks, in the planet's own coordinates. A ship nobody generates is
//! not handled yet (the far-nodes plan, `docs/specs/far-nodes-spec.md`, F4).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use block_models::BlockTable;
use coarse_cells::{CoarseCell, CoarseNode, EditedCubes, ViewSelection, select_around_brick};
use namespaced_id::NamespacedId;
use player_presence::{SharedPresence, lock};
use plugin_host::{HandlerSpec, Phase, PlayerLeft, Plugin, Registrar, SystemSpec};
use protocol::{ClientId, Message};
use region_octree::BrickCube;
use server::Audience;
use terrain::TerrainGenerator;
use voxel_grid::{Brick, ChannelIndex, ChannelStorage, Grid, GridLayout};
use voxel_meshing::{BrickChannels, BrickNeighbourhood, coarse_node_of_brick};
use voxel_space::{
    BrickBox, BrickCoordinate, FaceDirection, GridId, LevelIndex, LevelOfDetail, WorldPosition,
};
#[cfg(test)]
use world_storage::BrickSource;
use world_storage::{LevelData, WorldData};

use crate::messages::{CoarseNodeUpdate, CoarseNodes, MAXIMUM_UPDATES, SetDetail};

/// A node: its grid, its detail level and the origin of its cube, which is a total order that has the finest first when sorted by level.
type Key = (u8, GridId, [i128; 3]);

fn key_of(grid: GridId, cube: BrickCube) -> Key {
    let origin = cube.origin();
    (cube.level(), grid, [origin.x(), origin.y(), origin.z()])
}

fn cube_of(key: Key) -> BrickCube {
    BrickCube::new(BrickCoordinate::new(key.2[0], key.2[1], key.2[2]), key.0)
        .expect("a key is made from a cube")
}

/// How the brightness of a voxel is read from a channel of the game, for games that light: a function from the channel's value to 0 to 255.
#[derive(Clone)]
pub struct Brightness {
    pub channel: ChannelIndex,
    pub brightness_of: Arc<dyn Fn(u32) -> u8 + Send + Sync>,
}

impl std::fmt::Debug for Brightness {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Brightness")
            .field("channel", &self.channel)
            .finish_non_exhaustive()
    }
}

/// What the plugin needs to know of the game.
#[derive(Clone, Debug)]
pub struct CoarseConfig {
    pub table: BlockTable,
    /// The number of the air block.
    pub air: u32,
    pub block_channel: ChannelIndex,
    pub state_channel: Option<ChannelIndex>,
    pub orientation_channel: Option<ChannelIndex>,
    pub brightness: Option<Brightness>,
    /// The brightness of terrain no light reaches (what the mesher is given for it).
    pub unlit: u8,
    pub ticks_per_second: u32,
    /// The coarsest detail level that has overrides: an edit is lost in a cell bigger than this.
    pub maximum_override_level: u8,
    /// How far from a player, in voxels, nodes are chosen for them.
    pub view_distance: f64,
    /// The most dirty nodes a tick takes up for remaking, and the most that the worker that remakes them has in hand at once (a tick hands over what it can
    /// and does not wait for the worker: the nodes come back in a later tick).
    pub nodes_per_tick: usize,
    /// The levels whose own terrain has nodes kept (by their identifiers), or every level if `None`. A game leaves out a level it draws in bricks only, whose
    /// generator may be slow to make a node of for nothing. (The planets in any level are kept.)
    pub levels: Option<BTreeSet<NamespacedId>>,
    pub presence: SharedPresence,
}

impl CoarseConfig {
    /// A configuration with the usual limits: overrides up to detail level 12 (cells of 4 kilometres), nodes chosen out to a million voxels, and 32 nodes
    /// handed to the worker in a tick.
    pub fn new(
        table: BlockTable,
        air: u32,
        block_channel: ChannelIndex,
        unlit: u8,
        ticks_per_second: u32,
        presence: SharedPresence,
    ) -> Self {
        Self {
            table,
            air,
            block_channel,
            state_channel: None,
            orientation_channel: None,
            brightness: None,
            unlit,
            ticks_per_second,
            maximum_override_level: 12,
            view_distance: 1_048_576.0,
            nodes_per_tick: 32,
            levels: None,
            presence,
        }
    }
}

/// The least a client may be allowed to save up of what it asks for: enough for the biggest node, so that a slow rate still gets one through.
const BURST_MINIMUM_BYTES: f64 = 200_000.0;

struct Stored {
    node: CoarseNode,
    /// Counts up each time the node is remade differently in what is sent (not in its exposure, which is not).
    version: u64,
}

struct ClientState {
    detail: SetDetail,
    /// Where the selection was made from: the brick the client is in, in each grid that has nodes (the root grid of its level, and each planet in it), in the
    /// grid's own coordinates.
    selected_from: Option<Vec<(GridId, BrickCoordinate)>>,
    selected_for: Option<SetDetail>,
    leaves: Vec<(GridId, BrickCube)>,
    /// Which set of places with an override the leaves were last worked out for ([`State::edit_epoch`]).
    selected_edit_epoch: u64,
    /// What the client has been sent: the version of each node.
    sent: BTreeMap<Key, u64>,
    /// What is left to send, coarsest first (a node with `None` is to be taken back).
    pending: VecDeque<(Key, Option<u64>)>,
    /// Counts the overrides this was worked out against.
    pending_epoch: u64,
    credit: f64,
}

#[derive(Default)]
struct State {
    overrides: BTreeMap<Key, Stored>,
    dirty: BTreeSet<Key>,
    /// Counts up each time an override is stored or removed.
    epoch: u64,
    next_version: u64,
    clients: BTreeMap<ClientId, ClientState>,
    /// Where the overrides are, to find the cubes with one in them without looking at the rest of the world.
    edited: EditedCubes,
    /// Counts up each time a place gets its first override or loses its last.
    edit_epoch: u64,
    /// The nodes that are with the worker: handed over, not yet taken back.
    in_flight: BTreeSet<Key>,
    /// The thread that makes nodes, started when the first is handed over.
    worker: Option<Worker>,
}

/// The thread that makes nodes from [`NodeJob`]s, in the order it was given them (as the thread of `prefetch` in the server does for bricks). It ends when this
/// is dropped, after the node it is making.
struct Worker {
    jobs: Sender<(Key, NodeJob)>,
    made: Receiver<(Key, Option<CoarseNode>)>,
    /// How many jobs were handed over, and how many the thread has sent back (counted when the result is in the channel).
    handed: u64,
    finished: Arc<AtomicU64>,
}

impl Worker {
    fn start(config: Arc<CoarseConfig>) -> Self {
        let (jobs, wanted) = channel::<(Key, NodeJob)>();
        let (sender, made) = channel();
        let finished = Arc::new(AtomicU64::new(0));
        let counted = Arc::clone(&finished);
        std::thread::Builder::new()
            .name("coarse nodes".to_owned())
            .spawn(move || {
                while let Ok((key, job)) = wanted.recv() {
                    let node = job.run(&config);
                    if sender.send((key, node)).is_err() {
                        break;
                    }
                    counted.fetch_add(1, Ordering::SeqCst);
                }
            })
            .expect("a thread can be started");
        Self {
            jobs,
            made,
            handed: 0,
            finished,
        }
    }

    fn is_busy(&self) -> bool {
        self.finished.load(Ordering::SeqCst) != self.handed
    }
}

impl State {
    /// Keeps `stored` as the override of the cube of `key`.
    fn store(&mut self, key: Key, stored: Stored) {
        if self.overrides.insert(key, stored).is_none() && self.edited.add(key.1, cube_of(key)) {
            self.edit_epoch += 1;
        }
    }

    /// Takes back the override of the cube of `key`, if it has one.
    fn forget(&mut self, key: Key) {
        if self.overrides.remove(&key).is_some() && self.edited.remove(key.1, cube_of(key)) {
            self.edit_epoch += 1;
        }
    }

    /// Marks the cube of detail level 1 that holds `brick`, and those that hold the bricks that touch it (an edit at the edge of a brick changes the
    /// surface of the brick beside it), as to be remade.
    fn mark_brick(&mut self, grid: GridId, brick: BrickCoordinate, maximum_level: u8) {
        if maximum_level == 0 {
            return;
        }
        self.dirty
            .insert(key_of(grid, BrickCube::containing(brick, 1)));
        for face in FaceDirection::ALL {
            let step = face.offset();
            if let Some(next) = brick.checked_offset(step[0], step[1], step[2]) {
                self.dirty
                    .insert(key_of(grid, BrickCube::containing(next, 1)));
            }
        }
    }
}

fn locked(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The channels a brick is looked at through, owned: the brick itself and the brightness of each voxel where there is a channel for it.
struct Owned {
    brick: Arc<Brick>,
    light: Option<ChannelStorage>,
}

impl Owned {
    fn new(brick: Arc<Brick>, config: &CoarseConfig) -> Self {
        let light = config.brightness.as_ref().map(|brightness| {
            let values: Vec<u32> = brick
                .channel(brightness.channel)
                .iter()
                .map(|value| u32::from((brightness.brightness_of)(value)))
                .collect();
            ChannelStorage::from_values(&values)
        });
        Self { brick, light }
    }

    fn channels(&self, config: &CoarseConfig) -> BrickChannels<'_> {
        BrickChannels {
            blocks: self.brick.channel(config.block_channel),
            states: config
                .state_channel
                .map(|channel| self.brick.channel(channel)),
            orientations: config
                .orientation_channel
                .map(|channel| self.brick.channel(channel)),
            light: self.light.as_ref(),
        }
    }
}

/// Whether the plugin keeps nodes for the grid: the root grid of a level the configuration keeps, or a planet (a ship with terrain generators of its own).
fn keeps_nodes_of(config: &CoarseConfig, data: &WorldData, grid: GridId) -> bool {
    if let Some(level) = grid.root_level() {
        return keeps_level(config, data, level);
    }
    data.grids()
        .get(grid)
        .is_some_and(|ship| ship.terrain.is_some())
}

/// Whether the configuration keeps the nodes of the terrain of a level.
fn keeps_level(config: &CoarseConfig, data: &WorldData, level: LevelIndex) -> bool {
    config.levels.as_ref().is_none_or(|kept| {
        data.level_id(level)
            .is_some_and(|identifier| kept.contains(identifier))
    })
}

/// The bricks of a grid that has nodes: the ones that were stored (the edits), and what makes the ones nobody stored.
enum GridBricks<'a> {
    Level(&'a LevelData),
    /// A planet: the bricks of the generator inside the extent, nothing outside it, and the stored bricks over both (as a client makes them,
    /// `world_view::ShipTerrainSource`).
    Planet {
        voxels: &'a Grid,
        generator: Arc<dyn TerrainGenerator>,
        extent: BrickBox,
    },
}

impl<'a> GridBricks<'a> {
    /// `None` for a grid that has no nodes, or a planet whose generator this program lacks (a client cannot make its nodes either).
    fn of(data: &'a WorldData, grid: GridId) -> Option<Self> {
        if let Some(level) = grid.root_level() {
            return data.level(level).map(Self::Level);
        }
        let ship = data.grids().get(grid)?;
        let terrain = ship.terrain?;
        let generator = data
            .main()
            .terrain
            .generators()
            .entry(terrain.generator)?
            .content()
            .cloned()?;
        Some(Self::Planet {
            voxels: &ship.voxels,
            generator,
            extent: terrain.extent,
        })
    }

    /// The brick if it was stored.
    fn stored(&self, at: BrickCoordinate) -> Option<&Arc<Brick>> {
        match self {
            Self::Level(level) => level.overrides.brick(at),
            Self::Planet { voxels, .. } => voxels.brick(at),
        }
    }

    /// Where the brick comes from, for a thread to read it: the stored brick, or the generator that makes it, or nothing.
    fn source(&self, at: BrickCoordinate) -> Source {
        match self {
            Self::Level(level) => match level.overrides.brick(at) {
                Some(stored) => Source::Stored(Arc::clone(stored)),
                None => level
                    .terrain
                    .generator_of_brick(at)
                    .map_or(Source::Nothing, |generator| {
                        Source::Generated(generator, at)
                    }),
            },
            Self::Planet {
                voxels,
                generator,
                extent,
            } => {
                if let Some(stored) = voxels.brick(at) {
                    Source::Stored(Arc::clone(stored))
                } else if extent.contains(at) {
                    Source::Generated(Arc::clone(generator), at)
                } else {
                    Source::Blank
                }
            }
        }
    }

    /// The brick as the game sees it, stored or made (what [`GridBricks::source`] says, made).
    #[cfg(test)]
    fn effective(&self, at: BrickCoordinate) -> BrickSource {
        match self {
            Self::Level(level) => level.effective_brick(at),
            Self::Planet { voxels, .. } => match self.source(at) {
                Source::Stored(brick) => BrickSource::Edited(brick),
                Source::Generated(generator, at) => {
                    BrickSource::Generated(generator.generate_brick(voxels.layout(), at))
                }
                Source::Blank => BrickSource::Generated(Brick::new(voxels.layout())),
                Source::Nothing => BrickSource::Unassigned,
            },
        }
    }

    fn layout(&self) -> Arc<GridLayout> {
        match self {
            Self::Level(level) => Arc::clone(level.overrides.layout()),
            Self::Planet { voxels, .. } => Arc::clone(voxels.layout()),
        }
    }

    /// The generator whose coarse cells are the base of the node of the cube, which is what a client derives where nothing was edited.
    fn node_generator(&self, cube: BrickCube) -> Option<Arc<dyn TerrainGenerator>> {
        match self {
            Self::Level(level) => level.terrain.coarse_generator(cube),
            Self::Planet {
                generator, extent, ..
            } => cube.meets(*extent).then(|| Arc::clone(generator)),
        }
    }
}

/// A brick beside an edited one, as a thread that makes a node reads it.
enum Source {
    Stored(Arc<Brick>),
    /// Made by the generator, at these coordinates.
    Generated(Arc<dyn TerrainGenerator>, BrickCoordinate),
    /// Air, where a planet has nothing outside its extent.
    Blank,
    /// No generator makes it (none is assigned, or its plugin is missing): it is not looked at.
    Nothing,
}

/// One eighth of a node: what the node is patched with there.
enum Child {
    /// Nothing there to patch in.
    Absent,
    /// The override of the node of the half that is made of it.
    Node(CoarseNode),
    /// A stored brick, and the six beside it.
    Brick {
        center: Arc<Brick>,
        beside: Box<[Source; 6]>,
    },
}

/// Everything it takes to make a node of a cube, taken from the world on the tick so that a thread can do the making (the generators' work, which is what
/// costs) while the world goes on.
struct NodeJob {
    cube: BrickCube,
    generated: Option<Arc<dyn TerrainGenerator>>,
    layout: Arc<GridLayout>,
    children: [Child; 8],
}

impl NodeJob {
    /// The node: what the generator gives for the cube, patched with the exact cells of every half that is known exactly (an edited brick, or a node that
    /// has an override).
    fn run(&self, config: &CoarseConfig) -> Option<CoarseNode> {
        let parent_level = LevelOfDetail::new(self.cube.level())?;
        let mut node = self
            .generated
            .as_ref()
            .and_then(|generator| generator.coarse_node(self.cube, config.unlit))
            .unwrap_or_else(|| CoarseNode::uniform(CoarseCell::empty(config.air, config.unlit)));
        for (octant, child) in self.children.iter().enumerate() {
            let patch = match child {
                Child::Absent => continue,
                Child::Node(half) => half.clone(),
                Child::Brick { center, beside } => self.summarize_brick(config, center, beside),
            };
            node = node.patched_with_child(octant, &patch, parent_level, config.air);
        }
        Some(node)
    }

    /// The cells of an edited brick, exactly: from the brick and from the six beside it as the game sees them (edited or generated; a region with no
    /// generator is air).
    fn summarize_brick(
        &self,
        config: &CoarseConfig,
        center: &Arc<Brick>,
        beside: &[Source; 6],
    ) -> CoarseNode {
        let center = Owned::new(Arc::clone(center), config);
        let neighbours: Vec<Option<Owned>> = beside
            .iter()
            .map(|source| match source {
                Source::Stored(brick) => Some(Owned::new(Arc::clone(brick), config)),
                Source::Generated(generator, at) => Some(Owned::new(
                    Arc::new(generator.generate_brick(&self.layout, *at)),
                    config,
                )),
                Source::Blank => Some(Owned::new(Arc::new(Brick::new(&self.layout)), config)),
                Source::Nothing => None,
            })
            .collect();
        coarse_node_of_brick(
            &BrickNeighbourhood {
                center: center.channels(config),
                neighbours: std::array::from_fn(|index| {
                    neighbours[index]
                        .as_ref()
                        .map(|owned| owned.channels(config))
                }),
            },
            &config.table,
            config.air,
        )
    }
}

/// What it takes to make the node of a cube as the server makes it, or `None` if no half of it is known exactly (an edited brick, or a node that has an
/// override): there is nothing to say that the client does not derive.
fn gather(
    overrides: &BTreeMap<Key, Stored>,
    data: &WorldData,
    grid: GridId,
    cube: BrickCube,
) -> Option<NodeJob> {
    let bricks = GridBricks::of(data, grid)?;
    let half = (cube.edge_length() / 2) as i128;
    let origin = cube.origin();
    let mut touched = false;
    let children: [Child; 8] = std::array::from_fn(|octant| {
        let (dx, dy, dz) = (
            (octant & 1) as i128,
            ((octant >> 1) & 1) as i128,
            (octant >> 2) as i128,
        );
        let child_origin = BrickCoordinate::new(
            origin.x() + dx * half,
            origin.y() + dy * half,
            origin.z() + dz * half,
        );
        let child = if cube.level() == 1 {
            bricks.stored(child_origin).map(|center| Child::Brick {
                center: Arc::clone(center),
                beside: Box::new(std::array::from_fn(|index| {
                    let step = FaceDirection::ALL[index].offset();
                    child_origin
                        .checked_offset(step[0], step[1], step[2])
                        .map_or(Source::Nothing, |next| bricks.source(next))
                })),
            })
        } else {
            let child_cube =
                BrickCube::new(child_origin, cube.level() - 1).expect("half of a cube is a cube");
            overrides
                .get(&key_of(grid, child_cube))
                .map(|stored| Child::Node(stored.node.clone()))
        };
        touched |= child.is_some();
        child.unwrap_or(Child::Absent)
    });
    touched.then(|| NodeJob {
        cube,
        generated: bricks.node_generator(cube),
        layout: bricks.layout(),
        children,
    })
}

/// The node of a cube as the server makes it, on the spot (the worker makes it from the same [`gather`]ed job).
#[cfg(test)]
fn remake(
    config: &CoarseConfig,
    overrides: &BTreeMap<Key, Stored>,
    data: &WorldData,
    grid: GridId,
    cube: BrickCube,
) -> Option<CoarseNode> {
    gather(overrides, data, grid, cube)?.run(config)
}

impl State {
    /// How many nodes are waiting to be remade: dirty, or with the worker.
    fn waiting(&self) -> usize {
        self.dirty.len()
            + self
                .in_flight
                .iter()
                .filter(|key| !self.dirty.contains(key))
                .count()
    }

    /// What a tick does for the nodes: takes in what the worker has finished, and hands it more. It does not wait for the worker.
    fn work(&mut self, config: &Arc<CoarseConfig>, data: &WorldData) {
        self.take_in_finished(config.maximum_override_level);
        self.hand_over(config, data);
    }

    /// Keeps the nodes the worker has finished, in the order it was given them.
    fn take_in_finished(&mut self, maximum_level: u8) {
        let Some(worker) = &self.worker else {
            return;
        };
        let mut finished = Vec::new();
        loop {
            match worker.made.try_recv() {
                Ok(done) => finished.push(done),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    panic!("the thread that makes coarse nodes ended (a generator panicked?)")
                }
            }
        }
        for (key, node) in finished {
            self.in_flight.remove(&key);
            self.keep(maximum_level, key, node);
        }
    }

    /// Whether the node of `key` is made from nodes that are not final: one of its eight halves is dirty or with the worker.
    fn waits_for_its_halves(&self, key: Key) -> bool {
        let cube = cube_of(key);
        if cube.level() <= 1 {
            return false;
        }
        let half = (cube.edge_length() / 2) as i128;
        let origin = cube.origin();
        (0..8_i128).any(|octant| {
            let child_origin = BrickCoordinate::new(
                origin.x() + (octant & 1) * half,
                origin.y() + ((octant >> 1) & 1) * half,
                origin.z() + (octant >> 2) * half,
            );
            let child = key_of(
                key.1,
                BrickCube::new(child_origin, cube.level() - 1).expect("half of a cube is a cube"),
            );
            self.dirty.contains(&child) || self.in_flight.contains(&child)
        })
    }

    /// Takes up to `nodes_per_tick` dirty nodes, the finest first, and hands the work of making them to the worker. A node waits until the nodes it is made
    /// from are final (so none is made twice), and until the worker is done with the last time it was asked for it (an edit that comes while it is being made
    /// leaves it dirty, and it is made again from what is there then).
    fn hand_over(&mut self, config: &Arc<CoarseConfig>, data: &WorldData) {
        let limit = config.nodes_per_tick;
        let room = limit.saturating_sub(self.in_flight.len());
        let mut taken: Vec<(Key, Option<NodeJob>)> = Vec::new();
        let mut jobs = 0;
        for &key in &self.dirty {
            if taken.len() >= limit || jobs >= room {
                break;
            }
            if self.in_flight.contains(&key) || self.waits_for_its_halves(key) {
                continue;
            }
            let job = gather(&self.overrides, data, key.1, cube_of(key));
            jobs += usize::from(job.is_some());
            taken.push((key, job));
        }
        for (key, job) in taken {
            self.dirty.remove(&key);
            match job {
                None => self.keep(config.maximum_override_level, key, None),
                Some(job) => {
                    let worker = self
                        .worker
                        .get_or_insert_with(|| Worker::start(Arc::clone(config)));
                    worker.handed += 1;
                    worker
                        .jobs
                        .send((key, job))
                        .expect("the thread that makes coarse nodes is there");
                    self.in_flight.insert(key);
                }
            }
        }
    }

    /// Keeps the node that was made for `key` (`None`: there is nothing to say of it) if it is not the one that is kept, and has the node of the cube round it made
    /// again.
    fn keep(&mut self, maximum_level: u8, key: Key, made: Option<CoarseNode>) {
        let cube = cube_of(key);
        let changed = match (&made, self.overrides.get(&key)) {
            (Some(new), Some(old)) => *new != old.node,
            (None, None) => false,
            _ => true,
        };
        if !changed {
            return;
        }
        match made {
            Some(node) => {
                // The exposure of a cell is not sent, so a node that differs only in it is the same to every client: the version stays.
                let version = match self.overrides.get(&key) {
                    Some(old) if old.node.without_exposure() == node.without_exposure() => {
                        old.version
                    }
                    _ => {
                        self.next_version += 1;
                        self.next_version
                    }
                };
                self.store(key, Stored { node, version });
            }
            None => self.forget(key),
        }
        self.epoch += 1;
        if cube.level() < maximum_level {
            self.dirty.insert(key_of(
                key.1,
                BrickCube::containing(cube.origin(), cube.level() + 1),
            ));
        }
    }

    /// Works out, for a client, which cubes of the level it is in it needs (the selection from where it is, bricks left out) and then which of them have
    /// overrides it has not been sent, coarsest first.
    fn plan(&mut self, config: &CoarseConfig, data: &WorldData, client: ClientId) {
        let Some(state) = self.clients.get(&client) else {
            return;
        };
        let wanted = state.detail.bytes_per_second > 0;
        let pose = {
            let presence = lock(&config.presence);
            presence
                .pose_of(data, client)
                .map(|pose| (pose.position, presence.level_of(data, client)))
        };
        let state = self.clients.get_mut(&client).expect("it was just found");
        let Some((position, level)) = pose.filter(|_| wanted) else {
            state.pending.clear();
            state.leaves.clear();
            state.selected_from = None;
            return;
        };
        let from = viewer_bricks(config, data, level, position);
        let moved =
            state.selected_from.as_ref() != Some(&from) || state.selected_for != Some(state.detail);
        if moved || state.selected_edit_epoch != self.edit_epoch {
            state.leaves = from
                .iter()
                .flat_map(|&(grid, brick)| {
                    select_leaves(config, &self.edited, grid, brick, &state.detail)
                })
                .collect();
            state.selected_from = Some(from);
            state.selected_for = Some(state.detail);
            state.selected_edit_epoch = self.edit_epoch;
            state.pending_epoch = u64::MAX;
        }
        if state.pending_epoch != self.epoch {
            state.pending = pending_updates(&self.overrides, &state.sent, &state.leaves);
            state.pending_epoch = self.epoch;
        }
    }

    /// Sends what the client has credit for.
    fn send(&mut self, config: &CoarseConfig, client: ClientId) -> Vec<Vec<u8>> {
        let Some(state) = self.clients.get_mut(&client) else {
            return Vec::new();
        };
        state.credit = refilled(
            state.credit,
            state.detail.bytes_per_second,
            config.ticks_per_second,
        );
        let mut updates: Vec<CoarseNodeUpdate> = Vec::new();
        while state.credit > 0.0 {
            let Some((key, version)) = state.pending.pop_front() else {
                break;
            };
            let node =
                version.and_then(|_| self.overrides.get(&key).map(|stored| stored.node.clone()));
            let update = CoarseNodeUpdate {
                grid: key.1,
                cube: cube_of(key),
                node: node.map(|node| node.without_exposure()),
            };
            // What it costs is what it takes in a message of its own.
            state.credit -= CoarseNodes(vec![update.clone()]).encode().len() as f64;
            match version {
                Some(version) => {
                    state.sent.insert(key, version);
                }
                None => {
                    state.sent.remove(&key);
                }
            }
            updates.push(update);
        }
        in_messages(updates)
    }
}

/// What a client can spend after a tick: the rest of what it had, and a tick's share of its rate, never more than it can save up (a second of its rate,
/// or enough for the biggest node if that is more).
fn refilled(credit: f64, bytes_per_second: u32, ticks_per_second: u32) -> f64 {
    let rate = f64::from(bytes_per_second);
    let capacity = rate.max(BURST_MINIMUM_BYTES);
    (credit + rate / f64::from(ticks_per_second.max(1))).min(capacity)
}

/// Updates in the messages that carry them, no more than [`MAXIMUM_UPDATES`] in one, in order.
fn in_messages(updates: Vec<CoarseNodeUpdate>) -> Vec<Vec<u8>> {
    updates
        .chunks(MAXIMUM_UPDATES)
        .map(|chunk| CoarseNodes(chunk.to_vec()).encode())
        .collect()
}

/// What is left to tell a client: first the nodes it was sent that are no longer overridden (to be taken back), then the nodes of its `leaves` that have an
/// override it has not been sent in its latest version, coarsest first (the order of the walk otherwise, which is near to far).
fn pending_updates(
    overrides: &BTreeMap<Key, Stored>,
    sent: &BTreeMap<Key, u64>,
    leaves: &[(GridId, BrickCube)],
) -> VecDeque<(Key, Option<u64>)> {
    let mut wanted: Vec<Key> = leaves
        .iter()
        .map(|&(grid, cube)| key_of(grid, cube))
        .filter(|key| {
            overrides
                .get(key)
                .is_some_and(|stored| sent.get(key) != Some(&stored.version))
        })
        .collect();
    wanted.sort_by_key(|key| std::cmp::Reverse(key.0));
    sent.keys()
        .filter(|key| !overrides.contains_key(key))
        .map(|key| (*key, None))
        .chain(
            wanted
                .into_iter()
                .map(|key| (key, Some(overrides[&key].version))),
        )
        .collect()
}

/// The brick a client at `position` in `level` is in, in the root grid of the level and in each planet of it (the grids that have nodes), in each grid's own
/// coordinates. A planet that is turned or moves puts the client in another brick of it as it goes.
fn viewer_bricks(
    config: &CoarseConfig,
    data: &WorldData,
    level: LevelIndex,
    position: WorldPosition,
) -> Vec<(GridId, BrickCoordinate)> {
    let mut bricks = Vec::new();
    if keeps_level(config, data, level) {
        bricks.push((GridId::root(level), position.brick()));
    }
    bricks.extend(data.grids().iter().filter_map(|(id, ship)| {
        (ship.level == level && ship.terrain.is_some())
            .then(|| ship.pose.world_to_local(position))
            .flatten()
            .map(|local| (id, local.brick()))
    }));
    bricks
}

/// The cubes of detail level 1 or more that a client in `brick` of `grid` is to draw and that have an override in them: the selection with the bricks it
/// holds left as bricks, followed only to where there is an override. A cube without one is sent nothing, so whether the client draws it (it does not draw a
/// cube that is empty or buried) does not matter here, and the terrain is not asked. What it does draw of these is the client's to work out: it adds the
/// cubes of an override to those it keeps as it is told of them.
fn select_leaves(
    config: &CoarseConfig,
    edited: &EditedCubes,
    grid: GridId,
    brick: BrickCoordinate,
    detail: &SetDetail,
) -> Vec<(GridId, BrickCube)> {
    let view = ViewSelection {
        detail: detail.settings(),
        zoom: detail.zoom(),
        fine_radius: u32::from(detail.fine_radius),
        maximum_distance: config.view_distance,
    };
    let may_hold = |cube: BrickCube| edited.holds(grid, cube);
    select_around_brick(brick, &view, &may_hold)
        .into_iter()
        .filter(|cube| cube.level() >= 1)
        .map(|cube| (grid, cube))
        .collect()
}

/// The plugin: register it with the game's plugins, with a configuration from the game, and it depends on `player_presence:players`.
#[derive(Debug)]
pub struct CoarseServerPlugin {
    config: Arc<CoarseConfig>,
    state: Arc<Mutex<State>>,
}

impl std::fmt::Debug for State {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("State")
            .field("overrides", &self.overrides.len())
            .field("dirty", &self.dirty.len())
            .finish_non_exhaustive()
    }
}

/// A way to look at what a plugin holds after it has been given to the host (for tests and statistics).
#[derive(Clone, Debug)]
pub struct CoarseHandle {
    state: Arc<Mutex<State>>,
}

impl CoarseHandle {
    /// How many overrides there are and how many nodes are waiting to be remade (dirty, or with the worker).
    pub fn counts(&self) -> (usize, usize) {
        let state = locked(&self.state);
        (state.overrides.len(), state.waiting())
    }

    /// Whether the worker is making a node it was handed (the node is kept in the next tick after it is done).
    pub fn worker_is_busy(&self) -> bool {
        locked(&self.state)
            .worker
            .as_ref()
            .is_some_and(Worker::is_busy)
    }

    /// How many clients have said how they want things drawn and have not left.
    pub fn client_count(&self) -> usize {
        locked(&self.state).clients.len()
    }

    /// How many different versions of nodes have ever been made: the same as the number of overrides when each node was made once.
    pub fn versions_made(&self) -> u64 {
        locked(&self.state).next_version
    }

    /// The override of a node, if it has one, with its version.
    pub fn override_of(&self, grid: GridId, cube: BrickCube) -> Option<(CoarseNode, u64)> {
        locked(&self.state)
            .overrides
            .get(&key_of(grid, cube))
            .map(|stored| (stored.node.clone(), stored.version))
    }
}

impl CoarseServerPlugin {
    pub fn new(config: CoarseConfig) -> Self {
        Self {
            config: Arc::new(config),
            state: Arc::new(Mutex::new(State::default())),
        }
    }

    /// A handle to look at the state with, which stays good when the plugin is given to the host.
    pub fn handle(&self) -> CoarseHandle {
        CoarseHandle {
            state: Arc::clone(&self.state),
        }
    }
}

impl Plugin for CoarseServerPlugin {
    fn id(&self) -> NamespacedId {
        "engine:coarse_streaming"
            .parse()
            .expect("constant identifier is valid")
    }

    fn depends_on(&self) -> Vec<NamespacedId> {
        vec![
            "player_presence:players"
                .parse()
                .expect("constant identifier is valid"),
        ]
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        let config = Arc::clone(&self.config);
        // What was edited before the server started is not known to be so: every edited brick is marked, and the nodes over it are made in time.
        {
            let mut state = locked(&self.state);
            for (index, _, level) in registrar.world().levels() {
                if !keeps_level(&config, registrar.world(), index) {
                    continue;
                }
                let grid = GridId::root(index);
                for (brick, _) in level.overrides.bricks() {
                    state.mark_brick(grid, brick, config.maximum_override_level);
                }
            }
            // (A planet that is in a file is told of when it comes out, as a change of every voxel it holds.)
            for (grid, ship) in registrar.world().grids().iter() {
                if ship.terrain.is_some() {
                    for (brick, _) in ship.voxels.bricks() {
                        state.mark_brick(grid, brick, config.maximum_override_level);
                    }
                }
            }
        }
        registrar.declares_message::<CoarseNodes>();

        let detail_state = Arc::clone(&self.state);
        registrar.message(move |_, client, detail: SetDetail| {
            let mut state = locked(&detail_state);
            match state.clients.get_mut(&client) {
                Some(known) => known.detail = detail,
                None => {
                    state.clients.insert(
                        client,
                        ClientState {
                            detail,
                            selected_from: None,
                            selected_for: None,
                            leaves: Vec::new(),
                            selected_edit_epoch: 0,
                            sent: BTreeMap::new(),
                            pending: VecDeque::new(),
                            pending_epoch: u64::MAX,
                            credit: 0.0,
                        },
                    );
                }
            }
        });

        let leaving_state = Arc::clone(&self.state);
        registrar.on::<PlayerLeft>(
            HandlerSpec::new("engine:coarse_streaming_forget"),
            move |event, _| {
                locked(&leaving_state).clients.remove(&event.client);
            },
        );

        let mut channels = vec![config.block_channel];
        channels.extend(config.state_channel);
        channels.extend(config.orientation_channel);
        channels.extend(
            config
                .brightness
                .as_ref()
                .map(|brightness| brightness.channel),
        );
        let marking = Arc::clone(&self.state);
        let marking_config = Arc::clone(&self.config);
        let maximum = config.maximum_override_level;
        registrar.on_channels(
            HandlerSpec::new("engine:coarse_streaming_mark"),
            &channels,
            move |event, context| {
                if !keeps_nodes_of(&marking_config, context.world().data(), event.grid) {
                    return;
                }
                let mut state = locked(&marking);
                for voxel in &event.voxels {
                    state.mark_brick(event.grid, voxel.split().0, maximum);
                }
            },
        );

        let working = Arc::clone(&self.state);
        registrar.system(
            SystemSpec::new("engine:coarse_streaming", Phase::Rules),
            move |context| {
                let mut state = locked(&working);
                state.work(&config, context.world().data());
                let clients: Vec<ClientId> = state.clients.keys().copied().collect();
                let mut outgoing: Vec<(ClientId, Vec<u8>)> = Vec::new();
                for client in clients {
                    state.plan(&config, context.world().data(), client);
                    for payload in state.send(&config, client) {
                        outgoing.push((client, payload));
                    }
                }
                drop(state);
                for (client, payload) in outgoing {
                    context.send(Audience::Client(client), CoarseNodes::kind(), payload);
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use block_models::BlockTable;
    use voxel_space::VoxelCoordinate;
    use world_storage::test_support::{ALL_PLUGINS, Game};

    use super::*;

    fn cube(x: i128, level: u8) -> BrickCube {
        BrickCube::new(BrickCoordinate::new(x, 0, 0), level).expect("the origin is aligned")
    }

    fn stored(version: u64) -> Stored {
        Stored {
            node: CoarseNode::uniform(CoarseCell::empty(0, 0)),
            version,
        }
    }

    fn key(x: i128, level: u8) -> Key {
        key_of(GridId::MAIN, cube(x, level))
    }

    fn overrides(entries: &[(Key, u64)]) -> BTreeMap<Key, Stored> {
        entries
            .iter()
            .map(|&(key, version)| (key, stored(version)))
            .collect()
    }

    fn leaves(cubes: &[BrickCube]) -> Vec<(GridId, BrickCube)> {
        cubes.iter().map(|&cube| (GridId::MAIN, cube)).collect()
    }

    fn pending(
        overrides: &BTreeMap<Key, Stored>,
        sent: &[(Key, u64)],
        cubes: &[BrickCube],
    ) -> Vec<(Key, Option<u64>)> {
        let sent: BTreeMap<Key, u64> = sent.iter().copied().collect();
        pending_updates(overrides, &sent, &leaves(cubes)).into()
    }

    #[test]
    fn what_is_wanted_is_sent_coarsest_first_whatever_order_the_leaves_came_in() {
        let held = overrides(&[(key(2, 1), 7), (key(32, 5), 8), (key(8, 3), 9)]);
        let order = pending(&held, &[], &[cube(2, 1), cube(32, 5), cube(8, 3)]);
        assert_eq!(
            order,
            [
                (key(32, 5), Some(8)),
                (key(8, 3), Some(9)),
                (key(2, 1), Some(7))
            ]
        );
    }

    #[test]
    fn leaves_of_one_level_stay_in_the_order_the_walk_gave_them() {
        let held = overrides(&[(key(0, 2), 1), (key(4, 2), 2), (key(8, 2), 3)]);
        let order = pending(&held, &[], &[cube(4, 2), cube(8, 2), cube(0, 2)]);
        assert_eq!(
            order,
            [
                (key(4, 2), Some(2)),
                (key(8, 2), Some(3)),
                (key(0, 2), Some(1))
            ]
        );
    }

    #[test]
    fn a_node_the_client_has_in_its_latest_version_is_not_sent_again_and_one_it_has_in_an_older_version_is()
     {
        let held = overrides(&[(key(32, 5), 8), (key(8, 3), 9)]);
        let order = pending(
            &held,
            &[(key(32, 5), 8), (key(8, 3), 4)],
            &[cube(32, 5), cube(8, 3)],
        );
        assert_eq!(order, [(key(8, 3), Some(9))]);
    }

    #[test]
    fn only_leaves_with_an_override_are_wanted_and_an_override_off_the_leaves_is_not() {
        let held = overrides(&[(key(32, 5), 8), (key(8, 3), 9)]);
        // 32 at level 5 is not among the leaves (they split it); 16 at level 3 has no override.
        let order = pending(&held, &[], &[cube(8, 3), cube(16, 3)]);
        assert_eq!(order, [(key(8, 3), Some(9))]);
    }

    #[test]
    fn a_node_that_was_sent_and_is_no_longer_overridden_is_taken_back_before_anything_new_is_sent()
    {
        let held = overrides(&[(key(32, 5), 8)]);
        // The removed one is not among the leaves any more either: it is taken back all the same.
        let order = pending(
            &held,
            &[(key(0, 2), 1), (key(4, 1), 1)],
            &[cube(32, 5), cube(8, 3)],
        );
        // Which removal comes first does not matter, only that they come before what is new.
        let (removals, news) = order.split_at(2);
        let mut removals = removals.to_vec();
        removals.sort();
        assert_eq!(removals, [(key(4, 1), None), (key(0, 2), None)]);
        assert_eq!(news, [(key(32, 5), Some(8))]);
    }

    #[test]
    fn nodes_of_other_grids_are_told_apart() {
        let other = GridId::root(LevelIndex::new(1));
        let held: BTreeMap<Key, Stored> = [
            (key_of(GridId::MAIN, cube(0, 3)), stored(1)),
            (key_of(other, cube(0, 3)), stored(2)),
        ]
        .into_iter()
        .collect();
        let sent: BTreeMap<Key, u64> = [(key_of(GridId::MAIN, cube(0, 3)), 1)].into();
        let wanted = [(GridId::MAIN, cube(0, 3)), (other, cube(0, 3))];
        let order: Vec<_> = pending_updates(&held, &sent, &wanted).into();
        assert_eq!(order, [(key_of(other, cube(0, 3)), Some(2))]);
    }

    fn config() -> CoarseConfig {
        let layout = voxel_grid::GridLayout::new(vec![voxel_grid::ChannelDefinition::number(
            "core:block".parse().unwrap(),
            0,
        )])
        .unwrap();
        let channel = layout
            .channel_index(&"core:block".parse().unwrap())
            .unwrap();
        CoarseConfig::new(
            BlockTable::of_cubes(&[false, true]),
            0,
            channel,
            0,
            20,
            Arc::new(Mutex::new(player_presence::Presence::new())),
        )
    }

    fn client_state(pending: Vec<(Key, Option<u64>)>, sent: Vec<(Key, u64)>) -> ClientState {
        ClientState {
            detail: SetDetail {
                cell_pixels: 8.0,
                pixel_size: 0.001,
                bytes_per_second: 1_000_000,
                fine_radius: 2,
                zoom: None,
            },
            selected_from: None,
            selected_for: None,
            leaves: Vec::new(),
            selected_edit_epoch: 0,
            sent: sent.into_iter().collect(),
            pending: pending.into(),
            pending_epoch: 0,
            credit: 0.0,
        }
    }

    #[test]
    fn the_places_with_an_override_are_kept_in_step_with_the_overrides_and_counted_when_they_change()
     {
        let mut state = State::default();
        let first = key(0, 1);
        let beside = key(2, 1);
        state.store(first, stored(1));
        assert_eq!(state.edit_epoch, 1, "a place with its first override");
        assert!(state.edited.holds(GridId::MAIN, cube(0, 1)));
        assert!(
            state.edited.holds(GridId::MAIN, cube(0, 2)),
            "the cube that holds it"
        );
        // The same cube made again is not a new place, and neither is another in a cube that already holds one.
        state.store(first, stored(2));
        assert_eq!(state.edit_epoch, 1);
        assert_eq!(state.overrides[&first].version, 2);
        state.store(beside, stored(3));
        assert_eq!(state.edit_epoch, 2, "its own cube had none");
        state.store(key(0, 2), stored(4));
        assert_eq!(
            state.edit_epoch, 2,
            "the cube of level 2 already held an override"
        );
        // Taking away what was never there changes nothing, even for a cube that has others in it.
        state.forget(key(8, 1));
        assert_eq!(state.edit_epoch, 2);
        state.forget(key(0, 3));
        assert_eq!(state.edit_epoch, 2);
        assert!(
            state.edited.holds(GridId::MAIN, cube(0, 3)),
            "the overrides in it are still noted"
        );
        // The last override of a place is the end of it, and not the one before.
        state.forget(first);
        assert_eq!(state.edit_epoch, 3);
        assert!(!state.edited.holds(GridId::MAIN, cube(0, 1)));
        assert!(
            state.edited.holds(GridId::MAIN, cube(0, 2)),
            "others are in it"
        );
        state.forget(beside);
        state.forget(key(0, 2));
        assert_eq!(state.edit_epoch, 5);
        assert!(state.edited.is_empty());
        assert!(state.overrides.is_empty());
        // A cube that holds an override but has none of its own is not an override to take back.
        let mut other = State::default();
        other.store(key(64, 1), stored(1));
        other.forget(key(64, 2));
        assert_eq!(other.edit_epoch, 1);
        assert!(other.edited.holds(GridId::MAIN, cube(64, 2)));
        assert!(other.overrides.contains_key(&key(64, 1)));
    }

    fn at(x: i128) -> WorldPosition {
        WorldPosition::new(BrickCoordinate::new(x, 0, 0), [16.0; 3])
    }

    #[test]
    fn the_leaves_are_those_of_the_selection_with_an_override_in_them_and_the_bricks_held_are_not_among_them()
     {
        let config = config();
        let detail = SetDetail {
            cell_pixels: 8.0,
            pixel_size: 0.001,
            bytes_per_second: 1_000_000,
            fine_radius: 2,
            zoom: None,
        };
        let mut edited = EditedCubes::new();
        let none = select_leaves(&config, &edited, GridId::MAIN, at(100).brick(), &detail);
        assert!(none.is_empty(), "no override, no leaf: {none:?}");

        // An override in the middle distance and one far off, and one in the ball of bricks the client holds.
        edited.add(GridId::MAIN, cube(160, 1));
        edited.add(GridId::MAIN, cube(5_000, 1));
        edited.add(GridId::MAIN, cube(100, 1));
        let leaves = select_leaves(&config, &edited, GridId::MAIN, at(100).brick(), &detail);
        assert!(
            leaves
                .iter()
                .all(|(grid, cube)| *grid == GridId::MAIN && cube.level() >= 1)
        );
        // Each override is in exactly one leaf; and every leaf has one in it.
        let leaf_of = |place: BrickCube| -> Vec<BrickCube> {
            leaves
                .iter()
                .map(|(_, leaf)| *leaf)
                .filter(|leaf| leaf.contains(place.origin()))
                .collect()
        };
        for place in [cube(160, 1), cube(5_000, 1)] {
            assert_eq!(leaf_of(place).len(), 1, "{place:?} in {leaves:?}");
        }
        assert!(
            leaves
                .iter()
                .all(|(_, leaf)| edited.holds(GridId::MAIN, *leaf))
        );
        // The far one is in a bigger cube than the near one: the rule is coarser with distance.
        assert!(leaf_of(cube(5_000, 1))[0].level() > leaf_of(cube(160, 1))[0].level());
        // What is in the ball of bricks the client holds is bricks, and is sent no node.
        assert!(leaf_of(cube(100, 1)).is_empty());

        // The ones that are too far are not leaves: the distance limit of the configuration.
        let mut near_sighted = config.clone();
        near_sighted.view_distance = 1_000.0;
        let near_leaves = select_leaves(
            &near_sighted,
            &edited,
            GridId::MAIN,
            at(100).brick(),
            &detail,
        );
        assert!(
            near_leaves
                .iter()
                .all(|(_, leaf)| leaf.origin().x() < 1_000)
        );
        assert!(near_leaves.len() < leaves.len());
        // And not in another level.
        let other = select_leaves(
            &config,
            &edited,
            GridId::root(LevelIndex::new(1)),
            at(100).brick(),
            &detail,
        );
        assert!(other.is_empty());
    }

    #[test]
    fn marking_a_brick_marks_its_cube_and_the_cubes_of_the_bricks_that_touch_it() {
        let mut state = State::default();
        state.mark_brick(GridId::MAIN, BrickCoordinate::new(0, 0, 0), 12);
        // The cubes of 2 bricks: the one that holds the brick, and the three beside it that hold the bricks on its negative sides (the ones on its
        // positive sides are in the same cube).
        let marked: Vec<Key> = state.dirty.iter().copied().collect();
        let mut expected = vec![
            key_of(
                GridId::MAIN,
                BrickCube::containing(BrickCoordinate::new(0, 0, 0), 1),
            ),
            key_of(
                GridId::MAIN,
                BrickCube::containing(BrickCoordinate::new(-1, 0, 0), 1),
            ),
            key_of(
                GridId::MAIN,
                BrickCube::containing(BrickCoordinate::new(0, -1, 0), 1),
            ),
            key_of(
                GridId::MAIN,
                BrickCube::containing(BrickCoordinate::new(0, 0, -1), 1),
            ),
        ];
        expected.sort();
        assert_eq!(marked, expected);
    }

    #[test]
    fn a_brick_at_the_edge_of_its_cube_marks_the_cubes_on_both_sides() {
        let mut state = State::default();
        state.mark_brick(GridId::MAIN, BrickCoordinate::new(1, 1, 1), 12);
        let origins: Vec<[i128; 3]> = state.dirty.iter().map(|key| key.2).collect();
        assert_eq!(origins, [[0, 0, 0], [0, 0, 2], [0, 2, 0], [2, 0, 0]]);
    }

    #[test]
    fn nothing_is_marked_when_there_are_no_overrides_to_keep() {
        let mut state = State::default();
        state.mark_brick(GridId::MAIN, BrickCoordinate::ORIGIN, 0);
        assert!(state.dirty.is_empty());
    }

    #[test]
    fn nodes_are_kept_for_the_root_grids_of_levels_and_for_planets_and_for_no_other_grid() {
        let game = Game::new(&ALL_PLUGINS);
        let mut world = game.new_world();
        let made = game.sample_ship(&world, 0);
        let ship = world.grids_mut().create(made);
        let made = game.sample_planet(&world, 0);
        let planet = world.grids_mut().create(made);
        assert!(keeps_nodes_of(&config(), &world, GridId::MAIN));
        assert!(keeps_nodes_of(&config(), &world, planet));
        assert!(
            !keeps_nodes_of(&config(), &world, ship),
            "a ship nobody generates has no nodes yet"
        );
        // (A grid that is not there has none.)
        assert!(!keeps_nodes_of(
            &config(),
            &world,
            GridId::new(GridId::FIRST_ALLOCATED + 100)
        ));
    }

    #[test]
    fn a_game_can_keep_the_nodes_of_some_levels_and_every_planet() {
        let game = Game::new(&ALL_PLUGINS);
        let mut world = game.new_world();
        world
            .add_level(
                "test_content:second".parse().unwrap(),
                game.new_level("base_terrain:flat_v1"),
            )
            .unwrap();
        let made = game.sample_planet(&world, 0);
        let planet = world.grids_mut().create(made);
        let mut config = config();
        config.levels = Some(["test_content:second".parse().unwrap()].into());
        let second = GridId::root(LevelIndex::new(1));
        assert!(!keeps_nodes_of(&config, &world, GridId::MAIN));
        assert!(keeps_nodes_of(&config, &world, second));
        assert!(
            keeps_nodes_of(&config, &world, planet),
            "planets are kept in any case"
        );
        // A client in a level that is not kept is sent the nodes of its planets and of nothing else.
        let viewer = WorldPosition::new(BrickCoordinate::new(3, 25, 4), [5.0; 3]);
        let grids: Vec<GridId> = viewer_bricks(&config, &world, LevelIndex::MAIN, viewer)
            .into_iter()
            .map(|(grid, _)| grid)
            .collect();
        assert_eq!(grids, [planet]);
    }

    #[test]
    fn a_client_is_in_a_brick_of_each_planet_of_its_level_in_the_planets_own_coordinates() {
        let game = Game::new(&ALL_PLUGINS);
        let mut world = game.new_world();
        let mut made = game.sample_planet(&world, 0);
        made.pose = voxel_space::GridPose::new(
            WorldPosition::new(BrickCoordinate::new(0, 20, 0), [0.0; 3]),
            voxel_space::Orientation::IDENTITY,
        );
        let planet = world.grids_mut().create(made);
        let ship = game.sample_ship(&world, 0);
        world.grids_mut().create(ship);
        let mut elsewhere = game.sample_planet(&world, 0);
        elsewhere.level = LevelIndex::new(1);
        world.grids_mut().create(elsewhere);

        let viewer = WorldPosition::new(BrickCoordinate::new(3, 25, 4), [5.0; 3]);
        let bricks = viewer_bricks(&config(), &world, LevelIndex::MAIN, viewer);
        // The root grid of the level, in the level's coordinates, and the one planet of the level, in its own (20 bricks lower); no ship, no planet of another level.
        assert_eq!(
            bricks,
            [
                (GridId::MAIN, BrickCoordinate::new(3, 25, 4)),
                (planet, BrickCoordinate::new(3, 5, 4)),
            ]
        );
    }

    #[test]
    fn the_node_of_a_planet_is_what_its_generator_gives_with_the_cells_of_a_stored_brick_patched_in()
     {
        let game = Game::new(&ALL_PLUGINS);
        let mut world = game.new_world();
        let made = game.sample_planet(&world, 0);
        let planet = world.grids_mut().create(made);
        let layout = Arc::clone(world.main().overrides.layout());
        let block = layout
            .channel_index(&"core:block".parse().unwrap())
            .unwrap();
        let air = game.raw_block("base_blocks:air");
        let mut opaque = vec![true; 16];
        opaque[air as usize] = false;
        let config = CoarseConfig::new(
            BlockTable::of_cubes(&opaque),
            air,
            block,
            0,
            20,
            Arc::new(Mutex::new(player_presence::Presence::new())),
        );
        let ship = world.grids().get(planet).unwrap();
        let (stored, _) = ship
            .voxels
            .bricks()
            .next()
            .expect("the sample planet has stored bricks");
        let generator = Arc::clone(
            world
                .main()
                .terrain
                .generators()
                .entry(ship.terrain.unwrap().generator)
                .unwrap()
                .content()
                .unwrap(),
        );

        let cube = BrickCube::containing(stored, 1);
        let node = remake(&config, &BTreeMap::new(), &world, planet, cube)
            .expect("a cube with a stored brick has a node of its own");
        let generated = generator
            .coarse_node(cube, 0)
            .expect("the generator makes nodes of its extent");
        let (node, generated) = (node.without_exposure(), generated.without_exposure());
        let (same, different) = node.cells().zip(generated.cells()).fold(
            (0, 0),
            |(same, different), (made, generated)| {
                if made == generated {
                    (same + 1, different)
                } else {
                    (same, different + 1)
                }
            },
        );
        // A stored brick is one eighth of the cube; the rest of the node is the generator's, exactly.
        assert!(different > 0, "the stored brick is in the node");
        let stored_in_the_cube = ship
            .voxels
            .bricks()
            .filter(|(brick, _)| BrickCube::containing(*brick, 1) == cube)
            .count();
        assert!(
            different <= stored_in_the_cube * 16 * 16 * 16,
            "{different} cells differ, in more than the {stored_in_the_cube} stored bricks"
        );
        assert_eq!(same + different, 32 * 32 * 32);
        // And a cube with no stored brick in it has none of its own.
        let empty = BrickCube::containing(
            BrickCoordinate::new(stored.x() + 40, stored.y(), stored.z()),
            1,
        );
        assert!(remake(&config, &BTreeMap::new(), &world, planet, empty).is_none());
    }

    #[test]
    fn the_bricks_of_a_planet_are_the_stored_ones_and_what_its_generator_makes_inside_its_extent_and_nothing_outside_it()
     {
        let game = Game::new(&ALL_PLUGINS);
        let mut world = game.new_world();
        let made = game.sample_planet(&world, 0);
        let planet = world.grids_mut().create(made);
        let bricks = GridBricks::of(&world, planet).expect("the planet has a generator");
        let extent = world.grids().get(planet).unwrap().terrain.unwrap().extent;
        let inside = extent.min();
        let outside =
            BrickCoordinate::new(extent.max().x() + 1, extent.min().y(), extent.min().z());
        let generator = Arc::clone(
            world
                .main()
                .terrain
                .generators()
                .entry(
                    world
                        .grids()
                        .get(planet)
                        .unwrap()
                        .terrain
                        .unwrap()
                        .generator,
                )
                .unwrap()
                .content()
                .unwrap(),
        );
        let layout = Arc::clone(world.grids().get(planet).unwrap().voxels.layout());
        assert!(
            matches!(bricks.effective(inside), BrickSource::Generated(brick) if brick == generator.generate_brick(&layout, inside)),
            "inside the extent the bricks are the generator's"
        );
        assert!(
            matches!(bricks.effective(outside), BrickSource::Generated(brick) if brick == Brick::new(&Arc::clone(world.grids().get(planet).unwrap().voxels.layout())))
        );
        // The sample planet has stored bricks (its edits): those are what the game sees where they are.
        let stored: Vec<BrickCoordinate> = world
            .grids()
            .get(planet)
            .unwrap()
            .voxels
            .bricks()
            .map(|(brick, _)| brick)
            .collect();
        assert!(!stored.is_empty());
        for brick in stored {
            assert!(bricks.stored(brick).is_some());
            assert!(matches!(bricks.effective(brick), BrickSource::Edited(_)));
        }
        // A ship nobody generates has no nodes, so no bricks to read them from.
        let made = game.sample_ship(&world, 0);
        let ship = world.grids_mut().create(made);
        assert!(GridBricks::of(&world, ship).is_none());
    }

    /// A generator that gives what another gives, but whose coarse cells are not given until the test lets it: how long a slow generator keeps a tick
    /// waiting is shown by a generator that does not answer at all, which needs no clock.
    #[derive(Debug)]
    struct Gated {
        inner: Arc<dyn TerrainGenerator>,
        gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
        asked: Arc<std::sync::atomic::AtomicUsize>,
        gave_up: Arc<std::sync::atomic::AtomicBool>,
    }

    impl TerrainGenerator for Gated {
        fn generate_brick(&self, layout: &voxel_grid::GridLayout, at: BrickCoordinate) -> Brick {
            self.inner.generate_brick(layout, at)
        }

        fn coarse_node(&self, cube: BrickCube, unlit: u8) -> Option<CoarseNode> {
            self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let (lock, opened) = &*self.gate;
            let guard = lock.lock().unwrap();
            // A deadline so that a tick that waits for this ends the test with a failure instead of hanging it; nothing passes or fails by how long it is.
            let (_guard, timeout) = opened
                .wait_timeout_while(guard, std::time::Duration::from_secs(10), |open| !*open)
                .unwrap();
            if timeout.timed_out() {
                self.gave_up
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            }
            self.inner.coarse_node(cube, unlit)
        }
    }

    /// A generator that panics when it is asked for coarse cells.
    #[derive(Debug)]
    struct Panicking;

    impl TerrainGenerator for Panicking {
        fn generate_brick(&self, layout: &voxel_grid::GridLayout, _: BrickCoordinate) -> Brick {
            Brick::new(layout)
        }

        fn coarse_node(&self, _: BrickCube, _: u8) -> Option<CoarseNode> {
            panic!("a generator that cannot make a node");
        }
    }

    /// What the plugin's system does each tick for the nodes.
    fn work_a_tick(state: &mut State, config: &Arc<CoarseConfig>, world: &WorldData) {
        state.work(config, world);
    }

    /// A configuration over the blocks of the sample game, as the plugin would be given.
    fn config_for(game: &Game, world: &WorldData) -> Arc<CoarseConfig> {
        let block = world
            .main()
            .overrides
            .layout()
            .channel_index(&"core:block".parse().unwrap())
            .unwrap();
        let air = game.raw_block("base_blocks:air");
        let mut opaque = vec![true; 16];
        opaque[air as usize] = false;
        Arc::new(CoarseConfig::new(
            BlockTable::of_cubes(&opaque),
            air,
            block,
            0,
            20,
            Arc::new(Mutex::new(player_presence::Presence::new())),
        ))
    }

    struct GatedPlanet {
        world: WorldData,
        config: Arc<CoarseConfig>,
        planet: GridId,
        open: Arc<(Mutex<bool>, std::sync::Condvar)>,
        asked: Arc<std::sync::atomic::AtomicUsize>,
        gave_up: Arc<std::sync::atomic::AtomicBool>,
    }

    impl GatedPlanet {
        /// A world with the sample planet, whose generator does not give coarse cells until it is let to.
        fn new() -> Self {
            Self::wrapping(|inner, open, asked, gave_up| {
                Arc::new(Gated {
                    inner,
                    gate: open,
                    asked,
                    gave_up,
                })
            })
        }

        /// A world with the sample planet, whose generator is what `wrap` makes of the sample's.
        fn wrapping(
            wrap: impl FnOnce(
                Arc<dyn TerrainGenerator>,
                Arc<(Mutex<bool>, std::sync::Condvar)>,
                Arc<std::sync::atomic::AtomicUsize>,
                Arc<std::sync::atomic::AtomicBool>,
            ) -> Arc<dyn TerrainGenerator>,
        ) -> Self {
            let game = Game::new(&ALL_PLUGINS);
            let mut world = game.new_world();
            let mut made = game.sample_planet(&world, 0);
            let mut terrain = made.terrain.expect("a planet has terrain");
            let inner = Arc::clone(
                world
                    .main()
                    .terrain
                    .generators()
                    .entry(terrain.generator)
                    .unwrap()
                    .content()
                    .unwrap(),
            );
            let open = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
            let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let gave_up = Arc::new(std::sync::atomic::AtomicBool::new(false));
            terrain.generator = world
                .main_mut()
                .terrain
                .register_generator(
                    "test_content:gated".parse().unwrap(),
                    wrap(
                        inner,
                        Arc::clone(&open),
                        Arc::clone(&asked),
                        Arc::clone(&gave_up),
                    ),
                )
                .unwrap();
            made.terrain = Some(terrain);
            let planet = world.grids_mut().create(made);
            let config = config_for(&game, &world);
            Self {
                world,
                config,
                planet,
                open,
                asked,
                gave_up,
            }
        }

        /// The nodes of the planet's stored bricks made on the spot, finest first, one after the other, as the tick did before there was a worker.
        fn settled_on_the_spot(&self) -> Vec<(Key, CoarseNode)> {
            let mut spot = self.state_with_the_stored_bricks_marked();
            while let Some(key) = spot.dirty.pop_first() {
                let (cube, grid) = (cube_of(key), key.1);
                match remake(&self.config, &spot.overrides, &self.world, grid, cube) {
                    Some(node) => {
                        spot.store(key, Stored { node, version: 0 });
                        if cube.level() < self.config.maximum_override_level {
                            spot.dirty.insert(key_of(
                                grid,
                                BrickCube::containing(cube.origin(), cube.level() + 1),
                            ));
                        }
                    }
                    None => spot.forget(key),
                }
            }
            spot.overrides
                .iter()
                .map(|(key, stored)| (*key, stored.node.clone()))
                .collect()
        }

        fn close_the_gate(&self) {
            *self.open.0.lock().unwrap() = false;
        }

        fn open_the_gate(&self) {
            *self.open.0.lock().unwrap() = true;
            self.open.1.notify_all();
        }

        fn asked(&self) -> usize {
            self.asked.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn state_with_the_stored_bricks_marked(&self) -> State {
            let mut state = State::default();
            for (brick, _) in self.world.grids().get(self.planet).unwrap().voxels.bricks() {
                state.mark_brick(self.planet, brick, self.config.maximum_override_level);
            }
            assert!(!state.dirty.is_empty());
            state
        }
    }

    /// Works ticks until every node is made, waiting for the worker with a deadline.
    fn work_until_done(state: &mut State, config: &Arc<CoarseConfig>, world: &WorldData) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while state.waiting() > 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "the nodes are not made"
            );
            work_a_tick(state, config, world);
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_tick_does_not_wait_for_a_generator_that_has_not_answered() {
        let planet = GatedPlanet::new();
        let mut state = planet.state_with_the_stored_bricks_marked();
        for _ in 0..5 {
            work_a_tick(&mut state, &planet.config, &planet.world);
        }
        assert!(
            !planet.gave_up.load(std::sync::atomic::Ordering::SeqCst),
            "a tick waited for the generator"
        );
        // The generator was asked (by the worker, which is waiting for it) and nothing came of it yet.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while planet.asked() == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "the generator was never asked"
            );
            work_a_tick(&mut state, &planet.config, &planet.world);
            std::thread::yield_now();
        }
        assert!(state.overrides.is_empty());
        assert!(state.waiting() > 0);
        // Let to answer, the nodes come.
        planet.open_the_gate();
        work_until_done(&mut state, &planet.config, &planet.world);
        assert!(!state.overrides.is_empty());
        assert!(
            !planet.gave_up.load(std::sync::atomic::Ordering::SeqCst),
            "the generator gave up waiting"
        );
    }

    #[test]
    fn the_nodes_a_worker_makes_are_the_nodes_made_on_the_spot_and_each_is_made_from_the_nodes_it_has_at_the_end()
     {
        let planet = GatedPlanet::new();
        planet.open_the_gate();
        let mut worked = planet.state_with_the_stored_bricks_marked();
        work_until_done(&mut worked, &planet.config, &planet.world);

        let spot = planet.settled_on_the_spot();

        let nodes = |state: &State| -> Vec<(Key, CoarseNode)> {
            state
                .overrides
                .iter()
                .map(|(key, stored)| (*key, stored.node.clone()))
                .collect()
        };
        assert!(
            nodes(&worked).len() >= 12,
            "a planet's stored bricks have nodes at every level"
        );
        assert_eq!(nodes(&worked), spot);
    }

    #[test]
    fn a_node_that_is_edited_again_while_the_worker_has_it_is_made_again_from_what_is_there_then() {
        let mut planet = GatedPlanet::new();
        planet.open_the_gate();
        let before = planet.settled_on_the_spot();
        planet.close_the_gate();
        let asked_before = planet.asked();
        let mut state = planet.state_with_the_stored_bricks_marked();
        // The worker has been given the nodes, and waits for the generator.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while planet.asked() == asked_before {
            assert!(
                std::time::Instant::now() < deadline,
                "the generator was never asked"
            );
            work_a_tick(&mut state, &planet.config, &planet.world);
            std::thread::yield_now();
        }
        // An edit comes to a stored brick: the nodes it is in were handed over as they were.
        let brick = planet
            .world
            .grids()
            .get(planet.planet)
            .unwrap()
            .voxels
            .bricks()
            .next()
            .map(|(brick, _)| brick)
            .unwrap();
        let block = planet.config.block_channel;
        let air = planet.config.air;
        let ship = planet.world.grids_mut().get_mut(planet.planet).unwrap();
        for x in 0..16 {
            for y in 0..16 {
                for z in 0..16 {
                    let voxel = VoxelCoordinate::new(
                        brick.x() * 32 + x,
                        brick.y() * 32 + y,
                        brick.z() * 32 + z,
                    );
                    let value = if ship.voxels.get(block, voxel) == air {
                        1
                    } else {
                        air
                    };
                    ship.voxels.set(block, voxel, value);
                }
            }
        }
        assert!(
            !state.in_flight.is_empty(),
            "the node is with the worker when the edit comes"
        );
        state.mark_brick(planet.planet, brick, planet.config.maximum_override_level);
        planet.open_the_gate();
        work_until_done(&mut state, &planet.config, &planet.world);

        let after = planet.settled_on_the_spot();
        assert_ne!(before, after, "the edit changes the nodes");
        let nodes: Vec<(Key, CoarseNode)> = state
            .overrides
            .iter()
            .map(|(key, stored)| (*key, stored.node.clone()))
            .collect();
        assert_eq!(
            nodes, after,
            "what the worker had was made from the brick as it was before"
        );
    }

    #[test]
    #[should_panic(expected = "ended")]
    fn a_generator_that_panics_in_the_worker_ends_the_server_in_a_later_tick_and_is_not_forgotten()
    {
        let planet = GatedPlanet::wrapping(|_, _, _, _| Arc::new(Panicking));
        let mut state = planet.state_with_the_stored_bricks_marked();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            work_a_tick(&mut state, &planet.config, &planet.world);
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_node_is_not_handed_over_while_the_worker_has_it_and_waits_for_the_nodes_it_is_made_from() {
        let planet = GatedPlanet::new();
        let asked_before = planet.asked();
        let mut state = planet.state_with_the_stored_bricks_marked();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while planet.asked() == asked_before {
            assert!(
                std::time::Instant::now() < deadline,
                "the generator was never asked"
            );
            work_a_tick(&mut state, &planet.config, &planet.world);
            std::thread::yield_now();
        }
        let in_hand: Vec<Key> = state.in_flight.iter().copied().collect();
        assert!(!in_hand.is_empty());
        // Edited again, and ticks go by with the generator still not answering: what the worker has is not handed over a second time, and what is made from it
        // (a node of a coarser level, dirty or not) waits.
        for key in &in_hand {
            state.dirty.insert(*key);
            state.dirty.insert(key_of(
                key.1,
                BrickCube::containing(cube_of(*key).origin(), key.0 + 1),
            ));
        }
        for _ in 0..5 {
            work_a_tick(&mut state, &planet.config, &planet.world);
        }
        assert_eq!(
            state.worker.as_ref().map(|worker| worker.handed),
            Some(in_hand.len() as u64),
            "each node was handed over once"
        );
        assert!(in_hand.iter().all(|key| state.dirty.contains(key)));
        planet.open_the_gate();
        work_until_done(&mut state, &planet.config, &planet.world);
    }

    #[test]
    fn a_node_waits_while_one_of_its_halves_is_dirty_or_with_the_worker_and_a_node_of_the_finest_level_does_not()
     {
        let mut state = State::default();
        let parent = key_of(
            GridId::MAIN,
            BrickCube::new(BrickCoordinate::new(0, 0, 0), 2).unwrap(),
        );
        let half = |x: i128, grid: GridId| {
            key_of(
                grid,
                BrickCube::new(BrickCoordinate::new(x, 0, 0), 1).unwrap(),
            )
        };
        assert!(!state.waits_for_its_halves(parent));
        state.dirty.insert(half(2, GridId::MAIN));
        assert!(state.waits_for_its_halves(parent), "a half is dirty");
        state.dirty.clear();
        state.in_flight.insert(half(0, GridId::MAIN));
        assert!(
            state.waits_for_its_halves(parent),
            "a half is with the worker"
        );
        state.in_flight.clear();
        // The same cube of another grid, and a cube that is not a half of it, are not in the way.
        let other = GridId::new(GridId::FIRST_ALLOCATED);
        state.dirty.insert(half(0, other));
        state.dirty.insert(key_of(
            GridId::MAIN,
            BrickCube::new(BrickCoordinate::new(4, 0, 0), 1).unwrap(),
        ));
        assert!(!state.waits_for_its_halves(parent));
        // A cube of the finest level is made of bricks, which are not nodes: it waits for nothing.
        state.dirty.insert(half(0, GridId::MAIN));
        assert!(!state.waits_for_its_halves(half(0, GridId::MAIN)));
    }

    #[test]
    fn a_planet_has_the_node_of_its_generator_only_where_its_extent_is() {
        let game = Game::new(&ALL_PLUGINS);
        let mut world = game.new_world();
        let made = game.sample_planet(&world, 0);
        let planet = world.grids_mut().create(made);
        let extent = world.grids().get(planet).unwrap().terrain.unwrap().extent;
        let bricks = GridBricks::of(&world, planet).unwrap();
        let inside = BrickCube::containing(extent.min(), 1);
        let far = BrickCube::containing(
            BrickCoordinate::new(extent.max().x() + 64, extent.min().y(), extent.min().z()),
            1,
        );
        assert!(bricks.node_generator(inside).is_some());
        assert!(bricks.node_generator(far).is_none());
    }

    #[test]
    fn the_cells_of_a_stored_brick_are_made_from_the_bricks_the_game_sees_beside_it() {
        let game = Game::new(&ALL_PLUGINS);
        let mut world = game.new_world();
        let made = game.sample_planet(&world, 0);
        let planet = world.grids_mut().create(made);
        let config = config_for(&game, &world);
        let bricks = GridBricks::of(&world, planet).unwrap();
        let stored: Vec<BrickCoordinate> = world
            .grids()
            .get(planet)
            .unwrap()
            .voxels
            .bricks()
            .map(|(brick, _)| brick)
            .collect();
        let mut compared = 0;
        for at in stored {
            // What the game sees beside it (the edited or generated brick, as bricks the mesher reads), as the nodes of the first version of the server read them.
            let owned = |source: BrickSource| match source {
                BrickSource::Edited(brick) => Some(Owned::new(brick, &config)),
                BrickSource::Generated(brick) => Some(Owned::new(Arc::new(brick), &config)),
                _ => None,
            };
            let center = Owned::new(Arc::clone(bricks.stored(at).unwrap()), &config);
            let beside: Vec<Option<Owned>> = FaceDirection::ALL
                .iter()
                .map(|face| {
                    let step = face.offset();
                    at.checked_offset(step[0], step[1], step[2])
                        .and_then(|next| owned(bricks.effective(next)))
                })
                .collect();
            let expected = coarse_node_of_brick(
                &BrickNeighbourhood {
                    center: center.channels(&config),
                    neighbours: std::array::from_fn(|index| {
                        beside[index].as_ref().map(|owned| owned.channels(&config))
                    }),
                },
                &config.table,
                config.air,
            );
            let job = gather(
                &BTreeMap::new(),
                &world,
                planet,
                BrickCube::containing(at, 1),
            )
            .expect("a stored brick makes a job");
            let found = job
                .children
                .iter()
                .find_map(|child| match child {
                    Child::Brick { center, beside }
                        if Arc::ptr_eq(center, bricks.stored(at).unwrap()) =>
                    {
                        Some(job.summarize_brick(&config, center, beside))
                    }
                    _ => None,
                })
                .expect("the stored brick is a half of the job");
            assert_eq!(found, expected, "{at}");
            compared += 1;
        }
        assert!(compared > 0);
    }

    #[test]
    fn the_source_of_a_brick_is_what_the_game_sees_of_it() {
        let game = Game::new(&ALL_PLUGINS);
        let mut world = game.new_world();
        let made = game.sample_planet(&world, 0);
        let planet = world.grids_mut().create(made);
        // A level: a brick that was edited, one that is generated, and one in a region that nothing generates yet.
        world
            .main_mut()
            .overrides
            .brick_mut_or_default(BrickCoordinate::new(0, 0, 0));
        world
            .main_mut()
            .terrain
            .assign_unassigned(BrickCoordinate::new(0, 0, 0));
        let extent = world.grids().get(planet).unwrap().terrain.unwrap().extent;
        let stored = world
            .grids()
            .get(planet)
            .unwrap()
            .voxels
            .bricks()
            .next()
            .map(|(brick, _)| brick)
            .unwrap();
        let places = [
            (GridId::MAIN, BrickCoordinate::new(0, 0, 0)),
            (GridId::MAIN, BrickCoordinate::new(1, 0, 0)),
            (GridId::MAIN, BrickCoordinate::new(5000, 0, 0)),
            (planet, stored),
            (planet, extent.min()),
            (
                planet,
                BrickCoordinate::new(extent.max().x() + 1, extent.min().y(), extent.min().z()),
            ),
        ];
        let mut kinds = Vec::new();
        for (grid, at) in places {
            let bricks = GridBricks::of(&world, grid).unwrap();
            let layout = bricks.layout();
            let seen = bricks.effective(at);
            match bricks.source(at) {
                Source::Stored(brick) => {
                    assert_eq!(seen, BrickSource::Edited(brick), "{grid} {at}");
                    kinds.push("stored");
                }
                Source::Generated(generator, made_at) => {
                    assert_eq!(made_at, at);
                    assert_eq!(
                        seen,
                        BrickSource::Generated(generator.generate_brick(&layout, at)),
                        "{grid} {at}"
                    );
                    kinds.push("generated");
                }
                Source::Blank => {
                    assert_eq!(
                        seen,
                        BrickSource::Generated(Brick::new(&layout)),
                        "{grid} {at}"
                    );
                    kinds.push("blank");
                }
                Source::Nothing => {
                    assert!(
                        matches!(
                            seen,
                            BrickSource::Unassigned | BrickSource::MissingGenerator(_)
                        ),
                        "{grid} {at}: {seen:?}"
                    );
                    kinds.push("nothing");
                }
            }
        }
        for kind in ["stored", "generated", "blank", "nothing"] {
            assert!(kinds.contains(&kind), "{kind} in {kinds:?}");
        }
    }

    #[test]
    fn a_client_is_credited_a_ticks_share_of_its_rate_and_can_save_up_a_second_or_the_biggest_node()
    {
        assert_eq!(refilled(0.0, 2000, 20), 100.0);
        assert_eq!(refilled(50.0, 2000, 20), 150.0);
        assert_eq!(
            refilled(-30.0, 2000, 20),
            70.0,
            "a debt is paid off before anything is sent"
        );
        // At most a second of a rate that is over the least that is allowed to be saved...
        assert_eq!(refilled(999_990.0, 1_000_000, 20), 1_000_000.0);
        // ...and that least otherwise, so that a slow client can still be sent the biggest node.
        assert_eq!(refilled(199_999.0, 100, 20), 200_000.0);
        assert_eq!(refilled(0.0, 0, 20), 0.0);
        assert_eq!(
            refilled(0.0, 100, 0),
            100.0,
            "no ticks a second is taken as one"
        );
    }

    #[test]
    fn updates_are_put_in_messages_of_at_most_the_limit_in_order() {
        let update = |x: i128| CoarseNodeUpdate {
            grid: GridId::MAIN,
            cube: cube(x * 2, 1),
            node: None,
        };
        assert!(in_messages(Vec::new()).is_empty());
        let updates: Vec<_> = (0..(2 * MAXIMUM_UPDATES as i128 + 3)).map(update).collect();
        let messages = in_messages(updates.clone());
        let decoded: Vec<Vec<CoarseNodeUpdate>> = messages
            .iter()
            .map(|payload| CoarseNodes::decode(payload).unwrap().0)
            .collect();
        assert_eq!(
            decoded.iter().map(Vec::len).collect::<Vec<_>>(),
            [MAXIMUM_UPDATES, MAXIMUM_UPDATES, 3]
        );
        assert_eq!(decoded.concat(), updates);
        assert_eq!(in_messages(updates[..MAXIMUM_UPDATES].to_vec()).len(), 1);
    }

    #[test]
    fn a_node_that_is_no_longer_overridden_is_sent_as_taken_back_and_forgotten() {
        let config = config();
        let gone = key(8, 3);
        let kept = key(0, 2);
        let mut state = State::default();
        state.overrides.insert(kept, stored(5));
        let client = ClientId::new(1);
        state.clients.insert(
            client,
            client_state(
                VecDeque::from([(gone, None), (kept, Some(5))]).into(),
                vec![(gone, 3)],
            ),
        );
        let messages = state.send(&config, client);
        let [message] = &messages[..] else {
            panic!("{} messages", messages.len());
        };
        let updates = CoarseNodes::decode(message).unwrap().0;
        assert_eq!(updates.len(), 2);
        assert_eq!(
            (updates[0].cube, updates[0].node.clone()),
            (cube(8, 3), None)
        );
        assert_eq!(updates[1].cube, cube(0, 2));
        assert!(updates[1].node.is_some());
        let sent = &state.clients[&client].sent;
        assert_eq!(
            sent.get(&gone),
            None,
            "it is not held by the client any more"
        );
        assert_eq!(sent.get(&kept), Some(&5));
    }

    #[test]
    fn a_client_in_debt_is_sent_nothing_until_it_has_paid_it_off() {
        let config = config();
        let mut state = State::default();
        state.overrides.insert(key(0, 2), stored(1));
        let client = ClientId::new(1);
        let mut slow = client_state(vec![(key(0, 2), Some(1))], Vec::new());
        slow.detail.bytes_per_second = 20;
        slow.credit = -1000.0;
        state.clients.insert(client, slow);
        assert!(state.send(&config, client).is_empty());
        assert_eq!(state.clients[&client].pending.len(), 1, "it waits");
        assert_eq!(state.clients[&client].credit, -999.0);
    }
}
