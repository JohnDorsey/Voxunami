//! Couplings: bodies held together at pivots.
//!
//! A *coupling* is two bodies and a list of pivot pairs, never one joint per block (`docs/decisions/joints-and-cars.md`): the choice of how the pivots are held is inside
//! this file, and nothing outside it may know. Today (owner, 2026-11-09) every pivot pair is a spherical impulse joint of Rapier; one pivot is a ball, two on a line a hinge,
//! three not on a line a rigid joint. The kinds the owner means to choose per ship later (a multibody with impulse joints for loops, a multibody with a direct solver for
//! loops) are other bodies of this same box.

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

/// How hard the solver works on a body (Rapier keeps these per body, for the whole island the body is in). Nothing extra is the solver's ordinary work.
///
/// `extra_substeps` splits each step of the solver into more parts: the gap of a joint falls about four times for each doubling (`docs/history/loop-probe.md`, section 14), at a
/// cost in proportion. `extra_passes` goes over the constraints more often in each part, which an impact through a chain of joints needs and a swing does not.
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
        let joints = pivots
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
            .collect();
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

    /// Whether a body may go to sleep when it is still. A body that may not is awake for good (one that is asleep is woken): the cars of a mechanism must not stop because they fell
    /// asleep, mid-swing or at rest, and Rapier would put them to sleep together as soon as they were slow enough.
    pub fn set_can_sleep(&mut self, id: BodyId, can_sleep: bool) -> Result<(), BodyError> {
        let handle = self
            .by_id
            .get(&id)
            .ok_or(BodyError::UnknownBody(id))?
            .handle;
        let body = &mut self.bodies[handle];
        if can_sleep {
            // (Only a body that was kept awake gets the ordinary thresholds back; one that sleeps already is left as it is.)
            if body.activation().normalized_linear_threshold < 0.0 {
                *body.activation_mut() = RigidBodyActivation::active();
            }
        } else {
            *body.activation_mut() = RigidBodyActivation::cannot_sleep();
            body.wake_up(true);
        }
        Ok(())
    }

    /// How hard the solver works on a body, and so on every coupling it is in (see [`SolverDials`]).
    pub fn set_solver_dials(&mut self, id: BodyId, dials: SolverDials) -> Result<(), BodyError> {
        let handle = self
            .by_id
            .get(&id)
            .ok_or(BodyError::UnknownBody(id))?
            .handle;
        let body = &mut self.bodies[handle];
        body.set_additional_solver_iterations(dials.extra_substeps);
        body.set_additional_pgs_iterations(dials.extra_passes);
        Ok(())
    }
}
