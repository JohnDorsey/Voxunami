#![allow(dead_code)] // each test file uses a different part of this shared module

//! A server ticked by the test, and players connected to it.

pub mod docking;
pub mod parity;

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use game_shell::host::build_server_with;
use game_shell::session::{Mode, Session};
use game_shell::view::camera_at;
use game_shell::{GameContent, GameDefinition, Identity, KeyCode};
use namespaced_id::NamespacedId;
use player_presence::{PlayerClient, player_kind};
use plugin_host::{Phase, Plugin, Registrar, SystemSpec};
use protocol::ClientId;
use renderer::Camera;
use server::{GridView, Server, ServerWorld, ShipSettings};
use transport::Connection;
use transport::loopback::LoopbackConnector;
use voxel_space::{
    BrickCoordinate, EntityPose, GravitySettings, GridId, GridPose, LevelIndex, Orientation,
    VoxelCoordinate, WorldPosition,
};
use voxunami_cave::aboard::in_voxels;
use voxunami_cave::content::CaveContent;
use voxunami_cave::game::Cave;
use voxunami_cave::plugins::cave_plugins;
use world_storage::{MemoryShipFiles, ParsedWorld, ShipFileName, ShipFiles};

pub const VIEW_RADIUS: i64 = 3;
pub const SEED: u64 = 2024;

pub type Player = Session<Box<dyn Connection>, Cave>;

pub struct Rig {
    pub content: CaveContent,
    pub server: Server,
    pub connector: LoopbackConnector,
    /// The files of the server's ships (the server has a handle to the same ones).
    pub files: MemoryShipFiles,
    /// How many players have joined: each is an identity of its own.
    pub joined: u8,
}

impl Rig {
    pub fn new() -> Self {
        let content = CaveContent::new();
        let world = content.new_overworld_world(SEED);
        Self::with_world(content, world)
    }

    /// A server running these plugins (the game's own, perhaps in another order) for `world`.
    pub fn with_plugins(
        content: CaveContent,
        world: world_storage::WorldData,
        plugins: Vec<Box<dyn plugin_host::Plugin>>,
    ) -> Self {
        Self::with_plugins_and_files(content, world, plugins, MemoryShipFiles::new())
    }

    /// The same, with the files of the ships of `world` in these (a world that was saved, and its ships with it).
    pub fn with_plugins_and_files(
        content: CaveContent,
        world: world_storage::WorldData,
        plugins: Vec<Box<dyn plugin_host::Plugin>>,
        files: MemoryShipFiles,
    ) -> Self {
        let disk = Box::new(files.clone());
        Self::with_plugins_on_disk(content, world, plugins, files, disk)
    }

    /// The same, with the server reading and writing ship files through `disk` (which keeps them in `files`, for the test to look at).
    pub fn with_plugins_on_disk(
        content: CaveContent,
        world: world_storage::WorldData,
        plugins: Vec<Box<dyn plugin_host::Plugin>>,
        files: MemoryShipFiles,
        disk: Box<dyn ShipFiles>,
    ) -> Self {
        let (mut server, connector) =
            build_server_with::<Cave>(&content, world, VIEW_RADIUS, plugins, disk).unwrap();
        // The tests of what ships do have no player near them, and ships are kept moving wherever the players are; the tests of
        // what the distances do say so (with `server.set_ship_settings`).
        server.set_ship_settings(ShipSettings::everywhere());
        Self {
            content,
            server,
            connector,
            files,
            joined: 0,
        }
    }

    pub fn with_world(content: CaveContent, world: world_storage::WorldData) -> Self {
        let plugins = Cave::plugins(&content);
        Self::with_plugins(content, world, plugins)
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
        let start = camera_at(position, yaw, pitch);
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
        let start = camera_at(position, yaw, pitch);
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
                        .grid(GridId::root(LevelIndex::MAIN))
                        .unwrap()
                        .set(
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
                    .grid(GridId::root(LevelIndex::MAIN))
                    .unwrap()
                    .set(
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

/// What a player is told after typing a command, once the server has had two ticks to answer.
pub fn say(scene: &mut Scene, player: &mut Player, command: &str) -> String {
    player.send_command(command);
    tick_and_follow(scene, player, 2);
    player.take_command_replies().join(" | ")
}

/// The block in a voxel of the main level, as the server has it (this takes a tick).
pub fn block_at(scene: &mut Scene, at: [i64; 3]) -> u32 {
    let content = CaveContent::new();
    let found = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&found);
    scene.run(move |world| {
        let block = world
            .grid(GridId::root(LevelIndex::MAIN))
            .unwrap()
            .get(
                content.block_channel(),
                VoxelCoordinate::new(at[0], at[1], at[2]),
            )
            .unwrap();
        *slot.lock().unwrap() = Some(block);
    });
    found.lock().unwrap().expect("the tick ran the action")
}

/// Stone at a voxel of the main level (this takes a tick).
pub fn put_stone(scene: &mut Scene, at: [i64; 3]) {
    let content = CaveContent::new();
    scene.run(move |world| {
        world
            .grid(GridId::root(LevelIndex::MAIN))
            .unwrap()
            .set(
                content.block_channel(),
                VoxelCoordinate::new(at[0], at[1], at[2]),
                content.blocks.stone,
            )
            .unwrap();
    });
}

/// The files of ships that fail to be written while `failing` is set (and are otherwise those of `inner`).
#[derive(Clone, Debug)]
pub struct FlakyFiles {
    pub inner: MemoryShipFiles,
    pub failing: Arc<AtomicBool>,
}

impl ShipFiles for FlakyFiles {
    fn read(&self, name: &ShipFileName) -> io::Result<Vec<u8>> {
        self.inner.read(name)
    }

    fn write(&mut self, name: &ShipFileName, bytes: &[u8]) -> io::Result<()> {
        if self.failing.load(Ordering::SeqCst) {
            return Err(io::Error::other("the disk is full"));
        }
        self.inner.write(name, bytes)
    }

    fn remove(&mut self, name: &ShipFileName) -> io::Result<()> {
        self.inner.remove(name)
    }

    fn names(&self) -> io::Result<Vec<ShipFileName>> {
        self.inner.names()
    }
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

/// The planet most of the ship tests were written for: it pulls at 32 metres a second a second, six times as hard as Cave's (5, the owner's
/// choice of 2026-10-22). Those tests are about ships that fall fast, stop on thin ground, and are found where the numbers put them in a few ticks,
/// and not about the planet; one test says what the default planet does (`ships_fall_at_the_pull_of_cave`).
pub fn heavy_planet(scene: &mut Scene) {
    scene.run(|world| {
        world.set_gravity_settings(GravitySettings {
            base: [0.0, -32.0, 0.0],
            ..*world.gravity_settings()
        });
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
        Self::with_disk(|files| Box::new(files))
    }

    /// A scene, and a way to ask what the plugin that moves ships has done with the colliders of terrain.
    pub fn reporting() -> (Self, impl Fn() -> grid_bodies::BodiesReport) {
        let content = CaveContent::new();
        let world = content.new_overworld_world(SEED);
        Self::reporting_in(content, world)
    }

    /// The same, in the world a new game starts in (space with the planet, and the overworld second).
    pub fn reporting_in_a_new_world() -> (Self, impl Fn() -> grid_bodies::BodiesReport) {
        let content = CaveContent::new();
        let world = content.new_world(SEED);
        Self::reporting_in(content, world)
    }

    pub fn reporting_in(
        content: CaveContent,
        world: world_storage::WorldData,
    ) -> (Self, impl Fn() -> grid_bodies::BodiesReport) {
        let actions: Arc<Mutex<VecDeque<Action>>> = Arc::default();
        let (mut plugins, report) = voxunami_cave::plugins::cave_plugins_reporting(&content);
        plugins.push(Box::new(Script {
            actions: Arc::clone(&actions),
        }));
        let files = MemoryShipFiles::new();
        let disk = Box::new(files.clone());
        let rig = Rig::with_plugins_on_disk(content, world, plugins, files, disk);
        (Self { rig, actions }, report)
    }

    /// A scene in the world a new game starts in: space with the planet in it, and the overworld second.
    pub fn in_a_new_world() -> Self {
        let content = CaveContent::new();
        let world = content.new_world(SEED);
        Self::in_world(content, world, |files| Box::new(files))
    }

    /// A scene in this world of this content.
    pub fn in_this_world(content: CaveContent, world: world_storage::WorldData) -> Self {
        Self::in_world(content, world, |files| Box::new(files))
    }

    /// A scene in the overworld alone, whose server keeps ship files through the disk `disk` makes of the memory they are in.
    pub fn with_disk(disk: impl FnOnce(MemoryShipFiles) -> Box<dyn ShipFiles>) -> Self {
        let content = CaveContent::new();
        let world = content.new_overworld_world(SEED);
        Self::in_world(content, world, disk)
    }

    fn in_world(
        content: CaveContent,
        world: world_storage::WorldData,
        disk: impl FnOnce(MemoryShipFiles) -> Box<dyn ShipFiles>,
    ) -> Self {
        let actions: Arc<Mutex<VecDeque<Action>>> = Arc::default();
        let mut plugins = cave_plugins(&content);
        plugins.push(Box::new(Script {
            actions: Arc::clone(&actions),
        }));
        let files = MemoryShipFiles::new();
        let disk = disk(files.clone());
        Self {
            rig: Rig::with_plugins_on_disk(content, world, plugins, files, disk),
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

    /// What a restart of the program does, without the program: the world is saved as a server that goes on running saves it (a world
    /// file and a file for each ship that is in memory), and another server is made from what was written, with the same ship files. The
    /// players are not connected to the new one: they join again.
    pub fn restarted(self) -> Self {
        let Scene { rig, .. } = self;
        let Rig {
            mut content,
            mut server,
            files,
            ..
        } = rig;
        let save = server.save(&content.registries()).unwrap();
        let mut world_file = Vec::new();
        save.write_with(server.ship_files_mut(), |bytes| {
            world_file = bytes.to_vec();
            Ok(())
        })
        .unwrap();
        drop(server);
        let parsed = ParsedWorld::parse(&world_file).unwrap();
        let seed = parsed.world_seed();
        let (world, _) = parsed.load(content.load_context(seed)).unwrap();
        let actions: Arc<Mutex<VecDeque<Action>>> = Arc::default();
        let mut plugins = cave_plugins(&content);
        plugins.push(Box::new(Script {
            actions: Arc::clone(&actions),
        }));
        Self {
            rig: Rig::with_plugins_and_files(content, world, plugins, files),
            actions,
        }
    }
}

/// The height, in the planet's own coordinates, of the highest block that is not air in the column of voxels at `x` and `z`, as the planet's generator makes it
/// (caves and all).
pub fn top_of_the_column(x: i64, z: i64) -> i64 {
    let content = CaveContent::new();
    let world = content.new_overworld_world(SEED);
    let layout = Arc::clone(world.main().overrides.layout());
    let generator = content
        .blocks
        .terrain_generators(SEED, content.block_channel());
    let id = generator
        .runtime_id(&base_blocks::small_planet_generator_identifier())
        .unwrap();
    let generator = generator.entry(id).unwrap().content().unwrap();
    for brick_y in (1900..2010).rev() {
        let coordinate = BrickCoordinate::new(x.div_euclid(32), brick_y, z.div_euclid(32));
        let brick = generator.generate_brick(&layout, coordinate);
        for local_y in (0..32).rev() {
            let local = voxel_space::LocalVoxelIndex::new(
                x.rem_euclid(32) as u8,
                local_y,
                z.rem_euclid(32) as u8,
            );
            if brick.get(content.block_channel(), local) != content.blocks.air {
                return brick_y * 32 + i64::from(local_y);
            }
        }
    }
    panic!("no ground in the column");
}
