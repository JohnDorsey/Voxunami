//! Coarse cells: what far away voxels are drawn as (`docs/specs/lod-spec.md`).
//!
//! A [`CoarseCell`] stands for a cube of `2^n` voxels along each edge (detail level `n`, [`voxel_space::LevelOfDetail`]): its dominant block, how much of it
//! is filled, how bright it is, and how much of its surface touches air. Eight cells make the cell one level up ([`combine`]); a [`CoarseNode`] is 32 x 32
//! x 32 cells, the coarse counterpart of a brick, and eight nodes make the node one level up.
//!
//! Which nodes of a grid to draw for a viewer, finer near and coarser far, is [`select_nodes`].
//!
//! Cells are made from stored voxels by `voxel_meshing` (which knows what a face is) and from a density field by `terrain` (which knows what the
//! ground is). They are for drawing only: nothing in the simulation reads them, and they are never saved.

mod cell;
mod edited;
mod fine_region;
mod node;
mod selection;
mod swap;

pub use cell::{BRIGHTEST_LIGHT_UP_TO_LEVEL, CoarseCell, DRAWN_FILL, FULL_LIGHT, combine};
pub use edited::EditedCubes;
pub use fine_region::FineRegion;
pub use node::{CELLS_PER_NODE, CoarseNode, NODE_EDGE};
pub use selection::{
    DetailSettings, MAXIMUM_DETAIL_LEVEL, Selection, ViewSelection, Zoom, brick_centre,
    select_around_brick, select_nodes,
};
pub use swap::{Swap, plan_swaps};
