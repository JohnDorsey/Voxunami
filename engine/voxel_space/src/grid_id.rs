use std::fmt;

use crate::LevelIndex;

/// Which grid of voxels: the address a rule uses for the voxels it looks at.
///
/// A level's terrain is its *root grid*, and the id of a root grid is the level's index. Every other
/// grid (a ship, a creature, a tile) has an id allocated from a counter that is saved with the world,
/// starting at [`GridId::FIRST_ALLOCATED`], so an id means the same grid for as long as the world
/// lives, in a recording and after a reload. Until structures are built only root grids exist.
///
/// Ids order by number, so a rule that looks at its voxels "by grid, then by position" looks at the
/// root grids in level order first, exactly as it did when it was keyed by level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GridId(u64);

impl GridId {
    /// The root grid of the main level, where players start.
    pub const MAIN: Self = Self(0);

    /// The first id that is not a root grid.
    pub const FIRST_ALLOCATED: u64 = 1 << 32;

    /// The root grid of a level.
    pub const fn root(level: LevelIndex) -> Self {
        Self(level.get() as u64)
    }

    /// The grid with this number, root or allocated.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    /// The level this is the root grid of, or `None` for any other grid.
    pub const fn root_level(self) -> Option<LevelIndex> {
        if self.0 < Self::FIRST_ALLOCATED {
            Some(LevelIndex::new(self.0 as u32))
        } else {
            None
        }
    }
}

impl fmt::Display for GridId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "grid {}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_root_grid_has_its_levels_number_and_is_found_again_from_it() {
        assert_eq!(GridId::MAIN, GridId::root(LevelIndex::MAIN));
        for raw in [0, 1, 7, u32::MAX] {
            let level = LevelIndex::new(raw);
            let grid = GridId::root(level);
            assert_eq!(grid.get(), u64::from(raw));
            assert_eq!(grid.root_level(), Some(level));
        }
    }

    #[test]
    fn allocated_grids_are_not_roots_and_come_after_every_root() {
        let allocated = GridId::new(GridId::FIRST_ALLOCATED);
        assert_eq!(allocated.root_level(), None);
        assert!(GridId::root(LevelIndex::new(u32::MAX)) < allocated);
        assert!(allocated < GridId::new(GridId::FIRST_ALLOCATED + 1));
        assert_eq!(GridId::new(5).to_string(), "grid 5");
    }
}
