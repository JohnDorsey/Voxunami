//! Why an archive, an index or a name was refused. Each way a zip can differ from the canonical form has its own variant, so a person told "refused" is told why.

use thiserror::Error;

/// A reason to refuse. Paths are named as the archive spells them.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ArchiveError {
    /// A path that is not one of the allowed form: 1 to 255 characters of `a-z`, `0-9`, `_`, `.` and `/`, in parts that are not empty, `.` or `..`.
    #[error("the path {path:?} is not allowed: {reason}")]
    InvalidPath { path: String, reason: &'static str },
    /// Two entries with one path.
    #[error("the path {path:?} is in the archive twice")]
    Duplicate { path: String },
    /// Entries not in the order of their paths' bytes.
    #[error(
        "the path {path:?} is out of order: entries must be sorted by the bytes of their paths"
    )]
    NotSorted { path: String },
    /// A path that is a file and also the folder of another path.
    #[error("the path {path:?} is a file and also a folder of another path")]
    FileAndFolder { path: String },
    /// The bytes are not laid out as a canonical archive: what was found and where.
    #[error("not an archive in the canonical form: {reason}")]
    Malformed { reason: &'static str },
    /// An entry that is compressed (only stored entries are allowed).
    #[error("{path:?} is compressed; only stored entries are allowed")]
    Compressed { path: String },
    /// An entry with flag bits set (a data descriptor, encryption or the UTF-8 name flag).
    #[error("{path:?} has flag bits set; the canonical form has none")]
    Flags { path: String },
    /// An entry with an extra field.
    #[error("{path:?} has an extra field; the canonical form has none")]
    ExtraField { path: String },
    /// An entry with a comment.
    #[error("{path:?} has a comment; the canonical form has none")]
    Comment { path: String },
    /// A size or count that needs zip64.
    #[error("the archive uses zip64, which the canonical form does not")]
    Zip64,
    /// An entry whose bytes do not match its checksum.
    #[error("{path:?} does not match its checksum")]
    Checksum { path: String },
    /// Every field was understood, yet the bytes are not those the writer would write (the date, the attributes, the order of the pieces): the first byte that differs.
    #[error(
        "the archive is not canonical: the first difference from the canonical bytes is at byte {offset}"
    )]
    NotCanonical { offset: usize },
    /// More entries than allowed.
    #[error("{count} files are more than the {max} allowed")]
    TooManyFiles { count: usize, max: usize },
    /// A file larger than allowed.
    #[error("{path:?} is {size} bytes, more than the {max} allowed for one file")]
    FileTooLarge { path: String, size: u64, max: u64 },
    /// All the files together larger than allowed.
    #[error("the files add up to more than the {max} bytes allowed")]
    TooLarge { max: u64 },
    /// A hash that is not 64 lower-case hexadecimal digits.
    #[error("{text:?} is not a hash: it must be 64 lower-case hexadecimal digits")]
    BadHash { text: String },
    /// A file index that cannot be read.
    #[error("not a file index: {reason}")]
    BadIndex { reason: &'static str },
    /// A pack id that cannot be a file name.
    #[error("{id:?} is not a pack id: 1 to 64 characters of a-z, 0-9 and _")]
    BadPackId { id: String },
    /// The index lists a file the archive does not have.
    #[error("the file {path:?} is in the index and missing from the archive")]
    MissingFile { path: String },
    /// The archive has a file the index does not list.
    #[error("the file {path:?} is in the archive and not in the index")]
    UnlistedFile { path: String },
    /// A file whose size or hash is not the index's.
    #[error("the file {path:?} is not the one the index lists (its size or hash differs)")]
    WrongFile { path: String },
    /// A client's archive holding a file that the index says is withheld.
    #[error("the file {path:?} is withheld from clients and was sent anyway")]
    WithheldFileSent { path: String },
}
