use std::collections::HashMap;

use rapier3d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
use rapier3d::prelude::*;
use voxel_space::{BRICK_EDGE_LENGTH, BrickCoordinate, WorldPosition};

use crate::terrain::{CollisionSource, build_brick};

/// How far, in bricks, from its frame's origin a character may get before the frame is rebuilt
/// around it. `f32` is exact enough far beyond this; the limit only keeps every number small.
const FRAME_RADIUS_BRICKS: i64 = 6;

/// How many bricks around the one a character is in have colliders: one in every direction is
/// enough to move well inside the middle one, since a body moves a small fraction of a brick a step.
const TERRAIN_RADIUS_BRICKS: i64 = 1;

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
        };
        body.place_eyes(eye);
        body
    }

    pub fn settings(&self) -> &CharacterSettings {
        &self.settings
    }

    /// Where the eyes are.
    pub fn eye_position(&self) -> WorldPosition {
        let eye = self.center + Vector::Y * (self.settings.eye_height - self.settings.height / 2.0);
        self.world_position(eye)
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

    /// Puts the character with its eyes at a position, at rest and in the air.
    pub fn teleport(&mut self, eye: WorldPosition) {
        self.clear_terrain();
        self.origin = eye.brick();
        self.velocity = Vector::ZERO;
        self.grounded = false;
        self.place_eyes(eye);
    }

    fn place_eyes(&mut self, eye: WorldPosition) {
        let local = self.local_position(eye);
        self.center = local - Vector::Y * (self.settings.eye_height - self.settings.height / 2.0);
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
    pub fn update_terrain(&mut self, source: &mut impl CollisionSource) {
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

    /// One step of [`CharacterSettings::time_step`]: the character follows what the player asks, and
    /// gravity, and is stopped by the terrain.
    pub fn step(&mut self, input: MoveInput) {
        let settings = self.settings;
        let dt = settings.time_step;
        // Not the standard library's sin and cos, which differ between platforms.
        let (sin, cos) = (libm::sinf(input.yaw), libm::cosf(input.yaw));
        let forward = Vector::new(-sin, 0.0, -cos);
        let right = Vector::new(cos, 0.0, -sin);
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
            let target =
                (wish + Vector::Y * f32::from(input.vertical)).clamp_length_max(1.0) * speed;
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
            let horizontal = Vector::new(self.velocity.x, 0.0, self.velocity.z);
            let horizontal = approach_vector(horizontal, target, acceleration * dt);
            let mut vertical = self.velocity.y;
            if self.grounded && input.jump {
                vertical = settings.jump_speed;
            }
            vertical = (vertical - settings.gravity * dt).max(-settings.terminal_speed);
            self.velocity = Vector::new(horizontal.x, vertical, horizontal.z);
        }

        // Sideways first and then up or down, as two moves: a move that is both slides along the floor
        // for the part of it that is down and loses some of the part that is sideways.
        let desired = self.velocity * dt;
        let sideways = self.move_through_terrain(dt, Vector::new(desired.x, 0.0, desired.z));
        self.center += sideways.translation;
        let vertical = self.move_through_terrain(dt, Vector::new(0.0, desired.y, 0.0));
        self.center += vertical.translation;
        self.grounded = vertical.grounded;
        let moved = sideways.translation + vertical.translation;
        // What stopped the character stops its speed that way.
        if desired.y < 0.0 && moved.y > desired.y + 1.0e-5 {
            self.velocity.y = 0.0;
        }
        if desired.y > 0.0 && moved.y < desired.y - 1.0e-5 {
            self.velocity.y = 0.0;
        }
        if self.flying {
            // Flying into a wall stops the speed against it.
            for axis in [0, 2] {
                if desired[axis].abs() > 1.0e-5 && (moved[axis] - desired[axis]).abs() > 1.0e-5 {
                    self.velocity[axis] = 0.0;
                }
            }
        }
    }

    /// What the terrain lets the character do of a move; the character is not moved.
    fn move_through_terrain(
        &self,
        dt: f32,
        desired: Vector,
    ) -> rapier3d::control::EffectiveCharacterMovement {
        let query = self.broad_phase.as_query_pipeline(
            self.narrow_phase.query_dispatcher(),
            &self.bodies,
            &self.colliders,
            QueryFilter::default(),
        );
        self.controller.move_shape(
            dt,
            &query,
            &self.shape,
            &Pose::from_translation(self.center),
            desired,
            |_| {},
        )
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
        state
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
