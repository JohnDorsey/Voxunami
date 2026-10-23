//! Coarse cells: what far away voxels are drawn as (`docs/lod-spec.md`).
//!
//! A [`CoarseCell`] stands for a cube of `2^n` voxels along each edge (detail level `n`, [`voxel_space::LevelOfDetail`]): its dominant block, how much of it
//! is filled, how bright it is, and how much of its surface touches air. Eight cells make the cell one level up ([`combine`]); a [`CoarseNode`] is 32 x 32
//! x 32 cells, the coarse counterpart of a brick, and eight nodes make the node one level up.
//!
//! Cells are made from stored voxels by `voxel_meshing` (which knows what a face is) and from a density field by `terrain` (which knows what the
//! ground is). They are for drawing only: nothing in the simulation reads them, and they are never saved.

mod cell;
mod node;

pub use cell::{BRIGHTEST_LIGHT_UP_TO_LEVEL, CoarseCell, DRAWN_FILL, FULL_LIGHT, combine};
pub use node::{CELLS_PER_NODE, CoarseNode, NODE_EDGE};
