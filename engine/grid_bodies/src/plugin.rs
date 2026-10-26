use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use block_models::{BlockTable, PropertyHandle};
use namespaced_id::NamespacedId;
use physics::{BodyId, BodyKind, BodyMotion, BodySettings, BodyState, BodyWorld, MassMoments};
use plugin_host::{HandlerSpec, Phase, Plugin, Registrar, SystemSpec};
use server::{MergePlan, ServerWorld, SplitPart};
use voxel_grid::ChannelIndex;
use voxel_space::{
    AlignmentTolerance, BRICK_EDGE_LENGTH, BrickCoordinate, GridId, GridPose, LevelIndex,
    LocalVoxelIndex, VoxelCoordinate, WorldPosition,
};
use world_storage::MovingGrid;

use crate::collision::{BlockCollisions, GeneratedHull, ShipHull, WorldTerrain};
use crate::connectivity::{Connectivity, Outcome};
use crate::docking::{Docking, Port, PortView, approach, docked_pose, torque_of_force};

/// How far, in bricks, from the origin of a frame a ship may be: `f32` is exact enough within it. A ship that is not within this of the origin of any
/// frame of its level makes a frame of its own, centred on it, so ships far apart are all simulated; a ship that goes twice as far from the origin of
/// its frame is taken out of it and put in another (a frame is a place, not a ship's).
pub const FRAME_RADIUS_BRICKS: i64 = 500;

/// How far from the origin of a level's frame the origin of a planet may be, in bricks (4,096 is 131,072 voxels, where an `f32` is exact to a hundredth of one):
/// the planet's voxels are in its own coordinates, which are as far from the frame as it is. A planet farther than this has no body, and the ships over it fall
/// through it (a planet that large needs a frame of its own).
pub const PLANET_REACH_BRICKS: i64 = 4096;

/// How far from a ship's bounds terrain has colliders, in voxels: a few, and the way the ship goes in a few ticks. (A brick of natural terrain
/// costs ten milliseconds or more to generate and make colliders of, on the thread of the tick, so a ship that has colliders for all the bricks
/// within a brick of its bounds, as it first had, stopped the game for a quarter of a second each time it crossed from one brick to the next.)
const TERRAIN_LOOKAHEAD_VOXELS: f32 = 8.0;
const TERRAIN_LOOKAHEAD_SECONDS: f32 = 0.25;
/// The terrain a ship *needs* colliders of now: the bricks its bounds reach in the next two ticks (its speed for a tenth of a second) and two voxels more. These
/// are always made in the tick, whatever it costs, so that a ship never reaches a brick that has none.
const TERRAIN_NEEDED_VOXELS: f32 = 2.0;
const TERRAIN_NEEDED_SECONDS: f32 = 0.1;
/// The rest of the lookahead is *wanted*, not needed: it is made nearest first, at most this many bricks a tick. It is a count and not a time, so that what
/// is simulated is the same on every machine (a brick of natural terrain costs 5 to 25 ms to generate and make colliders of, so this bounds a tick).
const TERRAIN_WANTED_BRICKS_PER_TICK: usize = 1;
/// How much farther each ring of the wanted bricks reaches, in voxels: the nearer a brick is in rings, the sooner it is made.
const TERRAIN_RING_VOXELS: f32 = 8.0;

/// How many voxels of ships the checks for a ship that was cut in two may look at in a tick, all the ships together. A count and not a time, for
/// the same reason as the budget of terrain. A cut that does not separate anything is found in a few steps; one that does costs about the size of the
/// smaller part, so a ship that has lost a wing waits a few ticks (it is whole until the check is done) and a ship that is only dented does not.
const SPLIT_VISITS_PER_TICK: usize = 4096;
/// How much farther before the colliders of terrain are let go, so that a ship at the edge of the margin does not make and drop the same brick
/// every tick.
const TERRAIN_KEEP_EXTRA_VOXELS: f32 = 16.0;

/// How many ticks a pair of ships that could not be docked is left alone before it is tried again: a second.
const REFUSED_RETRY_TICKS: u64 = 20;

/// How much gravity may change, as a share of its size (or of one voxel a second squared, if that is more), before it wakes a ship that sleeps.
const GRAVITY_CHANGE_THAT_WAKES: f32 = 1e-3;

/// What the plugin needs to know of a game's blocks.
#[derive(Debug)]
pub struct GridBodiesConfig {
    pub table: BlockTable,
    pub block_channel: ChannelIndex,
    pub state_channel: ChannelIndex,
    /// Where the game keeps the orientation of its blocks, if it has any.
    pub orientation_channel: Option<ChannelIndex>,
    /// The block that is air, which makes a brick that is all of it empty without a look at each voxel.
    pub air: u32,
    /// The game's property for blocks bodies can pass through, if it has one.
    pub passable: Option<PropertyHandle<bool>>,
    /// A block's mass in kilograms a voxel (`engine:density`).
    pub density: PropertyHandle<u32>,
    /// How ships with docking ports are drawn together and joined, if the game has any (see [`Docking`]).
    pub docking: Option<Docking>,
    /// How a tick is cut into steps. The gravity of these settings is not used: gravity is the world's field, which the plugin applies as a
    /// force on each ship (see [`ServerWorld::gravity_at`]).
    pub settings: BodySettings,
}

/// Moves the grids of a world as rigid bodies: gravity, the forces asked with [`ServerWorld::apply_grid_force`], and collisions with the
/// terrain of their level, with the ground of the planets in it (ships with terrain: bodies that stay where they are, with colliders for the
/// bricks near the ships, made from the stored bricks or else the generator's), with each other and with the ships that are held. Edits to a ship
/// change its colliders and its mass; a ship put somewhere by anything else is simulated from there.
#[derive(Debug)]
pub struct GridBodiesPlugin {
    config: Arc<Mutex<Option<GridBodiesConfig>>>,
    report: Arc<Mutex<BodiesReport>>,
}

/// What the plugin has done with the colliders of terrain, for whoever runs the server and for tests. It counts bricks and not time: what is
/// simulated must not depend on how fast a machine is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BodiesReport {
    /// Bricks of terrain that got colliders in the last tick, in the most of any tick, and in all (edits that made one again do not count).
    pub terrain_bricks_made_last_tick: u64,
    pub terrain_bricks_made_most_in_a_tick: u64,
    pub terrain_bricks_made: u64,
    /// Ships made one by docking, and times two ships that were near enough and in line could not be (blocks of the two would be in one place).
    pub dockings_made: u64,
    pub dockings_refused: u64,
    /// How many frames there are now: places in levels that have ships in them, each simulated in a frame of its own.
    pub frames: u64,
}

impl GridBodiesPlugin {
    pub fn new(config: GridBodiesConfig) -> Self {
        Self {
            config: Arc::new(Mutex::new(Some(config))),
            report: Arc::default(),
        }
    }

    /// What the plugin has done so far.
    pub fn report(&self) -> BodiesReport {
        *self
            .report
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A handle to the report, which stays usable after the plugin moves into a server.
    pub fn report_handle(&self) -> impl Fn() -> BodiesReport + Send + Sync + 'static + use<> {
        let report = Arc::clone(&self.report);
        move || {
            *report
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        }
    }
}

fn locked(bodies: &Mutex<Bodies>) -> MutexGuard<'_, Bodies> {
    bodies
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Plugin for GridBodiesPlugin {
    fn id(&self) -> NamespacedId {
        "engine:grid_bodies"
            .parse()
            .expect("constant identifier is valid")
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        let config = self
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .expect("a plugin is registered once");
        let channels: Vec<ChannelIndex> = [config.block_channel, config.state_channel]
            .into_iter()
            .chain(config.orientation_channel)
            .collect();
        let bodies = Arc::new(Mutex::new(Bodies::new(config, Arc::clone(&self.report))));

        // Where blocks changed, the colliders and the mass are made again.
        let hearing = Arc::clone(&bodies);
        registrar.on_channels(
            HandlerSpec::new("engine:grid_bodies_remake_what_changed"),
            &channels,
            move |event, _| {
                let mut bodies = locked(&hearing);
                for voxel in &event.voxels {
                    bodies.changed(event.grid, *voxel);
                }
            },
        );

        let stepping = bodies;
        registrar.system(
            SystemSpec::new("engine:grid_bodies_move", Phase::Physics),
            move |context| locked(&stepping).step(context.world()),
        );
    }
}

/// One ship in the simulation.
struct Ship {
    /// The mass of each brick that has any, summed in order of brick for the body's mass.
    moments: BTreeMap<BrickCoordinate, MassMoments>,
    /// The docking ports in each brick that has any.
    ports: BTreeMap<BrickCoordinate, Vec<Port>>,
    kind: BodyKind,
    /// How much the ship weighs and where its centre of mass is in its coordinates, as last worked out.
    mass: f32,
    centre_of_mass: [f32; 3],
    /// The gravity the ship was last pulled by: a ship at rest sleeps while it is the same, and is woken when it changes.
    gravity: [f32; 3],
    /// What the simulation last said of where the ship is and how fast it moves: if the world says other, something else moved it.
    written: (GridPose, [f32; 3], [f32; 3]),
}

/// A planet (a ship with terrain) in the simulation: a body that is fixed, whose colliders are only of the bricks that ships are near.
struct Planet {
    /// The bricks, in the planet's own coordinates, that have colliders (made from what is stored there, or else from the generator).
    bricks: BTreeSet<BrickCoordinate>,
    /// Where the planet was last put in the bodies: if the world says other, something else moved it.
    written: GridPose,
    /// The docking ports in each brick of the planet that has any: only blocks that were built there are ports, so only its stored bricks are looked at.
    ports: BTreeMap<BrickCoordinate, Vec<Port>>,
    /// Whether the stored bricks have been looked at for ports (after that, the bricks that change).
    scanned: bool,
}

/// Which frame: the level it is in, and a number (a level has as many frames as it has places with ships in them).
type FrameKey = (LevelIndex, u32);

/// The bodies of one place in a level, in one frame: the frame's origin is the brick of the first ship that made it.
struct Frame {
    level: LevelIndex,
    bodies: BodyWorld,
    ships: BTreeMap<GridId, Ship>,
    /// The planets of the level that have a body. (They are no ships here: they weigh what their terrain says, are held, and do not keep a frame alive.)
    planets: BTreeMap<GridId, Planet>,
    /// The bricks of terrain that have colliders.
    terrain: BTreeSet<BrickCoordinate>,
}

/// Terrain that has colliders: the level's own, whose bricks are the frame's, or the ground of a planet, whose bricks are the planet's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ground {
    Level,
    Planet(GridId),
}

struct Bodies {
    config: GridBodiesConfig,
    collisions: BlockCollisions,
    frames: BTreeMap<FrameKey, Frame>,
    /// The number the next frame is given.
    next_frame: u32,
    /// The bricks of ships and of terrain changed since the last step.
    changed_ship_bricks: BTreeSet<(GridId, BrickCoordinate)>,
    /// The voxels of ships changed since the last step: where a ship may have been cut.
    changed_ship_voxels: BTreeMap<GridId, BTreeSet<VoxelCoordinate>>,
    /// The ships that are being looked at to see if they were cut in two.
    checks: BTreeMap<GridId, Check>,
    changed_terrain_bricks: BTreeSet<(LevelIndex, BrickCoordinate)>,
    /// What the world is to be told of the weight of ships, at the end of the step.
    weighed: BTreeMap<GridId, (f32, [f32; 3])>,
    /// Pairs of ships (the lower id first) that could not be docked, and the tick before which they are not tried again nor pulled together: the
    /// attempt works out the whole of the smaller ship, and a pair whose blocks meet and are pushed about by the bodies would be tried every tick.
    refused: BTreeMap<(GridId, GridId), u64>,
    /// Which tick this is, counting the steps of the bodies.
    tick: u64,
    report: Arc<Mutex<BodiesReport>>,
}

/// A ship that is being looked at to see whether it is still in one piece.
struct Check {
    /// The blocks beside the places where blocks were taken out, since the ship was last known to be whole.
    seeds: BTreeSet<VoxelCoordinate>,
    flood: Connectivity,
}

const FACES: [[i64; 3]; 6] = [
    [-1, 0, 0],
    [1, 0, 0],
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
];

fn pose_of(state: &BodyState) -> GridPose {
    GridPose::new(state.position, state.orientation)
}

/// Whether two bricks are more than `radius` bricks apart along any axis.
fn farther_than(radius: i64, first: BrickCoordinate, second: BrickCoordinate) -> bool {
    (first.x() - second.x()).abs() > radius
        || (first.y() - second.y()).abs() > radius
        || (first.z() - second.z()).abs() > radius
}

impl Bodies {
    fn new(config: GridBodiesConfig, report: Arc<Mutex<BodiesReport>>) -> Self {
        let collisions = BlockCollisions::new(config.table.clone(), config.passable);
        Self {
            config,
            collisions,
            frames: BTreeMap::new(),
            next_frame: 0,
            changed_ship_bricks: BTreeSet::new(),
            changed_ship_voxels: BTreeMap::new(),
            checks: BTreeMap::new(),
            changed_terrain_bricks: BTreeSet::new(),
            weighed: BTreeMap::new(),
            refused: BTreeMap::new(),
            tick: 0,
            report,
        }
    }

    /// The frame a ship has a body in, if it has one.
    fn frame_of(&self, id: GridId) -> Option<FrameKey> {
        self.frames
            .iter()
            .find(|(_, frame)| frame.ships.contains_key(&id))
            .map(|(key, _)| *key)
    }

    fn changed(&mut self, grid: GridId, voxel: VoxelCoordinate) {
        let brick = voxel.split().0;
        match grid.root_level() {
            Some(level) => {
                self.changed_terrain_bricks.insert((level, brick));
            }
            None => {
                self.changed_ship_bricks.insert((grid, brick));
                self.changed_ship_voxels
                    .entry(grid)
                    .or_default()
                    .insert(voxel);
            }
        }
    }

    /// The colliders of a brick of a ship, and the mass in it.
    fn remake_brick(
        &mut self,
        frame: FrameKey,
        id: GridId,
        brick: BrickCoordinate,
        ship: &MovingGrid,
    ) {
        let (moments, ports) = self.weigher().scan_brick(
            ship,
            brick,
            self.config.docking.as_ref().map(Docking::table),
        );
        let Some(bodies) = self.frames.get_mut(&frame) else {
            return;
        };
        let mut hull = ShipHull {
            voxels: &ship.voxels,
            block_channel: self.config.block_channel,
            state_channel: self.config.state_channel,
            orientation_channel: self.config.orientation_channel,
            collisions: &mut self.collisions,
        };
        // A brick too far from the ship's origin for a collider has none (and no mass either).
        let made = bodies.bodies.set_brick(BodyId(id.get()), brick, &mut hull);
        let (moments, ports) = if made.is_ok() {
            (moments, ports)
        } else {
            (MassMoments::NOTHING, Vec::new())
        };
        if let Some(record) = bodies.ships.get_mut(&id) {
            if moments.mass() > 0.0 {
                record.moments.insert(brick, moments);
            } else {
                record.moments.remove(&brick);
            }
            if ports.is_empty() {
                record.ports.remove(&brick);
            } else {
                record.ports.insert(brick, ports);
            }
        }
    }

    /// Changes the report.
    fn count(&self, change: impl FnOnce(&mut BodiesReport)) {
        change(
            &mut self
                .report
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
    }

    /// What it takes to weigh the voxels of a ship.
    fn weigher(&self) -> Weigher<'_> {
        Weigher {
            table: &self.config.table,
            density: self.config.density,
            block_channel: self.config.block_channel,
            state_channel: self.config.state_channel,
            orientation_channel: self.config.orientation_channel,
            air: self.config.air,
        }
    }

    /// Looks at the ships that blocks were taken out of, to see if they are still in one piece, with a little work each tick; a ship that is
    /// found to be in pieces is cut apart (see [`ServerWorld::split_grid`]). A ship is whole until its check says otherwise.
    fn split_ships(
        &mut self,
        world: &mut ServerWorld,
        changed: BTreeMap<GridId, BTreeSet<VoxelCoordinate>>,
    ) {
        let (block_channel, air) = (self.config.block_channel, self.config.air);
        for (id, voxels) in changed {
            let Some(ship) = world.data().grids().get(id) else {
                self.checks.remove(&id);
                continue;
            };
            if ship.terrain.is_some() {
                // (Its stored bricks are edits of a ground that is not stored, not a ship that can come apart or be gone when the last of them is air.)
                continue;
            }
            let is_part = |voxel: VoxelCoordinate| ship.voxels.get(block_channel, voxel) != air;
            // The blocks beside a place where a block was taken out.
            let mut seeds: BTreeSet<VoxelCoordinate> = BTreeSet::new();
            let mut removed_any = false;
            for voxel in voxels.into_iter().filter(|voxel| !is_part(*voxel)) {
                removed_any = true;
                for face in FACES {
                    let near = VoxelCoordinate::new(
                        voxel.x() + face[0],
                        voxel.y() + face[1],
                        voxel.z() + face[2],
                    );
                    if is_part(near) {
                        seeds.insert(near);
                    }
                }
            }
            // A ship that blocks were taken out of and that has none left is gone: there is nothing of it to carry.
            if seeds.is_empty() && removed_any && !has_blocks(ship, block_channel, air) {
                self.checks.remove(&id);
                let _ = world.remove_grid(id);
                continue;
            }
            if seeds.is_empty() && !self.checks.contains_key(&id) {
                continue;
            }
            // What was being looked at is looked at again with these as well (the ship has changed under it).
            if let Some(earlier) = self.checks.remove(&id) {
                seeds.extend(earlier.seeds.into_iter().filter(|seed| is_part(*seed)));
            }
            self.checks.insert(
                id,
                Check {
                    flood: Connectivity::new(seeds.iter().copied()),
                    seeds,
                },
            );
        }
        let mut budget = SPLIT_VISITS_PER_TICK;
        let ids: Vec<GridId> = self.checks.keys().copied().collect();
        for id in ids {
            let Some(ship) = world.data().grids().get(id) else {
                self.checks.remove(&id);
                continue;
            };
            let is_part = |voxel: VoxelCoordinate| ship.voxels.get(block_channel, voxel) != air;
            let check = self.checks.get_mut(&id).expect("the check is there");
            let before = check.flood.visited();
            let outcome = check.flood.step(budget, &is_part).clone();
            budget = budget.saturating_sub((check.flood.visited() - before) as usize);
            match outcome {
                Outcome::Working => {}
                Outcome::Whole => {
                    self.checks.remove(&id);
                }
                Outcome::Split { pieces } => {
                    self.checks.remove(&id);
                    let parts: Vec<SplitPart> = self
                        .weigher()
                        .parts_of(ship, pieces)
                        .into_iter()
                        .map(|(part, _)| part)
                        .collect();
                    if parts.len() >= 2 {
                        let _ = world.split_grid(id, parts);
                    }
                }
            }
            if budget == 0 {
                break;
            }
        }
    }

    /// One tick: what changed is made again, the ships are carried to where the world has them, the forces are applied, the bodies move and
    /// the ships are put where they went.
    fn step(&mut self, world: &mut ServerWorld) {
        self.tick += 1;
        let tick = self.tick;
        self.refused.retain(|_, until| tick < *until);
        let changed_ships = std::mem::take(&mut self.changed_ship_bricks);
        let changed_terrain = std::mem::take(&mut self.changed_terrain_bricks);
        let changed_voxels = std::mem::take(&mut self.changed_ship_voxels);
        self.dock(world);
        self.split_ships(world, changed_voxels);
        // (A ship with terrain, a planet, is no ship here: its blocks are mostly not stored, it weighs what its terrain says and it is held where it is.
        // It is a fixed body, which ships collide with, below.)
        let present: Vec<(GridId, LevelIndex)> = world
            .data()
            .grids()
            .iter()
            .filter(|(_, ship)| ship.terrain.is_none())
            .map(|(id, ship)| (id, ship.level))
            .collect();

        // Ships that are gone, or that have gone twice as far from the origin of their frame as a ship may be in one, leave their bodies (the second are
        // made bodies again below, in the frame of the place they are in); a frame with no ships is no more.
        for frame in self.frames.values_mut() {
            let origin = frame.bodies.origin();
            let gone: Vec<GridId> = frame
                .ships
                .keys()
                .filter(|id| {
                    !present.iter().any(|(held, _)| held == *id)
                        || world.data().grids().get(**id).is_none_or(|ship| {
                            farther_than(
                                2 * FRAME_RADIUS_BRICKS,
                                ship.pose.position.brick(),
                                origin,
                            )
                        })
                })
                .copied()
                .collect();
            for id in gone {
                frame.ships.remove(&id);
                let _ = frame.bodies.remove_body(BodyId(id.get()));
            }
        }
        self.frames.retain(|_, frame| !frame.ships.is_empty());

        for (id, level_index) in present {
            self.sync_ship(world, id, level_index, &changed_ships);
        }
        self.sync_planets(world);
        self.scan_planets_for_ports(world, &changed_ships);
        self.keep_terrain(world, &changed_terrain, &changed_ships);

        for (id, (mass, centre)) in std::mem::take(&mut self.weighed) {
            let _ = world.set_grid_mass(id, mass, centre);
        }
        self.apply_gravity(world);
        self.attract(world);
        for force in world.take_grid_forces() {
            let Some(frame) = self
                .frame_of(force.grid)
                .and_then(|key| self.frames.get_mut(&key))
            else {
                continue;
            };
            let _ = frame
                .bodies
                .apply_force(BodyId(force.grid.get()), force.force, force.at);
        }

        for torque in world.take_grid_torques() {
            let Some(frame) = self
                .frame_of(torque.grid)
                .and_then(|key| self.frames.get_mut(&key))
            else {
                continue;
            };
            let _ = frame
                .bodies
                .apply_torque(BodyId(torque.grid.get()), torque.torque);
        }

        for frame in self.frames.values_mut() {
            frame.bodies.step();
        }
        let frames = self.frames.len() as u64;
        self.count(|report| report.frames = frames);
        self.write_back(world);
    }

    /// Makes a ship's body and colliders, or brings them up to date with the ship.
    fn sync_ship(
        &mut self,
        world: &ServerWorld,
        id: GridId,
        level_index: LevelIndex,
        changed: &BTreeSet<(GridId, BrickCoordinate)>,
    ) {
        let Some(ship) = world.data().grids().get(id) else {
            return;
        };
        let kind = if ship.held || ship.frozen {
            BodyKind::Fixed
        } else {
            BodyKind::Dynamic
        };
        let state = BodyState {
            position: ship.pose.position,
            orientation: ship.pose.orientation,
            linear_velocity: ship.linear_velocity,
            angular_velocity: ship.angular_velocity,
        };
        let Some(key) = self.frame_of(id) else {
            // A ship is put in the first frame of its level that it is near the origin of, and makes a frame of its own where it is if there is none.
            let brick = ship.pose.position.brick();
            let key = self
                .frames
                .iter()
                .find(|(key, frame)| {
                    key.0 == level_index
                        && !farther_than(FRAME_RADIUS_BRICKS, brick, frame.bodies.origin())
                })
                .map(|(key, _)| *key);
            let key = match key {
                Some(key) => key,
                None => {
                    let key = (level_index, self.next_frame);
                    self.next_frame += 1;
                    self.frames.insert(
                        key,
                        Frame {
                            level: level_index,
                            bodies: BodyWorld::new(
                                brick,
                                BodySettings {
                                    gravity: [0.0; 3],
                                    ..self.config.settings
                                },
                            ),
                            ships: BTreeMap::new(),
                            planets: BTreeMap::new(),
                            terrain: BTreeSet::new(),
                        },
                    );
                    key
                }
            };
            let frame = self.frames.get_mut(&key).expect("the frame was just found");
            if frame
                .bodies
                .add_body(BodyId(id.get()), state, kind)
                .is_err()
            {
                return;
            }
            frame.ships.insert(
                id,
                Ship {
                    moments: BTreeMap::new(),
                    ports: BTreeMap::new(),
                    mass: 0.0,
                    centre_of_mass: [0.0; 3],
                    gravity: [0.0; 3],
                    kind,
                    written: (ship.pose, ship.linear_velocity, ship.angular_velocity),
                },
            );
            let bricks: Vec<BrickCoordinate> = ship.voxels.sorted_brick_coordinates();
            for brick in bricks {
                self.remake_brick(key, id, brick, ship);
            }
            self.set_mass(key, id);
            return;
        };
        // Bricks that changed.
        let mut remade = false;
        let bricks: Vec<BrickCoordinate> = changed
            .iter()
            .filter(|(grid, _)| *grid == id)
            .map(|(_, brick)| *brick)
            .collect();
        for brick in bricks {
            self.remake_brick(key, id, brick, ship);
            remade = true;
        }
        if remade {
            self.set_mass(key, id);
        }
        let Some(frame) = self.frames.get_mut(&key) else {
            return;
        };
        let Some(record) = frame.ships.get_mut(&id) else {
            return;
        };
        if record.kind != kind {
            record.kind = kind;
            let _ = frame.bodies.set_kind(BodyId(id.get()), kind);
        }
        // Moved by anything but the simulation: it goes on from there.
        if record.written != (ship.pose, ship.linear_velocity, ship.angular_velocity) {
            let _ = frame.bodies.set_state(BodyId(id.get()), state);
            record.written = (ship.pose, ship.linear_velocity, ship.angular_velocity);
        }
    }

    /// Gives a body the mass of all its bricks.
    fn set_mass(&mut self, key: FrameKey, id: GridId) {
        let Some(level) = self.frames.get_mut(&key) else {
            return;
        };
        let Some(record) = level.ships.get_mut(&id) else {
            return;
        };
        match MassMoments::sum(record.moments.values().copied()).properties() {
            Some(properties) => {
                let _ = level.bodies.set_mass(BodyId(id.get()), properties);
                record.mass = properties.mass;
                record.centre_of_mass = properties.centre_of_mass;
            }
            None => {
                record.mass = 0.0;
                record.centre_of_mass = [0.0; 3];
            }
        }
        self.weighed
            .insert(id, (record.mass, record.centre_of_mass));
    }

    /// Gives the planets of each level a body in each of its frames, puts those that were moved where they are, and lets those that are gone go.
    fn sync_planets(&mut self, world: &ServerWorld) {
        let planets: Vec<(GridId, LevelIndex)> = world
            .data()
            .grids()
            .iter()
            .filter(|(_, ship)| ship.terrain.is_some())
            .map(|(id, ship)| (id, ship.level))
            .collect();
        for frame in self.frames.values_mut() {
            let level = frame.level;
            let gone: Vec<GridId> = frame
                .planets
                .keys()
                .filter(|id| !planets.contains(&(**id, level)))
                .copied()
                .collect();
            for id in gone {
                frame.planets.remove(&id);
                let _ = frame.bodies.remove_body(BodyId(id.get()));
            }
            for &(id, _) in planets
                .iter()
                .filter(|(_, planet_level)| *planet_level == level)
            {
                let Some(planet) = world.data().grids().get(id) else {
                    continue;
                };
                let state = BodyState {
                    position: planet.pose.position,
                    orientation: planet.pose.orientation,
                    linear_velocity: planet.linear_velocity,
                    angular_velocity: planet.angular_velocity,
                };
                match frame.planets.get_mut(&id) {
                    Some(record) => {
                        if record.written != planet.pose {
                            let _ = frame.bodies.set_state(BodyId(id.get()), state);
                            record.written = planet.pose;
                        }
                    }
                    None => {
                        let origin = frame.bodies.origin();
                        if farther_than(PLANET_REACH_BRICKS, planet.pose.position.brick(), origin) {
                            continue;
                        }
                        if frame
                            .bodies
                            .add_body(BodyId(id.get()), state, BodyKind::Fixed)
                            .is_ok()
                        {
                            frame.planets.insert(
                                id,
                                Planet {
                                    bricks: BTreeSet::new(),
                                    written: planet.pose,
                                    ports: BTreeMap::new(),
                                    scanned: false,
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    /// Looks at the stored bricks of each planet for docking ports: all of them the first time, and then the ones that changed. (A planet's ground has no
    /// ports; they are blocks someone built.)
    fn scan_planets_for_ports(
        &mut self,
        world: &ServerWorld,
        changed: &BTreeSet<(GridId, BrickCoordinate)>,
    ) {
        let Some(docking) = &self.config.docking else {
            return;
        };
        let weigher = Weigher {
            table: &self.config.table,
            density: self.config.density,
            block_channel: self.config.block_channel,
            state_channel: self.config.state_channel,
            orientation_channel: self.config.orientation_channel,
            air: self.config.air,
        };
        for frame in self.frames.values_mut() {
            for (&id, planet) in &mut frame.planets {
                let Some(grid) = world.data().grids().get(id) else {
                    continue;
                };
                let bricks: Vec<BrickCoordinate> = if planet.scanned {
                    changed
                        .iter()
                        .filter(|(held, _)| *held == id)
                        .map(|(_, brick)| *brick)
                        .collect()
                } else {
                    grid.voxels.sorted_brick_coordinates()
                };
                planet.scanned = true;
                for brick in bricks {
                    let (_, ports) = weigher.scan_brick(grid, brick, Some(docking.table()));
                    if ports.is_empty() {
                        planet.ports.remove(&brick);
                    } else {
                        planet.ports.insert(brick, ports);
                    }
                }
            }
        }
    }

    /// Terrain has colliders where ships are, and not far from them: the level's, and the generated ground of its planets.
    fn keep_terrain(
        &mut self,
        world: &mut ServerWorld,
        changed_terrain: &BTreeSet<(LevelIndex, BrickCoordinate)>,
        changed_ships: &BTreeSet<(GridId, BrickCoordinate)>,
    ) {
        let keys: Vec<FrameKey> = self.frames.keys().copied().collect();
        let mut made_this_tick = 0u64;
        for key in keys {
            let level_index = self.frames[&key].level;
            let planets: Vec<GridId> = self.frames[&key].planets.keys().copied().collect();
            for ground in
                std::iter::once(Ground::Level).chain(planets.into_iter().map(Ground::Planet))
            {
                // The bricks of it that were edited, and so are made again if they have colliders.
                let changed: BTreeSet<BrickCoordinate> = match ground {
                    Ground::Level => changed_terrain
                        .iter()
                        .filter(|(level, _)| *level == level_index)
                        .map(|(_, brick)| *brick)
                        .collect(),
                    Ground::Planet(id) => changed_ships
                        .iter()
                        .filter(|(grid, _)| *grid == id)
                        .map(|(_, brick)| *brick)
                        .collect(),
                };
                let (to_drop, to_make, ahead) = self.bricks_to_change(key, ground, &changed);
                if let Ground::Planet(id) = ground {
                    // The wanted bricks that are not made this tick are made by a worker meanwhile, so that they are there when they are, with the bricks
                    // beside them and beside those that are made now (a collider is made looking at the voxels next to its brick).
                    let beside = ahead.iter().chain(&to_make).flat_map(|brick| {
                        FACES.map(|[x, y, z]| {
                            BrickCoordinate::new(brick.x() + x, brick.y() + y, brick.z() + z)
                        })
                    });
                    world.prefetch_ship_bricks(
                        id,
                        ahead.iter().copied().chain(beside).collect::<Vec<_>>(),
                    );
                }
                for brick in to_drop {
                    let level = self.frames.get_mut(&key).expect("the frame exists");
                    match ground {
                        Ground::Level => {
                            level.bodies.remove_terrain_brick(brick);
                            level.terrain.remove(&brick);
                        }
                        Ground::Planet(id) => {
                            let _ = level.bodies.remove_brick(BodyId(id.get()), brick);
                            if let Some(planet) = level.planets.get_mut(&id) {
                                planet.bricks.remove(&brick);
                            }
                        }
                    }
                }
                for brick in to_make {
                    let level = self.frames.get_mut(&key).expect("the frame exists");
                    match ground {
                        Ground::Level => {
                            made_this_tick += u64::from(!level.terrain.contains(&brick));
                            let mut source = WorldTerrain {
                                world,
                                level: level_index,
                                block_channel: self.config.block_channel,
                                state_channel: self.config.state_channel,
                                orientation_channel: self.config.orientation_channel,
                                air: self.config.air,
                                collisions: &mut self.collisions,
                            };
                            level.bodies.set_terrain_brick(brick, &mut source);
                            level.terrain.insert(brick);
                        }
                        Ground::Planet(id) => {
                            let Some(planet) = level.planets.get_mut(&id) else {
                                continue;
                            };
                            made_this_tick += u64::from(!planet.bricks.contains(&brick));
                            let mut source = GeneratedHull {
                                world,
                                grid: id,
                                block_channel: self.config.block_channel,
                                state_channel: self.config.state_channel,
                                orientation_channel: self.config.orientation_channel,
                                air: self.config.air,
                                collisions: &self.collisions,
                            };
                            let _ = level.bodies.set_brick(BodyId(id.get()), brick, &mut source);
                            planet.bricks.insert(brick);
                        }
                    }
                }
            }
        }
        let mut report = self
            .report
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        report.terrain_bricks_made_last_tick = made_this_tick;
        report.terrain_bricks_made_most_in_a_tick = report
            .terrain_bricks_made_most_in_a_tick
            .max(made_this_tick);
        report.terrain_bricks_made += made_this_tick;
    }

    /// Which bricks of a ground are to lose their colliders, which are to get them (or get them again, if `changed` has them and they had) this tick, and which are
    /// wanted and left for a later tick, nearest first.
    fn bricks_to_change(
        &self,
        key: FrameKey,
        ground: Ground,
        changed: &BTreeSet<BrickCoordinate>,
    ) -> (
        Vec<BrickCoordinate>,
        Vec<BrickCoordinate>,
        Vec<BrickCoordinate>,
    ) {
        let level = &self.frames[&key];
        // Which bricks of the ground have colliders, and how a body's bounds are seen in its bricks. (A planet's colliders are asked for wherever a ship is
        // near, and not only in the bricks its generator makes: what is built past them is the planet's too.)
        let loaded = match ground {
            Ground::Level => &level.terrain,
            Ground::Planet(id) => {
                let Some(planet) = level.planets.get(&id) else {
                    return (Vec::new(), Vec::new(), Vec::new());
                };
                &planet.bricks
            }
        };
        let bricks_near = |body: BodyId, margin: f32| match ground {
            Ground::Level => level.bodies.bricks_near(body, margin),
            Ground::Planet(id) => level.bodies.bricks_near_in(body, margin, BodyId(id.get())),
        };
        // The bricks that are needed now, those that are wanted (with how many rings from a ship), and those that are kept. A planet is fixed, so the ships
        // that would be held against it need nothing of it.
        let mut needed: BTreeSet<BrickCoordinate> = BTreeSet::new();
        let mut wanted: BTreeMap<BrickCoordinate, usize> = BTreeMap::new();
        let mut kept: BTreeSet<BrickCoordinate> = BTreeSet::new();
        for (id, record) in &level.ships {
            if ground != Ground::Level && record.kind == BodyKind::Fixed {
                continue;
            }
            let body = BodyId(id.get());
            let Some(state) = level.bodies.state(body) else {
                continue;
            };
            let [vx, vy, vz] = state.linear_velocity;
            let speed = (vx * vx + vy * vy + vz * vz).sqrt();
            let lookahead = TERRAIN_LOOKAHEAD_VOXELS + TERRAIN_LOOKAHEAD_SECONDS * speed;
            let needed_margin = TERRAIN_NEEDED_VOXELS + TERRAIN_NEEDED_SECONDS * speed;
            if let Some(bricks) = bricks_near(body, needed_margin) {
                needed.extend(bricks.bricks());
            }
            let mut ring = 0;
            let mut margin = needed_margin;
            while margin < lookahead {
                margin = (margin + TERRAIN_RING_VOXELS).min(lookahead);
                ring += 1;
                if let Some(bricks) = bricks_near(body, margin) {
                    for brick in bricks.bricks() {
                        let nearest = wanted.entry(brick).or_insert(ring);
                        *nearest = (*nearest).min(ring);
                    }
                }
            }
            if let Some(bricks) = bricks_near(body, lookahead + TERRAIN_KEEP_EXTRA_VOXELS) {
                kept.extend(bricks.bricks());
            }
        }
        kept.extend(needed.iter().copied());
        let to_drop: Vec<BrickCoordinate> = loaded
            .iter()
            .copied()
            .filter(|brick| !kept.contains(brick))
            .collect();
        // Everything needed; of the wanted, the nearest few that are not made yet (ties by coordinate: the map is in order).
        let mut missing_wanted: Vec<(usize, BrickCoordinate)> = wanted
            .iter()
            .filter(|(brick, _)| !needed.contains(brick) && !loaded.contains(brick))
            .map(|(brick, ring)| (*ring, *brick))
            .collect();
        missing_wanted.sort();
        let ahead = missing_wanted.iter().map(|(_, brick)| *brick).collect();
        let to_make = needed
            .iter()
            .copied()
            .filter(|brick| !loaded.contains(brick))
            .chain(
                missing_wanted
                    .into_iter()
                    .take(TERRAIN_WANTED_BRICKS_PER_TICK)
                    .map(|(_, brick)| brick),
            )
            .chain(
                changed
                    .iter()
                    .copied()
                    .filter(|brick| loaded.contains(brick)),
            )
            .collect();
        (to_drop, to_make, ahead)
    }

    /// Pulls every ship that moves with the gravity there is at its centre of mass: the level's, and the pull of the other ships that weigh enough.
    fn apply_gravity(&mut self, world: &ServerWorld) {
        for frame in self.frames.values_mut() {
            let (level_index, level) = (frame.level, frame);
            for (id, record) in &mut level.ships {
                if record.kind == BodyKind::Fixed || record.mass <= 0.0 {
                    continue;
                }
                let Some(ship) = world.data().grids().get(*id) else {
                    continue;
                };
                let centre = WorldPosition::new(BrickCoordinate::ORIGIN, record.centre_of_mass);
                let Some(there) = ship.pose.local_to_world(centre) else {
                    continue;
                };
                let gravity = world.gravity_at(level_index, there);
                let force = gravity.map(|component| component * record.mass);
                // A change in gravity wakes a ship that is asleep (a heavy ship came near); the same gravity does not, so a ship at rest sleeps.
                let change: f32 = (0..3)
                    .map(|axis| (gravity[axis] - record.gravity[axis]).abs())
                    .sum();
                let size: f32 = gravity.iter().map(|component| component.abs()).sum();
                record.gravity = gravity;
                let body = BodyId(id.get());
                let _ = if change > GRAVITY_CHANGE_THAT_WAKES * size.max(1.0) {
                    level.bodies.apply_force(body, force, None)
                } else {
                    level.bodies.apply_steady_force(body, force)
                };
            }
        }
    }

    /// Puts the ships where the bodies are.
    fn write_back(&mut self, world: &mut ServerWorld) {
        for level in self.frames.values_mut() {
            for (id, record) in &mut level.ships {
                let body = BodyId(id.get());
                if record.kind == BodyKind::Fixed {
                    continue;
                }
                if level.bodies.is_asleep(body) {
                    // A body asleep has no speed, whatever it was last written with: that is what someone who steps onto it should find.
                    let (pose, linear, angular) = record.written;
                    if (linear != [0.0; 3] || angular != [0.0; 3])
                        && world.set_grid_velocity(*id, [0.0; 3], [0.0; 3]).is_ok()
                    {
                        record.written = (pose, [0.0; 3], [0.0; 3]);
                    }
                    continue;
                }
                let Some(state) = level.bodies.state(body) else {
                    continue;
                };
                let now = (
                    pose_of(&state),
                    state.linear_velocity,
                    state.angular_velocity,
                );
                if now == record.written {
                    continue;
                }
                if world.set_grid_pose(*id, now.0).is_ok() {
                    let _ = world.set_grid_velocity(*id, now.1, now.2);
                    record.written = now;
                }
            }
        }
    }
}

/// What `port_pairs` needs of a ship or a planet that has docking ports.
struct Bearer<'a> {
    id: GridId,
    /// Whether it can move: a ship that is free does, a ship that is held and a planet do not.
    moves: bool,
    ports: &'a BTreeMap<BrickCoordinate, Vec<Port>>,
    ship: &'a MovingGrid,
}

/// A pair of ships with a pair of ports that pull on each other: the closest such pair of ports of the two.
struct PortPair {
    frame: FrameKey,
    first: GridId,
    second: GridId,
    first_port: Port,
    second_port: Port,
    approach: crate::docking::Approach,
}

impl Bodies {
    /// For every two ships of a frame (a planet is one, for this, fixed and with the ports someone built on it) that have ports that pull on each other, the
    /// closest pair of ports, in order of ship. Ships that are frozen, pairs that were undocked and have not yet come apart, pairs of which neither can move, and
    /// pairs that could not be docked where they are, have none.
    fn port_pairs(&self, world: &ServerWorld, docking: &Docking) -> Vec<PortPair> {
        let settings = docking.settings;
        let released = docking.released();
        let mut pairs = Vec::new();
        for (&frame_key, frame) in &self.frames {
            // Those that have ports, in order of identifier: whether each can move, its ports, and the grid.
            let mut bearers: Vec<Bearer<'_>> = frame
                .ships
                .iter()
                .filter_map(|(id, record)| {
                    let ship = world.data().grids().get(*id)?;
                    (!ship.frozen && !record.ports.is_empty()).then_some(Bearer {
                        id: *id,
                        moves: record.kind != BodyKind::Fixed,
                        ports: &record.ports,
                        ship,
                    })
                })
                .collect();
            bearers.extend(frame.planets.iter().filter_map(|(id, planet)| {
                let grid = world.data().grids().get(*id)?;
                (!planet.ports.is_empty()).then_some(Bearer {
                    id: *id,
                    moves: false,
                    ports: &planet.ports,
                    ship: grid,
                })
            }));
            bearers.sort_by_key(|bearer| bearer.id);
            for (index, first) in bearers.iter().enumerate() {
                for second in &bearers[index + 1..] {
                    let (first_id, second_id) = (&first.id, &second.id);
                    let (first_ship, second_ship) = (first.ship, second.ship);
                    if !(first.moves || second.moves)
                        || released.contains(&(*first_id, *second_id))
                        || self
                            .refused
                            .get(&(*first_id, *second_id))
                            .is_some_and(|until| self.tick < *until)
                    {
                        continue;
                    }
                    let from = first_ship.pose.position;
                    let mut best: Option<PortPair> = None;
                    for first_port in first.ports.values().flatten() {
                        let first_view = PortView::of(*first_port, &first_ship.pose, from);
                        for second_port in second.ports.values().flatten() {
                            let second_view = PortView::of(*second_port, &second_ship.pose, from);
                            let how = approach(&first_view, &second_view);
                            if settings.pulls(&first_view, &second_view, &how)
                                && best
                                    .as_ref()
                                    .is_none_or(|top| how.distance < top.approach.distance)
                            {
                                best = Some(PortPair {
                                    frame: frame_key,
                                    first: *first_id,
                                    second: *second_id,
                                    first_port: *first_port,
                                    second_port: *second_port,
                                    approach: how,
                                });
                            }
                        }
                    }
                    pairs.extend(best);
                }
            }
        }
        pairs
    }

    /// Ships whose ports have come together, and are in line, are made one ship.
    fn dock(&mut self, world: &mut ServerWorld) {
        let Some(docking) = self.config.docking.clone() else {
            return;
        };
        if docking.has_released() {
            docking.forget_those_that_have_come_apart(world, &self.levels_ports());
        }
        let mut joined: BTreeSet<GridId> = BTreeSet::new();
        for pair in self.port_pairs(world, &docking) {
            if !docking.settings.can_snap(&pair.approach)
                || joined.contains(&pair.first)
                || joined.contains(&pair.second)
            {
                continue;
            }
            if self.dock_pair(world, &docking, &pair) {
                joined.insert(pair.first);
                joined.insert(pair.second);
            }
        }
    }

    /// The ports of every ship that has any, for the docking that looks at what is near.
    fn levels_ports(&self) -> BTreeMap<GridId, Vec<Port>> {
        self.frames
            .values()
            .flat_map(|frame| {
                let ships = frame
                    .ships
                    .iter()
                    .map(|(id, ship)| (*id, ship.ports.values().flatten().copied().collect()));
                let planets = frame
                    .planets
                    .iter()
                    .map(|(id, planet)| (*id, planet.ports.values().flatten().copied().collect()));
                ships.chain(planets)
            })
            .collect()
    }

    /// The motion of a ship's body, with positions measured from a point.
    fn motion_of(
        &self,
        world: &ServerWorld,
        frame: FrameKey,
        id: GridId,
        from: WorldPosition,
    ) -> Option<BodyMotion> {
        let level = self.frames.get(&frame)?;
        let record = level.ships.get(&id)?;
        let state = level.bodies.state(BodyId(id.get()))?;
        let properties = MassMoments::sum(record.moments.values().copied()).properties()?;
        let pose = world.data().grids().get(id)?.pose;
        let rotation = pose.rotation_matrix();
        let origin = pose.origin_relative_to(from);
        let centre_of_mass = properties.centre_of_mass.map(f64::from);
        let centre = [0, 1, 2].map(|axis| {
            origin[axis]
                + (0..3)
                    .map(|column| rotation[axis][column] * centre_of_mass[column])
                    .sum::<f64>()
        });
        Some(BodyMotion::of_body(
            &properties,
            rotation,
            centre,
            state.linear_velocity.map(f64::from),
            state.angular_velocity.map(f64::from),
        ))
    }

    /// Docks two ships that are close enough and in line: the lighter (never a held one) is put exactly where it docks, and they are made one
    /// ship that goes as the two did together (the momentum of both). Whether it was done: it is not when blocks of the two would be in one place,
    /// and the pair is not tried again until one of them has moved.
    fn dock_pair(&mut self, world: &mut ServerWorld, docking: &Docking, pair: &PortPair) -> bool {
        let Some(level) = self.frames.get(&pair.frame) else {
            return false;
        };
        // A ship that docks with a planet is written into it.
        if let Some((planet, planet_port, ship, ship_port)) = [
            (pair.first, pair.first_port, pair.second, pair.second_port),
            (pair.second, pair.second_port, pair.first, pair.first_port),
        ]
        .into_iter()
        .find(|(planet, ..)| level.planets.contains_key(planet))
        {
            return self.dock_into_planet(
                world,
                docking,
                pair,
                (planet, planet_port),
                (ship, ship_port),
            );
        }
        let (Some(first), Some(second)) =
            (level.ships.get(&pair.first), level.ships.get(&pair.second))
        else {
            return false;
        };
        // The ports must still be there (an edit this tick may have taken one away).
        let (Some(first_ship), Some(second_ship)) = (
            world.data().grids().get(pair.first),
            world.data().grids().get(pair.second),
        ) else {
            return false;
        };
        if !docking.has_port(first_ship, pair.first_port)
            || !docking.has_port(second_ship, pair.second_port)
        {
            return false;
        }
        let first_moves = match (first.kind, second.kind) {
            (BodyKind::Fixed, _) => false,
            (_, BodyKind::Fixed) => true,
            _ => first.mass < second.mass,
        };
        let (still, still_port, moving, moving_port) = if first_moves {
            (pair.second, pair.second_port, pair.first, pair.first_port)
        } else {
            (pair.first, pair.first_port, pair.second, pair.second_port)
        };
        let still_pose = world.data().grids().get(still).expect("checked above").pose;
        let moving_pose = world
            .data()
            .grids()
            .get(moving)
            .expect("checked above")
            .pose;
        let from = still_pose.position;
        let held = first.kind == BodyKind::Fixed || second.kind == BodyKind::Fixed;
        // The motion of both, before one is moved.
        let motions = [still, moving].map(|id| self.motion_of(world, pair.frame, id, from));
        let (linear, angular) = match (held, motions) {
            (false, [Some(one), Some(other)]) => match BodyMotion::join(&[one, other]) {
                Some(whole) => (
                    whole.linear_velocity.map(|speed| speed as f32),
                    whole.angular_velocity.map(|speed| speed as f32),
                ),
                None => ([0.0; 3], [0.0; 3]),
            },
            _ => ([0.0; 3], [0.0; 3]),
        };
        // Where the lighter ship is when it has docked exactly.
        let still_view = PortView::of(still_port, &still_pose, from);
        let moving_view = PortView::of(moving_port, &moving_pose, from);
        let turn = approach(&still_view, &moving_view).turn;
        let Some((docked, _)) = docked_pose(&still_pose, still_port, moving_port, turn) else {
            return false;
        };
        if world.set_grid_pose(moving, docked).is_err() {
            return false;
        }
        // (The orientations of the blocks are turned by the merge itself; nothing else here needs turning.)
        let turn_values = |_: &mut Vec<(ChannelIndex, u32)>, _: voxel_space::LatticeRotation| {};
        let plan = MergePlan {
            block_channel: self.config.block_channel,
            orientation_channel: self.config.orientation_channel,
            tolerance: AlignmentTolerance::default(),
            turn: &turn_values,
            linear_velocity: linear,
            angular_velocity: angular,
        };
        match world.merge_grids(still, moving, &plan) {
            Ok(_) => {
                self.refused.remove(&(pair.first, pair.second));
                self.count(|report| report.dockings_made += 1);
                true
            }
            Err(_) => {
                let _ = world.set_grid_pose(moving, moving_pose);
                self.refused
                    .insert((pair.first, pair.second), self.tick + REFUSED_RETRY_TICKS);
                self.count(|report| report.dockings_refused += 1);
                false
            }
        }
    }

    /// Docks a ship with a port of a planet: the ship is put exactly where it docks and its blocks are written into the planet (see
    /// [`ServerWorld::dock_into`]), which keeps its identifier. Whether it was done: it is not when blocks of the ship would be where the planet has blocks,
    /// its ground's too, and the pair is not tried again until a while has gone.
    fn dock_into_planet(
        &mut self,
        world: &mut ServerWorld,
        docking: &Docking,
        pair: &PortPair,
        (planet, planet_port): (GridId, Port),
        (ship, ship_port): (GridId, Port),
    ) -> bool {
        let (Some(planet_grid), Some(ship_grid)) = (
            world.data().grids().get(planet),
            world.data().grids().get(ship),
        ) else {
            return false;
        };
        // The ports must still be there (an edit this tick may have taken one away).
        if !docking.has_port(planet_grid, planet_port) || !docking.has_port(ship_grid, ship_port) {
            return false;
        }
        let (still_pose, moving_pose) = (planet_grid.pose, ship_grid.pose);
        let from = still_pose.position;
        let still_view = PortView::of(planet_port, &still_pose, from);
        let moving_view = PortView::of(ship_port, &moving_pose, from);
        let turn = approach(&still_view, &moving_view).turn;
        let Some((docked, _)) = docked_pose(&still_pose, planet_port, ship_port, turn) else {
            return false;
        };
        if world.set_grid_pose(ship, docked).is_err() {
            return false;
        }
        let turn_values = |_: &mut Vec<(ChannelIndex, u32)>, _: voxel_space::LatticeRotation| {};
        let plan = MergePlan {
            block_channel: self.config.block_channel,
            orientation_channel: self.config.orientation_channel,
            tolerance: AlignmentTolerance::default(),
            turn: &turn_values,
            linear_velocity: [0.0; 3],
            angular_velocity: [0.0; 3],
        };
        match world.dock_into(planet, ship, &plan) {
            Ok(()) => {
                self.refused.remove(&(pair.first, pair.second));
                self.count(|report| report.dockings_made += 1);
                true
            }
            Err(_) => {
                let _ = world.set_grid_pose(ship, moving_pose);
                self.refused
                    .insert((pair.first, pair.second), self.tick + REFUSED_RETRY_TICKS);
                self.count(|report| report.dockings_refused += 1);
                false
            }
        }
    }

    /// Ships with ports that pull on each other are pulled together and turned into line: each of the two ports gets the same force toward the
    /// other, and each ship the same torque, whatever it weighs.
    fn attract(&mut self, world: &ServerWorld) {
        let Some(docking) = self.config.docking.clone() else {
            return;
        };
        for pair in self.port_pairs(world, &docking) {
            let force = docking.settings.attraction(&pair.approach);
            let twist = docking.settings.twist(&pair.approach);
            let Some(level) = self.frames.get_mut(&pair.frame) else {
                continue;
            };
            for (id, port, sign) in [
                (pair.first, pair.first_port, 1.0),
                (pair.second, pair.second_port, -1.0),
            ] {
                let (Some(record), Some(ship)) =
                    (level.ships.get(&id), world.data().grids().get(id))
                else {
                    continue;
                };
                if record.kind == BodyKind::Fixed {
                    continue;
                }
                // The force is at the port (the owner, 2026-10-22: a ship whose centre is far from its port is swung by it). It is put on the centre of
                // mass, and the turn it gives by being at the port is added as a torque worked out here from the centre of mass this plugin keeps:
                // Rapier works out a lever from a centre of mass it updates when it steps, which is not the one just set for a ship made this tick.
                let pull = force.map(|component| component * sign);
                let (at, centre) = (port.local_centre(), record.centre_of_mass);
                let arm = [0, 1, 2].map(|axis| at[axis] - f64::from(centre[axis]));
                let lever = torque_of_force(ship.pose.rotation_matrix(), arm, pull);
                // The second ship turns toward line and the first the other way.
                let turn = [0, 1, 2].map(|axis| lever[axis] - twist[axis] * sign);
                let body = BodyId(id.get());
                let _ =
                    level
                        .bodies
                        .apply_force(body, pull.map(|component| component as f32), None);
                let _ = level
                    .bodies
                    .apply_torque(body, turn.map(|component| component as f32));
            }
        }
    }
}

/// What it takes to know the mass of the voxels of a ship: the table, the density property and the channels.
pub(crate) struct Weigher<'a> {
    pub table: &'a BlockTable,
    pub density: PropertyHandle<u32>,
    pub block_channel: ChannelIndex,
    pub state_channel: ChannelIndex,
    pub orientation_channel: Option<ChannelIndex>,
    pub air: u32,
}

impl Weigher<'_> {
    /// The mass of one voxel of a ship, with the block and the state in it (`None` for air and the like).
    pub fn voxel_moments(
        &self,
        voxel: [i64; 3],
        block: u32,
        state: u32,
        orientation: u32,
    ) -> Option<MassMoments> {
        let model = self.table.model_of(block, state, orientation);
        let shape = self.table.shape(model);
        if shape.is_empty() {
            return None;
        }
        // The share of the cell the block fills, in eighths of each edge.
        let filled: u32 = if shape.is_full() {
            512
        } else {
            shape
                .boxes()
                .iter()
                .map(|part| {
                    (0..3)
                        .map(|axis| u32::from(part.max[axis] - part.min[axis]))
                        .product::<u32>()
                })
                .sum()
        };
        let density = self.table.get(self.density, model);
        let mass = f64::from(density) * f64::from(filled) / 512.0;
        Some(MassMoments::of_voxel(voxel, mass))
    }

    /// The mass in the voxels of a brick of a ship, and (if a table to read them by is given) the docking ports in it.
    pub fn scan_brick(
        &self,
        ship: &MovingGrid,
        brick: BrickCoordinate,
        ports_by: Option<&BlockTable>,
    ) -> (MassMoments, Vec<Port>) {
        let Some(contents) = ship.voxels.brick(brick) else {
            return (MassMoments::NOTHING, Vec::new());
        };
        let first = brick.origin();
        let mut parts = Vec::new();
        let mut ports = Vec::new();
        for z in 0..BRICK_EDGE_LENGTH {
            for y in 0..BRICK_EDGE_LENGTH {
                for x in 0..BRICK_EDGE_LENGTH {
                    let local = LocalVoxelIndex::new(x as u8, y as u8, z as u8);
                    let block = contents.get(self.block_channel, local);
                    let state = contents.get(self.state_channel, local);
                    let orientation = self
                        .orientation_channel
                        .map_or(0, |channel| contents.get(channel, local));
                    let voxel = [
                        first.x() + x as i64,
                        first.y() + y as i64,
                        first.z() + z as i64,
                    ];
                    parts.extend(self.voxel_moments(voxel, block, state, orientation));
                    if let Some(table) = ports_by
                        && block != self.air
                        && let Some(face) = table.docking_face(block, state, orientation)
                    {
                        ports.push(Port {
                            voxel: VoxelCoordinate::new(voxel[0], voxel[1], voxel[2]),
                            face,
                        });
                    }
                }
            }
        }
        (MassMoments::sum(parts), ports)
    }

    /// The parts a ship is cut into, given the pieces that were found cut off: those, and all the rest of its blocks as one more if there are
    /// any. Each says where its centre of mass is, and (beside it) what it weighs.
    pub fn parts_of(
        &self,
        ship: &MovingGrid,
        pieces: Vec<BTreeSet<VoxelCoordinate>>,
    ) -> Vec<(SplitPart, f64)> {
        let (block_channel, air) = (self.block_channel, self.air);
        let mut rest: BTreeSet<VoxelCoordinate> = BTreeSet::new();
        for brick in ship.voxels.sorted_brick_coordinates() {
            let contents = ship.voxels.brick(brick).expect("a listed brick exists");
            for local in LocalVoxelIndex::all() {
                if contents.get(block_channel, local) != air {
                    rest.insert(brick.voxel_at(local));
                }
            }
        }
        for piece in &pieces {
            for voxel in piece {
                rest.remove(voxel);
            }
        }
        let mut voxel_sets = pieces;
        if !rest.is_empty() {
            voxel_sets.push(rest);
        }
        voxel_sets
            .into_iter()
            .map(|voxels| {
                let moments = MassMoments::sum(voxels.iter().filter_map(|voxel| {
                    let block = ship.voxels.get(block_channel, *voxel);
                    let state = ship.voxels.get(self.state_channel, *voxel);
                    let orientation = self
                        .orientation_channel
                        .map_or(0, |channel| ship.voxels.get(channel, *voxel));
                    self.voxel_moments([voxel.x(), voxel.y(), voxel.z()], block, state, orientation)
                }));
                // A part with no weight (nothing in it that bodies feel) is at the middle of its blocks.
                let centre_of_mass = moments.properties().map_or_else(
                    || {
                        let count = voxels.len() as f64;
                        [
                            voxels.iter().map(|v| v.x() as f64 + 0.5).sum::<f64>() / count,
                            voxels.iter().map(|v| v.y() as f64 + 0.5).sum::<f64>() / count,
                            voxels.iter().map(|v| v.z() as f64 + 0.5).sum::<f64>() / count,
                        ]
                        .map(|value| value as f32)
                    },
                    |properties| properties.centre_of_mass,
                );
                (
                    SplitPart {
                        voxels,
                        centre_of_mass,
                    },
                    moments.mass(),
                )
            })
            .collect()
    }
}

/// Whether the ship has a block at all.
fn has_blocks(ship: &MovingGrid, block_channel: ChannelIndex, air: u32) -> bool {
    ship.voxels
        .sorted_brick_coordinates()
        .into_iter()
        .any(|brick| {
            let contents = ship.voxels.brick(brick).expect("a listed brick exists");
            LocalVoxelIndex::all().any(|local| contents.get(block_channel, local) != air)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_bricks_are_farther_apart_than_a_radius_when_any_axis_is_past_it() {
        let at = |x, y, z| BrickCoordinate::new(x, y, z);
        let origin = at(10, -20, 30);
        assert!(!farther_than(5, at(10, -20, 30), origin));
        // On the radius is not past it; one more is, on any axis and either side.
        assert!(!farther_than(5, at(15, -15, 35), origin));
        assert!(!farther_than(5, at(5, -25, 25), origin));
        assert!(farther_than(5, at(16, -20, 30), origin));
        assert!(farther_than(5, at(4, -20, 30), origin));
        assert!(farther_than(5, at(10, -14, 30), origin));
        assert!(farther_than(5, at(10, -26, 30), origin));
        assert!(farther_than(5, at(10, -20, 36), origin));
        assert!(farther_than(5, at(10, -20, 24), origin));
    }
}
