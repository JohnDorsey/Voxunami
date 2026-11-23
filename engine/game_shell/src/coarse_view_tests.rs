//! Terrain far away, drawn as coarse nodes (`docs/specs/lod-spec.md`): an edit made by the server shows in the coarse view of a player who is far from it, a player
//! who arrives later sees the same, and a recording shows what the live player saw. The terrain is the flat ground of the example game.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use coarse_streaming::{CoarseConfig, CoarseServerPlugin, DetailPlace};
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
use voxel_space::{BrickCoordinate, GridId, LevelIndex, VoxelCoordinate};
use world_view::MeshUpdate;

use crate::definition::{CoarseDetail, GameContent, GameDefinition};
use crate::files::{open_recording, record_connection};
use crate::host::build_server_with_plugins;
use crate::session::{Mode, Session};
use crate::test_support::{
    CoarseWatch, ExampleContent, ExampleDetailGame, ExampleGame, ExampleOneCoarseLevelGame,
};
use crate::view::camera_at;

type Player = Session<Box<dyn Connection>, ExampleGame>;

/// What the renderer of a player holds, as the updates say.
#[derive(Default, Debug)]
struct Screen {
    bricks: HashMap<BrickCoordinate, PackedBrickMesh>,
    nodes: HashMap<BrickCube, PackedBrickMesh>,
    /// The nodes the renderer holds and does not draw.
    hidden_nodes: HashMap<BrickCube, PackedBrickMesh>,
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
                    self.hidden_nodes.remove(&cube);
                }
                MeshUpdate::UploadHiddenCoarse { cube, mesh } => {
                    if let Some(drawn) = self.nodes.get_mut(&cube) {
                        *drawn = mesh;
                    } else {
                        self.hidden_nodes.insert(cube, mesh);
                    }
                }
                MeshUpdate::HideCoarse(cube) => {
                    let mesh = self.nodes.remove(&cube).expect("it was drawn");
                    self.hidden_nodes.insert(cube, mesh);
                }
                MeshUpdate::ShowCoarse(cube) => {
                    let mesh = self.hidden_nodes.remove(&cube).expect("it was hidden");
                    self.nodes.insert(cube, mesh);
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

/// Digs out the voxels it is given (of one grid), when the server ticks, as a rule of the game would.
#[derive(Debug)]
pub(crate) struct Digger {
    grid: GridId,
    block: ChannelIndex,
    air: u32,
    queue: Arc<Mutex<Vec<VoxelCoordinate>>>,
}

impl Digger {
    pub(crate) fn new(
        grid: GridId,
        block: ChannelIndex,
        air: u32,
        queue: Arc<Mutex<Vec<VoxelCoordinate>>>,
    ) -> Self {
        Self {
            grid,
            block,
            air,
            queue,
        }
    }
}

impl Plugin for Digger {
    fn id(&self) -> NamespacedId {
        "test:digger".parse().unwrap()
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        let (grid, block, air) = (self.grid, self.block, self.air);
        let queue = Arc::clone(&self.queue);
        registrar.system(
            SystemSpec::new("test:digger", Phase::Rules).writes(&[block]),
            move |context| {
                let voxels = std::mem::take(&mut *queue.lock().unwrap());
                if voxels.is_empty() {
                    return;
                }
                let mut grid = context.grid(grid).expect("the grid is there");
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
    nodes: CoarseWatch,
}

impl Rig {
    fn new() -> Self {
        Self::of::<ExampleGame>()
    }

    /// A server of the world of `ExampleContent` (which has a second level, `test_content:other`, of flat ground) for the game `G`.
    fn of<G: GameDefinition<Content = ExampleContent>>() -> Self {
        let content: ExampleContent = G::new_content();
        let mut world = content.new_world(7);
        world
            .add_level(
                "test_content:other".parse().unwrap(),
                content.game.new_level("base_terrain:flat_v1"),
            )
            .unwrap();
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
        let digger = Digger::new(GridId::MAIN, block, air, Arc::clone(&queue));
        let coarse = CoarseServerPlugin::new(config);
        let nodes = CoarseWatch::of(coarse.handle());
        let plugins: Vec<Box<dyn Plugin>> =
            vec![Box::new(players), Box::new(coarse), Box::new(digger)];
        let (server, connector) =
            build_server_with_plugins::<G>(&content, world, 3, plugins).unwrap();
        Self {
            server,
            connector,
            queue,
            nodes,
        }
    }

    /// A player who draws far terrain, at `position`, over a connection.
    fn join(&mut self, connection: Box<dyn Connection>, who: u8, position: [f64; 3]) -> Player {
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
        player.enable_coarse_detail(detail());
        player
    }

    /// A player of the game `G` at `position`, who draws each level as `G::level_detail` says.
    fn join_following_levels<G: GameDefinition<Content = ExampleContent>>(
        &mut self,
        connection: Box<dyn Connection>,
        who: u8,
        position: [f64; 3],
    ) -> Session<Box<dyn Connection>, G> {
        let (server, nodes) = (&mut self.server, &self.nodes);
        let mut player = Session::<_, G>::connect(
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
        player.enable_level_detail();
        player
    }

    fn connection(&self) -> Box<dyn Connection> {
        Box::new(self.connector.connect().unwrap())
    }

    /// Takes in everything the server has said and the player has to do about it, until the view is quiet.
    fn settle<G: GameDefinition>(
        &mut self,
        player: &mut Session<Box<dyn Connection>, G>,
        screen: &mut Screen,
    ) {
        player.send_pose();
        // Three rounds, and more until the server has made the nodes it was to (which it does on a thread of its own, a level of nodes in a tick): the round
        // after that takes them in.
        let mut rounds = 0;
        let mut rounds_after_done = 0;
        while rounds < 3 || rounds_after_done < 1 {
            let (server, nodes) = (&mut self.server, &self.nodes);
            let updates = player
                .settle(|| {
                    nodes.step(server);
                })
                .unwrap();
            screen.apply(updates);
            rounds += 1;
            if self.nodes.is_done() {
                rounds_after_done += 1;
            }
            assert!(rounds < 200, "the server is still making nodes");
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
        let edge = cube.edge_length() as i128 * 32;
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
fn a_place_that_is_kept_ready_is_held_hidden_and_a_player_who_stands_there_draws_what_is_held() {
    let mut rig = Rig::new();
    let connection = rig.connection();
    let mut watcher = rig.join(connection, 1, WATCHER);
    watcher.hold_hidden_pictures(64 << 20);
    let there = camera_at([8.5, 60.0, 8.5], 0.0, 0.0).position;
    watcher.keep_ready(GridId::MAIN, Some(there));
    assert_eq!(
        watcher.camera_place_in(GridId::MAIN),
        Some(watcher.camera.position)
    );
    let mut screen = Screen::default();
    rig.settle(&mut watcher, &mut screen);
    assert!(
        screen.hidden_nodes.len() > 20,
        "the place kept ready is held hidden: {} nodes",
        screen.hidden_nodes.len()
    );

    // A player who stands there draws nodes that the watcher draws or holds, the same pictures.
    let connection = rig.connection();
    let mut near = rig.join(connection, 2, [8.5, 60.0, 8.5]);
    let mut near_screen = Screen::default();
    rig.settle(&mut near, &mut near_screen);
    assert!(near_screen.nodes.len() > 20);
    for (cube, mesh) in &near_screen.nodes {
        assert_eq!(
            screen
                .nodes
                .get(cube)
                .or_else(|| screen.hidden_nodes.get(cube)),
            Some(mesh),
            "{cube:?} is drawn where the watcher keeps ready, and neither drawn nor held by the watcher"
        );
    }

    // Not kept ready any more: what was held beyond the budget goes (nothing here is over it), and nothing new is made for the place.
    watcher.keep_ready(GridId::MAIN, None);
    assert_eq!(
        watcher.camera_place_in(GridId::MAIN),
        Some(watcher.camera.position)
    );
}

/// Whether the player holds the edited brick at the origin, 350 voxels from where it is, after the crater was dug there, with a place beside the origin kept ready in
/// the grid given (if one is), and whether it holds it after the place is not kept ready any more.
fn holds_the_edited_brick_at_the_origin(kept_ready_in: Option<GridId>) -> (bool, bool) {
    let mut rig = Rig::new();
    let connection = rig.connection();
    let mut watcher = rig.join(connection, 1, WATCHER);
    watcher.hold_hidden_pictures(64 << 20);
    if let Some(grid) = kept_ready_in {
        let there = camera_at([8.5, 60.0, 8.5], 0.0, 0.0).position;
        watcher.keep_ready(grid, Some(there));
    }
    let mut screen = Screen::default();
    rig.settle(&mut watcher, &mut screen);
    rig.dig_a_crater();
    rig.settle(&mut watcher, &mut screen);
    let holds = |player: &Player| {
        player
            .client()
            .world()
            .world()
            .main()
            .overrides
            .brick(BrickCoordinate::ORIGIN)
            .is_some()
    };
    let while_kept = holds(&watcher);
    if let Some(grid) = kept_ready_in {
        watcher.keep_ready(grid, None);
    }
    rig.settle(&mut watcher, &mut screen);
    (while_kept, holds(&watcher))
}

#[test]
fn the_edited_bricks_of_a_place_that_is_kept_ready_are_held_while_it_is_and_forgotten_when_it_is_not_any_more()
 {
    // The player is 350 voxels from the crater, farther than its bricks are watched: it holds them only for the place that is kept ready.
    assert_eq!(holds_the_edited_brick_at_the_origin(None), (false, false));
    assert_eq!(
        holds_the_edited_brick_at_the_origin(Some(GridId::MAIN)),
        (true, false)
    );
    // A place kept ready in another level is not a place of this level's terrain: nothing is watched for it.
    assert_eq!(
        holds_the_edited_brick_at_the_origin(Some(GridId::root(LevelIndex::new(1)))),
        (false, false)
    );
}

/// Where the watcher is when it is far from the crater and a place is kept ready near it: 1,000 voxels from it, beyond the 700 that terrain is drawn to.
const FAR_FROM_THE_CRATER: [f64; 3] = [1_008.5, 60.0, 8.5];

#[test]
fn the_nodes_an_edit_changes_round_a_place_kept_ready_are_held_as_a_player_there_draws_them() {
    let mut rig = Rig::new();
    let connection = rig.connection();
    let mut watcher = rig.join(connection, 1, FAR_FROM_THE_CRATER);
    watcher.hold_hidden_pictures(64 << 20);
    // The place is where the player of the second connection stands, 350 voxels from the crater: the crater is in its cut, as a node, and not in the watcher's own.
    watcher.keep_ready(GridId::MAIN, Some(camera_at(WATCHER, 0.0, 0.0).position));
    let mut screen = Screen::default();
    let connection = rig.connection();
    let mut near = rig.join(connection, 2, WATCHER);
    let mut near_screen = Screen::default();
    rig.settle(&mut watcher, &mut screen);
    rig.settle(&mut near, &mut near_screen);
    let before: HashMap<BrickCube, PackedBrickMesh> = near_screen.nodes.clone();
    assert!(
        screen.hidden_nodes.len() > 20,
        "the place is held: {} nodes",
        screen.hidden_nodes.len()
    );

    rig.dig_a_crater();
    rig.settle(&mut watcher, &mut screen);
    rig.settle(&mut near, &mut near_screen);
    rig.settle(&mut watcher, &mut screen);

    let changed: Vec<BrickCube> = near_screen
        .nodes
        .iter()
        .filter(|(cube, mesh)| before.get(cube) != Some(mesh))
        .map(|(cube, _)| *cube)
        .collect();
    assert!(
        !changed.is_empty(),
        "the crater is not in the picture of the player who stands at the place"
    );
    for cube in &changed {
        assert_eq!(
            screen
                .nodes
                .get(cube)
                .or_else(|| screen.hidden_nodes.get(cube)),
            near_screen.nodes.get(cube),
            "{cube:?} is drawn with the crater in it where the place is, and not held so for it"
        );
    }
}

#[test]
fn the_places_the_server_is_asked_the_nodes_of_are_those_kept_ready_in_terrain_the_player_draws_in_nodes()
 {
    let mut rig = Rig::new();
    let there = camera_at(WATCHER, 0.0, 0.0).position;

    // A level drawn in nodes: the place kept ready in its terrain is asked for, with the brick it is in, and one in another level's terrain or in a grid there is not is not.
    let connection = rig.connection();
    let mut coarse = rig.join(connection, 1, FAR_FROM_THE_CRATER);
    assert_eq!(coarse.detail_places(), []);
    coarse.keep_ready(GridId::MAIN, Some(there));
    coarse.keep_ready(GridId::root(LevelIndex::new(1)), Some(there));
    coarse.keep_ready(GridId::new(GridId::FIRST_ALLOCATED + 40), Some(there));
    assert_eq!(
        coarse.detail_places(),
        [DetailPlace {
            grid: GridId::MAIN,
            brick: there.brick()
        }]
    );
    coarse.keep_ready(GridId::MAIN, None);
    assert_eq!(coarse.detail_places(), []);

    // A level drawn in bricks only has no nodes to ask for.
    let (server, nodes) = (&mut rig.server, &rig.nodes);
    let mut bricks = Session::<_, ExampleGame>::connect(
        Box::new(rig.connector.connect().unwrap()) as Box<dyn Connection>,
        &Identity::from_secret([2; 32]),
        Mode::Playing,
        3,
        camera_at(FAR_FROM_THE_CRATER, 0.0, 0.0),
        || {
            nodes.step(server);
        },
    )
    .unwrap();
    bricks.keep_ready(GridId::MAIN, Some(there));
    assert!(!bricks.draws_coarse_terrain());
    assert_eq!(bricks.detail_places(), []);
}

/// How many nodes a player holds hidden with a place kept ready, and how many after it is not kept ready any more, with the setting as given.
fn nodes_held_with_a_place_kept_ready_and_after(release: bool) -> (usize, usize) {
    let mut rig = Rig::new();
    let connection = rig.connection();
    let mut watcher = rig.join(connection, 1, WATCHER);
    watcher.hold_hidden_pictures(64 << 20);
    watcher.set_release_map_pictures(release);
    let there = camera_at([8.5, 60.0, 8.5], 0.0, 0.0).position;
    watcher.keep_ready(GridId::MAIN, Some(there));
    let mut screen = Screen::default();
    rig.settle(&mut watcher, &mut screen);
    let held = screen.hidden_nodes.len();
    watcher.keep_ready(GridId::MAIN, None);
    rig.settle(&mut watcher, &mut screen);
    (held, screen.hidden_nodes.len())
}

#[test]
fn a_place_that_is_not_kept_ready_any_more_has_its_pictures_let_go_of_unless_the_player_says_to_keep_them()
 {
    let (held, after) = nodes_held_with_a_place_kept_ready_and_after(true);
    assert!(held > 20, "the place was held: {held} nodes");
    assert_eq!(after, 0, "every picture was made for the place alone");

    let (held, after) = nodes_held_with_a_place_kept_ready_and_after(false);
    assert!(held > 20, "the place was held: {held} nodes");
    assert_eq!(after, held, "left to the budget, which is not full");
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

/// Moves a player to the level called `name` by the command a player types, and takes in what the server says of it.
fn go_to<G: GameDefinition>(
    rig: &mut Rig,
    player: &mut Session<Box<dyn Connection>, G>,
    screen: &mut Screen,
    name: &str,
) {
    player.send_command(&format!("/level go {name}"));
    rig.settle(player, screen);
    rig.settle(player, screen);
}

#[test]
fn an_edit_the_server_told_of_is_still_drawn_far_away_after_a_visit_to_a_level_that_is_drawn_in_bricks_only()
 {
    let mut rig = Rig::of::<ExampleOneCoarseLevelGame>();
    let connection = rig.connection();
    let mut watcher =
        rig.join_following_levels::<ExampleOneCoarseLevelGame>(connection, 1, WATCHER);
    let mut screen = Screen::default();
    rig.settle(&mut watcher, &mut screen);
    assert!(watcher.draws_coarse_terrain());
    let before = screen.nodes.clone();
    assert!(
        before.len() > 50,
        "the watcher sees terrain far away: {} nodes",
        before.len()
    );

    rig.dig_a_crater();
    rig.settle(&mut watcher, &mut screen);
    let with_the_crater = Screen {
        nodes: screen.nodes.clone(),
        bricks: HashMap::new(),
        hidden_nodes: HashMap::new(),
    };
    assert_ne!(
        with_the_crater.nodes, before,
        "the crater is in the picture before the visit"
    );

    // A visit to the level that is drawn in bricks only: the nodes are taken down, and the picture of the level is another view.
    go_to(&mut rig, &mut watcher, &mut screen, "other");
    assert!(!watcher.draws_coarse_terrain());
    assert!(screen.nodes.is_empty());

    // Back where they were: the server has told this client of the crater and does not say it again, so the picture has to keep it.
    go_to(&mut rig, &mut watcher, &mut screen, "main");
    assert!(watcher.draws_coarse_terrain());
    assert_same_nodes(&screen, &with_the_crater);
}

#[test]
fn a_game_that_only_says_how_far_terrain_is_drawn_draws_every_level_so_and_one_that_says_nothing_draws_none()
 {
    let level = |name: &str| -> NamespacedId { name.parse().unwrap() };
    for name in [
        "example:main",
        "example:elsewhere",
        "voxunami_cave:overworld",
    ] {
        let said = ExampleDetailGame::level_detail(&level(name));
        assert!(said.is_some(), "{name}");
        assert_eq!(said, ExampleDetailGame::coarse_detail());
        assert_eq!(ExampleGame::level_detail(&level(name)), None, "{name}");
    }
}
