use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use voxel_grid::Brick;
use voxel_space::BrickCoordinate;

/// How many generated bricks a server remembers.
const CAPACITY: usize = 128;

/// Recently generated bricks, so that game logic reading voxels of terrain nobody has edited does
/// not regenerate a whole brick for every read. A generated brick never changes once its region has
/// a generator (see `WorldTerrain`), so a remembered brick is always still right. When full, the
/// brick remembered longest ago is forgotten.
#[derive(Debug, Default)]
pub(crate) struct GeneratedBrickCache {
    bricks: BTreeMap<BrickCoordinate, Arc<Brick>>,
    oldest_first: VecDeque<BrickCoordinate>,
}

impl GeneratedBrickCache {
    pub(crate) fn get(&self, coordinate: BrickCoordinate) -> Option<&Arc<Brick>> {
        self.bricks.get(&coordinate)
    }

    pub(crate) fn insert(&mut self, coordinate: BrickCoordinate, brick: Arc<Brick>) {
        if self.bricks.insert(coordinate, brick).is_none() {
            self.oldest_first.push_back(coordinate);
            if self.oldest_first.len() > CAPACITY
                && let Some(forgotten) = self.oldest_first.pop_front()
            {
                self.bricks.remove(&forgotten);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.bricks.len()
    }
}

#[cfg(test)]
mod tests {
    use voxel_grid::test_support::StandardTestLayout;

    use super::*;

    #[test]
    fn it_holds_a_limited_number_of_bricks_and_forgets_the_oldest_first() {
        let layout = StandardTestLayout::default().layout;
        let brick = Arc::new(Brick::new(&layout));
        let mut cache = GeneratedBrickCache::default();
        for x in 0..CAPACITY as i64 + 5 {
            cache.insert(BrickCoordinate::new(x, 0, 0), Arc::clone(&brick));
        }
        assert_eq!(cache.len(), CAPACITY);
        assert!(cache.get(BrickCoordinate::new(4, 0, 0)).is_none());
        assert!(cache.get(BrickCoordinate::new(5, 0, 0)).is_some());
        // Putting a brick in again does not count it twice or make it newer.
        cache.insert(BrickCoordinate::new(5, 0, 0), Arc::clone(&brick));
        assert_eq!(cache.len(), CAPACITY);
        cache.insert(BrickCoordinate::new(999, 0, 0), brick);
        assert!(cache.get(BrickCoordinate::new(5, 0, 0)).is_none());
        assert!(cache.get(BrickCoordinate::new(6, 0, 0)).is_some());
    }
}
