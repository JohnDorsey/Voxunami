//! What a viewer needs to turn a world into pictures and actions.
//!
//! [`WorldMesher`] keeps the meshes of the bricks around the viewer up to date on background
//! threads, reading from a [`world_storage::WorldData`] (a client's picture of the world); it holds
//! the nearby bricks, so it can also answer which voxels are solid. What it draws for each block
//! and state comes from a [`block_models::BlockTable`]. [`cast_ray`] finds the voxel
//! under a crosshair using that answer.

mod brick_states;
mod cube_states;
mod grid_mesher;
mod horizon;
mod job_feed;
mod job_queue;
mod lod;
mod may_show_memo;
mod mesher;
mod occupancy;
mod ray;
mod ship_terrain_mesher;
mod sun_boxes;
mod sun_ray;
mod warm_cut;

pub use brick_states::HeldBricks;
pub use cube_states::HeldPictures;
pub use grid_mesher::GridMesher;
pub use lod::{
    CoarseTerrainSource, LodConfig, LodSettings, LodView, MayShowOracle, Stalled, Viewpoint,
};
pub use mesher::{
    BrickLight, BulkLight, MeshUpdate, TerrainSource, ViewChannels, ViewSettings, WorldMesher,
};
pub use occupancy::{OccupancyChange, OccupancyStatistics};
pub use ray::{RayHit, cast_ray};
pub use ship_terrain_mesher::{ShipTerrainMesher, ShipTerrainSource};
pub use sun_boxes::{SunBox, sun_blocked_by_boxes};
pub use sun_ray::{SunCell, sun_blocked, sun_blocked_by_shapes, sun_blocked_from_point};
