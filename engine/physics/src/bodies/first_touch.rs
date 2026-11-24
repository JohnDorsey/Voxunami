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
//! The same question tells the world that a hit has begun: from the moment a touch is found, cut at or not, or two shapes that touch or overlap are found going against each other,
//! with a contact (a chain that swings into the car it is joined to, a ship that rocks onto its other legs, a cube that slides) or with none yet (a cube that is sinking into terrain
//! slowly, to the depth at which a voxel contact engages), the pieces are solved with the settling passes of a hit ([`super::BodySettings::settling_passes`]) for
//! [`super::BodySettings::hit_duration`] from then, and with the one pass between hits otherwise (`docs/history/loop-probe.md`, section 21). What is found as a hit is a touch, or shapes
//! that move against each other, at a millimetre a second or more ([`HIT_SPEED_FLOOR`]; for a contact that is there, the speed of the two bodies against each other at its points,
//! and not a bound with the whole reach of a turning body in it: section 23), and not at what a step can take across the solver's margin, which was 1.2 voxels a second
//! at 3 steps a tick and 9.6 at 24, and made what a slow hit comes out as depend on how fast a ship far away went (a hit at 5 voxels a second kept 0.1986 on the chain at 24 steps
//! a tick, where 0.1999 was true); nor at what a tick can take across it (0.4 voxels a second): a cube that landed at 0.2 voxels a second went up at 0.05 and turned at 0.05, where the
//! world with the passes of a hit made at every step leaves nothing. The step is cut only at the touches that a step can take across the margin, as it was: cut at a slower one, a
//! chain swung at 70 and at 140 kilometres came to two different swings (0.5 apart, where they agree to 5e-10 without), for a cut is a break in the solving that a swinging chain makes
//! much of.
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

/// The slowest that two shapes go against each other, in voxels a second, for a touch or a contact that moves to be a hit: a millimetre a second. A voxel contact engages only when
/// the shapes have gone about 0.02 voxels into each other (the margin), so a cube that comes down slowly sinks that far, unresisted, over 0.02 voxels divided by its speed, and the
/// contact then pushes it out; with one settling pass that leaves a bounce and a turn of about 0.05, whatever the speed, where the passes of a hit leave under 0.0001 (ran: a cube
/// landing at 0.002 to 1 voxels a second, `engine_probe slowlanding`; a floor of 0.002 covers a landing at 0.002 and not under it). A body at rest goes at under 1e-12 voxels a second
/// (a stack of ten cubes wobbles at 3e-4 and turns at 4e-5, which with what the turn adds is over the floor: it is a hit all the time, as it was).
const HIT_SPEED_FLOOR: Real = 0.001;
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

/// What the question of a first touch found.
#[derive(Clone, Copy, Default)]
struct Findings {
    /// The first two shapes that are apart and closing to touch in the time, among those that a step can take across the solver's margin: what the step is cut at.
    touch: Option<Touch>,
    /// Whether a hit is on: two shapes that are apart will touch in the time, at [`HIT_SPEED_FLOOR`] or more (a step is cut at them only if a step can take them across the solver's
    /// margin, which is 1.2 voxels a second at 3 steps a tick and 9.6 at 24), or two shapes that touch, or overlap, are going against each other by as much, which is no first touch
    /// and has no time to cut at: with a contact (a chain that swings into the car it is joined to, a ship that lands tilted and rocks onto its other legs: the same two shapes, with
    /// a new point of contact; a cube that slides), or with none yet (a voxel contact engages only when the shapes have gone about 0.02 voxels into each other, and a cube that
    /// comes down slowly sinks that far in a second or more).
    hit: bool,
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
            let found = if self.settings.cut_at_first_touch && cuts < MOST_CUTS_A_STEP {
                self.first_touch(left)
            } else {
                Findings::default()
            };
            // (A touch found is a hit that has begun, cut at or not: one at the very end of a step is met in the step after, and that is solved as a hit too. So are shapes that touch
            // and go against each other, for as long as they do and for the time after.)
            if found.hit {
                self.hit_time_left = self.settings.hit_duration;
            }
            // (A touch at the very start is cut at too, by a first piece that is only the overshoot. One at the very end is left to the next step, which begins with it.)
            if let Some(touch) = found.touch
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

    /// When, within `within` seconds, the first pair of shapes that are apart and closing will touch, each going as its body goes, and the most they can be closing at, if any will,
    /// among those that a step can take across the solver's margin; and whether a hit is on (see [`Findings::hit`]).
    fn first_touch(&self, within: Real) -> Findings {
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
        let mut found = Findings::default();
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
                    // Too slow to be a hit. (A speed, and not what a step can take across the solver's margin, which is as short as the fastest body anywhere in the world makes it, so
                    // that what a slow hit comes out as does not depend on a ship far away: with the step, a hit at 5 voxels a second was not found when a tick was cut into 24 steps, and
                    // kept 0.1986 of its speed on a chain where 0.1999 was true.)
                    let relative = (first_velocity - second_velocity).length() + spin + second_spin;
                    if relative < HIT_SPEED_FLOOR {
                        continue;
                    }
                    // A contact that is there is not a first touch, and whether the shapes go against each other through it is a hit: judged by the speeds of the two bodies at the
                    // points of the contact, and not by the bound above, which for a body that turns has the whole of its reach in it (the ground of a planet that turns once in
                    // 12,566 seconds goes round at five voxels a second at its rim, and a ship that goes round with it was a hit in every piece: `docs/history/loop-probe.md`, section 23).
                    if let Some(pair) = self.narrow_phase.contact_pair(first_handle, second_handle)
                        && pair.has_any_active_contact()
                    {
                        if self.speed_across_contacts(pair) >= HIT_SPEED_FLOOR {
                            found.hit = true;
                        }
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
                    match time_of_touch(first, first_there, second, second_there, within) {
                        Cast::At(time) => {
                            found.hit = true;
                            // A step is cut at a touch only if a step can take the shapes across the margin at the speed they close at: a slower touch was never cut at, and a cut
                            // is a break in the solving that a swinging chain makes much of (cut at, two chains that were swung at 70 and 140 kilometres differed by 0.5 and not
                            // by 5e-10).
                            if relative * within >= prediction
                                && found.touch.is_none_or(|before| time < before.time)
                            {
                                found.touch = Some(Touch {
                                    time,
                                    closing_speed: relative,
                                });
                            }
                        }
                        // Touching or overlapping already, with no contact, and going against each other: a hit that is on, and no first touch.
                        Cast::Touching => found.hit = true,
                        Cast::Apart => {}
                    }
                }
            }
        }
        found
    }

    /// The farthest a point of a body is from its middle, by the boxes of its colliders (a body that does not turn has no use for it: 0).
    /// How fast the two sides of a contact go against each other at its points, the most of them: the speed of the one body at the point, as it turns and goes, minus the
    /// other's (a side that is no body, or is fixed, is still).
    fn speed_across_contacts(&self, pair: &ContactPair) -> Real {
        let velocity_at = |handle: ColliderHandle, point: Vector| {
            self.colliders[handle]
                .parent()
                .and_then(|body| self.bodies.get(body))
                .map_or(Vector::ZERO, |body| body.velocity_at_point(point))
        };
        let (first, second) = (
            &self.colliders[pair.collider1],
            &self.colliders[pair.collider2],
        );
        pair.solver_manifolds()
            .iter()
            .flat_map(|manifold| manifold.points.iter())
            .map(|contact| {
                let point = (*first.position() * contact.local_p1
                    + *second.position() * contact.local_p2)
                    * 0.5;
                (velocity_at(pair.collider1, point) - velocity_at(pair.collider2, point)).length()
            })
            .fold(0.0, Real::max)
    }

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

/// What a straight cast of two shapes found.
enum Cast {
    /// They do not touch in the time.
    Apart,
    /// They touch or overlap now.
    Touching,
    /// They are apart now and touch at this time.
    At(Real),
}

/// When, within `within` seconds, two shapes touch, each going at the speed it is given: a straight cast.
fn time_of_touch(
    first: &Collider,
    first_velocity: Vector,
    second: &Collider,
    second_velocity: Vector,
    within: Real,
) -> Cast {
    let options = ShapeCastOptions {
        max_time_of_impact: within,
        target_distance: 0.0,
        stop_at_penetration: true,
        compute_impact_geometry_on_penetration: false,
    };
    let Some(hit) = query::cast_shapes(
        first.position(),
        first_velocity,
        first.shape(),
        second.position(),
        second_velocity,
        second.shape(),
        options,
    )
    .ok()
    .flatten() else {
        return Cast::Apart;
    };
    // A pair that touches or overlaps at the start is not a first touch.
    if hit.status == ShapeCastStatus::PenetratingOrWithinTargetDist {
        Cast::Touching
    } else {
        Cast::At(hit.time_of_impact)
    }
}
