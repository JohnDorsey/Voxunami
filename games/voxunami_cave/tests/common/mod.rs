#![allow(dead_code)] // each test file uses a different part of this shared module

//! A server ticked by the test, and players connected to it.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use game_shell::host::build_server;
use game_shell::session::{Mode, Session};
use game_shell::view::camera_at;
use game_shell::{GameContent, Identity, KeyCode};
use namespaced_id::NamespacedId;
use plugin_host::{Phase, Plugin, Registrar, SystemSpec};
use renderer::Camera;
use server::{Server, ServerWorld};
use transport::Connection;
use transport::loopback::LoopbackConnector;
use voxel_space::{VoxelCoordinate, WorldPosition};
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
        let connection: Box<dyn Connection> = Box::new(self.connector.connect().unwrap());
        let start = camera_at(position, yaw, pitch, VIEW_RADIUS);
        self.joined += 1;
        let identity = Identity::from_secret([self.joined; 32]);
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
