//! Sparse voxel storage.
//!
//! A [`Grid`] is a sparse map from [`BrickCoordinate`](voxel_space::BrickCoordinate) to [`Brick`].
//! A brick holds one [`ChannelStorage`] per *channel* of the grid's [`GridLayout`]; a channel is
//! one `u32` per voxel with a meaning chosen by whoever defines it (block type, temperature, ...).
//! Each channel compresses independently, so a mostly-uniform temperature channel costs almost
//! nothing even where the block-type channel is varied.
//!
//! Bricks are shared with `Arc` and copied on write. Cloning a grid is therefore a cheap
//! snapshot: background meshing, saving, replay keyframes and double-buffered simulation can read
//! a frozen state while the live grid keeps changing.
//!
//! An absent brick means "every channel at its default". This crate does not generate terrain;
//! layering saved edits over procedurally generated bricks is the job of a higher level.

mod brick;
mod channel_storage;
mod digest;
mod encoding;
mod grid;
mod layout;
mod packed_indices;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use brick::Brick;
pub use channel_storage::{ChannelStorage, StorageKind};
pub use encoding::{BrickDecodeError, StorageDecodeError};
pub use grid::{ChannelCountMismatch, Grid};
pub use layout::{
    ChannelDefinition, ChannelIndex, ChannelValueKind, GridLayout, GridLayoutError,
    MAXIMUM_CHANNEL_COUNT,
};
