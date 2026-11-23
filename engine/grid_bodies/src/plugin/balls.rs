//! The ball of a joint (`docs/decisions/joints-and-cars.md`, "The block": *a ball of 0.5 m diameter is drawn in the gap, and only then*). Where two joint ends are coupled, or locked
//! (the ends of one ship that face each other), there is an **entity of the kind `engine:joint_ball`**, aboard the ship that has the first of the two ends, at the middle of the face
//! where they meet (the pivot). It is the server's, and clients are told of it as of any entity, which is what makes it the same for every client, for a recording and for a client that
//! comes late; a client draws it as a ball (the game's client plugin gives the kind its appearance).
//!
//! **Aboard the ship, not in the world:** its place in the ship's own coordinates does not change while the ends stay, so nothing moves it as the ship flies, turns on a rail or is
//! carried by a planet, and the ship's pose puts it where it is drawn. A merge or a split carries what is aboard with the blocks (`ServerWorld::merge_grids`), so a lock keeps the
//! ball it had (the pivot of the coupling is the middle of the locked pair's faces, to the exact lattice) and a split gives it to a car.
//!
//! **Derived, never kept:** each tick (`keep_the_balls`, at its end) the balls that there should be are worked out from the couplings and the ships' joint ends, and the entities of the
//! kind are made to be those and no others: one that none of them needs is taken away (a world that was saved with balls, a ball made by hand), one that is missing is made, and
//! one that is there is left alone (so an entity is the same entity tick after tick). A ball aboard a ship that the bodies have not yet made, or are making, is left until they have
//! (a ship that a lock has just made is new, and the ball it was carried onto is the one it is to have).

use entities::{ComponentBag, EntityKindSpec};
use namespaced_id::NamespacedId;
use server::ServerWorld;
use voxel_space::{EntityPose, GridId, Orientation, WorldPosition};

use super::Bodies;
use crate::docking::{BlockFace, facing_pairs};

/// The kind of the entity that is the ball of a joint.
pub fn joint_ball_kind() -> NamespacedId {
    "engine:joint_ball"
        .parse()
        .expect("constant identifier is valid")
}

/// The kind of a ball: no components, a place is all it has.
pub(super) fn joint_ball_kind_spec() -> EntityKindSpec {
    EntityKindSpec::new(joint_ball_kind().as_str())
}

/// Where a ball is: the ship it is aboard and the point in voxels in the ship's own coordinates.
type Place = (GridId, [f64; 3]);

impl Bodies {
    /// The places there should be a ball at, in order: the first end of every coupling, and the first end of every locked pair of ends of a ship.
    fn places_for_balls(&self, world: &ServerWorld) -> Vec<Place> {
        let mut places: Vec<Place> = self
            .couplings
            .iter()
            .map(|(key, coupled)| (key.0, coupled.first_end.local_centre()))
            .collect();
        for frame in self.frames.values() {
            for (id, record) in &frame.ships {
                if record.making.is_some() || record.joint_ends.is_empty() {
                    continue;
                }
                let ends: Vec<BlockFace> = record.joint_ends.values().flatten().copied().collect();
                places.extend(
                    facing_pairs(&ends)
                        .into_iter()
                        .map(|pair| (*id, pair.first.local_centre())),
                );
            }
        }
        // (A ship that is gone has no ball: a coupling that is let go in the next tick still names it in the tick of a merge.)
        places.retain(|(ship, _)| world.data().grids().get(*ship).is_some());
        places.sort_by(|(first, at), (second, other)| {
            first
                .cmp(second)
                .then(at.partial_cmp(other).expect("places are numbers"))
        });
        places.dedup();
        places
    }

    /// Whether the bodies have made a ship (its record is whole): a ball aboard one that they have not is not theirs to judge yet.
    fn has_been_made(&self, ship: GridId) -> bool {
        self.frames.values().any(|frame| {
            frame
                .ships
                .get(&ship)
                .is_some_and(|record| record.making.is_none())
        })
    }

    /// Makes the entities of the kind be the balls there should be, and no others (see the module).
    pub(super) fn keep_the_balls(&mut self, world: &mut ServerWorld) {
        let kind = joint_ball_kind();
        let mut missing = self.places_for_balls(world);
        let mut extra = Vec::new();
        for entity in world.entities().iter().filter(|entity| entity.kind == kind) {
            let place = (entity.pose.grid, entity.pose.position.in_voxels());
            match missing.iter().position(|wanted| *wanted == place) {
                Some(index) => {
                    missing.remove(index);
                }
                None => {
                    // (Aboard a ship the bodies have not made yet, it is left: a ship that a lock has just made has the ball it was carried onto.)
                    let undecided = entity.pose.grid.root_level().is_none()
                        && !self.has_been_made(entity.pose.grid);
                    if !undecided {
                        extra.push(entity.id);
                    }
                }
            }
        }
        for id in extra {
            world.despawn_entity(id);
        }
        for (ship, at) in missing {
            let pose = EntityPose::new(ship, WorldPosition::from_voxels(at), Orientation::IDENTITY);
            let _ = world.spawn_entity(&kind, pose, ComponentBag::new());
        }
    }
}
