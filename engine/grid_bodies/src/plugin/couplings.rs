//! The couplings of ships that have joint ends: which pairs of ships pull on each other, which are coupled, and the forces and joints the plugin puts on their bodies.
//! The rules (when ends pull, when they are coupled, how hard they are pulled) are in [`crate::joints`]; this finds the ends, the speeds and the masses they need, and
//! keeps the couplings true to the blocks. A coupling is derived from the blocks and the places of the ships and is never saved: a world that is loaded with two ships whose
//! ends are together couples them again in the first tick they are both moving.
//!
//! What is pulled is a *group*: ships joined by couplings move as one under a pull, so the mass a pull works against is the group's, and a group is pulled by one other group
//! at a time, the nearest (two pulls at once, or a pull on a ship that is held by its coupling to a third, were found by the tests of a chain to make the ends overshoot each
//! other and jam, with their tips in contact).
//!
//! For now (`docs/decisions/joints-and-cars.md`): ships are coupled only in the same frame; one pair of ends of two ships makes a coupling and the other ends of the same two
//! ships are left (the integer rule for a second ball between the same two cars is for later); two ships of one group are not coupled again (a loop is for later); a coupling
//! is let go when either ship or either end is gone, and not otherwise.

use std::collections::{BTreeMap, BTreeSet};

use physics::{
    BodyId, BodyKind, BodyState, CouplingId, CouplingOptions, MassMoments, PivotPair, SolverDials,
};
use server::ServerWorld;
use voxel_space::GridId;
use world_storage::MovingGrid;

use super::{Bodies, Frame, FrameKey, Ship};
use crate::docking::{Approach, BlockFace, BlockFaceView, approach, cross, turn};
use crate::joints::{JointSettings, effective_mass};

/// A pair of ships that are coupled: the frame they are in, and the lower identifier first.
pub(super) type CouplingKey = (FrameKey, GridId, GridId);

/// A coupling the plugin made, and the ends of the two ships that it is made at.
#[derive(Clone, Copy, Debug)]
pub(super) struct Coupled {
    id: CouplingId,
    first_end: BlockFace,
    second_end: BlockFace,
}

/// Two ships with ends that pull on each other: the closest such pair of ends, and what the rules need to know of how the ships go.
struct JointPair {
    first: GridId,
    second: GridId,
    first_end: BlockFace,
    second_end: BlockFace,
    first_view: BlockFaceView,
    second_view: BlockFaceView,
    how: Approach,
    /// Where each end is from the ship's centre of mass, in the world's axes.
    first_arm: [f64; 3],
    second_arm: [f64; 3],
    /// How fast the second end goes against the first, and how fast the second ship turns against the first.
    relative_velocity: [f64; 3],
    relative_spin: [f64; 3],
}

/// The ships of a frame joined by couplings, as groups, each named by its lowest identifier: how much a group weighs and whether it can move (a group with a ship that is
/// held, frozen or on a rail cannot).
struct Groups {
    parent: BTreeMap<GridId, GridId>,
    /// By the name of the group.
    mass: BTreeMap<GridId, f64>,
    movable: BTreeMap<GridId, bool>,
}

impl Groups {
    fn of(frame: &Frame, couplings: &BTreeMap<CouplingKey, Coupled>, frame_key: FrameKey) -> Self {
        let mut groups = Self {
            parent: frame.ships.keys().map(|id| (*id, *id)).collect(),
            mass: frame
                .ships
                .iter()
                .map(|(id, record)| (*id, record.mass))
                .collect(),
            movable: frame
                .ships
                .iter()
                .map(|(id, record)| (*id, record.kind != BodyKind::Fixed))
                .collect(),
        };
        for (_, first, second) in couplings.keys().filter(|key| key.0 == frame_key) {
            groups.join(*first, *second);
        }
        groups
    }

    fn name_of(&self, mut id: GridId) -> GridId {
        while let Some(&parent) = self.parent.get(&id) {
            if parent == id {
                break;
            }
            id = parent;
        }
        id
    }

    fn same(&self, first: GridId, second: GridId) -> bool {
        self.name_of(first) == self.name_of(second)
    }

    fn join(&mut self, first: GridId, second: GridId) {
        let (first, second) = (self.name_of(first), self.name_of(second));
        if first == second {
            return;
        }
        let (kept, absorbed) = (first.min(second), first.max(second));
        self.parent.insert(absorbed, kept);
        let (mass, movable) = (
            self.mass.get(&kept).copied().unwrap_or(0.0)
                + self.mass.remove(&absorbed).unwrap_or(0.0),
            self.movable.get(&kept).copied().unwrap_or(false)
                && self.movable.remove(&absorbed).unwrap_or(false),
        );
        self.mass.insert(kept, mass);
        self.movable.insert(kept, movable);
    }

    /// What a pull on this ship moves: the mass of its group, if the group can move.
    fn mass_moved_by_a_pull_on(&self, id: GridId) -> Option<f64> {
        let name = self.name_of(id);
        self.movable
            .get(&name)
            .copied()
            .unwrap_or(false)
            .then(|| self.mass.get(&name).copied())
            .flatten()
    }
}

fn velocity_at(state: &BodyState, arm: [f64; 3]) -> [f64; 3] {
    let spun = cross(state.angular_velocity, arm);
    [0, 1, 2].map(|axis| state.linear_velocity[axis] + spun[axis])
}

impl Bodies {
    /// Lets go the couplings whose ships or ends are gone, couples the ships whose ends have come together slowly and in line, and pulls the others together.
    pub(super) fn couple_joints(&mut self, world: &ServerWorld) {
        let Some(settings) = self.config.joints else {
            return;
        };
        self.keep_couplings(world);
        let frame_keys: Vec<FrameKey> = self.frames.keys().copied().collect();
        for frame_key in frame_keys {
            self.couple_and_pull_in(world, &settings, frame_key);
        }
        let now = self.couplings.len() as u64;
        self.count(|report| report.couplings = now);
    }

    /// One frame: the ends that can be coupled are, and then each group is pulled by the nearest group that has ends that pull on its own.
    fn couple_and_pull_in(
        &mut self,
        world: &ServerWorld,
        settings: &JointSettings,
        frame_key: FrameKey,
    ) {
        let Some(frame) = self.frames.get(&frame_key) else {
            return;
        };
        let mut groups = Groups::of(frame, &self.couplings, frame_key);
        let candidates = self.joint_pairs(world, settings, frame_key);
        let mut coupled: BTreeSet<(GridId, GridId)> = BTreeSet::new();
        for pair in &candidates {
            let can_couple = !groups.same(pair.first, pair.second)
                && settings.can_couple(
                    &pair.first_view,
                    &pair.second_view,
                    &pair.how,
                    pair.relative_velocity,
                    pair.relative_spin,
                );
            if can_couple && self.make_coupling(settings, frame_key, pair) {
                groups.join(pair.first, pair.second);
                coupled.insert((pair.first, pair.second));
            }
        }
        // The rest are pulled, nearest first, each group by one other at most.
        let mut pulling: Vec<&JointPair> = candidates
            .iter()
            .filter(|pair| {
                !coupled.contains(&(pair.first, pair.second))
                    && !groups.same(pair.first, pair.second)
            })
            .collect();
        pulling.sort_by(|a, b| {
            a.how
                .distance
                .total_cmp(&b.how.distance)
                .then(a.first.cmp(&b.first))
                .then(a.second.cmp(&b.second))
        });
        let mut busy: BTreeSet<GridId> = BTreeSet::new();
        for pair in pulling {
            let (first_group, second_group) =
                (groups.name_of(pair.first), groups.name_of(pair.second));
            if busy.contains(&first_group) || busy.contains(&second_group) {
                continue;
            }
            let Some(mass) = effective_mass(
                groups.mass_moved_by_a_pull_on(pair.first),
                groups.mass_moved_by_a_pull_on(pair.second),
            ) else {
                continue;
            };
            busy.insert(first_group);
            busy.insert(second_group);
            self.pull(settings, frame_key, pair, mass, &groups);
        }
    }

    /// Whether a coupling still holds: both ships are in its frame, the body world has it, and both ends are still there.
    fn coupling_holds(&self, world: &ServerWorld, key: CouplingKey, coupled: Coupled) -> bool {
        let (frame_key, first, second) = key;
        let Some(frame) = self.frames.get(&frame_key) else {
            return false;
        };
        let (Some(first_record), Some(second_record)) =
            (frame.ships.get(&first), frame.ships.get(&second))
        else {
            return false;
        };
        let has = |record: &Ship, end: BlockFace| {
            record
                .joint_ends
                .get(&end.voxel.split().0)
                .is_some_and(|ends| ends.contains(&end))
        };
        world.data().grids().get(first).is_some()
            && world.data().grids().get(second).is_some()
            && frame
                .bodies
                .couplings_of(BodyId(first.get()))
                .contains(&coupled.id)
            && has(first_record, coupled.first_end)
            && has(second_record, coupled.second_end)
    }

    fn keep_couplings(&mut self, world: &ServerWorld) {
        let gone: Vec<CouplingKey> = self
            .couplings
            .iter()
            .filter(|(key, coupled)| !self.coupling_holds(world, **key, **coupled))
            .map(|(key, _)| *key)
            .collect();
        for key in gone {
            self.let_go(key);
        }
    }

    /// Takes a coupling away: the joint if the body world still has it, and the dials and the sleep of a ship that is in no other.
    fn let_go(&mut self, key: CouplingKey) {
        let Some(coupled) = self.couplings.remove(&key) else {
            return;
        };
        // (A frame that is gone took its bodies with it.)
        let Some(frame) = self.frames.get_mut(&key.0) else {
            return;
        };
        let _ = frame.bodies.decouple(coupled.id);
        for id in [key.1, key.2] {
            let body = BodyId(id.get());
            if frame.bodies.couplings_of(body).is_empty() {
                let _ = frame.bodies.set_solver_dials(body, SolverDials::default());
                let _ = frame.bodies.set_can_sleep(body, true);
            }
        }
    }

    /// For every two ships of a frame that have ends that pull on each other, and are not coupled, the closest pair of ends, in order of ship. Ships that are frozen and pairs of
    /// which neither can move have none.
    fn joint_pairs(
        &self,
        world: &ServerWorld,
        settings: &JointSettings,
        frame_key: FrameKey,
    ) -> Vec<JointPair> {
        let mut pairs = Vec::new();
        let Some(frame) = self.frames.get(&frame_key) else {
            return pairs;
        };
        let bearers: Vec<(GridId, &Ship, &MovingGrid)> = frame
            .ships
            .iter()
            .filter_map(|(id, record)| {
                let ship = world.data().grids().get(*id)?;
                (!ship.frozen && !record.joint_ends.is_empty()).then_some((*id, record, ship))
            })
            .collect();
        for (index, &(first_id, first, first_ship)) in bearers.iter().enumerate() {
            for &(second_id, second, second_ship) in &bearers[index + 1..] {
                if (first.kind == BodyKind::Fixed && second.kind == BodyKind::Fixed)
                    || self
                        .couplings
                        .contains_key(&(frame_key, first_id, second_id))
                {
                    continue;
                }
                let from = first_ship.pose.position;
                let mut best: Option<(
                    BlockFace,
                    BlockFace,
                    BlockFaceView,
                    BlockFaceView,
                    Approach,
                )> = None;
                for first_end in first.joint_ends.values().flatten() {
                    let first_view = BlockFaceView::of(*first_end, &first_ship.pose, from);
                    for second_end in second.joint_ends.values().flatten() {
                        let second_view = BlockFaceView::of(*second_end, &second_ship.pose, from);
                        let how = approach(&first_view, &second_view);
                        if settings.pulls(&first_view, &second_view, &how)
                            && best
                                .as_ref()
                                .is_none_or(|top| how.distance < top.4.distance)
                        {
                            best = Some((*first_end, *second_end, first_view, second_view, how));
                        }
                    }
                }
                let Some((first_end, second_end, first_view, second_view, how)) = best else {
                    continue;
                };
                let (Some(first_state), Some(second_state)) = (
                    frame.bodies.state(BodyId(first_id.get())),
                    frame.bodies.state(BodyId(second_id.get())),
                ) else {
                    continue;
                };
                // (The speed of an end is the centre of mass' and what the ship's turning adds at the end's distance from it, worked out from the centre of mass this plugin
                // keeps, as for docking: Rapier's own is the one before a ship made this tick has a mass.)
                let arm = |ship: &MovingGrid, record: &Ship, end: BlockFace| {
                    let at = end.local_centre();
                    turn(
                        ship.pose.rotation_matrix(),
                        [0, 1, 2].map(|axis| at[axis] - record.centre_of_mass[axis]),
                    )
                };
                let (first_arm, second_arm) = (
                    arm(first_ship, first, first_end),
                    arm(second_ship, second, second_end),
                );
                let (first_velocity, second_velocity) = (
                    velocity_at(&first_state, first_arm),
                    velocity_at(&second_state, second_arm),
                );
                pairs.push(JointPair {
                    first: first_id,
                    second: second_id,
                    first_end,
                    second_end,
                    first_view,
                    second_view,
                    how,
                    first_arm,
                    second_arm,
                    relative_velocity: [0, 1, 2]
                        .map(|axis| second_velocity[axis] - first_velocity[axis]),
                    relative_spin: [0, 1, 2].map(|axis| {
                        second_state.angular_velocity[axis] - first_state.angular_velocity[axis]
                    }),
                });
            }
        }
        pairs
    }

    /// Couples two ships at their ends: a ball joint at the middle of the two faces, which are at one place to within the coupling distance and which the solver pulls
    /// together. Both ships get the extra substeps of the settings and may not sleep. Whether it was made.
    fn make_coupling(
        &mut self,
        settings: &JointSettings,
        frame_key: FrameKey,
        pair: &JointPair,
    ) -> bool {
        let id = CouplingId(self.next_coupling);
        let Some(frame) = self.frames.get_mut(&frame_key) else {
            return false;
        };
        let (first, second) = (BodyId(pair.first.get()), BodyId(pair.second.get()));
        let pivots = [PivotPair {
            first: pair.first_end.local_centre(),
            second: pair.second_end.local_centre(),
        }];
        // (Contacts between the two ships stay on: the owner, 2026-11-09.)
        if frame
            .bodies
            .couple(id, first, second, &pivots, CouplingOptions::default())
            .is_err()
        {
            return false;
        }
        for body in [first, second] {
            let _ = frame.bodies.set_solver_dials(
                body,
                SolverDials {
                    extra_substeps: settings.extra_substeps,
                    extra_passes: 0,
                },
            );
            let _ = frame.bodies.set_can_sleep(body, false);
        }
        self.next_coupling += 1;
        self.couplings.insert(
            (frame_key, pair.first, pair.second),
            Coupled {
                id,
                first_end: pair.first_end,
                second_end: pair.second_end,
            },
        );
        self.count(|report| report.couplings_made += 1);
        true
    }

    /// Pulls two ends toward each other: each of the two ships whose group can move gets the same force, the other way, at its end (as docking's is, owner, 2026-11-09), whatever
    /// it weighs; the group that weighs less moves more. A pull at an end that is away from the centre of mass swings the ship, and two ends side by side swing the two ships
    /// against each other, so the spin of the ships against each other is slowed by a torque against it (equal and opposite, `damping_torque_limit` at most): without it
    /// that spin stays (more than the 0.2 radians a second a coupling allows) and the pair is never coupled (found by the test of two pairs of ends between two cars,
    /// `tests/joint_coupling.rs`). Nothing turns the ships into line. A ball turns however it is.
    fn pull(
        &mut self,
        settings: &JointSettings,
        frame_key: FrameKey,
        pair: &JointPair,
        mass: f64,
        groups: &Groups,
    ) {
        let on_second = settings.pull_on_second(pair.how.gap, pair.relative_velocity, mass);
        // The inertia that the spin is slowed against: each ship's own, about its centre of mass, as a number (a third of the trace of the tensor).
        let inertia_of = |id: GridId| -> Option<f64> {
            groups.mass_moved_by_a_pull_on(id)?;
            let record = self.frames.get(&frame_key)?.ships.get(&id)?;
            let properties = MassMoments::sum(record.moments.values().copied()).properties()?;
            let tensor = properties.inertia;
            Some((tensor[0][0] + tensor[1][1] + tensor[2][2]) / 3.0)
        };
        let damping = effective_mass(inertia_of(pair.first), inertia_of(pair.second))
            .map_or([0.0; 3], |inertia| {
                settings.spin_damping_on_second(pair.relative_spin, inertia)
            });
        let Some(frame) = self.frames.get_mut(&frame_key) else {
            return;
        };
        for (id, arm, sign) in [
            (pair.first, pair.first_arm, -1.0),
            (pair.second, pair.second_arm, 1.0),
        ] {
            if groups.mass_moved_by_a_pull_on(id).is_none() {
                continue;
            }
            let force = on_second.map(|component| component * sign);
            let lever = cross(arm, force);
            let torque = [0, 1, 2].map(|axis| lever[axis] + damping[axis] * sign);
            let body = BodyId(id.get());
            let _ = frame.bodies.apply_force(body, force, None);
            let _ = frame.bodies.apply_torque(body, torque);
        }
    }
}
