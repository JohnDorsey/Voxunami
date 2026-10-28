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
mod great_river;
mod interval;
mod noise;
mod planet;
mod planet_family;
mod planet_node;
mod world_terrain;

pub use density::{DensityField, Heightfield, Minimum, ShellCaves};
pub use empty::{EmptyTerrain, empty};
pub use generator::{
    FamilyError, FieldTerrainGenerator, GeneratorFamily, RollingHillsSettings, SolidBall,
    TerrainGenerator, TerrainMaterials, rolling_hills,
};
pub use great_river::{
    GreatRivers, GreatRiversGenerator, RiverField, RiverKnot, RiverPath, great_rivers,
};
pub use interval::Interval;
pub use noise::{Dimensionality, FractalNoise, Octave, lattice_hash, lattice_value, mix};
pub use planet::{PlanetGenerator, PlanetGround, PlanetSettings, planet};
pub use planet_family::PlanetFamily;
pub use world_terrain::{GenerationOutcome, GeneratorId, WorldTerrain, WorldTerrainError};
