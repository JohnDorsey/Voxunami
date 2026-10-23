//! CPU meshing of voxel bricks.
//!
//! A brick becomes a [`BrickMesh`]. Blocks that fill their whole cell become [`Quad`]s: one rectangle
//! per maximal run of identical, equally bright, exposed voxel faces (greedy meshing), packed into 8
//! bytes ([`PackedQuad`]); the GPU expands each into two triangles, so no vertex or index buffers are
//! needed. Blocks that fill only part of their cell (slabs, stairs) and see-through ones become
//! [`DetailedQuad`]s in eighths of a voxel, packed into 16 bytes; there are few of them, so they are not
//! merged.
//!
//! What a block is, and which state of it, comes from a [`block_models::BlockTable`]: a face is hidden
//! by what lies in front of it according to the shapes of both. Each quad carries a brightness, the
//! light of the voxel it looks out at, and quads of different brightness do not merge.
//!
//! Faces inside a cell that a neighbour closes off (the gap above a bottom slab with another slab on
//! top of it) are still made. Transparent blocks that blend (water) need their own pass, which is not
//! built.

mod coarse;
mod mesher;
mod quad;

pub use coarse::{coarse_node_of_brick, mesh_coarse_node};
pub use mesher::{BrickChannels, BrickNeighbourhood, mesh_brick};
pub use quad::{
    BrickMesh, DetailedQuad, FULL_LIGHT, MAXIMUM_MATERIAL, PackedBrickMesh, PackedDetailedQuad,
    PackedQuad, Quad, quad_axes,
};
