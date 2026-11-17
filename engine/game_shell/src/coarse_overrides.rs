//! The coarse nodes the server has told the session of, kept for as long as the session lasts (`docs/specs/far-nodes-spec.md`, 3.5 and F1).
//!
//! The server keeps a record of what it has sent to each client and does not send a node again while the client has it in its latest version. So the client must not lose what
//! it was sent: a view that is made again (the player goes to a level the game draws differently, and the view of the terrain is made for it) is given what is held here.

use std::collections::HashMap;

use coarse_cells::CoarseNode;
use coarse_streaming::CoarseNodeUpdate;
use region_octree::BrickCube;
use voxel_space::GridId;

/// What the server said a node is, by grid and cube, as of the last message of each.
#[derive(Debug, Default)]
pub(crate) struct CoarseOverrides {
    held: HashMap<(GridId, BrickCube), CoarseNode>,
}

impl CoarseOverrides {
    /// Takes in what the server said of a node: its new cells, or (with no node) that it is no longer different from what the generators make.
    pub(crate) fn apply(&mut self, update: &CoarseNodeUpdate) {
        match &update.node {
            Some(node) => {
                self.held.insert((update.grid, update.cube), node.clone());
            }
            None => {
                self.held.remove(&(update.grid, update.cube));
            }
        }
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
