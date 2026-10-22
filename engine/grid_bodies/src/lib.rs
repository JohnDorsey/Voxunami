//! Ships as rigid bodies: [`GridBodiesPlugin`] moves the grids of a world with gravity, forces and collisions, using [`physics::BodyWorld`].
//!
//! The plugin keeps a body world for each level that has ships, in step with the server's world: a ship that appears gets a body, the
//! bricks of a ship that change have their colliders and mass made again, the terrain near ships has colliders, and each tick the ships
//! are put where the bodies went. What the blocks mean to a body (the collision of each block, the terrain of a level and the voxels of a ship
//! as sources of it) is in [`collision`], which a game also uses for the bodies of its players.

pub mod collision;
pub mod connectivity;
mod plugin;

pub use plugin::{BodiesReport, FRAME_RADIUS_BRICKS, GridBodiesConfig, GridBodiesPlugin};
