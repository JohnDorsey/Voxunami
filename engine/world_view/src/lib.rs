//! What a viewer needs to turn a world into pictures and actions.
//!
//! [`WorldMesher`] keeps the meshes of the bricks around the viewer up to date on background
//! threads, reading from a [`world_storage::WorldData`] (a client's picture of the world); it holds
//! the nearby bricks, so it can also answer which voxels are solid. What it draws for each block
//! and state comes from a [`block_models::BlockTable`]. [`cast_ray`] finds the voxel
//! under a crosshair using that answer.

mod grid_mesher;
mod lod;
mod mesher;
mod ray;
mod ship_terrain_mesher;

pub use grid_mesher::GridMesher;
pub use lod::{LodConfig, LodSettings, LodView};
pub use mesher::{
    BrickLight, BulkLight, MeshUpdate, TerrainSource, ViewChannels, ViewSettings, WorldMesher,
};
pub use ray::{RayHit, cast_ray};
pub use ship_terrain_mesher::{ShipTerrainMesher, ShipTerrainSource};
