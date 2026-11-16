//! The contact ghosts of the groups that are alone (step 4c-5-2 of `docs/decisions/joints-and-cars.md`; `ContactGhost` in `engine/physics`).
//!
//! A group that is alone has a frame of its own, the truth of what its mechanism does, in which nothing else is; what is near it is in frames of its own too (the one everything
//! is in, another group's). **What a mechanism does never changes by a bit because something is near**, only because something touches it: where bodies of two frames could touch,
//! a ghost holds a copy of them in two worlds, one in which they meet and one in which they pass through each other, and what the contacts did to each body (the difference of the
//! two) is added to what it did in its own frame. With no contact between the two there is no correction at all.
//!
//! **Which ghosts.** A ghost is made for the groups that are alone and what is near them (`ServerWorld::ships_near_group`, with a room that is generous: a ghost must be there a
//! step before anything can touch it, and one with nothing that could touch is not stepped). Groups that are near each other are one mechanism to the ghost, in **one**
//! ghost: a contact between two bodies is worked out where both are, and once. Every ship that is near is in it with the ships it is coupled to, and its couplings. **Sides:** each
//! group that is alone is a side, and everything else (what is in the frame everything is in, a group on a rail, ships coupled that are not alone) is one: bodies of one side meet
//! in both worlds, so that nothing that is not a mechanism's is worked out twice, by two ghosts that have the two near them (and at most 31 mechanisms are in a ghost).
//!
//! **A frame that falls.** The frame of a group that is alone falls with it, in the pull of the world at its point (`Follow::advance`): nothing pulls on what is in it. What is in the
//! frame everything is in is pulled by the whole of it. In the ghost, which is in the frame of the first group, the second is pulled by the difference only, which is what the pull
//! on a body here is, less what is applied to it there, and less what the ghost's frame falls with (`apply_gravity` is what a body had applied; nothing that a ghost knows of).
//!
//! **Not done:** the ground in the ghost (a ghost has none: a group has the ground in its own frame until it touches it, 4c-5-3, and a stranger that rests on the ground rests in the frame everything is in), the kick that a hit gives a group on a rail, a coupling that is made
//! or pulled by a ship in a ghost (a group is not alone when the ends of a ship pull on its own: `alone.rs`).

use std::collections::{BTreeMap, BTreeSet};

use physics::{BodyId, BodyKind, BodySettings, BodyState, ContactGhost, CouplingId, Side};
use server::{ContactRoom, ServerWorld};
use voxel_space::{BrickCoordinate, GridId, WorldPosition};

use super::couplings::CouplingKey;
use super::{Bodies, Follow, Followed, FrameKey, local_in, velocity_from, velocity_in};

/// How near a ship has to be to a group that is alone to be in its ghost: a margin in voxels and the way the two would close in this many seconds. A ghost that has nothing that
/// could touch it in the tick is not stepped, so what is asked for here is a ghost that exists in time (one has to be there a step before anything can touch) and not a cost.
pub(super) const GHOST_ROOM: ContactRoom = ContactRoom {
    margin: 32.0,
    seconds: 0.5,
};

/// How many sides a ghost has: the side of what is not a mechanism, and the rest for the groups that are.
const MECHANISM_SIDES: usize = ContactGhost::MOST_SIDES as usize - 1;

/// What a ghost is made of: the bodies, each with the frame it is truly in and its side, and the couplings. A ghost is made again when this changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GhostParts {
    /// The frame whose point the places of the ghost are measured from: that of the group with the lowest number.
    reference: FrameKey,
    bodies: BTreeMap<GridId, (FrameKey, u32)>,
    couplings: BTreeMap<CouplingKey, CouplingId>,
}

/// A ghost and what it was made of.
pub(super) struct MechanismGhost {
    parts: GhostParts,
    ghost: ContactGhost,
}

impl Bodies {
    /// What the ghosts of this tick are made of, by the lowest anchor of the groups that are alone in each.
    fn ghost_parts(&self, world: &ServerWorld) -> BTreeMap<GridId, GhostParts> {
        // The groups that are alone and have a frame of their own: their anchors, and the frames.
        let mechanisms: BTreeMap<GridId, FrameKey> = self
            .frames
            .iter()
            .filter_map(|(key, frame)| match frame.follow?.followed {
                Followed::Group(anchor) => Some((anchor, *key)),
                _ => None,
            })
            .collect();
        // What is near each, with the ships it is coupled to that have bodies.
        let grids = world.data().grids();
        let near: BTreeMap<GridId, BTreeSet<GridId>> = mechanisms
            .keys()
            .map(|anchor| {
                let ships: BTreeSet<GridId> = world
                    .ships_near_group(*anchor, GHOST_ROOM)
                    .into_iter()
                    .flat_map(|ship| {
                        if grids.has_partners(ship) {
                            grids.group_of(ship)
                        } else {
                            vec![ship]
                        }
                    })
                    .filter(|ship| self.frame_of(*ship).is_some())
                    .collect();
                (*anchor, ships)
            })
            .collect();
        // Groups that are near each other are one mechanism: the clusters of the groups, each by its lowest anchor.
        let anchors: Vec<GridId> = mechanisms.keys().copied().collect();
        let mut cluster_of: BTreeMap<GridId, GridId> = anchors.iter().map(|a| (*a, *a)).collect();
        for first in &anchors {
            for second in &anchors {
                let is_near = grids
                    .group_of(*second)
                    .iter()
                    .any(|ship| near[first].contains(ship));
                if second > first && is_near {
                    let (kept, absorbed) = (cluster_of[first], cluster_of[second]);
                    if kept != absorbed {
                        let (low, high) = (kept.min(absorbed), kept.max(absorbed));
                        for cluster in cluster_of.values_mut() {
                            if *cluster == high {
                                *cluster = low;
                            }
                        }
                    }
                }
            }
        }
        let mut clusters: BTreeMap<GridId, Vec<GridId>> = BTreeMap::new();
        for (anchor, cluster) in &cluster_of {
            clusters.entry(*cluster).or_default().push(*anchor);
        }
        let mut planned = BTreeMap::new();
        for (lowest, mut groups) in clusters {
            groups.truncate(MECHANISM_SIDES);
            let mut bodies: BTreeMap<GridId, (FrameKey, u32)> = BTreeMap::new();
            // The groups that are alone: a side each, from 1.
            for (index, anchor) in groups.iter().enumerate() {
                let frame_key = mechanisms[anchor];
                for ship in self.frames[&frame_key].ships.keys() {
                    bodies.insert(*ship, (frame_key, index as u32 + 1));
                }
            }
            // Everything near them that is not one of them: the one other side.
            let mut any_near = false;
            for anchor in &groups {
                for ship in &near[anchor] {
                    if let std::collections::btree_map::Entry::Vacant(entry) = bodies.entry(*ship)
                        && let Some(frame_key) = self.frame_of(*ship)
                    {
                        entry.insert((frame_key, 0));
                        any_near = true;
                    }
                }
            }
            let owned: BTreeSet<GridId> = bodies.keys().copied().collect();
            let any_mechanism_near_another = groups.len() > 1;
            if !any_near && !any_mechanism_near_another {
                continue;
            }
            let couplings = self
                .couplings
                .iter()
                .filter(|((first, second), _)| owned.contains(first) && owned.contains(second))
                .map(|(key, coupled)| (*key, coupled.id))
                .collect();
            planned.insert(
                lowest,
                GhostParts {
                    reference: mechanisms[&groups[0]],
                    bodies,
                    couplings,
                },
            );
        }
        planned
    }

    /// What a body is in the frame of the ghost, from what it is in the frame it is in: where it is in the world, and how fast it goes in it.
    fn state_in_the_ghost(
        &self,
        reference: Option<Follow>,
        frame_key: FrameKey,
        reference_key: FrameKey,
        id: GridId,
    ) -> Option<BodyState> {
        let frame = self.frames.get(&frame_key)?;
        let state = frame.bodies.state(BodyId(id.get()))?;
        if frame_key == reference_key {
            return Some(state);
        }
        let place = match frame.follow {
            None => state.position,
            Some(follow) => follow.world(state.position)?,
        };
        Some(BodyState {
            position: local_in(reference, place)?,
            linear_velocity: velocity_in(
                reference,
                velocity_from(frame.follow, state.linear_velocity),
            ),
            ..state
        })
    }

    /// A ghost made of `parts` as the frames are now, or `None` if something of it is not there.
    fn make_ghost(&self, parts: &GhostParts) -> Option<ContactGhost> {
        let reference = self.frames.get(&parts.reference)?;
        let mut ghost = ContactGhost::new(
            reference.bodies.origin(),
            BodySettings {
                gravity: [0.0; 3],
                ..self.config.settings
            },
        );
        for (id, (frame_key, side)) in &parts.bodies {
            let frame = self.frames.get(frame_key)?;
            let state =
                self.state_in_the_ghost(reference.follow, *frame_key, parts.reference, *id)?;
            ghost
                .add_body_from(Side(*side), &frame.bodies, BodyId(id.get()), state)
                .ok()?;
        }
        for (key, id) in &parts.couplings {
            let coupled = self.couplings.get(key)?;
            let frame = self.frames.get(&coupled.frame)?;
            ghost.add_coupling_from(&frame.bodies, *id).ok()?;
        }
        Some(ghost)
    }

    /// The force that makes the pull of the world on a body, in the ghost, what it is: the pull of the world on it less the pull of the world on the point the ghost's frame falls
    /// with, less what was applied to the body for it in its own frame (nothing, where that frame falls too). A body in the frame of the ghost is left as it is.
    fn pull_in_the_ghost(
        &self,
        world: &ServerWorld,
        reference_follow: Follow,
        reference_level: voxel_space::LevelIndex,
        frame_key: FrameKey,
        id: GridId,
    ) -> Option<[f64; 3]> {
        let frame = self.frames.get(&frame_key)?;
        let record = frame.ships.get(&id)?;
        if record.kind == BodyKind::Fixed || record.mass <= 0.0 {
            return None;
        }
        let ship = world.data().grids().get(id)?;
        let there = ship.pose.local_to_world(WorldPosition::new(
            BrickCoordinate::ORIGIN,
            record.centre_of_mass,
        ))?;
        let falls_with = reference_follow.world(reference_follow.anchor)?;
        let ghost_frame_pull = world.gravity_at(reference_level, falls_with);
        // What the body is pulled by: the world's, at its place (a group that is alone is pulled as a whole, at its point, by what its frame falls with).
        let pull = match frame.follow {
            Some(follow) if matches!(follow.followed, Followed::Group(_)) => {
                world.gravity_at(frame.level, follow.world(follow.anchor)?)
            }
            _ => world.gravity_at(frame.level, there),
        };
        // What was applied to it: `apply_gravity`.
        let applied = match frame.follow {
            None => Some(world.gravity_at(frame.level, there)),
            Some(follow) => follow.followed.gravity(world, there),
        };
        let force: [f64; 3] = [0, 1, 2].map(|axis| {
            record.mass * (pull[axis] - ghost_frame_pull[axis])
                - applied.map_or(0.0, |applied| record.mass * applied[axis])
        });
        (force != [0.0; 3]).then_some(force)
    }

    /// Before the frames step: the ghosts that should be are made (or kept, if what they are made of is as it was), the ghosts that should not are let go, each is told where its
    /// bodies are and what is applied to them, and stepped like the groups' own worlds (the most steps of them: a ghost is as converged as the most of its groups).
    pub(super) fn ghosts_before_the_step(&mut self, world: &ServerWorld) {
        let planned = self.ghost_parts(world);
        let mut ghosts = std::mem::take(&mut self.ghosts);
        ghosts.retain(|lowest, ghost| planned.get(lowest) == Some(&ghost.parts));
        let mut made = 0u64;
        for (lowest, parts) in planned {
            if ghosts.contains_key(&lowest) {
                continue;
            }
            if let Some(ghost) = self.make_ghost(&parts) {
                ghosts.insert(lowest, MechanismGhost { parts, ghost });
                made += 1;
            }
        }
        let mut stepped = 0u64;
        ghosts.retain(|_, mechanism| {
            let reference_frame = self.frames.get(&mechanism.parts.reference);
            let Some(reference) =
                reference_frame.and_then(|frame| frame.follow.map(|follow| (follow, frame.level)))
            else {
                return false;
            };
            let mut steps = 1;
            let mut told_everything = true;
            for (id, (frame_key, _)) in &mechanism.parts.bodies {
                let (Some(frame), Some(state)) = (
                    self.frames.get(frame_key),
                    self.state_in_the_ghost(
                        Some(reference.0),
                        *frame_key,
                        mechanism.parts.reference,
                        *id,
                    ),
                ) else {
                    told_everything = false;
                    break;
                };
                if mechanism
                    .ghost
                    .refresh(BodyId(id.get()), &frame.bodies, state)
                    .is_err()
                {
                    told_everything = false;
                    break;
                }
                if *frame_key != mechanism.parts.reference
                    && let Some(force) =
                        self.pull_in_the_ghost(world, reference.0, reference.1, *frame_key, *id)
                {
                    let _ = mechanism.ghost.push(BodyId(id.get()), force);
                }
                if matches!(
                    frame.follow,
                    Some(Follow {
                        followed: Followed::Group(_),
                        ..
                    })
                ) {
                    steps = steps.max(frame.bodies.steps_this_tick());
                }
            }
            if !told_everything {
                return false;
            }
            if mechanism.ghost.step(steps) {
                stepped += 1;
            }
            true
        });
        self.ghosts = ghosts;
        let alive = self.ghosts.len() as u64;
        self.count(|report| {
            report.ghosts = alive;
            report.ghosts_made += made;
            report.ghost_ticks += stepped;
        });
    }

    /// After the frames have stepped: what the contacts did to each body is added to what the body did in its own frame.
    pub(super) fn ghosts_after_the_step(&mut self) {
        let mut corrected = 0u64;
        for mechanism in self.ghosts.values() {
            for (id, (frame_key, _)) in &mechanism.parts.bodies {
                let Some(correction) = mechanism.ghost.correction(BodyId(id.get())) else {
                    continue;
                };
                if let Some(frame) = self.frames.get_mut(frame_key)
                    && frame
                        .bodies
                        .apply_correction(BodyId(id.get()), &correction)
                        .is_ok()
                {
                    corrected += 1;
                }
            }
        }
        if corrected > 0 {
            self.count(|report| report.ghost_corrections += corrected);
        }
    }
}
