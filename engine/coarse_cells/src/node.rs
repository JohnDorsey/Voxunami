use voxel_space::{LevelOfDetail, LocalVoxelIndex};

use crate::cell::{CoarseCell, combine};

/// Cells along an edge of a node: the same as the voxels along an edge of a brick.
pub const NODE_EDGE: usize = 32;

/// Cells in a node.
pub const CELLS_PER_NODE: usize = NODE_EDGE * NODE_EDGE * NODE_EDGE;

/// 32 x 32 x 32 cells of one detail level: the coarse counterpart of a brick. At detail level 0 a node is a brick's voxels; at level `n` each cell stands
/// for `2^n` voxels along an edge, and the node for `32 * 2^n`. The cells are indexed as the voxels of a brick are ([`LocalVoxelIndex`]).
///
/// A node whose cells are all the same is kept as that one cell, which is nearly all of them: the sky and the ground under the ground.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoarseNode {
    Uniform(CoarseCell),
    Cells(Box<[CoarseCell]>),
}

impl CoarseNode {
    /// A node of cells all the same.
    pub fn uniform(cell: CoarseCell) -> Self {
        Self::Uniform(cell)
    }

    /// A node of these cells, in the order of [`LocalVoxelIndex`]; kept as one cell if they are all the same. Panics unless there are
    /// [`CELLS_PER_NODE`] of them.
    pub fn from_cells(cells: Vec<CoarseCell>) -> Self {
        assert_eq!(
            cells.len(),
            CELLS_PER_NODE,
            "a node has {CELLS_PER_NODE} cells"
        );
        let first = cells[0];
        if cells.iter().all(|cell| *cell == first) {
            Self::Uniform(first)
        } else {
            Self::Cells(cells.into_boxed_slice())
        }
    }

    pub fn cell(&self, at: LocalVoxelIndex) -> CoarseCell {
        match self {
            Self::Uniform(cell) => *cell,
            Self::Cells(cells) => cells[at.index()],
        }
    }

    /// The one cell every cell is, if they are all the same.
    pub fn uniform_cell(&self) -> Option<CoarseCell> {
        match self {
            Self::Uniform(cell) => Some(*cell),
            Self::Cells(_) => None,
        }
    }

    /// Every cell, in the order of [`LocalVoxelIndex`].
    pub fn cells(&self) -> impl ExactSizeIterator<Item = CoarseCell> + '_ {
        (0..CELLS_PER_NODE).map(|index| match self {
            Self::Uniform(cell) => *cell,
            Self::Cells(cells) => cells[index],
        })
    }

    /// The node one detail level up that covers eight nodes: `children` are the nodes of the eight octants, in the order `x + 2 y + 4 z` (so the
    /// parent's cell at `(x, y, z)` is made from the eight cells at `(2x + dx, 2y + dy, 2z + dz) mod 32` of the child in octant `(x / 16, y / 16, z / 16)`).
    /// `parent_level` is the detail level of the result.
    pub fn from_children(children: [&Self; 8], parent_level: LevelOfDetail, air: u32) -> Self {
        // A child that is all one cell makes the same parent cell all over its half.
        let uniform_parent: [Option<CoarseCell>; 8] = children.map(|child| {
            child
                .uniform_cell()
                .map(|cell| combine(&[cell; 8], parent_level, air))
        });
        let mut cells = Vec::with_capacity(CELLS_PER_NODE);
        for z in 0..NODE_EDGE {
            for y in 0..NODE_EDGE {
                for x in 0..NODE_EDGE {
                    let octant = (x >> 4) + 2 * (y >> 4) + 4 * (z >> 4);
                    if let Some(cell) = uniform_parent[octant] {
                        cells.push(cell);
                        continue;
                    }
                    let child = children[octant];
                    let base = [
                        (2 * x) % NODE_EDGE,
                        (2 * y) % NODE_EDGE,
                        (2 * z) % NODE_EDGE,
                    ];
                    let eight: [CoarseCell; 8] = std::array::from_fn(|which| {
                        let (dx, dy, dz) = (which & 1, (which >> 1) & 1, which >> 2);
                        child.cell(LocalVoxelIndex::new(
                            (base[0] + dx) as u8,
                            (base[1] + dy) as u8,
                            (base[2] + dz) as u8,
                        ))
                    });
                    cells.push(combine(&eight, parent_level, air));
                }
            }
        }
        Self::from_cells(cells)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AIR: u32 = 0;
    const STONE: u32 = 1;
    const GRASS: u32 = 2;

    fn level(level: u8) -> LevelOfDetail {
        LevelOfDetail::new(level).unwrap()
    }

    fn solid(block: u32, exposure: u32) -> CoarseCell {
        CoarseCell {
            block,
            fill: 255,
            light: 7,
            exposure,
        }
    }

    /// A node whose cell at `(x, y, z)` is `of(x, y, z)`.
    fn node_of(of: impl Fn(usize, usize, usize) -> CoarseCell) -> CoarseNode {
        let mut cells = Vec::with_capacity(CELLS_PER_NODE);
        for z in 0..NODE_EDGE {
            for y in 0..NODE_EDGE {
                for x in 0..NODE_EDGE {
                    cells.push(of(x, y, z));
                }
            }
        }
        CoarseNode::from_cells(cells)
    }

    #[test]
    fn cells_that_are_all_the_same_are_kept_as_one_and_read_back_as_all_of_them() {
        let cell = solid(STONE, 0);
        let node = node_of(|_, _, _| cell);
        assert_eq!(node, CoarseNode::Uniform(cell));
        assert_eq!(node.uniform_cell(), Some(cell));
        assert!(node.cells().all(|found| found == cell));
        assert_eq!(node.cells().len(), CELLS_PER_NODE);
        assert_eq!(node.cell(LocalVoxelIndex::new(31, 0, 17)), cell);
    }

    #[test]
    fn cells_are_indexed_as_the_voxels_of_a_brick_are() {
        let node = node_of(|x, y, z| solid(1 + (x + 32 * y + 1024 * z) as u32, 0));
        assert_eq!(node.uniform_cell(), None);
        for (index, cell) in node.cells().enumerate() {
            assert_eq!(cell.block, 1 + index as u32);
            assert_eq!(node.cell(LocalVoxelIndex::from_index(index).unwrap()), cell);
        }
    }

    #[test]
    fn a_node_of_the_wrong_size_is_refused() {
        let result = std::panic::catch_unwind(|| CoarseNode::from_cells(vec![solid(1, 0); 5]));
        assert!(result.is_err());
    }

    #[test]
    fn a_parent_cell_is_made_of_the_eight_child_cells_it_covers_and_in_the_octant_it_lies_in() {
        // Each child cell is told apart by its block, made of where it is in its node and which node: nothing repeats among the 8 x 32768.
        let children: Vec<CoarseNode> = (0..8)
            .map(|octant| {
                node_of(|x, y, z| {
                    solid(
                        1 + (x + 32 * y + 1024 * z) as u32 + 40_000 * octant as u32,
                        0,
                    )
                })
            })
            .collect();
        let refs: [&CoarseNode; 8] = std::array::from_fn(|index| &children[index]);
        let parent = CoarseNode::from_children(refs, level(1), AIR);
        for &(x, y, z) in &[
            (0usize, 0usize, 0usize),
            (15, 15, 15),
            (16, 0, 0),
            (0, 16, 0),
            (0, 0, 16),
            (31, 31, 31),
            (17, 3, 29),
        ] {
            let octant = (x >> 4) + 2 * (y >> 4) + 4 * (z >> 4);
            let made = parent.cell(LocalVoxelIndex::new(x as u8, y as u8, z as u8));
            // The eight cells are all different blocks with no surface and equal fill, so the lowest octant's block wins: the cell at (2x, 2y, 2z) of the
            // child (mod 32).
            let (cx, cy, cz) = ((2 * x) % 32, (2 * y) % 32, (2 * z) % 32);
            assert_eq!(
                made.block,
                1 + (cx + 32 * cy + 1024 * cz) as u32 + 40_000 * octant as u32,
                "({x}, {y}, {z})"
            );
            assert_eq!(made.fill, 255);
        }
    }

    #[test]
    fn eight_uniform_nodes_make_a_parent_that_is_uniform_only_if_they_make_the_same_cell() {
        let stone = CoarseNode::uniform(solid(STONE, 0));
        let sky = CoarseNode::uniform(CoarseCell::empty(AIR, 255));
        let all_stone = CoarseNode::from_children([&stone; 8], level(4), AIR);
        assert_eq!(all_stone, CoarseNode::Uniform(solid(STONE, 0)).clone());
        let all_sky = CoarseNode::from_children([&sky; 8], level(4), AIR);
        assert_eq!(all_sky, CoarseNode::Uniform(CoarseCell::empty(AIR, 255)));
        // Ground below and sky above (octants with y = 0 are 0, 1, 4, 5): the parent has the two halves, each cell as combine makes it.
        let mixed = CoarseNode::from_children(
            [&stone, &stone, &sky, &sky, &stone, &stone, &sky, &sky],
            level(4),
            AIR,
        );
        assert_eq!(mixed.cell(LocalVoxelIndex::new(3, 3, 3)), solid(STONE, 0));
        assert_eq!(
            mixed.cell(LocalVoxelIndex::new(3, 20, 3)),
            CoarseCell::empty(AIR, 255)
        );
        assert_eq!(mixed.uniform_cell(), None);
    }

    #[test]
    fn a_floor_of_cells_with_a_face_each_is_a_floor_with_a_face_each_in_the_parent() {
        // The top layer of the cells (layer 15) has one face of surface, the layers under it none, the layers over it are sky.
        let low = node_of(|_, y, _| {
            if y > 15 {
                CoarseCell::empty(AIR, 7)
            } else if y == 15 {
                solid(GRASS, CoarseCell::EXPOSURE_ONE)
            } else {
                solid(STONE, 0)
            }
        });
        let sky = CoarseNode::uniform(CoarseCell::empty(AIR, 7));
        // The floor is in the lower half (octants with y = 0) of the parent, at its cell layer 7 (the child layers 14 and 15).
        let parent = CoarseNode::from_children(
            [&low, &low, &sky, &sky, &low, &low, &sky, &sky],
            level(1),
            AIR,
        );
        for y in 0..8 {
            let cell = parent.cell(LocalVoxelIndex::new(4, y, 4));
            // (Four cells with a face each over four with none, divided by four: one face.)
            let wanted = if y == 7 { CoarseCell::EXPOSURE_ONE } else { 0 };
            assert_eq!(cell.exposure, wanted, "layer {y}");
        }
        assert_eq!(parent.cell(LocalVoxelIndex::new(4, 7, 4)).block, GRASS);
    }
}
