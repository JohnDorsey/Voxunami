//! Cutting a step where two bodies first touch.
//!
//! The contact of two voxel shapes is found only once they overlap, so without a cut a body goes a whole step's way into another before the solver sees the touch: 0.7 voxels into
//! terrain at 60 voxels a second, 0.6 into another body (`docs/history/loop-probe.md`, section 20). What a hit does also depended on the moment of the step at which the shapes
//! touch, because the solver meets a touch as it finds it, at the start of a step, with the speeds the bodies have then: a stranger on a rigid chain of four kept 0.04 to 0.14 of
//! its speed over twelve moments of a step, where momentum says 0.2 (section 18). With the settling passes ([`super::BodySettings::settling_passes`], section 20) it no longer
//! does, even with no cut (0.1990 to 0.2000 at 5 to 150 voxels a second), and the cut is for the depth. So a step is cut where the first two shapes that are apart and closing
//! touch, each going as its body goes: the first piece ends with the shapes just touching, and the piece after it is the one that meets the touch, whatever the moment of the step
//! the touch was at. Three things make that come out true, each found by a measurement (`docs/history/spikes/loops`, `engine_probe landing|levers|startphases|chainmoments`; the
//! numbers of the first two were taken with one settling pass, and without the first one the settling passes do not make it right: a touch at the start of a step kept 0.1966):
//!
//! - **The first piece is solved with no prediction distance.** A pair that is within the solver's margin (0.02 voxel) when a piece begins has a contact that acts in it, before
//!   the shapes touch, and in a piece that is cut short it acts in a step of a hundred-thousandth of a second: the cars of a chain were turned at 0.5 to 1.4 radians a second by
//!   it, and the stranger kept 0.177 of its speed where 0.2 is true. With none, a touch that falls anywhere in a step, the very start of it included, keeps 0.1987 to 0.1988.
//! - **The first piece goes a hundred-thousandth of a voxel past the touch.** The contact of two voxel shapes is found only once they overlap (by about 1e-14 voxel: a cube that is
//!   1e-15 above terrain, or at 0, or at any gap within the margin, is not caught in the step, and one that is 1e-14 into it is); ending at the touch would leave the solver blind
//!   to it. (More is worse: the contact then begins with an overlap that has to be pushed out, and with six centimetres the stranger kept 0.163.)
//! - **The piece that meets the touch is what is left of the step, whatever its length.** It was made one length once (a twelfth of a tick, 4.2 milliseconds): a hit that was solved
//!   in a shorter piece bounced and turned more, up to 3.3 voxels a second and 4.8 radians a second for a cube that landed at 150 voxels a second. The length was not the cause. The soft
//!   contact lets more through in a shorter piece, and the one pass that Rapier makes to take that away again left part of it, unevenly over the points of a contact: with
//!   [`super::BodySettings::settling_passes`] of them (16) the length of the piece makes no difference that is seen (`docs/history/loop-probe.md`, section 20).
//!
//! The question is asked of the broad phase first and the shapes only for what is near: a voxel shape cast against another costs a third of a millisecond when they are about to
//! touch and a tenth of a microsecond when they are not (`engine_probe castcost`), and a pair that has a contact already is not asked. A step in which nothing is about to touch is
//! not cut, and is what it was bit for bit.
//!
//! What a body that turns does: the cast moves each shape at one speed, that of the place where the boxes of the two overlap, so a touch there is found as it is and one elsewhere
//! on a body that turns fast only about (the cast that follows a turn, `cast_shapes_nonlinear`, finds nothing when the shape that turns is a voxel one, ran, and parry measures no
//! distance between voxel shapes to follow it with). A bar of 12 voxels turning at up to 2 radians a second that hits a cube did not depend on the moment of the step to begin with
//! (0.2 percent), and with the cut it did not, but for one moment, 1.3 percent off.

use rapier3d::parry::bounding_volume::BoundingVolume;
use rapier3d::parry::query::{self, ShapeCastOptions, ShapeCastStatus};
use rapier3d::prelude::*;

use super::{BodyWorld, MOST_A_STEP_MOVES};

/// The most pieces a step is cut into by touches, less one: a heap of bodies that all touch in one step must not make a step into dozens, and each piece is a whole step of the
/// solver.
const MOST_CUTS_A_STEP: usize = 3;
/// How far the shapes are let go into each other at the end of the first piece, in prediction distances: a hundred-thousandth of a voxel (1e-5) at the solver's margin of 0.02.
const OVERSHOOT_IN_PREDICTIONS: Real = 0.0005;

/// When two shapes first touch, and the most they can be closing at (the relative speed of the bodies and what their turning adds, which a grazing approach does not reach).
#[derive(Clone, Copy)]
struct Touch {
    time: Real,
    closing_speed: Real,
}

impl BodyWorld {
    /// Moves everything `time` seconds, as one step of the solver, or as pieces of one that end where two shapes first touch (up to [`MOST_CUTS_A_STEP`] cuts), if the settings ask
    /// for that ([`super::BodySettings::cut_at_first_touch`]).
    pub(super) fn solve_in_pieces(&mut self, time: Real) {
        let overshoot = OVERSHOOT_IN_PREDICTIONS * self.parameters.prediction_distance();
        let mut left = time;
        let mut cuts = 0;
        while left > 0.0 {
            let mut piece = left;
            // (A touch at the very start is cut at too, by a first piece that is only the overshoot. One at the very end is left to the next step, which begins with it.)
            if self.settings.cut_at_first_touch
                && cuts < MOST_CUTS_A_STEP
                && let Some(touch) = self.first_touch(left)
                && touch.time + overshoot / touch.closing_speed < left
            {
                piece = touch.time + overshoot / touch.closing_speed;
                cuts += 1;
                self.cuts += 1;
                // The piece that ends at a touch: no contact may act before it.
                let margin = self.parameters.normalized_prediction_distance;
                self.parameters.normalized_prediction_distance = 0.0;
                self.solve(piece);
                self.parameters.normalized_prediction_distance = margin;
            } else {
                self.solve(piece);
            }
            left = if piece == left { 0.0 } else { left - piece };
        }
    }

    /// When, within `within` seconds, the first pair of shapes that are apart and closing will touch, each going as its body goes, and the most they can be closing at; `None` if
    /// none will.
    fn first_touch(&self, within: Real) -> Option<Touch> {
        let prediction = self.parameters.prediction_distance();
        let scene = self.broad_phase.as_query_pipeline(
            self.narrow_phase.query_dispatcher(),
            &self.bodies,
            &self.colliders,
            QueryFilter::default(),
        );
        // The broad phase last saw each collider where it was at the start of the last step (and a body that was put somewhere since is seen at the place it left): a step's
        // motion of the fastest body is what it can have moved, which is bounded by the rule that cuts a tick into steps.
        let stale = MOST_A_STEP_MOVES;
        let mut earliest: Option<Touch> = None;
        for (handle, body) in self.bodies.iter() {
            if !body.is_dynamic() || body.is_sleeping() {
                continue;
            }
            let reach = self.reach_of(body);
            let spin = body.angvel().length() * reach;
            for &first_handle in body.colliders() {
                let first = &self.colliders[first_handle];
                if first.is_sensor() {
                    continue;
                }
                let first_velocity = body.velocity_at_point(first.position().translation);
                // Where the shape can be in the time: its box now and its box then, a little more for what its turning sweeps.
                let start = first.compute_aabb();
                let moved = Aabb::new(
                    start.mins + first_velocity * within,
                    start.maxs + first_velocity * within,
                );
                let swept = start
                    .merged(&moved)
                    .loosened(spin * within + stale + prediction);
                for (second_handle, second) in scene.intersect_aabb_conservative(swept) {
                    if second_handle == first_handle || second.is_sensor() {
                        continue;
                    }
                    let other = second.parent();
                    if other == Some(handle) {
                        continue;
                    }
                    // Bodies that are held together by a joint whose contacts are off do not meet.
                    if let Some(other) = other
                        && self
                            .impulse_joints
                            .joints_between(handle, other)
                            .any(|(_, joint)| !joint.data.contacts_enabled)
                    {
                        continue;
                    }
                    let other_body = other
                        .and_then(|handle| self.bodies.get(handle))
                        .filter(|other| other.is_dynamic_or_kinematic());
                    let (second_velocity, second_spin) = match other_body {
                        Some(other) => (
                            other.velocity_at_point(second.position().translation),
                            other.angvel().length() * self.reach_of(other),
                        ),
                        None => (Vector::ZERO, 0.0),
                    };
                    // Closing too slowly to cross the solver's own margin in this time: its contact holds it.
                    let relative = (first_velocity - second_velocity).length() + spin + second_spin;
                    if relative * within < prediction {
                        continue;
                    }
                    // A contact that is there is not a first touch.
                    if self
                        .narrow_phase
                        .contact_pair(first_handle, second_handle)
                        .is_some_and(|pair| pair.has_any_active_contact())
                    {
                        continue;
                    }
                    // The boxes as they are now (the broad phase's are a step old), swept by the relative motion.
                    let second_box = second.compute_aabb();
                    if !start
                        .loosened(relative * within + prediction)
                        .intersects(&second_box)
                    {
                        continue;
                    }
                    // Where they are to meet: the middle of where the boxes overlap, which is the place whose speed counts for a body that turns.
                    let meeting = Aabb::new(
                        start.mins.max(second_box.mins),
                        start.maxs.min(second_box.maxs),
                    )
                    .center();
                    let (first_there, second_there) = (
                        body.velocity_at_point(meeting),
                        other_body.map_or(Vector::ZERO, |other| other.velocity_at_point(meeting)),
                    );
                    if let Some(time) =
                        time_of_touch(first, first_there, second, second_there, within)
                        && earliest.is_none_or(|before| time < before.time)
                    {
                        earliest = Some(Touch {
                            time,
                            closing_speed: relative,
                        });
                    }
                }
            }
        }
        earliest
    }

    /// The farthest a point of a body is from its middle, by the boxes of its colliders (a body that does not turn has no use for it: 0).
    pub(super) fn reach_of(&self, body: &RigidBody) -> Real {
        if body.angvel() == Vector::ZERO {
            return 0.0;
        }
        let middle = body.translation();
        body.colliders()
            .iter()
            .map(|&handle| {
                let aabb = self.colliders[handle].compute_aabb();
                (aabb.center() - middle).length() + aabb.half_extents().length()
            })
            .fold(0.0, Real::max)
    }
}

/// When, within `within` seconds, two shapes that are apart touch, each going at the speed it is given: a straight cast.
fn time_of_touch(
    first: &Collider,
    first_velocity: Vector,
    second: &Collider,
    second_velocity: Vector,
    within: Real,
) -> Option<Real> {
    let options = ShapeCastOptions {
        max_time_of_impact: within,
        target_distance: 0.0,
        stop_at_penetration: true,
        compute_impact_geometry_on_penetration: false,
    };
    let hit = query::cast_shapes(
        first.position(),
        first_velocity,
        first.shape(),
        second.position(),
        second_velocity,
        second.shape(),
        options,
    )
    .ok()
    .flatten()?;
    // A pair that touches or overlaps at the start is not a first touch.
    (hit.status != ShapeCastStatus::PenetratingOrWithinTargetDist).then_some(hit.time_of_impact)
}
