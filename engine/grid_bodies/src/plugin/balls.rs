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

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use entities::{ComponentBag, EntityKindSpec};
use namespaced_id::NamespacedId;
use protocol::{EntityId, Message};
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

/// Whether the ball is the ball of a joint that is rigid (made so by `/joint immobilize`, or locked): a client draws it in another colour. A component of the ball that clients are told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JointBallRigid(pub bool);

impl Message for JointBallRigid {
    fn kind() -> NamespacedId {
        "engine:joint_ball_rigid"
            .parse()
            .expect("constant identifier is valid")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.write_u8(u8::from(self.0));
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let rigid = match reader.read_u8()? {
            0 => false,
            1 => true,
            _ => return Err(DecodeError::Invalid("a rigid flag is 0 or 1")),
        };
        reader.finish()?;
        Ok(Self(rigid))
    }

    fn samples() -> Vec<Self> {
        vec![Self(false), Self(true)]
    }
}

/// The kind of a ball: whether its joint is rigid is all it has besides a place.
pub(super) fn joint_ball_kind_spec() -> EntityKindSpec {
    EntityKindSpec::new(joint_ball_kind().as_str()).component(JointBallRigid(false), true)
}

/// Where a ball is: the ship it is aboard and the point in voxels in the ship's own coordinates.
type Place = (GridId, [f64; 3]);

impl Bodies {
    /// The places there should be a ball at, in order, and whether each is the ball of a rigid joint: the first end of every coupling (rigid if the world says the pair is), and the
    /// first end of every locked pair of ends of a ship (a locked joint is rigid).
    fn places_for_balls(&self, world: &ServerWorld) -> Vec<(Place, bool)> {
        let mut places: Vec<(Place, bool)> = self
            .couplings
            .iter()
            .map(|(key, coupled)| {
                (
                    (key.0, coupled.first_end.local_centre()),
                    world.immobilized_ships().contains(key),
                )
            })
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
                        .map(|pair| ((*id, pair.first.local_centre()), true)),
                );
            }
        }
        // (A ship that is gone has no ball: a coupling that is let go in the next tick still names it in the tick of a merge.)
        places.retain(|((ship, _), _)| world.data().grids().get(*ship).is_some());
        places.sort_by(|((first, at), _), ((second, other), _)| {
            first
                .cmp(second)
                .then(at.partial_cmp(other).expect("places are numbers"))
        });
        places.dedup_by(|(later, later_rigid), (earlier, earlier_rigid)| {
            let same = later == earlier;
            *earlier_rigid |= same && *later_rigid;
            same
        });
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
        let mut extra: Vec<EntityId> = Vec::new();
        let mut to_mark: Vec<(EntityId, bool)> = Vec::new();
        for entity in world.entities().iter().filter(|entity| entity.kind == kind) {
            let place = (entity.pose.grid, entity.pose.position.in_voxels());
            match missing.iter().position(|(wanted, _)| *wanted == place) {
                Some(index) => {
                    let (_, rigid) = missing.remove(index);
                    // (A ball whose mark cannot be read is marked again.)
                    if !matches!(
                        entity.components.get::<JointBallRigid>(),
                        Ok(Some(JointBallRigid(marked))) if marked == rigid
                    ) {
                        to_mark.push((entity.id, rigid));
                    }
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
        for (id, rigid) in to_mark {
            let _ = world.set_entity_component(id, &JointBallRigid(rigid));
        }
        for ((ship, at), rigid) in missing {
            let pose = EntityPose::new(ship, WorldPosition::from_voxels(at), Orientation::IDENTITY);
            let mut marks = ComponentBag::new();
            marks.set(&JointBallRigid(rigid));
            let _ = world.spawn_entity(&kind, pose, marks);
        }
    }
}
