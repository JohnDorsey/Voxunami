//! Which groups of ships coupled by their joint ends are **alone**, and so have a frame of their own off a rail (step 4c-4 of `docs/decisions/joints-and-cars.md`), as the bodies
//! decide it.
//!
//! The world says the part that it knows (`ServerWorld::group_is_alone_but_for_the_ground`: the level and the ships' own state; and where the ground of a planet is). **Other ships are no part of
//! it** since step 4c-5-2: what is near a group is the contact ghost's, and the group keeps its frame and its bits whatever is near (`ghosts.rs`). What the bodies add is the one
//! thing that needs a shared frame: **ends that pull on each other**. Joints are made and pulled, and ships are docked, between ships of one frame (`couple_and_pull_in`,
//! `port_pairs`), so a group that has an end within the range of a pull of the end of a ship that is not of it (a coupling or a docking that is about to be made, not a touch)
//! is in the frame that ship is in, as it was before the ghost for everything that was near. (**Not for the ports or the joint ends of a ship that rides a
//! rail, alone or in a group**: step 4c-5-4b pulls those over the rails, `rail_ports.rs` and `rail_joints.rs`.)

use std::collections::{BTreeMap, BTreeSet};

use physics::{BodyId, BodyKind};
use server::{ContactRoom, ServerWorld};
use voxel_space::{BrickCoordinate, GridId, GridPose};
use world_storage::MovingGrid;

use super::rail_ports::is_a_rail_pair;
use super::{Bodies, Followed};
use crate::docking::{Approach, BlockFace, BlockFaceView, DockingSettings, approach};
use crate::joints::JointSettings;

impl Bodies {
    /// The groups of ships coupled by their joint ends that are off a rail and alone (`ServerWorld::group_is_alone_but_for_the_ground`, no ends of another ship that pull on theirs,
    /// and the ground, below), by their anchors. A group that has a frame of its own keeps it while it has the room of 1 and one that has not needs the room of 2 to get it, as for
    /// a rail, so that it does not go in and out of it at the edge. (The room counts for the ground and for the ends that pull; **whether a planet carries the group is no part of
    /// it**: the world carries a group whole, by its anchor, and lets it go by its own rule, and the frame of the group goes with the planet's displacement while it does: `Carrying`.)
    ///
    /// **The ground of a planet** (step 4c-5-3): a group that has its frame keeps it however near the ground comes, with the ground in it as a body that comes at the group, until
    /// the group touches it (`grounded`). Then it is in the frame everything is in, where a group at rest on the ground is as exact as a ship's, and it is alone again when the
    /// ground is out of the way of the room (`ServerWorld::ground_is_in_the_way_of_group`), as before. So what a mechanism does is its own until something touches it, the ground
    /// too. (A group at rest on the ground in a frame that falls was tried and creeps: the ground accelerates against the frame, and a body that the solver meets at constant
    /// speeds in a tick is left going at half a tick of the pull, 0.03 to 0.2 voxels a second, and slides down a slope.)
    pub(super) fn alone_groups(&self, world: &ServerWorld) -> BTreeSet<GridId> {
        let grids = world.data().grids();
        let mut alone = BTreeSet::new();
        for (id, _) in grids.iter() {
            if !grids.has_partners(id) || grids.group_of(id).first() != Some(&id) {
                continue;
            }
            let has_its_frame = self.frames.values().any(|frame| {
                frame
                    .follow
                    .is_some_and(|follow| follow.followed == Followed::Group(id))
            });
            let room = if has_its_frame { 1.0 } else { 2.0 };
            let keeps_its_frame_over_the_ground = has_its_frame && !self.grounded.contains(&id);
            if world.group_is_alone_but_for_the_ground(id)
                && (keeps_its_frame_over_the_ground
                    || !world.ground_is_in_the_way_of_group(id, room))
                && !self.ends_pull_on_the_group(world, id, room, self.config.settings.time_step)
            {
                alone.insert(id);
            }
        }
        alone
    }

    /// Notes which groups that have a frame of their own are on the ground of a planet: a ship of the group has a contact with a planet of the frame in the step that was just taken.
    pub(super) fn note_the_groups_on_the_ground(&mut self) {
        self.grounded.clear();
        for frame in self.frames.values() {
            let Some(Followed::Group(anchor)) = frame.follow.map(|follow| follow.followed) else {
                continue;
            };
            let on_the_ground = frame.ships.keys().any(|ship| {
                frame.planets.keys().any(|planet| {
                    frame
                        .bodies
                        .touches(BodyId(ship.get()), BodyId(planet.get()))
                })
            });
            if on_the_ground {
                self.grounded.insert(anchor);
            }
        }
    }

    /// Whether an end of a ship that is not of the group (a joint end or a docking port) is within the pull of an end of a ship of it (the range of the pull, times `room`): the
    /// two pull on each other, are coupled or docked, and need the one frame for it. The ends are those that [`Bodies::couple_joints`] and [`Bodies::dock`] would look at: a
    /// ship that is frozen has none, a pair of which neither can move is nothing, and a pair that was undocked and has not come apart, or could not be docked, is left alone.
    /// `lag` is the seconds that the places of the ships that the rails place are ahead of the others' (a tick, before the bodies step; 0 after it): see `ServerWorld::ships_near_group_lagging`.
    pub(super) fn ends_pull_on_the_group(
        &self,
        world: &ServerWorld,
        anchor: GridId,
        room: f64,
        lag: f64,
    ) -> bool {
        let joints = self.config.joints.map(|settings| JointSettings {
            range: settings.range * room,
            ..settings
        });
        let docking = self.config.docking.as_ref().map(|docking| DockingSettings {
            range: docking.settings.range * room,
            ..docking.settings
        });
        let reach = joints
            .map(|settings| settings.range)
            .into_iter()
            .chain(docking.map(|settings| settings.range))
            .fold(0.0_f64, f64::max);
        if reach <= 0.0 {
            return false;
        }
        // (A pull is between the middles of two faces, which are on the boxes of the blocks: nearer than the hulls' spheres by what is not a sphere. A voxel more is the rounding.)
        let near = world.ships_near_mechanism_lagging(
            anchor,
            ContactRoom {
                margin: reach + 1.0,
                seconds: 0.0,
            },
            lag,
        );
        if near.is_empty() {
            return false;
        }
        let released = self
            .config
            .docking
            .as_ref()
            .map(|docking| docking.released())
            .unwrap_or_default();
        let members = world.data().grids().group_of(anchor);
        // **A ship on a rail, alone or in a group, is not taken off it for a port that pulls** (step 4c-5-4b), and no ship is taken out of the frame it is in for a port that pulls a ship on
        // a rail: the pull is a force on the rail and a torque on the bodies, which give it to the ship in the frame it is in (see `rail_ports.rs`), and only the dock, which is a touch,
        // takes the rail. The ends that count here are those of ships of which neither rides a rail (a coupling and a dock are made in one frame).
        for stranger in near {
            let (Some(other), Some(other_record)) =
                (world.data().grids().get(stranger), self.record_of(stranger))
            else {
                continue;
            };
            let other_pose = pose_at_the_start(world, stranger, other, lag);
            if other.frozen {
                continue;
            }
            for member in &members {
                let (Some(ship), Some(record)) =
                    (world.data().grids().get(*member), self.record_of(*member))
                else {
                    continue;
                };
                let pose = pose_at_the_start(world, *member, ship, lag);
                if record.kind == BodyKind::Fixed && other_record.kind == BodyKind::Fixed {
                    continue;
                }
                let pair = (*member.min(&stranger), *member.max(&stranger));
                if let Some(settings) = joints
                    && !is_a_rail_pair(world, *member, stranger)
                    && any_pull(
                        (&pose, &record.joint_ends),
                        (&other_pose, &other_record.joint_ends),
                        |first, second, how| settings.pulls(first, second, how),
                    )
                {
                    return true;
                }
                if let Some(settings) = docking
                    && !is_a_rail_pair(world, *member, stranger)
                    && !released.contains(&pair)
                    && !self
                        .refused
                        .get(&pair)
                        .is_some_and(|until| self.tick < *until)
                    && any_pull(
                        (&pose, &record.ports),
                        (&other_pose, &other_record.ports),
                        |first, second, how| settings.pulls(first, second, how),
                    )
                {
                    return true;
                }
            }
        }
        false
    }
}

/// Where a ship is at the start of the tick that is about to be stepped: the world has a ship that the rails place at its place of the end of it (`ServerWorld::is_placed_by_a_rail`),
/// so it is put back by `lag` seconds of its speed, and a ship that is free is as it is.
pub(super) fn pose_at_the_start(
    world: &ServerWorld,
    id: GridId,
    ship: &MovingGrid,
    lag: f64,
) -> GridPose {
    if lag > 0.0 && world.is_placed_by_a_rail(id) {
        GridPose::new(
            ship.pose
                .position
                .translated(ship.linear_velocity.map(|speed| -speed * lag)),
            ship.pose.orientation,
        )
    } else {
        ship.pose
    }
}

/// The ends of a ship by the brick they are in (a ship's record keeps them so).
type Ends = BTreeMap<BrickCoordinate, Vec<BlockFace>>;

/// Whether an end of the first ship and an end of the second pull on each other by `pulls` (the rule of joints or of docking), both ships with the ends they have.
fn any_pull(
    first: (&GridPose, &Ends),
    second: (&GridPose, &Ends),
    pulls: impl Fn(&BlockFaceView, &BlockFaceView, &Approach) -> bool,
) -> bool {
    let from = first.0.position;
    for first_end in first.1.values().flatten() {
        let first_view = BlockFaceView::of(*first_end, first.0, from);
        for second_end in second.1.values().flatten() {
            let second_view = BlockFaceView::of(*second_end, second.0, from);
            let how = approach(&first_view, &second_view);
            if pulls(&first_view, &second_view, &how) {
                return true;
            }
        }
    }
    false
}
