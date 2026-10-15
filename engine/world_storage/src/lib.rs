//! Saving and loading worlds.
//!
//! A world file is a [container](container) of checksummed sections. The engine defines a few
//! ([engine_sections]); plugins may add their own, which are opaque to everything here. The rule
//! the whole crate is built around: **loading and saving a world must never lose data, even when
//! plugins that wrote it are not installed.**
//!
//! - Content is named by identifier (`plugin:name`) in the file and numbered at runtime. Loading
//!   translates, giving unknown identifiers placeholder entries; saving translates back, so unknown
//!   content is written under its original name.
//! - Channels nobody defines, registry tables nobody supplies, and sections nobody owns are kept
//!   verbatim.
//! - Terrain the generator can reproduce is not stored; only bricks that differ from it are.

pub mod container;
pub mod engine_sections;
pub mod file;
#[cfg(feature = "test-support")]
pub mod test_support;
mod translation;
mod world;

pub use container::{ContainerError, Section};
pub use translation::{SourceTranslator, TranslationError};
pub use world::{
    BrickSource, LoadContext, LoadError, LoadReport, ParsedWorld, SaveError, WorldData,
};
