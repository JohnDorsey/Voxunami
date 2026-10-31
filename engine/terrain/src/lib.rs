//! Deterministic procedural terrain.
//!
//! Terrain is described by a [`DensityField`]: a function of position whose sign says solid
//! (positive) or air (zero or negative). A field can be asked three things:
//! - its value at a voxel ([`DensityField::sample`]);
//! - conservative bounds over a whole box ([`DensityField::bounds`]), so generators can classify
//!   a region as all air or all solid without sampling it;
//! - both of the above at a coarser [`LevelOfDetail`](voxel_space::LevelOfDetail), which ignores
//!   features smaller than the level's footprint. Distant terrain is therefore the *same*
//!   function evaluated more coarsely, not a separate approximation.
//!
//! Everything here must give bit-identical results on every platform, because clients generate
//! terrain locally and must agree with the server: noise uses integer hashing and only IEEE
//! add/subtract/multiply/divide, never the platform's transcendental functions.
//!
//! A [`TerrainGenerator`] turns a field into bricks. [`WorldTerrain`] chooses which registered
//! generator version produced each region of a world.

mod coarse;
mod density;
mod empty;
mod generator;
mod interval;
mod noise;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
mod world_terrain;

pub use coarse::{coarse_node_from_fills, coarse_node_of_field, cube_may_show};
#[cfg(any(test, feature = "test-support"))]
pub use coarse::{coarse_node_of_field_cell_by_cell, fills_of_field_cell_by_cell, node_digest};
pub use density::{DensityField, Heightfield, Minimum, ShellCaves};
pub use empty::{EmptyTerrain, empty};
pub use generator::{
    FamilyError, FieldTerrainGenerator, GeneratorFamily, SURFACE_LAYER_DEPTH, SolidBall,
    SurfaceLayers, TerrainGenerator,
};
pub use interval::Interval;
pub use noise::{
    Dimensionality, FractalNoise, MAXIMUM_WAVELENGTH_LOG2, Octave, lattice_hash, lattice_value, mix,
};
pub use world_terrain::{GenerationOutcome, GeneratorId, WorldTerrain, WorldTerrainError};
