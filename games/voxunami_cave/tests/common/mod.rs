#![allow(dead_code)] // each test file uses a different part of this shared module

//! A server ticked by the test, and players connected to it.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use game_shell::host::build_server;
use game_shell::session::{Mode, Session};
use game_shell::view::camera_at;
use game_shell::{GameContent, Identity, KeyCode};
use namespaced_id::NamespacedId;
use player_presence::{PlayerClient, player_kind};
use plugin_host::{Phase, Plugin, Registrar, SystemSpec};
use protocol::ClientId;
use renderer::Camera;
use server::{GridView, Server, ServerWorld};
use transport::Connection;
use transport::loopback::LoopbackConnector;
use voxel_space::{
    BrickCoordinate, EntityPose, GridId, GridPose, LevelIndex, Orientation, VoxelCoordinate,
    WorldPosition,
};
use voxunami_cave::aboard::in_voxels;
use voxunami_cave::content::CaveContent;
use voxunami_cave::game::Cave;
use voxunami_cave::plugins::cave_plugins;

pub const VIEW_RADIUS: i64 = 3;
pub const SEED: u64 = 2024;

pub type Player = Session<Box<dyn Connection>, Cave>;

pub struct Rig {
    pub content: CaveContent,
    pub server: Server,
    pub connector: LoopbackConnector,
    /// How many players have joined: each is an identity of its own.
    pub joined: u8,
}

impl Rig {
    pub fn new() -> Self {
        let content = CaveContent::new();
        let world = content.new_world(SEED);
        let (server, connector) = build_server::<Cave>(&content, world, VIEW_RADIUS).unwrap();
        Self {
            content,
            server,
            connector,
            joined: 0,
        }
    }

    /// A server running these plugins (the game's own, perhaps in another order) for `world`.
    pub fn with_plugins(
        content: CaveContent,
        world: world_storage::WorldData,
        plugins: Vec<Box<dyn plugin_host::Plugin>>,
    ) -> Self {
        let (server, connector) = game_shell::host::build_server_with_plugins::<Cave>(
            &content,
            world,
            VIEW_RADIUS,
            plugins,
        )
        .unwrap();
        Self {
            content,
            server,
            connector,
            joined: 0,
        }
    }

    pub fn with_world(content: CaveContent, world: world_storage::WorldData) -> Self {
        let (server, connector) = build_server::<Cave>(&content, world, VIEW_RADIUS).unwrap();
        Self {
            content,
            server,
            connector,
            joined: 0,
        }
    }

    /// A player at `position` (in voxels) looking along the given angles, joined to the server.
    pub fn join(&mut self, position: [f64; 3], yaw: f32, pitch: f32) -> Player {
        let connection = self.connector.connect().unwrap();
        self.join_with(Box::new(connection), position, yaw, pitch)
    }

    pub fn join_with(
        &mut self,
        connection: Box<dyn Connection>,
        position: [f64; 3],
        yaw: f32,
        pitch: f32,
    ) -> Player {
        self.joined += 1;
        self.join_as(connection, self.joined, position, yaw, pitch)
    }

    /// A player who is the install numbered `who` (the same number is the same player, as a key file is),
    /// at `position` if the server has no earlier place for them.
    pub fn join_as(
        &mut self,
        connection: Box<dyn Connection>,
        who: u8,
        position: [f64; 3],
        yaw: f32,
        pitch: f32,
    ) -> Player {
        let start = camera_at(position, yaw, pitch, VIEW_RADIUS);
        let identity = Identity::from_secret([who; 32]);
        let server = &mut self.server;
        let mut player = Session::<_, Cave>::connect(
            connection,
            &identity,
            Mode::Playing,
            VIEW_RADIUS,
            start,
            || {
                server.step();
            },
        )
        .unwrap();
        // Players of these tests fly, so that they stay where they are put (the server moves them;
        // the tests of walking join with `join_walking`).
        player.send_command("/fly");
        player
    }

    /// A player who is walking: they fall, and stand on what is under them.
    pub fn join_walking(&mut self, position: [f64; 3], yaw: f32, pitch: f32) -> Player {
        self.joined += 1;
        self.join_walking_as(self.joined, position, yaw, pitch)
    }

    /// A walking player who is the install numbered `who` (the same number is the same player, as a key file is), at `position` if the
    /// server has no earlier place for them.
    pub fn join_walking_as(&mut self, who: u8, position: [f64; 3], yaw: f32, pitch: f32) -> Player {
        let connection: Box<dyn Connection> = Box::new(self.connector.connect().unwrap());
        self.join_walking_over(connection, who, position, yaw, pitch)
    }

    /// The same over a connection the test made (to record it, say).
    pub fn join_walking_over(
        &mut self,
        connection: Box<dyn Connection>,
        who: u8,
        position: [f64; 3],
        yaw: f32,
        pitch: f32,
    ) -> Player {
        let start = camera_at(position, yaw, pitch, VIEW_RADIUS);
        let identity = Identity::from_secret([who; 32]);
        let server = &mut self.server;
        Session::<_, Cave>::connect(
            connection,
            &identity,
            Mode::Playing,
            VIEW_RADIUS,
            start,
            || {
                server.step();
            },
        )
        .unwrap()
    }

    pub fn step(&mut self) {
        self.server.step();
    }

    /// Ticks the server and lets every player take in the result, until their views are quiet.
    pub fn settle(&mut self, players: &mut [&mut Player]) {
        for player in players.iter_mut() {
            player.send_pose();
        }
        // Two rounds, so what one player's settling causes reaches the others.
        for _ in 0..2 {
            for player in players.iter_mut() {
                let server = &mut self.server;
                player
                    .settle(|| {
                        server.step();
                    })
                    .unwrap();
            }
        }
    }
}

/// The height of the highest solid voxel in the column holding `at`, as the player sees it.
pub fn ground_height(player: &Player, x: i64, z: i64) -> i64 {
    (-100..400)
        .rev()
        .find(|&y| player.mesher().is_opaque_at(VoxelCoordinate::new(x, y, z)) == Some(true))
        .expect("the column has ground")
}

/// A camera `height` voxels above the ground of a column, looking straight down.
pub fn above_ground(player: &Player, x: i64, z: i64, height: f32) -> Camera {
    let ground = ground_height(player, x, z) as f32 + 1.0;
    let mut camera = camera_at(
        [x as f64 + 0.5, f64::from(ground + height), z as f64 + 0.5],
        0.0,
        0.0,
        VIEW_RADIUS,
    );
    camera.look_by(0.0, -std::f32::consts::FRAC_PI_2); // clamped to just short of straight down
    camera
}

pub fn position_of(camera: &Camera) -> WorldPosition {
    camera.position
}

/// A position in voxels, as plain numbers.
pub fn voxels_of(camera: &Camera) -> [f64; 3] {
    let position = position_of(camera);
    [
        position.brick().x() as f64 * 32.0 + f64::from(position.offset()[0]),
        position.brick().y() as f64 * 32.0 + f64::from(position.offset()[1]),
        position.brick().z() as f64 * 32.0 + f64::from(position.offset()[2]),
    ]
}

/// What a test can do to a player that the server moves: put them where their camera is.
pub trait PlayerExt {
    /// Asks the server to put the player's eyes where their camera is (`/tp`).
    fn teleport_to_camera(&mut self);

    /// Says what the keys held ask of the player's body, as the window does: from now on, until
    /// this is said again.
    fn hold(&mut self, keys: &[KeyCode]);
}

impl PlayerExt for Player {
    fn teleport_to_camera(&mut self) {
        let [x, y, z] = voxels_of(&self.camera);
        self.send_command(&format!("/tp {x} {y} {z}"));
    }

    fn hold(&mut self, keys: &[KeyCode]) {
        let keys: std::collections::HashSet<KeyCode> = keys.iter().copied().collect();
        let (kind, payload) =
            <Cave as game_shell::GameDefinition>::movement_message(&keys, &self.camera)
                .expect("Cave moves players by messages");
        self.send_game_message(kind, payload);
    }
}

/// Something to do to the server's world, inside the server.
pub type Action = Box<dyn FnOnce(&mut ServerWorld) + Send>;

/// A plugin that runs the actions it is given, one in each tick, before the rules.
struct Script {
    actions: Arc<Mutex<VecDeque<Action>>>,
}

impl Plugin for Script {
    fn id(&self) -> NamespacedId {
        "test:script".parse().unwrap()
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        let actions = Arc::clone(&self.actions);
        registrar.system(
            SystemSpec::new("test:run_the_script", Phase::Decide),
            move |context| {
                let next = actions.lock().unwrap().pop_front();
                if let Some(action) = next {
                    action(context.world());
                }
            },
        );
    }
}

/// The height of the top of the floor `flat_floor` makes.
pub const FLOOR: f32 = 201.0;

/// A flat floor of stone three voxels thick, from 0 to 64 along x and z, above the hills of the terrain: ground that is the same everywhere,
/// since the terrain of Cave is bumpy and a ship on a slope rolls.
pub fn flat_floor(scene: &mut Scene) {
    let content = CaveContent::new();
    scene.run(move |world| {
        for x in 0..64 {
            for z in 0..64 {
                for y in 198..201 {
                    world
                        .set_voxel(
                            LevelIndex::MAIN,
                            content.block_channel(),
                            VoxelCoordinate::new(x, y, z),
                            content.blocks.stone,
                        )
                        .unwrap();
                }
            }
        }
    });
}

/// A deck of stone `length` along x, 8 along z and 2 thick, held, with its origin at `(x, height above the floor, 20)`: its top is
/// `height + 2` above the floor.
pub fn deck(scene: &mut Scene, x: f32, height: f32, length: i64) {
    make_ship(
        scene,
        pose_at(x, FLOOR + height, 20.0),
        true,
        [length, 2, 8],
        |_| true,
    );
}

/// The entity of the player who joined `number`th (counting from 1).
pub fn player_pose(scene: &Scene, number: u32) -> EntityPose {
    let client = PlayerClient(ClientId::new(number));
    scene
        .rig
        .server
        .world()
        .entities()
        .iter()
        .find(|entity| {
            entity.kind == player_kind()
                && entity.components.get::<PlayerClient>() == Ok(Some(client))
        })
        .expect("the player has an entity")
        .pose
}

pub fn ship_pose(scene: &Scene, number: u64) -> GridPose {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(number))
        .expect("the ship exists")
        .pose
}

/// Where the player's eyes are in the world, as plain numbers.
pub fn eyes_in_the_world(scene: &Scene, number: u32) -> [f64; 3] {
    let pose = player_pose(scene, number);
    match pose.grid.root_level() {
        Some(_) => in_voxels(pose.position),
        None => {
            let ship = scene
                .rig
                .server
                .world()
                .grids()
                .get(pose.grid)
                .expect("the ship they are in exists");
            in_voxels(ship.pose.local_to_world(pose.position).unwrap())
        }
    }
}

pub fn eyes_in_their_frame(scene: &Scene, number: u32) -> [f64; 3] {
    in_voxels(player_pose(scene, number).position)
}

/// A raft of stone, one thick and `size` by `size` voxels, built in the air with its lowest corner at the voxel `(x, y, z)`: blocks
/// that were built and are joined to nothing else, which `/ship create` makes a ship of.
pub fn build_raft(scene: &mut Scene, x: i64, y: i64, z: i64, size: i64) {
    let content = CaveContent::new();
    scene.run(move |world| {
        for across in 0..size {
            for along in 0..size {
                world
                    .set_voxel(
                        LevelIndex::MAIN,
                        content.block_channel(),
                        VoxelCoordinate::new(x + across, y, z + along),
                        content.blocks.stone,
                    )
                    .unwrap();
            }
        }
    });
}

/// Lets the server tick `count` times, the player taking in what it says after each.
pub fn tick_and_follow(scene: &mut Scene, player: &mut Player, count: usize) {
    for _ in 0..count {
        scene.rig.step();
        player.advance().unwrap();
    }
}

/// Runs `count` ticks, in each of which `act` is run in the server with the number of the tick (from 0).
pub fn each_tick(
    scene: &mut Scene,
    player: &mut Player,
    count: usize,
    act: impl Fn(&mut server::ServerWorld, usize) + Clone + Send + 'static,
) {
    for tick in 0..count {
        let act = act.clone();
        scene.run(move |world| act(world, tick));
        player.advance().unwrap();
    }
}

/// A flat floor, and a deck of 30 along x and 8 along z held 3 above it (its top is 5 above the floor) from x = 5, z = 20.
pub fn scene_with_a_deck() -> Scene {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    deck(&mut scene, 5.0, 3.0, 30);
    scene
}

/// A walker who falls onto the middle of the deck from 8 above it and is aboard it, standing still.
pub fn walker_on_the_deck(scene: &mut Scene) -> Player {
    let mut player = scene
        .rig
        .join_walking([20.0, FLOOR as f64 + 5.0 + 8.0, 24.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(scene, &mut player, 60);
    player
}

/// The id of the ship made `number`th (counting from 0) in a world with no ships before.
pub fn ship_id(number: u64) -> GridId {
    GridId::new(GridId::FIRST_ALLOCATED + number)
}

/// A pose in the main level, unturned, with the origin at this place in voxels.
pub fn pose_at(x: f32, y: f32, z: f32) -> GridPose {
    GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z]),
        Orientation::IDENTITY,
    )
}

/// Makes a ship of stone at `pose`, held still from the tick it is made in if `held`: the voxels of the box of `size` (from the ship's
/// origin) that `solid` says are stone, set in order of x, then y, then z.
pub fn make_ship(
    scene: &mut Scene,
    pose: GridPose,
    held: bool,
    size: [i64; 3],
    solid: impl Fn([i64; 3]) -> bool + Send + 'static,
) {
    let content = CaveContent::new();
    scene.run(move |world| {
        let grid = world.create_grid(LevelIndex::MAIN, pose).unwrap();
        world.set_grid_held(grid, held).unwrap();
        let mut view = world.grid(grid).unwrap();
        for x in 0..size[0] {
            for y in 0..size[1] {
                for z in 0..size[2] {
                    if solid([x, y, z]) {
                        view.set(
                            content.block_channel(),
                            VoxelCoordinate::new(x, y, z),
                            content.blocks.stone,
                        )
                        .unwrap();
                    }
                }
            }
        }
    });
}

/// A server with Cave's plugins and a script, in which a test can do things to the world that no player can: make a ship, move it, remove
/// it.
pub struct Scene {
    pub rig: Rig,
    actions: Arc<Mutex<VecDeque<Action>>>,
}

impl Scene {
    pub fn new() -> Self {
        let content = CaveContent::new();
        let world = content.new_world(SEED);
        let actions: Arc<Mutex<VecDeque<Action>>> = Arc::default();
        let mut plugins = cave_plugins(&content);
        plugins.push(Box::new(Script {
            actions: Arc::clone(&actions),
        }));
        Self {
            rig: Rig::with_plugins(content, world, plugins),
            actions,
        }
    }

    /// Has the server run this in its next tick, and runs that tick.
    pub fn run(&mut self, action: impl FnOnce(&mut ServerWorld) + Send + 'static) {
        self.actions.lock().unwrap().push_back(Box::new(action));
        self.rig.step();
    }

    pub fn ticks(&mut self, count: usize) {
        for _ in 0..count {
            self.rig.step();
        }
    }
}
