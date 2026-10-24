//! The server half of coarse levels of detail (`docs/lod-spec.md`, 3.4 and 3.5): the coarse nodes that differ from what a client derives, kept up as the world
//! is edited, and sent to the clients that need them, coarsest first and no faster than they ask.
//!
//! A client works out the coarse cells of unedited terrain for itself from the terrain generators it was told of. Where bricks have been edited those cells are
//! wrong, so the server keeps, for each cube of bricks of detail level 1 to [`CoarseConfig::maximum_override_level`] that has an edited brick in it, an **override**:
//! the cells the generator gives for the cube, with the cells under each edited brick made from the brick itself and the cells under each half that has an override
//! made from that. The overrides are kept up by marking the cubes round an edit **dirty** and remaking a few of them each tick, finest first.
//!
//! Everything is keyed by the grid, though only the root grids of levels (terrain) are handled here: a ship's nodes are Phase 38j, and a ship with terrain
//! generators of its own (Phase 39, `lod-spec.md` 3.8) is the same keyed by its grid.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use block_models::BlockTable;
use coarse_cells::{CoarseCell, CoarseNode, EditedCubes, ViewSelection, select_around_brick};
use namespaced_id::NamespacedId;
use player_presence::{SharedPresence, lock};
use plugin_host::{HandlerSpec, Phase, PlayerLeft, Plugin, Registrar, SystemSpec};
use protocol::{ClientId, Message};
use region_octree::BrickCube;
use server::Audience;
use voxel_grid::{Brick, ChannelIndex, ChannelStorage};
use voxel_meshing::{BrickChannels, BrickNeighbourhood, coarse_node_of_brick};
use voxel_space::{
    BrickCoordinate, FaceDirection, GridId, LevelIndex, LevelOfDetail, WorldPosition,
};
use world_storage::{BrickSource, LevelData, WorldData};

use crate::messages::{CoarseNodeUpdate, CoarseNodes, MAXIMUM_UPDATES, SetDetail};

/// A node: its grid, its detail level and the origin of its cube, which is a total order that has the finest first when sorted by level.
type Key = (u8, GridId, [i64; 3]);

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
    /// How many dirty nodes are remade in a tick.
    pub nodes_per_tick: usize,
    pub presence: SharedPresence,
}

impl CoarseConfig {
    /// A configuration with the usual limits: overrides up to detail level 12 (cells of 4 kilometres), nodes chosen out to a million voxels, and 8 nodes
    /// remade in a tick.
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
            nodes_per_tick: 8,
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
    /// Where the selection was made from, and the cubes of it that have a detail level (not the bricks).
    selected_from: Option<(LevelIndex, BrickCoordinate)>,
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
        if maximum_level == 0 || grid.root_level().is_none() {
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

/// The cells of an edited brick, exactly: from the brick and from the six beside it as the game sees them (edited or generated; a region with no
/// generator is air).
fn summarize_edited_brick(
    config: &CoarseConfig,
    level: &LevelData,
    at: BrickCoordinate,
) -> Option<CoarseNode> {
    let center = Owned::new(Arc::clone(level.overrides.brick(at)?), config);
    let neighbours: Vec<Option<Owned>> = FaceDirection::ALL
        .iter()
        .map(|face| {
            let step = face.offset();
            let next = at.checked_offset(step[0], step[1], step[2])?;
            match level.effective_brick(next) {
                BrickSource::Edited(brick) => Some(Owned::new(brick, config)),
                BrickSource::Generated(brick) => Some(Owned::new(Arc::new(brick), config)),
                BrickSource::Unassigned | BrickSource::MissingGenerator(_) => None,
            }
        })
        .collect();
    Some(coarse_node_of_brick(
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
    ))
}

/// The node of a cube as the server makes it: what the generator gives for it, patched with the exact cells of every half that is known exactly (an edited
/// brick, or a node that has an override). `None` if no half is: there is nothing to say that the client does not derive.
fn remake(
    config: &CoarseConfig,
    overrides: &BTreeMap<Key, Stored>,
    data: &WorldData,
    grid: GridId,
    cube: BrickCube,
) -> Option<CoarseNode> {
    let level_data = data.level(grid.root_level()?)?;
    let parent_level = LevelOfDetail::new(cube.level())?;
    let mut node = level_data
        .terrain
        .coarse_node(cube, config.unlit)
        .unwrap_or_else(|| CoarseNode::uniform(CoarseCell::empty(config.air, config.unlit)));
    let half = (cube.edge_length() / 2) as i64;
    let origin = cube.origin();
    let mut touched = false;
    for octant in 0..8usize {
        let (dx, dy, dz) = (
            (octant & 1) as i64,
            ((octant >> 1) & 1) as i64,
            (octant >> 2) as i64,
        );
        let child_origin = BrickCoordinate::new(
            origin.x() + dx * half,
            origin.y() + dy * half,
            origin.z() + dz * half,
        );
        let child = if cube.level() == 1 {
            summarize_edited_brick(config, level_data, child_origin)
        } else {
            let child_cube =
                BrickCube::new(child_origin, cube.level() - 1).expect("half of a cube is a cube");
            overrides
                .get(&key_of(grid, child_cube))
                .map(|stored| stored.node.clone())
        };
        if let Some(child) = child {
            node = node.patched_with_child(octant, &child, parent_level, config.air);
            touched = true;
        }
    }
    touched.then_some(node)
}

impl State {
    /// Remakes up to `budget` dirty nodes, the finest first, so that the ones a node is made from are done before it.
    fn remake_dirty(&mut self, config: &CoarseConfig, data: &WorldData, budget: usize) {
        for _ in 0..budget {
            let Some(key) = self.dirty.pop_first() else {
                break;
            };
            let cube = cube_of(key);
            let grid = key.1;
            let made = remake(config, &self.overrides, data, grid, cube);
            let changed = match (&made, self.overrides.get(&key)) {
                (Some(new), Some(old)) => *new != old.node,
                (None, None) => false,
                _ => true,
            };
            if !changed {
                continue;
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
            if cube.level() < config.maximum_override_level {
                self.dirty.insert(key_of(
                    grid,
                    BrickCube::containing(cube.origin(), cube.level() + 1),
                ));
            }
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
        let from = (level, position.brick());
        let moved = state.selected_from != Some(from) || state.selected_for != Some(state.detail);
        if moved || state.selected_edit_epoch != self.edit_epoch {
            state.leaves = select_leaves(config, &self.edited, level, position, &state.detail);
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

/// The cubes of detail level 1 or more that a client at `position` in `level` is to draw and that have an override in them: the selection with the bricks it
/// holds left as bricks, followed only to where there is an override. A cube without one is sent nothing, so whether the client draws it (it does not draw a
/// cube that is empty or buried) does not matter here, and the terrain is not asked. What it does draw of these is the client's to work out: it adds the
/// cubes of an override to those it keeps as it is told of them.
fn select_leaves(
    config: &CoarseConfig,
    edited: &EditedCubes,
    level: LevelIndex,
    position: WorldPosition,
    detail: &SetDetail,
) -> Vec<(GridId, BrickCube)> {
    let grid = GridId::root(level);
    let view = ViewSelection {
        detail: detail.settings(),
        zoom: detail.zoom(),
        fine_radius: u32::from(detail.fine_radius),
        maximum_distance: config.view_distance,
    };
    let may_hold = |cube: BrickCube| edited.holds(grid, cube);
    select_around_brick(position.brick(), &view, &may_hold)
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
    /// How many overrides there are and how many nodes are waiting to be remade.
    pub fn counts(&self) -> (usize, usize) {
        let state = locked(&self.state);
        (state.overrides.len(), state.dirty.len())
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
                let grid = GridId::root(index);
                for (brick, _) in level.overrides.bricks() {
                    state.mark_brick(grid, brick, config.maximum_override_level);
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
        let maximum = config.maximum_override_level;
        registrar.on_channels(
            HandlerSpec::new("engine:coarse_streaming_mark"),
            &channels,
            move |event, _| {
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
                state.remake_dirty(&config, context.world().data(), config.nodes_per_tick);
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

    use super::*;

    fn cube(x: i64, level: u8) -> BrickCube {
        BrickCube::new(BrickCoordinate::new(x, 0, 0), level).expect("the origin is aligned")
    }

    fn stored(version: u64) -> Stored {
        Stored {
            node: CoarseNode::uniform(CoarseCell::empty(0, 0)),
            version,
        }
    }

    fn key(x: i64, level: u8) -> Key {
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

    fn at(x: i64) -> WorldPosition {
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
        let none = select_leaves(&config, &edited, LevelIndex::MAIN, at(100), &detail);
        assert!(none.is_empty(), "no override, no leaf: {none:?}");

        // An override in the middle distance and one far off, and one in the ball of bricks the client holds.
        edited.add(GridId::MAIN, cube(160, 1));
        edited.add(GridId::MAIN, cube(5_000, 1));
        edited.add(GridId::MAIN, cube(100, 1));
        let leaves = select_leaves(&config, &edited, LevelIndex::MAIN, at(100), &detail);
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
        let near_leaves = select_leaves(&near_sighted, &edited, LevelIndex::MAIN, at(100), &detail);
        assert!(
            near_leaves
                .iter()
                .all(|(_, leaf)| leaf.origin().x() < 1_000)
        );
        assert!(near_leaves.len() < leaves.len());
        // And not in another level.
        let other = select_leaves(&config, &edited, LevelIndex::new(1), at(100), &detail);
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
        let origins: Vec<[i64; 3]> = state.dirty.iter().map(|key| key.2).collect();
        assert_eq!(origins, [[0, 0, 0], [0, 0, 2], [0, 2, 0], [2, 0, 0]]);
    }

    #[test]
    fn nothing_is_marked_when_there_are_no_overrides_to_keep_or_the_grid_is_not_a_levels_root() {
        let mut state = State::default();
        state.mark_brick(GridId::MAIN, BrickCoordinate::ORIGIN, 0);
        assert!(state.dirty.is_empty());
        let ship = GridId::new(GridId::FIRST_ALLOCATED);
        assert!(ship.root_level().is_none(), "a ship is not a root grid");
        state.mark_brick(ship, BrickCoordinate::ORIGIN, 12);
        assert!(state.dirty.is_empty());
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
        let update = |x: i64| CoarseNodeUpdate {
            grid: GridId::MAIN,
            cube: cube(x * 2, 1),
            node: None,
        };
        assert!(in_messages(Vec::new()).is_empty());
        let updates: Vec<_> = (0..(2 * MAXIMUM_UPDATES as i64 + 3)).map(update).collect();
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
