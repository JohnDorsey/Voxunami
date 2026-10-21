use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use block_models::{BlockTable, PropertyHandle};
use namespaced_id::NamespacedId;
use physics::{BodyId, BodyKind, BodySettings, BodyState, BodyWorld, MassMoments};
use plugin_host::{HandlerSpec, Phase, Plugin, Registrar, SystemSpec};
use server::ServerWorld;
use voxel_grid::ChannelIndex;
use voxel_space::{
    BRICK_EDGE_LENGTH, BrickCoordinate, GridId, GridPose, LevelIndex, LocalVoxelIndex,
    WorldPosition,
};
use world_storage::MovingGrid;

use crate::collision::{BlockCollisions, ShipHull, WorldTerrain};

/// How far, in bricks, from the origin of a level's frame a ship may be to be simulated: `f32` is exact enough within it. A ship farther
/// from the first ship of its level than this stays where it is (the simulation distance and a frame for each place will replace this).
pub const FRAME_RADIUS_BRICKS: i64 = 500;

/// How many bricks around a ship's bounds terrain has colliders for (one is enough for a ship that moves a fraction of a brick a tick), and
/// how many before they are let go (so that a ship at the edge of the margin does not make and drop the same brick every tick).
const TERRAIN_MARGIN_BRICKS: i64 = 1;
const TERRAIN_KEEP_BRICKS: i64 = 2;

/// How much gravity may change, as a share of its size (or of one voxel a second squared, if that is more), before it wakes a ship that sleeps.
const GRAVITY_CHANGE_THAT_WAKES: f32 = 1e-3;

/// What the plugin needs to know of a game's blocks.
#[derive(Debug)]
pub struct GridBodiesConfig {
    pub table: BlockTable,
    pub block_channel: ChannelIndex,
    pub state_channel: ChannelIndex,
    /// The block that is air, which makes a brick that is all of it empty without a look at each voxel.
    pub air: u32,
    /// The game's property for blocks bodies can pass through, if it has one.
    pub passable: Option<PropertyHandle<bool>>,
    /// A block's mass in kilograms a voxel (`engine:density`).
    pub density: PropertyHandle<u32>,
    /// How a tick is cut into steps. The gravity of these settings is not used: gravity is the world's field, which the plugin applies as a
    /// force on each ship (see [`ServerWorld::gravity_at`]).
    pub settings: BodySettings,
}

/// Moves the grids of a world as rigid bodies: gravity, the forces asked with [`ServerWorld::apply_grid_force`], and collisions with the
/// terrain of their level, with each other and with the ships that are held. Edits to a ship change its colliders and its mass; a ship put
/// somewhere by anything else is simulated from there.
#[derive(Debug)]
pub struct GridBodiesPlugin {
    config: Arc<Mutex<Option<GridBodiesConfig>>>,
}

impl GridBodiesPlugin {
    pub fn new(config: GridBodiesConfig) -> Self {
        Self {
            config: Arc::new(Mutex::new(Some(config))),
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
        let channels = [config.block_channel, config.state_channel];
        let bodies = Arc::new(Mutex::new(Bodies::new(config)));

        // Where blocks changed, the colliders and the mass are made again.
        let hearing = Arc::clone(&bodies);
        registrar.on_channels(
            HandlerSpec::new("engine:grid_bodies_remake_what_changed"),
            &channels,
            move |event, _| {
                let mut bodies = locked(&hearing);
                for voxel in &event.voxels {
                    bodies.changed(event.grid, voxel.split().0);
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
    kind: BodyKind,
    /// How much the ship weighs and where its centre of mass is in its coordinates, as last worked out.
    mass: f32,
    centre_of_mass: [f32; 3],
    /// The gravity the ship was last pulled by: a ship at rest sleeps while it is the same, and is woken when it changes.
    gravity: [f32; 3],
    /// What the simulation last said of where the ship is and how fast it moves: if the world says other, something else moved it.
    written: (GridPose, [f32; 3], [f32; 3]),
}

/// The bodies of one level of the world, in one frame.
struct LevelBodies {
    bodies: BodyWorld,
    ships: BTreeMap<GridId, Ship>,
    /// The bricks of terrain that have colliders.
    terrain: BTreeSet<BrickCoordinate>,
}

struct Bodies {
    config: GridBodiesConfig,
    collisions: BlockCollisions,
    levels: BTreeMap<LevelIndex, LevelBodies>,
    /// The bricks of ships and of terrain changed since the last step.
    changed_ship_bricks: BTreeSet<(GridId, BrickCoordinate)>,
    changed_terrain_bricks: BTreeSet<(LevelIndex, BrickCoordinate)>,
    /// What the world is to be told of the weight of ships, at the end of the step.
    weighed: BTreeMap<GridId, (f32, [f32; 3])>,
}

fn pose_of(state: &BodyState) -> GridPose {
    GridPose::new(state.position, state.orientation)
}

impl Bodies {
    fn new(config: GridBodiesConfig) -> Self {
        let collisions = BlockCollisions::new(config.table.clone(), config.passable);
        Self {
            config,
            collisions,
            levels: BTreeMap::new(),
            changed_ship_bricks: BTreeSet::new(),
            changed_terrain_bricks: BTreeSet::new(),
            weighed: BTreeMap::new(),
        }
    }

    fn changed(&mut self, grid: GridId, brick: BrickCoordinate) {
        match grid.root_level() {
            Some(level) => {
                self.changed_terrain_bricks.insert((level, brick));
            }
            None => {
                self.changed_ship_bricks.insert((grid, brick));
            }
        }
    }

    /// The colliders of a brick of a ship, and the mass in it.
    fn remake_brick(
        &mut self,
        level: LevelIndex,
        id: GridId,
        brick: BrickCoordinate,
        ship: &MovingGrid,
    ) {
        let moments = self.moments_of(ship, brick);
        let Some(bodies) = self.levels.get_mut(&level) else {
            return;
        };
        let mut hull = ShipHull {
            voxels: &ship.voxels,
            block_channel: self.config.block_channel,
            state_channel: self.config.state_channel,
            collisions: &mut self.collisions,
        };
        // A brick too far from the ship's origin for a collider has none (and no mass either).
        let made = bodies.bodies.set_brick(BodyId(id.get()), brick, &mut hull);
        let moments = if made.is_ok() {
            moments
        } else {
            MassMoments::NOTHING
        };
        if let Some(record) = bodies.ships.get_mut(&id) {
            if moments.mass() > 0.0 {
                record.moments.insert(brick, moments);
            } else {
                record.moments.remove(&brick);
            }
        }
    }

    /// The mass in the voxels of a brick of a ship.
    fn moments_of(&self, ship: &MovingGrid, brick: BrickCoordinate) -> MassMoments {
        let Some(contents) = ship.voxels.brick(brick) else {
            return MassMoments::NOTHING;
        };
        let first = brick.origin();
        let mut parts = Vec::new();
        for z in 0..BRICK_EDGE_LENGTH {
            for y in 0..BRICK_EDGE_LENGTH {
                for x in 0..BRICK_EDGE_LENGTH {
                    let local = LocalVoxelIndex::new(x as u8, y as u8, z as u8);
                    let block = contents.get(self.config.block_channel, local);
                    let state = contents.get(self.config.state_channel, local);
                    let table = self.collisions.table();
                    let model = table.model_of(block, state);
                    let shape = table.shape(model);
                    if shape.is_empty() {
                        continue;
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
                    let density = table.get(self.config.density, model);
                    let mass = f64::from(density) * f64::from(filled) / 512.0;
                    parts.push(MassMoments::of_voxel(
                        [
                            first.x() + x as i64,
                            first.y() + y as i64,
                            first.z() + z as i64,
                        ],
                        mass,
                    ));
                }
            }
        }
        MassMoments::sum(parts)
    }

    /// One tick: what changed is made again, the ships are carried to where the world has them, the forces are applied, the bodies move and
    /// the ships are put where they went.
    fn step(&mut self, world: &mut ServerWorld) {
        let changed_ships = std::mem::take(&mut self.changed_ship_bricks);
        let changed_terrain = std::mem::take(&mut self.changed_terrain_bricks);
        let present: Vec<(GridId, LevelIndex)> = world
            .data()
            .grids()
            .iter()
            .map(|(id, ship)| (id, ship.level))
            .collect();

        // Ships that are gone leave their bodies; a level with no ships has no frame.
        for level in self.levels.values_mut() {
            let gone: Vec<GridId> = level
                .ships
                .keys()
                .filter(|id| !present.iter().any(|(held, _)| held == *id))
                .copied()
                .collect();
            for id in gone {
                level.ships.remove(&id);
                let _ = level.bodies.remove_body(BodyId(id.get()));
            }
        }
        self.levels.retain(|_, level| !level.ships.is_empty());

        for (id, level_index) in present {
            self.sync_ship(world, id, level_index, &changed_ships);
        }
        self.keep_terrain(world, &changed_terrain);

        for (id, (mass, centre)) in std::mem::take(&mut self.weighed) {
            let _ = world.set_grid_mass(id, mass, centre);
        }
        self.apply_gravity(world);
        for force in world.take_grid_forces() {
            let Some(level) = world
                .data()
                .grids()
                .get(force.grid)
                .and_then(|ship| self.levels.get_mut(&ship.level))
            else {
                continue;
            };
            if level.ships.contains_key(&force.grid) {
                let _ = level
                    .bodies
                    .apply_force(BodyId(force.grid.get()), force.force, force.at);
            }
        }

        for level in self.levels.values_mut() {
            level.bodies.step();
        }
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
        let kind = if ship.held {
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
        let known = self
            .levels
            .get(&level_index)
            .is_some_and(|level| level.ships.contains_key(&id));
        if !known {
            // The first ship of a level sets the frame; one too far from it is not simulated.
            let origin = match self.levels.get(&level_index) {
                Some(level) => level.bodies.origin(),
                None => ship.pose.position.brick(),
            };
            let far = |a: i64, b: i64| (a - b).abs() > FRAME_RADIUS_BRICKS;
            let brick = ship.pose.position.brick();
            if far(brick.x(), origin.x())
                || far(brick.y(), origin.y())
                || far(brick.z(), origin.z())
            {
                return;
            }
            let level = self
                .levels
                .entry(level_index)
                .or_insert_with(|| LevelBodies {
                    bodies: BodyWorld::new(
                        origin,
                        BodySettings {
                            gravity: [0.0; 3],
                            ..self.config.settings
                        },
                    ),
                    ships: BTreeMap::new(),
                    terrain: BTreeSet::new(),
                });
            if level
                .bodies
                .add_body(BodyId(id.get()), state, kind)
                .is_err()
            {
                return;
            }
            level.ships.insert(
                id,
                Ship {
                    moments: BTreeMap::new(),
                    mass: 0.0,
                    centre_of_mass: [0.0; 3],
                    gravity: [0.0; 3],
                    kind,
                    written: (ship.pose, ship.linear_velocity, ship.angular_velocity),
                },
            );
            let bricks: Vec<BrickCoordinate> = ship.voxels.sorted_brick_coordinates();
            for brick in bricks {
                self.remake_brick(level_index, id, brick, ship);
            }
            self.set_mass(level_index, id);
            return;
        }
        // Bricks that changed.
        let mut remade = false;
        let bricks: Vec<BrickCoordinate> = changed
            .iter()
            .filter(|(grid, _)| *grid == id)
            .map(|(_, brick)| *brick)
            .collect();
        for brick in bricks {
            self.remake_brick(level_index, id, brick, ship);
            remade = true;
        }
        if remade {
            self.set_mass(level_index, id);
        }
        let Some(level) = self.levels.get_mut(&level_index) else {
            return;
        };
        let Some(record) = level.ships.get_mut(&id) else {
            return;
        };
        if record.kind != kind {
            record.kind = kind;
            let _ = level.bodies.set_kind(BodyId(id.get()), kind);
        }
        // Moved by anything but the simulation: it goes on from there.
        if record.written != (ship.pose, ship.linear_velocity, ship.angular_velocity) {
            let _ = level.bodies.set_state(BodyId(id.get()), state);
            record.written = (ship.pose, ship.linear_velocity, ship.angular_velocity);
        }
    }

    /// Gives a body the mass of all its bricks.
    fn set_mass(&mut self, level_index: LevelIndex, id: GridId) {
        let Some(level) = self.levels.get_mut(&level_index) else {
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

    /// Terrain has colliders where ships are, and not far from them.
    fn keep_terrain(
        &mut self,
        world: &mut ServerWorld,
        changed: &BTreeSet<(LevelIndex, BrickCoordinate)>,
    ) {
        let level_indices: Vec<LevelIndex> = self.levels.keys().copied().collect();
        for level_index in level_indices {
            let mut near: BTreeSet<BrickCoordinate> = BTreeSet::new();
            let mut kept: BTreeSet<BrickCoordinate> = BTreeSet::new();
            let ship_ids: Vec<GridId> = self.levels[&level_index].ships.keys().copied().collect();
            for id in ship_ids {
                let Some(bounds) = world
                    .data()
                    .grids()
                    .get(id)
                    .and_then(MovingGrid::world_brick_bounds)
                else {
                    continue;
                };
                for (margin, set) in [
                    (TERRAIN_MARGIN_BRICKS, &mut near),
                    (TERRAIN_KEEP_BRICKS, &mut kept),
                ] {
                    for x in bounds.min().x() - margin..=bounds.max().x() + margin {
                        for y in bounds.min().y() - margin..=bounds.max().y() + margin {
                            for z in bounds.min().z() - margin..=bounds.max().z() + margin {
                                set.insert(BrickCoordinate::new(x, y, z));
                            }
                        }
                    }
                }
            }
            let loaded: Vec<BrickCoordinate> =
                self.levels[&level_index].terrain.iter().copied().collect();
            let to_drop: Vec<BrickCoordinate> = loaded
                .iter()
                .copied()
                .filter(|brick| !kept.contains(brick))
                .collect();
            let to_make: Vec<BrickCoordinate> = near
                .iter()
                .copied()
                .filter(|brick| !loaded.contains(brick))
                .chain(
                    changed
                        .iter()
                        .filter(|(level, _)| *level == level_index)
                        .map(|(_, brick)| *brick)
                        .filter(|brick| loaded.contains(brick)),
                )
                .collect();
            for brick in to_drop {
                let level = self.levels.get_mut(&level_index).expect("the level exists");
                level.bodies.remove_terrain_brick(brick);
                level.terrain.remove(&brick);
            }
            for brick in to_make {
                let mut source = WorldTerrain {
                    world,
                    level: level_index,
                    block_channel: self.config.block_channel,
                    state_channel: self.config.state_channel,
                    air: self.config.air,
                    collisions: &mut self.collisions,
                };
                let level = self.levels.get_mut(&level_index).expect("the level exists");
                level.bodies.set_terrain_brick(brick, &mut source);
                level.terrain.insert(brick);
            }
        }
    }

    /// Pulls every ship that moves with the gravity there is at its centre of mass: the level's, and the pull of the other ships that weigh enough.
    fn apply_gravity(&mut self, world: &ServerWorld) {
        for (level_index, level) in &mut self.levels {
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
                let gravity = world.gravity_at(*level_index, there);
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
        for level in self.levels.values_mut() {
            for (id, record) in &mut level.ships {
                let body = BodyId(id.get());
                if record.kind == BodyKind::Fixed || level.bodies.is_asleep(body) {
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
