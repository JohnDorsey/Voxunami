use std::collections::BTreeMap;

use voxel_grid::Grid;
use voxel_space::{GridId, GridPose, LatticeTransform, LevelIndex};

/// A grid of voxels that is not the terrain of a level: a ship. Its voxels are in its own coordinates, and its pose says where
/// those coordinates are in its level.
#[derive(Clone, Debug, PartialEq)]
pub struct MovingGrid {
    /// The level this grid is in.
    pub level: LevelIndex,
    pub pose: GridPose,
    /// The voxels. An absent brick is every channel at its default (nothing generates a ship).
    pub voxels: Grid,
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
    use voxel_space::{LatticeRotation, VoxelCoordinate};

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
            voxels,
        }
    }

    fn shift(x: i64) -> LatticeTransform {
        LatticeTransform {
            offset: [x, 0, 0],
            ..LatticeTransform::IDENTITY
        }
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
