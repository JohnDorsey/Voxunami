//! Making one ship of two: [`ServerWorld::merge_grids`].

use voxel_grid::ChannelIndex;
use voxel_space::{
    AlignmentTolerance, BrickCoordinate, EntityPose, GridId, GridPose, LatticeRotation,
    LatticeTransform, LocalVoxelIndex, Orientation, VoxelCoordinate, WorldPosition,
    lattice_alignment,
};
use world_storage::MovingGrid;

use crate::server_world::{AccessError, ServerWorld};

/// What the values of a voxel become when the ship they are in is turned: see [`MergePlan::turn`].
pub trait TurnValues: Fn(&mut Vec<(ChannelIndex, u32)>, LatticeRotation) {}

impl<T: Fn(&mut Vec<(ChannelIndex, u32)>, LatticeRotation)> TurnValues for T {}

/// What a merge needs to know, besides the two ships.
pub struct MergePlan<'a> {
    /// The channel of the blocks: a voxel whose block is not the channel's default is a block of the ship (the rest of a ship is air).
    pub block_channel: ChannelIndex,
    /// The channel that holds the orientation of each block, if the game has one: the number of a turn of the lattice (`LatticeRotation::index`), 0 for
    /// none. A block of the other ship that is carried onto the lattice of the base is turned by the turn that carries it, whatever the block is, and
    /// so are the blocks of plugins that are not loaded: its orientation becomes its own turn followed by the carrying one.
    pub orientation_channel: Option<ChannelIndex>,
    /// How near to one lattice the ships must be.
    pub tolerance: AlignmentTolerance,
    /// What the values of a voxel become when the ship they are in is turned to be on the lattice of the other: given the non-default values of a
    /// voxel (a channel and its value) and the turn, it changes them (a stair that faced north faces east), and may add a channel whose value was
    /// the default and is not now. Called only for a turn that is not none.
    pub turn: &'a dyn TurnValues,
    /// How the merged ship moves: the caller works it out from the masses (the server does not know them).
    pub linear_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
}

impl std::fmt::Debug for MergePlan<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MergePlan")
            .field("block_channel", &self.block_channel)
            .field("tolerance", &self.tolerance)
            .finish_non_exhaustive()
    }
}

/// Why two ships could not be made one.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MergeError {
    #[error(transparent)]
    Access(#[from] AccessError),
    #[error("a ship cannot be merged with itself")]
    SameShip,
    #[error("the ships are not in the same level")]
    DifferentLevels,
    #[error(
        "the ships are not on one lattice: their turns or places differ from a whole number of quarter turns and voxels"
    )]
    NotOnOneLattice,
    #[error("the ships have a block in the same place ({0:?} of the larger)")]
    Overlap(VoxelCoordinate),
    #[error("the merged ship would reach outside what the world can hold")]
    OutOfRange,
}

/// A position as a point in voxels from the origin of its grid.
fn point_of(position: WorldPosition) -> [f64; 3] {
    let brick = position.brick();
    let offset = position.offset();
    [
        brick.x() as f64 * 32.0 + f64::from(offset[0]),
        brick.y() as f64 * 32.0 + f64::from(offset[1]),
        brick.z() as f64 * 32.0 + f64::from(offset[2]),
    ]
}

fn position_of(point: [f64; 3]) -> Option<WorldPosition> {
    let whole = point.map(|value| (value / 32.0).floor() as i64);
    let brick = BrickCoordinate::try_new(whole[0], whole[1], whole[2]).ok()?;
    WorldPosition::try_new(
        brick,
        [0, 1, 2].map(|axis| (point[axis] - whole[axis] as f64 * 32.0) as f32),
    )
}

impl ServerWorld {
    /// Makes one ship of two that are on one lattice (see [`voxel_space::lattice_alignment`]) and do not share a place: the larger keeps its
    /// coordinates and the smaller's blocks are carried onto them by the exact turn and offset that put the smaller on the lattice of the larger
    /// (the values of a voxel turned by `plan.turn`). The merged ship is a new ship, in the larger one's place and turned as it is, and neither
    /// old id is its id: each leaves a tombstone naming it, with the transform to it. Whatever was aboard either is aboard it, and does not
    /// move relative to the blocks it stood on. It is held if either was, goes as `plan` says, and the rules hear of it as of a ship that came
    /// out of a file. Nothing changes on an error.
    pub fn merge_grids(
        &mut self,
        a: GridId,
        b: GridId,
        plan: &MergePlan<'_>,
    ) -> Result<GridId, MergeError> {
        if a == b {
            return Err(MergeError::SameShip);
        }
        let first = self.data.grids().get(a).ok_or(AccessError::NoSuchGrid(a))?;
        let second = self.data.grids().get(b).ok_or(AccessError::NoSuchGrid(b))?;
        if first.level != second.level {
            return Err(MergeError::DifferentLevels);
        }
        // The larger keeps its coordinates (of two as large, the one with the lower id).
        let (base_id, other_id) =
            if (second.voxels.bricks().len(), a) > (first.voxels.bricks().len(), b) {
                (b, a)
            } else {
                (a, b)
            };
        let base = self.data.grids().get(base_id).expect("the ship is there");
        let other = self.data.grids().get(other_id).expect("the ship is there");
        let transform = lattice_alignment(&other.pose, &base.pose, plan.tolerance)
            .ok_or(MergeError::NotOnOneLattice)?;

        // The blocks of the other, carried onto the lattice of the base. (Everything is worked out before anything is changed.)
        let layout = base.voxels.layout().clone();
        let channels: Vec<ChannelIndex> = layout.channel_indices().collect();
        let default_block = layout.default_value(plan.block_channel);
        let mut voxels = base.voxels.clone();
        for (brick, contents) in other.voxels.bricks() {
            for local in LocalVoxelIndex::all() {
                if contents.get(plan.block_channel, local) == default_block {
                    continue;
                }
                let target = transform
                    .apply(brick.voxel_at(local))
                    .ok_or(MergeError::OutOfRange)?;
                if base.voxels.get(plan.block_channel, target) != default_block {
                    return Err(MergeError::Overlap(target));
                }
                let mut values: Vec<(ChannelIndex, u32)> = channels
                    .iter()
                    .map(|&channel| (channel, contents.get(channel, local)))
                    .filter(|(channel, value)| *value != layout.default_value(*channel))
                    .collect();
                if transform.rotation != LatticeRotation::IDENTITY {
                    if let Some(channel) = plan.orientation_channel {
                        let position = values.iter().position(|(found, _)| *found == channel);
                        let own = position.map_or_else(
                            || layout.default_value(channel),
                            |position| values[position].1,
                        );
                        // (A number that is not a turn is left as it is: a world is read as it is found.)
                        if let Some(own) =
                            u8::try_from(own).ok().and_then(LatticeRotation::from_index)
                        {
                            let turned = u32::from(own.then(transform.rotation).index());
                            match position {
                                Some(position) => values[position].1 = turned,
                                None => values.push((channel, turned)),
                            }
                        }
                    }
                    (plan.turn)(&mut values, transform.rotation);
                }
                for (channel, value) in values {
                    voxels.set(channel, target, value);
                }
            }
        }
        // Where what is aboard goes: onto the same block of the merged ship, as a point carried by the same map and a turn carried by the same turn.
        let mut aboard: Vec<(protocol::EntityId, WorldPosition, Orientation)> = Vec::new();
        for id in self.data.entities().in_grid(base_id) {
            if let Some(entity) = self.data.entities().get(id) {
                aboard.push((id, entity.pose.position, entity.pose.orientation));
            }
        }
        let turn = transform.rotation.orientation();
        for id in self.data.entities().in_grid(other_id) {
            let Some(entity) = self.data.entities().get(id) else {
                continue;
            };
            let point = transform.apply_point(point_of(entity.pose.position));
            let position = position_of(point).ok_or(MergeError::OutOfRange)?;
            aboard.push((id, position, turn.compose(entity.pose.orientation)));
        }

        let mut sections = base.sections.clone();
        for section in &other.sections {
            if !sections
                .iter()
                .any(|kept| kept.identifier == section.identifier)
            {
                sections.push(section.clone());
            }
        }
        let merged = MovingGrid {
            level: base.level,
            pose: GridPose::new(base.pose.position, base.pose.orientation),
            linear_velocity: plan.linear_velocity,
            angular_velocity: plan.angular_velocity,
            mass: 0.0,
            centre_of_mass: [0.0; 3],
            held: base.held || other.held,
            frozen: base.frozen && other.frozen,
            on_pilot_logout: base.on_pilot_logout,
            voxels,
            terrain: None,
            sections,
        };
        let (_, merged_id) = self
            .data
            .grids_mut()
            .merge(
                &[(base_id, LatticeTransform::IDENTITY), (other_id, transform)],
                merged,
            )
            .expect("both ships are there and are two");
        for (id, position, orientation) in aboard {
            let _ = self
                .data
                .entities_mut()
                .set_pose(id, EntityPose::new(merged_id, position, orientation));
        }
        self.woken.push(merged_id);
        Ok(merged_id)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use entities::ComponentBag;

    use super::*;
    use crate::GridView;
    use crate::server_world::tests::{aboard, block_channel, world_with_a_walker};

    const BLOCK: u32 = 7;

    /// A ship at this pose with a block of value `BLOCK` at each of these voxels, and `temperature` beside it if there is one.
    fn ship(
        world: &mut ServerWorld,
        pose: GridPose,
        voxels: &[[i64; 3]],
        temperature: Option<u32>,
    ) -> GridId {
        let ship = world
            .create_grid(voxel_space::LevelIndex::MAIN, pose)
            .unwrap();
        let block = block_channel(world.data().main());
        let temperature_channel = temperature_channel(world);
        for &[x, y, z] in voxels {
            let mut view = world.grid(ship).unwrap();
            view.set(block, VoxelCoordinate::new(x, y, z), BLOCK)
                .unwrap();
            if let Some(value) = temperature {
                view.set(temperature_channel, VoxelCoordinate::new(x, y, z), value)
                    .unwrap();
            }
        }
        ship
    }

    fn temperature_channel(world: &ServerWorld) -> ChannelIndex {
        world
            .data()
            .main()
            .overrides
            .layout()
            .channel_index(&"core:temperature".parse().unwrap())
            .unwrap()
    }

    fn at(x: f32, y: f32, z: f32) -> WorldPosition {
        WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z])
    }

    /// The pose of a ship at this place in the world, turned a quarter about the vertical axis `quarters` times.
    fn pose(x: f32, y: f32, z: f32, quarters: f32) -> GridPose {
        GridPose::new(
            at(x, y, z),
            Orientation::from_yaw_pitch(quarters * std::f32::consts::FRAC_PI_2, 0.0),
        )
    }

    fn nothing_turns(_: &mut Vec<(ChannelIndex, u32)>, _: LatticeRotation) {}

    fn plan<'a>(world: &ServerWorld, turn: &'a dyn TurnValues) -> MergePlan<'a> {
        MergePlan {
            block_channel: block_channel(world.data().main()),
            orientation_channel: None,
            tolerance: AlignmentTolerance::default(),
            turn,
            linear_velocity: [1.0, 2.0, 3.0],
            angular_velocity: [0.0, 0.5, 0.0],
        }
    }

    /// Where a point of a grid, in voxels from its origin, is in the world.
    fn world_point(world: &ServerWorld, grid: GridId, point: [f64; 3]) -> [f64; 3] {
        let pose = world.data().grids().get(grid).unwrap().pose;
        point_of(pose.local_to_world(position_of(point).unwrap()).unwrap())
    }

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|axis| (a[axis] - b[axis]).abs() < 1e-3)
    }

    /// Whether two orientations are the same turn (a quaternion and its negative are).
    fn same_turn(a: Orientation, b: Orientation) -> bool {
        let (a, b) = (a.components(), b.components());
        let dot: f64 = (0..4).map(|axis| a[axis] * b[axis]).sum();
        dot.abs() > 1.0 - 1e-5
    }

    const FIVE: [[i64; 3]; 5] = [[0, 0, 0], [1, 0, 0], [2, 0, 0], [3, 0, 0], [4, 0, 0]];
    const THREE: [[i64; 3]; 3] = [[0, 0, 0], [1, 0, 0], [2, 0, 0]];

    #[test]
    fn two_ships_become_one_with_every_block_where_it_was_in_the_world_and_a_tombstone_for_each() {
        let (mut world, _) = world_with_a_walker();
        // The larger has five blocks along its x. The other, a quarter turn about the vertical axis and 6 voxels along from the larger's origin,
        // has three along its own x (which is along the larger's z).
        let large = ship(&mut world, pose(3.0, 40.0, 3.0, 0.0), &FIVE, None);
        let small_pose = pose(9.0, 40.0, 3.0, 1.0);
        let small = ship(&mut world, small_pose, &THREE, None);
        let turned = AtomicUsize::new(0);
        let turn = |_: &mut Vec<(ChannelIndex, u32)>, _: LatticeRotation| {
            turned.fetch_add(1, Ordering::Relaxed);
        };
        let merged = world
            .merge_grids(small, large, &plan(&world, &turn))
            .unwrap();
        // Neither old id is the new one, both are gone, and both name it.
        assert!(merged.get() > large.get().max(small.get()));
        assert!(
            world.data().grids().get(large).is_none() && world.data().grids().get(small).is_none()
        );
        assert_eq!(
            world.data().grids().resolve(large),
            [(merged, LatticeTransform::IDENTITY)]
        );
        let from_small = world.data().grids().resolve(small);
        assert_eq!(from_small.len(), 1);
        assert_eq!(from_small[0].0, merged);
        let transform = from_small[0].1;
        // The larger kept its coordinates and its place; the merged ship has eight blocks.
        let block = block_channel(world.data().main());
        let ship = world.data().grids().get(merged).unwrap();
        assert_eq!(ship.pose, pose(3.0, 40.0, 3.0, 0.0));
        let count = (-10..12)
            .flat_map(|x| (-10..12).map(move |z| VoxelCoordinate::new(x, 0, z)))
            .filter(|voxel| ship.voxels.get(block, *voxel) == BLOCK)
            .count();
        assert_eq!(count, 8);
        // Every block of the smaller is where it was in the world: the middle of its voxel and the middle of the voxel it went to are one point.
        for x in 0..3 {
            let there = transform.apply(VoxelCoordinate::new(x, 0, 0)).unwrap();
            assert_eq!(ship.voxels.get(block, there), BLOCK);
            let before = point_of(
                small_pose
                    .local_to_world(at(x as f32 + 0.5, 0.5, 0.5))
                    .unwrap(),
            );
            let now = world_point(
                &world,
                merged,
                [
                    there.x() as f64 + 0.5,
                    there.y() as f64 + 0.5,
                    there.z() as f64 + 0.5,
                ],
            );
            assert!(close(now, before), "{now:?} against {before:?}");
        }
        // The turn was asked about for the smaller's three blocks, and for none of the larger's.
        assert_eq!(turned.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn what_is_aboard_either_ship_stays_where_it_is_in_the_world_and_turned_as_it_was() {
        let (mut world, walker) = world_with_a_walker();
        let large = ship(&mut world, pose(3.0, 40.0, 3.0, 0.0), &FIVE, None);
        let small = ship(&mut world, pose(9.0, 40.0, 3.0, 1.0), &THREE, None);
        let on_large = world
            .spawn_entity(
                &walker,
                aboard(
                    large,
                    [0, 0, 0],
                    [1.5, 2.0, 0.5],
                    Orientation::from_yaw_pitch(0.3, 0.0),
                ),
                ComponentBag::new(),
            )
            .unwrap();
        let on_small = world
            .spawn_entity(
                &walker,
                aboard(
                    small,
                    [0, 0, 0],
                    [1.5, 2.0, 0.5],
                    Orientation::from_yaw_pitch(-0.2, 0.0),
                ),
                ComponentBag::new(),
            )
            .unwrap();
        let in_the_world = |world: &ServerWorld, entity| {
            let pose = world.entities().get(entity).unwrap().pose;
            let frame = world.data().grids().get(pose.grid).unwrap().pose;
            (
                point_of(frame.local_to_world(pose.position).unwrap()),
                frame.orientation.compose(pose.orientation),
            )
        };
        let (large_before, small_before) = (
            in_the_world(&world, on_large),
            in_the_world(&world, on_small),
        );
        let merged = world
            .merge_grids(large, small, &plan(&world, &nothing_turns))
            .unwrap();
        for (entity, before) in [(on_large, large_before), (on_small, small_before)] {
            assert_eq!(world.entities().get(entity).unwrap().pose.grid, merged);
            let after = in_the_world(&world, entity);
            assert!(
                close(after.0, before.0),
                "{:?} against {:?}",
                after.0,
                before.0
            );
            assert!(same_turn(after.1, before.1));
        }
    }

    #[test]
    fn the_merged_ship_goes_as_the_plan_says_is_held_if_either_was_and_the_rules_hear_of_it() {
        let (mut world, _) = world_with_a_walker();
        let a = ship(
            &mut world,
            pose(3.0, 40.0, 3.0, 0.0),
            &[[0, 0, 0], [1, 0, 0]],
            None,
        );
        let b = ship(&mut world, pose(3.0, 40.0, 8.0, 0.0), &[[0, 0, 0]], None);
        world.set_grid_held(b, true).unwrap();
        let block = block_channel(world.data().main());
        world.watch_channels([block].into_iter().collect());
        world.take_changes();
        let merged = world
            .merge_grids(a, b, &plan(&world, &nothing_turns))
            .unwrap();
        let ship = world.data().grids().get(merged).unwrap();
        assert!(ship.held);
        assert_eq!(ship.linear_velocity, [1.0, 2.0, 3.0]);
        assert_eq!(ship.angular_velocity, [0.0, 0.5, 0.0]);
        let changes = world.take_changes();
        assert_eq!(changes.len(), 3);
        assert!(changes.iter().all(|change| change.grid == merged));
    }

    #[test]
    fn it_does_not_matter_which_ship_is_named_first() {
        let make = || {
            let (mut world, _) = world_with_a_walker();
            let large = ship(&mut world, pose(3.0, 40.0, 3.0, 0.0), &THREE, None);
            let small = ship(&mut world, pose(9.0, 40.0, 3.0, 1.0), &[[0, 0, 0]], None);
            (world, large, small)
        };
        let ((mut one, large, small), (mut other, large_too, small_too)) = (make(), make());
        let first = one
            .merge_grids(large, small, &plan(&one, &nothing_turns))
            .unwrap();
        let second = other
            .merge_grids(small_too, large_too, &plan(&other, &nothing_turns))
            .unwrap();
        let (one, other) = (
            one.data().grids().get(first).unwrap(),
            other.data().grids().get(second).unwrap(),
        );
        assert_eq!(one.pose, other.pose);
        assert_eq!(one.voxels.content_digest(), other.voxels.content_digest());
    }

    #[test]
    fn values_of_a_turned_ship_are_turned_with_it_and_those_of_one_that_is_not_are_not() {
        // The turn adds a hundred to the temperature, standing for a stair that faces another way.
        let (mut world, _) = world_with_a_walker();
        let channel = temperature_channel(&world);
        let turn = move |values: &mut Vec<(ChannelIndex, u32)>, _: LatticeRotation| {
            for (found, value) in values.iter_mut() {
                if *found == channel {
                    *value += 100;
                }
            }
        };
        let large = ship(
            &mut world,
            pose(3.0, 40.0, 3.0, 0.0),
            &[[0, 0, 0], [1, 0, 0]],
            Some(55),
        );
        let small = ship(
            &mut world,
            pose(9.0, 40.0, 3.0, 1.0),
            &[[0, 0, 0]],
            Some(55),
        );
        let merged = world
            .merge_grids(large, small, &plan(&world, &turn))
            .unwrap();
        let (_, transform) = world.data().grids().resolve(small)[0];
        let there = transform.apply(VoxelCoordinate::ORIGIN).unwrap();
        let mut view = world.grid(merged).unwrap();
        assert_eq!(
            view.get(channel, there).unwrap(),
            155,
            "the smaller was turned"
        );
        assert_eq!(
            view.get(channel, VoxelCoordinate::ORIGIN).unwrap(),
            55,
            "the larger was not"
        );

        // Two ships that are not turned relative to each other: nothing is turned.
        let (mut world, _) = world_with_a_walker();
        let large = ship(
            &mut world,
            pose(3.0, 40.0, 3.0, 0.0),
            &[[0, 0, 0]],
            Some(55),
        );
        let small = ship(
            &mut world,
            pose(8.0, 40.0, 3.0, 0.0),
            &[[0, 0, 0]],
            Some(55),
        );
        let merged = world
            .merge_grids(large, small, &plan(&world, &turn))
            .unwrap();
        let mut view = world.grid(merged).unwrap();
        assert_eq!(
            view.get(channel, VoxelCoordinate::new(5, 0, 0)).unwrap(),
            55
        );
    }

    #[test]
    fn a_turn_may_give_a_voxel_a_value_in_a_channel_where_it_had_the_default() {
        // The turn gives the temperature 9 to a block that had none (a state that was the first and is not the first once turned).
        let (mut world, _) = world_with_a_walker();
        let channel = temperature_channel(&world);
        let turn = move |values: &mut Vec<(ChannelIndex, u32)>, _: LatticeRotation| {
            if !values.iter().any(|(found, _)| *found == channel) {
                values.push((channel, 9));
            }
        };
        let large = ship(
            &mut world,
            pose(3.0, 40.0, 3.0, 0.0),
            &[[0, 0, 0], [1, 0, 0]],
            None,
        );
        let small = ship(&mut world, pose(9.0, 40.0, 3.0, 1.0), &[[0, 0, 0]], None);
        let merged = world
            .merge_grids(large, small, &plan(&world, &turn))
            .unwrap();
        let (_, transform) = world.data().grids().resolve(small)[0];
        let there = transform.apply(VoxelCoordinate::ORIGIN).unwrap();
        let default = world
            .data()
            .main()
            .overrides
            .layout()
            .default_value(channel);
        let mut view = world.grid(merged).unwrap();
        assert_eq!(view.get(channel, there).unwrap(), 9);
        assert_eq!(
            view.get(channel, VoxelCoordinate::ORIGIN).unwrap(),
            default,
            "the larger ship's voxels are not asked"
        );
    }

    #[test]
    fn the_orientation_of_every_block_of_the_turned_ship_is_its_own_turn_followed_by_the_turn_that_carries_it_whatever_the_block_is()
     {
        // The channel stands in for the orientation channel (it is a number channel like it; its default, 20, is a turn too).
        let (mut world, _) = world_with_a_walker();
        let channel = temperature_channel(&world);
        let large = ship(&mut world, pose(3.0, 40.0, 3.0, 0.0), &[[0, 0, 0]], None);
        let small = ship(
            &mut world,
            pose(9.0, 40.0, 3.0, 1.0),
            &[[0, 0, 0], [1, 0, 0]],
            Some(5),
        );
        // (The second block is one the first plugin does not know: it has a number nobody knows the meaning of, and it is turned all the same.)
        let mut with_orientation = plan(&world, &nothing_turns);
        with_orientation.orientation_channel = Some(channel);
        let merged = world.merge_grids(large, small, &with_orientation).unwrap();
        let (_, transform) = world.data().grids().resolve(small)[0];
        assert_ne!(transform.rotation, LatticeRotation::IDENTITY);
        let mut view = world.grid(merged).unwrap();
        for x in 0..2 {
            let there = transform.apply(VoxelCoordinate::new(x, 0, 0)).unwrap();
            let now = view.get(channel, there).unwrap();
            let before = LatticeRotation::from_index(5).unwrap();
            let after = LatticeRotation::from_index(now as u8).unwrap();
            // Where each axis ends up is where the old turn sends it, and then the carrying turn.
            for axis in [[1, 0, 0], [0, 1, 0], [0, 0, 1]] {
                assert_eq!(
                    after.apply(axis),
                    transform.rotation.apply(before.apply(axis)),
                    "block {x}, axis {axis:?}"
                );
            }
        }
        // The larger ship's block is not turned.
        assert_eq!(view.get(channel, VoxelCoordinate::ORIGIN).unwrap(), 20);
    }

    #[test]
    fn a_ship_not_turned_to_the_lattice_keeps_every_orientation_and_a_number_that_is_no_turn_is_left_as_it_is()
     {
        let (mut world, _) = world_with_a_walker();
        let channel = temperature_channel(&world);
        let large = ship(&mut world, pose(3.0, 40.0, 3.0, 0.0), &[[0, 0, 0]], None);
        let small = ship(&mut world, pose(8.0, 40.0, 3.0, 0.0), &[[0, 0, 0]], Some(7));
        let mut with_orientation = plan(&world, &nothing_turns);
        with_orientation.orientation_channel = Some(channel);
        let merged = world.merge_grids(large, small, &with_orientation).unwrap();
        let mut view = world.grid(merged).unwrap();
        assert_eq!(view.get(channel, VoxelCoordinate::new(5, 0, 0)).unwrap(), 7);
        // A number of 24 or more is nothing the engine can turn.
        let (mut world, _) = world_with_a_walker();
        let channel = temperature_channel(&world);
        let large = ship(&mut world, pose(3.0, 40.0, 3.0, 0.0), &[[0, 0, 0]], None);
        let small = ship(
            &mut world,
            pose(9.0, 40.0, 3.0, 1.0),
            &[[0, 0, 0]],
            Some(99),
        );
        let mut with_orientation = plan(&world, &nothing_turns);
        with_orientation.orientation_channel = Some(channel);
        let merged = world.merge_grids(large, small, &with_orientation).unwrap();
        let (_, transform) = world.data().grids().resolve(small)[0];
        let there = transform.apply(VoxelCoordinate::ORIGIN).unwrap();
        assert_eq!(world.grid(merged).unwrap().get(channel, there).unwrap(), 99);
    }

    #[test]
    fn ships_that_cannot_be_merged_say_why_and_nothing_changes() {
        let (mut world, walker) = world_with_a_walker();
        let large = ship(
            &mut world,
            pose(3.0, 40.0, 3.0, 0.0),
            &[[0, 0, 0], [1, 0, 0], [2, 0, 0], [3, 0, 0]],
            None,
        );
        let on_it = world
            .spawn_entity(
                &walker,
                aboard(large, [0, 0, 0], [1.5, 2.0, 0.5], Orientation::IDENTITY),
                ComponentBag::new(),
            )
            .unwrap();
        // A block in the same place.
        let overlapping = ship(&mut world, pose(5.0, 40.0, 3.0, 0.0), &[[0, 0, 0]], None);
        assert!(matches!(
            world.merge_grids(large, overlapping, &plan(&world, &nothing_turns)),
            Err(MergeError::Overlap(_))
        ));
        // Half a voxel off the lattice.
        let off = ship(&mut world, pose(12.5, 40.0, 3.0, 0.0), &[[0, 0, 0]], None);
        assert_eq!(
            world.merge_grids(large, off, &plan(&world, &nothing_turns)),
            Err(MergeError::NotOnOneLattice)
        );
        // Not a quarter turn: a tilt.
        let tilted = ship(
            &mut world,
            GridPose::new(at(20.0, 40.0, 3.0), Orientation::from_yaw_pitch(0.4, 0.0)),
            &[[0, 0, 0]],
            None,
        );
        assert_eq!(
            world.merge_grids(large, tilted, &plan(&world, &nothing_turns)),
            Err(MergeError::NotOnOneLattice)
        );
        // Itself, and one that is not there.
        assert_eq!(
            world.merge_grids(large, large, &plan(&world, &nothing_turns)),
            Err(MergeError::SameShip)
        );
        let gone = GridId::new(GridId::FIRST_ALLOCATED + 99);
        assert_eq!(
            world.merge_grids(large, gone, &plan(&world, &nothing_turns)),
            Err(MergeError::Access(AccessError::NoSuchGrid(gone)))
        );
        // After all of that: every ship is there, nothing has a tombstone, and the walker is where it was.
        assert_eq!(world.data().grids().len(), 4);
        assert_eq!(world.data().grids().tombstones().count(), 0);
        assert_eq!(world.entities().get(on_it).unwrap().pose.grid, large);
    }
}
