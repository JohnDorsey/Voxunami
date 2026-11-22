//! The canonical archive of a pack, and a pack's identity (`docs/specs/pack-archive-spec.md`).
//!
//! One set of files is one byte sequence: [`Archive`] writes a zip in a single form and reads only that form, so the server and a client hold the same
//! bytes, and an author's own zip is rewritten into it rather than trusted. [`FileIndex`] lists every file with its size, its hash and whether clients are
//! sent it, and its [`root_hash`](FileIndex::root_hash) is the pack's identity: a change to any file, even one a client is not sent, is a different root.
//! The crate knows nothing of what a pack means; it reads and writes bytes and has no dependency on the engine.

mod archive;
mod error;
mod index;

pub use archive::{Archive, Limits, check_path};
pub use error::ArchiveError;
pub use index::{FileIndex, Hash, IndexEntry, cache_file_name};
