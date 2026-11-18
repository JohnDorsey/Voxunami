//! The coarse nodes the server has told the session of, kept for as long as the session lasts (`docs/specs/far-nodes-spec.md`, 3.5 and F1).
//!
//! The server keeps a record of what it has sent to each client and does not send a node again while the client has it in its latest version. So the client must not lose what
//! it was sent: a view that is made again (the player goes to a level the game draws differently, and the view of the terrain is made for it) is given what is held here.

use std::collections::{HashMap, HashSet};

use coarse_cells::CoarseNode;
use coarse_streaming::CoarseNodeUpdate;
use region_octree::BrickCube;
use voxel_space::GridId;

/// What the server said a node is, by grid and cube, as of the last message of each.
#[derive(Debug, Default)]
pub(crate) struct CoarseOverrides {
    held: HashMap<(GridId, BrickCube), CoarseNode>,
    /// The cubes the server said have no node: for a far ship, which has no generator, that is that they are empty (and so is what is inside them), and a view that
    /// waits for the server to say what is in a cube must be told again when it is made afresh.
    said_empty: HashSet<(GridId, BrickCube)>,
}

impl CoarseOverrides {
    /// Takes in what the server said of a node: its new cells, or (with no node) that it is no longer different from what the generators make, which for a ship that
    /// has no generator is that the cube is empty.
    pub(crate) fn apply(&mut self, update: &CoarseNodeUpdate) {
        let key = (update.grid, update.cube);
        match &update.node {
            Some(node) => {
                self.held.insert(key, node.clone());
                self.said_empty.remove(&key);
            }
            None => {
                self.held.remove(&key);
                self.said_empty.insert(key);
            }
        }
    }

    /// Forgets every node of a grid: the server tells of it again when the grid comes back (a ship that was far and is near, or the other way round, is told of afresh).
    pub(crate) fn forget_grid(&mut self, grid: GridId) {
        self.held.retain(|(held, _), _| *held != grid);
        self.said_empty.retain(|(held, _)| *held != grid);
    }

    /// The cubes of ships (grids that are not the root of a level) that the server said have no node, in an order that is the same whenever the same is held (finest
    /// first, then by grid and place).
    pub(crate) fn empties_of_ships_in_order(&self) -> Vec<(GridId, BrickCube)> {
        let mut all: Vec<(GridId, BrickCube)> = self
            .said_empty
            .iter()
            .copied()
            .filter(|(grid, _)| grid.root_level().is_none())
            .collect();
        all.sort_by_key(|(grid, cube)| {
            let origin = cube.origin();
            (cube.level(), *grid, [origin.x(), origin.y(), origin.z()])
        });
        all
    }

    /// Tells `tell` everything held of `grid`, to give to a view that is made now: the nodes (finest first), and then the cubes said to have no node (`None`).
    pub(crate) fn tell_of_a_grid(
        &self,
        grid: GridId,
        mut tell: impl FnMut(BrickCube, Option<CoarseNode>),
    ) {
        for (held, cube, node) in self.in_order() {
            if held == grid {
                tell(cube, Some(node.clone()));
            }
        }
        for (empty, cube) in self.empties_of_ships_in_order() {
            if empty == grid {
                tell(cube, None);
            }
        }
    }

    /// The ships (grids that are not the root of a level) there is a node held of, or one said to be empty, in order.
    pub(crate) fn ships(&self) -> Vec<GridId> {
        let mut ships: Vec<GridId> = self
            .held
            .keys()
            .chain(self.said_empty.iter())
            .map(|(grid, _)| *grid)
            .filter(|grid| grid.root_level().is_none())
            .collect();
        ships.sort_unstable();
        ships.dedup();
        ships
    }

    /// Everything held, in an order that is the same whenever the same is held (finest first, then by grid and place), so that a view given it is made the same way each time.
    pub(crate) fn in_order(&self) -> Vec<(GridId, BrickCube, &CoarseNode)> {
        let mut all: Vec<_> = self
            .held
            .iter()
            .map(|(&(grid, cube), node)| (grid, cube, node))
            .collect();
        all.sort_by_key(|(grid, cube, _)| {
            let origin = cube.origin();
            (cube.level(), *grid, [origin.x(), origin.y(), origin.z()])
        });
        all
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.held.len()
    }
}

#[cfg(test)]
mod tests {
    use coarse_cells::CoarseCell;
    use voxel_space::BrickCoordinate;

    use super::*;

    fn cube(x: i128, level: u8) -> BrickCube {
        BrickCube::new(BrickCoordinate::new(x, 0, 0), level).expect("the origin is aligned")
    }

    fn node(block: u32) -> CoarseNode {
        CoarseNode::uniform(CoarseCell {
            block,
            fill: 255,
            light: 255,
            exposure: 0,
        })
    }

    fn said(grid: GridId, cube: BrickCube, node: Option<CoarseNode>) -> CoarseNodeUpdate {
        CoarseNodeUpdate { grid, cube, node }
    }

    #[test]
    fn a_node_that_is_said_again_replaces_the_one_before_and_one_that_is_taken_back_is_gone() {
        let mut held = CoarseOverrides::default();
        held.apply(&said(GridId::MAIN, cube(0, 2), Some(node(1))));
        held.apply(&said(GridId::MAIN, cube(0, 2), Some(node(2))));
        assert_eq!(held.len(), 1);
        assert_eq!(held.in_order()[0].2, &node(2));
        held.apply(&said(GridId::MAIN, cube(0, 2), None));
        assert_eq!(held.len(), 0);
        // (Taking back what was never said changes nothing.)
        held.apply(&said(GridId::MAIN, cube(8, 2), None));
        assert_eq!(held.len(), 0);
    }

    #[test]
    fn nodes_of_the_same_cube_in_other_grids_are_other_nodes() {
        let mut held = CoarseOverrides::default();
        let other = GridId::root(voxel_space::LevelIndex::new(1));
        held.apply(&said(GridId::MAIN, cube(0, 2), Some(node(1))));
        held.apply(&said(other, cube(0, 2), Some(node(2))));
        assert_eq!(held.len(), 2);
        held.apply(&said(other, cube(0, 2), None));
        let left = held.in_order();
        assert_eq!(left.len(), 1);
        assert_eq!((left[0].0, left[0].2), (GridId::MAIN, &node(1)));
    }

    #[test]
    fn a_cube_said_to_have_no_node_is_kept_as_empty_until_a_node_is_said_of_it_and_goes_with_its_ship()
     {
        let ship = GridId::new(GridId::FIRST_ALLOCATED + 3);
        let other = GridId::new(GridId::FIRST_ALLOCATED + 4);
        let mut held = CoarseOverrides::default();
        held.apply(&said(ship, cube(8, 3), None));
        held.apply(&said(ship, cube(0, 1), None));
        held.apply(&said(other, cube(0, 2), None));
        // (A level's own grid is not a ship: what it says is a node taken back.)
        held.apply(&said(GridId::MAIN, cube(0, 2), None));
        assert_eq!(held.len(), 0);
        assert_eq!(
            held.empties_of_ships_in_order(),
            [(ship, cube(0, 1)), (other, cube(0, 2)), (ship, cube(8, 3))]
        );
        assert_eq!(held.ships(), [ship, other]);
        // A node said of one of them makes it a node and no longer empty.
        held.apply(&said(ship, cube(0, 1), Some(node(1))));
        assert_eq!(
            held.empties_of_ships_in_order(),
            [(other, cube(0, 2)), (ship, cube(8, 3))]
        );
        assert_eq!(held.len(), 1);
        // And a node taken back is empty.
        held.apply(&said(ship, cube(0, 1), None));
        assert_eq!(held.len(), 0);
        assert_eq!(held.empties_of_ships_in_order().len(), 3);
        // A ship that is forgotten is forgotten whole.
        held.forget_grid(ship);
        assert_eq!(held.empties_of_ships_in_order(), [(other, cube(0, 2))]);
        assert_eq!(held.ships(), [other]);
    }

    #[test]
    fn what_is_told_to_a_view_made_later_is_the_nodes_of_the_grid_and_then_its_empty_cubes() {
        let ship = GridId::new(GridId::FIRST_ALLOCATED + 3);
        let other = GridId::new(GridId::FIRST_ALLOCATED + 4);
        let mut held = CoarseOverrides::default();
        held.apply(&said(ship, cube(8, 3), None));
        held.apply(&said(ship, cube(0, 2), Some(node(1))));
        held.apply(&said(other, cube(0, 2), Some(node(2))));
        held.apply(&said(ship, cube(0, 1), None));
        held.apply(&said(ship, cube(16, 1), Some(node(3))));
        held.apply(&said(other, cube(4, 1), None));
        let mut told: Vec<(BrickCube, Option<CoarseNode>)> = Vec::new();
        held.tell_of_a_grid(ship, |cube, node| told.push((cube, node)));
        assert_eq!(
            told,
            [
                (cube(16, 1), Some(node(3))),
                (cube(0, 2), Some(node(1))),
                (cube(0, 1), None),
                (cube(8, 3), None),
            ]
        );
    }

    #[test]
    fn what_is_held_comes_out_finest_first_and_the_same_every_time() {
        let mut held = CoarseOverrides::default();
        for (x, level) in [(32, 3), (0, 3), (16, 2), (0, 5)] {
            held.apply(&said(GridId::MAIN, cube(x, level), Some(node(1))));
        }
        let order: Vec<(u8, i128)> = held
            .in_order()
            .iter()
            .map(|(_, cube, _)| (cube.level(), cube.origin().x()))
            .collect();
        assert_eq!(order, vec![(2, 16), (3, 0), (3, 32), (5, 0)]);
    }
}
