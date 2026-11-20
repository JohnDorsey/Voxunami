use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use block_models::{BlockTable, PropertyHandle};
use namespaced_id::NamespacedId;
use physics::{
    BodyColliders, BodyId, BodyKind, BodyMotion, BodySettings, BodyState, BodyWorld, MassMoments,
    Reach, bricks_whose_colliders_read,
};
use plugin_host::{HandlerSpec, Phase, Plugin, Registrar, SystemSpec};
use region_octree::{BrickCube, RegionOctree};
use server::{MergePlan, ServerWorld, SplitPart};
use voxel_grid::{ChannelIndex, Grid};
use voxel_space::{
    AlignmentTolerance, BRICK_EDGE_LENGTH, BrickBox, BrickCoordinate, Displacement, GridId,
    GridPose, LevelIndex, LocalVoxelIndex, Orientation, VoxelCoordinate, WorldPosition,
};
use world_storage::MovingGrid;

use crate::collision::{BlockCollisions, GeneratedHull, ShipHull, WorldTerrain};
use crate::connectivity::{Connectivity, Outcome};
use crate::docking::{BlockFace, BlockFaceView, Docking, approach, docked_pose, torque_of_force};
use crate::joints::JointSettings;

mod couplings;

use couplings::{Coupled, CouplingKey};

/// How far, in bricks, from the origin of a frame a ship may be. (Chosen when Rapier worked in `f32`, which is exact to a hundredth of a voxel at 500 bricks; it works in `f64` now, which is exact
/// to 1e-10 of one there, and the limit is kept: raising it is open.) A ship that is not within this of the origin of any
/// frame of its level makes a frame of its own, centred on it, so ships far apart are all simulated; a ship that goes twice as far from the origin of
/// its frame is taken out of it and put in another (a frame is a place, not a ship's).
pub const FRAME_RADIUS_BRICKS: i128 = 500;

/// How far from the origin of a level's frame the origin of a planet may be, in bricks (4,096 is 131,072 voxels, where an `f32` was exact to a hundredth of one; an `f64` is exact to 1e-11 and the limit is kept, raising it is open):
/// the planet's voxels are in its own coordinates, which are as far from the frame as it is. A planet farther than this has no body, and the ships over it fall
/// through it (a planet that large needs a frame of its own).
pub const PLANET_REACH_BRICKS: i128 = 4096;

/// How far from a ship's bounds terrain has colliders, in voxels: a few, and the way the ship goes in a few ticks. (A brick of natural terrain
/// costs ten milliseconds or more to generate and make colliders of, on the thread of the tick, so a ship that has colliders for all the bricks
/// within a brick of its bounds, as it first had, stopped the game for a quarter of a second each time it crossed from one brick to the next.)
const TERRAIN_LOOKAHEAD_VOXELS: f64 = 8.0;
const TERRAIN_LOOKAHEAD_SECONDS: f64 = 0.25;
/// The terrain a ship *needs* colliders of now: the bricks its bounds reach in the next two ticks (its speed for a tenth of a second) and two voxels more. These
/// are always made in the tick, whatever it costs, so that a ship never reaches a brick that has none.
const TERRAIN_NEEDED_VOXELS: f64 = 2.0;
const TERRAIN_NEEDED_SECONDS: f64 = 0.1;
/// The rest of the lookahead is *wanted*, not needed: it is made nearest first, at most this many bricks a tick. It is a count and not a time, so that what
/// is simulated is the same on every machine (a brick of natural terrain costs 5 to 25 ms to generate and make colliders of, so this bounds a tick).
const TERRAIN_WANTED_BRICKS_PER_TICK: usize = 1;
/// How much farther each ring of the wanted bricks reaches, in voxels: the nearer a brick is in rings, the sooner it is made.
const TERRAIN_RING_VOXELS: f64 = 8.0;

/// How many bricks of ships that have just come into memory get their colliders, their mass, their ports and their joint ends in a tick, all the ships together. A count and not a
/// time, for the same reason as the budget of terrain (what is simulated must not depend on how fast a machine is); a brick costs from about 1.5 ms (a few blocks: the weighing alone) to
/// 10 ms (all 32,768 voxels stone: four of them took 41 to 48 ms of a tick, measured 2026-11-19), so a tick spends 45 ms at the most on them and a few milliseconds on a hollow hull. **A ship whose bricks are all made in the tick it comes (up to this many bricks, with the other ships that
/// came in it) is made whole, as it was before there was a budget**: only a bigger ship is *being made*, held where it is meanwhile (see `Making`).
pub const SHIP_BRICKS_PER_TICK: usize = 4;
/// How many bricks of terrain that a ship that is being made needs, and that cost anything to make (those that are not all air or all one solid block: a brick of natural terrain
/// with caves in it costs 12 ms), are made in a tick for each ground of each frame, once all the ship's own bricks are made. Bricks that cost nothing are made at once.
pub const SETTLING_TERRAIN_BRICKS_PER_TICK: usize = 2;

/// How many voxels of ships the checks for a ship that was cut in two may look at in a tick, all the ships together. A count and not a time, for
/// the same reason as the budget of terrain. A cut that does not separate anything is found in a few steps; one that does costs about the size of the
/// smaller part, so a ship that has lost a wing waits a few ticks (it is whole until the check is done) and a ship that is only dented does not.
const SPLIT_VISITS_PER_TICK: usize = 4096;
/// How much farther before the colliders of terrain are let go, so that a ship at the edge of the margin does not make and drop the same brick
/// every tick.
const TERRAIN_KEEP_EXTRA_VOXELS: f64 = 16.0;

/// How many ticks a pair of ships that could not be docked is left alone before it is tried again: a second.
const REFUSED_RETRY_TICKS: u64 = 20;

/// How much gravity may change, as a share of its size (or of one voxel a second squared, if that is more), before it wakes a ship that sleeps.
const GRAVITY_CHANGE_THAT_WAKES: f64 = 1e-3;

/// How near two places of a frame must be to be the same one, in voxels: a ship that a planet carried is where the frame has it to within the rounding of the
/// arithmetic of carrying it, which is millionths of a voxel; one that someone moved is at least this far from it.
const SAME_PLACE_VOXELS: f64 = 1e-3;

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
    /// How ships with joint ends are drawn together and coupled, if the game has any (see [`JointSettings`]).
    pub joints: Option<JointSettings>,
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
    /// Pairs of ships that are coupled by their joint ends now, and couplings made in all.
    pub couplings: u64,
    pub couplings_made: u64,
    /// Couplings that were made again in another frame, because the group they are in was moved there whole (a coupling that is made again is not one made).
    pub couplings_moved: u64,
    /// How many frames there are now: places in levels that have ships in them, each simulated in a frame of its own.
    pub frames: u64,
    /// Bricks of ships whose colliders and mass were worked out from their voxels (the 32,768 places of each looked at), in all: the first time a ship has a body, and each
    /// brick an edit changes.
    pub ship_bricks_made: u64,
    /// The same in the last tick, and in the most of any tick.
    pub ship_bricks_made_last_tick: u64,
    pub ship_bricks_made_most_in_a_tick: u64,
    /// Ships that are being made now: held where they are while their bricks, and then the terrain they need, are made a few at a tick (see [`SHIP_BRICKS_PER_TICK`]).
    pub ships_being_made: u64,
    /// Bricks of ships that went with the ship from the frame it left to the one it entered, with no voxel looked at (see [`BodyWorld::take_colliders`]), in all.
    pub ship_bricks_carried: u64,
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
    ports: BTreeMap<BrickCoordinate, Vec<BlockFace>>,
    /// The ends of joints in each brick that has any.
    joint_ends: BTreeMap<BrickCoordinate, Vec<BlockFace>>,
    kind: BodyKind,
    /// How much the ship weighs and where its centre of mass is in its coordinates, as last worked out.
    mass: f64,
    centre_of_mass: [f64; 3],
    /// The gravity the ship was last pulled by: a ship at rest sleeps while it is the same, and is woken when it changes.
    gravity: [f64; 3],
    /// What the simulation last said of where the ship is and how fast it moves: if the world says other, something else moved it.
    written: (GridPose, [f64; 3], [f64; 3]),
    /// Set while the ship is being made (see [`Making`]).
    making: Option<Making>,
}

/// A ship that came into memory and is too big to be made in the tick it came: its bricks are made a few at a tick (see [`SHIP_BRICKS_PER_TICK`]), and then the terrain it needs
/// a few bricks at a tick (see [`SETTLING_TERRAIN_BRICKS_PER_TICK`]). **Meanwhile it is held where it is**: its body is a fixed one (it does not move, whatever the world says of its
/// speed, and nothing is written back of it), the world keeps the weight it had of it (the weight of the bricks made so far is not the ship's), it asks for no terrain until its own bricks
/// are made, and it is neither docked nor coupled (the ports and joint ends it has are of the bricks made so far). Bricks that are made are solid to what comes near. When it is
/// made it goes on from the place, the speed and the kind (held, frozen, on a rail, free) the world has for it then.
struct Making {
    /// The bricks of the ship that have not been made yet, in order of coordinate. (An edit to one is left alone: it is made from what is in it when its turn comes.)
    unmade: BTreeSet<BrickCoordinate>,
    /// What the ship is to the solver when it is let go: the kind it would have if it were not held.
    kind: BodyKind,
    /// Whether the body has been given the ship's weight (when the last brick was made).
    weighed: bool,
    /// Whether all the terrain the ship needs had colliders at the end of the last tick's work (looked at once its bricks are made).
    terrain_ready: bool,
    /// Whether it is the tick the ship came in: a ship whose bricks are all made in it was never being made, and goes on at once, as it did before there was a budget.
    first_tick: bool,
}

/// A planet (a ship with terrain) in the simulation: a body that is fixed, whose colliders are only of the bricks that ships are near.
struct Planet {
    /// The bricks, in the planet's own coordinates, that have colliders (made from what is stored there, or else from the generator).
    bricks: BTreeSet<BrickCoordinate>,
    /// Where the planet was last put in the bodies: if the world says other, something else moved it.
    written: GridPose,
    /// Fixed for a planet that stands still in a frame that does; kinematic for one on a rail, and for every planet in a frame that goes with a planet.
    kind: BodyKind,
    /// The docking ports in each brick of the planet that has any: only blocks that were built there are ports, so only its stored bricks are looked at.
    ports: BTreeMap<BrickCoordinate, Vec<BlockFace>>,
    /// Whether the stored bricks have been looked at for ports (after that, the bricks that change).
    scanned: bool,
}

/// Which frame: the level it is in, and a number (a level has as many frames as it has places with ships in them).
type FrameKey = (LevelIndex, u32);

/// The bodies of one place in a level, in one frame: the frame's origin is the brick of the first ship that made it.
struct Frame {
    level: LevelIndex,
    /// The point the frame goes with, if it goes with one (see [`Follow`]).
    follow: Option<Follow>,
    bodies: BodyWorld,
    ships: BTreeMap<GridId, Ship>,
    /// The planets of the level that have a body. (They are no ships here: they weigh what their terrain says, are held, and do not keep a frame alive.)
    planets: BTreeMap<GridId, Planet>,
    /// The bricks of terrain that have colliders.
    terrain: BTreeSet<BrickCoordinate>,
}

/// What a frame goes with: a point that moves in the world (see [`Followed`]). The places of such a frame are the world's less how far the point has gone since the
/// frame was made, and the velocities in it are the world's less the point's.
/// For a planet on a rail that carries the ships in it (see `ServerWorld::carrier_of`): a ship that rests on the planet rests in the frame whatever the planet's speed,
/// and a ship in orbit round the planet moves as if the planet stood still and only turned. The frame is in free fall with the planet, so the gravity in it leaves out the
/// pull of what the planet goes round (`ServerWorld::gravity_in_the_free_fall_of`), and it has no terrain of the level, which stands still in the world (blocks built in
/// space near a planet on a rail are not solid to what goes with the planet).
/// For the centre of mass of a ship that turns on a rail: the frame has that one ship in it, at rest, with no pull and no planets, and no terrain of the level either.
/// For the centre of mass of a group of ships coupled by their joint ends that rides a rail: the frame has the ships of the group in it, free bodies with their joints, and
/// the same: no pull, no planets, no terrain, nothing else. The bodies hold the centre of mass of the group at the frame's point (see `Bodies::hold_groups_at_their_points`).
/// For the centre of mass of a group that is **off a rail and alone** (`ServerWorld::group_is_alone`): the same frame, but the point is nobody's rail: it is the group's own, and
/// the frame is in free fall, so the bodies carry it on from tick to tick (the pull of the world at the point and whatever the group does as a whole, a thrust or a push, are
/// taken from the bodies' step into the follow and the group put back at the point: see [`Follow::advance`]).
#[derive(Clone, Copy, Debug)]
struct Follow {
    followed: Followed,
    /// Where the point was when the frame was made.
    anchor: WorldPosition,
    /// How far it has gone since, as of this tick.
    shift: Displacement,
    /// How fast it goes in the world, as of this tick.
    velocity: [f64; 3],
}

/// The point a frame goes with, and what the world says of it. Everything that is particular to what is followed is here (which ships belong in the frame, where the point is
/// and how fast it goes, what pulls on a frame that falls with it); [`Follow`] only moves places and velocities by where it has gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Followed {
    /// A planet on a rail: the point is its place in the world, and the ships in the frame are those it carries.
    Planet(GridId),
    /// The centre of mass of a ship that turns on a rail: the rail holds that point and nothing else, and the ship's turn is the bodies' work, done in a frame of its own with
    /// the ship at rest at the point. It has nothing else in it (no planet, no terrain, no other ship) and nothing pulls on it: it falls with the ship.
    RailPoint(GridId),
    /// The centre of mass of a group of ships coupled by their joint ends, whose anchor (the ship of the group with the lowest number, named) holds the rail: the rail holds the
    /// centre of mass of the ships and nothing else, and what the ships do against each other is the bodies' work, done in a frame of its own, the ships free bodies with their
    /// joints. Nothing is in it but the group and nothing pulls on it: it falls with the group. The ships in it are put in the world by the frame (their places are the point's
    /// and what the bodies have), and the world does not put them in it.
    RailGroup(GridId),
    /// The centre of mass of a group of ships coupled by their joint ends that is off a rail and alone, named by its anchor, the ship of it with the lowest number. As a
    /// [`Followed::RailGroup`] frame it has nothing in it but the group and nothing pulls on the bodies in it; unlike it, no rail holds the point: the follow is moved by the
    /// bodies each tick from what the world's pull is at the point and from what the group did as a whole (see [`Follow::advance`]), and the world does not move the group but the
    /// frame does. The frame is made where the group is, so its numbers are the group's own, small and relative, however far it goes and however fast.
    Group(GridId),
}

impl Followed {
    /// Where the point is in the world now, and how fast it goes; `None` if the world has nothing of it.
    fn state(self, world: &ServerWorld) -> Option<(WorldPosition, [f64; 3])> {
        match self {
            Self::Planet(planet) => {
                let grid = world.data().grids().get(planet)?;
                Some((grid.pose.position, grid.linear_velocity))
            }
            Self::RailPoint(ship) => {
                let grid = world.data().grids().get(ship)?;
                let middle = grid.pose.local_to_world(WorldPosition::new(
                    BrickCoordinate::ORIGIN,
                    grid.centre_of_mass,
                ))?;
                Some((middle, grid.linear_velocity))
            }
            Self::RailGroup(anchor) => world.rail_group_centre(anchor),
            Self::Group(anchor) => {
                let grids = world.data().grids();
                let centre = grids.group_centre(&grids.group_of(anchor))?;
                Some((centre.position, centre.velocity))
            }
        }
    }

    /// Whether the follow of such a frame is moved by the bodies (the point is the group's own) and not found from the world at the start of each tick.
    fn moves_itself(self) -> bool {
        matches!(self, Self::Group(_))
    }

    /// The acceleration of gravity at a place of the world for what falls with the point; `None` where nothing pulls what is in the frame at all.
    fn gravity(self, world: &ServerWorld, there: WorldPosition) -> Option<[f64; 3]> {
        match self {
            Self::Planet(planet) => Some(world.gravity_in_the_free_fall_of(planet, there)),
            Self::RailPoint(_) | Self::RailGroup(_) | Self::Group(_) => None,
        }
    }

    /// Whether the planets of the level are bodies in a frame that goes with this. (Not in the frame of a ship on a rail: nothing there is to meet one.)
    fn has_planets(self) -> bool {
        matches!(self, Self::Planet(_))
    }

    /// What the frame of a ship goes with: the planet that carries it (see `ServerWorld::carrier_of`), or the centre of mass of its group if it is coupled to other ships and the
    /// group rides a rail, or its own centre of mass if it turns on a rail (a rail with no spin of its own: it is the rail that has the place and the bodies that have the turn),
    /// or, if it is off a rail and alone, its own (see [`Followed::Group`]), or nothing. **A group is in its frame whole, frozen or not:** a ship of it that is frozen is a fixed body there, which the others hang from by their joints (a group
    /// that goes in and out of a frame as it is frozen and let go would lose its joints and its bodies' state each time; a ship that is held has taken the rail from its group).
    fn of(
        world: &ServerWorld,
        id: GridId,
        ship: &MovingGrid,
        alone: &BTreeSet<GridId>,
    ) -> Option<Self> {
        if let Some(planet) = world.carrier_of(id) {
            return Some(Self::Planet(planet));
        }
        if world.data().grids().has_partners(id) {
            if let Some(group) = world.rail_group_of(id) {
                return Some(Self::RailGroup(group.anchor));
            }
            // (A group off a rail that is alone, `Bodies::alone_groups`: the ship of it with the lowest number speaks for it.)
            let anchor = *world.data().grids().group_of(id).first()?;
            return alone.contains(&anchor).then_some(Self::Group(anchor));
        }
        let turns_on_a_rail = !ship.held
            && !ship.frozen
            && ship.angular_velocity != [0.0; 3]
            && ship.rail.as_ref().is_some_and(|rail| rail.spin.is_none());
        turns_on_a_rail.then_some(Self::RailPoint(id))
    }
}

impl Follow {
    /// A frame made now for the point `followed`; `None` if the world has nothing of it.
    fn begin(followed: Followed, world: &ServerWorld) -> Option<Follow> {
        let (anchor, velocity) = followed.state(world)?;
        Some(Follow {
            followed,
            anchor,
            shift: Displacement::NONE,
            velocity,
        })
    }

    /// Where the point has gone since the frame was made, and how fast it goes, as of this tick (both stay as they were if the world has nothing of it).
    fn update(&mut self, world: &ServerWorld) {
        if self.followed.moves_itself() {
            return;
        }
        if let Some((position, velocity)) = self.followed.state(world) {
            self.shift = self.anchor.displacement_to(position);
            self.velocity = velocity;
        }
    }

    /// Moves the follow of a frame that moves itself by a tick, after the bodies have stepped. The group was at rest at the point, and the frame went at `velocity`; in the step
    /// the bodies moved its centre of mass `moved` and gave it the speed `sped` against the frame (a thrust or a push, or a blow, as a whole); the pull of the world at the point is
    /// `gravity`. The group is then where it would be if it had gone at `velocity` for a tick, been pulled by `gravity` and moved as the bodies moved it, and goes at `velocity` and
    /// what the bodies gave it and the pull over a tick: **a semi-implicit step, as the bodies take their own**. The bodies put the group back at the point and at rest in the frame
    /// (`BodyWorld::shift_bodies` by `-moved` and `-sped`), which changes nothing of where the ships are in the world: places in the frame are the world's less the shift.
    fn advance(&mut self, time_step: f64, gravity: [f64; 3], moved: [f64; 3], sped: [f64; 3]) {
        let (shift, velocity) =
            advanced(self.shift, self.velocity, time_step, gravity, moved, sped);
        self.shift = shift;
        self.velocity = velocity;
    }

    fn local(&self, world: WorldPosition) -> Option<WorldPosition> {
        world.displaced(self.shift.negated())
    }

    fn world(&self, local: WorldPosition) -> Option<WorldPosition> {
        local.displaced(self.shift)
    }
}

/// Where a place of the world is in a frame that goes with `follow`, or in one that goes with nothing (where it is the same); `None` where the world cannot hold it.
fn local_in(follow: Option<Follow>, world: WorldPosition) -> Option<WorldPosition> {
    match follow {
        None => Some(world),
        Some(follow) => follow.local(world),
    }
}

/// How fast something goes in such a frame, from how fast it goes in the world.
fn velocity_in(follow: Option<Follow>, world: [f64; 3]) -> [f64; 3] {
    match follow {
        None => world,
        Some(follow) => [0, 1, 2].map(|axis| world[axis] - follow.velocity[axis]),
    }
}

/// How fast something goes in the world, from how fast it goes in such a frame.
fn velocity_from(follow: Option<Follow>, local: [f64; 3]) -> [f64; 3] {
    match follow {
        None => local,
        Some(follow) => [0, 1, 2].map(|axis| local[axis] + follow.velocity[axis]),
    }
}

/// A displacement moved by `delta` voxels, kept as whole bricks and a rest in `[0, 32)`, so that it is exact however far a frame has gone (an offset that was added to for hours
/// would not be).
fn shifted_by(shift: Displacement, delta: [f64; 3]) -> Displacement {
    let edge = BRICK_EDGE_LENGTH as f64;
    let mut bricks = shift.bricks;
    let mut offset = shift.offset;
    for axis in 0..3 {
        offset[axis] += delta[axis];
        let whole = (offset[axis] / edge).floor();
        bricks[axis] += whole as i128;
        offset[axis] -= whole * edge;
    }
    Displacement { bricks, offset }
}

/// The shift and the velocity of a frame that moves itself after a tick (see [`Follow::advance`]): the shift moves by `velocity * time_step`, the pull for a tick on it and the
/// bodies' own move of the group; the velocity gains what the bodies gave the group and the pull over the tick.
fn advanced(
    shift: Displacement,
    velocity: [f64; 3],
    time_step: f64,
    gravity: [f64; 3],
    moved: [f64; 3],
    sped: [f64; 3],
) -> (Displacement, [f64; 3]) {
    let kick = gravity.map(|part| part * time_step);
    let delta =
        [0, 1, 2].map(|axis| velocity[axis] * time_step + kick[axis] * time_step + moved[axis]);
    let faster = [0, 1, 2].map(|axis| velocity[axis] + sped[axis] + kick[axis]);
    (shifted_by(shift, delta), faster)
}

/// What a ship is to the solver. A ship that is held or frozen does not move; **neither does one that is on a rail**: it is where the clock puts it whatever the solver does, so it
/// is a fixed body that other ships are stopped by, and it asks for no terrain of a planet (a fixed ship is held against it). The world takes a ship off its rail before anything near
/// it could meet it or push it (`server::ServerWorld::update_coasting`), and then it is the solver's again.
/// (The exception is a ship that turns on a rail: the caller does not say it is on one, since it is a free body in a frame of its own, see [`Followed::of`].)
fn body_kind(held: bool, frozen: bool, on_a_rail: bool) -> BodyKind {
    if held || frozen || on_a_rail {
        BodyKind::Fixed
    } else {
        BodyKind::Dynamic
    }
}

/// Whether two places of a frame are the same (see [`SAME_PLACE_VOXELS`]).
fn same_place(first: WorldPosition, second: WorldPosition) -> bool {
    first
        .displacement_to(second)
        .in_voxels()
        .iter()
        .all(|apart| apart.abs() < SAME_PLACE_VOXELS)
}

/// The state of a ship's body in a frame, from its pose and velocities in the world; `None` where the world cannot give it a place. A ship that is held is at
/// rest in a frame that goes with a planet, as it is in one that does not: it goes with the planet.
fn ship_state_in(
    follow: Option<Follow>,
    kind: BodyKind,
    pose: GridPose,
    linear_velocity: [f64; 3],
    angular_velocity: [f64; 3],
) -> Option<BodyState> {
    Some(BodyState {
        position: local_in(follow, pose.position)?,
        orientation: pose.orientation,
        linear_velocity: if follow.is_some() && kind == BodyKind::Fixed {
            [0.0; 3]
        } else {
            velocity_in(follow, linear_velocity)
        },
        angular_velocity,
    })
}

/// Where a body that is at `state` at the end of a step of `time_step` seconds was at the start of it, going as it goes. A kinematic body is set to this before the
/// step, so that it ends the step where the world says it is, and what rests on it is carried as the planet turns and goes, and not a step ahead of it.
fn state_before(state: BodyState, time_step: f64) -> BodyState {
    let position = state
        .position
        .translated(state.linear_velocity.map(|speed| -speed * time_step));
    let [x, y, z] = state.angular_velocity.map(f64::from);
    let speed = libm::sqrt(x * x + y * y + z * z);
    let orientation = if speed > 0.0 {
        let half = 0.5 * speed * time_step;
        let (sine, cosine) = (libm::sin(half) / speed, libm::cos(half));
        match Orientation::from_components([x * sine, y * sine, z * sine, cosine]) {
            Some(turn) => turn.inverse().compose(state.orientation),
            None => state.orientation,
        }
    } else {
        state.orientation
    };
    BodyState {
        position,
        orientation,
        linear_velocity: state.linear_velocity,
        angular_velocity: state.angular_velocity,
    }
}

/// The state a planet's body is given before the step of a tick, from the pose and velocities the world has the planet at (the end of the tick): a planet that moves,
/// set as it was at the start of the step (see [`state_before`]), and one that stands still, as it is. In the frame it is in: its place there, and its velocities
/// against the frame. `None` where the world cannot give it a place there.
fn planet_state(
    follow: Option<Follow>,
    moves: bool,
    pose: GridPose,
    linear_velocity: [f64; 3],
    angular_velocity: [f64; 3],
    time_step: f64,
) -> Option<BodyState> {
    let end_of_tick = BodyState {
        position: local_in(follow, pose.position)?,
        orientation: pose.orientation,
        linear_velocity: velocity_in(follow, linear_velocity),
        angular_velocity,
    };
    Some(if moves {
        state_before(end_of_tick, time_step)
    } else {
        end_of_tick
    })
}

/// Terrain that has colliders: the level's own, whose bricks are the frame's, or the ground of a planet, whose bricks are the planet's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ground {
    Level,
    Planet(GridId),
}

/// What a ground's colliders are to do in this tick (see [`Bodies::bricks_to_change`]).
#[derive(Default)]
struct TerrainPlan {
    /// Bricks that lose their colliders.
    to_drop: Vec<BrickCoordinate>,
    /// Bricks that get them (or get them again): all that are needed now, the nearest of those wanted, and those that were edited and had them.
    to_make: Vec<BrickCoordinate>,
    /// Bricks wanted ahead that are left for a later tick, nearest first.
    ahead: Vec<BrickCoordinate>,
    /// The bricks each ship that is being made needs, made or not.
    settling: BTreeMap<GridId, BTreeSet<BrickCoordinate>>,
}

/// Which bricks of a level's grid are stored (built in, dug out of, or kept for any other reason): the bricks that are not what the generator makes of them. A cube that the
/// generator says is air all through (`WorldTerrain::is_air`) is air all through only if none of them is in it, so a ship's terrain is not looked for in it only if this says no.
///
/// It is kept from the bricks that changed in each tick, and checked each tick against how many bricks the grid has: if the two do not agree something changed that nobody
/// said, and it is made again from the grid. (A mistake here is a ship that goes through what was built, and so it is not left to be one.)
struct StoredBricks {
    index: RegionOctree<bool>,
    count: usize,
}

impl StoredBricks {
    /// Made from the bricks of a grid.
    fn scan(grid: &Grid) -> Self {
        let mut stored = Self {
            index: RegionOctree::new(false),
            count: 0,
        };
        for (brick, _) in grid.bricks() {
            stored.set(brick, true);
        }
        stored
    }

    fn set(&mut self, brick: BrickCoordinate, stored: bool) {
        if *self.index.get(brick) == stored {
            return;
        }
        self.index.set_cube(BrickCube::containing(brick, 0), stored);
        if stored {
            self.count += 1;
        } else {
            self.count -= 1;
        }
    }

    /// Whether any brick of the cube is stored.
    fn any_in(&self, cube: BrickCube) -> bool {
        !self.index.all_in_cube(cube, |stored| !*stored)
    }
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
    /// Which bricks of each level are stored in its grid (see [`StoredBricks`]), for the levels that have a frame.
    stored_bricks: BTreeMap<LevelIndex, StoredBricks>,
    /// The same for the planets that have a body: which bricks of their own voxels are stored (built or dug), by planet.
    stored_planet_bricks: BTreeMap<GridId, StoredBricks>,
    /// What the world is to be told of the weight of ships, at the end of the step.
    weighed: BTreeMap<GridId, (f64, [f64; 3])>,
    /// Pairs of ships (the lower id first) that could not be docked, and the tick before which they are not tried again nor pulled together: the
    /// attempt works out the whole of the smaller ship, and a pair whose blocks meet and are pushed about by the bodies would be tried every tick.
    refused: BTreeMap<(GridId, GridId), u64>,
    /// The pairs of ships that are coupled by their joint ends, by frame and the two ships (see `couplings.rs`).
    couplings: BTreeMap<CouplingKey, Coupled>,
    /// The number the next coupling is given.
    next_coupling: u64,
    /// What the ships that left a frame in this step take to the frame they enter in the same step, by ship (empty again at the end of the step).
    carried: BTreeMap<GridId, Carried>,
    /// Which tick this is, counting the steps of the bodies.
    tick: u64,
    /// The groups, named by their anchors, that are off a rail and alone this tick (see [`Bodies::alone_groups`]).
    alone: BTreeSet<GridId>,
    report: Arc<Mutex<BodiesReport>>,
}

/// What a ship takes from the frame it leaves to the one it enters: the colliders of its bricks and what was worked out of each (the mass, the docking ports, the ends of
/// joints), so that no brick has its 32,768 voxels looked at again. The body is made again (it has a place, a speed and a kind of the new frame); these are what the body is made of.
struct Carried {
    colliders: BodyColliders,
    moments: BTreeMap<BrickCoordinate, MassMoments>,
    ports: BTreeMap<BrickCoordinate, Vec<BlockFace>>,
    joint_ends: BTreeMap<BrickCoordinate, Vec<BlockFace>>,
}

/// A ship that is being looked at to see whether it is still in one piece.
struct Check {
    /// The blocks beside the places where blocks were taken out, since the ship was last known to be whole.
    seeds: BTreeSet<VoxelCoordinate>,
    flood: Connectivity,
}

const FACES: [[i128; 3]; 6] = [
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
fn farther_than(radius: i128, first: BrickCoordinate, second: BrickCoordinate) -> bool {
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
            stored_bricks: BTreeMap::new(),
            stored_planet_bricks: BTreeMap::new(),
            next_frame: 0,
            changed_ship_bricks: BTreeSet::new(),
            changed_ship_voxels: BTreeMap::new(),
            checks: BTreeMap::new(),
            changed_terrain_bricks: BTreeSet::new(),
            weighed: BTreeMap::new(),
            refused: BTreeMap::new(),
            couplings: BTreeMap::new(),
            next_coupling: 0,
            carried: BTreeMap::new(),
            tick: 0,
            alone: BTreeSet::new(),
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

    /// Notes that a voxel changed. The colliders of its brick are made again, and so are those of the brick across each face of it that the voxel is on (they are made
    /// from the voxels across that face: a box with nothing open touching it has none, and one beside solid terrain reaches into it).
    fn changed(&mut self, grid: GridId, voxel: VoxelCoordinate) {
        let bricks = bricks_whose_colliders_read(voxel);
        match grid.root_level() {
            Some(level) => {
                self.changed_terrain_bricks
                    .extend(bricks.into_iter().map(|brick| (level, brick)));
            }
            None => {
                self.changed_ship_bricks
                    .extend(bricks.into_iter().map(|brick| (grid, brick)));
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
        self.count(|report| report.ship_bricks_made += 1);
        // (The table the faces are read by: the game's docking has one, and a game with joints and no docking uses the game's own.)
        let faces_by = match &self.config.docking {
            Some(docking) => Some(docking.table()),
            None => self.config.joints.is_some().then_some(&self.config.table),
        };
        let BrickScan {
            moments,
            ports,
            joint_ends,
        } = self.weigher().scan_brick(ship, brick, faces_by);
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
        let (moments, ports, joint_ends) = if made.is_ok() {
            (moments, ports, joint_ends)
        } else {
            (MassMoments::NOTHING, Vec::new(), Vec::new())
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
            if joint_ends.is_empty() {
                record.joint_ends.remove(&brick);
            } else {
                record.joint_ends.insert(brick, joint_ends);
            }
        }
    }

    /// Gives a ship that has come into a frame what it took from the one it left, and makes again only the bricks that were changed in this step (the ones it was not looked at
    /// again for are as they were: its voxels do not change when it goes from one frame to another, only the body's place, speed and kind). `bricks` are all its stored bricks.
    fn take_over(
        &mut self,
        frame: FrameKey,
        id: GridId,
        ship: &MovingGrid,
        carried: Carried,
        bricks: &[BrickCoordinate],
        changed: &BTreeSet<(GridId, BrickCoordinate)>,
    ) {
        let Carried {
            colliders,
            moments,
            ports,
            joint_ends,
        } = carried;
        let given = self.frames.get_mut(&frame).is_some_and(|bodies| {
            bodies
                .bodies
                .give_colliders(BodyId(id.get()), colliders)
                .is_ok()
        });
        if !given {
            self.begin_making(frame, id, bricks);
            return;
        }
        if let Some(record) = self
            .frames
            .get_mut(&frame)
            .and_then(|bodies| bodies.ships.get_mut(&id))
        {
            record.moments = moments;
            record.ports = ports;
            record.joint_ends = joint_ends;
        }
        let carried_count = bricks.len() as u64;
        self.count(|report| report.ship_bricks_carried += carried_count);
        let edited: Vec<BrickCoordinate> = changed
            .iter()
            .filter(|(grid, _)| *grid == id)
            .map(|(_, brick)| *brick)
            .collect();
        for brick in edited {
            self.remake_brick(frame, id, brick, ship);
        }
    }

    /// Has the bricks of a ship that has a body and nothing else of it made in this tick and the ones after it (see [`Bodies::make_ship_bricks`]).
    fn begin_making(&mut self, frame: FrameKey, id: GridId, bricks: &[BrickCoordinate]) {
        if let Some(record) = self
            .frames
            .get_mut(&frame)
            .and_then(|bodies| bodies.ships.get_mut(&id))
        {
            record.making = Some(Making {
                unmade: bricks.iter().copied().collect(),
                kind: record.kind,
                weighed: false,
                terrain_ready: false,
                first_tick: true,
            });
        }
    }

    /// The record of a ship in a frame, if it has one.
    fn record_of(&self, frame: FrameKey, id: GridId) -> Option<&Ship> {
        self.frames.get(&frame)?.ships.get(&id)
    }

    /// Whether a ship is being made (see [`Making`]).
    fn is_being_made(&self, frame: FrameKey, id: GridId) -> bool {
        self.record_of(frame, id)
            .is_some_and(|record| record.making.is_some())
    }

    /// Whether a brick of a ship is one that has not been made yet.
    fn is_unmade(&self, frame: FrameKey, id: GridId, brick: BrickCoordinate) -> bool {
        self.record_of(frame, id).is_some_and(|record| {
            record
                .making
                .as_ref()
                .is_some_and(|making| making.unmade.contains(&brick))
        })
    }

    /// Whether a ship still has bricks that have not been made.
    fn has_bricks_unmade(&self, frame: FrameKey, id: GridId) -> bool {
        self.record_of(frame, id).is_some_and(|record| {
            record
                .making
                .as_ref()
                .is_some_and(|making| !making.unmade.is_empty())
        })
    }

    /// Makes the bricks of the ships that are being made, **no more than [`SHIP_BRICKS_PER_TICK`] in all**, and weighs a ship when its last brick is made. A ship whose bricks are
    /// all made in the tick it came was never being made: it goes on at once, as it did before there was a budget.
    /// When the bricks that are left of all the ships fit in the budget they are made in order of ship, each ship whole, as they were before there was a budget; when they do not, a
    /// brick of each ship in turn (in order of ship), so that a big ship does not keep a small one that came after it from being made.
    fn make_ship_bricks(&mut self, world: &ServerWorld) {
        let mut order: Vec<(GridId, FrameKey)> = self
            .frames
            .iter()
            .flat_map(|(key, frame)| {
                frame
                    .ships
                    .iter()
                    .filter(|(_, record)| {
                        record
                            .making
                            .as_ref()
                            .is_some_and(|making| !making.unmade.is_empty())
                    })
                    .map(move |(id, _)| (*id, *key))
            })
            .collect();
        order.sort();
        let total: usize = order
            .iter()
            .filter_map(|(id, key)| self.record_of(*key, *id)?.making.as_ref())
            .map(|making| making.unmade.len())
            .sum();
        let (mut left, each) = if total <= SHIP_BRICKS_PER_TICK {
            (usize::MAX, usize::MAX)
        } else {
            (SHIP_BRICKS_PER_TICK, 1)
        };
        'rounds: loop {
            let mut made_any = false;
            for &(id, key) in &order {
                if left == 0 {
                    break 'rounds;
                }
                let Some(ship) = world.data().grids().get(id) else {
                    continue;
                };
                let Some(making) = self
                    .frames
                    .get_mut(&key)
                    .and_then(|frame| frame.ships.get_mut(&id))
                    .and_then(|record| record.making.as_mut())
                else {
                    continue;
                };
                let bricks: Vec<BrickCoordinate> =
                    making.unmade.iter().copied().take(each.min(left)).collect();
                for brick in &bricks {
                    making.unmade.remove(brick);
                }
                made_any |= !bricks.is_empty();
                left = left.saturating_sub(bricks.len());
                for brick in bricks {
                    self.remake_brick(key, id, brick, ship);
                }
            }
            if !made_any {
                break;
            }
        }
        // The ships whose bricks are all made have their weight now (a ship that has no bricks too).
        let weighed: Vec<(GridId, FrameKey)> = self
            .frames
            .iter()
            .flat_map(|(key, frame)| {
                frame
                    .ships
                    .iter()
                    .filter(|(_, record)| {
                        record
                            .making
                            .as_ref()
                            .is_some_and(|making| making.unmade.is_empty() && !making.weighed)
                    })
                    .map(move |(id, _)| (*id, *key))
            })
            .collect();
        for (id, key) in weighed {
            self.set_mass(key, id);
            if let Some(record) = self
                .frames
                .get_mut(&key)
                .and_then(|frame| frame.ships.get_mut(&id))
                && let Some(making) = record.making.as_mut()
            {
                making.weighed = true;
                if making.first_tick {
                    record.making = None;
                }
            }
        }
    }

    /// At the end of the tick's work: holds the ships that are still being made (a fixed body, whatever it is to be), and lets go those that are made, their bricks and the terrain
    /// they need, to go on from the place, the speed and the kind the world has for them now.
    fn settle_ships(&mut self, world: &ServerWorld) {
        let mut made: Vec<(FrameKey, GridId)> = Vec::new();
        let mut being_made = 0;
        for (key, frame) in &mut self.frames {
            for (id, record) in &mut frame.ships {
                let Some(making) = record.making.as_mut() else {
                    continue;
                };
                making.first_tick = false;
                if making.unmade.is_empty() && making.terrain_ready {
                    made.push((*key, *id));
                    continue;
                }
                being_made += 1;
                if record.kind != BodyKind::Fixed {
                    record.kind = BodyKind::Fixed;
                    let _ = frame.bodies.set_kind(BodyId(id.get()), BodyKind::Fixed);
                }
            }
        }
        for (key, id) in made {
            self.release_ship(world, key, id);
        }
        self.count(|report| report.ships_being_made = being_made);
    }

    /// Lets a ship that is made go on: its body is what the world says it is (its kind, and where it is and how fast it goes if it was held for the making).
    fn release_ship(&mut self, world: &ServerWorld, key: FrameKey, id: GridId) {
        let Some(ship) = world.data().grids().get(id) else {
            return;
        };
        let Some(frame) = self.frames.get_mut(&key) else {
            return;
        };
        let Some(record) = frame.ships.get_mut(&id) else {
            return;
        };
        let Some(making) = record.making.take() else {
            return;
        };
        if record.kind == making.kind {
            return;
        }
        record.kind = making.kind;
        let _ = frame.bodies.set_kind(BodyId(id.get()), making.kind);
        if let Some(state) = ship_state_in(
            frame.follow,
            making.kind,
            ship.pose,
            ship.linear_velocity,
            ship.angular_velocity,
        ) {
            let _ = frame.bodies.set_state(BodyId(id.get()), state);
        }
        record.written = (ship.pose, ship.linear_velocity, ship.angular_velocity);
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
        let ship_bricks_before = self.report().ship_bricks_made;
        self.refused.retain(|_, until| tick < *until);
        let changed_ships = std::mem::take(&mut self.changed_ship_bricks);
        let changed_terrain = std::mem::take(&mut self.changed_terrain_bricks);
        let changed_voxels = std::mem::take(&mut self.changed_ship_voxels);
        self.dock(world);
        self.split_ships(world, changed_voxels);
        self.follow_points(world);
        self.alone = self.alone_groups(world);
        // (A ship with terrain, a planet, is no ship here: its blocks are mostly not stored, it weighs what its terrain says and it is held where it is.
        // It is a fixed body, which ships collide with, below.)
        let present: Vec<(GridId, LevelIndex)> = world
            .data()
            .grids()
            .iter()
            .filter(|(_, ship)| ship.terrain.is_none())
            .map(|(id, ship)| (id, ship.level))
            .collect();

        // Ships that are gone, or that are carried by another planet than the one their frame goes with (or by none, or by one when it goes with none), or that have
        // gone twice as far from the origin of their frame as a ship may be in one, leave their bodies (the last two kinds are made bodies again below, in the frame of
        // the place they are in); a frame with no ships is no more.
        let mut leaving: BTreeSet<GridId> = BTreeSet::new();
        for frame in self.frames.values() {
            let origin = frame.bodies.origin();
            let follow = frame.follow;
            leaving.extend(frame.ships.keys().filter(|id| {
                !present.iter().any(|(held, _)| held == *id)
                    || world.data().grids().get(**id).is_none_or(|ship| {
                        Followed::of(world, **id, ship, &self.alone)
                            != follow.map(|follow| follow.followed)
                            || local_in(follow, ship.pose.position).is_none_or(|place| {
                                farther_than(2 * FRAME_RADIUS_BRICKS, place.brick(), origin)
                            })
                    })
            }));
        }
        // A group of ships coupled by their joint ends is always in one frame, so the ships coupled to one that leaves leave with it, and are put in a frame together below
        // (a joint dies with the body that has it: a car left behind would lose it). A ship that is gone altogether takes nobody with it.
        let going: BTreeSet<GridId> = leaving
            .iter()
            .copied()
            .filter(|id| present.iter().any(|(held, _)| held == id))
            .collect();
        leaving.extend(
            self.coupled_with(&going)
                .into_iter()
                .filter(|id| present.iter().any(|(held, _)| held == id)),
        );
        for frame in self.frames.values_mut() {
            let gone: Vec<GridId> = frame
                .ships
                .keys()
                .filter(|id| leaving.contains(id))
                .copied()
                .collect();
            for id in gone {
                let record = frame.ships.remove(&id);
                // A ship that is still in the world goes to another frame, and takes what was made of its bricks with it (one that is gone altogether takes nothing, and nor does
                // one that is being made: it has only some of its bricks, and is begun again in the frame it enters).
                let colliders = frame.bodies.take_colliders(BodyId(id.get())).ok();
                let _ = frame.bodies.remove_body(BodyId(id.get()));
                if let (Some(record), Some(colliders)) = (record, colliders)
                    && record.making.is_none()
                    && present.iter().any(|(held, _)| *held == id)
                {
                    self.carried.insert(
                        id,
                        Carried {
                            colliders,
                            moments: record.moments,
                            ports: record.ports,
                            joint_ends: record.joint_ends,
                        },
                    );
                }
            }
        }
        self.frames.retain(|_, frame| !frame.ships.is_empty());

        for (id, level_index) in present {
            self.sync_ship(world, id, level_index, &changed_ships);
        }
        // (What was not given to a body in this step, a ship that could not be made one, is made again from its voxels in the next.)
        self.carried.clear();
        self.make_ship_bricks(world);
        self.sync_planets(world);
        self.scan_planets_for_ports(world, &changed_ships);
        self.keep_terrain(world, &changed_terrain, &changed_ships);
        self.settle_ships(world);

        for (id, (mass, centre)) in std::mem::take(&mut self.weighed) {
            let _ = world.set_grid_mass(id, mass, centre);
        }
        self.apply_gravity(world);
        self.couple_joints(world);
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
        self.hold_groups_at_their_points(world);
        let frames = self.frames.len() as u64;
        self.count(|report| {
            report.frames = frames;
            report.ship_bricks_made_last_tick = report.ship_bricks_made - ship_bricks_before;
            report.ship_bricks_made_most_in_a_tick = report
                .ship_bricks_made_most_in_a_tick
                .max(report.ship_bricks_made_last_tick);
        });
        self.write_back(world);
    }

    /// What the report says of the bricks made so far, as a copy.
    fn report(&self) -> BodiesReport {
        *self
            .report
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
        // A ship that turns on a rail is a free body in a frame of its own (the rail holds its centre of mass, the bodies turn it); any other ship on a rail is a fixed one.
        let followed = Followed::of(world, id, ship, &self.alone);
        let rides = matches!(
            followed,
            Some(Followed::RailPoint(_) | Followed::RailGroup(_))
        );
        let kind = body_kind(ship.held, ship.frozen, ship.rail.is_some() && !rides);
        let Some(key) = self.frame_of(id) else {
            // A ship is put in the first frame of its level that goes with the planet that carries it (or with none, if none does) and that it is near the origin of,
            // and makes a frame of its own where it is if there is none.
            let brick = ship.pose.position.brick();
            let near = |key: &FrameKey, frame: &Frame| {
                key.0 == level_index
                    && frame.follow.map(|follow| follow.followed) == followed
                    && local_in(frame.follow, ship.pose.position).is_some_and(|place| {
                        !farther_than(FRAME_RADIUS_BRICKS, place.brick(), frame.bodies.origin())
                    })
            };
            // (The frame of a ship it is coupled to, if it is near enough to be in it: a group moved from one frame to another goes whole.)
            let partners = self
                .frame_of_a_coupled_partner(id)
                .filter(|key| self.frames.get(key).is_some_and(|frame| near(key, frame)));
            let key = partners.or_else(|| {
                self.frames
                    .iter()
                    .find(|(key, frame)| near(key, frame))
                    .map(|(key, _)| *key)
            });
            let key = match key {
                Some(key) => key,
                None => {
                    let follow = match followed {
                        None => None,
                        Some(followed) => {
                            let Some(follow) = Follow::begin(followed, world) else {
                                return;
                            };
                            Some(follow)
                        }
                    };
                    let key = (level_index, self.next_frame);
                    self.next_frame += 1;
                    self.frames.insert(
                        key,
                        Frame {
                            level: level_index,
                            follow,
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
            let Some(state) = ship_state_in(
                frame.follow,
                kind,
                ship.pose,
                ship.linear_velocity,
                ship.angular_velocity,
            ) else {
                return;
            };
            if frame
                .bodies
                .add_body(BodyId(id.get()), state, kind)
                .is_err()
            {
                return;
            }
            // (A rider touches nothing in its frame, so it never sleeps and its slow turn is for good: a body sleeps only while it touches something.)
            frame.ships.insert(
                id,
                Ship {
                    moments: BTreeMap::new(),
                    ports: BTreeMap::new(),
                    joint_ends: BTreeMap::new(),
                    mass: 0.0,
                    centre_of_mass: [0.0; 3],
                    gravity: [0.0; 3],
                    kind,
                    written: (ship.pose, ship.linear_velocity, ship.angular_velocity),
                    making: None,
                },
            );
            let bricks: Vec<BrickCoordinate> = ship.voxels.sorted_brick_coordinates();
            match self.carried.remove(&id) {
                Some(carried) => self.take_over(key, id, ship, carried, &bricks, changed),
                // (Its bricks are made below, a few at a tick for all the ships that came together, and its weight when the last is.)
                None => self.begin_making(key, id, &bricks),
            }
            if !self.is_being_made(key, id) {
                self.set_mass(key, id);
            }
            return;
        };
        // Bricks that changed. (One of a ship that is being made and has no colliders yet is made from what is in it when its turn comes.)
        let mut remade = false;
        let bricks: Vec<BrickCoordinate> = changed
            .iter()
            .filter(|(grid, _)| *grid == id)
            .map(|(_, brick)| *brick)
            .collect();
        for brick in bricks {
            if self.is_unmade(key, id, brick) {
                continue;
            }
            self.remake_brick(key, id, brick, ship);
            remade = true;
        }
        // (A ship that is still having its bricks made is weighed when the last is.)
        if remade && !self.has_bricks_unmade(key, id) {
            self.set_mass(key, id);
        }
        let Some(frame) = self.frames.get_mut(&key) else {
            return;
        };
        let Some(record) = frame.ships.get_mut(&id) else {
            return;
        };
        // A ship that is being made is held (a fixed body) whatever it is to be: what it is to be is kept for when it is let go.
        let kind = match record.making.as_mut() {
            Some(making) => {
                making.kind = kind;
                BodyKind::Fixed
            }
            None => kind,
        };
        // A ship that changes what it is to the solver gets its state from the world: a fixed body has no speed (a ship on a rail has the speed its rail gives it, in the
        // world's data and nowhere else), so a ship taken off its rail (by a ship that came near: nothing was written for it, the rail's pose is the last one the body was
        // given) would go on from rest.
        let kind_changed = record.kind != kind;
        if kind_changed {
            record.kind = kind;
            let _ = frame.bodies.set_kind(BodyId(id.get()), kind);
        }
        // Moved by anything but the simulation: it goes on from there. (In a frame that goes with a planet, a ship that the planet carried is where it was from the
        // planet, which is not where it was in the world, so its place is compared in the frame.)
        let follow = frame.follow;
        let written = record.written;
        let moved = kind_changed
            || match follow {
                None => written != (ship.pose, ship.linear_velocity, ship.angular_velocity),
                // A ship of a group on a rail is the bodies' (its places against the others, its speeds against them, its turn): the world moves the whole group along the rail
                // and does not put a ship of it anywhere, and anything else that does takes the group off the rail first, which takes it out of the frame.
                Some(Follow {
                    followed: Followed::RailGroup(_),
                    ..
                }) => false,
                Some(_) => {
                    written.0.orientation != ship.pose.orientation
                        || written.1 != ship.linear_velocity
                        || written.2 != ship.angular_velocity
                        || match (
                            local_in(follow, ship.pose.position),
                            frame.bodies.state(BodyId(id.get())),
                        ) {
                            (Some(place), Some(body)) => !same_place(place, body.position),
                            _ => true,
                        }
                }
            };
        if moved {
            if let Some(state) = ship_state_in(
                follow,
                kind,
                ship.pose,
                ship.linear_velocity,
                ship.angular_velocity,
            ) {
                let _ = frame.bodies.set_state(BodyId(id.get()), state);
            }
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

    /// The groups of ships coupled by their joint ends that are off a rail and alone (`ServerWorld::group_is_alone`), by their anchors. A group that has a frame of its own
    /// keeps it while it has the room of 1 and one that has not needs the room of 2 to get it, as for a rail, so that it does not go in and out of it at the edge.
    fn alone_groups(&self, world: &ServerWorld) -> BTreeSet<GridId> {
        let grids = world.data().grids();
        let mut alone = BTreeSet::new();
        for (id, _) in grids.iter() {
            if !grids.has_partners(id) || grids.group_of(id).first() != Some(&id) {
                continue;
            }
            let has_its_frame = self.frames.values().any(|frame| {
                frame
                    .follow
                    .is_some_and(|follow| follow.followed == Followed::Group(id))
            });
            let room = if has_its_frame { 1.0 } else { 2.0 };
            if world.group_is_alone(id, room) {
                alone.insert(id);
            }
        }
        alone
    }

    /// Where each frame that goes with a point is, and how fast it goes: the point's displacement since the frame was made, and its velocity.
    fn follow_points(&mut self, world: &ServerWorld) {
        for follow in self
            .frames
            .values_mut()
            .filter_map(|frame| frame.follow.as_mut())
        {
            follow.update(world);
        }
    }

    /// Gives the planets of each level a body in each of its frames, puts those that were moved where they are, and lets those that are gone go. A planet on a rail is
    /// a kinematic body that goes at the velocity its rail gives it, and so is every planet in a frame that goes with a planet (they move against it), set every tick
    /// to where it was at the start of the tick (the world has it where it is at the end), so that what rests on it is carried with it.
    fn sync_planets(&mut self, world: &ServerWorld) {
        let time_step = self.config.settings.time_step;
        let planets: Vec<(GridId, LevelIndex)> = world
            .data()
            .grids()
            .iter()
            .filter(|(_, ship)| ship.terrain.is_some())
            .map(|(id, ship)| (id, ship.level))
            .collect();
        for frame in self.frames.values_mut() {
            let (level, follow) = (frame.level, frame.follow);
            if follow.is_some_and(|follow| !follow.followed.has_planets()) {
                continue;
            }
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
                let moves = planet.rail.is_some() || follow.is_some();
                let kind = if moves {
                    BodyKind::Kinematic
                } else {
                    BodyKind::Fixed
                };
                let Some(state) = planet_state(
                    follow,
                    moves,
                    planet.pose,
                    planet.linear_velocity,
                    planet.angular_velocity,
                    time_step,
                ) else {
                    continue;
                };
                let origin = frame.bodies.origin();
                match frame.planets.get_mut(&id) {
                    Some(record) => {
                        // A planet that moves is let go when it is far from the frame (it comes back when it is near: the colliders it had are made again).
                        if moves
                            && farther_than(2 * PLANET_REACH_BRICKS, state.position.brick(), origin)
                        {
                            frame.planets.remove(&id);
                            let _ = frame.bodies.remove_body(BodyId(id.get()));
                            continue;
                        }
                        if record.kind != kind {
                            let _ = frame.bodies.set_kind(BodyId(id.get()), kind);
                            record.kind = kind;
                        }
                        if moves || record.written != planet.pose {
                            let _ = frame.bodies.set_state(BodyId(id.get()), state);
                            record.written = planet.pose;
                        }
                    }
                    None => {
                        if farther_than(PLANET_REACH_BRICKS, state.position.brick(), origin) {
                            continue;
                        }
                        if frame.bodies.add_body(BodyId(id.get()), state, kind).is_ok() {
                            frame.planets.insert(
                                id,
                                Planet {
                                    bricks: BTreeSet::new(),
                                    written: planet.pose,
                                    kind,
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
                    let ports = weigher.scan_brick(grid, brick, Some(docking.table())).ports;
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
        self.refresh_stored_bricks(world, changed_terrain, changed_ships);
        let keys: Vec<FrameKey> = self.frames.keys().copied().collect();
        let mut made_this_tick = 0u64;
        for key in keys {
            let level_index = self.frames[&key].level;
            let planets: Vec<GridId> = self.frames[&key].planets.keys().copied().collect();
            // (The terrain of the level stands still in the world, and a frame that goes with a planet does not.)
            let level_ground = self.frames[&key].follow.is_none();
            // The ships that are being made and need terrain that has no colliders yet, in all the grounds of the frame.
            let mut unready: BTreeSet<GridId> = BTreeSet::new();
            for ground in level_ground
                .then_some(Ground::Level)
                .into_iter()
                .chain(planets.into_iter().map(Ground::Planet))
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
                let TerrainPlan {
                    to_drop,
                    to_make,
                    ahead,
                    settling,
                } = self.bricks_to_change(world, key, ground, &changed);
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
                    made_this_tick +=
                        u64::from(self.make_ground_brick(world, key, level_index, ground, brick));
                }
                // The terrain that the ships being made need: the bricks that cost something to make, a few a tick (those with nothing in them, or all of one block, are made
                // at once), in order of coordinate.
                let missing: BTreeSet<BrickCoordinate> = settling
                    .values()
                    .flatten()
                    .copied()
                    .filter(|brick| !self.has_ground_brick(key, ground, *brick))
                    .collect();
                let mut costly_left = SETTLING_TERRAIN_BRICKS_PER_TICK;
                for brick in missing {
                    if self.ground_brick_costs(world, level_index, ground, brick) {
                        if costly_left == 0 {
                            continue;
                        }
                        costly_left -= 1;
                    }
                    made_this_tick +=
                        u64::from(self.make_ground_brick(world, key, level_index, ground, brick));
                }
                for (id, bricks) in &settling {
                    if !bricks
                        .iter()
                        .all(|brick| self.has_ground_brick(key, ground, *brick))
                    {
                        unready.insert(*id);
                    }
                }
            }
            // A ship that is being made, whose bricks are made, is ready to be let go when it needs no more terrain than there is.
            let frame = self.frames.get_mut(&key).expect("the frame exists");
            for (id, record) in &mut frame.ships {
                if let Some(making) = record.making.as_mut()
                    && making.unmade.is_empty()
                {
                    making.terrain_ready = !unready.contains(id);
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

    /// Whether a brick of a ground has colliders.
    fn has_ground_brick(&self, key: FrameKey, ground: Ground, brick: BrickCoordinate) -> bool {
        let Some(frame) = self.frames.get(&key) else {
            return false;
        };
        match ground {
            Ground::Level => frame.terrain.contains(&brick),
            Ground::Planet(id) => frame
                .planets
                .get(&id)
                .is_some_and(|planet| planet.bricks.contains(&brick)),
        }
    }

    /// Whether making the colliders of a brick of a ground costs something: it has voxels of more than one kind in it (one that is all air or all one block is made with no
    /// look at its voxels). The hint is what the making of it asks first.
    fn ground_brick_costs(
        &mut self,
        world: &mut ServerWorld,
        level_index: LevelIndex,
        ground: Ground,
        brick: BrickCoordinate,
    ) -> bool {
        use physics::{BrickHint, CollisionSource};
        let hint = match ground {
            Ground::Level => WorldTerrain {
                world,
                level: level_index,
                block_channel: self.config.block_channel,
                state_channel: self.config.state_channel,
                orientation_channel: self.config.orientation_channel,
                air: self.config.air,
                collisions: &mut self.collisions,
            }
            .brick_hint(brick),
            Ground::Planet(id) => GeneratedHull {
                world,
                grid: id,
                block_channel: self.config.block_channel,
                state_channel: self.config.state_channel,
                orientation_channel: self.config.orientation_channel,
                air: self.config.air,
                collisions: &self.collisions,
            }
            .brick_hint(brick),
        };
        hint == BrickHint::Unknown
    }

    /// Gives a brick of a ground its colliders (again, if it had them). Whether it had none.
    fn make_ground_brick(
        &mut self,
        world: &mut ServerWorld,
        key: FrameKey,
        level_index: LevelIndex,
        ground: Ground,
        brick: BrickCoordinate,
    ) -> bool {
        let level = self.frames.get_mut(&key).expect("the frame exists");
        match ground {
            Ground::Level => {
                let new = !level.terrain.contains(&brick);
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
                new
            }
            Ground::Planet(id) => {
                let Some(planet) = level.planets.get_mut(&id) else {
                    return false;
                };
                let new = !planet.bricks.contains(&brick);
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
                new
            }
        }
    }

    /// Brings what is known of the stored bricks of the levels that have a frame up to date: a level seen for the first time is scanned, and in the others the bricks that
    /// were edited in this tick are read again; and if the count does not agree with the grid's, the index is made again.
    fn refresh_stored_bricks(
        &mut self,
        world: &ServerWorld,
        changed_terrain: &BTreeSet<(LevelIndex, BrickCoordinate)>,
        changed_ships: &BTreeSet<(GridId, BrickCoordinate)>,
    ) {
        let planets: BTreeSet<GridId> = self
            .frames
            .values()
            .flat_map(|frame| frame.planets.keys().copied())
            .collect();
        self.stored_planet_bricks
            .retain(|id, _| planets.contains(id));
        for id in planets {
            let Some(planet) = world.data().grids().get(id) else {
                continue;
            };
            let grid = &planet.voxels;
            let stored = self
                .stored_planet_bricks
                .entry(id)
                .or_insert_with(|| StoredBricks::scan(grid));
            for (_, brick) in changed_ships.iter().filter(|(changed, _)| *changed == id) {
                stored.set(*brick, grid.brick(*brick).is_some());
            }
            if stored.count != grid.brick_count() {
                *stored = StoredBricks::scan(grid);
            }
        }
        let levels: BTreeSet<LevelIndex> = self.frames.values().map(|frame| frame.level).collect();
        for level in levels {
            let Some(data) = world.data().level(level) else {
                continue;
            };
            let grid = &data.overrides;
            let stored = self
                .stored_bricks
                .entry(level)
                .or_insert_with(|| StoredBricks::scan(grid));
            for (_, brick) in changed_terrain
                .iter()
                .filter(|(changed, _)| *changed == level)
            {
                stored.set(*brick, grid.brick(*brick).is_some());
            }
            if stored.count != grid.brick_count() {
                *stored = StoredBricks::scan(grid);
            }
        }
    }

    /// Which bricks of a ground are to lose their colliders, which are to get them (or get them again, if `changed` has them and they had) this tick, which are
    /// wanted and left for a later tick, nearest first, and which each ship that is being made needs (made a few at a tick, see [`SETTLING_TERRAIN_BRICKS_PER_TICK`]).
    ///
    /// A brick of a ground is left out of all of it when it is in a cube that the generator says is air all through and nothing is stored in: a brick of air has no colliders, so it
    /// is not made, and not kept. For a ship in empty space that is no brick at all, whatever its speed.
    fn bricks_to_change(
        &self,
        world: &ServerWorld,
        key: FrameKey,
        ground: Ground,
        changed: &BTreeSet<BrickCoordinate>,
    ) -> TerrainPlan {
        let level = &self.frames[&key];
        // Which bricks of the ground have colliders. (A planet's colliders are asked for wherever a ship is near, and not only in the bricks its generator makes: what is
        // built past them is the planet's too.)
        let loaded = match ground {
            Ground::Level => &level.terrain,
            Ground::Planet(id) => {
                let Some(planet) = level.planets.get(&id) else {
                    return TerrainPlan::default();
                };
                &planet.bricks
            }
        };
        // The bricks a body will reach, for margins that grow: each is told with the first of the margins that has it (see `physics::visit_bricks_by_first_margin`: the bricks
        // of a margin are not made, since a ship at 400,000 voxels a second has 8,000 of them, each a tube of a thousand boxes).
        let visit_bricks_near =
            |body: BodyId,
             reach: Reach<'_>,
             skip: &mut dyn FnMut(BrickCube) -> bool,
             visit: &mut dyn FnMut(BrickCoordinate, usize)| match ground {
                Ground::Level => level.bodies.visit_bricks_ahead(body, reach, skip, visit),
                Ground::Planet(id) => {
                    level
                        .bodies
                        .visit_bricks_ahead_in(body, reach, BodyId(id.get()), skip, visit)
                }
            };
        // A cube of the ground that has no ground in it: for a level, the generator that makes all of it says it is air, and no brick of it is stored; for a planet, the same of the
        // cubes that are in the bricks its generator makes, and a cube that is outside them is what the layout of its grid has for nothing, which is air if that is (see
        // `GeneratedHull`).
        let air_of_the_level = world
            .data()
            .level(level.level)
            .zip(self.stored_bricks.get(&level.level));
        let air_of_the_planet = match ground {
            Ground::Level => None,
            Ground::Planet(id) => (|| {
                let planet = world.data().grids().get(id)?;
                let terrain = planet.terrain?;
                let generator = world
                    .data()
                    .main()
                    .terrain
                    .generators()
                    .entry(terrain.generator)?
                    .content()?
                    .clone();
                let nothing_is_air = planet
                    .voxels
                    .layout()
                    .default_value(self.config.block_channel)
                    == self.config.air;
                Some((
                    terrain.extent,
                    generator,
                    nothing_is_air,
                    self.stored_planet_bricks.get(&id)?,
                ))
            })(),
        };
        let mut skip = |cube: BrickCube| match ground {
            Ground::Level => air_of_the_level
                .is_some_and(|(data, stored)| data.terrain.is_air(cube) && !stored.any_in(cube)),
            Ground::Planet(_) => air_of_the_planet.as_ref().is_some_and(
                |(extent, generator, nothing_is_air, stored)| {
                    let last = cube.edge_length() as i128 - 1;
                    let origin = cube.origin();
                    let whole = BrickBox::new(
                        origin,
                        BrickCoordinate::new(
                            origin.x().saturating_add(last),
                            origin.y().saturating_add(last),
                            origin.z().saturating_add(last),
                        ),
                    );
                    let Some(whole) = whole else {
                        return false;
                    };
                    let is_air = if extent.contains(whole.min()) && extent.contains(whole.max()) {
                        generator.is_air(cube)
                    } else {
                        *nothing_is_air && extent.intersection(whole).is_none()
                    };
                    is_air && !stored.any_in(cube)
                },
            ),
        };
        // The bricks that are needed now, those that are wanted (with how many rings from a ship), and those that are kept. A planet is fixed, so the ships
        // that would be held against it need nothing of it.
        let mut needed: BTreeSet<BrickCoordinate> = BTreeSet::new();
        let mut settling: BTreeMap<GridId, BTreeSet<BrickCoordinate>> = BTreeMap::new();
        let mut wanted: BTreeMap<BrickCoordinate, usize> = BTreeMap::new();
        let mut kept: BTreeSet<BrickCoordinate> = BTreeSet::new();
        for (id, record) in &level.ships {
            // A ship that is being made asks for nothing while its own bricks are made (it is held, and what it needs is where it will be let go from, which is all of it); then it
            // asks for what it needs as the ship it is to be (a free ship asks for a planet's ground, a held one does not), the terrain it needs made a few bricks at a tick.
            let (kind, being_made) = match &record.making {
                Some(making) if making.unmade.is_empty() => (making.kind, true),
                Some(_) => continue,
                None => (record.kind, false),
            };
            if ground != Ground::Level && kind == BodyKind::Fixed {
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
            // (The way it goes, in the axes of the frame. A ship that stands has none: its margins are the cube.)
            let way = if speed > 0.0 {
                [vx / speed, vy / speed, vz / speed]
            } else {
                [0.0; 3]
            };
            // The margins, the least first: what is needed, then a ring of what is wanted for each 8 voxels more to the lookahead (ring 1 is the first of them), then what is kept.
            let mut margins = vec![needed_margin];
            let mut margin = needed_margin;
            while margin < lookahead {
                margin = (margin + TERRAIN_RING_VOXELS).min(lookahead);
                margins.push(margin);
            }
            let rings = margins.len() - 1;
            margins.push(lookahead + TERRAIN_KEEP_EXTRA_VOXELS);
            let mut needed_by_this_ship: BTreeSet<BrickCoordinate> = BTreeSet::new();
            let reach = Reach {
                margins: &margins,
                cap: physics::TERRAIN_EVERY_WAY_VOXELS,
                direction: way,
            };
            let reached = visit_bricks_near(body, reach, &mut skip, &mut |brick, margin| {
                kept.insert(brick);
                if margin == 0 {
                    needed_by_this_ship.insert(brick);
                }
                // (What is needed is in the first ring of what is wanted, and what is kept and no more is in none.)
                if rings > 0 && margin <= rings {
                    let ring = margin.max(1);
                    wanted
                        .entry(brick)
                        .and_modify(|nearest| *nearest = (*nearest).min(ring))
                        .or_insert(ring);
                }
            });
            if reached.is_some() {
                if being_made {
                    settling.entry(*id).or_default().extend(needed_by_this_ship);
                } else {
                    needed.extend(needed_by_this_ship);
                }
            }
        }
        kept.extend(needed.iter().copied());
        kept.extend(settling.values().flatten().copied());
        let to_drop: Vec<BrickCoordinate> = loaded
            .iter()
            .copied()
            .filter(|brick| !kept.contains(brick))
            .collect();
        // Everything needed; of the wanted, the nearest few that are not made yet (ties by coordinate: the map is in order). (What a ship that is being made needs is made on
        // its own account, so it is not wanted as well.)
        let mut missing_wanted: Vec<(usize, BrickCoordinate)> = wanted
            .iter()
            .filter(|(brick, _)| {
                !needed.contains(brick)
                    && !loaded.contains(brick)
                    && !settling.values().any(|bricks| bricks.contains(brick))
            })
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
        TerrainPlan {
            to_drop,
            to_make,
            ahead,
            settling,
        }
    }

    /// Pulls every ship that moves with the gravity there is at its centre of mass: the level's, and the pull of the other ships that weigh enough.
    /// (In a frame that goes with a planet it is the planet's, and everything's but that of what the planet goes round: the frame falls with the planet.)
    fn apply_gravity(&mut self, world: &ServerWorld) {
        for frame in self.frames.values_mut() {
            let (level_index, follow, level) = (frame.level, frame.follow, frame);
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
                let gravity = match follow {
                    None => world.gravity_at(level_index, there),
                    Some(follow) => match follow.followed.gravity(world, there) {
                        Some(gravity) => gravity,
                        None => continue,
                    },
                };
                let force = gravity.map(|component| component * record.mass);
                // A change in gravity wakes a ship that is asleep (a heavy ship came near); the same gravity does not, so a ship at rest sleeps.
                let change: f64 = (0..3)
                    .map(|axis| (gravity[axis] - record.gravity[axis]).abs())
                    .sum();
                let size: f64 = gravity.iter().map(|component| component.abs()).sum();
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

    /// Holds the centre of mass of each group that has a frame of its own at the point of the frame, at rest there: all its bodies are moved by the one vector, and their speeds
    /// changed by the one, that put it there. What the bodies do against each other is what it was; the joints and the turns are not touched. **For a group on a rail the rail is the
    /// authority for where the group goes**, and the group's own motion adds nothing to it: a body world conserves momentum to the rounding of its steps, and what is left is taken
    /// out here each tick (an hour of ticks adds up to nothing that shows); an edit that moves the centre of mass of the group (a block placed or broken, which changes what a ship
    /// weighs and where its middle is) steps the whole group by the shift, as it steps a single ship, which is the owner's decision of 2026-11-10 and what a single ship on a rail
    /// does. **For a group that is off a rail and alone nothing is the authority but the bodies and the pull of the world**: what the group did as a whole in the step (the move
    /// of its centre of mass and the speed it gained) goes into the follow of the frame, with the pull at the point (see [`Follow::advance`]), and the group is put back at the
    /// point and at rest in it, which changes nothing of where its ships are in the world; an edit that moves the centre of mass does not step the group.
    fn hold_groups_at_their_points(&mut self, world: &ServerWorld) {
        let time_step = self.config.settings.time_step;
        for frame in self.frames.values_mut() {
            let Some(Follow {
                followed: Followed::RailGroup(_) | Followed::Group(_),
                anchor,
                ..
            }) = frame.follow
            else {
                continue;
            };
            // (A group with a ship in it that is being made is not whole: it is held where it is until the ship is made, and then put at its point.)
            if frame.ships.values().any(|record| record.making.is_some()) {
                continue;
            }
            let (mut mass, mut apart, mut speed) = (0.0f64, [0.0f64; 3], [0.0f64; 3]);
            for (id, record) in &frame.ships {
                if record.mass <= 0.0 || record.kind == BodyKind::Fixed {
                    continue;
                }
                let Some(state) = frame.bodies.state(BodyId(id.get())) else {
                    continue;
                };
                let Some(middle) = pose_of(&state).local_to_world(WorldPosition::new(
                    BrickCoordinate::ORIGIN,
                    record.centre_of_mass,
                )) else {
                    continue;
                };
                let from_the_point = middle.relative_to(anchor);
                for axis in 0..3 {
                    apart[axis] += record.mass * from_the_point[axis];
                    speed[axis] += record.mass * state.linear_velocity[axis];
                }
                mass += record.mass;
            }
            if mass <= 0.0 {
                continue;
            }
            let moved = apart.map(|part| part / mass);
            let sped = speed.map(|part| part / mass);
            frame
                .bodies
                .shift_bodies(moved.map(|part| -part), sped.map(|part| -part));
            // (A group that is alone is carried on by the follow: the pull at the point it was at, and what it did as a whole.)
            let level = frame.level;
            if let Some(follow) = frame.follow.as_mut()
                && follow.followed.moves_itself()
                && let Some(point) = follow.world(anchor)
            {
                let gravity = world.gravity_at(level, point);
                follow.advance(time_step, gravity, moved, sped);
            }
        }
    }

    /// Puts the ships where the bodies are.
    fn write_back(&mut self, world: &mut ServerWorld) {
        for level in self.frames.values_mut() {
            if let Some(follow) = level.follow {
                // In a frame that goes with a planet every ship is written, the held and the sleeping too: where it is in the world is where it is in the frame and the
                // planet's displacement, which changes every tick, and it goes at the planet's velocity and what it has against the planet (nothing, for a body that
                // is held: `ship_state_in` gives it no velocity).
                for (id, record) in &mut level.ships {
                    let Some(state) = level.bodies.state(BodyId(id.get())) else {
                        continue;
                    };
                    let Some(position) = follow.world(state.position) else {
                        continue;
                    };
                    let now = (
                        GridPose::new(position, state.orientation),
                        velocity_from(Some(follow), state.linear_velocity),
                        state.angular_velocity,
                    );
                    if now == record.written {
                        continue;
                    }
                    if world.move_grid_by_bodies(*id, now.0, now.1, now.2).is_ok() {
                        record.written = now;
                    }
                }
                continue;
            }
            for (id, record) in &mut level.ships {
                let body = BodyId(id.get());
                if record.kind == BodyKind::Fixed {
                    continue;
                }
                if level.bodies.is_asleep(body) {
                    // A body asleep has no speed, whatever it was last written with: that is what someone who steps onto it should find.
                    let (pose, linear, angular) = record.written;
                    if (linear != [0.0; 3] || angular != [0.0; 3])
                        && world.slow_grid_by_bodies(*id, [0.0; 3], [0.0; 3]).is_ok()
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
                if world.move_grid_by_bodies(*id, now.0, now.1, now.2).is_ok() {
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
    ports: &'a BTreeMap<BrickCoordinate, Vec<BlockFace>>,
    ship: &'a MovingGrid,
}

/// A pair of ships with a pair of ports that pull on each other: the closest such pair of ports of the two.
struct PortPair {
    frame: FrameKey,
    first: GridId,
    second: GridId,
    first_port: BlockFace,
    second_port: BlockFace,
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
                    // (A ship that is being made has the ports of the bricks made so far: it is docked when it is made.)
                    (!ship.frozen && !record.ports.is_empty() && record.making.is_none()).then_some(
                        Bearer {
                            id: *id,
                            moves: record.kind != BodyKind::Fixed,
                            ports: &record.ports,
                            ship,
                        },
                    )
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
                        let first_view = BlockFaceView::of(*first_port, &first_ship.pose, from);
                        for second_port in second.ports.values().flatten() {
                            let second_view =
                                BlockFaceView::of(*second_port, &second_ship.pose, from);
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
    fn levels_ports(&self) -> BTreeMap<GridId, Vec<BlockFace>> {
        self.frames
            .values()
            .flat_map(|frame| {
                // (A ship that is being made has none: it is docked when it is made.)
                let ships = frame
                    .ships
                    .iter()
                    .filter(|(_, ship)| ship.making.is_none())
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
        let mut state = level.bodies.state(BodyId(id.get()))?;
        // (Its velocity in the world, and not against the planet the frame goes with.)
        state.linear_velocity = velocity_from(level.follow, state.linear_velocity);
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
                    whole.linear_velocity.map(|speed| speed),
                    whole.angular_velocity.map(|speed| speed),
                ),
                None => ([0.0; 3], [0.0; 3]),
            },
            _ => ([0.0; 3], [0.0; 3]),
        };
        // Where the lighter ship is when it has docked exactly.
        let still_view = BlockFaceView::of(still_port, &still_pose, from);
        let moving_view = BlockFaceView::of(moving_port, &moving_pose, from);
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
        (planet, planet_port): (GridId, BlockFace),
        (ship, ship_port): (GridId, BlockFace),
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
        let still_view = BlockFaceView::of(planet_port, &still_pose, from);
        let moving_view = BlockFaceView::of(ship_port, &moving_pose, from);
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
                let arm = [0, 1, 2].map(|axis| at[axis] - centre[axis]);
                let lever = torque_of_force(ship.pose.rotation_matrix(), arm, pull);
                // The second ship turns toward line and the first the other way.
                let turn = [0, 1, 2].map(|axis| lever[axis] - twist[axis] * sign);
                let body = BodyId(id.get());
                let _ = level
                    .bodies
                    .apply_force(body, pull.map(|component| component), None);
                let _ = level
                    .bodies
                    .apply_torque(body, turn.map(|component| component));
            }
        }
    }
}

/// What one look at a brick of a ship finds (see [`Weigher::scan_brick`]).
pub(crate) struct BrickScan {
    /// The mass in the brick.
    pub moments: MassMoments,
    /// The faces of its docking ports (`engine:docking_face`), in the order its voxels are scanned.
    pub ports: Vec<BlockFace>,
    /// The faces that are the ends of its joints (`engine:joint_face`), in the same order. A joint end is never in `ports` and a port never here.
    pub joint_ends: Vec<BlockFace>,
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
        voxel: [i128; 3],
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

    /// The mass in the voxels of a brick of a ship, and (if a table to read them by is given) the docking ports and the ends of joints in it.
    pub fn scan_brick(
        &self,
        ship: &MovingGrid,
        brick: BrickCoordinate,
        faces_by: Option<&BlockTable>,
    ) -> BrickScan {
        let Some(contents) = ship.voxels.brick(brick) else {
            return BrickScan {
                moments: MassMoments::NOTHING,
                ports: Vec::new(),
                joint_ends: Vec::new(),
            };
        };
        let first = brick.origin();
        let mut parts = Vec::new();
        let mut ports = Vec::new();
        let mut joint_ends = Vec::new();
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
                        first.x() + x as i128,
                        first.y() + y as i128,
                        first.z() + z as i128,
                    ];
                    parts.extend(self.voxel_moments(voxel, block, state, orientation));
                    if let Some(table) = faces_by
                        && block != self.air
                    {
                        let at = VoxelCoordinate::new(voxel[0], voxel[1], voxel[2]);
                        if let Some(face) = table.docking_face(block, state, orientation) {
                            ports.push(BlockFace { voxel: at, face });
                        }
                        if let Some(face) = table.joint_face(block, state, orientation) {
                            joint_ends.push(BlockFace { voxel: at, face });
                        }
                    }
                }
            }
        }
        BrickScan {
            moments: MassMoments::sum(parts),
            ports,
            joint_ends,
        }
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
                        .map(|value| value)
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

    fn cube_at(x: i128, y: i128, z: i128, level: u8) -> BrickCube {
        BrickCube::containing(BrickCoordinate::new(x, y, z), level)
    }

    #[test]
    fn stored_bricks_say_whether_any_brick_of_a_cube_is_stored_and_count_those_that_are() {
        let mut stored = StoredBricks {
            index: RegionOctree::new(false),
            count: 0,
        };
        let at = |x, y, z| BrickCoordinate::new(x, y, z);
        assert!(!stored.any_in(cube_at(0, 0, 0, 116)), "nothing at all");
        stored.set(at(5, 6, 7), true);
        stored.set(at(5, 6, 7), true);
        stored.set(at(-40, 2, 9), true);
        assert_eq!(stored.count, 2, "a brick is counted once");
        // The cube of 8 bricks that has the first, and the bigger ones that have it, and none that are beside it.
        assert!(stored.any_in(cube_at(0, 0, 0, 3)));
        assert!(stored.any_in(cube_at(0, 0, 0, 20)));
        assert!(stored.any_in(cube_at(0, 0, 0, 116)));
        assert!(stored.any_in(cube_at(5, 6, 7, 0)));
        assert!(!stored.any_in(cube_at(6, 6, 7, 0)));
        assert!(!stored.any_in(cube_at(8, 0, 0, 3)));
        assert!(!stored.any_in(cube_at(0, 8, 0, 3)));
        // One that is let go is not stored, and the cube that had only it has none.
        stored.set(at(5, 6, 7), false);
        stored.set(at(5, 6, 7), false);
        assert_eq!(stored.count, 1);
        assert!(!stored.any_in(cube_at(0, 0, 0, 3)));
        assert!(stored.any_in(cube_at(-64, 0, 0, 6)));
    }

    #[test]
    fn stored_bricks_scanned_from_a_grid_are_those_of_the_grid() {
        use std::sync::Arc;
        use voxel_grid::{ChannelDefinition, GridLayout};
        let layout = Arc::new(
            GridLayout::new(vec![ChannelDefinition::number(
                "test:number".parse().unwrap(),
                0,
            )])
            .unwrap(),
        );
        let mut grid = Grid::new(layout);
        let channel = grid
            .layout()
            .channel_index(&"test:number".parse().unwrap())
            .unwrap();
        for brick in [
            BrickCoordinate::new(1, 2, 3),
            BrickCoordinate::new(-9, 0, 40),
        ] {
            let voxel = brick.origin();
            grid.set(channel, voxel, 7);
        }
        let stored = StoredBricks::scan(&grid);
        assert_eq!(stored.count, grid.brick_count());
        assert_eq!(stored.count, 2);
        assert!(stored.any_in(cube_at(0, 0, 0, 2)));
        assert!(stored.any_in(cube_at(-16, 0, 32, 4)));
        assert!(!stored.any_in(cube_at(16, 0, 0, 4)));
        assert!(
            !StoredBricks::scan(&Grid::new(Arc::clone(grid.layout())))
                .any_in(cube_at(0, 0, 0, 116))
        );
    }

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

    fn body_at(
        brick: (i128, i128, i128),
        offset: [f64; 3],
        orientation: Orientation,
        linear: [f64; 3],
        angular: [f64; 3],
    ) -> BodyState {
        BodyState {
            position: WorldPosition::new(BrickCoordinate::new(brick.0, brick.1, brick.2), offset),
            orientation,
            linear_velocity: linear,
            angular_velocity: angular,
        }
    }

    fn follows(shift_bricks: [i128; 3], shift_offset: [f64; 3], velocity: [f64; 3]) -> Follow {
        Follow {
            followed: Followed::Planet(GridId::new(GridId::FIRST_ALLOCATED + 1)),
            anchor: WorldPosition::new(BrickCoordinate::new(7, 8, 9), [1.0; 3]),
            shift: Displacement {
                bricks: shift_bricks,
                offset: shift_offset,
            },
            velocity,
        }
    }

    /// The turn of `angle` radians about the axis of `direction` (any length).
    fn turn_about(direction: [f64; 3], angle: f64) -> Orientation {
        let length = libm::sqrt(direction.iter().map(|part| part * part).sum::<f64>());
        let (sine, cosine) = (libm::sin(angle / 2.0), libm::cos(angle / 2.0));
        let [x, y, z] = direction.map(|part| part / length * sine);
        Orientation::from_components([x, y, z, cosine]).unwrap()
    }

    fn same_turn(first: Orientation, second: Orientation) -> bool {
        // (A quaternion and its negative are the same turn.)
        let (a, b) = (first.components(), second.components());
        let direct = (0..4).all(|index| (a[index] - b[index]).abs() < 1e-9);
        let opposite = (0..4).all(|index| (a[index] + b[index]).abs() < 1e-9);
        direct || opposite
    }

    #[test]
    fn a_body_at_the_end_of_a_tick_was_at_the_start_of_it_one_tick_of_its_velocity_before() {
        let state = body_at(
            (100, -3, 7),
            [4.0, 5.0, 6.0],
            Orientation::IDENTITY,
            [10.0, -20.0, 4.0],
            [0.0; 3],
        );
        let before = state_before(state, 0.05);
        // A twentieth of a second at (10, -20, 4): half a voxel back along x, a voxel forward along y, a fifth of a voxel back along z.
        let moved = before.position.displacement_to(state.position).in_voxels();
        assert!((moved[0] - 0.5).abs() < 1e-5, "{moved:?}");
        assert!((moved[1] + 1.0).abs() < 1e-5, "{moved:?}");
        assert!((moved[2] - 0.2).abs() < 1e-5, "{moved:?}");
        assert_eq!(before.linear_velocity, state.linear_velocity);
        assert_eq!(before.angular_velocity, state.angular_velocity);
        assert!(same_turn(before.orientation, state.orientation));
    }

    #[test]
    fn a_body_at_the_end_of_a_tick_was_turned_one_tick_of_its_spin_back_about_the_axis_of_it() {
        // Two radians a second about y for a twentieth of a second is a tenth of a radian: it was a tenth of a radian back about y.
        let state = body_at(
            (0, 0, 0),
            [0.0; 3],
            Orientation::IDENTITY,
            [0.0; 3],
            [0.0, 2.0, 0.0],
        );
        let before = state_before(state, 0.05);
        assert!(same_turn(
            before.orientation,
            turn_about([0.0, 1.0, 0.0], -0.1)
        ));
        assert_eq!(before.position, state.position);
        // Any axis and any orientation to begin with: a step of the same spin forward from where it was is where it is.
        let oblique = body_at(
            (0, 0, 0),
            [0.0; 3],
            turn_about([1.0, -2.0, 0.5], 1.1),
            [0.0; 3],
            [1.0, 2.0, 2.0],
        );
        let before = state_before(oblique, 0.05);
        let step = turn_about([1.0, 2.0, 2.0], 3.0 * 0.05);
        assert!(same_turn(
            step.compose(before.orientation),
            oblique.orientation
        ));
        assert!(!same_turn(before.orientation, oblique.orientation));
    }

    #[test]
    fn a_body_that_does_not_move_or_turn_was_where_it_is() {
        let state = body_at(
            (5, 6, 7),
            [1.0, 2.0, 3.0],
            turn_about([0.0, 1.0, 0.0], 0.7),
            [0.0; 3],
            [0.0; 3],
        );
        assert_eq!(state_before(state, 0.05), state);
    }

    #[test]
    fn places_and_velocities_in_a_frame_that_goes_with_a_planet_are_the_worlds_less_the_planets_and_come_back()
     {
        let planet = follows([2, -1, 0], [0.5, 3.0, 4.0], [100.0, 0.0, -50.0]);
        let world = WorldPosition::new(BrickCoordinate::new(10, 11, 12), [8.0, 9.0, 10.0]);
        let local = local_in(Some(planet), world).unwrap();
        // The planet has gone two bricks along x and a brick back along y, and 0.5, 3 and 4 voxels more.
        let moved = local.displacement_to(world).in_voxels();
        assert_eq!(moved, [2.0 * 32.0 + 0.5, -32.0 + 3.0, 4.0]);
        assert!(same_place(planet.world(local).unwrap(), world));
        assert_eq!(
            velocity_in(Some(planet), [130.0, 5.0, -50.0]),
            [30.0, 5.0, 0.0]
        );
        assert_eq!(
            velocity_from(Some(planet), [30.0, 5.0, 0.0]),
            [130.0, 5.0, -50.0]
        );
        // A frame that goes with nothing is the world.
        assert_eq!(local_in(None, world), Some(world));
        assert_eq!(velocity_in(None, [1.0, 2.0, 3.0]), [1.0, 2.0, 3.0]);
        assert_eq!(velocity_from(None, [1.0, 2.0, 3.0]), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn a_ship_that_is_held_or_frozen_or_on_a_rail_is_a_fixed_body_and_any_other_is_dynamic() {
        assert_eq!(body_kind(false, false, false), BodyKind::Dynamic);
        assert_eq!(body_kind(true, false, false), BodyKind::Fixed);
        assert_eq!(body_kind(false, true, false), BodyKind::Fixed);
        assert_eq!(body_kind(false, false, true), BodyKind::Fixed);
        assert_eq!(body_kind(true, true, true), BodyKind::Fixed);
    }

    #[test]
    fn two_places_are_the_same_within_a_thousandth_of_a_voxel_on_every_axis() {
        let at =
            |x: f64, y: f64, z: f64| WorldPosition::new(BrickCoordinate::new(3, 4, 5), [x, y, z]);
        let place = at(10.0, 10.0, 10.0);
        assert!(same_place(place, place));
        assert!(same_place(place, at(10.0009, 9.9991, 10.0)));
        for moved in [
            at(10.0011, 10.0, 10.0),
            at(10.0, 9.9989, 10.0),
            at(10.0, 10.0, 10.0011),
        ] {
            assert!(!same_place(place, moved));
            assert!(!same_place(moved, place));
        }
        // A brick apart is not the same place, however near the offsets.
        let next_brick = WorldPosition::new(BrickCoordinate::new(4, 4, 5), [10.0; 3]);
        assert!(!same_place(place, next_brick));
    }

    #[test]
    fn a_ship_that_is_held_has_no_speed_against_a_frame_that_goes_with_a_planet_and_in_one_that_does_not_it_has_the_worlds()
     {
        let planet = follows([0; 3], [0.0; 3], [100.0, 0.0, 0.0]);
        let pose = GridPose::new(
            WorldPosition::new(BrickCoordinate::new(1, 2, 3), [4.0, 5.0, 6.0]),
            Orientation::IDENTITY,
        );
        let (moving, turning) = ([100.0, 7.0, 0.0], [0.0, 0.5, 0.0]);
        let free = ship_state_in(Some(planet), BodyKind::Dynamic, pose, moving, turning).unwrap();
        assert_eq!(free.linear_velocity, [0.0, 7.0, 0.0]);
        assert_eq!(free.angular_velocity, turning);
        let held = ship_state_in(Some(planet), BodyKind::Fixed, pose, moving, turning).unwrap();
        assert_eq!(held.linear_velocity, [0.0; 3]);
        assert_eq!(held.position, free.position);
        // (In a frame that goes with nothing a held ship has the velocity it is given: a held ship has none, and the bodies leave it alone.)
        let still = ship_state_in(None, BodyKind::Fixed, pose, moving, turning).unwrap();
        assert_eq!(still.linear_velocity, moving);
    }

    /// **The rider is put at rest with its centre of mass at the point of its frame, every tick** (how the plugin does it: no line says "re-centre"; the world has the ship where the rail
    /// puts it, the frame has gone with the rail's point, and the ship's state in the frame is worked out from the two). The rail moves the point and turns its speed every tick, the
    /// facing changes with the turn, and still the body is given no speed at all (exactly: the speed of the ship less that of the point, the same numbers) and its centre of mass is the
    /// anchor, however far the point has gone.
    #[test]
    fn a_ship_that_turns_on_a_rail_is_given_rest_with_its_centre_of_mass_at_the_point_of_its_frame_whatever_the_rail_does()
     {
        let ship = GridId::new(GridId::FIRST_ALLOCATED + 1);
        let centre = [1.0, 1.5, 2.5];
        let spin = [0.2, 0.3, 0.1];
        let anchor = WorldPosition::new(BrickCoordinate::new(100, 2187, -3), [4.0, 5.0, 6.0]);
        let mut follow = Follow {
            followed: Followed::RailPoint(ship),
            anchor,
            shift: Displacement::NONE,
            velocity: [761.0, 0.0, 0.0],
        };
        for tick in 1..=5 {
            let tick = f64::from(tick);
            // The point went on 38 voxels a tick along x and bends down, and its speed turned.
            let point = anchor.translated([38.0 * tick, -0.01 * tick * tick, 0.5 * tick]);
            let velocity = [761.0 - 0.012 * tick, -0.41 * tick, 0.5 * tick];
            follow.shift = anchor.displacement_to(point);
            follow.velocity = velocity;
            let facing = turn_about(spin, 0.3 + 0.016 * tick);
            // The origin of the ship is the point less the centre of mass turned with the ship.
            let arm = GridPose::new(point, facing).direction_to_world(centre);
            let origin = point.translated(arm.map(|part| -part));
            let pose = GridPose::new(origin, facing);
            let state =
                ship_state_in(Some(follow), BodyKind::Dynamic, pose, velocity, spin).unwrap();
            assert_eq!(state.linear_velocity, [0.0; 3], "tick {tick}: at rest");
            assert_eq!(state.angular_velocity, spin, "tick {tick}: turning");
            assert!(same_turn(state.orientation, facing), "tick {tick}: facing");
            let found = GridPose::new(state.position, state.orientation)
                .local_to_world(WorldPosition::new(BrickCoordinate::ORIGIN, centre))
                .unwrap();
            let apart = found.displacement_to(anchor).in_voxels();
            assert!(
                apart.iter().all(|part| part.abs() < 1e-9),
                "tick {tick}: the centre of mass is {apart:?} from the anchor of the frame"
            );
        }
    }

    #[test]
    fn a_planet_that_stands_still_is_given_its_state_as_it_is_and_one_that_moves_its_state_a_tick_before()
     {
        let pose = GridPose::new(
            WorldPosition::new(BrickCoordinate::new(40, 0, -3), [8.0, 9.0, 10.0]),
            turn_about([0.0, 1.0, 0.0], 0.3),
        );
        let (linear, angular) = ([0.0, 0.0, -100.0], [0.0, 2.0, 0.0]);
        let standing = planet_state(None, false, pose, linear, angular, 0.05).unwrap();
        assert_eq!(standing.position, pose.position);
        assert_eq!(standing.orientation, pose.orientation);
        assert_eq!(
            (standing.linear_velocity, standing.angular_velocity),
            (linear, angular)
        );
        let moving = planet_state(None, true, pose, linear, angular, 0.05).unwrap();
        // Five voxels back along its way (a twentieth of a second at a hundred), and a tenth of a radian back about its axis; with the velocities it has.
        let back = moving.position.displacement_to(pose.position).in_voxels();
        assert!(
            (back[2] + 5.0).abs() < 1e-4 && back[0].abs() < 1e-6 && back[1].abs() < 1e-6,
            "{back:?}"
        );
        assert!(same_turn(
            moving.orientation,
            turn_about([0.0, 1.0, 0.0], 0.3 - 0.1)
        ));
        assert_eq!(
            (moving.linear_velocity, moving.angular_velocity),
            (linear, angular)
        );
    }

    #[test]
    fn a_planet_in_a_frame_that_goes_with_a_planet_is_given_its_place_and_velocity_there_and_a_tick_before()
     {
        // The frame has gone two bricks along x; the planet it goes with is this one, which goes at (100, 0, -50) in the world: against the frame it does not move.
        let frame = follows([2, 0, 0], [0.0; 3], [100.0, 0.0, -50.0]);
        let pose = GridPose::new(
            WorldPosition::new(BrickCoordinate::new(12, 0, 0), [8.0, 9.0, 10.0]),
            Orientation::IDENTITY,
        );
        let this =
            planet_state(Some(frame), true, pose, [100.0, 0.0, -50.0], [0.0; 3], 0.05).unwrap();
        assert_eq!(this.linear_velocity, [0.0; 3]);
        assert_eq!(
            this.position,
            WorldPosition::new(BrickCoordinate::new(10, 0, 0), [8.0, 9.0, 10.0])
        );
        // Another planet that stands still in the world goes against the frame at minus the frame's velocity, (-100, 0, 50), and was a tick of that before: it is five
        // voxels along x and two and a half back along z of where it was put, so the place it was put is (-5, 0, 2.5) from it.
        let other = planet_state(Some(frame), true, pose, [0.0; 3], [0.0; 3], 0.05).unwrap();
        assert_eq!(other.linear_velocity, [-100.0, 0.0, 50.0]);
        let back = other.position.displacement_to(this.position).in_voxels();
        assert!(
            (back[0] + 5.0).abs() < 1e-4 && back[1].abs() < 1e-6 && (back[2] - 2.5).abs() < 1e-4,
            "{back:?}"
        );
    }

    #[test]
    fn a_planet_the_world_cannot_place_in_a_frame_is_given_no_state() {
        use voxel_space::BRICK_COORDINATE_MAXIMUM;
        let frame = follows([-1, 0, 0], [0.0; 3], [0.0; 3]);
        let last = GridPose::new(
            WorldPosition::new(
                BrickCoordinate::new(BRICK_COORDINATE_MAXIMUM, 0, 0),
                [0.0; 3],
            ),
            Orientation::IDENTITY,
        );
        assert!(planet_state(Some(frame), false, last, [0.0; 3], [0.0; 3], 0.05).is_none());
        assert!(planet_state(None, false, last, [0.0; 3], [0.0; 3], 0.05).is_some());
    }

    // ---- A frame that moves itself (a group off a rail that is alone) ------------------------------------------------------------------------------------------------------

    fn voxels_of(shift: Displacement) -> [f64; 3] {
        shift.in_voxels()
    }

    #[test]
    fn a_shift_moved_by_a_vector_is_the_shift_and_the_vector_and_keeps_its_rest_inside_a_brick() {
        let start = Displacement {
            bricks: [3, -2, 0],
            offset: [30.0, 1.0, 16.0],
        };
        let moved = shifted_by(start, [5.0, -33.0, 100.5]);
        // The same place: 3 bricks and 30 + 5 = 35 is 4 bricks and 3; -2 bricks and 1 - 33 = -32 is -3 bricks and 0; 16 + 100.5 = 116.5 is 3 bricks and 20.5.
        assert_eq!(moved.bricks, [4, -3, 3]);
        assert_eq!(moved.offset, [3.0, 0.0, 20.5]);
        assert!(moved.offset.iter().all(|part| (0.0..32.0).contains(part)));
        for axis in 0..3 {
            let before = voxels_of(start)[axis];
            let after = voxels_of(moved)[axis];
            assert!(
                (after - before - [5.0, -33.0, 100.5][axis]).abs() < 1e-12,
                "axis {axis}"
            );
        }
        // Nothing to add is the same shift.
        assert_eq!(shifted_by(start, [0.0; 3]), start);
    }

    #[test]
    fn a_shift_that_has_gone_far_is_still_exact_to_the_rounding_of_a_brick() {
        // A million steps of 1.7 voxels are 1.7 million voxels; added to one number the last of them would have lost digits at each step, and as bricks and a rest they have not.
        let mut shift = Displacement::NONE;
        for _ in 0..1_000_000 {
            shift = shifted_by(shift, [1.7, -0.3, 0.0]);
        }
        let expected = [1_700_000.0, -300_000.0, 0.0];
        for (axis, expected) in expected.iter().enumerate() {
            let found = shift.bricks[axis] as f64 * 32.0 + shift.offset[axis];
            assert!(
                (found - expected).abs() < 1e-4,
                "axis {axis}: {found} against {expected}"
            );
            assert!((0.0..32.0).contains(&shift.offset[axis]));
        }
    }

    #[test]
    fn a_frame_that_moves_itself_goes_at_its_velocity_for_a_tick_and_keeps_it() {
        let (shift, velocity) = advanced(
            Displacement::NONE,
            [100.0, 0.0, -50.0],
            0.05,
            [0.0; 3],
            [0.0; 3],
            [0.0; 3],
        );
        assert_eq!(voxels_of(shift), [5.0, 0.0, -2.5]);
        assert_eq!(velocity, [100.0, 0.0, -50.0]);
    }

    #[test]
    fn the_pull_of_a_tick_speeds_the_frame_and_moves_it_as_a_semi_implicit_step_does() {
        // A pull of 5 for a tick of 0.05: the speed is 0.25 more, and the frame has gone its speed and the pull's part of a tick: v dt + g dt^2.
        let (shift, velocity) = advanced(
            Displacement::NONE,
            [0.0, 10.0, 0.0],
            0.05,
            [0.0, -5.0, 0.0],
            [0.0; 3],
            [0.0; 3],
        );
        assert_eq!(velocity, [0.0, 9.75, 0.0]);
        // The same as the world works it out: the speed after the pull, over the tick.
        assert!((voxels_of(shift)[1] - 9.75 * 0.05).abs() < 1e-12);
    }

    #[test]
    fn what_the_bodies_did_to_the_group_as_a_whole_is_added_to_where_it_has_gone_and_to_how_fast() {
        // The bodies moved the centre of mass 0.01 and gave it 0.2 against the frame; the frame went at 100 and no pull.
        let (shift, velocity) = advanced(
            Displacement::NONE,
            [100.0, 0.0, 0.0],
            0.05,
            [0.0; 3],
            [0.01, 0.0, 0.0],
            [0.2, 0.0, 0.0],
        );
        assert!((voxels_of(shift)[0] - (5.0 + 0.01)).abs() < 1e-12);
        assert!((velocity[0] - 100.2).abs() < 1e-12);
        // A push that the bodies give as a semi-implicit step gives (the speed it gains over the tick, and that over the tick as the move) takes the group where a body that was
        // pushed in the world would be: the world's own step is the new speed over the tick, from the old place.
        let gained = 0.2;
        let (shift, velocity) = advanced(
            Displacement::NONE,
            [100.0, 0.0, 0.0],
            0.05,
            [0.0; 3],
            [gained * 0.05, 0.0, 0.0],
            [gained, 0.0, 0.0],
        );
        assert!((voxels_of(shift)[0] - velocity[0] * 0.05).abs() < 1e-12);
    }

    #[test]
    fn only_a_group_that_is_alone_has_a_frame_that_moves_itself() {
        assert!(Followed::Group(GridId::new(7)).moves_itself());
        assert!(!Followed::RailGroup(GridId::new(7)).moves_itself());
        assert!(!Followed::RailPoint(GridId::new(7)).moves_itself());
        assert!(!Followed::Planet(GridId::new(7)).moves_itself());
        // Nothing pulls on what is in it: it falls with the group.
        let world_free = Followed::Group(GridId::new(7));
        assert!(!world_free.has_planets());
    }
}
