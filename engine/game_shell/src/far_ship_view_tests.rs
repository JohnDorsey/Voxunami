//! A ship that is far away in a player's session (`docs/specs/far-nodes-spec.md`, F5): the server tells the player of it with no brick, sends the nodes it is drawn
//! from, and the session draws them; and what is drawn follows the ship as it comes near (bricks) and goes far again (nodes again).

use std::collections::HashMap;
use std::sync::Arc;

use coarse_streaming::{CoarseConfig, CoarseServerPlugin};
use identity::Identity;
use player_presence::PlayersPlugin;
use plugin_host::Plugin;
use region_octree::BrickCube;
use server::{FarShipSettings, Server};
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

/// A ship nobody generates: a slab of stone along its four bricks, standing still.
fn ship(
    world: &world_storage::WorldData,
    block: ChannelIndex,
    stone: u32,
    at: [i128; 3],
) -> MovingGrid {
    let layout = Arc::clone(world.main().overrides.layout());
    let mut voxels = Grid::new(layout);
    for x in 0..128 {
        for y in 0..16 {
            for z in 0..16 {
                voxels.set(block, VoxelCoordinate::new(x, y, z), stone);
            }
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

struct Rig {
    server: Server,
    connector: LoopbackConnector,
    nodes: CoarseWatch,
    ship: GridId,
}

impl Rig {
    /// A server with the ship at the brick `at`.
    fn new(at: [i128; 3]) -> Self {
        let content: ExampleContent = ExampleGame::new_content();
        let mut world = content.new_world(7);
        let block = content.view_channels().blocks;
        let air = content.game.raw_block("base_blocks:air");
        let made = ship(
            &world,
            block,
            content.game.raw_block("base_blocks:stone"),
            at,
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
        let plugins: Vec<Box<dyn Plugin>> = vec![Box::new(players), Box::new(coarse)];
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
        }
    }

    /// A player at `position` who draws what is far away in detail.
    fn join(&mut self, position: [f64; 3]) -> Player {
        let connection: Box<dyn Connection> = Box::new(self.connector.connect().unwrap());
        let (server, nodes) = (&mut self.server, &self.nodes);
        let mut player = Session::<_, ExampleGame>::connect(
            connection,
            &Identity::from_secret([1; 32]),
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
