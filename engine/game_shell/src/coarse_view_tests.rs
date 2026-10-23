//! Terrain far away, drawn as coarse nodes (`docs/lod-spec.md`): an edit made by the server shows in the coarse view of a player who is far from it, a player
//! who arrives later sees the same, and a recording shows what the live player saw. The terrain is the flat ground of the example game.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use coarse_streaming::{CoarseConfig, CoarseServerPlugin};
use identity::Identity;
use namespaced_id::NamespacedId;
use player_presence::PlayersPlugin;
use plugin_host::{Phase, Plugin, Registrar, SystemSpec};
use region_octree::BrickCube;
use server::{GridView, Server};
use transport::Connection;
use transport::loopback::LoopbackConnector;
use voxel_grid::ChannelIndex;
use voxel_meshing::PackedBrickMesh;
use voxel_space::{BrickCoordinate, GridId, VoxelCoordinate};
use world_view::MeshUpdate;

use crate::definition::{CoarseDetail, GameContent, GameDefinition};
use crate::files::{open_recording, record_connection};
use crate::host::build_server_with_plugins;
use crate::session::{Mode, Session};
use crate::test_support::{ExampleContent, ExampleGame};
use crate::view::camera_at;

type Player = Session<Box<dyn Connection>, ExampleGame>;

/// What the renderer of a player holds, as the updates say.
#[derive(Default, Debug)]
struct Screen {
    bricks: HashMap<BrickCoordinate, PackedBrickMesh>,
    nodes: HashMap<BrickCube, PackedBrickMesh>,
}

impl Screen {
    fn apply(&mut self, updates: Vec<MeshUpdate>) {
        for update in updates {
            match update {
                MeshUpdate::Upload { coordinate, mesh } if mesh.is_empty() => {
                    self.bricks.remove(&coordinate);
                }
                MeshUpdate::Upload { coordinate, mesh } => {
                    self.bricks.insert(coordinate, mesh);
                }
                MeshUpdate::Remove(coordinate) => {
                    self.bricks.remove(&coordinate);
                }
                MeshUpdate::UploadCoarse { cube, mesh } if mesh.is_empty() => {
                    self.nodes.remove(&cube);
                }
                MeshUpdate::UploadCoarse { cube, mesh } => {
                    self.nodes.insert(cube, mesh);
                }
                MeshUpdate::RemoveCoarse(cube) => {
                    self.nodes.remove(&cube);
                }
                _ => {}
            }
        }
    }
}

fn detail() -> CoarseDetail {
    CoarseDetail {
        maximum_distance: 700.0,
        unlit: 0,
        cell_pixels: 3.0,
        bytes_per_second: 1_000_000,
    }
}

/// Digs out the voxels it is given, when the server ticks, as a rule of the game would.
#[derive(Debug)]
struct Digger {
    block: ChannelIndex,
    air: u32,
    queue: Arc<Mutex<Vec<VoxelCoordinate>>>,
}

impl Plugin for Digger {
    fn id(&self) -> NamespacedId {
        "test:digger".parse().unwrap()
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        let (block, air) = (self.block, self.air);
        let queue = Arc::clone(&self.queue);
        registrar.system(
            SystemSpec::new("test:digger", Phase::Rules).writes(&[block]),
            move |context| {
                let voxels = std::mem::take(&mut *queue.lock().unwrap());
                if voxels.is_empty() {
                    return;
                }
                let mut grid = context.grid(GridId::MAIN).expect("the main level is there");
                for voxel in voxels {
                    grid.set(block, voxel, air).expect("a block of the game");
                }
            },
        );
    }
}

struct Rig {
    server: Server,
    connector: LoopbackConnector,
    queue: Arc<Mutex<Vec<VoxelCoordinate>>>,
}

impl Rig {
    fn new() -> Self {
        let content: ExampleContent = ExampleGame::new_content();
        let world = content.new_world(7);
        let players = PlayersPlugin::new(true);
        let block = content.view_channels().blocks;
        let air = content.game.raw_block("base_blocks:air");
        let mut config = CoarseConfig::new(
            content.block_table(),
            air,
            block,
            detail().unlit,
            ExampleGame::ticks_per_second(),
            players.presence(),
        );
        config.view_distance = detail().maximum_distance;
        let queue: Arc<Mutex<Vec<VoxelCoordinate>>> = Arc::default();
        let digger = Digger {
            block,
            air,
            queue: Arc::clone(&queue),
        };
        let plugins: Vec<Box<dyn Plugin>> = vec![
            Box::new(players),
            Box::new(CoarseServerPlugin::new(config)),
            Box::new(digger),
        ];
        let (server, connector) =
            build_server_with_plugins::<ExampleGame>(&content, world, 3, plugins).unwrap();
        Self {
            server,
            connector,
            queue,
        }
    }

    /// A player who draws far terrain, at `position`, over a connection.
    fn join(&mut self, connection: Box<dyn Connection>, who: u8, position: [f64; 3]) -> Player {
        let server = &mut self.server;
        let mut player = Session::<_, ExampleGame>::connect(
            connection,
            &Identity::from_secret([who; 32]),
            Mode::Playing,
            3,
            camera_at(position, 0.0, 0.0),
            || {
                server.step();
            },
        )
        .unwrap();
        player.enable_coarse_detail(detail());
        player
    }

    fn connection(&self) -> Box<dyn Connection> {
        Box::new(self.connector.connect().unwrap())
    }

    /// Takes in everything the server has said and the player has to do about it, until the view is quiet.
    fn settle(&mut self, player: &mut Player, screen: &mut Screen) {
        player.send_pose();
        for _ in 0..3 {
            let server = &mut self.server;
            let updates = player
                .settle(|| {
                    server.step();
                })
                .unwrap();
            screen.apply(updates);
        }
    }

    /// Digs a crater 16 voxels square in the ground at the origin.
    fn dig_a_crater(&self) {
        let voxels = (0..16).flat_map(|x| {
            (0..16).flat_map(move |z| (0..10).map(move |y| VoxelCoordinate::new(x, y, z)))
        });
        self.queue.lock().unwrap().extend(voxels);
    }
}

/// Where the watcher is: 350 voxels from the crater, in the air.
const WATCHER: [f64; 3] = [358.5, 60.0, 8.5];

fn assert_same_nodes(found: &Screen, expected: &Screen) {
    assert_eq!(found.nodes.len(), expected.nodes.len());
    for (cube, mesh) in &expected.nodes {
        assert_eq!(found.nodes.get(cube), Some(mesh), "{cube:?}");
    }
}

#[test]
fn an_edit_far_away_shows_in_the_coarse_view_of_a_player_who_is_far_from_it_and_a_late_arrival_sees_the_same()
 {
    let mut rig = Rig::new();
    let connection = rig.connection();
    let mut watcher = rig.join(connection, 1, WATCHER);
    let mut screen = Screen::default();
    rig.settle(&mut watcher, &mut screen);
    let before: HashMap<BrickCube, PackedBrickMesh> = screen.nodes.clone();
    assert!(
        before.len() > 50,
        "the watcher sees terrain far away: {} nodes",
        before.len()
    );

    rig.dig_a_crater();
    rig.settle(&mut watcher, &mut screen);

    // Some node over the crater is drawn differently, and none that is not over it.
    let changed: Vec<BrickCube> = screen
        .nodes
        .iter()
        .filter(|(cube, mesh)| before.get(cube) != Some(mesh))
        .map(|(cube, _)| *cube)
        .chain(
            before
                .keys()
                .filter(|cube| !screen.nodes.contains_key(cube))
                .copied(),
        )
        .collect();
    assert!(!changed.is_empty(), "the crater is not in the picture");
    for cube in &changed {
        let origin = cube.origin();
        let edge = cube.edge_length() as i64 * 32;
        let (x, y, z) = (origin.x() * 32, origin.y() * 32, origin.z() * 32);
        assert!(
            x < 16 && x + edge > 0 && z < 16 && z + edge > 0 && y < 10 && y + edge > 0,
            "{cube:?} changed and is not over the crater"
        );
    }

    // A player who arrives later, where the watcher is, sees what the watcher sees.
    let connection = rig.connection();
    let mut late = rig.join(connection, 2, WATCHER);
    let mut late_screen = Screen::default();
    rig.settle(&mut late, &mut late_screen);
    assert_same_nodes(&late_screen, &screen);
}

#[test]
fn a_recording_of_the_watcher_shows_what_the_watcher_saw() {
    let directory =
        std::env::temp_dir().join(format!("voxunami_coarse_view_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("session.voxreplay");
    let mut rig = Rig::new();
    let recorded: Box<dyn Connection> =
        record_connection(rig.connector.connect().unwrap(), &path).unwrap();
    let mut watcher = rig.join(recorded, 1, WATCHER);
    let mut screen = Screen::default();
    rig.settle(&mut watcher, &mut screen);
    rig.dig_a_crater();
    rig.settle(&mut watcher, &mut screen);
    drop(watcher);

    let connection: Box<dyn Connection> = Box::new(open_recording(&path).unwrap());
    let mut replay = Session::<_, ExampleGame>::connect(
        connection,
        &Identity::generate(),
        Mode::Replaying,
        3,
        camera_at([0.0; 3], 0.0, 0.0),
        || {},
    )
    .unwrap();
    replay.enable_coarse_detail(detail());
    while replay.advance_one_tick().unwrap_or(false) {}
    let mut replay_screen = Screen::default();
    replay_screen.apply(replay.settle(|| {}).unwrap());

    assert!(replay_screen.nodes.len() > 50);
    assert_same_nodes(&replay_screen, &screen);
    std::fs::remove_dir_all(&directory).unwrap();
}
