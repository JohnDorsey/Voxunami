use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use voxel_grid::Brick;
use voxel_space::{BrickCoordinate, GridId};

/// A brick of a grid: the root grid of a level (for its terrain) or a ship with terrain.
pub(crate) type BrickKey = (GridId, BrickCoordinate);

/// How many generated bricks a server remembers.
const CAPACITY: usize = 128;

/// Recently generated bricks, so that game logic reading voxels of terrain nobody has edited does
/// not regenerate a whole brick for every read. A generated brick never changes once its region has
/// a generator (see `WorldTerrain`), so a remembered brick is always still right. When full, the
/// brick remembered longest ago is forgotten.
#[derive(Debug, Default)]
pub(crate) struct GeneratedBrickCache {
    bricks: BTreeMap<BrickKey, Arc<Brick>>,
    oldest_first: VecDeque<BrickKey>,
}

impl GeneratedBrickCache {
    pub(crate) fn get(&self, key: BrickKey) -> Option<&Arc<Brick>> {
        self.bricks.get(&key)
    }

    pub(crate) fn insert(&mut self, key: BrickKey, brick: Arc<Brick>) {
        if self.bricks.insert(key, brick).is_none() {
            self.oldest_first.push_back(key);
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
    use voxel_space::LevelIndex;

    use super::*;

    #[test]
    fn it_holds_a_limited_number_of_bricks_and_forgets_the_oldest_first() {
        let layout = StandardTestLayout::default().layout;
        let brick = Arc::new(Brick::new(&layout));
        let key = |x: i64| (GridId::MAIN, BrickCoordinate::new(x, 0, 0));
        let mut cache = GeneratedBrickCache::default();
        for x in 0..CAPACITY as i64 + 5 {
            cache.insert(key(x), Arc::clone(&brick));
        }
        assert_eq!(cache.len(), CAPACITY);
        assert!(cache.get(key(4)).is_none());
        assert!(cache.get(key(5)).is_some());
        // Putting a brick in again does not count it twice or make it newer.
        cache.insert(key(5), Arc::clone(&brick));
        assert_eq!(cache.len(), CAPACITY);
        cache.insert(key(999), Arc::clone(&brick));
        assert!(cache.get(key(5)).is_none());
        assert!(cache.get(key(6)).is_some());
    }

    #[test]
    fn the_same_brick_of_different_grids_is_remembered_separately() {
        // A level's terrain and a planet (or two levels) make the same coordinates differently.
        let layout = StandardTestLayout::default().layout;
        let (first, second) = (Arc::new(Brick::new(&layout)), Arc::new(Brick::new(&layout)));
        let coordinate = BrickCoordinate::new(1, 2, 3);
        let level = GridId::root(LevelIndex::MAIN);
        let other_level = GridId::root(LevelIndex::new(1));
        let planet = GridId::new(GridId::FIRST_ALLOCATED);
        let mut cache = GeneratedBrickCache::default();
        cache.insert((level, coordinate), Arc::clone(&first));
        assert!(cache.get((other_level, coordinate)).is_none());
        assert!(cache.get((planet, coordinate)).is_none());
        cache.insert((other_level, coordinate), Arc::clone(&second));
        cache.insert((planet, coordinate), Arc::clone(&second));
        assert_eq!(cache.len(), 3);
        assert!(Arc::ptr_eq(cache.get((level, coordinate)).unwrap(), &first));
        assert!(Arc::ptr_eq(
            cache.get((other_level, coordinate)).unwrap(),
            &second
        ));
        assert!(Arc::ptr_eq(
            cache.get((planet, coordinate)).unwrap(),
            &second
        ));
    }
}
