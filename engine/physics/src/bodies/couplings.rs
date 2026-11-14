//! Couplings: bodies held together at pivots.
//!
//! A *coupling* is two bodies and a list of pivot pairs, never one joint per block (`docs/decisions/joints-and-cars.md`): the choice of how the pivots are held is inside
//! this file, and nothing outside it may know. Today (owner, 2026-11-09) every pivot pair is a spherical impulse joint of Rapier; one pivot is a ball, two on a line a hinge, and
//! three that are not on a line (and are the same shape on both bodies) are one fixed joint, whose frames are worked out from the pivots (since 2026-11-14: three balls on three
//! pivots over-ask the solver, and a hit through a chain of them gave way; step 4c-5-A). The kinds the owner means to choose per ship later (a multibody with impulse joints for
//! loops, a multibody with a direct solver for loops) are other bodies of this same box.

use rapier3d::prelude::*;

use super::{BodyError, BodyId, BodyWorld};
use crate::real::vector;

/// Which coupling: a number the caller chooses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CouplingId(pub u64);

/// Where two bodies are held together: the place in the first body's own coordinates and the place in the second's, in voxels. They are the same place in the world when the
/// coupling is made, or the solver pulls the second body in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PivotPair {
    pub first: [f64; 3],
    pub second: [f64; 3],
}

/// How a coupling is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CouplingOptions {
    /// Whether the colliders of the two bodies meet each other. Off, they pass through each other, which is what a pair that overlaps at its pivot needs: with contacts on,
    /// two cubes that overlap by half a voxel at the pivot lock up (the contact pushes them apart where the joint holds them; 1,833 to 6 of energy in five seconds, measured in
    /// `tests/couplings.rs`), and `docs/history/loop-probe.md` found 1.2 MJ made from an overlap of half a metre in another scene.
    pub contacts_between_bodies: bool,
}

impl Default for CouplingOptions {
    /// The colliders meet (the decision of 2026-11-01: cars collide with each other).
    fn default() -> Self {
        Self {
            contacts_between_bodies: true,
        }
    }
}

/// How hard the solver works on a body (Rapier keeps these per body, and runs the largest of a connected set of bodies, the contacts and the joints between them, on all of
/// it). Nothing extra is the solver's ordinary work.
///
/// `extra_substeps` splits each step of the solver into more parts: the gap of a joint falls about four times for each doubling (`docs/history/loop-probe.md`, section 14), at a
/// cost in proportion. `extra_passes` goes over the constraints more often in each part, which an impact through a chain of joints needs and a swing does not.
///
/// The world gives every body some of this already ([`super::BodySettings::solver`], 15 passes): what is set for a body is in addition only where it is more.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SolverDials {
    pub extra_substeps: usize,
    pub extra_passes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CouplingError {
    #[error(transparent)]
    Body(#[from] BodyError),
    #[error("there already is a coupling {0:?}")]
    DuplicateCoupling(CouplingId),
    #[error("there is no coupling {0:?}")]
    UnknownCoupling(CouplingId),
    #[error("a coupling is between two bodies, and {0:?} is both")]
    SameBody(BodyId),
    #[error("a coupling needs a pivot")]
    NoPivots,
    #[error("a pivot is not a number")]
    PivotNotANumber,
}

/// How far the pivots of a coupling may be from being the same shape on both bodies, in voxels, for them to hold the pair rigidly.
const RIGID_TOLERANCE: f64 = 1e-6;

/// A frame fixed by three points: the first point's side is the x axis along the first two points, the z axis is across the plane of the three, and the place is the middle of
/// them. `None` if they are on a line (or one place), where no frame is fixed.
fn frame_of(points: [Vector; 3]) -> Option<Pose> {
    let along = points[1] - points[0];
    let across = points[2] - points[0];
    let normal = along.cross(across);
    if normal.length_squared() <= 1e-12 * along.length_squared() * across.length_squared() {
        return None;
    }
    let x = along.normalize();
    let z = normal.normalize();
    let y = z.cross(x);
    Some(Pose::from_parts(
        (points[0] + points[1] + points[2]) / 3.0,
        Rotation::from_mat3(&Matrix::from_cols(x, y, z)),
    ))
}

/// The frames, one in the coordinates of each body, that are the same frame in the world when the pivots of a coupling are met, if the pivots hold the pair rigidly: there are
/// three of them that are not on a line, and every pivot is at the same place in the frame its three fix on both bodies (the pivots are the same shape on both).
fn rigid_frames(pivots: &[PivotPair]) -> Option<(Pose, Pose)> {
    let first_points: Vec<Vector> = pivots.iter().map(|pivot| vector(pivot.first)).collect();
    let second_points: Vec<Vector> = pivots.iter().map(|pivot| vector(pivot.second)).collect();
    // The three that make the widest triangle (the best conditioned frame): the first pivot, the one farthest from it, and the one farthest from the line they make.
    let farthest = |from: Vector, points: &[Vector]| -> Option<usize> {
        (0..points.len()).max_by(|&a, &b| {
            (points[a] - from)
                .length_squared()
                .total_cmp(&(points[b] - from).length_squared())
        })
    };
    let second = farthest(first_points[0], &first_points)?;
    let line = first_points[second] - first_points[0];
    let third = (0..first_points.len()).max_by(|&a, &b| {
        let off = |index: usize| {
            (first_points[index] - first_points[0])
                .cross(line)
                .length_squared()
        };
        off(a).total_cmp(&off(b))
    })?;
    let chosen = [0, second, third];
    let first_frame = frame_of(chosen.map(|index| first_points[index]))?;
    let second_frame = frame_of(chosen.map(|index| second_points[index]))?;
    let matches = |point: usize| {
        let in_first = first_frame.inverse_transform_point(first_points[point]);
        let in_second = second_frame.inverse_transform_point(second_points[point]);
        (in_first - in_second).length() <= RIGID_TOLERANCE
    };
    (0..pivots.len())
        .all(matches)
        .then_some((first_frame, second_frame))
}

/// A coupling as the world has it.
pub(super) struct Coupling {
    pub(super) first: BodyId,
    pub(super) second: BodyId,
    pivots: Vec<PivotPair>,
    joints: Vec<ImpulseJointHandle>,
}

impl BodyWorld {
    /// Holds two bodies together at some pivots. The bodies need not be in any particular place: a pivot that is not at the same place in the world for both is pulled
    /// together by the solver, which is how a car that was moved by something outside it is brought back. Several couplings may join the same two bodies.
    pub fn couple(
        &mut self,
        id: CouplingId,
        first: BodyId,
        second: BodyId,
        pivots: &[PivotPair],
        options: CouplingOptions,
    ) -> Result<(), CouplingError> {
        if self.couplings.contains_key(&id) {
            return Err(CouplingError::DuplicateCoupling(id));
        }
        if first == second {
            return Err(CouplingError::SameBody(first));
        }
        let handle = |world: &Self, body: BodyId| {
            world
                .by_id
                .get(&body)
                .map(|found| found.handle)
                .ok_or(BodyError::UnknownBody(body))
        };
        let (first_handle, second_handle) = (handle(self, first)?, handle(self, second)?);
        if pivots.is_empty() {
            return Err(CouplingError::NoPivots);
        }
        if pivots.iter().any(|pivot| {
            !pivot
                .first
                .iter()
                .chain(&pivot.second)
                .all(|part| part.is_finite())
        }) {
            return Err(CouplingError::PivotNotANumber);
        }
        let joints = match rigid_frames(pivots) {
            // Three pivots that are not on a line hold the pair as one body: one joint that locks the whole frame, which the solver converges on far better than three balls on
            // the same points (three balls ask nine things of six degrees of freedom, and a hit through a chain of them gave way: `tests/settled_hit.rs`).
            Some((first_frame, second_frame)) => {
                let joint = FixedJointBuilder::new()
                    .local_frame1(first_frame)
                    .local_frame2(second_frame)
                    .contacts_enabled(options.contacts_between_bodies)
                    .build();
                vec![
                    self.impulse_joints
                        .insert(first_handle, second_handle, joint, true),
                ]
            }
            None => pivots
                .iter()
                .map(|pivot| {
                    let joint = SphericalJointBuilder::new()
                        .local_anchor1(vector(pivot.first))
                        .local_anchor2(vector(pivot.second))
                        .contacts_enabled(options.contacts_between_bodies)
                        .build();
                    self.impulse_joints
                        .insert(first_handle, second_handle, joint, true)
                })
                .collect(),
        };
        self.couplings.insert(
            id,
            Coupling {
                first,
                second,
                pivots: pivots.to_vec(),
                joints,
            },
        );
        Ok(())
    }

    /// Lets two bodies go. They keep the velocities they have.
    pub fn decouple(&mut self, id: CouplingId) -> Result<(), CouplingError> {
        let coupling = self
            .couplings
            .remove(&id)
            .ok_or(CouplingError::UnknownCoupling(id))?;
        for joint in coupling.joints {
            self.impulse_joints.remove(joint, true);
        }
        Ok(())
    }

    /// The couplings a body is in, in order of number.
    pub fn couplings_of(&self, body: BodyId) -> Vec<CouplingId> {
        self.couplings
            .iter()
            .filter(|(_, coupling)| coupling.first == body || coupling.second == body)
            .map(|(id, _)| *id)
            .collect()
    }

    /// How far apart the farthest pair of pivots of a coupling are in the world now, in voxels; `None` if there is no such coupling. The solver holds this to a small
    /// fraction of a voxel (about a millimetre at the game's settings); a gap far past that says something outside it moved a body.
    pub fn pivot_gap(&self, id: CouplingId) -> Option<f64> {
        let coupling = self.couplings.get(&id)?;
        let first = self.bodies.get(self.by_id.get(&coupling.first)?.handle)?;
        let second = self.bodies.get(self.by_id.get(&coupling.second)?.handle)?;
        Some(
            coupling
                .pivots
                .iter()
                .map(|pivot| {
                    let apart = *first.position() * vector(pivot.first)
                        - *second.position() * vector(pivot.second);
                    apart.length()
                })
                .fold(0.0, Real::max),
        )
    }

    /// How hard the solver works on a body, and so on every coupling it is in (see [`SolverDials`]): at least what the world gives every body, so asking for none puts a body back
    /// to that.
    pub fn set_solver_dials(&mut self, id: BodyId, dials: SolverDials) -> Result<(), BodyError> {
        self.by_id
            .get_mut(&id)
            .ok_or(BodyError::UnknownBody(id))?
            .dials = dials;
        self.apply_solver_dials(id);
        Ok(())
    }

    /// Tells the solver what a body is to be worked with: the larger of what was asked for it and what the world gives every body.
    pub(super) fn apply_solver_dials(&mut self, id: BodyId) {
        let Some(body) = self.by_id.get(&id) else {
            return;
        };
        let (asked, world, handle) = (body.dials, self.settings.solver, body.handle);
        let rapier = &mut self.bodies[handle];
        rapier.set_additional_solver_iterations(asked.extra_substeps.max(world.extra_substeps));
        rapier.set_additional_pgs_iterations(asked.extra_passes.max(world.extra_passes));
    }
}
