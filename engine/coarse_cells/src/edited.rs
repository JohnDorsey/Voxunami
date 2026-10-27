//! Where the world has been edited, as far as drawing it goes (`docs/lod-spec.md`, 9b): a cube that holds an edit is never left out as empty or buried, and nor is a
//! cube beside one, since its walls face the edit. The server follows the cubes that hold an override to find the leaves it has something to send for.
//!
//! A cube that holds an edit has an override (the server keeps one for every cube from detail level 1 to 12 that holds an edited brick, and sends it to
//! a client that draws that cube), so this counts the overrides a client or a server has: for each cube the overrides inside it or equal to it.

use std::collections::HashMap;

use region_octree::BrickCube;
use voxel_space::GridId;

use crate::selection::MAXIMUM_DETAIL_LEVEL;

/// The cubes that hold an override, counted, so that a cube can be asked whether one is in it or beside it.
#[derive(Clone, Debug, Default)]
pub struct EditedCubes {
    /// For a cube, how many overrides are the cube itself or inside it. A cube is not in the map when it has none.
    counts: HashMap<(GridId, BrickCube), u32>,
}

/// The cubes that hold `cube`, `cube` first, up to the coarsest detail level there is.
fn cube_and_holders(cube: BrickCube) -> impl Iterator<Item = BrickCube> {
    let origin = cube.origin();
    std::iter::once(cube).chain(
        (cube.level() + 1..=MAXIMUM_DETAIL_LEVEL)
            .map(move |level| BrickCube::containing(origin, level)),
    )
}

impl EditedCubes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Notes an override for `cube`. Returns whether the cube had none inside it before, so that what was left out for being empty or buried may now have to be put in.
    pub fn add(&mut self, grid: GridId, cube: BrickCube) -> bool {
        let newly_edited = !self.holds(grid, cube);
        for holder in cube_and_holders(cube) {
            *self.counts.entry((grid, holder)).or_insert(0) += 1;
        }
        newly_edited
    }

    /// Notes that an override for `cube` is gone (only one that was noted is to be taken away). Returns whether the cube has none inside it now; a cube that had
    /// none to begin with is left as it was and gives `false`.
    pub fn remove(&mut self, grid: GridId, cube: BrickCube) -> bool {
        if !self.holds(grid, cube) {
            return false;
        }
        for holder in cube_and_holders(cube) {
            let key = (grid, holder);
            if let Some(count) = self.counts.get_mut(&key) {
                *count -= 1;
                if *count == 0 {
                    self.counts.remove(&key);
                }
            }
        }
        !self.holds(grid, cube)
    }

    /// Whether an override is the cube or inside it.
    pub fn holds(&self, grid: GridId, cube: BrickCube) -> bool {
        self.counts.contains_key(&(grid, cube))
    }

    /// Whether an override is in the cube or in any of the cubes of its size that touch it, the 26 beside it.
    pub fn near(&self, grid: GridId, cube: BrickCube) -> bool {
        if self.counts.is_empty() {
            return false;
        }
        let edge = cube.edge_length() as i128;
        let origin = cube.origin();
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(beside) = origin.checked_offset(dx * edge, dy * edge, dz * edge)
                    else {
                        continue;
                    };
                    if let Ok(beside) = BrickCube::new(beside, cube.level())
                        && self.holds(grid, beside)
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Whether there are no overrides at all.
    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use voxel_space::BrickCoordinate;

    use super::*;

    fn cube(x: i128, y: i128, z: i128, level: u8) -> BrickCube {
        BrickCube::new(BrickCoordinate::new(x, y, z), level).unwrap()
    }

    #[test]
    fn a_cube_holds_what_is_inside_it_and_the_cubes_that_hold_that_do_too() {
        let mut edited = EditedCubes::new();
        let main = GridId::MAIN;
        assert!(edited.add(main, cube(4, 0, 2, 1)), "the first in the cube");
        assert!(edited.holds(main, cube(4, 0, 2, 1)));
        assert!(edited.holds(main, cube(4, 0, 0, 2)));
        assert!(edited.holds(main, cube(0, 0, 0, 3)));
        // Up to the coarsest level there is, the root cube of the world, past the old limit of 40.
        let around = |level| BrickCube::containing(BrickCoordinate::new(4, 0, 2), level);
        for level in [40, 41, 64, 100, MAXIMUM_DETAIL_LEVEL] {
            assert!(edited.holds(main, around(level)), "level {level}");
        }
        assert!(!edited.holds(
            main,
            BrickCube::containing(BrickCoordinate::new(-4, 0, 2), 100)
        ));
        assert!(
            !edited.holds(main, cube(6, 0, 2, 1)),
            "the cube beside it holds nothing"
        );
        assert!(
            !edited.holds(main, cube(4, 0, 0, 0)),
            "nor does a brick of it"
        );
        assert!(!edited.holds(
            GridId::root(voxel_space::LevelIndex::new(1)),
            cube(4, 0, 2, 1)
        ));
    }

    #[test]
    fn an_edit_is_new_to_a_cube_only_the_first_time_and_clears_when_the_last_goes() {
        let mut edited = EditedCubes::new();
        let main = GridId::MAIN;
        assert!(edited.add(main, cube(0, 0, 0, 1)));
        // Another in the cube above is new to the cube it is at, and not to the one that held the first.
        assert!(edited.add(main, cube(2, 0, 0, 1)));
        assert!(!edited.add(main, cube(0, 0, 0, 1)), "the same again");
        assert!(!edited.remove(main, cube(0, 0, 0, 1)), "one is left");
        assert!(edited.remove(main, cube(0, 0, 0, 1)));
        assert!(!edited.holds(main, cube(0, 0, 0, 1)));
        assert!(edited.holds(main, cube(0, 0, 0, 2)), "the other is in it");
        assert!(edited.remove(main, cube(2, 0, 0, 1)));
        assert!(edited.is_empty());
        // Taking away what was never there changes nothing.
        assert!(!edited.remove(main, cube(8, 8, 8, 1)));
        assert!(edited.is_empty());
    }

    #[test]
    fn near_is_the_cube_and_the_twenty_six_beside_it_at_its_size_and_no_more() {
        let mut edited = EditedCubes::new();
        let main = GridId::MAIN;
        assert!(!edited.near(main, cube(0, 0, 0, 2)), "nothing is edited");
        edited.add(main, cube(8, 8, 8, 2));
        // The cubes of 4 bricks touching the one at (8, 8, 8): from (4, 4, 4) to (12, 12, 12).
        for (x, y, z) in [(8, 8, 8), (4, 4, 4), (12, 12, 12), (4, 8, 12), (8, 12, 8)] {
            assert!(edited.near(main, cube(x, y, z, 2)), "{x} {y} {z}");
        }
        for (x, y, z) in [(0, 8, 8), (16, 8, 8), (8, 8, 16), (8, 0, 8), (16, 16, 16)] {
            assert!(!edited.near(main, cube(x, y, z, 2)), "{x} {y} {z}");
        }
        // A bigger cube that holds it is near it, and so is one beside the one that holds it.
        assert!(edited.near(main, cube(0, 0, 0, 3)));
        assert!(edited.near(main, cube(16, 0, 0, 3)));
        assert!(!edited.near(main, cube(32, 0, 0, 3)));
        assert!(!edited.near(
            GridId::root(voxel_space::LevelIndex::new(1)),
            cube(8, 8, 8, 2)
        ));
    }

    #[test]
    fn cubes_at_the_edge_of_the_world_have_fewer_neighbours_and_no_overflow() {
        let mut edited = EditedCubes::new();
        let low = voxel_space::BRICK_COORDINATE_MINIMUM;
        edited.add(GridId::MAIN, cube(low, low, low, 3));
        assert!(edited.near(GridId::MAIN, cube(low, low, low, 3)));
        assert!(edited.near(GridId::MAIN, cube(low + 8, low, low, 3)));
        assert!(!edited.near(GridId::MAIN, cube(low + 16, low, low, 3)));
    }
}
