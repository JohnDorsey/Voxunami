use std::collections::{BTreeMap, HashMap};

use rapier3d::prelude::*;
use voxel_space::{
    BRICK_EDGE_LENGTH, BrickCoordinate, Orientation, VoxelCoordinate, WorldPosition,
};

use crate::mass::BodyMass;
use crate::terrain::{BrickHint, Collision, CollisionSource, build_brick};

const EDGE: usize = BRICK_EDGE_LENGTH;

/// Which body: a number the caller chooses (for a ship, its grid's number).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BodyId(pub u64);

/// How a body moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind {
    /// Gravity, forces and collisions move it.
    Dynamic,
    /// It stays where it is, and everything else collides with it.
    Fixed,
}

/// Where a body is and how it moves, in the world: the position of its origin, its orientation, and its velocities (per second; the angular one
/// as an axis scaled by the turn in radians).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyState {
    pub position: WorldPosition,
    pub orientation: Orientation,
    pub linear_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
}

/// The farthest, in voxels, that the fastest point of a body is let move in one solver step.
const MOST_A_STEP_MOVES: f32 = 0.8;
/// The most solver steps a tick is cut into (Rapier caps a body at 400 voxels a second, which is 20 a tick and 0.8 a step at 24).
const MOST_STEPS: u32 = 24;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodySettings {
    /// How long one call to [`BodyWorld::step`] is, in seconds: a tick of the server.
    pub time_step: f32,
    /// How many steps of the solver a call is cut into. More are steadier (a body resting on another does not jitter) and cost more.
    pub substeps: u32,
    /// The acceleration of gravity, in voxels per second per second.
    pub gravity: [f32; 3],
}

impl Default for BodySettings {
    /// A tick of a twentieth of a second solved at 60 steps a second, and the gravity characters fall in.
    fn default() -> Self {
        Self {
            time_step: 1.0 / 20.0,
            substeps: 3,
            gravity: [0.0, -5.0, 0.0],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BodyError {
    #[error("there is no body {0:?}")]
    UnknownBody(BodyId),
    #[error("there already is a body {0:?}")]
    DuplicateBody(BodyId),
    #[error("a brick of a body is more than 2^31 voxels from its origin")]
    BrickOutOfRange,
}

/// One row of 32 voxels of a brick, as bits.
type Rows = Box<[u32; EDGE * EDGE]>;

struct Body {
    handle: RigidBodyHandle,
    kind: BodyKind,
    /// The voxels of the body that are whole cubes, as one collider; made when the first one is set.
    voxels: Option<ColliderHandle>,
    /// The bits set in `voxels`, by brick, so that a brick changes only the voxels that differ.
    filled: HashMap<BrickCoordinate, Rows>,
    /// The collider of the blocks that do not fill their cells, one for each brick that has any.
    partial: HashMap<BrickCoordinate, ColliderHandle>,
}

/// Rigid bodies made of voxels, and the terrain they hit, in one Rapier world.
///
/// Rapier works in `f32`, so everything is in a *local frame*: a brick of the level is the origin of it, and positions are exact only
/// within some tens of thousands of voxels of it ([`BodyWorld::recentre`] moves it). A body's voxels are in the body's own coordinates;
/// terrain is in the frame's. Everything is stepped on one thread in the order bodies were made, with Rapier's `enhanced-determinism`,
/// so the same calls give the same bits on every machine.
pub struct BodyWorld {
    settings: BodySettings,
    origin: BrickCoordinate,
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
    /// The bodies, in order of number.
    by_id: BTreeMap<BodyId, Body>,
    /// The collider of each brick of terrain that has any.
    terrain: HashMap<BrickCoordinate, ColliderHandle>,
}

impl std::fmt::Debug for BodyWorld {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BodyWorld")
            .field("origin", &self.origin)
            .field("bodies", &self.by_id.len())
            .field("terrain_bricks", &self.terrain.len())
            .finish_non_exhaustive()
    }
}

impl BodyWorld {
    /// An empty world whose frame has the corner of `origin` as its origin.
    pub fn new(origin: BrickCoordinate, settings: BodySettings) -> Self {
        let parameters = IntegrationParameters {
            dt: settings.time_step / settings.substeps.max(1) as f32,
            ..IntegrationParameters::default()
        };
        Self {
            settings,
            origin,
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
            by_id: BTreeMap::new(),
            terrain: HashMap::new(),
        }
    }

    pub fn origin(&self) -> BrickCoordinate {
        self.origin
    }

    pub fn body_count(&self) -> usize {
        self.by_id.len()
    }

    pub fn terrain_brick_count(&self) -> usize {
        self.terrain.len()
    }

    /// Where a position is in the frame, in voxels from its origin.
    fn local(&self, position: WorldPosition) -> Vector {
        let (brick, offset) = (position.brick(), position.offset());
        let along = |brick: i64, origin: i64, offset: f32| {
            ((brick - origin) * EDGE as i64) as f64 + f64::from(offset)
        };
        Vector::new(
            along(brick.x(), self.origin.x(), offset[0]) as f32,
            along(brick.y(), self.origin.y(), offset[1]) as f32,
            along(brick.z(), self.origin.z(), offset[2]) as f32,
        )
    }

    /// The position in the world of a place in the frame, or `None` if it is not finite.
    fn world(&self, local: Vector) -> Option<WorldPosition> {
        local
            .is_finite()
            .then(|| WorldPosition::new(self.origin, [local.x, local.y, local.z]))
    }

    fn rotation_of(orientation: Orientation) -> Rotation {
        let [x, y, z, w] = orientation.components_single();
        Rotation::from_xyzw(x, y, z, w).normalize()
    }

    /// Adds a body with no voxels (give it some with [`BodyWorld::set_brick`] and a mass with [`BodyWorld::set_mass`]).
    pub fn add_body(
        &mut self,
        id: BodyId,
        state: BodyState,
        kind: BodyKind,
    ) -> Result<(), BodyError> {
        if self.by_id.contains_key(&id) {
            return Err(BodyError::DuplicateBody(id));
        }
        let builder = match kind {
            BodyKind::Dynamic => RigidBodyBuilder::dynamic(),
            BodyKind::Fixed => RigidBodyBuilder::fixed(),
        }
        .pose(Pose::from_parts(
            self.local(state.position),
            Self::rotation_of(state.orientation),
        ))
        .linvel(Vector::from_array(state.linear_velocity))
        .angvel(Vector::from_array(state.angular_velocity))
        // Until it is given a mass, it weighs one and turns like a ball.
        .additional_mass_properties(MassProperties::new(Vector::ZERO, 1.0, Vector::ONE));
        let handle = self.bodies.insert(builder);
        self.by_id.insert(
            id,
            Body {
                handle,
                kind,
                voxels: None,
                filled: HashMap::new(),
                partial: HashMap::new(),
            },
        );
        Ok(())
    }

    /// Removes a body and all its colliders.
    pub fn remove_body(&mut self, id: BodyId) -> Result<(), BodyError> {
        let body = self.by_id.remove(&id).ok_or(BodyError::UnknownBody(id))?;
        self.bodies.remove(
            body.handle,
            &mut self.islands,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.soft_bodies,
            true,
        );
        Ok(())
    }

    /// Whether a body moves or stays; one made to move again starts from rest.
    pub fn set_kind(&mut self, id: BodyId, kind: BodyKind) -> Result<(), BodyError> {
        let body = self.by_id.get_mut(&id).ok_or(BodyError::UnknownBody(id))?;
        if body.kind == kind {
            return Ok(());
        }
        body.kind = kind;
        let rapier = &mut self.bodies[body.handle];
        match kind {
            BodyKind::Fixed => rapier.set_body_type(RigidBodyType::Fixed, true),
            BodyKind::Dynamic => rapier.set_body_type(RigidBodyType::Dynamic, true),
        }
        Ok(())
    }

    /// Puts a body somewhere, with the velocities it has there.
    pub fn set_state(&mut self, id: BodyId, state: BodyState) -> Result<(), BodyError> {
        let handle = self
            .by_id
            .get(&id)
            .ok_or(BodyError::UnknownBody(id))?
            .handle;
        let pose = Pose::from_parts(
            self.local(state.position),
            Self::rotation_of(state.orientation),
        );
        let rapier = &mut self.bodies[handle];
        rapier.set_position(pose, true);
        rapier.set_linvel(Vector::from_array(state.linear_velocity), true);
        rapier.set_angvel(Vector::from_array(state.angular_velocity), true);
        Ok(())
    }

    /// Gives a body its mass, centre of mass (in the body's coordinates) and tensor of inertia.
    pub fn set_mass(&mut self, id: BodyId, mass: BodyMass) -> Result<(), BodyError> {
        let handle = self
            .by_id
            .get(&id)
            .ok_or(BodyError::UnknownBody(id))?
            .handle;
        let tensor = Matrix::from_cols_array_2d(&mass.inertia);
        self.bodies[handle].set_additional_mass_properties(
            MassProperties::with_inertia_matrix(
                Vector::from_array(mass.centre_of_mass),
                mass.mass,
                tensor,
            ),
            true,
        );
        Ok(())
    }

    /// Where a body is and how it moves, or `None` if there is no such body or the solver has made it not a number.
    pub fn state(&self, id: BodyId) -> Option<BodyState> {
        let body = self.bodies.get(self.by_id.get(&id)?.handle)?;
        let [x, y, z, w] = body.rotation().normalize().to_array();
        Some(BodyState {
            position: self.world(body.translation())?,
            orientation: Orientation::from_single_components([x, y, z, w])?,
            linear_velocity: body.linvel().to_array(),
            angular_velocity: body.angvel().to_array(),
        })
    }

    pub fn is_asleep(&self, id: BodyId) -> bool {
        self.by_id
            .get(&id)
            .and_then(|body| self.bodies.get(body.handle))
            .is_some_and(RigidBody::is_sleeping)
    }

    /// Makes the colliders of one brick of a body (in the body's own coordinates) from what `source` says of its voxels: whole cubes
    /// are voxels of the body's one voxel collider, and blocks that fill part of their cell are boxes. Only what differs from before is
    /// changed. `source` must answer for the body's own coordinates, with an empty voxel where the body has nothing.
    pub fn set_brick(
        &mut self,
        id: BodyId,
        brick: BrickCoordinate,
        source: &mut impl CollisionSource,
    ) -> Result<(), BodyError> {
        let first = brick.origin();
        let key = |x: usize, y: usize, z: usize| -> Result<IVector, BodyError> {
            let voxel = first
                .checked_offset(x as i64, y as i64, z as i64)
                .ok_or(BodyError::BrickOutOfRange)?;
            let component =
                |value: i64| i32::try_from(value).map_err(|_| BodyError::BrickOutOfRange);
            Ok(IVector::new(
                component(voxel.x())?,
                component(voxel.y())?,
                component(voxel.z())?,
            ))
        };
        // Look first, so a brick that is out of range changes nothing (its far corner is the one that is out: bricks start on multiples of 32).
        key(EDGE - 1, EDGE - 1, EDGE - 1)?;
        let hint = source.brick_hint(brick);
        let mut wanted: Rows = Box::new([0; EDGE * EDGE]);
        let mut boxes: Vec<(Pose, SharedShape)> = Vec::new();
        if hint != BrickHint::Empty {
            for z in 0..EDGE {
                for y in 0..EDGE {
                    for x in 0..EDGE {
                        let voxel = VoxelCoordinate::new(
                            first.x() + x as i64,
                            first.y() + y as i64,
                            first.z() + z as i64,
                        );
                        match if hint == BrickHint::Solid {
                            Collision::Full
                        } else {
                            source.collision_at(voxel)
                        } {
                            Collision::Empty => {}
                            Collision::Full => wanted[z * EDGE + y] |= 1 << x,
                            Collision::Partial(parts) => {
                                for part in parts.iter() {
                                    let eighths = |value: u8| f32::from(value) / 8.0;
                                    let min = part.min.map(eighths);
                                    let max = part.max.map(eighths);
                                    boxes.push((
                                        Pose::from_translation(Vector::new(
                                            x as f32 + (min[0] + max[0]) / 2.0,
                                            y as f32 + (min[1] + max[1]) / 2.0,
                                            z as f32 + (min[2] + max[2]) / 2.0,
                                        )),
                                        SharedShape::cuboid(
                                            (max[0] - min[0]) / 2.0,
                                            (max[1] - min[1]) / 2.0,
                                            (max[2] - min[2]) / 2.0,
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        let body = self.by_id.get_mut(&id).ok_or(BodyError::UnknownBody(id))?;
        let before = body.filled.remove(&brick);
        let changes = |x: usize, y: usize, z: usize| -> Option<bool> {
            let bit = 1u32 << x;
            let was = before
                .as_ref()
                .is_some_and(|rows| rows[z * EDGE + y] & bit != 0);
            let is = wanted[z * EDGE + y] & bit != 0;
            (was != is).then_some(is)
        };
        // The voxel collider: edited in place if it exists, or made from the voxels of the first brick that has any.
        match body.voxels {
            Some(handle) => {
                if let Some(voxels) = self
                    .colliders
                    .get_mut(handle)
                    .and_then(|collider| collider.shape_mut().as_voxels_mut())
                {
                    for z in 0..EDGE {
                        for y in 0..EDGE {
                            for x in 0..EDGE {
                                if let Some(filled) = changes(x, y, z) {
                                    voxels.set_voxel(key(x, y, z)?, filled);
                                }
                            }
                        }
                    }
                }
            }
            None => {
                let mut keys = Vec::new();
                for z in 0..EDGE {
                    for y in 0..EDGE {
                        for x in 0..EDGE {
                            if changes(x, y, z) == Some(true) {
                                keys.push(key(x, y, z)?);
                            }
                        }
                    }
                }
                if !keys.is_empty() {
                    let collider = ColliderBuilder::voxels(Vector::ONE, &keys)
                        .density(0.0)
                        .friction(0.6);
                    body.voxels = Some(self.colliders.insert_with_parent(
                        collider,
                        body.handle,
                        &mut self.bodies,
                    ));
                }
            }
        }
        if wanted.iter().any(|row| *row != 0) {
            body.filled.insert(brick, wanted);
        }
        // The boxes: the brick's one compound is made again.
        if let Some(old) = body.partial.remove(&brick) {
            self.colliders.remove(
                old,
                &mut self.islands,
                &mut self.bodies,
                &mut self.soft_bodies,
                true,
            );
        }
        if !boxes.is_empty() {
            let collider = ColliderBuilder::compound(boxes)
                .translation(Vector::new(
                    first.x() as f32,
                    first.y() as f32,
                    first.z() as f32,
                ))
                .density(0.0)
                .friction(0.6);
            body.partial.insert(
                brick,
                self.colliders
                    .insert_with_parent(collider, body.handle, &mut self.bodies),
            );
        }
        Ok(())
    }

    /// Makes the collider of one brick of the terrain (in the frame's coordinates) from what `source` says of it, replacing the one it had.
    pub fn set_terrain_brick(&mut self, brick: BrickCoordinate, source: &mut impl CollisionSource) {
        self.remove_terrain_brick(brick);
        if let Some(collider) = build_brick(source, brick, self.origin).terrain {
            let handle = self.colliders.insert(collider);
            self.terrain.insert(brick, handle);
        }
    }

    /// The bricks of the level, in the world, that a body's colliders come within `margin` voxels of, as they stand now (the box around the
    /// turned body, so a little more than it fills if it is turned), or `None` for a body with no blocks yet or one that is not there. It
    /// is what terrain a body needs colliders of, as far ahead as it can get in the time that is wanted.
    pub fn bricks_near(&self, id: BodyId, margin: f32) -> Option<voxel_space::BrickBox> {
        let body = self.by_id.get(&id)?;
        let mut low = Vector::splat(f32::INFINITY);
        let mut high = Vector::splat(f32::NEG_INFINITY);
        for &handle in self.bodies[body.handle].colliders() {
            let bounds = self.colliders[handle].compute_aabb();
            low = low.min(bounds.mins);
            high = high.max(bounds.maxs);
        }
        if !(low.x <= high.x && low.y <= high.y && low.z <= high.z) {
            return None;
        }
        let brick = |value: f32, origin: i64| {
            origin.saturating_add((f64::from(value) / EDGE as f64).floor() as i64)
        };
        let (low, high) = (low - Vector::splat(margin), high + Vector::splat(margin));
        voxel_space::BrickBox::new(
            BrickCoordinate::new(
                brick(low.x, self.origin.x()),
                brick(low.y, self.origin.y()),
                brick(low.z, self.origin.z()),
            ),
            BrickCoordinate::new(
                brick(high.x, self.origin.x()),
                brick(high.y, self.origin.y()),
                brick(high.z, self.origin.z()),
            ),
        )
    }

    pub fn remove_terrain_brick(&mut self, brick: BrickCoordinate) {
        if let Some(handle) = self.terrain.remove(&brick) {
            self.colliders.remove(
                handle,
                &mut self.islands,
                &mut self.bodies,
                &mut self.soft_bodies,
                true,
            );
        }
    }

    /// Pushes a body with a force (voxel masses times voxels per second per second) for the next [`BodyWorld::step`] only, as if applied at
    /// a place on the body in the body's own coordinates, or at its centre of mass if none is given. A force off the centre also turns it.
    pub fn apply_force(
        &mut self,
        id: BodyId,
        force: [f32; 3],
        at: Option<[f32; 3]>,
    ) -> Result<(), BodyError> {
        let handle = self
            .by_id
            .get(&id)
            .ok_or(BodyError::UnknownBody(id))?
            .handle;
        let body = &mut self.bodies[handle];
        let force = Vector::from_array(force);
        match at {
            None => body.add_force(force, true),
            Some(place) => {
                let point = body.position() * Vector::from_array(place);
                body.add_force_at_point(force, point, true);
            }
        }
        Ok(())
    }

    /// Turns a body with a torque (an axis scaled by voxel masses times voxels squared per second per second, in the world's axes) about its
    /// centre of mass, for the next [`BodyWorld::step`] only.
    pub fn apply_torque(&mut self, id: BodyId, torque: [f32; 3]) -> Result<(), BodyError> {
        let handle = self
            .by_id
            .get(&id)
            .ok_or(BodyError::UnknownBody(id))?
            .handle;
        self.bodies[handle].add_torque(Vector::from_array(torque), true);
        Ok(())
    }

    /// Pushes a body with a force for the next [`BodyWorld::step`] only, like [`BodyWorld::apply_force`] at its centre of mass, except that it
    /// does not wake a body that is asleep. It is for forces that are always there, such as gravity, which a body at rest has no need to be
    /// woken by: if something else wakes it, the force is on it again.
    pub fn apply_steady_force(&mut self, id: BodyId, force: [f32; 3]) -> Result<(), BodyError> {
        let handle = self
            .by_id
            .get(&id)
            .ok_or(BodyError::UnknownBody(id))?
            .handle;
        self.bodies[handle].add_force(Vector::from_array(force), false);
        Ok(())
    }

    /// Moves everything one tick: gravity and the forces applied since the last call, then collisions, in
    /// [`BodySettings::substeps`] steps. The forces are then forgotten.
    pub fn step(&mut self) {
        let gravity = Vector::from_array(self.settings.gravity);
        let steps = self.steps_for_this_tick();
        self.parameters.dt = self.settings.time_step / steps as f32;
        for _ in 0..steps {
            self.pipeline.step(
                gravity,
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
        for (_, body) in self.bodies.iter_mut() {
            body.reset_forces(false);
            body.reset_torques(false);
        }
    }

    /// How many solver steps this tick is cut into: the settings' number, or more when a body is fast enough to cross [`MOST_A_STEP_MOVES`] voxels in
    /// one of them, since a body that moves farther than the ground is thick goes through it. (Rapier's own continuous detection leaves out the voxel
    /// colliders that ships are made of.) At most [`MOST_STEPS`]. A tick in which nothing is fast is cut as the settings say, bit for bit.
    fn steps_for_this_tick(&self) -> u32 {
        let base = self.settings.substeps.max(1);
        let mut fastest = 0.0f32;
        for (_, body) in self.bodies.iter() {
            if !body.is_dynamic() {
                continue;
            }
            let mut speed = body.linvel().length();
            let turn = body.angvel().length();
            if turn > 0.0 {
                // The farthest point of the body from its middle, by the boxes of its colliders.
                let middle = body.translation();
                let reach = body
                    .colliders()
                    .iter()
                    .map(|&handle| {
                        let aabb = self.colliders[handle].compute_aabb();
                        (aabb.center() - middle).length() + aabb.half_extents().length()
                    })
                    .fold(0.0f32, f32::max);
                speed += turn * reach;
            }
            fastest = fastest.max(speed);
        }
        let needed = (fastest * self.settings.time_step / MOST_A_STEP_MOVES).ceil();
        (needed as u32).clamp(base, MOST_STEPS.max(base))
    }

    /// Moves the frame's origin to the corner of another brick, keeping everything where it is in the world. Positions near the new origin
    /// are more exact than they were.
    pub fn recentre(&mut self, origin: BrickCoordinate) {
        let shift = Vector::new(
            ((origin.x() - self.origin.x()) * EDGE as i64) as f32,
            ((origin.y() - self.origin.y()) * EDGE as i64) as f32,
            ((origin.z() - self.origin.z()) * EDGE as i64) as f32,
        );
        for (_, body) in self.bodies.iter_mut() {
            let pose = *body.position();
            body.set_position(
                Pose::from_parts(pose.translation - shift, pose.rotation),
                false,
            );
        }
        for (_, collider) in self.colliders.iter_mut() {
            // Only the colliders that have no body (the terrain's) are placed in the frame.
            if collider.parent().is_none() {
                let pose = *collider.position();
                collider.set_position(Pose::from_parts(pose.translation - shift, pose.rotation));
            }
        }
        self.origin = origin;
    }

    /// A hash of every body's place and motion, bit for bit, in order of number: two runs of the same calls on any machines must agree.
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
        for (id, body) in &self.by_id {
            feed(id.0);
            let rapier = &self.bodies[body.handle];
            for value in rapier
                .translation()
                .to_array()
                .into_iter()
                .chain(rapier.rotation().to_array())
                .chain(rapier.linvel().to_array())
                .chain(rapier.angvel().to_array())
            {
                feed(u64::from(value.to_bits()));
            }
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use block_models::ShapeBox;

    use super::*;
    use crate::mass::MassMoments;

    /// Ground: every voxel below `top` is a whole cube.
    struct Ground {
        top: i64,
    }

    impl CollisionSource for Ground {
        fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
            if voxel.y() < self.top {
                Collision::Full
            } else {
                Collision::Empty
            }
        }

        fn brick_hint(&mut self, brick: BrickCoordinate) -> BrickHint {
            let low = brick.y() * EDGE as i64;
            if low + EDGE as i64 <= self.top {
                BrickHint::Solid
            } else if low >= self.top {
                BrickHint::Empty
            } else {
                BrickHint::Unknown
            }
        }
    }

    /// The voxels of a ship, by coordinates; nothing where it has none.
    #[derive(Clone, Default)]
    struct Hull(HashMap<VoxelCoordinate, Collision>);

    impl Hull {
        fn cube(size: i64) -> Self {
            let mut hull = Self::default();
            for x in 0..size {
                for y in 0..size {
                    for z in 0..size {
                        hull.0
                            .insert(VoxelCoordinate::new(x, y, z), Collision::Full);
                    }
                }
            }
            hull
        }

        fn mass(&self, density: f64) -> BodyMass {
            let mut voxels: Vec<&VoxelCoordinate> = self.0.keys().collect();
            voxels.sort();
            MassMoments::sum(
                voxels
                    .into_iter()
                    .map(|v| MassMoments::of_voxel([v.x(), v.y(), v.z()], density)),
            )
            .properties()
            .expect("the hull has mass")
        }
    }

    impl CollisionSource for &Hull {
        fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
            self.0.get(&voxel).cloned().unwrap_or(Collision::Empty)
        }
    }

    fn at(x: f32, y: f32, z: f32) -> WorldPosition {
        WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z])
    }

    fn resting_at(position: WorldPosition) -> BodyState {
        BodyState {
            position,
            orientation: Orientation::IDENTITY,
            linear_velocity: [0.0; 3],
            angular_velocity: [0.0; 3],
        }
    }

    /// Flat ground with its top at 0, as colliders of the bricks around the origin.
    /// A planet that pulls at 32 metres a second a second, six times as hard as the default (5): most of these tests were written for falls that
    /// are fast, to see that bodies are stopped by thin ground and rest on what they land on, and they are about bodies and not about the planet.
    fn heavy_planet() -> BodySettings {
        BodySettings {
            gravity: [0.0, -32.0, 0.0],
            ..BodySettings::default()
        }
    }

    fn world_with_ground() -> BodyWorld {
        world_with_ground_and(heavy_planet())
    }

    fn world_with_ground_and(settings: BodySettings) -> BodyWorld {
        let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
        for x in -1..=1 {
            for z in -1..=1 {
                world.set_terrain_brick(BrickCoordinate::new(x, -1, z), &mut Ground { top: 0 });
            }
        }
        world
    }

    /// A floor `thickness` voxels thick with its top at 0, as the colliders of the bricks around the origin.
    struct Floor {
        thickness: i64,
    }

    impl CollisionSource for Floor {
        fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
            if voxel.y() < 0 && voxel.y() >= -self.thickness {
                Collision::Full
            } else {
                Collision::Empty
            }
        }
    }

    #[test]
    fn a_body_that_falls_faster_than_the_floor_is_thick_still_stops_on_it() {
        for thickness in [1, 3] {
            for size in [1, 4] {
                for speed in [100.0f32, 250.0, 400.0] {
                    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, heavy_planet());
                    for x in -1..=1 {
                        for z in -1..=1 {
                            world.set_terrain_brick(
                                BrickCoordinate::new(x, -1, z),
                                &mut Floor { thickness },
                            );
                        }
                    }
                    let hull = Hull::cube(size);
                    let mut state = resting_at(at(2.0, 20.0, 2.0));
                    state.linear_velocity = [0.0, -speed, 0.0];
                    ship_of(&mut world, 1, &hull, state, BodyKind::Dynamic);
                    run(&mut world, 40);
                    let height = world.state(BodyId(1)).unwrap().position.offset()[1]
                        + world.state(BodyId(1)).unwrap().position.brick().y() as f32 * 32.0;
                    // On top of the floor: its corner is on the floor or, if it came to rest on another face, a cube's width above (not below the top).
                    assert!(
                        (-0.2..=size as f32 + 0.2).contains(&height),
                        "a cube of {size} at {speed} on a floor of {thickness} ended at {height}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_tick_in_which_nothing_is_fast_is_cut_as_the_settings_say() {
        let mut world = world_with_ground();
        let hull = Hull::cube(2);
        let mut state = resting_at(at(2.0, 20.0, 2.0));
        state.linear_velocity = [0.0, -40.0, 0.0];
        ship_of(&mut world, 1, &hull, state, BodyKind::Dynamic);
        assert_eq!(world.steps_for_this_tick(), 3);
        let mut faster = state;
        faster.linear_velocity = [0.0, -100.0, 0.0];
        world.set_state(BodyId(1), faster).unwrap();
        assert_eq!(world.steps_for_this_tick(), 7);
        faster.linear_velocity = [0.0, -4000.0, 0.0];
        world.set_state(BodyId(1), faster).unwrap();
        assert_eq!(world.steps_for_this_tick(), 24);
    }

    fn ship_of(world: &mut BodyWorld, id: u64, hull: &Hull, state: BodyState, kind: BodyKind) {
        world.add_body(BodyId(id), state, kind).unwrap();
        world
            .set_brick(BodyId(id), BrickCoordinate::ORIGIN, &mut &*hull)
            .unwrap();
        world.set_mass(BodyId(id), hull.mass(1.0)).unwrap();
    }

    fn run(world: &mut BodyWorld, ticks: usize) {
        for _ in 0..ticks {
            world.step();
        }
    }

    fn height_of(world: &BodyWorld, id: u64) -> f32 {
        let state = world.state(BodyId(id)).unwrap();
        let (brick, offset) = (state.position.brick(), state.position.offset());
        brick.y() as f32 * 32.0 + offset[1]
    }

    fn speed_of(world: &BodyWorld, id: u64) -> f32 {
        let [x, y, z] = world.state(BodyId(id)).unwrap().linear_velocity;
        (x * x + y * y + z * z).sqrt()
    }

    #[test]
    fn the_bricks_near_a_body_are_those_its_colliders_come_within_a_margin_of() {
        use voxel_space::BrickBox;
        let bricks = |low: [i64; 3], high: [i64; 3]| {
            Some(
                BrickBox::new(
                    BrickCoordinate::new(low[0], low[1], low[2]),
                    BrickCoordinate::new(high[0], high[1], high[2]),
                )
                .unwrap(),
            )
        };
        let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings::default());
        // A body with no blocks yet has no bounds.
        world
            .add_body(
                BodyId(1),
                resting_at(at(10.0, 10.0, 10.0)),
                BodyKind::Dynamic,
            )
            .unwrap();
        assert_eq!(world.bricks_near(BodyId(1), 5.0), None);
        // A cube of 4 with its corner at (10, 10, 10) is inside the brick (0, 0, 0), and the margin takes in the bricks beside it only when it
        // reaches them: the cube is from 10 to 14, the brick from 0 to 32. (Within the voxels of a collider a little: its bounds are of
        // the cells of its grid.)
        let hull = Hull::cube(4);
        world
            .set_brick(BodyId(1), BrickCoordinate::ORIGIN, &mut &hull)
            .unwrap();
        assert_eq!(world.bricks_near(BodyId(1), 0.0), bricks([0; 3], [0; 3]));
        assert_eq!(world.bricks_near(BodyId(1), 4.0), bricks([0; 3], [0; 3]));
        // 10 away from 10 is the corner of the brick, 11 is past it; 14 and 18 away is 25, not yet the next brick, and 19 away is 33.
        assert_eq!(world.bricks_near(BodyId(1), 10.0), bricks([0; 3], [0; 3]));
        assert_eq!(world.bricks_near(BodyId(1), 11.0), bricks([-1; 3], [0; 3]));
        assert_eq!(world.bricks_near(BodyId(1), 19.0), bricks([-1; 3], [1; 3]));
        // 33 below the corner of the cube's 10 is -33: two bricks down.
        assert_eq!(world.bricks_near(BodyId(1), 43.0), bricks([-2; 3], [1; 3]));
        // No body of that number.
        assert_eq!(world.bricks_near(BodyId(7), 5.0), None);
    }

    #[test]
    fn the_bricks_near_a_body_are_in_the_world_whatever_the_frame_and_follow_its_turn() {
        use voxel_space::BrickBox;
        // The frame's origin is the brick (2, 0, 0); the body is in the brick (3, 0, 1).
        let mut world = BodyWorld::new(BrickCoordinate::new(2, 0, 0), BodySettings::default());
        let hull = Hull::cube(4);
        world
            .add_body(
                BodyId(1),
                resting_at(WorldPosition::new(
                    BrickCoordinate::new(3, 0, 1),
                    [10.0, 10.0, 10.0],
                )),
                BodyKind::Dynamic,
            )
            .unwrap();
        world
            .set_brick(BodyId(1), BrickCoordinate::ORIGIN, &mut &hull)
            .unwrap();
        let only = BrickBox::new(BrickCoordinate::new(3, 0, 1), BrickCoordinate::new(3, 0, 1));
        assert_eq!(world.bricks_near(BodyId(1), 2.0), only);
        // A long body turned a quarter about the vertical axis lies where it is turned to, not where it was: a bar of 10 along x from (40, 5, 36)
        // goes along -z, to z from 26 to 36, and is in the bricks z = 0 and 1 and not only 1.
        let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings::default());
        let mut bar = Hull::default();
        for x in 0..10 {
            bar.0.insert(VoxelCoordinate::new(x, 0, 0), Collision::Full);
        }
        world
            .add_body(
                BodyId(1),
                BodyState {
                    position: at(40.0, 5.0, 36.0),
                    orientation: Orientation::from_yaw_pitch(std::f32::consts::FRAC_PI_2, 0.0),
                    linear_velocity: [0.0; 3],
                    angular_velocity: [0.0; 3],
                },
                BodyKind::Dynamic,
            )
            .unwrap();
        world
            .set_brick(BodyId(1), BrickCoordinate::ORIGIN, &mut &bar)
            .unwrap();
        let turned = world.bricks_near(BodyId(1), 0.0).unwrap();
        assert_eq!(
            (
                turned.min().x(),
                turned.max().x(),
                turned.min().z(),
                turned.max().z()
            ),
            (1, 1, 0, 1)
        );
    }

    #[test]
    fn a_ship_falls_by_gravity_comes_to_rest_on_the_ground_and_goes_to_sleep() {
        // On the planet of the default, 5 metres a second a second.
        let mut world = world_with_ground_and(BodySettings::default());
        let hull = Hull::cube(4);
        ship_of(
            &mut world,
            1,
            &hull,
            resting_at(at(8.0, 6.0, 8.0)),
            BodyKind::Dynamic,
        );
        run(&mut world, 2);
        // After a tenth of a second in a gravity of 5 it is falling at about half a metre a second.
        let velocity = world.state(BodyId(1)).unwrap().linear_velocity;
        assert!((velocity[1] + 0.5).abs() < 0.02, "{velocity:?}");
        assert!(velocity[0].abs() < 1e-4 && velocity[2].abs() < 1e-4);
        run(&mut world, 98);
        let height = height_of(&world, 1);
        assert!(
            height.abs() < 0.02,
            "it rests on the ground, not in it: {height}"
        );
        assert!(speed_of(&world, 1) < 0.01);
        assert!(world.is_asleep(BodyId(1)), "a body at rest sleeps");
        let state = world.state(BodyId(1)).unwrap();
        let offset = state.position.offset();
        // Landing on a face may slide it a few hundredths of a voxel.
        assert!(
            (offset[0] - 8.0).abs() < 0.05 && (offset[2] - 8.0).abs() < 0.05,
            "{offset:?}"
        );
        let [x, y, z, w] = state.orientation.components();
        assert!(x.abs() < 1e-3 && y.abs() < 1e-3 && z.abs() < 1e-3 && w > 0.9999);
    }

    #[test]
    fn a_force_accelerates_a_ship_by_its_mass_for_one_tick_only_and_one_off_centre_turns_it() {
        let mut world = BodyWorld::new(
            BrickCoordinate::ORIGIN,
            BodySettings {
                gravity: [0.0; 3],
                ..BodySettings::default()
            },
        );
        // A 2 by 2 by 2 ship of density 5: a mass of 40.
        let hull = Hull::cube(2);
        ship_of(
            &mut world,
            1,
            &hull,
            resting_at(at(0.0, 0.0, 0.0)),
            BodyKind::Dynamic,
        );
        world.set_mass(BodyId(1), hull.mass(5.0)).unwrap();
        world
            .apply_force(BodyId(1), [400.0, 0.0, 0.0], None)
            .unwrap();
        world.step();
        let velocity = world.state(BodyId(1)).unwrap().linear_velocity;
        // 400 / 40 = 10 voxels a second a second, for a twentieth of a second.
        assert!((velocity[0] - 0.5).abs() < 1e-3, "{velocity:?}");
        // The force was for that step alone.
        world.step();
        assert!((world.state(BodyId(1)).unwrap().linear_velocity[0] - 0.5).abs() < 1e-3);
        assert_eq!(world.state(BodyId(1)).unwrap().angular_velocity, [0.0; 3]);
        // The same push at a place further along z than the centre (z = 1) turns the ship about y, counter-clockwise seen from above.
        world
            .set_state(BodyId(1), resting_at(at(0.0, 0.0, 0.0)))
            .unwrap();
        world
            .apply_force(BodyId(1), [400.0, 0.0, 0.0], Some([1.0, 1.0, 2.0]))
            .unwrap();
        world.step();
        let turning = world.state(BodyId(1)).unwrap().angular_velocity;
        assert!(turning[1] > 0.1, "{turning:?}");
        assert!(turning[0].abs() < 1e-4 && turning[2].abs() < 1e-4);
    }

    #[test]
    fn a_torque_turns_a_ship_by_its_inertia_for_one_tick_only_and_moves_its_centre_nowhere() {
        let mut world = BodyWorld::new(
            BrickCoordinate::ORIGIN,
            BodySettings {
                gravity: [0.0; 3],
                ..BodySettings::default()
            },
        );
        // A 2 by 2 by 2 ship of density 5: a mass of 40, and about any axis through its middle 40 * (4 + 4) / 12.
        let hull = Hull::cube(2);
        ship_of(
            &mut world,
            1,
            &hull,
            resting_at(at(0.0, 0.0, 0.0)),
            BodyKind::Dynamic,
        );
        world.set_mass(BodyId(1), hull.mass(5.0)).unwrap();
        world.apply_torque(BodyId(1), [0.0, 100.0, 0.0]).unwrap();
        world.step();
        let state = world.state(BodyId(1)).unwrap();
        // 100 / (80 / 3) = 3.75 radians a second a second, for a twentieth of a second.
        assert!(
            (state.angular_velocity[1] - 3.75 / 20.0).abs() < 1e-3,
            "{:?}",
            state.angular_velocity
        );
        assert!(state.angular_velocity[0].abs() < 1e-5 && state.angular_velocity[2].abs() < 1e-5);
        assert_eq!(state.linear_velocity, [0.0; 3], "a torque does not push");
        // The torque was for that step alone.
        world.step();
        assert!((world.state(BodyId(1)).unwrap().angular_velocity[1] - 3.75 / 20.0).abs() < 1e-3);
    }

    #[test]
    fn taking_the_blocks_under_a_ship_away_lets_it_sink_by_that_much() {
        let mut world = world_with_ground();
        let mut hull = Hull::cube(4);
        ship_of(
            &mut world,
            1,
            &hull,
            resting_at(at(8.0, 0.0, 8.0)),
            BodyKind::Dynamic,
        );
        run(&mut world, 40);
        assert!(height_of(&world, 1).abs() < 0.02);
        // The bottom layer goes: the ship is made again of what is left, and settles a voxel lower.
        for x in 0..4 {
            for z in 0..4 {
                hull.0.remove(&VoxelCoordinate::new(x, 0, z));
            }
        }
        world
            .set_brick(BodyId(1), BrickCoordinate::ORIGIN, &mut &hull)
            .unwrap();
        run(&mut world, 80);
        let height = height_of(&world, 1);
        assert!((height + 1.0).abs() < 0.02, "{height}");
    }

    #[test]
    fn blocks_that_fill_part_of_their_cell_are_boxes_that_bodies_rest_on() {
        let mut world = world_with_ground();
        let mut hull = Hull::default();
        // The upper half of the one voxel: its underside is half way up the voxel.
        hull.0.insert(
            VoxelCoordinate::new(0, 0, 0),
            Collision::Partial(Arc::from(vec![ShapeBox {
                min: [0, 4, 0],
                max: [8, 8, 8],
            }])),
        );
        world
            .add_body(BodyId(1), resting_at(at(8.0, 3.0, 8.0)), BodyKind::Dynamic)
            .unwrap();
        world
            .set_brick(BodyId(1), BrickCoordinate::ORIGIN, &mut &hull)
            .unwrap();
        world
            .set_mass(
                BodyId(1),
                MassMoments::of_voxel([0, 0, 0], 1.0).properties().unwrap(),
            )
            .unwrap();
        run(&mut world, 80);
        let height = height_of(&world, 1);
        assert!(
            (height + 0.5).abs() < 0.02,
            "the ship origin is half a voxel under the ground: {height}"
        );
        // Taking the block away takes the box away: nothing is left to rest on.
        hull.0.clear();
        world
            .set_brick(BodyId(1), BrickCoordinate::ORIGIN, &mut &hull)
            .unwrap();
        run(&mut world, 20);
        assert!(
            height_of(&world, 1) < -3.0,
            "it fell through where it had nothing"
        );
    }

    #[test]
    fn a_ship_lands_on_a_fixed_one_and_a_fixed_one_made_to_move_again_starts_from_rest() {
        let mut world = world_with_ground();
        ship_of(
            &mut world,
            1,
            &Hull::cube(4),
            resting_at(at(0.0, 0.0, 0.0)),
            BodyKind::Fixed,
        );
        ship_of(
            &mut world,
            2,
            &Hull::cube(2),
            resting_at(at(1.0, 10.0, 1.0)),
            BodyKind::Dynamic,
        );
        run(&mut world, 80);
        assert_eq!(height_of(&world, 1), 0.0, "a fixed body does not move");
        let height = height_of(&world, 2);
        assert!(
            (height - 4.0).abs() < 0.02,
            "it rests on top of the fixed one: {height}"
        );
        // Fixed, a falling ship stops where it is; dynamic again, it falls from rest.
        ship_of(
            &mut world,
            3,
            &Hull::cube(2),
            resting_at(at(20.0, 20.0, 20.0)),
            BodyKind::Dynamic,
        );
        run(&mut world, 4);
        world.set_kind(BodyId(3), BodyKind::Fixed).unwrap();
        let held = height_of(&world, 3);
        run(&mut world, 10);
        assert_eq!(height_of(&world, 3), held);
        world.set_kind(BodyId(3), BodyKind::Dynamic).unwrap();
        assert_eq!(speed_of(&world, 3), 0.0);
        run(&mut world, 2);
        assert!(height_of(&world, 3) < held);
    }

    #[test]
    fn a_ship_is_held_up_by_the_terrain_colliders_that_exist_and_falls_where_they_do_not() {
        let mut world = world_with_ground();
        assert_eq!(world.terrain_brick_count(), 9);
        let hull = Hull::cube(2);
        ship_of(
            &mut world,
            1,
            &hull,
            resting_at(at(8.0, 3.0, 8.0)),
            BodyKind::Dynamic,
        );
        run(&mut world, 60);
        assert!(height_of(&world, 1).abs() < 0.02);
        // The brick under it goes, and so does the ground there.
        world.remove_terrain_brick(BrickCoordinate::new(0, -1, 0));
        assert_eq!(world.terrain_brick_count(), 8);
        world
            .set_state(BodyId(1), resting_at(at(8.0, 3.0, 8.0)))
            .unwrap();
        run(&mut world, 40);
        assert!(height_of(&world, 1) < -10.0);
        // And back: made again, it holds.
        world.set_terrain_brick(BrickCoordinate::new(0, -1, 0), &mut Ground { top: 0 });
        world
            .set_state(BodyId(1), resting_at(at(8.0, 3.0, 8.0)))
            .unwrap();
        run(&mut world, 60);
        assert!(height_of(&world, 1).abs() < 0.02);
        // A brick that has nothing to touch makes no collider.
        world.set_terrain_brick(BrickCoordinate::new(5, 5, 5), &mut Ground { top: 0 });
        assert_eq!(world.terrain_brick_count(), 9);
    }

    #[test]
    fn a_box_of_a_brick_far_along_the_ship_is_where_that_brick_is() {
        let mut world = world_with_ground();
        // The same half block as before, in the voxel (40, 0, 0), which is in the brick (1, 0, 0).
        let mut hull = Hull::default();
        hull.0.insert(
            VoxelCoordinate::new(40, 0, 0),
            Collision::Partial(Arc::from(vec![ShapeBox {
                min: [0, 4, 0],
                max: [8, 8, 8],
            }])),
        );
        world
            .add_body(
                BodyId(1),
                resting_at(at(-32.0, 3.0, 8.0)),
                BodyKind::Dynamic,
            )
            .unwrap();
        world
            .set_brick(BodyId(1), BrickCoordinate::new(1, 0, 0), &mut &hull)
            .unwrap();
        world
            .set_mass(
                BodyId(1),
                MassMoments::of_voxel([40, 0, 0], 1.0).properties().unwrap(),
            )
            .unwrap();
        run(&mut world, 80);
        // The block is at x 40 of the ship, so at x 8 of the world, over the ground, where it holds the ship up half a voxel down.
        let height = height_of(&world, 1);
        assert!((height + 0.5).abs() < 0.02, "{height}");
    }

    #[test]
    fn a_removed_body_leaves_nothing_behind_and_a_terrain_brick_made_again_replaces_the_old_one() {
        let mut world = world_with_ground();
        ship_of(
            &mut world,
            1,
            &Hull::cube(6),
            resting_at(at(8.0, 0.0, 8.0)),
            BodyKind::Fixed,
        );
        world.remove_body(BodyId(1)).unwrap();
        // Where it was there is only the ground: a ship dropped there rests on that.
        ship_of(
            &mut world,
            2,
            &Hull::cube(2),
            resting_at(at(9.0, 10.0, 9.0)),
            BodyKind::Dynamic,
        );
        run(&mut world, 80);
        assert!(
            height_of(&world, 2).abs() < 0.02,
            "{}",
            height_of(&world, 2)
        );
        // The ground there made again from a source with nothing in it: nothing is left of the old one.
        struct Nothing;
        impl CollisionSource for Nothing {
            fn collision_at(&mut self, _: VoxelCoordinate) -> Collision {
                Collision::Empty
            }
        }
        world.set_terrain_brick(BrickCoordinate::new(0, -1, 0), &mut Nothing);
        world
            .set_state(BodyId(2), resting_at(at(9.0, 3.0, 9.0)))
            .unwrap();
        run(&mut world, 40);
        assert!(height_of(&world, 2) < -10.0);
    }

    #[test]
    fn a_turned_ship_falls_without_turning_and_lands_without_going_into_the_ground() {
        let mut world = world_with_ground();
        let hull = Hull::cube(4);
        let orientation = Orientation::from_yaw_pitch(0.5, 0.3);
        let state = BodyState {
            orientation,
            ..resting_at(at(8.0, 12.0, 8.0))
        };
        ship_of(&mut world, 1, &hull, state, BodyKind::Dynamic);
        run(&mut world, 5);
        // (The solver holds it as f32: the turn that was set, rounded to f32, is what stays.)
        assert_eq!(
            world
                .state(BodyId(1))
                .unwrap()
                .orientation
                .components_single(),
            orientation.components_single()
        );
        run(&mut world, 120);
        assert!(world.is_asleep(BodyId(1)));
        // No corner of the hull is below the ground, to within the skin of a contact, and the lowest is on it.
        let final_state = world.state(BodyId(1)).unwrap();
        let [x, y, z, w] = final_state.orientation.components_single();
        let turn = Rotation::from_xyzw(x, y, z, w);
        let origin_height = height_of(&world, 1);
        let mut lowest = f32::MAX;
        for corner_x in [0.0f32, 4.0] {
            for corner_y in [0.0f32, 4.0] {
                for corner_z in [0.0f32, 4.0] {
                    let corner = turn * Vector::new(corner_x, corner_y, corner_z);
                    lowest = lowest.min(origin_height + corner.y);
                }
            }
        }
        assert!(lowest > -0.05, "{lowest}");
        assert!(lowest < 0.05, "it rests on the ground: {lowest}");
    }

    #[test]
    fn positions_far_from_the_origin_of_the_frame_and_moving_it_keep_a_ship_where_it_is() {
        let mut world = world_with_ground();
        let far = WorldPosition::new(BrickCoordinate::new(500, 3, -400), [1.0, 2.0, 3.0]);
        ship_of(
            &mut world,
            1,
            &Hull::cube(2),
            resting_at(far),
            BodyKind::Fixed,
        );
        let read = world.state(BodyId(1)).unwrap().position;
        assert_eq!(read.brick(), far.brick());
        for axis in 0..3 {
            assert!(
                (read.offset()[axis] - far.offset()[axis]).abs() < 0.002,
                "{read:?}"
            );
        }
        // A ship at rest on the ground, and the frame moved to a brick far away, with the terrain it stands on: it rests as before.
        ship_of(
            &mut world,
            2,
            &Hull::cube(2),
            resting_at(at(8.0, 3.0, 8.0)),
            BodyKind::Dynamic,
        );
        run(&mut world, 60);
        let before = world.state(BodyId(2)).unwrap().position;
        world.recentre(BrickCoordinate::new(3, 0, 3));
        assert_eq!(world.origin(), BrickCoordinate::new(3, 0, 3));
        let after = world.state(BodyId(2)).unwrap().position;
        assert_eq!(after.brick(), before.brick());
        for axis in 0..3 {
            assert!((after.offset()[axis] - before.offset()[axis]).abs() < 1e-3);
        }
        world
            .set_state(BodyId(2), resting_at(at(8.0, 3.0, 8.0)))
            .unwrap();
        run(&mut world, 60);
        assert!(
            height_of(&world, 2).abs() < 0.02,
            "the terrain moved with the frame"
        );
    }

    #[test]
    fn what_is_asked_of_a_body_that_is_not_there_or_a_brick_that_is_too_far_is_an_error_and_changes_nothing()
     {
        let mut world = world_with_ground();
        let ghost = BodyId(9);
        let state = resting_at(at(0.0, 0.0, 0.0));
        let unknown = Err(BodyError::UnknownBody(ghost));
        assert_eq!(world.remove_body(ghost), unknown);
        assert_eq!(world.set_kind(ghost, BodyKind::Fixed), unknown);
        assert_eq!(world.set_state(ghost, state), unknown);
        assert_eq!(world.apply_force(ghost, [1.0; 3], None), unknown);
        assert_eq!(world.apply_torque(ghost, [1.0; 3]), unknown);
        assert_eq!(world.set_mass(ghost, Hull::cube(1).mass(1.0)), unknown);
        assert_eq!(
            world.set_brick(ghost, BrickCoordinate::ORIGIN, &mut &Hull::cube(1)),
            unknown
        );
        assert_eq!(world.state(ghost), None);
        assert!(!world.is_asleep(ghost));

        world.add_body(BodyId(1), state, BodyKind::Dynamic).unwrap();
        assert_eq!(
            world.add_body(BodyId(1), state, BodyKind::Fixed),
            Err(BodyError::DuplicateBody(BodyId(1)))
        );
        // A brick 2^27 bricks along is 2^32 voxels: beyond what the coordinates of a collider hold.
        assert_eq!(
            world.set_brick(
                BodyId(1),
                BrickCoordinate::new(1 << 27, 0, 0),
                &mut &Hull::cube(1)
            ),
            Err(BodyError::BrickOutOfRange)
        );
        assert_eq!(world.body_count(), 1);
        world.remove_body(BodyId(1)).unwrap();
        assert_eq!(world.body_count(), 0);
        run(&mut world, 3);
    }

    /// A scene that uses most of what there is: ground, ships of different shapes (one turned), a fixed one, forces, an edit.
    fn busy_scene() -> BodyWorld {
        let mut world = world_with_ground();
        let mut ell = Hull::cube(3);
        ell.0.remove(&VoxelCoordinate::new(2, 2, 2));
        ell.0.remove(&VoxelCoordinate::new(2, 2, 1));
        ship_of(
            &mut world,
            1,
            &ell,
            resting_at(at(8.0, 9.0, 8.0)),
            BodyKind::Dynamic,
        );
        ship_of(
            &mut world,
            2,
            &Hull::cube(4),
            BodyState {
                orientation: Orientation::from_yaw_pitch(0.7, -0.4),
                linear_velocity: [1.0, 0.0, 0.5],
                ..resting_at(at(-6.0, 14.0, 7.0))
            },
            BodyKind::Dynamic,
        );
        ship_of(
            &mut world,
            3,
            &Hull::cube(2),
            resting_at(at(9.0, 0.0, -9.0)),
            BodyKind::Fixed,
        );
        for tick in 0..120 {
            if tick % 7 == 0 {
                world
                    .apply_force(BodyId(1), [30.0, 0.0, -10.0], Some([1.0, 1.0, 1.0]))
                    .unwrap();
            }
            if tick == 60 {
                ell.0.remove(&VoxelCoordinate::new(0, 0, 0));
                world
                    .set_brick(BodyId(1), BrickCoordinate::ORIGIN, &mut &ell)
                    .unwrap();
            }
            world.step();
        }
        world
    }

    #[test]
    fn the_same_calls_give_the_same_bits_and_those_bits_are_the_same_on_every_machine() {
        let (first, second) = (busy_scene(), busy_scene());
        assert_eq!(first.state_hash(), second.state_hash());
        // Written down once; a change in it is a change in the simulation (and Windows and Linux must agree on it).
        // (Moved on purpose, Phase 38a: 8_592_658_610_343_128_269 before. Orientations are f64 (Phase 38a, 2026-10-23): a yaw and a pitch are turned into a quaternion in f64 and rounded once to the f32 the solver takes, which is not always the f32 the old arithmetic in f32 gave, to the last bit. Control: with `from_yaw_pitch` put back to the f32 arithmetic and nothing else changed, the old value came back; so this is the only thing that moved it.)
        assert_eq!(first.state_hash(), 13_989_029_006_912_403_583);
    }
}
