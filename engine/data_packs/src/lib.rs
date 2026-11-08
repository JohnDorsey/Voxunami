//! Packs of JSON that add blocks to a game and patch the blocks it has, with no code: what a mod author can
//! do first. This crate reads the files (every problem is reported with its file and the path inside it), and
//! later loads packs in dependency order, applies their patches and generates the JSON Schema that editors
//! use. See `docs/specs/data-packs-spec.md`.

mod block;
mod body;
mod directory;
mod error;
mod face_colors;
mod family;
mod load;
mod manifest;
mod patch;
mod pictures;
mod schema;
mod shape;
mod tile;
mod walk;

pub use block::read_block;
pub use body::{
    Around, Body, BodyContext, BodyRail, BodySupport, GREATEST_EXTENT_BRICKS, GREATEST_MASS,
    OrbitData, POSITION_LIMIT, SMALLEST_ORBIT_RATIO, SpinData, Weight, read_body,
    read_family_parameters,
};
pub use directory::read_directory;
pub use error::{PackError, PackErrors};
pub use family::{FamilyData, FamilyParameters, FamilyProblem};
pub use load::{Loaded, RawFile, RawPack, RawTexture, load_packs};
pub use manifest::{PackManifest, read_manifest};
pub use patch::{Operation, Patch, PatchValue, Place, PropertyValue, read_patch};
pub use pictures::{
    GameTexture, MOST_TILE_SIZES, MOST_TILES_OF_A_SIZE, Pictures, Textures, resolve_pictures,
    settle_game_pictures,
};
pub use schema::{PackSchemas, pack_schemas};
pub use shape::{FACE_NAMES, SHAPE_NAMES};
pub use tile::{
    GREATEST_FILE_BYTES, GREATEST_TILE, SMALLEST_TILE, Tile, average_color, is_tile_size,
};
