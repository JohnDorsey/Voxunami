//! Bodies that move through voxel terrain, on Rapier.
//!
//! Nothing else in the engine imports Rapier: this crate hides it behind types of the engine's own
//! ([`WorldPosition`](voxel_space::WorldPosition), [`MoveInput`]). It follows the architecture of
//! `docs/history/physics-spike.md`:
//!
//! - Rapier works in `f64` (the `rapier3d-f64` package), and so does this crate's interface (speeds, forces, settings, the offset in a position: nothing is rounded on
//!   the way in or out; `real.rs` makes Rapier's vectors), and each body still lives in a *local frame* (a brick of the world as origin) near it, which is rebuilt when
//!   the body has gone some way from its origin, so that the numbers stay small and a place maps to a brick and an offset in it;
//! - terrain colliders are made lazily, from bricks, out of what a [`CollisionSource`] says each voxel
//!   is: whole cubes as a voxel collider of just the voxels that can be touched, and blocks that
//!   fill part of their cell as boxes;
//! - the only things stepped are the bodies we move ourselves (here a character with Rapier's
//!   kinematic controller), single-threaded, in a fixed order, with `enhanced-determinism`, so the
//!   same inputs give the same positions bit for bit on every machine.
//!
//! A character is moved by [`CharacterBody::step`] with the player's [`MoveInput`]: it walks, jumps,
//! falls and slides along walls, or flies.

mod bodies;
mod character;
mod mass;
mod motion;
mod real;
mod sweep;
mod terrain;

pub use bodies::{BodyError, BodyId, BodyKind, BodySettings, BodyState, BodyWorld};
pub use character::{CharacterBody, CharacterSettings, ForeignGrid, MoveInput};
pub use mass::{BodyMass, MassMoments};
pub use motion::BodyMotion;
pub use sweep::{SweptBricks, TERRAIN_EVERY_WAY_VOXELS};
pub use terrain::{BrickHint, Collision, CollisionSource};
