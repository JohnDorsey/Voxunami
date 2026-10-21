use std::collections::BTreeMap;

use voxel_grid::Grid;
use voxel_space::{
    BrickBox, BrickCoordinate, GridId, GridPose, LatticeTransform, LevelIndex, WorldPosition,
};

/// A grid of voxels that is not the terrain of a level: a ship. Its voxels are in its own coordinates, and its pose says where
/// those coordinates are in its level.
#[derive(Clone, Debug, PartialEq)]
pub struct MovingGrid {
    /// The level this grid is in.
    pub level: LevelIndex,
    pub pose: GridPose,
    /// How fast the origin of the ship moves, in voxels a second, and how fast it turns (an axis scaled by radians a second). Bodies
    /// write them as they move the ship; a ship made anew has none.
    pub linear_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    /// How much the ship weighs in kilograms and where its centre of mass is in its own coordinates, as the bodies work it out from its blocks
    /// (zero where nothing has). A ship of enough mass pulls things toward its centre of mass (see `voxel_space::gravity_at`).
    pub mass: f32,
    pub centre_of_mass: [f32; 3],
    /// A held ship does not move: gravity, forces and collisions leave it where it is, and everything else collides with it.
    pub held: bool,
    /// The voxels. An absent brick is every channel at its default (nothing generates a ship).
    pub voxels: Grid,
}

impl MovingGrid {
    /// The lowest and the highest brick that has anything in it, along each axis, in the ship's own coordinates. `None` for a ship with
    /// no bricks.
    pub fn brick_extent(&self) -> Option<(BrickCoordinate, BrickCoordinate)> {
        let mut bounds: Option<(BrickCoordinate, BrickCoordinate)> = None;
        for (brick, _) in self.voxels.bricks() {
            bounds = Some(match bounds {
                None => (brick, brick),
                Some((low, high)) => (
                    BrickCoordinate::new(
                        low.x().min(brick.x()),
                        low.y().min(brick.y()),
                        low.z().min(brick.z()),
                    ),
                    BrickCoordinate::new(
                        high.x().max(brick.x()),
                        high.y().max(brick.y()),
                        high.z().max(brick.z()),
                    ),
                ),
            });
        }
        bounds
    }

    /// How fast a place in the world moves because the ship carries it, in voxels a second in the world's axes: the speed of the ship's centre
    /// of mass, and what its turning adds, which is the turn (an axis scaled by radians a second) crossed with the vector from the
    /// centre of mass to the place. It is the speed on record: a ship that is held has none until whoever moves it says how fast.
    pub fn velocity_at(&self, place: WorldPosition) -> [f32; 3] {
        let centre = WorldPosition::new(BrickCoordinate::ORIGIN, self.centre_of_mass);
        let Some(centre) = self.pose.local_to_world(centre) else {
            return self.linear_velocity;
        };
        let arm = place.relative_to(centre).map(f64::from);
        let turn = self.angular_velocity.map(f64::from);
        let carried = [
            turn[1] * arm[2] - turn[2] * arm[1],
            turn[2] * arm[0] - turn[0] * arm[2],
            turn[0] * arm[1] - turn[1] * arm[0],
        ];
        [0, 1, 2].map(|axis| (f64::from(self.linear_velocity[axis]) + carried[axis]) as f32)
    }

    /// The bricks of the level (its root grid's, in the level's coordinates) that the ship's bricks are in or touch, as a box: the box
    /// around the turned box of its bricks. `None` for a ship with no bricks, or one whose bricks are outside the supported range in
    /// the world. It is a bound, not a fit: a ship turned half way across the diagonal is covered by a box larger than it.
    pub fn world_brick_bounds(&self) -> Option<BrickBox> {
        let (low, high) = self.brick_extent()?;
        // The corners of the box of voxels, in the ship's own coordinates: from the first voxel of the lowest brick to the voxel
        // after the last of the highest.
        let mut corners = Vec::with_capacity(8);
        for x in [low.x(), high.x() + 1] {
            for y in [low.y(), high.y() + 1] {
                for z in [low.z(), high.z() + 1] {
                    let corner = BrickCoordinate::try_new(x, y, z).ok()?;
                    corners.push(
                        self.pose
                            .local_to_world(WorldPosition::new(corner, [0.0; 3]))?,
                    );
                }
            }
        }
        // The bricks the box touches run from the brick of its lowest corner to the brick of its highest, where a highest corner exactly
        // on the boundary between two bricks touches only the one below.
        let mut min = corners[0].brick();
        let mut max = min;
        for corner in &corners {
            let brick = corner.brick();
            let offset = corner.offset();
            let upper = |value: i64, offset: f32| if offset != 0.0 { value } else { value - 1 };
            min = BrickCoordinate::new(
                min.x().min(brick.x()),
                min.y().min(brick.y()),
                min.z().min(brick.z()),
            );
            max = BrickCoordinate::new(
                max.x().max(upper(brick.x(), offset[0])),
                max.y().max(upper(brick.y(), offset[1])),
                max.z().max(upper(brick.z(), offset[2])),
            );
        }
        BrickBox::new(min, max)
    }
}

/// The grids that are not the root grids of levels, and the ids they had before they were split or merged.
///
/// Ids come from a counter that only goes up, starting at [`GridId::FIRST_ALLOCATED`], so an id is never given twice in a world and a
/// recording and a reload agree about which grid it names. A grid that is split or merged has its id retired: it leaves a tombstone naming
/// the grids that took its place and the transform from its voxels to theirs, so a saved reference to it still resolves
/// ([`GridStore::resolve`]).
#[derive(Clone, Debug, PartialEq)]
pub struct GridStore {
    next: u64,
    grids: BTreeMap<GridId, MovingGrid>,
    tombstones: BTreeMap<GridId, Vec<(GridId, LatticeTransform)>>,
}

impl Default for GridStore {
    fn default() -> Self {
        Self::new()
    }
}

impl GridStore {
    pub fn new() -> Self {
        Self {
            next: GridId::FIRST_ALLOCATED,
            grids: BTreeMap::new(),
            tombstones: BTreeMap::new(),
        }
    }

    fn allocate(&mut self) -> GridId {
        let id = GridId::new(self.next);
        self.next += 1;
        id
    }

    /// Adds a grid under a new id.
    pub fn create(&mut self, grid: MovingGrid) -> GridId {
        let id = self.allocate();
        self.grids.insert(id, grid);
        id
    }

    pub fn get(&self, id: GridId) -> Option<&MovingGrid> {
        self.grids.get(&id)
    }

    pub fn get_mut(&mut self, id: GridId) -> Option<&mut MovingGrid> {
        self.grids.get_mut(&id)
    }

    /// The grids, in order of id.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (GridId, &MovingGrid)> {
        self.grids.iter().map(|(id, grid)| (*id, grid))
    }

    pub fn len(&self) -> usize {
        self.grids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.grids.is_empty()
    }

    /// Removes a grid for good (a ship that was destroyed), leaving no tombstone: nothing took its place.
    pub fn remove(&mut self, id: GridId) -> Option<MovingGrid> {
        self.grids.remove(&id)
    }

    /// Replaces a grid by others, as a split or a merge does: each of `parts` is added under a new id (so none keeps the id of
    /// the grid, and every one is greater than it), the grid is removed, and its id leaves a tombstone naming the parts with the
    /// transform from the old grid's voxels to each. Returns the old grid and the new ids in order, or `None`, changing nothing, if
    /// there is no such grid.
    pub fn retire(
        &mut self,
        id: GridId,
        parts: Vec<(MovingGrid, LatticeTransform)>,
    ) -> Option<(MovingGrid, Vec<GridId>)> {
        let old = self.grids.remove(&id)?;
        let mut successors = Vec::with_capacity(parts.len());
        for (grid, transform) in parts {
            successors.push((self.create(grid), transform));
        }
        let ids = successors.iter().map(|(id, _)| *id).collect();
        self.tombstones.insert(id, successors);
        Some((old, ids))
    }

    /// Replaces several grids by one, as a merge does: `merged` is added under a new id (so it keeps none of theirs, and the id is
    /// greater than all of them), the grids are removed, and each id leaves a tombstone naming the merged grid with the transform from
    /// that grid's voxels to its. Returns the old grids in the order given and the new id, or `None`, changing nothing, if an id is
    /// not a grid or is named twice.
    pub fn merge(
        &mut self,
        grids: &[(GridId, LatticeTransform)],
        merged: MovingGrid,
    ) -> Option<(Vec<MovingGrid>, GridId)> {
        let distinct = grids
            .iter()
            .enumerate()
            .all(|(index, (id, _))| !grids[..index].iter().any(|(earlier, _)| earlier == id));
        if !distinct || !grids.iter().all(|(id, _)| self.grids.contains_key(id)) {
            return None;
        }
        let old = grids
            .iter()
            .map(|(id, _)| self.grids.remove(id).expect("the grid exists"))
            .collect();
        let merged_id = self.create(merged);
        for &(id, transform) in grids {
            self.tombstones.insert(id, vec![(merged_id, transform)]);
        }
        Some((old, merged_id))
    }

    /// The grids that now hold what the grid with this id held, each with the transform from the old id's voxels to its own: the
    /// grid itself (with no transform) if it still exists, the parts it was split into or the grid it was merged into (following
    /// further splits and merges), or nothing if the grid is gone.
    pub fn resolve(&self, id: GridId) -> Vec<(GridId, LatticeTransform)> {
        if self.grids.contains_key(&id) {
            return vec![(id, LatticeTransform::IDENTITY)];
        }
        let Some(successors) = self.tombstones.get(&id) else {
            return Vec::new();
        };
        successors
            .iter()
            .flat_map(|&(successor, first)| {
                self.resolve(successor)
                    .into_iter()
                    .filter_map(move |(grid, next)| Some((grid, first.then(next)?)))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use voxel_grid::{ChannelDefinition, GridLayout};
    use voxel_space::{LatticeRotation, Orientation, VoxelCoordinate};

    use super::*;

    fn grid(marker: u32) -> MovingGrid {
        let layout = Arc::new(
            GridLayout::new(vec![ChannelDefinition::number(
                "test:marker".parse().unwrap(),
                0,
            )])
            .unwrap(),
        );
        let mut voxels = Grid::new(layout);
        let channel = voxels.layout().channel_indices().next().unwrap();
        voxels.set(channel, VoxelCoordinate::new(1, 2, 3), marker);
        MovingGrid {
            level: LevelIndex::MAIN,
            pose: GridPose::identity(),
            linear_velocity: [0.0; 3],
            angular_velocity: [0.0; 3],
            mass: 0.0,
            centre_of_mass: [0.0; 3],
            held: false,
            voxels,
        }
    }

    #[test]
    fn a_place_on_a_ship_moves_with_the_ship_and_a_turning_ship_carries_places_around_its_centre_of_mass()
     {
        let mut ship = grid(1);
        ship.pose = GridPose::new(
            WorldPosition::new(BrickCoordinate::new(2, 0, 0), [1.0, 2.0, 3.0]),
            // Turned a quarter about the vertical axis: its x is the world's -z.
            Orientation::from_yaw_pitch(std::f32::consts::FRAC_PI_2, 0.0),
        );
        ship.centre_of_mass = [4.0, 1.0, 4.0];
        ship.linear_velocity = [1.0, 2.0, 3.0];
        let there =
            |x: f32, y: f32, z: f32| WorldPosition::new(BrickCoordinate::new(2, 0, 0), [x, y, z]);
        // Not turning: every place moves at the ship's speed.
        assert_eq!(ship.velocity_at(there(7.0, 2.0, 9.0)), [1.0, 2.0, 3.0]);
        // Turning at 2 radians a second about the vertical axis. The centre of mass is the ship's (4, 1, 4) turned: from the origin
        // at (1, 2, 3), 4 along the ship's x is 4 along the world's -z and 4 along its z is 4 along the world's x, so it is at
        // (5, 3, -1) from the brick's corner. A place 3 along the world's +x from it goes round: omega x r = (0, 2, 0) x (3, 0, 0) =
        // (0, 0, -6).
        ship.angular_velocity = [0.0, 2.0, 0.0];
        let centre = there(5.0, 3.0, -1.0);
        assert_eq!(ship.velocity_at(centre), [1.0, 2.0, 3.0]);
        let moved = ship.velocity_at(there(8.0, 3.0, -1.0));
        assert!(
            (moved[0] - 1.0).abs() < 1e-5
                && (moved[1] - 2.0).abs() < 1e-5
                && (moved[2] - (3.0 - 6.0)).abs() < 1e-5,
            "{moved:?}"
        );
        // A place above it, 2 up, with the turn about the world's x axis (1 radian a second): (1, 0, 0) x (0, 2, 0) = (0, 0, 2).
        ship.angular_velocity = [1.0, 0.0, 0.0];
        let above = ship.velocity_at(there(5.0, 5.0, -1.0));
        assert!(
            (above[0] - 1.0).abs() < 1e-5
                && (above[1] - 2.0).abs() < 1e-5
                && (above[2] - 5.0).abs() < 1e-5,
            "{above:?}"
        );
        // A ship is as fast as is on record, held or not: a held ship that something moves goes at the speed it was told.
        ship.held = true;
        assert_eq!(ship.velocity_at(there(5.0, 5.0, -1.0)), above);
    }

    #[test]
    fn a_ship_turning_about_no_particular_axis_carries_a_place_by_the_cross_product_in_all_three_components()
     {
        // Turning (1, 2, 3) radians a second, 4 along x, 5 up and 6 along z from the centre of mass, which is the origin of an unturned ship:
        // (1, 2, 3) x (4, 5, 6) = (2 * 6 - 3 * 5, 3 * 4 - 1 * 6, 1 * 5 - 2 * 4) = (-3, 6, -3). Every term of every component is not zero.
        let mut ship = grid(1);
        ship.angular_velocity = [1.0, 2.0, 3.0];
        let place = WorldPosition::new(BrickCoordinate::ORIGIN, [4.0, 5.0, 6.0]);
        assert_eq!(ship.velocity_at(place), [-3.0, 6.0, -3.0]);
        // With the ship's own speed added.
        ship.linear_velocity = [10.0, 20.0, 30.0];
        assert_eq!(ship.velocity_at(place), [7.0, 26.0, 27.0]);
    }

    fn shift(x: i64) -> LatticeTransform {
        LatticeTransform {
            offset: [x, 0, 0],
            ..LatticeTransform::IDENTITY
        }
    }

    /// A ship with a brick at each of these places and this pose.
    fn ship_with_bricks(bricks: &[[i64; 3]], pose: GridPose) -> MovingGrid {
        let mut ship = grid(1);
        ship.voxels = Grid::new(Arc::clone(ship.voxels.layout()));
        let channel = ship.voxels.layout().channel_indices().next().unwrap();
        for brick in bricks {
            let voxel = BrickCoordinate::new(brick[0], brick[1], brick[2]).origin();
            ship.voxels.set(channel, voxel, 1);
        }
        ship.pose = pose;
        ship
    }

    fn bounds(ship: &MovingGrid) -> Option<([i64; 3], [i64; 3])> {
        ship.world_brick_bounds().map(|bounds| {
            let (min, max) = (bounds.min(), bounds.max());
            ([min.x(), min.y(), min.z()], [max.x(), max.y(), max.z()])
        })
    }

    #[test]
    fn the_bricks_a_ship_touches_in_the_world_are_found_for_a_move_a_turn_and_a_ship_with_none() {
        let at = |x: i64, offset: [f32; 3]| {
            GridPose::new(
                WorldPosition::new(BrickCoordinate::new(x, 0, 0), offset),
                voxel_space::Orientation::IDENTITY,
            )
        };
        let bricks = [[0, 0, 0], [2, 1, 0]];
        assert_eq!(bounds(&ship_with_bricks(&[], at(0, [0.0; 3]))), None);
        // Where the bricks of the ship are is where the bricks of the world are, on the lattice ...
        assert_eq!(
            bounds(&ship_with_bricks(&bricks, at(0, [0.0; 3]))),
            Some(([0, 0, 0], [2, 1, 0]))
        );
        assert_eq!(
            bounds(&ship_with_bricks(&bricks, at(10, [0.0; 3]))),
            Some(([10, 0, 0], [12, 1, 0]))
        );
        // ... and half a brick off it, the ship reaches into one more brick.
        assert_eq!(
            bounds(&ship_with_bricks(&bricks, at(0, [16.0, 0.0, 0.0]))),
            Some(([0, 0, 0], [3, 1, 0]))
        );
        // A quarter turn about the vertical axis takes x onto -z and z onto x: bricks 0 to 1 along x become -2 to -1 along z.
        let quarter = LatticeRotation::all()
            .into_iter()
            .find(|rotation| {
                rotation.apply([1, 0, 0]) == [0, 0, -1] && rotation.apply([0, 1, 0]) == [0, 1, 0]
            })
            .unwrap();
        let turned = GridPose::new(
            WorldPosition::new(BrickCoordinate::new(0, 0, 0), [0.0; 3]),
            quarter.orientation(),
        );
        let (low, high) = bounds(&ship_with_bricks(&[[0, 0, 0], [1, 0, 0]], turned)).unwrap();
        // (The quaternion of a quarter turn is not exact in 32 bits, so a face that should be on a boundary may be a hair over it: the
        // bound may be a brick wider, never narrower.)
        assert!(
            (-1..=0).contains(&low[0]) && (0..=1).contains(&high[0]),
            "{low:?} {high:?}"
        );
        assert!(
            (-1..=0).contains(&low[1]) && (0..=1).contains(&high[1]),
            "{low:?} {high:?}"
        );
        assert!(
            (-3..=-2).contains(&low[2]) && (-1..=0).contains(&high[2]),
            "{low:?} {high:?}"
        );
        // At an angle the box of the world is larger than the ship.
        let slanted = GridPose::new(
            WorldPosition::new(BrickCoordinate::new(0, 0, 0), [0.0; 3]),
            voxel_space::Orientation::from_yaw_pitch(std::f32::consts::FRAC_PI_4, 0.0),
        );
        let (low, high) = bounds(&ship_with_bricks(&[[0, 0, 0]], slanted)).unwrap();
        assert!(low[0] <= 0 && high[0] >= 1 && low[2] <= -1 && high[2] >= 0);
    }

    #[test]
    fn ids_start_after_the_root_grids_and_are_never_given_twice() {
        let mut store = GridStore::new();
        let first = store.create(grid(1));
        let second = store.create(grid(2));
        assert_eq!(first.get(), GridId::FIRST_ALLOCATED);
        assert_eq!(second.get(), GridId::FIRST_ALLOCATED + 1);
        assert_eq!(first.root_level(), None);
        // A removed grid's id is not reused.
        assert!(store.remove(first).is_some());
        assert_eq!(store.create(grid(3)).get(), GridId::FIRST_ALLOCATED + 2);
        assert_eq!(store.len(), 2);
        assert_eq!(
            store.iter().map(|(id, _)| id.get()).collect::<Vec<_>>(),
            [GridId::FIRST_ALLOCATED + 1, GridId::FIRST_ALLOCATED + 2]
        );
    }

    #[test]
    fn a_removed_grid_is_gone_and_resolves_to_nothing() {
        let mut store = GridStore::new();
        let id = store.create(grid(1));
        assert_eq!(store.resolve(id), [(id, LatticeTransform::IDENTITY)]);
        assert_eq!(store.remove(id), Some(grid(1)));
        assert_eq!(store.remove(id), None);
        assert_eq!(store.get(id), None);
        assert_eq!(store.resolve(id), []);
        assert_eq!(store.resolve(GridId::MAIN), []);
    }

    #[test]
    fn splitting_gives_every_part_a_new_id_and_the_old_one_resolves_to_all_of_them() {
        let mut store = GridStore::new();
        let ship = store.create(grid(1));
        let (old, parts) = store
            .retire(
                ship,
                vec![
                    (grid(2), shift(0)),
                    (grid(3), shift(5)),
                    (grid(4), shift(9)),
                ],
            )
            .unwrap();
        assert_eq!(old, grid(1));
        assert_eq!(parts.len(), 3);
        assert!(parts.iter().all(|part| *part > ship));
        assert!(!parts.contains(&ship));
        assert_eq!(store.get(ship), None);
        assert_eq!(
            store.resolve(ship),
            [
                (parts[0], shift(0)),
                (parts[1], shift(5)),
                (parts[2], shift(9))
            ]
        );
        // An id that was never a grid still resolves to nothing, and retiring a missing grid changes nothing.
        assert_eq!(store.retire(ship, vec![(grid(9), shift(0))]), None);
        assert_eq!(store.len(), 3);
    }

    #[test]
    fn a_merge_gives_the_result_a_new_id_and_both_old_ids_resolve_to_it() {
        let mut store = GridStore::new();
        let (left, right) = (store.create(grid(1)), store.create(grid(2)));
        let (old, merged) = store
            .merge(&[(left, shift(0)), (right, shift(7))], grid(3))
            .unwrap();
        assert_eq!(old, [grid(1), grid(2)]);
        assert!(merged > left && merged > right);
        assert_eq!(store.len(), 1);
        assert_eq!(store.resolve(left), [(merged, shift(0))]);
        assert_eq!(store.resolve(right), [(merged, shift(7))]);
        // The merged grid is split later: both old ids now lead to the parts, with the transforms of both steps.
        let (_, parts) = store
            .retire(merged, vec![(grid(5), shift(100)), (grid(6), shift(200))])
            .unwrap();
        assert_eq!(
            store.resolve(left),
            [(parts[0], shift(100)), (parts[1], shift(200))]
        );
        let voxel = VoxelCoordinate::new(1, 2, 3);
        let (found, transform) = store.resolve(right)[1];
        assert_eq!(found, parts[1]);
        assert_eq!(
            transform.apply(voxel),
            Some(VoxelCoordinate::new(208, 2, 3))
        );
    }

    #[test]
    fn a_merge_that_names_a_missing_grid_or_the_same_grid_twice_changes_nothing() {
        let mut store = GridStore::new();
        let (left, right) = (store.create(grid(1)), store.create(grid(2)));
        let before = store.clone();
        let missing = GridId::new(GridId::FIRST_ALLOCATED + 50);
        assert_eq!(
            store.merge(&[(left, shift(0)), (missing, shift(0))], grid(3)),
            None
        );
        assert_eq!(
            store.merge(&[(left, shift(0)), (left, shift(1))], grid(3)),
            None
        );
        assert_eq!(store, before);
        assert!(store.merge(&[(right, shift(0))], grid(3)).is_some());
    }

    #[test]
    fn turns_along_a_chain_compose_in_order() {
        let mut store = GridStore::new();
        let first = store.create(grid(1));
        // A turn that moves the x axis, so that the order of the two steps shows.
        let turn = LatticeRotation::all()
            .into_iter()
            .find(|rotation| rotation.apply([1, 0, 0]) != [1, 0, 0])
            .unwrap();
        let there = LatticeTransform {
            rotation: turn,
            offset: [10, 0, 0],
        };
        let (_, second) = store.retire(first, vec![(grid(2), there)]).unwrap();
        let (_, third) = store
            .retire(second[0], vec![(grid(3), shift(1000))])
            .unwrap();
        let (found, transform) = store.resolve(first)[0];
        assert_eq!(found, third[0]);
        let voxel = VoxelCoordinate::new(4, 5, 6);
        let expected = shift(1000).apply(there.apply(voxel).unwrap());
        assert_eq!(transform.apply(voxel), expected);
    }
}
