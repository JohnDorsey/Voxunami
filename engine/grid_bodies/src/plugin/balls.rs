//! The ball of a joint (`docs/decisions/joints-and-cars.md`, "The block": *a ball of 0.5 m diameter is drawn in the gap, and only then*). Where two joint ends are coupled, or locked
//! (the ends of one ship that face each other), there is an **entity of the kind `engine:joint_ball`**, aboard the ship that has the first of the two ends, at the middle of the face
//! where they meet (the pivot). It is the server's, and clients are told of it as of any entity, which is what makes it the same for every client, for a recording and for a client that
//! comes late; a client draws it as a ball (the game's client plugin gives the kind its appearance).
//!
//! **A hinge has a cylinder where a ball joint has a ball:** two hinge ends whose lines are one line (a coupling that holds a hinge, or a locked pair of hinges whose lines run the same way)
//! have an entity of the kind **`engine:joint_hinge`** at the same place, turned so that its length (its own x axis) runs along the line of the hinge; two axles have one of the kind
//! **`engine:joint_axle`**, along their line (the length of the joints, from one bar's tip to the other's). Ends that cannot be coupled, or do
//! not fit, are no joint and have nothing.
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
use voxel_space::{
    BrickCoordinate, EntityPose, FaceDirection, GridId, GridPose, LatticeRotation, Orientation,
    WorldPosition,
};

use super::Bodies;
use crate::docking::{BlockFace, JointKind, locked_joint_pairs};
use crate::joints::Holding;

/// The kind of the entity that is the ball of a joint.
pub fn joint_ball_kind() -> NamespacedId {
    "engine:joint_ball"
        .parse()
        .expect("constant identifier is valid")
}

/// The kind of the entity that is the cylinder of a hinge.
pub fn joint_hinge_kind() -> NamespacedId {
    "engine:joint_hinge"
        .parse()
        .expect("constant identifier is valid")
}

/// The kind of the entity that is the cylinder of an axle.
pub fn joint_axle_kind() -> NamespacedId {
    "engine:joint_axle"
        .parse()
        .expect("constant identifier is valid")
}

/// Whether the ball or the cylinder is the picture of a joint that is rigid (made so by `/joint immobilize`, or locked): a client draws it in another colour. A component of the entity that
/// clients are told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JointRigid(pub bool);

impl Message for JointRigid {
    fn kind() -> NamespacedId {
        "engine:joint_rigid"
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
    EntityKindSpec::new(joint_ball_kind().as_str()).component(JointRigid(false), true)
}

/// The kinds of the cylinders: whether its joint is rigid is all a cylinder has besides a place and the turn that says which way it runs.
pub(super) fn joint_hinge_kind_spec() -> EntityKindSpec {
    EntityKindSpec::new(joint_hinge_kind().as_str()).component(JointRigid(false), true)
}

pub(super) fn joint_axle_kind_spec() -> EntityKindSpec {
    EntityKindSpec::new(joint_axle_kind().as_str()).component(JointRigid(false), true)
}

/// What a joint is drawn as: a ball, or a cylinder along the line of a hinge or an axle (the end of the line that its first end names, in the axes of the ship the cylinder is aboard).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drawing {
    Ball,
    /// A hinge's cylinder, across the bars.
    Hinge(FaceDirection),
    /// An axle's, along them.
    Axle(FaceDirection),
}

impl Drawing {
    /// What two ends that are held together by a coupling are drawn as: the cylinder of their hinge, or a ball.
    fn of_a_coupling(holding: Holding) -> Self {
        match holding {
            Holding::Ball => Self::Ball,
            Holding::Hinge(axes) => Self::Hinge(axes.first),
            Holding::Axle(axes) => Self::Axle(axes.first),
        }
    }

    /// What a locked pair of ends of one ship (they fit: [`locked_joint_pairs`]) is drawn as, by its first end.
    fn of_a_locked_pair(first: &BlockFace) -> Self {
        match (first.kind(), first.line) {
            (JointKind::Hinge, Some(line)) => Self::Hinge(line),
            (JointKind::Axle, Some(line)) => Self::Axle(line),
            _ => Self::Ball,
        }
    }

    fn kind(self) -> NamespacedId {
        match self {
            Self::Ball => joint_ball_kind(),
            Self::Hinge(_) => joint_hinge_kind(),
            Self::Axle(_) => joint_axle_kind(),
        }
    }

    /// The turn a drawing is made with: none for a ball, and for a cylinder the one that carries its length (x) onto its line.
    fn orientation(self) -> Orientation {
        match self {
            Self::Ball => Orientation::IDENTITY,
            Self::Hinge(line) | Self::Axle(line) => LatticeRotation::all()
                .into_iter()
                .find(|turn| turn.apply_face(FaceDirection::PositiveX) == line)
                .expect("some turn of the lattice carries x onto any face")
                .orientation(),
        }
    }

    /// Whether an entity of the server's is this drawing as it stands: the kind, and for a cylinder the line it runs along (either way, as a cylinder is the same one turned half round: a merge
    /// of two ships can turn the cylinder it carries half round and no more).
    fn is_what_the_entity_is(self, kind: &NamespacedId, orientation: Orientation) -> bool {
        if *kind != self.kind() {
            return false;
        }
        match self {
            Self::Ball => true,
            Self::Hinge(line) | Self::Axle(line) => {
                axis_a_cylinder_runs_along(orientation) == Some(line.axis())
            }
        }
    }
}

/// The axis (0 for x, 1 for y, 2 for z) a cylinder turned by `orientation` has its length along, if it is along one (a turn of the lattice puts it on one, to rounding).
fn axis_a_cylinder_runs_along(orientation: Orientation) -> Option<usize> {
    let turn = GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [0.0; 3]),
        orientation,
    )
    .rotation_matrix();
    (0..3).find(|&axis| turn[axis][0].abs() > 1.0 - 1e-6)
}

/// Where a ball is: the ship it is aboard and the point in voxels in the ship's own coordinates.
type Place = (GridId, [f64; 3]);

impl Bodies {
    /// The places there should be a ball or a cylinder at, in order, what each is, and whether it is of a rigid joint: the first end of every coupling (rigid if the world says the pair is),
    /// and the first end of every locked pair of ends of a ship (a locked joint is rigid).
    fn places_for_balls(&self, world: &ServerWorld) -> Vec<(Place, Drawing, bool)> {
        let mut places: Vec<(Place, Drawing, bool)> = self
            .couplings
            .iter()
            .map(|(key, coupled)| {
                (
                    (key.0, coupled.first_end.local_centre()),
                    Drawing::of_a_coupling(coupled.holding),
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
                places.extend(locked_joint_pairs(&ends).into_iter().map(|pair| {
                    (
                        (*id, pair.first.local_centre()),
                        Drawing::of_a_locked_pair(&pair.first),
                        true,
                    )
                }));
            }
            // (A planet has them too, of the blocks built on it: two ends facing each other on the ground are locked, and the car that is cut out of them is coupled.)
        }
        for (id, planet) in &self.planet_ends {
            let ends: Vec<BlockFace> = planet.by_brick.values().flatten().copied().collect();
            places.extend(locked_joint_pairs(&ends).into_iter().map(|pair| {
                (
                    (*id, pair.first.local_centre()),
                    Drawing::of_a_locked_pair(&pair.first),
                    true,
                )
            }));
        }
        // (A ship that is gone has no ball: a coupling that is let go in the next tick still names it in the tick of a merge.)
        places.retain(|((ship, _), _, _)| world.data().grids().get(*ship).is_some());
        places.sort_by(|((first, at), _, _), ((second, other), _, _)| {
            first
                .cmp(second)
                .then(at.partial_cmp(other).expect("places are numbers"))
        });
        places.dedup_by(|(later, _, later_rigid), (earlier, _, earlier_rigid)| {
            let same = later == earlier;
            *earlier_rigid |= same && *later_rigid;
            same
        });
        places
    }

    /// Whether the bodies have made a ship (its record is whole): a ball aboard one that they have not is not theirs to judge yet.
    fn has_been_made(&self, ship: GridId) -> bool {
        self.planet_ends.contains_key(&ship)
            || self.frames.values().any(|frame| {
                frame
                    .ships
                    .get(&ship)
                    .is_some_and(|record| record.making.is_none())
            })
    }

    /// Makes the entities of the kinds be the balls and cylinders there should be, and no others (see the module).
    pub(super) fn keep_the_balls(&mut self, world: &mut ServerWorld) {
        let kinds = [joint_ball_kind(), joint_hinge_kind(), joint_axle_kind()];
        let mut missing = self.places_for_balls(world);
        let mut extra: Vec<EntityId> = Vec::new();
        let mut to_mark: Vec<(EntityId, bool)> = Vec::new();
        for entity in world
            .entities()
            .iter()
            .filter(|entity| kinds.contains(&entity.kind))
        {
            let place = (entity.pose.grid, entity.pose.position.in_voxels());
            match missing.iter().position(|(wanted, drawing, _)| {
                *wanted == place
                    && drawing.is_what_the_entity_is(&entity.kind, entity.pose.orientation)
            }) {
                Some(index) => {
                    let (_, _, rigid) = missing.remove(index);
                    // (A ball whose mark cannot be read is marked again.)
                    if !matches!(
                        entity.components.get::<JointRigid>(),
                        Ok(Some(JointRigid(marked))) if marked == rigid
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
            let _ = world.set_entity_component(id, &JointRigid(rigid));
        }
        for ((ship, at), drawing, rigid) in missing {
            let pose = EntityPose::new(ship, WorldPosition::from_voxels(at), drawing.orientation());
            let mut marks = ComponentBag::new();
            marks.set(&JointRigid(rigid));
            let _ = world.spawn_entity(&drawing.kind(), pose, marks);
        }
    }
}
