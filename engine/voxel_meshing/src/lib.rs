//! CPU meshing of voxel bricks.
//!
//! A brick becomes a list of [`Quad`]s: one rectangle per maximal run of identical, exposed
//! voxel faces (greedy meshing). A face is exposed when its voxel is opaque and the voxel it
//! looks at is not. Quads are packed into 8 bytes ([`PackedQuad`]); the GPU expands each into two
//! triangles, so no vertex or index buffers are needed.
//!
//! Only opaque cubes are meshed so far. Transparent voxels (leaves, water) need their own pass.

mod mesher;
mod quad;

pub use mesher::{BrickNeighbourhood, mesh_brick};
pub use quad::{PackedQuad, Quad};
