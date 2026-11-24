//! What the contact ghost (`ghost.rs`) needs of a world of bodies: a step that is written down as it is taken and one that is taken again from that record, the difference
//! that two worlds' bodies have, that difference given to a body, the copy of a body into another world, and the labels of the sides that bodies are on.
//!
//! **A step that is taken again.** The ghost has two worlds that must step *alike*: the one in which the sides touch decides how a step is cut (where two bodies first touch, how
//! many solver steps, whether a piece is a hit with the passes of one), and the one in which they do not takes those pieces as they were (`docs/history/loop-probe.md`, section
//! 18: cuts at a touch are made in both ghosts alike, or what the difference holds is the cut and not the contact).

use std::collections::BTreeMap;

use rapier3d::parry::bounding_volume::BoundingVolume;
use rapier3d::prelude::*;

use super::{
    BodyColliders, BodyError, BodyId, BodyState, BodyWorld, CouplingId, CouplingOptions, PivotPair,
};
use crate::real::{array, vector};

/// How many times over a body's speed at the start of a tick is allowed to be, for the way it can go in the tick (a force, or a joint, can add to it in the tick).
const GATE_SPEED_FACTOR: Real = 2.0;
/// How far, in voxels, beyond the way a body can go in a tick two bodies of different sides must be for the ghost to leave them alone: more than a force that is a hundred times the
/// pull of a voxel's gravity moves a body in a tick (half of 800 voxels a second a second, times the tick squared, is one).
const GATE_SLACK: Real = 1.0;

/// One piece of a step, as it was solved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Piece {
    /// How long it was, in seconds.
    pub(super) length: Real,
    /// Whether it was solved with no prediction distance (the piece that ends where two bodies first touch: no contact may act before it ends).
    pub(super) without_margin: bool,
    /// Whether it was solved as a hit (with the settling passes of one).
    pub(super) as_a_hit: bool,
}

/// How a tick was stepped: the pieces it was solved in, one after another, and whether any of them had a contact between bodies on different sides.
#[derive(Clone, Debug, PartialEq)]
pub struct StepPlan {
    pieces: Vec<Piece>,
    crossed: bool,
}

impl StepPlan {
    /// How many pieces the tick was solved in (the solver steps it was cut into, and the pieces that touches cut them into).
    pub fn pieces(&self) -> usize {
        self.pieces.len()
    }

    /// Whether, in any piece, two bodies that are on different sides had a contact the solver worked on (the ghost puts each body on one).
    pub fn crossed(&self) -> bool {
        self.crossed
    }
}

/// What to add to a body's place and speed to make it what another world's copy of it is: the place and the turn as a difference (the turn as a rotation that is put before the
/// body's own), and the speeds as differences. The sides of a frame that differ only by where they are and how fast they go (a shift and a speed) give the same difference.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Correction {
    /// How far to move the body, in voxels.
    pub position: [f64; 3],
    /// The rotation to turn the body by (before its own orientation), `[x, y, z, w]`.
    pub rotation: [f64; 4],
    pub linear_velocity: [f64; 3],
    pub angular_velocity: [f64; 3],
}

impl BodyWorld {
    /// How many steps of the solver this tick is cut into, by the fastest body there is now (see `step`).
    pub fn steps_this_tick(&self) -> u32 {
        self.steps_for_this_tick()
    }

    /// Moves everything one tick like [`BodyWorld::step`], cut into `steps` steps (and into pieces where two bodies first touch, if the settings say so), and writes down the
    /// pieces that it was solved in: another world takes the same tick with [`BodyWorld::step_replaying`].
    pub fn step_recording(&mut self, steps: u32) -> StepPlan {
        self.apply_sleep_rule();
        let steps = steps.max(1);
        self.recorder = Some(Vec::new());
        self.crossed = false;
        let length = self.settings.time_step / Real::from(steps);
        for _ in 0..steps {
            self.solve_in_pieces(length);
        }
        self.forget_forces();
        StepPlan {
            pieces: self.recorder.take().unwrap_or_default(),
            crossed: self.crossed,
        }
    }

    /// Moves everything one tick in the pieces that another world's step was solved in (a [`StepPlan`]), as hits where they were hits, with no prediction distance where they
    /// had none: nothing is asked of this world about where bodies first touch, or how many steps a tick is cut into.
    pub fn step_replaying(&mut self, plan: &StepPlan) {
        self.apply_sleep_rule();
        for piece in &plan.pieces {
            let margin = self.parameters.normalized_prediction_distance;
            if piece.without_margin {
                self.parameters.normalized_prediction_distance = 0.0;
            }
            self.forced_hit = Some(piece.as_a_hit);
            self.solve(piece.length);
            self.parameters.normalized_prediction_distance = margin;
        }
        self.forced_hit = None;
        self.forget_forces();
    }

    /// Forgets the forces and torques that were applied for the tick.
    pub(super) fn forget_forces(&mut self) {
        for (_, body) in self.bodies.iter_mut() {
            body.reset_forces(false);
            body.reset_torques(false);
        }
    }

    /// Whether any two bodies on different sides have a contact that the solver works on, as the last piece found them.
    pub(super) fn contact_across_sides(&self) -> bool {
        let side = |collider: ColliderHandle| {
            self.colliders
                .get(collider)
                .and_then(Collider::parent)
                .and_then(|body| self.bodies.get(body))
                .map(|body| body.user_data)
        };
        self.narrow_phase.contact_pairs().any(|pair| {
            pair.has_any_active_contact()
                && matches!(
                    (side(pair.collider1), side(pair.collider2)),
                    (Some(first), Some(second)) if first != 0 && second != 0 && first != second
                )
        })
    }

    /// Whether two bodies that are on different sides could touch in the tick (see `step`), counting how far each can go in it, which is a fact of where it is and how it moves now:
    /// its box as it stands, loosened by the way it goes in the tick (its speed and what its turn sweeps, [`GATE_SPEED_FACTOR`] times over, for what a tick can add to a speed that
    /// is known at its start) and by [`GATE_SLACK`] and the solver's margin. If no two boxes of different sides meet, nothing of one side can touch the other in the tick, and a step
    /// of the world is a step of nothing: its result is that no contact between sides was met. (`docs/history/loop-probe.md`, section 24: the ghost's copies never sleep, and
    /// stepping them cost twenty-five times what the mechanism's own world costs, asleep.)
    pub(crate) fn could_touch_across_sides(&self) -> bool {
        let time = self.settings.time_step;
        let prediction = self.parameters.prediction_distance();
        let mut boxes: Vec<(u32, Aabb)> = Vec::new();
        for body in self.by_id.values() {
            let Some(side) = body.side else {
                continue;
            };
            let rapier = &self.bodies[body.handle];
            // (A body that was just put somewhere has its colliders where they were until the next step, so the box is made from the body's place.)
            let pose = *rapier.position();
            let mut merged: Option<Aabb> = None;
            for &handle in rapier.colliders() {
                let collider = &self.colliders[handle];
                let Some(local) = collider.position_wrt_parent() else {
                    continue;
                };
                let aabb = collider.shape().compute_aabb(&(pose * *local));
                merged = Some(merged.map_or(aabb, |before| before.merged(&aabb)));
            }
            let Some(aabb) = merged else {
                continue;
            };
            let reach = (aabb.center() - pose.translation).length() + aabb.half_extents().length();
            let travel = (rapier.linvel().length() + rapier.angvel().length() * reach)
                * time
                * GATE_SPEED_FACTOR;
            boxes.push((side, aabb.loosened(travel + prediction + GATE_SLACK)));
        }
        boxes.iter().enumerate().any(|(index, (side, aabb))| {
            boxes[index + 1..]
                .iter()
                .any(|(other, other_aabb)| other != side && aabb.intersects(other_aabb))
        })
    }

    /// Puts a body on a side (a number from 0 to 31) for the contact ghost: contacts between bodies on different sides are what [`StepPlan::crossed`] says. With
    /// `meet_only_their_own` the body's colliders meet only those of bodies on the same side, and pass through the others.
    pub(crate) fn set_side(
        &mut self,
        id: BodyId,
        side: u32,
        meet_only_their_own: bool,
    ) -> Result<(), BodyError> {
        let body = self.by_id.get_mut(&id).ok_or(BodyError::UnknownBody(id))?;
        body.side = Some(side);
        let handle = body.handle;
        self.bodies[handle].user_data = u128::from(side) + 1;
        if meet_only_their_own {
            let group = Group::from_bits_truncate(1 << side);
            let groups = InteractionGroups::new(group, group, InteractionTestMode::And);
            let colliders: Vec<ColliderHandle> = self.bodies[handle].colliders().to_vec();
            for collider in colliders {
                if let Some(collider) = self.colliders.get_mut(collider) {
                    collider.set_collision_groups(groups);
                }
            }
        }
        Ok(())
    }

    /// A number that goes up each time something about a body changes that a copy of it must follow: its colliders, its mass, its kind, its dials.
    pub(crate) fn version_of(&self, id: BodyId) -> Option<u64> {
        self.by_id.get(&id).map(|body| body.version)
    }

    /// The colliders of a body, to give to a copy of it in another world (the shapes are shared, not copied; an edit of one world's body does not touch the other's).
    pub(crate) fn copy_colliders(&self, id: BodyId) -> Result<BodyColliders, BodyError> {
        let body = self.by_id.get(&id).ok_or(BodyError::UnknownBody(id))?;
        let voxels = body
            .voxels
            .and_then(|handle| self.colliders.get(handle))
            .map(|collider| collider.shared_shape().clone());
        let partial: BTreeMap<_, _> = body
            .partial
            .iter()
            .filter_map(|(brick, handle)| {
                self.colliders
                    .get(*handle)
                    .map(|collider| (*brick, collider.shared_shape().clone()))
            })
            .collect();
        Ok(BodyColliders {
            voxels,
            filled: body.filled.clone(),
            partial,
        })
    }

    /// Makes a body in `other` of what this world's body is (its kind, colliders, mass and solver dials), in the place `state` says, and one that never sleeps there.
    pub(crate) fn copy_body_into(
        &self,
        id: BodyId,
        other: &mut BodyWorld,
        state: BodyState,
    ) -> Result<(), BodyError> {
        let body = self.by_id.get(&id).ok_or(BodyError::UnknownBody(id))?;
        other.add_body(id, state, body.kind)?;
        other.set_can_sleep(id, false)?;
        self.recopy_body_into(id, other)
    }

    /// Makes the copy of a body that is in `other` what this world's body is now (its kind, colliders, mass and solver dials; not where it is).
    pub(crate) fn recopy_body_into(
        &self,
        id: BodyId,
        other: &mut BodyWorld,
    ) -> Result<(), BodyError> {
        let body = self.by_id.get(&id).ok_or(BodyError::UnknownBody(id))?;
        other.set_kind(id, body.kind)?;
        other.give_colliders(id, self.copy_colliders(id)?)?;
        if let Some(mass) = body.mass {
            other.set_mass(id, mass)?;
        }
        other.set_solver_dials(id, body.dials)
    }

    /// The force and torque that are applied to a body for the tick now, as the total of what was applied (`apply_force`, at a point or not, `apply_torque`, and the steady ones).
    pub(crate) fn pending_forces(&self, id: BodyId) -> Option<([f64; 3], [f64; 3])> {
        let body = self.bodies.get(self.by_id.get(&id)?.handle)?;
        Some((array(body.user_force()), array(body.user_torque())))
    }

    /// Gives a body the force and torque of a copy of it (the same as the copy had applied, exactly).
    pub(crate) fn set_pending_forces(
        &mut self,
        id: BodyId,
        force: [f64; 3],
        torque: [f64; 3],
    ) -> Result<(), BodyError> {
        let handle = self
            .by_id
            .get(&id)
            .ok_or(BodyError::UnknownBody(id))?
            .handle;
        let body = &mut self.bodies[handle];
        body.reset_forces(false);
        body.reset_torques(false);
        if force != [0.0; 3] {
            body.add_force(vector(force), false);
        }
        if torque != [0.0; 3] {
            body.add_torque(vector(torque), false);
        }
        Ok(())
    }

    /// What a coupling is: the two bodies, its pivots and how it was made.
    pub(crate) fn coupling_parts(
        &self,
        id: CouplingId,
    ) -> Option<(BodyId, BodyId, Vec<PivotPair>, CouplingOptions)> {
        let coupling = self.couplings.get(&id)?;
        Some((
            coupling.first,
            coupling.second,
            coupling.pivots.clone(),
            coupling.options,
        ))
    }

    /// What this world's body has that another world's copy of it has not, as a [`Correction`] to give the other's: `None` if the two are the same to the bit.
    pub(crate) fn correction_against(&self, other: &BodyWorld, id: BodyId) -> Option<Correction> {
        let here = self.bodies.get(self.by_id.get(&id)?.handle)?;
        let there = other.bodies.get(other.by_id.get(&id)?.handle)?;
        let position = array(here.translation() - there.translation());
        let rotation = (*here.rotation() * there.rotation().inverse()).normalize();
        let linear_velocity = array(here.linvel() - there.linvel());
        let angular_velocity = array(here.angvel() - there.angvel());
        let turned = here.rotation() != there.rotation();
        if position == [0.0; 3]
            && linear_velocity == [0.0; 3]
            && angular_velocity == [0.0; 3]
            && !turned
        {
            return None;
        }
        Some(Correction {
            position,
            rotation: rotation.to_array(),
            linear_velocity,
            angular_velocity,
        })
    }

    /// Adds a [`Correction`] to a body: it is moved and turned and its speeds changed by it, and it is woken.
    pub fn apply_correction(
        &mut self,
        id: BodyId,
        correction: &Correction,
    ) -> Result<(), BodyError> {
        let handle = self
            .by_id
            .get(&id)
            .ok_or(BodyError::UnknownBody(id))?
            .handle;
        let body = &mut self.bodies[handle];
        let pose = *body.position();
        let [x, y, z, w] = correction.rotation;
        let turn = Rotation::from_xyzw(x, y, z, w).normalize();
        body.set_position(
            Pose::from_parts(
                pose.translation + vector(correction.position),
                (turn * pose.rotation).normalize(),
            ),
            true,
        );
        body.set_linvel(body.linvel() + vector(correction.linear_velocity), true);
        body.set_angvel(body.angvel() + vector(correction.angular_velocity), true);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use voxel_space::{BrickCoordinate, Orientation, WorldPosition};

    use super::*;

    fn world_with_a_body_at(state: BodyState) -> BodyWorld {
        let mut world = BodyWorld::new(
            BrickCoordinate::ORIGIN,
            super::super::BodySettings::default(),
        );
        world
            .add_body(BodyId(1), state, super::super::BodyKind::Dynamic)
            .unwrap();
        world
    }

    fn state_turned(
        about: [f64; 3],
        angle: f64,
        at: [f64; 3],
        velocity: [f64; 3],
        spin: [f64; 3],
    ) -> BodyState {
        let length = (about[0] * about[0] + about[1] * about[1] + about[2] * about[2]).sqrt();
        let (sine, cosine) = (libm::sin(angle / 2.0) / length, libm::cos(angle / 2.0));
        BodyState {
            position: WorldPosition::new(BrickCoordinate::ORIGIN, at),
            orientation: Orientation::from_components([
                about[0] * sine,
                about[1] * sine,
                about[2] * sine,
                cosine,
            ])
            .unwrap(),
            linear_velocity: velocity,
            angular_velocity: spin,
        }
    }

    #[test]
    fn a_correction_between_two_bodies_made_to_the_one_is_the_other() {
        // Turns of more than a radian about different axes, which do not commute: the rotation that takes the one body's turn to the other's is one way about and not the other.
        let one = world_with_a_body_at(state_turned(
            [1.0, 2.0, 3.0],
            1.3,
            [4.0, 5.0, 6.0],
            [7.0, -8.0, 9.0],
            [0.5, -0.25, 2.0],
        ));
        let mut other = world_with_a_body_at(state_turned(
            [3.0, -1.0, 2.0],
            2.1,
            [-1.0, 2.0, 0.5],
            [1.0, 2.0, 3.0],
            [-1.0, 0.0, 0.25],
        ));
        let correction = one
            .correction_against(&other, BodyId(1))
            .expect("they differ");
        other.apply_correction(BodyId(1), &correction).unwrap();
        let (wanted, got) = (
            one.state(BodyId(1)).unwrap(),
            other.state(BodyId(1)).unwrap(),
        );
        // (A rotation and its negative are the same turn.)
        let (a, b) = (
            wanted.orientation.components(),
            got.orientation.components(),
        );
        let dot: f64 = (0..4).map(|axis| a[axis] * b[axis]).sum();
        assert!(
            (dot.abs() - 1.0).abs() < 1e-12,
            "the orientations are {a:?} and {b:?}"
        );
        for axis in 0..3 {
            assert!((wanted.linear_velocity[axis] - got.linear_velocity[axis]).abs() < 1e-12);
            assert!((wanted.angular_velocity[axis] - got.angular_velocity[axis]).abs() < 1e-12);
            assert!((wanted.position.offset()[axis] - got.position.offset()[axis]).abs() < 1e-12);
        }
    }

    #[test]
    fn two_bodies_that_are_the_same_to_the_bit_have_no_correction() {
        let state = state_turned(
            [1.0, 2.0, 3.0],
            1.3,
            [4.0, 5.0, 6.0],
            [7.0, -8.0, 9.0],
            [0.5, -0.25, 2.0],
        );
        let (one, other) = (world_with_a_body_at(state), world_with_a_body_at(state));
        assert_eq!(one.correction_against(&other, BodyId(1)), None);
    }
}
