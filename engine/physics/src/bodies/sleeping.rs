//! When a body may sleep.
//!
//! Rapier puts a body to sleep when it has been slow for half a second, and takes its velocity away. That is a saving for a stack of bodies at rest on the ground, and a fault
//! for a body that touches nothing: the ship audit ran a free ship at 0.1 m/s that stopped dead, a ship turning at 0.002 rad/s that stopped turning, and one released at rest in
//! a pull of 0.15 m/s/s that never fell. So the rule of this world (owner, 2026-11-10, `docs/decisions/joints-and-cars.md`, "Sleeping") is that **a body sleeps only while it
//! touches something**: another body or the terrain. Before each step every dynamic body that touches nothing is made unable to sleep (and woken, if it was asleep), and one
//! that touches something gets Rapier's own thresholds back. A body that touches something is as it always was: it sleeps once it has been still for the half second.
//!
//! What "touches" means is Rapier's own: a contact pair with a contact the solver works on, as found in the last step. A body just made has had no step, so it touches nothing
//! until its first one has been made; a body that rests on the ground sleeps one tick later than Rapier's rule alone would have it.

use std::collections::HashSet;

use rapier3d::prelude::*;

use super::{BodyError, BodyId, BodyKind, BodyWorld};

impl BodyWorld {
    /// Whether a body may go to sleep when it is still, apart from the rule that it never does while it touches nothing. A body that may not is awake for good (one that is
    /// asleep is woken): the cars of a mechanism must not stop because they fell asleep, mid-swing or at rest, and Rapier would put them to sleep together as soon as they were
    /// slow enough. (Bodies may by default.)
    pub fn set_can_sleep(&mut self, id: BodyId, can_sleep: bool) -> Result<(), BodyError> {
        let body = self.by_id.get_mut(&id).ok_or(BodyError::UnknownBody(id))?;
        body.kept_awake = !can_sleep;
        if body.kind == BodyKind::Dynamic {
            let touching = self.touching_bodies();
            let body = self.by_id.get_mut(&id).expect("it was found above");
            let wanted = !body.kept_awake && touching.contains(&body.handle);
            Self::allow_sleep(&mut self.bodies, body, wanted);
        }
        Ok(())
    }

    /// The bodies that have a contact with something now, as the last step found them.
    fn touching_bodies(&self) -> HashSet<RigidBodyHandle> {
        let mut touching = HashSet::new();
        for pair in self.narrow_phase.contact_pairs() {
            if !pair.has_any_active_contact() {
                continue;
            }
            for collider in [pair.collider1, pair.collider2] {
                // (A collider taken away in this tick may be in a pair that the next step is to drop.)
                if let Some(body) = self.colliders.get(collider).and_then(Collider::parent) {
                    touching.insert(body);
                }
            }
        }
        touching
    }

    /// Before a step: a dynamic body sleeps only while it touches something and has not been kept awake by [`BodyWorld::set_can_sleep`].
    pub(super) fn apply_sleep_rule(&mut self) {
        let touching = self.touching_bodies();
        for body in self.by_id.values_mut() {
            if body.kind != BodyKind::Dynamic {
                continue;
            }
            let wanted = !body.kept_awake && touching.contains(&body.handle);
            Self::allow_sleep(&mut self.bodies, body, wanted);
        }
    }

    /// Lets Rapier put a body to sleep, or stops it and wakes the body; nothing changes if the body is as it is asked to be (a timer that counts how long a body has been still
    /// would be reset by a call every tick).
    fn allow_sleep(bodies: &mut RigidBodySet, body: &mut super::Body, may_sleep: bool) {
        if body.may_sleep == may_sleep {
            return;
        }
        body.may_sleep = may_sleep;
        let rapier = &mut bodies[body.handle];
        if may_sleep {
            *rapier.activation_mut() = RigidBodyActivation::active();
        } else {
            *rapier.activation_mut() = RigidBodyActivation::cannot_sleep();
            rapier.wake_up(true);
        }
    }
}
