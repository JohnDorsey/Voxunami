//! Making a coupling rigid and mobile again (`/joint immobilize [SECONDS]` and `/joint mobilize`, `docs/decisions/joints-and-cars.md`, "The command").
//!
//! **The world says which pairs are rigid** (`ServerWorld::immobilized_ships`, saved with the pairs); the plugin makes the body world's coupling be what the world says (`keep_the_stiffness_true`,
//! before the step, after the couplings of the tick are made): a coupling that is new, or made again in another frame, or made again when its ships come back from files or a world is loaded,
//! is made rigid at the turn its bodies have then, so a mechanism that was made stiff is stiff when it is brought back. A pair that is let go of is not rigid any more (the world forgets it with
//! the coupling), and a pair coupled again is a ball.
//!
//! **Braking:** a player may ask for the swing to be braked first (`JointDesk::immobilize`, a time in seconds; none is at once). Before the step each such pair is given the torque, equal and
//! opposite on the two cars, that takes their spin against each other to nothing in the time left (the pair's inertia times the spin over the time: so the momentum is what it was); after the
//! step the time left is counted down, and **the pair is told to the world as rigid when the time is up or the cars have stopped turning against each other** (a pair at rest does not wait out a
//! long time). Rigid is at the turn the cars have then, so the angle it holds is where it stopped. As for a lock, **a group on a rail is taken off it where it is** while its pairs are braked
//! and when one is made rigid or mobile: the cars are fixed bodies on a rail and cannot be turned.

use physics::{BodyId, BodyKind};
use server::ServerWorld;

use super::Bodies;
use crate::docking::turn;
use crate::joints::effective_inertia;

/// How slowly the cars of a pair may turn against each other, in radians a second, for the pair to be made rigid before its time is up.
const STOPPED: f64 = 1e-3;

impl Bodies {
    /// Makes each coupling be rigid or mobile as the world says its pair is, in its body world (see the module).
    pub(super) fn keep_the_stiffness_true(&mut self, world: &ServerWorld) {
        let rigid = world.immobilized_ships();
        let changes: Vec<_> = self
            .couplings
            .iter()
            .filter(|(key, coupled)| rigid.contains(key) != coupled.rigid)
            .map(|(key, coupled)| (*key, coupled.id, coupled.frame, rigid.contains(key)))
            .collect();
        for (key, id, frame_key, want) in changes {
            let Some(frame) = self.frames.get_mut(&frame_key) else {
                continue;
            };
            let done = if want {
                frame.bodies.make_rigid(id)
            } else {
                frame.bodies.make_mobile(id)
            };
            if done.is_ok()
                && let Some(coupled) = self.couplings.get_mut(&key)
            {
                coupled.rigid = want;
            }
        }
    }

    /// How fast the second car of a coupling turns against the first, in the world's axes, from the bodies; `None` if either has no body.
    fn spin_between(&self, key: (voxel_space::GridId, voxel_space::GridId)) -> Option<[f64; 3]> {
        let frame = self
            .frame_of(key.0)
            .and_then(|frame| self.frames.get(&frame))?;
        let first = frame.bodies.state(BodyId(key.0.get()))?;
        let second = frame.bodies.state(BodyId(key.1.get()))?;
        Some([0, 1, 2].map(|axis| second.angular_velocity[axis] - first.angular_velocity[axis]))
    }

    /// Before the step: each pair that is being braked is given the torque that takes the spin of its cars against each other to nothing in the time that is left (see the module).
    pub(super) fn brake_the_pairs_to_be_made_rigid(&mut self, world: &ServerWorld) {
        for (pair, left) in self.config.joint_desk.brakes() {
            let Some(coupled) = self.couplings.get(&pair).copied() else {
                continue;
            };
            let Some(spin) = self.spin_between(pair) else {
                continue;
            };
            // (None left: the pair is made rigid after this step, as it is.)
            if left <= 0.0 {
                continue;
            }
            let Some(frame) = self.frames.get_mut(&coupled.frame) else {
                continue;
            };
            let inertia_of = |id| {
                let record = frame.ships.get(&id)?;
                if record.kind == BodyKind::Fixed {
                    return None;
                }
                let ship = world.data().grids().get(id)?;
                record.inertia_in_the_worlds_axes(ship.pose.rotation_matrix())
            };
            let Some(inertia) = effective_inertia(inertia_of(pair.0), inertia_of(pair.1)) else {
                continue;
            };
            let torque = turn(inertia, spin).map(|component| -component / left);
            for (id, sign) in [(pair.0, -1.0), (pair.1, 1.0)] {
                if frame
                    .ships
                    .get(&id)
                    .is_none_or(|record| record.kind == BodyKind::Fixed)
                {
                    continue;
                }
                let _ = frame
                    .bodies
                    .apply_torque(BodyId(id.get()), torque.map(|component| component * sign));
            }
        }
    }

    /// After the step: the pairs asked to be let turn again are told to the world as mobile; the pairs that are braking have the time left counted down and are told to the world as rigid
    /// when it is up or the cars have stopped turning against each other; and a group on a rail that has any of them to do is taken off it where it is (see the module). A pair that is
    /// not coupled any more is forgotten.
    pub(super) fn handle_the_stiffness_asked_for(&mut self, world: &mut ServerWorld) {
        let desk = self.config.joint_desk.clone();
        let time_step = self.config.settings.time_step;
        let mut off_the_rail = Vec::new();
        desk.keep_mobilizations_only(|pair| self.couplings.contains_key(&pair));
        for (pair, left) in desk.mobilizations() {
            if left <= time_step * (1.0 + 1e-9) {
                world.set_ships_immobilized(pair.0, pair.1, false);
                desk.end_mobilization(pair);
                off_the_rail.push(pair);
            } else {
                desk.set_mobilization_left(pair, left - time_step);
            }
        }
        desk.keep_brakes_only(|pair| self.couplings.contains_key(&pair));
        for (pair, left) in desk.brakes() {
            off_the_rail.push(pair);
            let stopped = self
                .spin_between(pair)
                .is_some_and(|spin| crate::docking::length(spin) <= STOPPED);
            if left <= time_step * (1.0 + 1e-9) || stopped {
                world.set_ships_immobilized(pair.0, pair.1, true);
                desk.end_brake(pair);
            } else {
                desk.set_brake_left(pair, left - time_step);
            }
        }
        for pair in off_the_rail {
            if let Some(anchor) = world
                .rides_a_rail_in_a_group(pair.0)
                .or_else(|| world.rides_a_rail_in_a_group(pair.1))
            {
                world.take_group_off_its_rail(anchor);
            }
        }
    }
}
