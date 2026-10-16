//! What a viewer needs to turn a world into pictures and actions.
//!
//! [`WorldMesher`] keeps the meshes of the bricks around the viewer up to date on background
//! threads, reading from a [`world_storage::WorldData`] (a client's picture of the world); it holds
//! the nearby bricks, so it can also answer which voxels are solid. What it draws for each block
//! and state comes from a [`block_models::BlockTable`]. [`cast_ray`] finds the voxel
//! under a crosshair using that answer.

mod mesher;
mod ray;

pub use mesher::{MeshUpdate, ViewChannels, ViewSettings, WorldMesher};
pub use ray::{RayHit, cast_ray};
