//! The server half of coarse levels of detail, run by the plugin host on a real server with the pretend game of `world_storage::test_support`:
//! which overrides it keeps, who is sent them, in what order and how fast.

use std::sync::{Arc, Mutex};

use block_models::BlockTable;
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
use server::{GridView, Server, ServerConfig};
use transport::Connection;
use transport::loopback::{LoopbackConnection, loopback_listener};
use voxel_grid::ChannelIndex;
use voxel_space::{BrickCoordinate, GridId, LocalVoxelIndex, VoxelCoordinate, WorldPosition};
use world_storage::WorldData;
use world_storage::test_support::{Game, Plugin as FixturePlugin, id, sample_world};

/// The numbers of `base_blocks:air` and `base_blocks:stone`, the first blocks the pretend game registers.
const AIR: u32 = 0;
const STONE: u32 = 1;
const TICKS_PER_SECOND: u32 = 20;

type Edits = Arc<Mutex<Vec<(ChannelIndex, VoxelCoordinate, u32)>>>;

/// A plugin that makes the edits a test queues, the way a rule of a game does, so that the plugin under test hears of them from the host.
struct Editor {
    channels: Vec<ChannelIndex>,
    edits: Edits,
}

impl Plugin for Editor {
    fn id(&self) -> NamespacedId {
        id("test:editor")
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        let edits = Arc::clone(&self.edits);
        registrar.system(
            SystemSpec::new("test:editor", Phase::Rules).writes(&self.channels),
            move |context| {
                let queued = std::mem::take(&mut *edits.lock().unwrap());
                let mut grid = context.grid(GridId::MAIN).unwrap();
                for (channel, voxel, value) in queued {
                    grid.set(channel, voxel, value).unwrap();
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
    configure(&mut config, temperature);
    let coarse = CoarseServerPlugin::new(config);
    let handle = coarse.handle();
    let edits: Edits = Arc::default();
    let editor = Editor {
        channels: vec![block, temperature],
        edits: Arc::clone(&edits),
    };
    let plugins: Vec<Box<dyn Plugin>> = vec![Box::new(players), Box::new(coarse), Box::new(editor)];
    let host = PluginHost::new(plugins, &world).unwrap();
    let (listener, connector) = loopback_listener();
    let mut server = Server::new(
        ServerConfig::new(id("test_game:game")).with_counted_challenges(1),
        world,
        &game.registries(),
        Box::new(host),
        Box::new(listener),
    )
    .unwrap();
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
        block,
        temperature,
    }
}

impl Rig {
    fn step(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.server.step();
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
        let mut edits = self.edits.lock().unwrap();
        for x in from[0]..to[0] {
            for y in from[1]..to[1] {
                for z in from[2]..to[2] {
                    edits.push((channel, VoxelCoordinate::new(x, y, z), value));
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
    rig.step(5);
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
