//! The server half of coarse levels of detail, run by the plugin host on a real server with the pretend game of `world_storage::test_support`:
//! which overrides it keeps, who is sent them, in what order and how fast.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use block_models::BlockTable;
use coarse_cells::{CoarseCell, CoarseNode};
use coarse_streaming::{
    Brightness, CoarseConfig, CoarseHandle, CoarseNodeUpdate, CoarseNodes, CoarseServerPlugin,
    SetDetail, ZoomRequest,
};
use namespaced_id::NamespacedId;
use player_presence::{PlayerPose, PlayersPlugin, PoseReport};
use plugin_host::{Phase, Plugin, PluginHost, Registrar, SystemSpec};
use protocol::{ClientMessage, Message, ServerMessage};
use region_octree::BrickCube;
use server::test_support::log_in;
use server::{FarShipSettings, GridView, Server, ServerConfig};
use transport::Connection;
use transport::loopback::{LoopbackConnection, loopback_listener};
use voxel_grid::{ChannelIndex, Grid};
use voxel_space::{
    BrickBox, BrickCoordinate, GridId, GridPose, LevelIndex, LocalVoxelIndex, Orientation,
    VoxelCoordinate, WorldPosition,
};
use world_storage::far_nodes::{FarFileName, FarNodes, FarNodesRead, read_far_nodes};
use world_storage::test_support::{Game, Plugin as FixturePlugin, id, sample_world};
use world_storage::{MemoryShipFiles, ShipFiles, ShipFormat, WorldData};

/// The numbers of `base_blocks:air` and `base_blocks:stone`, the first blocks the pretend game registers.
const AIR: u32 = 0;
const STONE: u32 = 1;
const TICKS_PER_SECOND: u32 = 20;

type Edits = Arc<Mutex<Vec<(GridId, ChannelIndex, VoxelCoordinate, u32)>>>;

/// A plugin that makes the edits a test queues, the way a rule of a game does, so that the plugin under test hears of them from the host.
struct Editor {
    channels: Vec<ChannelIndex>,
    edits: Edits,
    /// How many ships to make (each with a few voxels of stone) in the next tick, and the ones made.
    ships_to_make: Arc<Mutex<usize>>,
    ships_made: Arc<Mutex<Vec<GridId>>>,
    /// Whether a ship made is put into its file in the tick it is made, and the ships to put into their files in the next tick (after the edits).
    park_new_ships: Arc<AtomicBool>,
    ships_to_park: Arc<Mutex<Vec<GridId>>>,
    ships_to_load: Arc<Mutex<Vec<GridId>>>,
}

impl Plugin for Editor {
    fn id(&self) -> NamespacedId {
        id("test:editor")
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        let edits = Arc::clone(&self.edits);
        let (ships_to_make, ships_made) = (
            Arc::clone(&self.ships_to_make),
            Arc::clone(&self.ships_made),
        );
        let block = self.channels[0];
        let (park_new_ships, ships_to_park, ships_to_load) = (
            Arc::clone(&self.park_new_ships),
            Arc::clone(&self.ships_to_park),
            Arc::clone(&self.ships_to_load),
        );
        registrar.system(
            SystemSpec::new("test:editor", Phase::Rules).writes(&self.channels),
            move |context| {
                let to_make = std::mem::take(&mut *ships_to_make.lock().unwrap());
                for _ in 0..to_make {
                    let world = context.world();
                    let pose = GridPose::new(
                        WorldPosition::new(BrickCoordinate::ORIGIN, [0.0; 3]),
                        Orientation::IDENTITY,
                    );
                    let ship = world.create_grid(LevelIndex::MAIN, pose).unwrap();
                    ships_made.lock().unwrap().push(ship);
                    for x in 0..4 {
                        context
                            .grid(ship)
                            .unwrap()
                            .set(block, VoxelCoordinate::new(x, 1, 1), STONE)
                            .unwrap();
                    }
                    if park_new_ships.load(Ordering::SeqCst) {
                        context.world().park_ship(ship, false).unwrap();
                    }
                }
                let queued = std::mem::take(&mut *edits.lock().unwrap());
                for (grid, channel, voxel, value) in queued {
                    context
                        .grid(grid)
                        .unwrap()
                        .set(channel, voxel, value)
                        .unwrap();
                }
                for ship in std::mem::take(&mut *ships_to_park.lock().unwrap()) {
                    context.world().park_ship(ship, false).unwrap();
                }
                for ship in std::mem::take(&mut *ships_to_load.lock().unwrap()) {
                    context.world().load_ship(ship).unwrap();
                }
            },
        );
    }
}

struct Client {
    connection: LoopbackConnection,
}

impl Client {
    fn send(&mut self, message: &ClientMessage) {
        self.connection.send(message.encode()).unwrap();
    }

    fn send_game_message(&mut self, kind: NamespacedId, payload: Vec<u8>) {
        self.send(&ClientMessage::GameMessage { kind, payload });
    }

    fn set_detail(&mut self, detail: &SetDetail) {
        self.send_game_message(SetDetail::kind(), detail.encode());
    }

    /// Asks to watch the bricks round `brick` of the main level (the region a server tells ships of by).
    fn watch(&mut self, brick: [i128; 3], radius: u32) {
        self.send(&ClientMessage::SetInterest {
            level: LevelIndex::MAIN,
            region: BrickBox::around(BrickCoordinate::new(brick[0], brick[1], brick[2]), radius),
        });
    }

    fn stand_at(&mut self, brick: [i128; 3]) {
        let pose = PlayerPose {
            position: WorldPosition::new(
                BrickCoordinate::new(brick[0], brick[1], brick[2]),
                [16.0, 20.0, 16.0],
            ),
            yaw: 0.0,
            pitch: 0.0,
        };
        self.send_game_message(PoseReport::kind(), PoseReport(pose).encode());
    }

    /// The coarse node messages that have come, as the payloads and as the updates in them in order.
    fn coarse_nodes(&mut self) -> (Vec<usize>, Vec<CoarseNodeUpdate>) {
        let mut sizes = Vec::new();
        let mut updates = Vec::new();
        while let Some(frame) = self.connection.receive().unwrap_or(None) {
            if let ServerMessage::GameMessage { kind, payload } =
                ServerMessage::decode(&frame).unwrap()
                && kind == CoarseNodes::kind()
            {
                sizes.push(payload.len());
                updates.extend(CoarseNodes::decode(&payload).unwrap().0);
            }
        }
        (sizes, updates)
    }
}

struct Rig {
    server: Server,
    client: Client,
    handle: CoarseHandle,
    edits: Edits,
    ships_to_make: Arc<Mutex<usize>>,
    ships_made: Arc<Mutex<Vec<GridId>>>,
    park_new_ships: Arc<AtomicBool>,
    ships_to_park: Arc<Mutex<Vec<GridId>>>,
    ships_to_load: Arc<Mutex<Vec<GridId>>>,
    /// The files of the server's ships (clones share them), and how the world writes ships.
    files: MemoryShipFiles,
    format: ShipFormat,
    block: ChannelIndex,
    temperature: ChannelIndex,
}

fn detail(bytes_per_second: u32) -> SetDetail {
    SetDetail {
        cell_pixels: 8.0,
        pixel_size: 0.001,
        bytes_per_second,
        fine_radius: 2,
        zoom: None,
    }
}

/// A rig over a world nobody has edited: flat ground, solid below a height of 10 voxels.
fn rig() -> Rig {
    let game = Game::new(&[FixturePlugin::BaseBlocks]);
    let world = game.new_world();
    rig_over(&game, world, |_, _| {})
}

/// Like [`rig`], with the configuration changed (it is given the number channel `core:temperature` too, for a brightness to be read from).
fn rig_configured(configure: impl FnOnce(&mut CoarseConfig, ChannelIndex)) -> Rig {
    let game = Game::new(&[FixturePlugin::BaseBlocks]);
    let world = game.new_world();
    rig_over(&game, world, configure)
}

/// Like [`rig_over_the_sample_world`], with the configuration changed.
fn rig_over_the_sample_world_configured(configure: impl FnOnce(&mut CoarseConfig)) -> Rig {
    let game = Game::new(&[FixturePlugin::BaseBlocks]);
    let world = sample_world(&game);
    rig_over(&game, world, |config, _| configure(config))
}

/// A rig over the sample world, which has edits in the brick at the origin before the server starts.
fn rig_over_the_sample_world() -> Rig {
    let game = Game::new(&[FixturePlugin::BaseBlocks]);
    let world = sample_world(&game);
    rig_over(&game, world, |_, _| {})
}

/// A server with the players plugin, the coarse plugin (looking out 20,000 voxels) and an editor, and one client who has joined (as test identity 1).
fn rig_over(
    game: &Game,
    world: WorldData,
    configure: impl FnOnce(&mut CoarseConfig, ChannelIndex),
) -> Rig {
    rig_over_files(game, world, MemoryShipFiles::new(), configure)
}

/// Like [`rig_over`], with the files of the ships that are in files already (a server that starts again).
fn rig_over_files(
    game: &Game,
    world: WorldData,
    files: MemoryShipFiles,
    configure: impl FnOnce(&mut CoarseConfig, ChannelIndex),
) -> Rig {
    rig_over_files_telling_of_far_ships(game, world, files, None, configure)
}

/// Like [`rig_over_files`], with ships told of as far away if `far_ships` says so.
fn rig_over_files_telling_of_far_ships(
    game: &Game,
    world: WorldData,
    files: MemoryShipFiles,
    far_ships: Option<FarShipSettings>,
    configure: impl FnOnce(&mut CoarseConfig, ChannelIndex),
) -> Rig {
    let format = game.ship_format(&world);
    let block = world
        .main()
        .overrides
        .layout()
        .channel_index(&id("core:block"))
        .unwrap();
    let temperature = world
        .main()
        .overrides
        .layout()
        .channel_index(&id("core:temperature"))
        .unwrap();
    let players = PlayersPlugin::new(true);
    let mut config = CoarseConfig::new(
        BlockTable::of_cubes(&[false, true, true, true]),
        AIR,
        block,
        0,
        TICKS_PER_SECOND,
        players.presence(),
    );
    config.view_distance = 20_000.0;
    // (Not the machine's: what a test does must not depend on how many processors it runs on.)
    config.node_workers = 3;
    configure(&mut config, temperature);
    let coarse = CoarseServerPlugin::new(config);
    let handle = coarse.handle();
    let edits: Edits = Arc::default();
    let ships_to_make: Arc<Mutex<usize>> = Arc::default();
    let ships_made: Arc<Mutex<Vec<GridId>>> = Arc::default();
    let park_new_ships: Arc<AtomicBool> = Arc::default();
    let ships_to_park: Arc<Mutex<Vec<GridId>>> = Arc::default();
    let ships_to_load: Arc<Mutex<Vec<GridId>>> = Arc::default();
    let editor = Editor {
        channels: vec![block, temperature],
        edits: Arc::clone(&edits),
        ships_to_make: Arc::clone(&ships_to_make),
        ships_made: Arc::clone(&ships_made),
        park_new_ships: Arc::clone(&park_new_ships),
        ships_to_park: Arc::clone(&ships_to_park),
        ships_to_load: Arc::clone(&ships_to_load),
    };
    let plugins: Vec<Box<dyn Plugin>> = vec![Box::new(players), Box::new(coarse), Box::new(editor)];
    let host = PluginHost::new(plugins, &world).unwrap();
    let (listener, connector) = loopback_listener();
    let mut server_config = ServerConfig::new(id("test_game:game")).with_counted_challenges(1);
    server_config.far_ships = far_ships;
    let mut server = Server::new(
        server_config,
        world,
        &game.registries(),
        Box::new(host),
        Box::new(listener),
    )
    .unwrap()
    .with_ship_files(Box::new(files.clone()));
    let mut client = Client {
        connection: connector.connect().unwrap(),
    };
    log_in(
        &mut server,
        &mut client.connection,
        &id("test_game:game"),
        1,
    );
    Rig {
        server,
        client,
        handle,
        edits,
        ships_to_make,
        ships_made,
        park_new_ships,
        ships_to_park,
        ships_to_load,
        files,
        format,
        block,
        temperature,
    }
}

impl Rig {
    /// Runs ticks. After each the worker that makes the nodes is waited for (until it has no more than it was handed, with a deadline), so that how many ticks
    /// it takes for a node to be kept is the same on every run: the nodes it made are kept at the start of the next tick.
    fn step(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.server.step();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            while self.handle.worker_is_busy() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "the worker did not finish"
                );
                std::thread::yield_now();
            }
        }
    }

    /// Queues the digging of every voxel in the box (to air), for the editor to do at the next tick.
    fn dig(&self, from: [i128; 3], to: [i128; 3]) {
        self.place(from, to, AIR);
    }

    /// Queues the filling of every voxel in the box with a block.
    fn place(&self, from: [i128; 3], to: [i128; 3], block: u32) {
        self.set_channel(from, to, self.block, block);
    }

    /// Queues the setting of a channel to a value in every voxel in the box.
    fn set_channel(&self, from: [i128; 3], to: [i128; 3], channel: ChannelIndex, value: u32) {
        self.set_channel_in(GridId::MAIN, from, to, channel, value);
    }

    /// The same in a ship.
    fn set_channel_in(
        &self,
        grid: GridId,
        from: [i128; 3],
        to: [i128; 3],
        channel: ChannelIndex,
        value: u32,
    ) {
        let mut edits = self.edits.lock().unwrap();
        for x in from[0]..to[0] {
            for y in from[1]..to[1] {
                for z in from[2]..to[2] {
                    edits.push((grid, channel, VoxelCoordinate::new(x, y, z), value));
                }
            }
        }
    }

    /// Steps until every node that was to be remade has been.
    fn settle(&mut self) {
        for _ in 0..200 {
            self.step(1);
            if self.handle.counts().1 == 0 {
                return;
            }
        }
        panic!("the nodes are still being remade after 200 ticks");
    }

    fn dig_and_settle(&mut self, from: [i128; 3], to: [i128; 3]) {
        self.dig(from, to);
        self.settle();
    }

    /// The override of the node at this level that holds the brick at the origin, with its version.
    fn override_at_origin(&self, level: u8) -> Option<(coarse_cells::CoarseNode, u64)> {
        self.handle.override_of(
            GridId::MAIN,
            BrickCube::containing(BrickCoordinate::ORIGIN, level),
        )
    }
}

fn cube_at_origin(level: u8) -> BrickCube {
    BrickCube::containing(BrickCoordinate::ORIGIN, level)
}

#[test]
fn bricks_edited_before_the_server_started_get_overrides_up_to_the_limit_and_no_more() {
    let mut rig = rig_over_the_sample_world();
    // (The worker makes a level of nodes in a tick: the nodes of the next level are made from them.)
    rig.settle();
    // The sample world has edits in one brick, the one at the origin: a node at each level from 1 to 12 holds it.
    assert_eq!(rig.handle.counts(), (12, 0));
    assert_eq!(
        rig.handle.versions_made(),
        12,
        "each node was made once, from the nodes it is made from"
    );
    for level in 1..=12 {
        assert!(rig.override_at_origin(level).is_some(), "level {level}");
    }
    assert!(rig.override_at_origin(13).is_none());
}

#[test]
fn nothing_is_sent_to_a_client_who_has_not_asked_or_does_not_want_any_or_has_not_said_where_it_is()
{
    let mut rig = rig_over_the_sample_world();
    rig.client.stand_at([96, 0, 0]);
    rig.step(5);
    assert_eq!(rig.handle.client_count(), 0);
    assert!(
        rig.client.coarse_nodes().1.is_empty(),
        "it never asked to be sent any"
    );

    rig.client.set_detail(&detail(0));
    rig.step(5);
    assert_eq!(rig.handle.client_count(), 1);
    assert!(
        rig.client.coarse_nodes().1.is_empty(),
        "it asked for a rate of none"
    );

    rig.client.set_detail(&detail(1_000_000));
    rig.step(5);
    let (_, updates) = rig.client.coarse_nodes();
    assert_eq!(updates.len(), 1, "now it wants them: {updates:?}");
}

#[test]
fn a_client_who_has_not_said_where_it_is_is_sent_nothing() {
    let mut rig = rig();
    rig.client.set_detail(&detail(1_000_000));
    rig.step(5);
    assert_eq!(rig.handle.client_count(), 1);
    assert!(rig.client.coarse_nodes().1.is_empty());
}

#[test]
fn a_far_client_is_sent_the_override_of_the_leaf_that_holds_the_edit_once() {
    let mut rig = rig();
    rig.dig_and_settle([0, 0, 0], [16, 8, 16]);
    rig.client.stand_at([96, 0, 0]);
    rig.client.set_detail(&detail(1_000_000));
    rig.step(5);
    let (_, updates) = rig.client.coarse_nodes();
    let [update] = &updates[..] else {
        panic!("{updates:?}");
    };
    assert_eq!(update.grid, GridId::MAIN);
    assert_eq!(update.cube, cube_at_origin(update.cube.level()));
    assert!((1..=12).contains(&update.cube.level()));
    let (held, _) = rig
        .override_at_origin(update.cube.level())
        .expect("it was sent from the overrides");
    assert_eq!(update.node, Some(held.without_exposure()));

    // Nothing has changed: nothing is sent again, however long it waits.
    rig.step(40);
    assert!(rig.client.coarse_nodes().1.is_empty());
}

#[test]
fn what_a_client_holds_as_bricks_is_not_sent_as_nodes() {
    let mut rig = rig();
    rig.dig_and_settle([0, 0, 0], [16, 8, 16]);
    // 12 bricks from the edit: far enough for the rule to choose a node there, if the client did not hold the bricks.
    rig.client.stand_at([12, 0, 0]);
    let mut holding = detail(1_000_000);
    holding.fine_radius = 12;
    rig.client.set_detail(&holding);
    rig.step(5);
    assert!(
        rig.client.coarse_nodes().1.is_empty(),
        "the edit is in the bricks the client holds"
    );

    // The same client holding fewer is sent it.
    holding.fine_radius = 2;
    rig.client.set_detail(&holding);
    rig.step(5);
    let (_, updates) = rig.client.coarse_nodes();
    let [update] = &updates[..] else {
        panic!("{updates:?}");
    };
    assert_eq!(update.cube, cube_at_origin(update.cube.level()));
}

#[test]
fn an_edit_made_later_is_sent_again_in_a_newer_version_and_the_digging_shows_in_the_cells() {
    let mut rig = rig();
    rig.dig_and_settle([0, 0, 0], [16, 8, 16]);
    rig.client.stand_at([96, 0, 0]);
    rig.client.set_detail(&detail(1_000_000));
    rig.step(5);
    let (_, first) = rig.client.coarse_nodes();
    let [first] = &first[..] else {
        panic!("{first:?}");
    };
    let (before, version_before) = rig.override_at_origin(1).unwrap();
    // The first dig stops at 16 along x: the voxels from 16 to 24 are still ground, in cells 8 to 11 of this node.
    assert!(
        before.cell(LocalVoxelIndex::new(8, 0, 0)).is_drawn(),
        "it is solid there to begin with"
    );

    rig.dig_and_settle([16, 0, 0], [24, 8, 16]);

    let (node, version_after) = rig.override_at_origin(1).unwrap();
    assert!(version_after > version_before);
    let dug = node.cell(LocalVoxelIndex::new(8, 0, 0));
    assert_eq!((dug.block, dug.fill, dug.exposure), (AIR, 0, 0), "dug out");
    assert_eq!(
        node.cell(LocalVoxelIndex::new(11, 3, 7)).fill,
        0,
        "the box is now 12 cells wide and 4 high in this node, from its corner"
    );
    assert!(
        node.cell(LocalVoxelIndex::new(12, 0, 0)).is_drawn(),
        "and no wider"
    );

    let (_, resent) = rig.client.coarse_nodes();
    let [resent] = &resent[..] else {
        panic!("{resent:?}");
    };
    assert_eq!(
        resent.cube, first.cube,
        "the same leaf, as the client has not moved"
    );
    assert_ne!(resent.node, first.node, "and now different");
    let (held, _) = rig.override_at_origin(resent.cube.level()).unwrap();
    assert_eq!(resent.node, Some(held.without_exposure()));

    rig.step(40);
    assert!(rig.client.coarse_nodes().1.is_empty());
}

#[test]
fn an_edit_too_small_to_change_the_cells_of_the_leaf_is_not_sent_and_does_not_make_the_nodes_above_it_new()
 {
    let mut rig = rig();
    // Deep in the ground, where every cell is full: one voxel out of a cell of 8 changes it to 224 of 255, which the cell of 8 of those rounds back to
    // 255 (and the same for a second voxel in another cell of 4 that is in the same cell of 8).
    let deep = BrickCoordinate::new(0, -1, 0);
    let version_at = |rig: &Rig, level| {
        rig.handle
            .override_of(GridId::MAIN, BrickCube::containing(deep, level))
            .unwrap()
            .1
    };
    rig.dig_and_settle([20, -20, 20], [21, -19, 21]);
    rig.client.stand_at([96, 0, 0]);
    rig.client.set_detail(&detail(1_000_000));
    rig.step(5);
    let (_, first) = rig.client.coarse_nodes();
    let [first] = &first[..] else {
        panic!("{first:?}");
    };
    assert!(
        first.cube.level() >= 3,
        "a leaf this far away is made of big cells: {first:?}"
    );
    let before: Vec<u64> = (1..=12).map(|level| version_at(&rig, level)).collect();

    rig.dig_and_settle([22, -20, 22], [23, -19, 23]);
    let after: Vec<u64> = (1..=12).map(|level| version_at(&rig, level)).collect();
    assert!(after[0] > before[0], "the exact cells changed");
    assert!(after[1] > before[1], "so did those of 4");
    assert_eq!(
        after[2..],
        before[2..],
        "the cells of 8 and above are as they were"
    );
    assert!(
        rig.client.coarse_nodes().1.is_empty(),
        "so the leaf is not sent again"
    );
}

#[test]
fn an_edit_high_in_the_air_or_deep_in_the_ground_is_sent_though_the_cubes_it_is_in_are_otherwise_left_out()
 {
    let mut rig = rig();
    rig.client.stand_at([0, 0, 0]);
    rig.client.set_detail(&detail(1_000_000));
    rig.step(5);
    assert!(rig.client.coarse_nodes().1.is_empty(), "nothing is edited");

    // A block of stone floating 25 bricks over the ground and 10 along, and a hole dug 13 bricks under it: the cubes round both are air all through, or
    // ground all through with ground all round them, and are not drawn unless an edit is in them or beside them.
    let sky = BrickCoordinate::new(10, 25, 0);
    let hole = BrickCoordinate::new(0, -13, 0);
    rig.place([320, 800, 0], [324, 804, 4], STONE);
    rig.dig([0, -400, 0], [8, -392, 8]);
    rig.settle();
    // (The leaves are worked out again some ticks after an edit in a new place.)
    rig.step(40);
    let (_, updates) = rig.client.coarse_nodes();
    for edit in [sky, hole] {
        let [update] = &updates
            .iter()
            .filter(|update| update.cube.contains(edit))
            .collect::<Vec<_>>()[..]
        else {
            panic!("{edit:?} in {updates:?}");
        };
        assert!(update.cube.level() >= 1);
        let (held, _) = rig
            .handle
            .override_of(GridId::MAIN, update.cube)
            .expect("it was sent from the overrides");
        assert_eq!(update.node, Some(held.without_exposure()));
    }
    assert_eq!(updates.len(), 2, "{updates:?}");
}

/// Edits in four bricks a long way from each other, so that each is in a leaf of its own for a client a long way from all of them.
fn dig_four_far_apart(rig: &mut Rig) {
    for brick in [0_i128, 64, 128, 192] {
        let x = brick * 32;
        rig.dig([x, 0, 0], [x + 8, 4, 8]);
    }
    rig.settle();
    let (overrides, _) = rig.handle.counts();
    assert_eq!(
        rig.handle.versions_made(),
        overrides as u64,
        "the finest nodes are made first, so no node is made twice"
    );
}

#[test]
fn a_fast_client_is_sent_every_leaf_that_has_an_override_coarsest_first() {
    let mut rig = rig_with_four_edits();
    rig.client.set_detail(&detail(10_000_000));
    rig.step(10);
    let (_, updates) = rig.client.coarse_nodes();
    let mut cubes: Vec<_> = updates.iter().map(|update| update.cube).collect();
    cubes.sort_by_key(|cube| (cube.origin().x(), cube.level()));
    cubes.dedup();
    assert_eq!(cubes.len(), 4, "{updates:?}");
    assert_eq!(updates.len(), 4, "each once: {updates:?}");
    let levels: Vec<u8> = updates.iter().map(|update| update.cube.level()).collect();
    assert!(
        levels.windows(2).all(|pair| pair[0] >= pair[1]),
        "coarsest first: {levels:?}"
    );
}

#[test]
fn a_slow_client_is_sent_no_more_bytes_than_it_asked_for_and_the_rest_waits() {
    // A fast client shows what the four updates are, and how big one is.
    let mut fast = rig_with_four_edits();
    fast.client.set_detail(&detail(10_000_000));
    fast.step(10);
    let (_, updates) = fast.client.coarse_nodes();
    assert_eq!(updates.len(), 4);
    let one_update = u32::try_from(CoarseNodes(vec![updates[0].clone()]).encode().len()).unwrap();

    // A slow one asks for a rate at which one update takes 2 seconds, so in 2 seconds it gets some but not all.
    let mut slow = rig_with_four_edits();
    let rate = one_update * TICKS_PER_SECOND / 40;
    slow.client.set_detail(&detail(rate));
    slow.step(40);
    let (_, received) = slow.client.coarse_nodes();
    assert!(
        (1..4).contains(&received.len()),
        "{} of 4 at {rate} bytes a second",
        received.len()
    );

    // Given the time it gets the rest, each once.
    slow.step(40 * 8);
    let (_, rest) = slow.client.coarse_nodes();
    assert_eq!(received.len() + rest.len(), 4);
}

fn rig_with_four_edits() -> Rig {
    let mut rig = rig();
    dig_four_far_apart(&mut rig);
    rig.client.stand_at([96, 0, 0]);
    rig
}

#[test]
fn a_client_who_leaves_is_forgotten() {
    let mut rig = rig();
    rig.client.stand_at([96, 0, 0]);
    rig.client.set_detail(&detail(1_000_000));
    rig.step(3);
    assert_eq!(rig.handle.client_count(), 1);
    rig.client.send(&ClientMessage::Goodbye);
    rig.step(3);
    assert_eq!(rig.handle.client_count(), 0);
}

#[test]
fn only_so_many_nodes_are_remade_in_a_tick_and_a_node_comes_after_those_it_is_made_from() {
    let mut rig = rig_over_the_sample_world_configured(|config| config.nodes_per_tick = 1);
    // The sample world has four cubes of 2 bricks to look at (the one with the edits and three beside it that have none), and a node at each of 12 levels.
    let mut counts = vec![rig.handle.counts()];
    // (Logging in took two ticks, and two of the four cubes were done in them.)
    assert_eq!(counts[0], (0, 2), "before the first tick we look at");
    for _ in 0..16 {
        rig.step(1);
        counts.push(rig.handle.counts());
    }
    for pair in counts.windows(2) {
        assert!(
            pair[1].0 <= pair[0].0 + 1,
            "one node at most is made in a tick: {counts:?}"
        );
    }
    assert_eq!(
        counts.last(),
        Some(&(12, 0)),
        "all is done in the end: {counts:?}"
    );
    // The chain goes up one level at a time: whenever a node has been made, the nodes it is made from are there already.
    let mut other = rig_over_the_sample_world_configured(|config| config.nodes_per_tick = 1);
    for _ in 0..16 {
        other.step(1);
        let made: Vec<u8> = (1..=12)
            .filter(|&level| other.override_at_origin(level).is_some())
            .collect();
        let expected: Vec<u8> = (1..=made.len() as u8).collect();
        assert_eq!(made, expected, "from the finest up, without gaps");
    }
}

#[test]
fn a_client_who_moves_is_sent_the_leaf_for_where_it_is_now() {
    let mut rig = rig();
    rig.dig_and_settle([0, 0, 0], [16, 8, 16]);
    rig.client.stand_at([96, 0, 0]);
    rig.client.set_detail(&detail(1_000_000));
    rig.step(5);
    let (_, far) = rig.client.coarse_nodes();
    let [far] = &far[..] else {
        panic!("{far:?}");
    };

    // Nearer, the same edit is in a smaller leaf.
    rig.client.stand_at([24, 0, 0]);
    rig.step(5);
    let (_, near) = rig.client.coarse_nodes();
    let [near] = &near[..] else {
        panic!("{near:?}");
    };
    assert!(near.cube.level() < far.cube.level(), "{near:?} and {far:?}");
    assert_eq!(near.cube, cube_at_origin(near.cube.level()));
    let (held, _) = rig.override_at_origin(near.cube.level()).unwrap();
    assert_eq!(near.node, Some(held.without_exposure()));

    // Back where it was: the leaf it was sent before is not sent again.
    rig.client.stand_at([96, 0, 0]);
    rig.step(5);
    assert!(rig.client.coarse_nodes().1.is_empty());
}

#[test]
fn a_client_who_zooms_is_sent_finer_leaves_in_the_cone() {
    let mut rig = rig();
    rig.dig_and_settle([0, 0, 0], [16, 8, 16]);
    rig.client.stand_at([96, 0, 0]);
    rig.client.set_detail(&detail(1_000_000));
    rig.step(5);
    let (_, plain) = rig.client.coarse_nodes();
    let [plain] = &plain[..] else {
        panic!("{plain:?}");
    };

    // A telescope toward the edit, with pixels a tenth of the size.
    let mut zoomed = detail(1_000_000);
    zoomed.zoom = Some(ZoomRequest {
        direction: [-1.0, 0.0, 0.0],
        half_angle: 0.1,
        pixel_size: 0.0001,
    });
    rig.client.set_detail(&zoomed);
    rig.step(5);
    let (_, finer) = rig.client.coarse_nodes();
    let [finer] = &finer[..] else {
        panic!("{finer:?}");
    };
    assert!(finer.cube.level() < plain.cube.level(), "{finer:?}");

    // A telescope toward the sky leaves it as it was.
    let mut elsewhere = detail(1_000_000);
    elsewhere.zoom = Some(ZoomRequest {
        direction: [0.0, 1.0, 0.0],
        half_angle: 0.1,
        pixel_size: 0.0001,
    });
    rig.client.set_detail(&elsewhere);
    rig.step(5);
    let (_, again) = rig.client.coarse_nodes();
    assert!(
        again.is_empty(),
        "the plain leaf was sent already: {again:?}"
    );
}

#[test]
fn an_edited_brick_is_in_the_part_of_its_node_it_is_in_whichever_of_the_eight_that_is() {
    let mut rig = rig();
    // Three bricks of the cube of 2 bricks at the origin, none of them the one at the corner, each with a block of stone floating in the air of it:
    // the bricks (1, 1, 0), (0, 1, 1) and (1, 0, 1), which are the octants 3, 6 and 5. (The ground is at a height of 10 voxels, in the bricks with y = 0.)
    rig.place([34, 34, 2], [40, 40, 8], STONE);
    rig.place([2, 34, 34], [8, 40, 40], STONE);
    rig.place([34, 12, 34], [40, 18, 40], STONE);
    rig.settle();
    let (node, _) = rig.override_at_origin(1).unwrap();
    // A voxel is 2 to a cell of this node.
    for (cell, voxel) in [
        ([17, 17, 1], [34, 34, 2]),
        ([1, 17, 17], [2, 34, 34]),
        ([17, 6, 17], [34, 12, 34]),
    ] {
        let found = node.cell(LocalVoxelIndex::new(cell[0], cell[1], cell[2]));
        assert!(
            found.is_drawn() && found.block == STONE,
            "the stone at {voxel:?} is in the cell {cell:?}: {found:?}"
        );
    }
    // And the air round them is air.
    for cell in [[17, 17, 17], [1, 17, 1], [17, 17, 6]] {
        let found = node.cell(LocalVoxelIndex::new(cell[0], cell[1], cell[2]));
        assert_eq!(found.fill, 0, "{cell:?}");
    }
}

#[test]
fn the_light_of_a_cell_is_the_brightness_of_the_air_it_looks_at() {
    let mut rig = rig_configured(|config, temperature| {
        config.brightness = Some(Brightness {
            channel: temperature,
            brightness_of: Arc::new(|value| value.min(255) as u8),
        });
        config.unlit = 7;
    });
    // A pit in the ground, 16 voxels square and 8 deep, whose air is bright (the pretend game lights with its temperature channel, 20 by default).
    rig.dig([0, 0, 0], [16, 8, 16]);
    rig.set_channel([0, 0, 0], [16, 8, 16], rig.temperature, 200);
    rig.settle();
    let (node, _) = rig.override_at_origin(1).unwrap();
    let light_of = |cell: [u8; 3]| {
        node.cell(LocalVoxelIndex::new(cell[0], cell[1], cell[2]))
            .light
    };
    // The side of the pit (voxels 16 to 18 along x, cell 8) looks at the air in it.
    assert_eq!(light_of([8, 2, 4]), 200);
    // The ground away from it (but in the same brick: the others are not edited, and their cells are the generator's), under the open air of 20.
    assert_eq!(light_of([12, 4, 12]), 20);
    // The deep ground, which looks at no air at all, keeps the brightness of its own voxels.
    assert_eq!(light_of([12, 0, 12]), 20);
    // And what no brick of the edits is in is the generator's, which knows of no light: the game's darkness.
    assert_eq!(light_of([20, 20, 20]), 7);
}

/// A rig over a world with no edits and one ship that nothing generates (the sample ship: two bricks, one at the origin and one beside it up and along), whose
/// blocks are all opaque but air.
fn rig_with_a_ship() -> (Rig, GridId) {
    let game = Game::new(&world_storage::test_support::ALL_PLUGINS);
    let mut world = game.new_world();
    let made = game.sample_ship(&world, 0);
    let ship = world.grids_mut().create(made);
    let rig = rig_over(&game, world, |config, _| {
        let mut opaque = vec![true; 16];
        opaque[AIR as usize] = false;
        config.table = BlockTable::of_cubes(&opaque);
    });
    (rig, ship)
}

#[test]
fn the_nodes_of_a_ship_nothing_generates_are_made_from_its_bricks_follow_its_edits_and_are_not_sent_to_a_client_that_has_the_bricks()
 {
    let (mut rig, ship) = rig_with_a_ship();
    rig.settle();
    let at_level = |rig: &Rig, level: u8| {
        rig.handle
            .override_of(ship, BrickCube::containing(BrickCoordinate::ORIGIN, level))
    };
    // The ship came into the world as a change of every voxel it holds: a node at each level from 1 to 12 holds its bricks.
    let (first, first_version) = at_level(&rig, 1).expect("the first level of nodes");
    for level in 1..=12 {
        assert!(at_level(&rig, level).is_some(), "level {level}");
    }
    assert!(at_level(&rig, 13).is_none());
    assert!(
        first.cells().any(|cell| cell.fill > 0),
        "the stone of the ship is in its node"
    );

    // A block put in a brick of the ship changes the node over it and not the ones that a block that small does not show in.
    rig.set_channel_in(ship, [8, 8, 8], [16, 16, 16], rig.block, STONE);
    rig.settle();
    let (second, second_version) = at_level(&rig, 1).unwrap();
    assert_ne!(first, second);
    assert!(second_version > first_version);

    // A client near the ship, and one far from it, are sent nothing for it: a client has the bricks of the ships it knows, and makes their nodes itself.
    for stand in [[0, 0, 0], [96, 0, 0], [4000, 0, 0]] {
        rig.client.stand_at(stand);
        rig.client.set_detail(&detail(10_000_000));
        rig.step(10);
        let (_, updates) = rig.client.coarse_nodes();
        assert!(updates.is_empty(), "{stand:?}: {updates:?}");
    }
}

#[test]
fn a_ship_made_while_the_server_runs_has_nodes_too() {
    let mut rig = rig();
    *rig.ships_to_make.lock().unwrap() = 1;
    rig.settle();
    let ship = rig.ships_made.lock().unwrap()[0];
    for level in 1..=12 {
        assert!(
            rig.handle
                .override_of(ship, BrickCube::containing(BrickCoordinate::ORIGIN, level))
                .is_some(),
            "level {level}"
        );
    }
}

// ---------------------------------------------------------------- the far nodes of ships that are put away

/// The nodes of a ship, as the plugin keeps them, coarsest first (the order of a far nodes file).
fn nodes_kept_of(rig: &Rig, ship: GridId) -> FarNodes {
    let mut kept = FarNodes::new();
    for (cube, node) in rig.handle.overrides_of(ship) {
        kept.insert(cube, node);
    }
    kept
}

/// What is in the far nodes file made for the ship file that `ship` is in, read as a server that starts again reads it.
fn far_nodes_read(rig: &mut Rig, ship: GridId) -> FarNodesRead {
    let parked = rig.server.world().grids().get_parked(ship).unwrap().clone();
    let (format, block) = (rig.format.clone(), rig.block);
    read_far_nodes(
        rig.files.far_files_mut(),
        &FarFileName::of(&parked.file),
        parked.checksum,
        |identifier| format.content_number(block, identifier),
    )
}

/// A rig over a world with one ship whose nodes are made, and then the ship is put into its file. Also the nodes it had.
fn rig_with_a_ship_put_away() -> (Rig, GridId, FarNodes) {
    let (mut rig, ship) = rig_with_a_ship();
    rig.settle();
    let before = nodes_kept_of(&rig, ship);
    assert!(before.len() >= 12, "a node at each of 12 levels at least");
    rig.ships_to_park.lock().unwrap().push(ship);
    rig.step(1);
    assert!(rig.server.world().grids().get(ship).is_none());
    (rig, ship, before)
}

#[test]
fn the_nodes_of_a_ship_that_is_put_away_are_written_beside_its_file_as_they_were() {
    let (mut rig, ship, before) = rig_with_a_ship_put_away();
    match far_nodes_read(&mut rig, ship) {
        FarNodesRead::Found(found) => assert_eq!(found, before),
        other => panic!("{other:?}"),
    }
    // The ship being in a file does not take its nodes from the plugin.
    assert_eq!(nodes_kept_of(&rig, ship), before);
}

#[test]
fn a_ship_put_away_with_nodes_still_to_be_made_has_no_nodes_file_and_keeps_no_nodes() {
    let (mut rig, ship) = rig_with_a_ship();
    rig.settle();
    // A block is put in the ship in one tick, and the ship is put away in the next, when the nodes over the block are not all made again (the levels go one
    // at a time).
    rig.set_channel_in(ship, [8, 8, 8], [9, 9, 9], rig.block, STONE);
    rig.step(1);
    assert!(rig.handle.counts().1 > 0, "nodes are still to be made");
    rig.ships_to_park.lock().unwrap().push(ship);
    rig.step(1);
    assert!(rig.server.world().grids().get_parked(ship).is_some());
    assert!(matches!(
        far_nodes_read(&mut rig, ship),
        FarNodesRead::Absent
    ));
    assert!(
        nodes_kept_of(&rig, ship).is_empty(),
        "nodes of a state the file does not have are not kept"
    );
    // What the worker was making for it when it was put away is not kept either.
    rig.step(3);
    assert!(
        nodes_kept_of(&rig, ship).is_empty(),
        "nodes the worker made"
    );
    assert_eq!(rig.handle.counts(), (0, 0));
}

#[test]
fn a_planet_put_away_has_no_nodes_file_for_its_generator_makes_its_nodes() {
    let game = Game::new(&world_storage::test_support::ALL_PLUGINS);
    let mut world = game.new_world();
    let made = game.sample_planet(&world, 0);
    let planet = world.grids_mut().create(made);
    let mut rig = rig_over(&game, world, |_, _| {});
    rig.settle();
    rig.ships_to_park.lock().unwrap().push(planet);
    rig.step(2);
    let parked = rig.server.world().grids().get_parked(planet).unwrap();
    assert!(parked.has_terrain);
    assert_eq!(rig.files.far_files_mut().names().unwrap(), []);

    // And a file of nodes that is there for a planet all the same is not read: a planet's nodes are its generator's.
    let name = FarFileName::of(&parked.file);
    let checksum = parked.checksum;
    let mut nodes = FarNodes::new();
    nodes.insert(
        BrickCube::containing(BrickCoordinate::ORIGIN, 1),
        CoarseNode::uniform(CoarseCell {
            block: STONE,
            fill: 255,
            light: 0,
            exposure: 0,
        }),
    );
    let (format, block) = (rig.format.clone(), rig.block);
    let bytes = nodes
        .encode(checksum, |number| format.content_identifier(block, number))
        .unwrap();
    let mut again = restarted(&mut rig, |files| {
        files.far_files_mut().write(&name, &bytes).unwrap();
    });
    again.step(1);
    assert!(again.server.world().grids().get_parked(planet).is_some());
    assert!(
        nodes_kept_of(&again, planet).is_empty(),
        "{:?}",
        again
            .server
            .world()
            .grids()
            .parked()
            .map(|(id, parked)| (id, parked.has_terrain, parked.file))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_ship_made_and_put_away_in_one_tick_has_no_nodes_file_and_not_the_nodes_of_nothing() {
    let mut rig = rig();
    rig.park_new_ships.store(true, Ordering::SeqCst);
    *rig.ships_to_make.lock().unwrap() = 1;
    rig.step(3);
    let ship = rig.ships_made.lock().unwrap()[0];
    assert!(rig.server.world().grids().get_parked(ship).is_some());
    // (Its voxels were heard of when it was not in memory: nothing is known of what it holds, and a file that said it holds nothing would be false.)
    assert!(matches!(
        far_nodes_read(&mut rig, ship),
        FarNodesRead::Absent
    ));
    assert!(nodes_kept_of(&rig, ship).is_empty());
}

/// A server made as a game that is started again makes it: from the world file the first one saved and its ship files, where `change` has changed the files
/// first (to see what a damaged or lost file does).
fn restarted(rig: &mut Rig, change: impl FnOnce(&mut MemoryShipFiles)) -> Rig {
    let game = Game::new(&world_storage::test_support::ALL_PLUGINS);
    let save = rig.server.save(&game.registries()).unwrap();
    let mut world_file = Vec::new();
    save.write_with(&mut rig.files, |bytes| {
        world_file = bytes.to_vec();
        Ok(())
    })
    .unwrap();
    let mut files = rig.files.duplicate();
    change(&mut files);
    let mut again = Game::new(&world_storage::test_support::ALL_PLUGINS);
    let (reloaded, _) = again.load(&world_file).unwrap();
    rig_over_files(&again, reloaded, files, |config, _| {
        let mut opaque = vec![true; 16];
        opaque[AIR as usize] = false;
        config.table = BlockTable::of_cubes(&opaque);
    })
}

#[test]
fn a_server_that_starts_again_has_the_nodes_of_a_ship_in_its_file_without_bringing_the_ship_in() {
    let (mut rig, ship, before) = rig_with_a_ship_put_away();
    let mut again = restarted(&mut rig, |_| {});
    assert!(again.server.world().grids().get(ship).is_none());
    again.step(1);
    assert!(again.server.world().grids().get_parked(ship).is_some());
    assert!(again.server.world().grids().get(ship).is_none());
    assert_eq!(nodes_kept_of(&again, ship), before);
    // Each is a version no client has been sent.
    for (cube, _) in before.iter() {
        assert!(again.handle.override_of(ship, cube).unwrap().1 > 0);
    }
    // They are read once: what the plugin makes of them later is its own.
    again.step(5);
    assert_eq!(nodes_kept_of(&again, ship), before);
}

#[test]
fn nodes_files_that_are_damaged_or_of_another_state_or_missing_cost_the_far_view_of_the_ship_and_nothing_else()
 {
    let (mut rig, ship, before) = rig_with_a_ship_put_away();
    let parked = rig.server.world().grids().get_parked(ship).unwrap().clone();
    let name = FarFileName::of(&parked.file);
    let good = rig.files.far_files_mut().read(&name).unwrap();
    let (format, block) = (rig.format.clone(), rig.block);
    let another_state = before
        .encode(parked.checksum ^ 1, |number| {
            format.content_identifier(block, number)
        })
        .unwrap();
    let mut damaged = good.clone();
    damaged[good.len() / 2] ^= 0x10;
    for (what, bytes) in [
        ("damaged", damaged),
        ("cut short", good[..good.len() - 3].to_vec()),
        ("not a file of nodes", b"VOXUNAMI".to_vec()),
        ("another state", another_state),
    ] {
        let mut broken = restarted(&mut rig, |files| {
            files.far_files_mut().write(&name, &bytes).unwrap();
        });
        broken.step(1);
        assert!(
            broken.server.world().grids().get_parked(ship).is_some(),
            "{what}"
        );
        assert!(nodes_kept_of(&broken, ship).is_empty(), "{what}");
    }
    let mut lost = restarted(&mut rig, |files| {
        files.far_files_mut().remove(&name).unwrap();
    });
    lost.step(1);
    assert!(lost.server.world().grids().get_parked(ship).is_some());
    assert!(nodes_kept_of(&lost, ship).is_empty());
}

#[test]
fn a_ship_that_comes_back_has_its_nodes_made_again_from_its_bricks_and_is_the_same() {
    let (mut rig, ship, before) = rig_with_a_ship_put_away();
    let mut again = restarted(&mut rig, |_| {});
    again.step(1);
    again.ships_to_load.lock().unwrap().push(ship);
    again.settle();
    assert!(again.server.world().grids().get(ship).is_some());
    assert_eq!(nodes_kept_of(&again, ship), before);
}

// ---- ships that are far away ----

/// A rig that tells of ships as far away when they look a quarter of a degree long, with a ship nothing generates whose first brick is 100 bricks along from where the
/// sample ship's pose puts it (so a client at the origin is some 3,200 voxels from it).
fn rig_with_a_far_ship() -> (Rig, GridId) {
    let game = Game::new(&world_storage::test_support::ALL_PLUGINS);
    let mut world = game.new_world();
    let made = game.sample_ship(&world, 100);
    let ship = world.grids_mut().create(made);
    let rig = rig_over_files_telling_of_far_ships(
        &game,
        world,
        MemoryShipFiles::new(),
        Some(FarShipSettings {
            minimum_apparent_size: 0.005,
            maximum_ships: 4,
        }),
        |config, _| {
            let mut opaque = vec![true; 16];
            opaque[AIR as usize] = false;
            config.table = BlockTable::of_cubes(&opaque);
        },
    );
    (rig, ship)
}

/// The cubes of a ship that the nodes sent say, in the order they came.
fn cubes_of(updates: &[CoarseNodeUpdate], ship: GridId) -> Vec<BrickCube> {
    updates
        .iter()
        .filter(|update| update.grid == ship)
        .map(|update| update.cube)
        .collect()
}

/// Where a ship is and the box of its own bricks, whether it is in memory or in a file.
fn place_of(rig: &Rig, ship: GridId) -> (GridPose, BrickBox) {
    let grids = rig.server.world().grids();
    match (grids.get(ship), grids.get_parked(ship)) {
        (Some(moving), _) => (moving.pose, moving.own_brick_box().unwrap()),
        (None, Some(parked)) => (parked.pose, parked.extent.unwrap()),
        (None, None) => panic!("{ship:?} is nowhere"),
    }
}

/// What a client standing in `brick` of the main level, asking with `detail`, is to be sent of a far ship: what the shared selection says, among the cubes that have
/// an override.
fn expected_far_nodes(
    rig: &Rig,
    ship: GridId,
    brick: [i128; 3],
    asked: &SetDetail,
) -> Vec<BrickCube> {
    let (pose, _) = place_of(rig, ship);
    let at = WorldPosition::new(
        BrickCoordinate::new(brick[0], brick[1], brick[2]),
        [16.0, 20.0, 16.0],
    );
    let local = pose.world_to_local(at).unwrap().brick();
    let view = coarse_cells::ViewSelection {
        detail: asked.settings(),
        zoom: asked.zoom(),
        fine_radius: u32::from(asked.fine_radius),
        maximum_distance: 20_000.0,
    };
    let held = |cube: BrickCube| rig.handle.override_of(ship, cube).is_some();
    let nodes = coarse_cells::far_ship_nodes_to_send(
        local,
        &view,
        &|cube| {
            rig.handle.overrides_of(ship).iter().any(|(held, _)| {
                held.level() <= cube.level()
                    && BrickCube::containing(held.origin(), cube.level()) == cube
            })
        },
        12,
    );
    nodes.into_iter().filter(|cube| held(*cube)).collect()
}

#[test]
fn a_client_is_sent_the_nodes_of_a_ship_it_was_told_of_as_far_away_coarsest_first_and_each_once() {
    let (mut rig, ship) = rig_with_a_far_ship();
    rig.settle();
    rig.client.stand_at([0, 0, 0]);
    rig.client.watch([0, 0, 0], 2);
    let asked = detail(10_000_000);
    rig.client.set_detail(&asked);
    rig.step(10);
    let (_, updates) = rig.client.coarse_nodes();
    let sent = cubes_of(&updates, ship);
    assert!(sent.len() > 4, "{sent:?}");
    // What the selection says, in the order it says it, once each.
    assert_eq!(sent, expected_far_nodes(&rig, ship, [0, 0, 0], &asked));
    let levels: Vec<u8> = sent.iter().map(|cube| cube.level()).collect();
    assert!(
        levels.windows(2).all(|pair| pair[0] >= pair[1]),
        "{levels:?}"
    );
    // Nothing more comes while nothing changes.
    rig.step(10);
    assert!(rig.client.coarse_nodes().1.is_empty());
}

#[test]
fn nothing_is_sent_for_a_ship_that_is_not_told_of_as_far_away() {
    let (mut rig, ship) = rig_with_a_far_ship();
    rig.settle();
    // Too far to look big enough: 3,200 voxels away from a viewer 20,000 bricks off.
    rig.client.stand_at([20_000, 0, 0]);
    rig.client.watch([20_000, 0, 0], 2);
    rig.client.set_detail(&detail(10_000_000));
    rig.step(10);
    assert!(cubes_of(&rig.client.coarse_nodes().1, ship).is_empty());
}

#[test]
fn a_ship_that_comes_near_and_goes_far_again_is_sent_its_nodes_again_for_the_client_forgot_them() {
    let (mut rig, ship) = rig_with_a_far_ship();
    rig.settle();
    rig.client.stand_at([0, 0, 0]);
    rig.client.watch([0, 0, 0], 2);
    rig.client.set_detail(&detail(10_000_000));
    rig.step(10);
    let first = cubes_of(&rig.client.coarse_nodes().1, ship);
    assert!(!first.is_empty());
    // Near: its bricks are sent instead, and no node.
    rig.client.stand_at([97, 0, 0]);
    rig.client.watch([97, 0, 0], 8);
    // (The plugin hears which ships are far a tick late, so what it sends in the tick of the move is for a client that was far a moment ago.)
    rig.step(3);
    rig.client.coarse_nodes();
    rig.step(10);
    assert!(cubes_of(&rig.client.coarse_nodes().1, ship).is_empty());
    // Far again: the same nodes.
    rig.client.stand_at([0, 0, 0]);
    rig.client.watch([0, 0, 0], 2);
    rig.step(10);
    let again = cubes_of(&rig.client.coarse_nodes().1, ship);
    assert_eq!(again, first);
}

// ---- the cubes of a far ship's box that are empty ----

/// The far ship with a block of stone in a far corner of its box as well, a brick (3, 3, 0) of it: the cubes of its box between the two blocks are empty.
fn rig_with_a_far_ship_with_a_gap() -> (Rig, GridId) {
    let (mut rig, ship) = rig_with_a_far_ship();
    rig.settle();
    rig.set_channel_in(ship, [100, 100, 2], [101, 101, 3], rig.block, STONE);
    rig.settle();
    (rig, ship)
}

/// The cubes of a ship that the nodes sent give a node for, in the order they came.
fn nodes_of(updates: &[CoarseNodeUpdate], ship: GridId) -> Vec<BrickCube> {
    updates
        .iter()
        .filter(|update| update.grid == ship && update.node.is_some())
        .map(|update| update.cube)
        .collect()
}

/// The cubes of a ship that the nodes sent say are empty.
fn empties_of(updates: &[CoarseNodeUpdate], ship: GridId) -> Vec<BrickCube> {
    updates
        .iter()
        .filter(|update| update.grid == ship && update.node.is_none())
        .map(|update| update.cube)
        .collect()
}

/// The cubes of a far ship a client in `brick` is to be told are empty, as the shared function says (the ship's box is that of its bricks).
fn expected_empty_cubes(
    rig: &Rig,
    ship: GridId,
    brick: [i128; 3],
    asked: &SetDetail,
) -> Vec<BrickCube> {
    let (pose, bounds) = place_of(rig, ship);
    let at = WorldPosition::new(
        BrickCoordinate::new(brick[0], brick[1], brick[2]),
        [16.0, 20.0, 16.0],
    );
    let local = pose.world_to_local(at).unwrap().brick();
    let view = coarse_cells::ViewSelection {
        detail: asked.settings(),
        zoom: asked.zoom(),
        fine_radius: u32::from(asked.fine_radius),
        maximum_distance: 20_000.0,
    };
    coarse_cells::far_ship_empty_cubes(
        local,
        &view,
        bounds,
        &|cube| {
            rig.handle.overrides_of(ship).iter().any(|(held, _)| {
                held.level() <= cube.level()
                    && BrickCube::containing(held.origin(), cube.level()) == cube
            })
        },
        12,
    )
}

fn sorted(mut cubes: Vec<BrickCube>) -> Vec<BrickCube> {
    cubes.sort_by_key(|cube| (cube.level(), cube.origin()));
    cubes
}

#[test]
fn a_client_is_told_which_cubes_in_the_box_of_a_far_ship_are_empty_with_its_nodes_coarsest_first_and_each_once()
 {
    let (mut rig, ship) = rig_with_a_far_ship_with_a_gap();
    rig.client.stand_at([88, 0, 0]);
    rig.client.watch([88, 0, 0], 2);
    let asked = detail(10_000_000);
    rig.client.set_detail(&asked);
    rig.step(10);
    let (_, updates) = rig.client.coarse_nodes();
    let expected = expected_empty_cubes(&rig, ship, [88, 0, 0], &asked);
    assert!(!expected.is_empty(), "the viewer does not test anything");
    assert_eq!(
        nodes_of(&updates, ship),
        expected_far_nodes(&rig, ship, [88, 0, 0], &asked)
    );
    let empties = empties_of(&updates, ship);
    assert_eq!(sorted(empties.clone()), sorted(expected));
    // All of it coarsest first, and no cube twice.
    let all: Vec<BrickCube> = cubes_of(&updates, ship);
    let levels: Vec<u8> = all.iter().map(|cube| cube.level()).collect();
    assert!(
        levels.windows(2).all(|pair| pair[0] >= pair[1]),
        "{levels:?}"
    );
    assert_eq!(
        sorted(all.clone()).len(),
        all.iter().collect::<std::collections::HashSet<_>>().len()
    );
    // Nothing more comes while nothing changes.
    rig.step(10);
    assert!(rig.client.coarse_nodes().1.is_empty());
}

#[test]
fn a_client_that_comes_nearer_to_a_far_ship_is_told_what_is_empty_there_and_not_what_it_was_told_before()
 {
    let (mut rig, ship) = rig_with_a_far_ship_with_a_gap();
    a_client_comes_nearer_to(&mut rig, ship);
}

#[test]
fn a_client_that_comes_nearer_to_a_far_ship_in_a_file_is_told_the_same_from_the_ships_place_in_the_world_file()
 {
    let (mut rig, ship) = rig_with_a_far_ship_with_a_gap_in_a_file();
    a_client_comes_nearer_to(&mut rig, ship);
}

/// The far ship with a gap, put in a file.
fn rig_with_a_far_ship_with_a_gap_in_a_file() -> (Rig, GridId) {
    let (mut rig, ship) = rig_with_a_far_ship_with_a_gap();
    rig.ships_to_park.lock().unwrap().push(ship);
    rig.step(1);
    assert!(rig.server.world().grids().get(ship).is_none());
    assert!(rig.server.world().grids().get_parked(ship).is_some());
    (rig, ship)
}

/// A client that comes nearer to a far ship, step by step, is told what is empty there and not what it was told before.
fn a_client_comes_nearer_to(rig: &mut Rig, ship: GridId) {
    let asked = detail(10_000_000);
    rig.client.set_detail(&asked);
    let mut told: Vec<CoarseNodeUpdate> = Vec::new();
    for brick in [[60, 0, 0], [80, 0, 0], [88, 0, 0], [91, 0, 0]] {
        rig.client.stand_at(brick);
        rig.client.watch(brick, 2);
        rig.step(10);
        let (_, updates) = rig.client.coarse_nodes();
        for update in &updates {
            assert!(
                !told
                    .iter()
                    .any(|before| before.grid == update.grid && before.cube == update.cube),
                "{brick:?}: {:?} was told before",
                update.cube
            );
        }
        told.extend(updates);
    }
    // Everything it needs at the last place, it was told at one place or another.
    let last = [91, 0, 0];
    for cube in expected_far_nodes(rig, ship, last, &asked) {
        assert!(nodes_of(&told, ship).contains(&cube), "{cube:?}");
    }
    let expected = expected_empty_cubes(rig, ship, last, &asked);
    assert!(!expected.is_empty());
    for cube in expected {
        assert!(empties_of(&told, ship).contains(&cube), "{cube:?}");
    }
}

#[test]
fn what_is_empty_is_told_again_when_a_ship_that_came_near_goes_far_again_for_the_client_forgot_it()
{
    let (mut rig, ship) = rig_with_a_far_ship_with_a_gap();
    rig.client.stand_at([88, 0, 0]);
    rig.client.watch([88, 0, 0], 2);
    rig.client.set_detail(&detail(10_000_000));
    rig.step(10);
    let first: Vec<(BrickCube, bool)> = rig
        .client
        .coarse_nodes()
        .1
        .iter()
        .filter(|update| update.grid == ship)
        .map(|update| (update.cube, update.node.is_some()))
        .collect();
    assert!(first.iter().any(|(_, node)| !node), "{first:?}");
    // Near: its bricks are sent instead.
    rig.client.stand_at([97, 0, 0]);
    rig.client.watch([97, 0, 0], 8);
    rig.step(3);
    rig.client.coarse_nodes();
    rig.step(10);
    assert!(cubes_of(&rig.client.coarse_nodes().1, ship).is_empty());
    // Far again: the same, the empty cubes too.
    rig.client.stand_at([88, 0, 0]);
    rig.client.watch([88, 0, 0], 2);
    rig.step(10);
    let again: Vec<(BrickCube, bool)> = rig
        .client
        .coarse_nodes()
        .1
        .iter()
        .filter(|update| update.grid == ship)
        .map(|update| (update.cube, update.node.is_some()))
        .collect();
    assert_eq!(again, first);
}

/// What a client in `brick` is sent of a ship, node or empty, in order.
fn told_of(rig: &mut Rig, ship: GridId, brick: [i128; 3]) -> Vec<(BrickCube, bool)> {
    rig.client.stand_at(brick);
    rig.client.watch(brick, 2);
    rig.step(10);
    rig.client
        .coarse_nodes()
        .1
        .iter()
        .filter(|update| update.grid == ship)
        .map(|update| (update.cube, update.node.is_some()))
        .collect()
}

#[test]
fn a_client_is_sent_the_same_of_a_far_ship_whether_it_is_in_memory_or_in_a_file() {
    let (mut in_memory, ship) = rig_with_a_far_ship_with_a_gap();
    in_memory.client.set_detail(&detail(10_000_000));
    let from_memory = told_of(&mut in_memory, ship, [88, 0, 0]);
    let (mut in_a_file, ship) = rig_with_a_far_ship_with_a_gap_in_a_file();
    in_a_file.client.set_detail(&detail(10_000_000));
    let from_a_file = told_of(&mut in_a_file, ship, [88, 0, 0]);
    assert!(from_memory.iter().any(|(_, node)| *node));
    assert!(from_memory.iter().any(|(_, node)| !*node));
    assert_eq!(from_a_file, from_memory);
}

// ---- far files at a save ----

#[test]
fn a_world_saved_with_a_ship_in_memory_keeps_its_nodes_beside_the_ship_file_and_a_server_that_starts_again_has_them()
 {
    let (mut rig, ship) = rig_with_a_ship();
    rig.settle();
    let before = nodes_kept_of(&rig, ship);
    assert!(before.len() >= 12, "a node at each of 12 levels at least");
    let mut again = restarted(&mut rig, |_| {});
    // The ship is in its file, and the world has its nodes without the ship having been in memory.
    assert!(again.server.world().grids().get_parked(ship).is_some());
    again.step(1);
    assert!(again.server.world().grids().get(ship).is_none());
    assert_eq!(nodes_kept_of(&again, ship), before);
    // And the ship in memory of the server that saved is where it was.
    assert!(rig.server.world().grids().get(ship).is_some());
    assert_eq!(nodes_kept_of(&rig, ship), before);
}

#[test]
fn a_ship_saved_with_nodes_still_to_be_made_has_none_written_and_the_world_it_starts_again_in_has_no_far_view_of_it()
 {
    let (mut rig, ship) = rig_with_a_ship();
    rig.settle();
    rig.set_channel_in(ship, [8, 8, 8], [9, 9, 9], rig.block, STONE);
    rig.step(1);
    assert!(rig.handle.counts().1 > 0, "nodes are still to be made");
    let mut again = restarted(&mut rig, |_| {});
    again.step(1);
    assert!(again.server.world().grids().get_parked(ship).is_some());
    assert!(nodes_kept_of(&again, ship).is_empty());
    // The server that saved goes on and makes them, and the next save has them.
    rig.settle();
    let after = nodes_kept_of(&rig, ship);
    assert!(!after.is_empty());
    let mut later = restarted(&mut rig, |_| {});
    later.step(1);
    assert_eq!(nodes_kept_of(&later, ship), after);
}

#[test]
fn what_a_save_writes_of_the_nodes_of_a_ship_is_what_it_says_of_the_file_it_made_and_a_planet_has_none()
 {
    let game = Game::new(&world_storage::test_support::ALL_PLUGINS);
    let mut world = game.new_world();
    let made = game.sample_ship(&world, 0);
    let ship = world.grids_mut().create(made);
    let made = game.sample_planet(&world, 0);
    let planet = world.grids_mut().create(made);
    let mut rig = rig_over_files(&game, world, MemoryShipFiles::new(), |config, _| {
        let mut opaque = vec![true; 16];
        opaque[AIR as usize] = false;
        config.table = BlockTable::of_cubes(&opaque);
    });
    rig.settle();
    let save = rig.server.save(&game.registries()).unwrap();
    let names: Vec<(GridId, u64)> = save
        .new_far_files
        .iter()
        .map(|(name, _)| (name.grid, name.serial))
        .collect();
    let saved = save.new_ships();
    let file_of = |grid: GridId| saved.iter().find(|saved| saved.grid == grid).unwrap().file;
    assert_eq!(names, [(ship, file_of(ship).serial)]);
    assert!(!names.iter().any(|(grid, _)| *grid == planet));
}

#[test]
fn a_world_a_server_gives_back_when_it_ends_is_saved_with_the_far_files_of_its_ships_too() {
    let (mut rig, ship) = rig_with_a_ship();
    rig.settle();
    assert!(!nodes_kept_of(&rig, ship).is_empty());
    let game = Game::new(&world_storage::test_support::ALL_PLUGINS);
    // (The server is gone, and what the plugin kept of the ship is still what the world is saved with.)
    let server = rig.server;
    let mut world = server.into_world();
    let save = world.save_with_ships(&game.registries()).unwrap();
    let saved = save.new_ships();
    assert_eq!(saved.len(), 1);
    let names: Vec<FarFileName> = save.new_far_files.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, [FarFileName::of(&saved[0].file)]);
}

#[test]
fn a_ship_with_no_bricks_has_no_far_file_written_at_a_save() {
    let game = Game::new(&world_storage::test_support::ALL_PLUGINS);
    let mut world = game.new_world();
    let mut made = game.sample_ship(&world, 0);
    made.voxels = Grid::new(Arc::clone(world.main().overrides.layout()));
    world.grids_mut().create(made);
    let mut rig = rig_over_files(&game, world, MemoryShipFiles::new(), |config, _| {
        let mut opaque = vec![true; 16];
        opaque[AIR as usize] = false;
        config.table = BlockTable::of_cubes(&opaque);
    });
    rig.settle();
    let save = rig.server.save(&game.registries()).unwrap();
    assert_eq!(save.new_ships().len(), 1, "the ship is saved");
    assert!(save.new_far_files.is_empty(), "and it has no nodes to keep");
}

#[test]
fn the_server_has_work_pending_while_nodes_are_to_be_made_and_not_when_they_are_all_made() {
    let (mut rig, ship) = rig_with_a_ship();
    rig.settle();
    assert!(!rig.server.work_pending(), "everything is made");
    // A block put in the ship makes the nodes over it dirty, and they are made a level at a time.
    rig.set_channel_in(ship, [8, 8, 8], [9, 9, 9], rig.block, STONE);
    rig.step(1);
    assert!(rig.handle.counts().1 > 0, "nodes are still to be made");
    assert!(rig.server.work_pending());
    rig.settle();
    assert!(!rig.server.work_pending());
}
