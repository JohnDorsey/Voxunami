//! Which ships are near a group of ships coupled by their joint ends: the ships that the contact ghost of the group is made for (step (a) of 4c-5-2 in
//! `docs/decisions/joints-and-cars.md`; `ContactGhost` in `engine/physics`).
//!
//! The rules of the rails (`coasting/neighbours.rs`) ask for the lowest of the grids that is in the way, by the sphere through the far corner of a ship's bricks (55 voxels for a
//! ship of one brick, whatever is in it) and a margin of 64 voxels. A ghost needs **all** the ships that could touch the group, and a measure that is not that coarse: the hull of
//! each ship (the box its blocks fill, step 4c-5-1, [`world_storage::HullBox`]), taken as the sphere about its middle through its farthest corner, which no turn of the ship
//! changes. A ghost that is born near nothing costs nothing (its gate leaves a tick unstepped when no two sides could touch), so the room asked for is the caller's and may be
//! generous: a ghost has to be there a step before anything can touch (`docs/history/loop-probe.md`, section 18).

use std::collections::BTreeMap;

use voxel_space::{BrickCoordinate, GridId, WorldPosition};
use world_storage::MovingGrid;

use super::ServerWorld;

/// How much room a ghost is given: two ships are near each other when the gap between their hulls is less than `margin` voxels and the way they would close in `seconds` at the
/// speed they close at, which counts the speed of the two origins against each other and what the turn of each carries its far corner (the turn in radians a second times the
/// reach of the hull).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactRoom {
    /// The gap, in voxels, that is near whatever the speed.
    pub margin: f64,
    /// The seconds the speed they close at is carried for.
    pub seconds: f64,
}

/// Where the blocks of a ship are and how they move: the sphere about the middle of its hull through the farthest corner, the speed of its origin and the speed of its turn.
pub(super) struct HullSphere {
    centre: WorldPosition,
    radius: f64,
    velocity: [f64; 3],
    spin: f64,
}

impl HullSphere {
    /// What a ship is now; `None` for one that has no hull (a planet, a ship with no blocks or one that has not been weighed: nothing of it can touch) or whose hull the world
    /// cannot place.
    ///
    /// **`lag`** is the seconds that a ship on a rail is ahead of the ships that are not, in the world's places while the bodies are about to step: the rails put a ship at the
    /// place of the end of the tick (`ServerWorld::apply_rails`, at the start of it) and a ship that is free is where it was at the start, which the step takes to the end. A ship
    /// on a rail is put back by that many seconds of its speed, so that the two are measured at one instant (0 for the places as the world has them: at orbital speed a tick is 38
    /// voxels, more than the room of a ghost).
    pub(super) fn of(ship: &MovingGrid, lag: f64) -> Option<HullSphere> {
        // (`lag` is for a ship that is placed by a rail: 0 for one that is not.)
        if ship.terrain.is_some() {
            return None;
        }
        let hull = ship.hull?;
        let mut centre = ship
            .pose
            .local_to_world(WorldPosition::new(BrickCoordinate::ORIGIN, hull.middle()))?;
        if lag > 0.0 {
            centre = centre.translated(ship.linear_velocity.map(|speed| -speed * lag));
        }
        Some(HullSphere {
            centre,
            radius: hull.radius(),
            velocity: ship.linear_velocity,
            spin: size(ship.angular_velocity),
        })
    }

    /// Whether the gap between the two spheres is less than the room: the gap between the middles less the two radii, against the margin and the way the two close in the seconds
    /// (the speed of one against the other and what each turn carries a corner).
    pub(super) fn is_near(&self, other: &HullSphere, room: ContactRoom) -> bool {
        let apart = size(self.centre.displacement_to(other.centre).in_voxels());
        let gap = apart - self.radius - other.radius;
        let relative: [f64; 3] = [0, 1, 2].map(|axis| other.velocity[axis] - self.velocity[axis]);
        let closing = size(relative) + self.spin * self.radius + other.spin * other.radius;
        gap < room.margin + closing * room.seconds
    }
}

fn size(vector: [f64; 3]) -> f64 {
    libm::sqrt(vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2])
}

impl ServerWorld {
    /// Every ship that is near the group of ships coupled by their joint ends that `anchor` (the ship of it with the lowest number) speaks for, in the order of their numbers: a
    /// ship of the group's level that is not of the group and is near one of its ships by the room given (see [`ContactRoom`]). A ship with no hull is not counted (it has no
    /// blocks to touch with, or is not weighed yet), nor a member without one, nor a planet (the ground is a matter of its own), and nothing is given for a ship that is not
    /// the anchor of a group of two ships or more. Every ship is looked at once, so the cost is the ships times the group's.
    pub fn ships_near_group(&self, anchor: GridId, room: ContactRoom) -> Vec<GridId> {
        self.ships_near_group_lagging(anchor, room, 0.0)
    }

    /// Whether the rails place this ship: it has one, or it is a ship of a group whose anchor has one (`rail_group_of`). The world puts such a ship at its place of the end of the
    /// tick at the start of it (`apply_rails`), and a ship that is free is at its place of the start until the bodies step it.
    pub fn is_placed_by_a_rail(&self, ship: GridId) -> bool {
        let grids = self.data.grids();
        // (A ship whose rail waits for the step is where the last tick left it, as a free one is.)
        let placed = |id: GridId| {
            self.rails_placed_at_the_start
                .as_ref()
                .is_none_or(|placed| placed.contains(&id))
        };
        (grids.get(ship).is_some_and(|grid| grid.rail.is_some()) && placed(ship))
            || (grids.has_partners(ship)
                && self
                    .rail_group_of(ship)
                    .is_some_and(|group| placed(group.anchor)))
    }

    /// `lag` for a ship that the rails place, and 0 for one that is not.
    fn lag_of(&self, ship: GridId, lag: f64) -> f64 {
        if lag > 0.0 && self.is_placed_by_a_rail(ship) {
            lag
        } else {
            0.0
        }
    }

    /// As [`ServerWorld::ships_near_group`], with the places of the ships that are on a rail put back by `lag` seconds of their speed (see `HullSphere::of`): the bodies, which are
    /// about to step a tick, ask it with the length of the tick, so that a group on a rail and a stranger that is free are measured at the same instant.
    pub fn ships_near_group_lagging(
        &self,
        anchor: GridId,
        room: ContactRoom,
        lag: f64,
    ) -> Vec<GridId> {
        let members = self.data.grids().group_of(anchor);
        if members.len() < 2 || members[0] != anchor {
            return Vec::new();
        }
        self.ships_near_mechanism_lagging(anchor, room, lag)
    }

    /// As [`ServerWorld::ships_near_group_lagging`], **for a ship that is alone as well**: the ship that `anchor` speaks for is the one or the group (`anchor` is the ship of it with
    /// the lowest number), and nothing is given for a ship that is not the anchor of what it is in or that is not there. A single ship on a rail has a frame of its own while a
    /// stranger is near it (step 4c-5-4c of `docs/decisions/joints-and-cars.md`), and is a mechanism of a ghost as a group is.
    pub fn ships_near_mechanism_lagging(
        &self,
        anchor: GridId,
        room: ContactRoom,
        lag: f64,
    ) -> Vec<GridId> {
        self.ships_near_mechanisms_lagging(&[anchor], room, lag)
            .remove(&anchor)
            .unwrap_or_default()
    }

    /// What [`ServerWorld::ships_near_mechanism_lagging`] gives for each of `anchors`, found together: the hull of each ship is worked out once and not once for every mechanism
    /// (a world in which every ship on a rail has a stranger near it has a mechanism for each ship, and the cost of asking for each was the ships times the ships: 35 ms a tick at 400
    /// ships, measured 2026-11-18). An anchor that speaks for nothing is given an empty list.
    pub fn ships_near_mechanisms_lagging(
        &self,
        anchors: &[GridId],
        room: ContactRoom,
        lag: f64,
    ) -> BTreeMap<GridId, Vec<GridId>> {
        let grids = self.data.grids();
        let spheres: Vec<(GridId, voxel_space::LevelIndex, Option<HullSphere>)> = grids
            .iter()
            .map(|(id, ship)| (id, ship.level, HullSphere::of(ship, self.lag_of(id, lag))))
            .collect();
        anchors
            .iter()
            .map(|anchor| {
                let members = grids.group_of(*anchor);
                if members.first() != Some(anchor) {
                    return (*anchor, Vec::new());
                }
                let of_the_members: Vec<&(GridId, voxel_space::LevelIndex, Option<HullSphere>)> =
                    spheres
                        .iter()
                        .filter(|(id, _, _)| members.contains(id))
                        .collect();
                let near = spheres
                    .iter()
                    .filter(|(id, _, _)| !members.contains(id))
                    .filter_map(|(id, level, sphere)| {
                        let sphere = sphere.as_ref()?;
                        of_the_members
                            .iter()
                            .any(|(_, member_level, member)| {
                                member_level == level
                                    && member
                                        .as_ref()
                                        .is_some_and(|member| member.is_near(sphere, room))
                            })
                            .then_some(*id)
                    })
                    .collect();
                (*anchor, near)
            })
            .collect()
    }
}
