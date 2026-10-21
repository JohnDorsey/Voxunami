use std::collections::{BTreeMap, BTreeSet, HashMap};

use rapier3d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
use rapier3d::glamx::Mat3;
use rapier3d::prelude::*;
use voxel_space::{
    BRICK_EDGE_LENGTH, BrickCoordinate, GridId, GridPose, VoxelCoordinate, WorldPosition,
};

use crate::terrain::{Collision, CollisionSource, build_brick};

/// How far, in bricks, from its frame's origin a character may get before the frame is rebuilt
/// around it. `f32` is exact enough far beyond this; the limit only keeps every number small.
const FRAME_RADIUS_BRICKS: i64 = 6;

/// How many bricks around the one a character is in have colliders: one in every direction is
/// enough to move well inside the middle one, since a body moves a small fraction of a brick a step.
const TERRAIN_RADIUS_BRICKS: i64 = 1;

/// The same for the bricks of a grid that is not the one the character's frame belongs to, counted in that grid's bricks.
const FOREIGN_RADIUS_BRICKS: i64 = 1;

/// How far, in voxels, a character that is put inside terrain is moved at most to find room, and how far
/// at a time. An eighth is the finest a block's shape goes, so no ledge is stepped over. Two bricks is far
/// enough for a character put some way into a hill, and keeps the search, which may have terrain
/// generated for it, small.
const MAXIMUM_CLEARANCE_DISTANCE: f32 = 64.0;
const CLEARANCE_STEP: f32 = 0.125;

/// How far, in voxels, a character that something has moved into is pushed at most in a tick to find room (see
/// [`CharacterBody::keep_clear`]): more than a ship that moves a few voxels a tick goes into it. Less than the distance for a character
/// that is put somewhere, because this search can run every tick for a character that is stuck, and the character is not lost if it fails.
const MAXIMUM_PUSH_DISTANCE: f32 = 8.0;

/// The ways a character put inside terrain is moved, in the order tried at one distance: up first (onto
/// the ground it was put into), then down, then sideways.
const CLEARANCE_DIRECTIONS: [[f32; 3]; 6] = [
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
];

/// How much smaller than the character the box is that is tested against the terrain when it is put
/// somewhere: resting on the ground, or against a wall, with the controller's small gap, is not inside it.
const CLEARANCE_MARGIN: f32 = 0.02;

/// A pull of gravity weaker than this (voxels per second per second) is none: the direction of nothing is not a direction.
const LEAST_PULL: f32 = 1.0e-6;

/// How close to straight opposite two directions of up are for the turn from one to the other to be a half turn about the character's
/// right, which is the one turn between them that the shortest way does not name.
const OPPOSITE: f32 = -0.999_999;

/// What a character is like. Lengths are in voxels (a voxel is a metre in Voxunami Cave) and times
/// in seconds. The defaults are close to those of the games that taught people what walking in a
/// block world feels like: a player of 0.6 by 1.8 that can jump a block and a quarter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CharacterSettings {
    /// Half the width of the character.
    pub radius: f32,
    /// Standing height.
    pub height: f32,
    /// How high above the feet the eyes are.
    pub eye_height: f32,
    /// Downward acceleration while walking.
    pub gravity: f32,
    /// The upward speed of a jump: with the gravity, the height of a jump is `jump_speed^2 / (2 *
    /// gravity)`.
    pub jump_speed: f32,
    pub walk_speed: f32,
    pub sprint_speed: f32,
    pub fly_speed: f32,
    pub fly_sprint_speed: f32,
    /// The fastest a falling character goes.
    pub terminal_speed: f32,
    /// How fast the horizontal speed follows what the player asks for, on the ground and in the air.
    pub ground_acceleration: f32,
    pub air_acceleration: f32,
    /// The highest step a walking character climbs without jumping: slabs and the steps of stairs.
    pub step_height: f32,
    /// The time one step of [`CharacterBody::step`] stands for.
    pub time_step: f32,
}

impl Default for CharacterSettings {
    fn default() -> Self {
        Self {
            radius: 0.3,
            height: 1.8,
            eye_height: 1.62,
            gravity: 32.0,
            jump_speed: 9.0,
            walk_speed: 4.3,
            sprint_speed: 5.6,
            fly_speed: 10.9,
            fly_sprint_speed: 21.6,
            terminal_speed: 78.0,
            ground_acceleration: 80.0,
            air_acceleration: 14.0,
            step_height: 0.6,
            time_step: 1.0 / 60.0,
        }
    }
}

/// What a player asks of a character for a step: the keys held, and which way they face.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MoveInput {
    /// 1 to walk the way the player faces, -1 for backwards, 0 for neither.
    pub forward: i8,
    /// 1 to walk to the right, -1 to the left.
    pub strafe: i8,
    /// While flying: 1 to go up, -1 to go down.
    pub vertical: i8,
    /// Jump, if on the ground.
    pub jump: bool,
    pub sprint: bool,
    /// Which way the player faces, in radians, as the camera's yaw: 0 is toward -z and positive
    /// turns toward -x.
    pub yaw: f32,
}

/// A grid other than the one a character's frame belongs to, as the character meets it: a ship, to a character in a level (or the level, to
/// a character aboard a ship). Its blocks near the character are colliders that stand where the grid is and turn as it does, so the
/// character walks on it and is stopped by it.
pub struct ForeignGrid<'a> {
    /// Which grid it is. Colliders are kept for a grid from one call to the next by this.
    pub id: GridId,
    /// Where the grid is, in the coordinates of the grid the character's frame belongs to.
    pub pose: GridPose,
    /// What its blocks are, in its own coordinates.
    pub source: &'a mut dyn CollisionSource,
}

impl std::fmt::Debug for ForeignGrid<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ForeignGrid")
            .field("id", &self.id)
            .field("pose", &self.pose)
            .finish_non_exhaustive()
    }
}

/// The collider of one brick of a foreign grid, and the pose it was last put at (as the bits of its numbers).
struct ForeignBrick {
    collider: Option<ColliderHandle>,
    placed: Option<[u32; 7]>,
}

/// A body that is moved with the player's input through the terrain: it falls, walks, jumps and
/// slides along what it runs into, or flies. It owns the Rapier world its terrain is in.
pub struct CharacterBody {
    settings: CharacterSettings,
    /// The brick the local frame's origin is the corner of.
    origin: BrickCoordinate,
    /// Where the middle of the body is, in the local frame.
    center: Vector,
    velocity: Vector,
    grounded: bool,
    flying: bool,
    /// Which way is up in the frame, a unit vector. The frame is a level's, or a ship's, and gravity is whatever the world's field is
    /// there, so up is not always `+y`.
    up: Vector,
    /// The direction the character faces at yaw 0: a unit vector across `up`. It is carried along as `up` turns, by the shortest
    /// turn each time, so a character in a room that tips keeps facing the way a person would, and is not set upright again.
    heading: Vector,
    /// The turn that takes the box of the character from standing in `+y` to standing in `up`. `None` for `up` straight `+y`, which is
    /// the plain case and does no arithmetic at all.
    shape_turn: Option<Rotation>,
    controller: KinematicCharacterController,
    shape: Cuboid,
    pipeline: PhysicsPipeline,
    parameters: IntegrationParameters,
    islands: IslandManager,
    broad_phase: BroadPhaseBvh,
    narrow_phase: NarrowPhase,
    bodies: RigidBodySet,
    colliders: ColliderSet,
    impulse_joints: ImpulseJointSet,
    multibody_joints: MultibodyJointSet,
    soft_bodies: SoftBodySet,
    ccd: CCDSolver,
    /// The colliders of each brick that has any, in the order they were made.
    loaded: HashMap<BrickCoordinate, Vec<ColliderHandle>>,
    /// The colliders of the bricks of foreign grids near the character, which are put where their grids are each step.
    foreign: BTreeMap<(GridId, BrickCoordinate), ForeignBrick>,
    /// The character was put somewhere (made, or teleported) and has not been checked yet for being
    /// inside the terrain; the next [`CharacterBody::update_terrain`] lifts it out if it is.
    needs_clearance: bool,
}

impl std::fmt::Debug for CharacterBody {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CharacterBody")
            .field("position", &self.eye_position())
            .field("velocity", &self.velocity())
            .field("grounded", &self.grounded)
            .field("flying", &self.flying)
            .finish_non_exhaustive()
    }
}

impl CharacterBody {
    /// A character whose eyes are at `eye`, falling from rest. It has no terrain until
    /// [`CharacterBody::update_terrain`] makes some.
    pub fn new(eye: WorldPosition, settings: CharacterSettings) -> Self {
        let parameters = IntegrationParameters {
            dt: settings.time_step,
            ..IntegrationParameters::default()
        };
        let mut body = Self {
            settings,
            origin: eye.brick(),
            center: Vector::ZERO,
            velocity: Vector::ZERO,
            grounded: false,
            flying: false,
            up: Vector::Y,
            heading: Vector::NEG_Z,
            shape_turn: None,
            controller: KinematicCharacterController {
                up: Vector::Y,
                offset: CharacterLength::Absolute(0.01),
                slide: true,
                autostep: Some(CharacterAutostep {
                    max_height: CharacterLength::Absolute(settings.step_height),
                    min_width: CharacterLength::Absolute(settings.radius * 0.5),
                    include_dynamic_bodies: false,
                }),
                max_slope_climb_angle: 50.0_f32.to_radians(),
                min_slope_slide_angle: 50.0_f32.to_radians(),
                snap_to_ground: None,
                normal_nudge_factor: 1.0e-5,
            },
            shape: Cuboid::new(Vector::new(
                settings.radius,
                settings.height / 2.0,
                settings.radius,
            )),
            pipeline: PhysicsPipeline::new(),
            parameters,
            islands: IslandManager::new(),
            broad_phase: BroadPhaseBvh::new(),
            narrow_phase: NarrowPhase::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            impulse_joints: ImpulseJointSet::new(),
            multibody_joints: MultibodyJointSet::new(),
            soft_bodies: SoftBodySet::new(),
            ccd: CCDSolver::new(),
            loaded: HashMap::new(),
            foreign: BTreeMap::new(),
            needs_clearance: true,
        };
        body.place_eyes(eye);
        body
    }

    pub fn settings(&self) -> &CharacterSettings {
        &self.settings
    }

    /// Where the eyes are.
    pub fn eye_position(&self) -> WorldPosition {
        let eye = self.center + self.up * (self.settings.eye_height - self.settings.height / 2.0);
        self.world_position(eye)
    }

    /// Where the middle of the body is: half its height from the feet, along the character's up.
    pub fn middle_position(&self) -> WorldPosition {
        self.world_position(self.center)
    }

    /// Where the feet are: the length of the body below the middle of it, along the character's up.
    pub fn feet_position(&self) -> WorldPosition {
        self.world_position(self.center - self.up * (self.settings.height / 2.0))
    }

    /// Which way is up for the character, a unit vector in the axes of its frame, as of its last step.
    pub fn up(&self) -> [f32; 3] {
        self.up.to_array()
    }

    /// The turn of the character's frame, as the quaternion `[x, y, z, w]` that takes the axes a character has when it stands in a
    /// plain level (right `+x`, up `+y`, back `+z`, facing `-z` at yaw 0) to its own in the axes of the grid. The identity until gravity
    /// has turned. A player's view is this turn and then the angles they give.
    pub fn frame_rotation(&self) -> [f32; 4] {
        if self.is_level() {
            return [0.0, 0.0, 0.0, 1.0];
        }
        let right = self.heading.cross(self.up);
        let back = -self.heading;
        Rotation::from_mat3(&Mat3::from_cols(right, self.up, back))
            .normalize()
            .to_array()
    }

    /// Whether the frame is the plain one: up is `+y` and the character faces `-z` at yaw 0. Then nothing about the character is
    /// computed differently from how a character in a world that is not turned has always been.
    fn is_level(&self) -> bool {
        self.up == Vector::Y && self.heading == Vector::NEG_Z
    }

    /// Moves the character to another frame, with no break in its motion: its eyes at `eye` and its velocity as `velocity` in the axes of
    /// that frame, and the frame turned as `frame_rotation` says (see [`CharacterBody::frame_rotation`]). Whether it stands on the
    /// ground, and whether it flies, are as they were: a character carried from a level to a ship it stands on is still standing. The
    /// terrain is the new frame's; the next [`CharacterBody::update_terrain`] makes it, and lifts the character out of it if it was put
    /// inside.
    pub fn reframe(&mut self, eye: WorldPosition, velocity: [f32; 3], frame_rotation: [f32; 4]) {
        self.clear_terrain();
        self.needs_clearance = true;
        self.origin = eye.brick();
        self.velocity = Vector::from_array(velocity);
        if frame_rotation == [0.0, 0.0, 0.0, 1.0] {
            self.up = Vector::Y;
            self.heading = Vector::NEG_Z;
        } else {
            let [x, y, z, w] = frame_rotation;
            let turn = Rotation::from_xyzw(x, y, z, w).normalize();
            self.up = turn * Vector::Y;
            self.heading = turn * Vector::NEG_Z;
        }
        self.set_shape_turn();
        self.place_eyes(eye);
    }

    /// Makes the box and the controller stand in the current `up`.
    fn set_shape_turn(&mut self) {
        self.controller.up = self.up;
        self.shape_turn = if self.up == Vector::Y {
            None
        } else if self.up.y < OPPOSITE {
            // Upside down: the turn the shortest way does not name is a half turn about x.
            Some(Rotation::from_xyzw(1.0, 0.0, 0.0, 0.0))
        } else {
            Some(Rotation::from_rotation_arc(Vector::Y, self.up))
        };
    }

    /// Turns the frame so that up is `new_up`, carrying the way the character faces along by the shortest turn between the two ups
    /// (about the character's right if they are opposite).
    fn turn_up_to(&mut self, new_up: Vector) {
        if new_up == self.up {
            return;
        }
        let old = self.up;
        let right = self.heading.cross(old);
        let carried = turned_between(self.heading, old, new_up, right);
        // Whatever drift the turns leave is taken out: across the new up, and of length one.
        let across = carried - new_up * carried.dot(new_up);
        self.heading = across.normalize();
        self.up = new_up;
        self.set_shape_turn();
    }

    /// The brick whose corner is the origin of the frame the character's terrain is in. It is rebuilt
    /// around the character when the character has gone some bricks from it.
    pub fn frame_origin(&self) -> BrickCoordinate {
        self.origin
    }

    /// The velocity, in voxels a second.
    pub fn velocity(&self) -> [f32; 3] {
        self.velocity.to_array()
    }

    /// Whether the character stands on something.
    pub fn is_grounded(&self) -> bool {
        self.grounded
    }

    pub fn is_flying(&self) -> bool {
        self.flying
    }

    /// Starts or stops flying: no gravity, and the player controls the height.
    pub fn set_flying(&mut self, flying: bool) {
        if self.flying != flying {
            self.flying = flying;
            self.velocity = Vector::ZERO;
        }
    }

    /// Puts the character with its eyes at a position, at rest and in the air. If that is inside the
    /// terrain, the next [`CharacterBody::update_terrain`] lifts it straight up onto the surface above.
    pub fn teleport(&mut self, eye: WorldPosition) {
        self.needs_clearance = true;
        self.clear_terrain();
        self.origin = eye.brick();
        self.velocity = Vector::ZERO;
        self.grounded = false;
        self.place_eyes(eye);
    }

    fn place_eyes(&mut self, eye: WorldPosition) {
        let local = self.local_position(eye);
        self.center = local - self.up * (self.settings.eye_height - self.settings.height / 2.0);
    }

    fn local_position(&self, position: WorldPosition) -> Vector {
        let brick = position.brick();
        let edge = BRICK_EDGE_LENGTH as i64;
        let offset = position.offset();
        Vector::new(
            ((brick.x() - self.origin.x()) * edge) as f32 + offset[0],
            ((brick.y() - self.origin.y()) * edge) as f32 + offset[1],
            ((brick.z() - self.origin.z()) * edge) as f32 + offset[2],
        )
    }

    fn world_position(&self, local: Vector) -> WorldPosition {
        WorldPosition::new(self.origin, local.to_array())
    }

    fn brick_of_center(&self) -> BrickCoordinate {
        self.world_position(self.center).brick()
    }

    fn clear_terrain(&mut self) {
        let loaded = std::mem::take(&mut self.loaded);
        for (_, handles) in loaded {
            for handle in handles {
                self.remove_collider(handle);
            }
        }
        // Their places were in the frame that is gone: they are made again around the new origin.
        let foreign = std::mem::take(&mut self.foreign);
        for (_, brick) in foreign {
            if let Some(handle) = brick.collider {
                self.remove_collider(handle);
            }
        }
    }

    fn remove_collider(&mut self, handle: ColliderHandle) {
        self.colliders.remove(
            handle,
            &mut self.islands,
            &mut self.bodies,
            &mut self.soft_bodies,
            false,
        );
    }

    /// Makes the colliders of the terrain around the character that are not made yet, and drops the
    /// ones that are far behind it. Call it before [`CharacterBody::step`] each tick; it does
    /// nothing, quickly, when the character has not moved to another brick.
    ///
    /// A character that was just put somewhere ([`CharacterBody::new`], [`CharacterBody::teleport`]) and
    /// is inside the terrain is first moved the shortest way out of it (straight up, down or sideways: the
    /// least distance, and up before the others at the same distance), so that it does not stay half in a
    /// block, or fall through the ground, as the physics alone would leave it. If there is no room within
    /// [`MAXIMUM_CLEARANCE_DISTANCE`] it stays where it was put.
    pub fn update_terrain(&mut self, source: &mut impl CollisionSource) {
        if std::mem::take(&mut self.needs_clearance) {
            self.move_out(source, false, MAXIMUM_CLEARANCE_DISTANCE);
        }
        self.load_terrain_around_character(source);
    }

    /// Moves the character out of what something has moved into it since the last tick, the shortest way out (up before the others at the
    /// same distance): a block built where it stands, or a ship that sailed into it or fell onto it, whose colliders
    /// [`CharacterBody::update_foreign`] has just put where the ship is. Call it each tick after that and
    /// [`CharacterBody::update_terrain`], before stepping, and it does nothing, quickly, when the character is in the open.
    ///
    /// The step alone does not do this well. A controller slides a character out of an overlap along the surface of whichever collider it
    /// finds first, with no regard to what is behind the character, so something that comes down on a character presses it into the floor
    /// (and the floor pushes it back, so it shakes), and the colliders of terrain leave out boxes buried in solid terrain, so a character
    /// pressed deep enough touches nothing and goes on down through it. If there is no room within [`MAXIMUM_PUSH_DISTANCE`] the
    /// character stays where it is.
    pub fn keep_clear(&mut self, source: &mut impl CollisionSource) {
        self.move_out(source, true, MAXIMUM_PUSH_DISTANCE);
    }

    /// Moves the character the shortest way out of the terrain, and of the blocks of other grids if `among_other_grids`, within `limit`.
    fn move_out(&mut self, source: &mut impl CollisionSource, among_other_grids: bool, limit: f32) {
        let put_at = self.center;
        if !self.is_inside(put_at, source, among_other_grids) {
            return;
        }
        let mut distance = 0.0;
        while distance < limit {
            distance += CLEARANCE_STEP;
            for direction in CLEARANCE_DIRECTIONS {
                let direction = Vector::from_array(direction);
                let direction = match self.shape_turn {
                    None => direction,
                    Some(turn) => turn * direction,
                };
                let candidate = put_at + direction * distance;
                if !self.is_inside(candidate, source, among_other_grids) {
                    self.center = candidate;
                    return;
                }
            }
        }
        // No room near: where it was put is as good as anywhere (the center was never changed).
    }

    /// Whether the character with its middle at `center` is inside the terrain, or inside a block of another grid if `among_other_grids`.
    fn is_inside(
        &self,
        center: Vector,
        source: &mut impl CollisionSource,
        among_other_grids: bool,
    ) -> bool {
        self.overlaps_terrain(center, source)
            || (among_other_grids && self.overlaps_foreign(center))
    }

    /// Whether the character's box, a little smaller than it is (as for the terrain), with its middle at `center`, reaches into a collider
    /// of a block of another grid, as the grid stands now.
    fn overlaps_foreign(&self, center: Vector) -> bool {
        if self.foreign.values().all(|brick| brick.collider.is_none()) {
            return false;
        }
        let shrunk = Cuboid::new(self.shape.half_extents - Vector::splat(CLEARANCE_MARGIN));
        let pose = match self.shape_turn {
            None => Pose::from_translation(center),
            Some(turn) => Pose::from_parts(center, turn),
        };
        let query = self.broad_phase.as_query_pipeline(
            self.narrow_phase.query_dispatcher(),
            &self.bodies,
            &self.colliders,
            QueryFilter::default(),
        );
        query.intersect_shape(pose, &shrunk).any(|(handle, _)| {
            self.foreign
                .values()
                .any(|brick| brick.collider == Some(handle))
        })
    }

    /// Whether the character's box, a little smaller than it is, with its middle at `center` (in the local
    /// frame), reaches into the terrain. Asks the source, voxel by voxel: the colliders leave out boxes
    /// buried inside solid terrain, so a character deep in the ground touches none of them. A place the
    /// world does not have counts as solid, as the source's contract says.
    fn overlaps_terrain(&self, center: Vector, source: &mut impl CollisionSource) -> bool {
        let own = [
            f64::from(self.settings.radius - CLEARANCE_MARGIN),
            f64::from(self.settings.height / 2.0 - CLEARANCE_MARGIN),
            f64::from(self.settings.radius - CLEARANCE_MARGIN),
        ];
        // A character turned over is held by the box along the axes of the frame that reaches as far each way as the turned one does:
        // exact for a frame turned by quarter turns, and a little more than needed for any other.
        let half = match self.shape_turn {
            None => own,
            Some(turn) => {
                let matrix = Mat3::from_quat(turn);
                [0, 1, 2].map(|axis| {
                    (0..3)
                        .map(|column| f64::from(matrix.col(column)[axis].abs()) * own[column])
                        .sum()
                })
            }
        };
        let edge = BRICK_EDGE_LENGTH as i64;
        let origin = [self.origin.x(), self.origin.y(), self.origin.z()].map(|brick| brick * edge);
        let center = center.to_array().map(f64::from);
        let low = [0, 1, 2].map(|axis| (center[axis] - half[axis]).floor() as i64);
        let high = [0, 1, 2].map(|axis| (center[axis] + half[axis]).floor() as i64);
        for x in low[0]..=high[0] {
            for y in low[1]..=high[1] {
                for z in low[2]..=high[2] {
                    let Ok(voxel) = VoxelCoordinate::try_new(
                        origin[0].saturating_add(x),
                        origin[1].saturating_add(y),
                        origin[2].saturating_add(z),
                    ) else {
                        return true;
                    };
                    let cell = [x as f64, y as f64, z as f64];
                    let reaches_in = |min: [f64; 3], max: [f64; 3]| {
                        (0..3).all(|axis| {
                            center[axis] + half[axis] > cell[axis] + min[axis]
                                && center[axis] - half[axis] < cell[axis] + max[axis]
                        })
                    };
                    let inside = match source.collision_at(voxel) {
                        Collision::Empty => false,
                        Collision::Full => reaches_in([0.0; 3], [1.0; 3]),
                        Collision::Partial(parts) => parts.iter().any(|part| {
                            reaches_in(
                                part.min.map(|value| f64::from(value) / 8.0),
                                part.max.map(|value| f64::from(value) / 8.0),
                            )
                        }),
                    };
                    if inside {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn load_terrain_around_character(&mut self, source: &mut impl CollisionSource) {
        let here = self.brick_of_center();
        // Far from the origin of the frame: build a new frame around the character.
        if (here.x() - self.origin.x()).abs() > FRAME_RADIUS_BRICKS
            || (here.y() - self.origin.y()).abs() > FRAME_RADIUS_BRICKS
            || (here.z() - self.origin.z()).abs() > FRAME_RADIUS_BRICKS
        {
            let world = self.world_position(self.center);
            self.clear_terrain();
            self.origin = here;
            self.center = Vector::new(world.offset()[0], world.offset()[1], world.offset()[2]);
        }
        let wanted: Vec<BrickCoordinate> = (-TERRAIN_RADIUS_BRICKS..=TERRAIN_RADIUS_BRICKS)
            .flat_map(|dx| {
                (-TERRAIN_RADIUS_BRICKS..=TERRAIN_RADIUS_BRICKS).flat_map(move |dy| {
                    (-TERRAIN_RADIUS_BRICKS..=TERRAIN_RADIUS_BRICKS).map(move |dz| (dx, dy, dz))
                })
            })
            .map(|(dx, dy, dz)| BrickCoordinate::new(here.x() + dx, here.y() + dy, here.z() + dz))
            .collect();
        let far: Vec<BrickCoordinate> = self
            .loaded
            .keys()
            .copied()
            .filter(|brick| {
                (brick.x() - here.x()).abs() > TERRAIN_RADIUS_BRICKS + 1
                    || (brick.y() - here.y()).abs() > TERRAIN_RADIUS_BRICKS + 1
                    || (brick.z() - here.z()).abs() > TERRAIN_RADIUS_BRICKS + 1
            })
            .collect();
        for brick in far {
            self.unload(brick);
        }
        let mut changed = false;
        for brick in wanted {
            if !self.loaded.contains_key(&brick) {
                self.load(source, brick);
                changed = true;
            }
        }
        if changed {
            self.refresh_queries();
        }
    }

    /// Makes the colliders of a brick again, from what the source says now: the terrain in it was
    /// edited. Nothing happens if the brick has no colliders (it is not near the character).
    pub fn brick_changed(&mut self, source: &mut impl CollisionSource, brick: BrickCoordinate) {
        if self.loaded.contains_key(&brick) {
            self.unload(brick);
            self.load(source, brick);
            self.refresh_queries();
        }
    }

    /// Keeps the colliders of the blocks of other grids that are near the character, and puts them where their grids are now: call it
    /// each tick after [`CharacterBody::update_terrain`], with every grid that might be in reach (the others it is not asked about).
    /// A grid that is not in `grids` any more has none left, and one that is far from the character has no more than the asking of
    /// [`CollisionSource::brick_hint`] about the bricks that would be near it.
    pub fn update_foreign(&mut self, grids: &mut [ForeignGrid<'_>]) {
        let present: BTreeSet<GridId> = grids.iter().map(|grid| grid.id).collect();
        let mut changed = false;
        let gone: Vec<(GridId, BrickCoordinate)> = self
            .foreign
            .keys()
            .filter(|(grid, _)| !present.contains(grid))
            .copied()
            .collect();
        for key in gone {
            changed |= self.drop_foreign(key);
        }
        let here = self.world_position(self.center);
        for grid in grids.iter_mut() {
            // Where the character is in the grid, as a brick of it.
            let Some(there) = grid.pose.world_to_local(here).map(|place| place.brick()) else {
                continue;
            };
            let near = |brick: &BrickCoordinate, radius: i64| {
                (brick.x() - there.x()).abs() <= radius
                    && (brick.y() - there.y()).abs() <= radius
                    && (brick.z() - there.z()).abs() <= radius
            };
            let far: Vec<(GridId, BrickCoordinate)> = self
                .foreign
                .keys()
                .filter(|(id, brick)| *id == grid.id && !near(brick, FOREIGN_RADIUS_BRICKS + 1))
                .copied()
                .collect();
            for key in far {
                changed |= self.drop_foreign(key);
            }
            for dx in -FOREIGN_RADIUS_BRICKS..=FOREIGN_RADIUS_BRICKS {
                for dy in -FOREIGN_RADIUS_BRICKS..=FOREIGN_RADIUS_BRICKS {
                    for dz in -FOREIGN_RADIUS_BRICKS..=FOREIGN_RADIUS_BRICKS {
                        let Some(brick) = there.checked_offset(dx, dy, dz) else {
                            continue;
                        };
                        if !self.foreign.contains_key(&(grid.id, brick)) {
                            self.load_foreign(grid, brick);
                            changed = true;
                        }
                    }
                }
            }
            changed |= self.place_foreign(grid);
        }
        if changed {
            self.refresh_queries();
        }
    }

    /// Makes the colliders of a brick of a foreign grid again, from what its source says now: the grid was edited. Nothing happens if the
    /// brick has no colliders (it is not near the character).
    pub fn foreign_brick_changed(&mut self, grid: &mut ForeignGrid<'_>, brick: BrickCoordinate) {
        if self.foreign.contains_key(&(grid.id, brick)) {
            self.drop_foreign((grid.id, brick));
            self.load_foreign(grid, brick);
            self.place_foreign(grid);
            self.refresh_queries();
        }
    }

    /// Makes the collider of a brick of a foreign grid, at the place of the corner of the grid until it is put where it belongs.
    fn load_foreign(&mut self, grid: &mut ForeignGrid<'_>, brick: BrickCoordinate) {
        // The brick is its own origin, so the boxes are in its corner's frame, and the collider is put where the grid has the brick.
        let made = build_brick(&mut *grid.source, brick, brick);
        let collider = made.terrain.map(|collider| self.colliders.insert(collider));
        self.foreign.insert(
            (grid.id, brick),
            ForeignBrick {
                collider,
                placed: None,
            },
        );
    }

    /// Takes away the collider of a brick of a foreign grid. Returns whether there was one to take away.
    fn drop_foreign(&mut self, key: (GridId, BrickCoordinate)) -> bool {
        let Some(brick) = self.foreign.remove(&key) else {
            return false;
        };
        match brick.collider {
            Some(handle) => {
                self.remove_collider(handle);
                true
            }
            None => false,
        }
    }

    /// Puts the colliders of a foreign grid where the grid is. Returns whether any moved.
    fn place_foreign(&mut self, grid: &ForeignGrid<'_>) -> bool {
        // Where the grid's origin is in the character's local frame, and how it is turned. The grid's own corner is the origin of its
        // frame, so a brick is at its corner turned and added.
        let frame = WorldPosition::new(self.origin, [0.0; 3]);
        let origin = grid.pose.origin_relative_to(frame);
        let matrix = grid.pose.rotation_matrix();
        let [x, y, z, w] = grid.pose.orientation.components();
        let turn = Rotation::from_xyzw(x, y, z, w).normalize();
        let edge = BRICK_EDGE_LENGTH as f64;
        let mut moved = false;
        let keys: Vec<(GridId, BrickCoordinate)> = self
            .foreign
            .range((grid.id, min_brick())..=(grid.id, max_brick()))
            .map(|(key, _)| *key)
            .collect();
        for key in keys {
            let corner = [
                key.1.x() as f64 * edge,
                key.1.y() as f64 * edge,
                key.1.z() as f64 * edge,
            ];
            let translation: [f32; 3] = [0, 1, 2].map(|axis| {
                (origin[axis]
                    + matrix[axis][0] * corner[0]
                    + matrix[axis][1] * corner[1]
                    + matrix[axis][2] * corner[2]) as f32
            });
            let bits = [
                translation[0].to_bits(),
                translation[1].to_bits(),
                translation[2].to_bits(),
                x.to_bits(),
                y.to_bits(),
                z.to_bits(),
                w.to_bits(),
            ];
            let brick = self.foreign.get_mut(&key).expect("the key was just found");
            if brick.placed == Some(bits) {
                continue;
            }
            brick.placed = Some(bits);
            if let Some(handle) = brick.collider {
                self.colliders[handle]
                    .set_position(Pose::from_parts(Vector::from_array(translation), turn));
                moved = true;
            }
        }
        moved
    }

    fn load(&mut self, source: &mut impl CollisionSource, brick: BrickCoordinate) {
        let made = build_brick(source, brick, self.origin);
        let mut handles = Vec::new();
        if let Some(collider) = made.terrain {
            handles.push(self.colliders.insert(collider));
        }
        self.loaded.insert(brick, handles);
    }

    fn unload(&mut self, brick: BrickCoordinate) {
        for handle in self.loaded.remove(&brick).unwrap_or_default() {
            self.remove_collider(handle);
        }
    }

    /// Brings the structure that finds what a shape would hit up to date with the colliders.
    fn refresh_queries(&mut self) {
        self.pipeline.step(
            Vector::ZERO,
            &self.parameters,
            &mut self.islands,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.soft_bodies,
            &mut self.ccd,
            &(),
            &(),
        );
    }

    /// One step of [`CharacterSettings::time_step`] in a world whose gravity is the character's own, straight down: the character
    /// follows what the player asks, and gravity, and is stopped by the terrain.
    pub fn step(&mut self, input: MoveInput) {
        self.step_with_gravity(input, [0.0, -self.settings.gravity, 0.0]);
    }

    /// One step, pulled by `gravity`: an acceleration in the axes of the character's frame, whatever the world's field is where it is
    /// (a ship that is turned over has the level's gravity pointing up in its own axes). Up is against it, so a character walks on
    /// whatever floor that makes, and the way it faces is carried along as the pull turns. A pull of nothing leaves the character
    /// weightless: it keeps its frame and drifts on at the speed it had.
    pub fn step_with_gravity(&mut self, input: MoveInput, gravity: [f32; 3]) {
        let settings = self.settings;
        let dt = settings.time_step;
        let pull = if gravity[0] == 0.0 && gravity[2] == 0.0 {
            // The plain case is exact without a square root.
            gravity[1].abs()
        } else {
            Vector::from_array(gravity).length()
        };
        let pull = if pull > LEAST_PULL {
            self.turn_up_to(-Vector::from_array(gravity) / pull);
            pull
        } else {
            0.0
        };
        let level = self.is_level();

        // Not the standard library's sin and cos, which differ between platforms.
        let (sin, cos) = (libm::sinf(input.yaw), libm::cosf(input.yaw));
        let (forward, right) = if level {
            (Vector::new(-sin, 0.0, -cos), Vector::new(cos, 0.0, -sin))
        } else {
            let across = self.heading.cross(self.up);
            (
                self.heading * cos - across * sin,
                across * cos + self.heading * sin,
            )
        };
        let mut wish = forward * f32::from(input.forward) + right * f32::from(input.strafe);
        if wish.length_squared() > 1.0 {
            wish = wish.normalize();
        }
        if self.flying {
            let speed = if input.sprint {
                settings.fly_sprint_speed
            } else {
                settings.fly_speed
            };
            let target = (wish + self.up * f32::from(input.vertical)).clamp_length_max(1.0) * speed;
            self.velocity =
                approach_vector(self.velocity, target, settings.ground_acceleration * dt);
        } else {
            let speed = if input.sprint {
                settings.sprint_speed
            } else {
                settings.walk_speed
            };
            let target = wish * speed;
            let acceleration = if self.grounded {
                settings.ground_acceleration
            } else {
                settings.air_acceleration
            };
            let (across, mut along) = self.split(self.velocity);
            let across = approach_vector(across, target, acceleration * dt);
            if self.grounded && input.jump {
                along = settings.jump_speed;
            }
            along = (along - pull * dt).max(-settings.terminal_speed);
            self.velocity = self.join(across, along);
        }

        // Sideways first and then up or down, as two moves: a move that is both slides along the floor
        // for the part of it that is down and loses some of the part that is sideways.
        let desired = self.velocity * dt;
        let (desired_across, desired_along) = self.split(desired);
        let (sideways, wall) = self.move_through_terrain(dt, desired_across);
        let mut sideways = sideways.translation;
        if let Some(wall) = wall {
            // The slide along a wall, where the wall meets the floor, can come out as nothing: the shape cast gives the wall a normal with a
            // hair of slope (a hundred-thousandth), so the slide has a hair of downward speed, and the controller takes the floor, which it
            // meets at once, to be a slope the walker must not slide down and gives up. It happens to the wall of a ship that is not along
            // the axes of the frame, and now and then to the wall of terrain that is. When it moved the character hardly at all and the
            // slide along the wall is something, the slide is made here, level and along the wall (leaning a little away from it, so
            // that the wall is not met again), and goes through the terrain the same way; it is taken if it gets the character most of
            // the way. (Head on, or into a corner, the slide is nothing, or is stopped again, and nothing changes.)
            let slide = slide_along(desired_across, wall, self.up);
            if sideways.length() < 0.1 * slide.length() {
                let (again, _) = self.move_through_terrain(dt, slide);
                if again.translation.length() > 0.5 * slide.length() {
                    sideways = again.translation;
                }
            }
        }
        self.center += sideways;
        let (vertical, _) = self.move_through_terrain(dt, self.join(Vector::ZERO, desired_along));
        self.center += vertical.translation;
        self.grounded = vertical.grounded;
        let moved = sideways + vertical.translation;
        let (_, moved_along) = self.split(moved);
        // What stopped the character stops its speed that way.
        if (desired_along < 0.0 && moved_along > desired_along + 1.0e-5)
            || (desired_along > 0.0 && moved_along < desired_along - 1.0e-5)
        {
            self.stop_along_up();
        }
        if self.flying {
            // Flying into a wall stops the speed against it.
            if level {
                for axis in [0, 2] {
                    if desired[axis].abs() > 1.0e-5 && (moved[axis] - desired[axis]).abs() > 1.0e-5
                    {
                        self.velocity[axis] = 0.0;
                    }
                }
            } else {
                let across = self.heading.cross(self.up);
                for direction in [across, self.heading] {
                    let (wanted, got) = (desired.dot(direction), moved.dot(direction));
                    if wanted.abs() > 1.0e-5 && (got - wanted).abs() > 1.0e-5 {
                        self.velocity -= direction * self.velocity.dot(direction);
                    }
                }
            }
        }
    }

    /// A vector as the part across the character's up and the length along it.
    fn split(&self, vector: Vector) -> (Vector, f32) {
        if self.up == Vector::Y {
            (Vector::new(vector.x, 0.0, vector.z), vector.y)
        } else {
            let along = vector.dot(self.up);
            (vector - self.up * along, along)
        }
    }

    /// The vector that has `across` across the character's up and `along` along it.
    fn join(&self, across: Vector, along: f32) -> Vector {
        if self.up == Vector::Y {
            Vector::new(across.x, along, across.z)
        } else {
            across + self.up * along
        }
    }

    /// Takes away the speed along the character's up.
    fn stop_along_up(&mut self) {
        if self.up == Vector::Y {
            self.velocity.y = 0.0;
        } else {
            self.velocity -= self.up * self.velocity.dot(self.up);
        }
    }

    /// What the terrain lets the character do of a move, and the normal of the last upright surface (a wall) the move ran into, if any;
    /// the character is not moved.
    fn move_through_terrain(
        &self,
        dt: f32,
        desired: Vector,
    ) -> (
        rapier3d::control::EffectiveCharacterMovement,
        Option<Vector>,
    ) {
        let query = self.broad_phase.as_query_pipeline(
            self.narrow_phase.query_dispatcher(),
            &self.bodies,
            &self.colliders,
            QueryFilter::default(),
        );
        let pose = match self.shape_turn {
            None => Pose::from_translation(self.center),
            Some(turn) => Pose::from_parts(self.center, turn),
        };
        let mut wall = None;
        let up = self.up;
        let movement =
            self.controller
                .move_shape(dt, &query, &self.shape, &pose, desired, |collision| {
                    let normal = collision.hit.normal1;
                    if normal.dot(up).abs() < 0.5 {
                        wall = Some(normal);
                    }
                });
        (movement, wall)
    }

    /// A hash of the character's position, speed and state, bit for bit: two runs of the same
    /// steps on any machines must agree on it.
    pub fn state_hash(&self) -> u64 {
        let mut state: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |value: u64| {
            for byte in value.to_le_bytes() {
                state ^= u64::from(byte);
                state = state.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        feed(self.origin.x() as u64);
        feed(self.origin.y() as u64);
        feed(self.origin.z() as u64);
        for value in self
            .center
            .to_array()
            .into_iter()
            .chain(self.velocity.to_array())
        {
            feed(u64::from(value.to_bits()));
        }
        feed(u64::from(self.grounded));
        feed(u64::from(self.flying));
        // A frame that is not the plain one is part of the state; the plain one adds nothing, so the hash of a character that has only
        // ever stood in a level is what it always was.
        if !self.is_level() {
            for value in self
                .up
                .to_array()
                .into_iter()
                .chain(self.heading.to_array())
            {
                feed(u64::from(value.to_bits()));
            }
        }
        state
    }
}

/// `vector` turned by the shortest turn that takes the unit vector `from` to the unit vector `to`: the sum of the vector, what the turn
/// adds to it and what its square adds, in the form that needs no sine or cosine. If the two are opposite there is no shortest turn
/// and it is a half turn about `axis`, a unit vector across them.
fn turned_between(vector: Vector, from: Vector, to: Vector, axis: Vector) -> Vector {
    let cosine = from.dot(to);
    if cosine < OPPOSITE {
        return axis * (2.0 * axis.dot(vector)) - vector;
    }
    let sine_axis = from.cross(to);
    vector + sine_axis.cross(vector) + sine_axis.cross(sine_axis.cross(vector)) / (1.0 + cosine)
}

fn min_brick() -> BrickCoordinate {
    BrickCoordinate::new(
        voxel_space::BRICK_COORDINATE_MINIMUM,
        voxel_space::BRICK_COORDINATE_MINIMUM,
        voxel_space::BRICK_COORDINATE_MINIMUM,
    )
}

fn max_brick() -> BrickCoordinate {
    BrickCoordinate::new(
        voxel_space::BRICK_COORDINATE_MAXIMUM,
        voxel_space::BRICK_COORDINATE_MAXIMUM,
        voxel_space::BRICK_COORDINATE_MAXIMUM,
    )
}

/// How far from the wall, as a fraction of the slide, the slide made again leans: enough that the controller, which counts a surface
/// within its gap of the character as touched, does not meet the wall again and take its slope's hair for a downward slide once more.
const SLIDE_LEAN_FROM_WALL: f32 = 0.05;

/// A move across the up with what runs into `wall` taken out: it goes along the wall, level, and leans a little away from it. A move that
/// does not run into it is as it was.
fn slide_along(across: Vector, wall: Vector, up: Vector) -> Vector {
    let level = (wall - up * wall.dot(up)).normalize_or_zero();
    let into = across.dot(level);
    if into >= 0.0 {
        across
    } else {
        let along = across - level * into;
        along + level * (SLIDE_LEAN_FROM_WALL * along.length())
    }
}

/// Moves `value` toward `target` by at most `amount`, in a straight line.
fn approach_vector(value: Vector, target: Vector, amount: f32) -> Vector {
    let difference = target - value;
    let distance = difference.length();
    if distance <= amount || distance == 0.0 {
        target
    } else {
        value + difference * (amount / distance)
    }
}
