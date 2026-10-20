//! Packs of JSON that add blocks to a game and patch the blocks it has, with no code: what a mod author can
//! do first. This crate reads the files (every problem is reported with its file and the path inside it), and
//! later loads packs in dependency order, applies their patches and generates the JSON Schema that editors
//! use. See `docs/data-packs-spec.md`.

mod block;
mod directory;
mod error;
mod load;
mod manifest;
mod patch;
mod schema;
mod shape;
mod walk;

pub use block::read_block;
pub use directory::read_directory;
pub use error::{PackError, PackErrors};
pub use load::{Loaded, RawFile, RawPack, load_packs};
pub use manifest::{PackManifest, read_manifest};
pub use patch::{Operation, Patch, PatchValue, Place, PropertyValue, read_patch};
pub use schema::{PackSchemas, pack_schemas};
pub use shape::{FACE_NAMES, SHAPE_NAMES};
