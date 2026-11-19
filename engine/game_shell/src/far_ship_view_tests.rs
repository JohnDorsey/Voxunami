//! A ship that is far away in a player's session (`docs/specs/far-nodes-spec.md`, F5): the server tells the player of it with no brick, sends the nodes it is drawn
//! from, and the session draws them; and what is drawn follows the ship as it comes near (bricks) and goes far again (nodes again).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use coarse_streaming::{CoarseConfig, CoarseServerPlugin};
use identity::Identity;
use namespaced_id::NamespacedId;
use player_presence::PlayersPlugin;
use plugin_host::{Phase, Plugin, Registrar, SystemSpec};
use region_octree::BrickCube;
use server::{FarShipSettings, GridView, Server};
use transport::Connection;
use transport::loopback::LoopbackConnector;
use voxel_grid::{ChannelIndex, Grid};
use voxel_meshing::PackedBrickMesh;
use voxel_space::{
    BrickBox, BrickCoordinate, GridId, GridPose, LevelIndex, Orientation, VoxelCoordinate,
    WorldPosition,
};
use world_storage::{MovingGrid, PilotLogout};
use world_view::MeshUpdate;

use crate::definition::{CoarseDetail, GameContent, GameDefinition};
use crate::files::{open_recording, record_connection};
use crate::host::build_server_telling_of_far_ships;
use crate::session::{Mode, Session};
use crate::test_support::{CoarseWatch, ExampleContent, ExampleGame};
use crate::view::camera_at;

type Player = Session<Box<dyn Connection>, ExampleGame>;

/// The brick of the level the ship's own origin is in, for a ship far away: 60 bricks along x, and high over the ground.
const SHIP_AT: [i128; 3] = [60, 30, 0];

/// And for a ship that is only just beyond the region the player watches (a few bricks beyond the seven it does), where it is drawn from several nodes.
const SHIP_BESIDE: [i128; 3] = [12, 0, 0];

/// The ship's bricks, in its own coordinates: four along x.
fn ship_bounds() -> BrickBox {
    BrickBox::new(BrickCoordinate::new(0, 0, 0), BrickCoordinate::new(3, 0, 0)).unwrap()
}

/// What the renderer of a player holds of the ship, as the updates say.
#[derive(Default, Debug)]
struct Screen {
    bricks: HashMap<BrickCoordinate, PackedBrickMesh>,
    nodes: HashMap<BrickCube, PackedBrickMesh>,
    /// How many bricks of the ship were ever put on the screen.
    bricks_ever: usize,
    /// How many times the renderer was told to drop the ship.
    dropped: usize,
}

impl Screen {
    fn apply(&mut self, ship: GridId, updates: Vec<MeshUpdate>) {
        for update in updates {
            match update {
                MeshUpdate::UploadGridBrick {
                    grid,
                    coordinate,
                    mesh,
                } => {
                    assert_eq!(grid, ship);
                    if mesh.is_empty() {
                        self.bricks.remove(&coordinate);
                    } else {
                        self.bricks_ever += 1;
                        self.bricks.insert(coordinate, mesh);
                    }
                }
                MeshUpdate::RemoveGridBrick { grid, coordinate } => {
                    assert_eq!(grid, ship);
                    self.bricks.remove(&coordinate);
                }
                MeshUpdate::UploadGridCoarse { grid, cube, mesh } => {
                    assert_eq!(grid, ship);
                    if mesh.is_empty() {
                        self.nodes.remove(&cube);
                    } else {
                        self.nodes.insert(cube, mesh);
                    }
                }
                MeshUpdate::RemoveGridCoarse { grid, cube } => {
                    assert_eq!(grid, ship);
                    self.nodes.remove(&cube);
                }
                MeshUpdate::RemoveGrid(grid) => {
                    assert_eq!(grid, ship);
                    self.bricks.clear();
                    self.nodes.clear();
                    self.dropped += 1;
                }
                _ => {}
            }
        }
    }
}

/// What a ship is made of.
#[derive(Clone, Copy, Debug)]
enum Shape {
    /// A slab of stone along four bricks: `ship_bounds`.
    Slab,
    /// A block of stone in each of two corners of a box of 4 by 4 bricks (the bricks (0, 0, 0) and (3, 3, 0)): the cubes of the box that are between them are empty.
    Corners,
}

/// The bricks a [`Shape::Corners`] ship has.
fn corners_bounds() -> BrickBox {
    BrickBox::new(BrickCoordinate::new(0, 0, 0), BrickCoordinate::new(3, 3, 0)).unwrap()
}

/// A ship nobody generates, standing still.
fn ship(
    world: &world_storage::WorldData,
    block: ChannelIndex,
    stone: u32,
    at: [i128; 3],
    shape: Shape,
) -> MovingGrid {
    let layout = Arc::clone(world.main().overrides.layout());
    let mut voxels = Grid::new(layout);
    let mut fill = |from: [i128; 3], to: [i128; 3]| {
        for x in from[0]..to[0] {
            for y in from[1]..to[1] {
                for z in from[2]..to[2] {
                    voxels.set(block, VoxelCoordinate::new(x, y, z), stone);
                }
            }
        }
    };
    match shape {
        Shape::Slab => fill([0, 0, 0], [128, 16, 16]),
        Shape::Corners => {
            fill([0, 0, 0], [16, 16, 16]);
            fill([96, 96, 0], [112, 112, 16]);
        }
    }
    MovingGrid {
        level: LevelIndex::MAIN,
        pose: GridPose::new(
            WorldPosition::new(BrickCoordinate::new(at[0], at[1], at[2]), [0.0; 3]),
            Orientation::IDENTITY,
        ),
        linear_velocity: [0.0; 3],
        angular_velocity: [0.0; 3],
        mass: 1.0e5,
        centre_of_mass: [0.0; 3],
        held: true,
        frozen: false,
        on_pilot_logout: PilotLogout::Stay,
        voxels,
        terrain: None,
        rail: None,
        sections: Vec::new(),
    }
}

/// A box of voxels of a ship to set to a block: `[low, high)` along each axis.
type Fill = (GridId, [i128; 3], [i128; 3], u32);

/// A plugin that puts the ships a test names into their files, and edits the ships as a test says, in the next tick (what the lifecycle of a game does to a ship nobody is
/// near, and what a player or a rule does to a ship).
#[derive(Debug)]
struct Parker {
    ships: Arc<Mutex<Vec<GridId>>>,
    fills: Arc<Mutex<Vec<Fill>>>,
    block: ChannelIndex,
}

impl Plugin for Parker {
    fn id(&self) -> NamespacedId {
        "test:parker".parse().expect("an identifier")
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        let ships = Arc::clone(&self.ships);
        let fills = Arc::clone(&self.fills);
        let block = self.block;
        registrar.system(
            SystemSpec::new("test:parker", Phase::Rules).writes(&[block]),
            move |context| {
                for (ship, low, high, value) in std::mem::take(&mut *fills.lock().unwrap()) {
                    let mut view = context.grid(ship).unwrap();
                    for x in low[0]..high[0] {
                        for y in low[1]..high[1] {
                            for z in low[2]..high[2] {
                                view.set(block, VoxelCoordinate::new(x, y, z), value)
                                    .unwrap();
                            }
                        }
                    }
                }
                for ship in std::mem::take(&mut *ships.lock().unwrap()) {
                    context.world().park_ship(ship, false).unwrap();
                }
            },
        );
    }
}

struct Rig {
    server: Server,
    connector: LoopbackConnector,
    nodes: CoarseWatch,
    ship: GridId,
    ships_to_park: Arc<Mutex<Vec<GridId>>>,
    fills: Arc<Mutex<Vec<Fill>>>,
}

impl Rig {
    /// A server with a slab ship at the brick `at`.
    fn new(at: [i128; 3]) -> Self {
        Self::shaped(at, Shape::Slab)
    }

    /// A server with a ship of `shape` at the brick `at`.
    fn shaped(at: [i128; 3], shape: Shape) -> Self {
        let content: ExampleContent = ExampleGame::new_content();
        let mut world = content.new_world(7);
        let block = content.view_channels().blocks;
        let air = content.game.raw_block("base_blocks:air");
        let made = ship(
            &world,
            block,
            content.game.raw_block("base_blocks:stone"),
            at,
            shape,
        );
        let ship = world.grids_mut().create(made);
        let players = PlayersPlugin::new(true);
        let mut config = CoarseConfig::new(
            content.block_table(),
            air,
            block,
            detail().unlit,
            ExampleGame::ticks_per_second(),
            players.presence(),
        );
        config.view_distance = detail().maximum_distance;
        let coarse = CoarseServerPlugin::new(config);
        let nodes = CoarseWatch::of(coarse.handle());
        let ships_to_park = Arc::new(Mutex::new(Vec::new()));
        let fills = Arc::new(Mutex::new(Vec::new()));
        let parker = Parker {
            ships: Arc::clone(&ships_to_park),
            fills: Arc::clone(&fills),
            block,
        };
        let plugins: Vec<Box<dyn Plugin>> =
            vec![Box::new(players), Box::new(coarse), Box::new(parker)];
        let (server, connector) = build_server_telling_of_far_ships::<ExampleGame>(
            &content,
            world,
            3,
            plugins,
            FarShipSettings {
                minimum_apparent_size: 0.005,
                maximum_ships: 4,
            },
        )
        .unwrap();
        Self {
            server,
            connector,
            nodes,
            ship,
            ships_to_park,
            fills,
        }
    }

    /// Sets the voxels of the ship in `[low, high)` to the block `value`, in the next tick.
    fn fill(&mut self, low: [i128; 3], high: [i128; 3], value: u32) {
        self.fills
            .lock()
            .unwrap()
            .push((self.ship, low, high, value));
    }

    /// Lets the server make the nodes of the ship, and then puts the ship into its file, out of memory.
    fn put_the_ship_in_its_file(&mut self) {
        let mut ticks = 0;
        while !self.nodes.is_done() || ticks < 3 {
            self.nodes.step(&mut self.server);
            ticks += 1;
            assert!(ticks < 200, "the server is still making nodes");
        }
        self.ships_to_park.lock().unwrap().push(self.ship);
        self.nodes.step(&mut self.server);
        assert!(self.server.world().grids().get(self.ship).is_none());
        assert!(self.server.world().grids().get_parked(self.ship).is_some());
    }

    /// A player at `position` who draws what is far away in detail.
    fn join(&mut self, position: [f64; 3]) -> Player {
        self.join_as(1, position)
    }

    /// The same, as the install numbered `who`.
    fn join_as(&mut self, who: u8, position: [f64; 3]) -> Player {
        let connection: Box<dyn Connection> = Box::new(self.connector.connect().unwrap());
        let (server, nodes) = (&mut self.server, &self.nodes);
        let mut player = Session::<_, ExampleGame>::connect(
            connection,
            &Identity::from_secret([who; 32]),
            Mode::Playing,
            3,
            camera_at(position, 0.0, 0.0),
            || {
                nodes.step(server);
            },
        )
        .unwrap();
        player.enable_grid_detail(detail());
        player
    }

    /// Runs until the player has what the server has to send for where it is.
    fn settle(&mut self, player: &mut Player, screen: &mut Screen) {
        player.send_pose();
        let mut rounds = 0;
        let mut rounds_after_done = 0;
        while rounds < 4 || rounds_after_done < 2 {
            let (server, nodes) = (&mut self.server, &self.nodes);
            let updates = player.settle(|| nodes.step(server)).unwrap();
            screen.apply(self.ship, updates);
            rounds += 1;
            if self.nodes.is_done() {
                rounds_after_done += 1;
            }
            assert!(rounds < 200, "the server is still making nodes");
        }
    }
}

fn detail() -> CoarseDetail {
    CoarseDetail {
        maximum_distance: 4000.0,
        unlit: 0,
        cell_pixels: 3.0,
        bytes_per_second: 10_000_000,
    }
}

/// Over the ground, near the origin: the ship is 1,900 voxels away.
const FAR_FROM_THE_SHIP: [f64; 3] = [5.5, 14.0, 5.5];

/// Inside the box of the ship's bricks.
fn at_the_ship(at: [i128; 3]) -> [f64; 3] {
    [
        (at[0] * 32 + 20) as f64,
        (at[1] * 32 + 20) as f64,
        (at[2] * 32 + 20) as f64,
    ]
}

/// Whether the cube holds the brick.
fn holds(cube: BrickCube, brick: BrickCoordinate) -> bool {
    cube.contains(brick)
}

#[test]
fn a_ship_far_away_is_drawn_from_the_nodes_the_server_sends_and_in_no_brick() {
    let mut rig = Rig::new(SHIP_AT);
    let mut player = rig.join(FAR_FROM_THE_SHIP);
    let mut screen = Screen::default();
    rig.settle(&mut player, &mut screen);

    let held = player.client().world().grids().get(rig.ship).unwrap();
    assert_eq!(held.far, Some(ship_bounds()));
    assert_eq!(held.voxels.brick_count(), 0, "no brick of it was sent");
    assert_eq!(screen.bricks_ever, 0);
    assert!(screen.bricks.is_empty());

    // Nodes of the ship, and only where it is: each has a brick of the ship in it, and the four bricks are each under one of them and under only one.
    assert!(!screen.nodes.is_empty(), "{:?}", screen.nodes.keys());
    for cube in screen.nodes.keys() {
        assert!(cube.level() >= 1);
        assert!(cube.meets(ship_bounds()), "{cube:?}");
    }
    for x in 0..4 {
        let brick = BrickCoordinate::new(x, 0, 0);
        let covering = screen
            .nodes
            .keys()
            .filter(|cube| holds(**cube, brick))
            .count();
        assert_eq!(covering, 1, "brick {x} is under {covering} nodes");
    }
}

#[test]
fn a_ship_the_player_comes_to_is_drawn_in_bricks_instead_of_nodes_and_in_nodes_again_when_the_player_goes_away()
 {
    let mut rig = Rig::new(SHIP_AT);
    let mut player = rig.join(FAR_FROM_THE_SHIP);
    let mut screen = Screen::default();
    rig.settle(&mut player, &mut screen);
    let far_nodes: Vec<BrickCube> = {
        let mut cubes: Vec<BrickCube> = screen.nodes.keys().copied().collect();
        cubes.sort_by_key(|cube| (cube.level(), cube.origin()));
        cubes
    };
    assert!(!far_nodes.is_empty());

    // Near: the ship is told of again with its bricks, which are drawn, and no node of it is.
    player.camera.position = WorldPosition::from_voxels(at_the_ship(SHIP_AT));
    rig.settle(&mut player, &mut screen);
    assert!(screen.nodes.is_empty(), "{:?}", screen.nodes.keys());
    assert_eq!(screen.bricks.len(), 4);
    let held = player.client().world().grids().get(rig.ship).unwrap();
    assert_eq!((held.far, held.voxels.brick_count()), (None, 4));
    assert_eq!(screen.dropped, 1);

    // Away again: no brick, and the same nodes as the first time.
    player.camera.position = WorldPosition::from_voxels(FAR_FROM_THE_SHIP);
    rig.settle(&mut player, &mut screen);
    assert!(screen.bricks.is_empty());
    let held = player.client().world().grids().get(rig.ship).unwrap();
    assert_eq!(
        (held.far, held.voxels.brick_count()),
        (Some(ship_bounds()), 0)
    );
    let mut again: Vec<BrickCube> = screen.nodes.keys().copied().collect();
    again.sort_by_key(|cube| (cube.level(), cube.origin()));
    assert_eq!(again, far_nodes);
    assert_eq!(screen.dropped, 2);
}

#[test]
fn a_ship_just_beyond_the_region_is_drawn_from_several_nodes_that_cover_each_of_its_bricks_once() {
    let mut rig = Rig::new(SHIP_BESIDE);
    let mut player = rig.join(FAR_FROM_THE_SHIP);
    let mut screen = Screen::default();
    rig.settle(&mut player, &mut screen);
    let held = player.client().world().grids().get(rig.ship).unwrap();
    assert_eq!(held.far, Some(ship_bounds()));
    assert_eq!(screen.bricks_ever, 0);
    assert!(screen.nodes.len() > 1, "{:?}", screen.nodes.keys());
    for x in 0..4 {
        let brick = BrickCoordinate::new(x, 0, 0);
        let covering: Vec<BrickCube> = screen
            .nodes
            .keys()
            .copied()
            .filter(|cube| holds(*cube, brick))
            .collect();
        assert_eq!(covering.len(), 1, "brick {x}: {covering:?}");
    }
}

#[test]
fn a_recording_of_a_player_who_saw_a_ship_far_away_shows_the_same_nodes() {
    let directory =
        std::env::temp_dir().join(format!("voxunami_far_ship_view_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("session.voxreplay");
    let mut rig = Rig::new(SHIP_BESIDE);
    let recorded: Box<dyn Connection> =
        record_connection(rig.connector.connect().unwrap(), &path).unwrap();
    let (server, nodes) = (&mut rig.server, &rig.nodes);
    let mut player = Session::<_, ExampleGame>::connect(
        recorded,
        &Identity::from_secret([1; 32]),
        Mode::Playing,
        3,
        camera_at(FAR_FROM_THE_SHIP, 0.0, 0.0),
        || {
            nodes.step(server);
        },
    )
    .unwrap();
    player.enable_grid_detail(detail());
    let mut screen = Screen::default();
    rig.settle(&mut player, &mut screen);
    assert!(screen.nodes.len() > 1);
    drop(player);

    let connection: Box<dyn Connection> = Box::new(open_recording(&path).unwrap());
    let mut replay = Session::<_, ExampleGame>::connect(
        connection,
        &Identity::generate(),
        Mode::Replaying,
        3,
        camera_at(FAR_FROM_THE_SHIP, 0.0, 0.0),
        || {},
    )
    .unwrap();
    replay.enable_grid_detail(detail());
    while replay.advance_one_tick().unwrap_or(false) {}
    let mut replay_screen = Screen::default();
    replay_screen.apply(rig.ship, replay.settle(|| {}).unwrap());
    assert_eq!(replay_screen.bricks_ever, 0);
    assert_eq!(replay_screen.nodes.len(), screen.nodes.len());
    for (cube, mesh) in &screen.nodes {
        assert_eq!(replay_screen.nodes.get(cube), Some(mesh), "{cube:?}");
    }
    std::fs::remove_dir_all(&directory).unwrap();
}

/// Which of `bricks` have a node over them.
fn bricks_with_a_node(screen: &Screen, bricks: &[BrickCoordinate]) -> usize {
    bricks
        .iter()
        .filter(|brick| screen.nodes.keys().any(|cube| holds(*cube, **brick)))
        .count()
}

/// A player flies at `bricks_per_round` along x towards a ship at the brick 140 on x, from 120 bricks to 12 bricks short of it, while the server hears of where
/// the player was `lag` rounds ago (the client acts on where it is at once; the server answers after it has heard). Returns how many rounds some brick of the
/// ship had no node over it, and the screen at the end.
fn fly_to_a_ship(
    shape: Shape,
    bricks_to_look_at: &[BrickCoordinate],
    bricks_per_round: f64,
    lag: i64,
) -> (usize, Screen, Rig, Player) {
    fly_to_a_ship_in(shape, false, bricks_to_look_at, bricks_per_round, lag)
}

/// [`fly_to_a_ship`], with the ship in its file (out of memory) from before the player comes if `in_a_file`.
fn fly_to_a_ship_in(
    shape: Shape,
    in_a_file: bool,
    bricks_to_look_at: &[BrickCoordinate],
    bricks_per_round: f64,
    lag: i64,
) -> (usize, Screen, Rig, Player) {
    let mut rig = Rig::shaped([140, 0, 0], shape);
    if in_a_file {
        rig.put_the_ship_in_its_file();
    }
    let start = 20.0 * 32.0 + 16.0;
    let end = 128.0 * 32.0 + 16.0;
    let at = |round: i64| (start + bricks_per_round * 32.0 * (round + 1).max(0) as f64).min(end);
    let mut player = rig.join([start, 14.0, 16.0]);
    let mut screen = Screen::default();
    rig.settle(&mut player, &mut screen);
    assert_eq!(
        bricks_with_a_node(&screen, bricks_to_look_at),
        bricks_to_look_at.len(),
        "it starts with a picture"
    );
    let mut rounds_with_a_hole = 0;
    let mut round = 0;
    while at(round - 1) < end || round < 40 {
        player.camera.position = WorldPosition::from_voxels([at(round), 14.0, 16.0]);
        let updates = player.settle(|| {}).unwrap();
        // What the server hears is where the player was some rounds ago.
        player.camera.position = WorldPosition::from_voxels([at(round - lag), 14.0, 16.0]);
        player.send_pose();
        player.camera.position = WorldPosition::from_voxels([at(round), 14.0, 16.0]);
        rig.nodes.step(&mut rig.server);
        screen.apply(rig.ship, updates);
        if bricks_with_a_node(&screen, bricks_to_look_at) < bricks_to_look_at.len() {
            rounds_with_a_hole += 1;
        }
        round += 1;
        assert!(round < 400);
    }
    (rounds_with_a_hole, screen, rig, player)
}

#[test]
fn a_ship_a_player_closes_on_faster_than_the_server_hears_of_it_does_not_vanish_while_the_finer_nodes_are_on_their_way()
 {
    let bricks: Vec<BrickCoordinate> = (0..4).map(|x| BrickCoordinate::new(x, 0, 0)).collect();
    // (Measured before the fix: 6 bricks a round with the server ten rounds behind left a hole for six rounds, 20 a round and three behind for two, and a jump of
    // a hundred bricks for one round, whatever the lag.)
    for (speed, lag) in [(6.0, 10), (20.0, 3), (20.0, 10), (200.0, 0), (200.0, 3)] {
        let (holes, screen, ..) = fly_to_a_ship(Shape::Slab, &bricks, speed, lag);
        assert_eq!(
            holes, 0,
            "speed {speed} bricks a round, the server {lag} rounds behind"
        );
        // And the picture is the finest there is once the server has answered.
        assert!(
            screen.nodes.keys().all(|cube| cube.level() == 1),
            "speed {speed}, lag {lag}: {:?}",
            screen.nodes.keys()
        );
    }
}

#[test]
fn a_ship_with_empty_cubes_in_its_box_is_drawn_in_its_finest_nodes_when_the_player_is_near_and_has_no_hole_on_the_way()
 {
    // The cubes of level 1 that are in the box and have no brick of the ship are empty, and the server has to say so: the client cannot tell that from a node that
    // has not come yet, and does not draw the finer picture until it knows what is in every cube of it.
    let looked_at = [BrickCoordinate::new(0, 0, 0), BrickCoordinate::new(3, 3, 0)];
    for (speed, lag) in [(2.0, 0), (20.0, 3)] {
        let (holes, screen, rig, player) = fly_to_a_ship(Shape::Corners, &looked_at, speed, lag);
        assert_eq!(holes, 0, "speed {speed}, lag {lag}");
        let held = player.client().world().grids().get(rig.ship).unwrap();
        assert_eq!(held.far, Some(corners_bounds()));
        assert_eq!(
            screen.nodes.len(),
            2,
            "speed {speed}, lag {lag}: {:?}",
            screen.nodes.keys()
        );
        assert!(screen.nodes.keys().all(|cube| cube.level() == 1));
    }
}

// ---- a ship in a file ----

#[test]
fn a_ship_in_its_file_is_drawn_from_the_nodes_the_server_kept_of_it_as_one_in_memory_is_and_in_no_brick()
 {
    let mut rig = Rig::new(SHIP_AT);
    rig.put_the_ship_in_its_file();
    let mut player = rig.join(FAR_FROM_THE_SHIP);
    let mut screen = Screen::default();
    rig.settle(&mut player, &mut screen);
    let held = player.client().world().grids().get(rig.ship).unwrap();
    assert_eq!(held.far, Some(ship_bounds()));
    assert_eq!(held.voxels.brick_count(), 0);
    assert_eq!(screen.bricks_ever, 0);
    assert!(!screen.nodes.is_empty());
    for x in 0..4 {
        let brick = BrickCoordinate::new(x, 0, 0);
        let covering = screen
            .nodes
            .keys()
            .filter(|cube| holds(**cube, brick))
            .count();
        assert_eq!(covering, 1, "brick {x} is under {covering} nodes");
    }
}

#[test]
fn what_a_player_sees_of_a_ship_is_the_same_whether_it_is_in_memory_or_in_its_file() {
    let picture = |in_a_file: bool| {
        let mut rig = Rig::shaped(SHIP_BESIDE, Shape::Corners);
        if in_a_file {
            rig.put_the_ship_in_its_file();
        }
        let mut player = rig.join(FAR_FROM_THE_SHIP);
        let mut screen = Screen::default();
        rig.settle(&mut player, &mut screen);
        let mut nodes: Vec<(BrickCube, PackedBrickMesh)> = screen.nodes.into_iter().collect();
        nodes.sort_by_key(|(cube, _)| (cube.level(), cube.origin()));
        nodes
    };
    let in_memory = picture(false);
    assert!(in_memory.len() >= 2);
    assert_eq!(picture(true), in_memory);
}

#[test]
fn a_ship_in_its_file_is_flown_to_with_no_hole_and_drawn_in_its_finest_nodes_at_the_end() {
    let looked_at = [BrickCoordinate::new(0, 0, 0), BrickCoordinate::new(3, 3, 0)];
    for (speed, lag) in [(2.0, 0), (20.0, 3)] {
        let (holes, screen, rig, player) =
            fly_to_a_ship_in(Shape::Corners, true, &looked_at, speed, lag);
        assert_eq!(holes, 0, "speed {speed}, lag {lag}");
        assert_eq!(screen.nodes.len(), 2, "{:?}", screen.nodes.keys());
        assert!(screen.nodes.keys().all(|cube| cube.level() == 1));
        let held = player.client().world().grids().get(rig.ship).unwrap();
        assert_eq!(held.far, Some(corners_bounds()));
        assert!(rig.server.world().grids().get(rig.ship).is_none());
    }
}

// ---- edits to a far ship ----

/// What a player's renderer holds of the ship's nodes, in order.
fn picture(screen: &Screen) -> Vec<(BrickCube, PackedBrickMesh)> {
    let mut nodes: Vec<(BrickCube, PackedBrickMesh)> = screen
        .nodes
        .iter()
        .map(|(cube, mesh)| (*cube, mesh.clone()))
        .collect();
    nodes.sort_by_key(|(cube, _)| (cube.level(), cube.origin()));
    nodes
}

/// A player who was there when the ship was edited, and one who comes after, see the same of it; and what they see of the places that were edited is not what the first
/// saw before.
fn two_players_see_the_same_after(
    shape: Shape,
    at: [i128; 3],
    edit: impl FnOnce(&mut Rig),
) -> (Rig, Player, Screen, Vec<(BrickCube, PackedBrickMesh)>) {
    let mut rig = Rig::shaped(at, shape);
    let mut watcher = rig.join(FAR_FROM_THE_SHIP);
    let mut screen = Screen::default();
    rig.settle(&mut watcher, &mut screen);
    let before = picture(&screen);
    assert!(!before.is_empty());
    edit(&mut rig);
    rig.settle(&mut watcher, &mut screen);
    let mut newcomer = rig.join_as(2, FAR_FROM_THE_SHIP);
    let mut newcomers = Screen::default();
    rig.settle(&mut newcomer, &mut newcomers);
    assert_eq!(
        picture(&screen),
        picture(&newcomers),
        "the player who saw the edit and the one who came after do not see the same"
    );
    (rig, watcher, screen, before)
}

#[test]
fn an_edit_inside_a_far_ship_is_seen_by_the_player_who_watches_and_the_one_who_comes_after() {
    // A hole is dug in the second brick of the slab (not an end brick, so the box of the ship stays).
    let (rig, watcher, screen, before) =
        two_players_see_the_same_after(Shape::Slab, SHIP_BESIDE, |rig| {
            rig.fill([40, 2, 2], [56, 12, 12], 0);
        });
    assert_ne!(picture(&screen), before, "the edit changed what is drawn");
    let held = watcher.client().world().grids().get(rig.ship).unwrap();
    assert_eq!(held.far, Some(ship_bounds()));
}

#[test]
fn an_edit_that_fills_a_cube_that_was_empty_in_the_box_of_a_far_ship_draws_it() {
    // The ship has a block in two corners of its box of 4 by 4 bricks, so that two of the cubes of level 1 are empty; a block put in one of them makes it held.
    let at = SHIP_BESIDE;
    let (_, _, screen, before) = two_players_see_the_same_after(Shape::Corners, at, |rig| {
        rig.fill([100, 5, 5], [108, 13, 13], 2);
    });
    let brick = BrickCoordinate::new(3, 0, 0);
    assert!(
        !before.iter().any(|(cube, _)| holds(*cube, brick)),
        "the cube was empty"
    );
    assert!(
        screen.nodes.keys().any(|cube| holds(*cube, brick)),
        "and is drawn now: {:?}",
        screen.nodes.keys()
    );
}

#[test]
fn an_edit_that_makes_a_far_ship_bigger_is_drawn_where_it_grew() {
    // A block put a brick beyond the end of the slab: the box of the ship is 6 bricks long now, and the player was told of 4.
    let (rig, watcher, screen, before) =
        two_players_see_the_same_after(Shape::Slab, SHIP_BESIDE, |rig| {
            rig.fill([170, 2, 2], [178, 10, 10], 2);
        });
    let grown = BrickCoordinate::new(5, 0, 0);
    assert!(!before.iter().any(|(cube, _)| holds(*cube, grown)));
    assert!(
        screen.nodes.keys().any(|cube| holds(*cube, grown)),
        "{:?}",
        screen.nodes.keys()
    );
    let held = watcher.client().world().grids().get(rig.ship).unwrap();
    assert_eq!(
        held.far,
        Some(BrickBox::new(BrickCoordinate::new(0, 0, 0), BrickCoordinate::new(5, 0, 0)).unwrap())
    );
}

#[test]
fn digging_away_an_end_brick_of_a_far_ship_leaves_its_box_and_the_two_players_see_the_same() {
    // The last brick of the slab is dug to air. An empty brick is still the ship's, so the box is what it was; and what the player who was there sees is the finest, as a
    // new player's is.
    let (rig, watcher, screen, before) =
        two_players_see_the_same_after(Shape::Slab, SHIP_BESIDE, |rig| {
            rig.fill([96, 0, 0], [128, 16, 16], 0);
        });
    assert!(
        screen.nodes.keys().all(|cube| cube.level() == 1),
        "{:?}",
        screen.nodes.keys()
    );
    assert_ne!(picture(&screen), before, "the dig changed what is drawn");
    let held = watcher.client().world().grids().get(rig.ship).unwrap();
    assert_eq!(held.far, Some(ship_bounds()));
}

#[test]
fn digging_away_all_that_is_in_a_cube_of_a_far_ship_takes_its_picture_away_for_both_players() {
    // The block in the first corner of the ship is dug away: the cube of level 1 that held it has nothing to draw, and the server says it is empty (the node it had is taken
    // back, which for a ship that nothing generates is the same).
    let first = BrickCoordinate::new(0, 0, 0);
    let (_, _, screen, before) =
        two_players_see_the_same_after(Shape::Corners, SHIP_BESIDE, |rig| {
            rig.fill([0, 0, 0], [16, 16, 16], 0);
        });
    assert!(
        before.iter().any(|(cube, _)| holds(*cube, first)),
        "it was drawn"
    );
    assert!(
        !screen.nodes.keys().any(|cube| holds(*cube, first)),
        "and is not now: {:?}",
        screen.nodes.keys()
    );
    // What is left of the ship is drawn.
    assert!(
        screen
            .nodes
            .keys()
            .any(|cube| holds(*cube, BrickCoordinate::new(3, 3, 0)))
    );
}
