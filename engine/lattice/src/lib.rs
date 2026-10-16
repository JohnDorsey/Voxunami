//! What rules that act on voxels tick by tick have in common (light, fire; later fluids and heat).
//!
//! Such a rule follows the tick rules of the design notes: every voxel it looks at in a tick is
//! worked out from the world as it stood at the start of the tick, the results are written
//! afterwards, and the voxels that changed (and their neighbours) are looked at next. That makes
//! the order, and the way the world is divided into bricks, irrelevant to the result. This crate has
//! the two pieces every such rule needs and nothing of any rule itself:
//!
//! - [`ActiveVoxels`]: the voxels a rule still has to look at, in a fixed order, that can be saved
//!   with the world and restored, so that a world saved in the middle of something carries on;
//! - [`voxel_random`]: a random number for a voxel at a tick, from the world's seed, the same on
//!   every machine, so that chance can be part of a rule without making it depend on anything but
//!   the world and the tick.

mod active;
mod random;

pub use active::ActiveVoxels;
pub use random::voxel_random;
