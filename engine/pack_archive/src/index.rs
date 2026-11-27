//! A pack's identity: the hash of each file, the index of them, and the root hash of the index.

use std::fmt;

use crate::archive::{Archive, check_path};
use crate::error::ArchiveError;

/// A BLAKE3 hash, 32 bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hash([u8; 32]);

impl Hash {
    /// The hash of some bytes.
    pub fn of(bytes: &[u8]) -> Hash {
        Hash(*blake3::hash(bytes).as_bytes())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Hash {
        Hash(bytes)
    }

    /// 64 lower-case hexadecimal digits.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// The one spelling `to_hex` writes: 64 lower-case hexadecimal digits.
    pub fn from_hex(text: &str) -> Result<Hash, ArchiveError> {
        let bad = || ArchiveError::BadHash {
            text: text.to_owned(),
        };
        if text.len() != 64 {
            return Err(bad());
        }
        let mut bytes = [0u8; 32];
        for (index, pair) in text.as_bytes().chunks(2).enumerate() {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                _ => None,
            };
            bytes[index] = digit(pair[0]).ok_or_else(bad)? << 4 | digit(pair[1]).ok_or_else(bad)?;
        }
        Ok(Hash(bytes))
    }

    /// The first 32 hexadecimal digits: the key a cached pack's file name carries. It only finds the file; the whole hash is checked when the file is loaded.
    pub fn short_key(&self) -> String {
        self.to_hex()[..32].to_owned()
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Hash({})", self.to_hex())
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

/// One line of the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub path: String,
    pub size: u64,
    pub hash: Hash,
    /// Whether clients are sent this file. A withheld file (server code that is not wasm) is a line a client can see and cannot read.
    pub sent: bool,
}

/// Every file of a pack, sorted by path: the size, the hash and whether it is sent. Its hash, [`FileIndex::root_hash`], is the pack's identity: a change to
/// any file, sent or withheld, is a different root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIndex {
    entries: Vec<IndexEntry>,
}

const MAGIC: &[u8; 4] = b"VXPI";
/// 2: the hashes are BLAKE3 (they were SHA-256 in version 1).
const INDEX_VERSION: u8 = 2;
/// Path length (2) + one path byte + size (8) + hash (32) + sent (1).
const SMALLEST_ENTRY: usize = 2 + 1 + 8 + 32 + 1;

impl FileIndex {
    /// The index of an archive, `sent` saying for each path whether clients are sent it.
    pub fn of(archive: &Archive, sent: impl Fn(&str) -> bool) -> FileIndex {
        FileIndex {
            entries: archive
                .files()
                .map(|(path, bytes)| IndexEntry {
                    path: path.to_owned(),
                    size: bytes.len() as u64,
                    hash: Hash::of(bytes),
                    sent: sent(path),
                })
                .collect(),
        }
    }

    /// An index from its lines, which must be allowed paths, sorted, without a path twice.
    pub fn from_entries(entries: Vec<IndexEntry>) -> Result<FileIndex, ArchiveError> {
        for entry in &entries {
            check_path(&entry.path)?;
        }
        for pair in entries.windows(2) {
            if pair[0].path == pair[1].path {
                return Err(ArchiveError::Duplicate {
                    path: pair[1].path.clone(),
                });
            }
            if pair[0].path > pair[1].path {
                return Err(ArchiveError::NotSorted {
                    path: pair[1].path.clone(),
                });
            }
        }
        Ok(FileIndex { entries })
    }

    pub fn entries(&self) -> &[IndexEntry] {
        &self.entries
    }

    /// The bytes the root hash is made of: `VXPI`, the version 2, the count, then for each entry the path (length and bytes), the size, the hash and the sent
    /// flag, all numbers little-endian.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.push(INDEX_VERSION);
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        for entry in &self.entries {
            out.extend_from_slice(&(entry.path.len() as u16).to_le_bytes());
            out.extend_from_slice(entry.path.as_bytes());
            out.extend_from_slice(&entry.size.to_le_bytes());
            out.extend_from_slice(entry.hash.as_bytes());
            out.push(u8::from(entry.sent));
        }
        out
    }

    /// Reads what [`FileIndex::to_bytes`] wrote and refuses anything else: another magic or version, a flag that is not 0 or 1, a path not allowed, entries
    /// out of order, a count the bytes cannot hold, bytes left over.
    pub fn from_bytes(bytes: &[u8]) -> Result<FileIndex, ArchiveError> {
        let bad = |reason| ArchiveError::BadIndex { reason };
        let mut reader = Reader { bytes, at: 0 };
        if reader.take(4).ok_or(bad("too short"))? != MAGIC {
            return Err(bad("wrong magic"));
        }
        if reader.take(1).ok_or(bad("too short"))? != [INDEX_VERSION] {
            return Err(bad("an unknown version"));
        }
        let count = u32::from_le_bytes(
            reader
                .take(4)
                .ok_or(bad("too short"))?
                .try_into()
                .unwrap_or([0; 4]),
        ) as usize;
        if count > (bytes.len() - reader.at) / SMALLEST_ENTRY {
            return Err(bad("the count is more than the bytes can hold"));
        }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            let length = u16::from_le_bytes(
                reader
                    .take(2)
                    .ok_or(bad("too short"))?
                    .try_into()
                    .unwrap_or([0; 2]),
            );
            let path =
                std::str::from_utf8(reader.take(usize::from(length)).ok_or(bad("too short"))?)
                    .map_err(|_| bad("a path is not text"))?
                    .to_owned();
            let size = u64::from_le_bytes(
                reader
                    .take(8)
                    .ok_or(bad("too short"))?
                    .try_into()
                    .unwrap_or([0; 8]),
            );
            let hash: [u8; 32] = reader
                .take(32)
                .ok_or(bad("too short"))?
                .try_into()
                .unwrap_or([0; 32]);
            let sent = match reader.take(1).ok_or(bad("too short"))?[0] {
                0 => false,
                1 => true,
                _ => return Err(bad("a sent flag that is neither 0 nor 1")),
            };
            entries.push(IndexEntry {
                path,
                size,
                hash: Hash(hash),
                sent,
            });
        }
        if reader.at != bytes.len() {
            return Err(bad("bytes after the last entry"));
        }
        FileIndex::from_entries(entries)
    }

    /// The pack's identity: the hash of [`FileIndex::to_bytes`].
    pub fn root_hash(&self) -> Hash {
        Hash::of(&self.to_bytes())
    }

    /// Checks a **whole** archive (the server's own, holding withheld files too) against the index: every line has its file with that size and hash, and the
    /// archive has no file beyond the lines.
    pub fn verify(&self, archive: &Archive) -> Result<(), ArchiveError> {
        self.verify_with(archive, true)
    }

    /// Checks what a **client** was sent: every line marked sent has its file with that size and hash, no withheld file came along, and no file is unlisted.
    /// A line marked withheld is not asked for.
    pub fn verify_received(&self, archive: &Archive) -> Result<(), ArchiveError> {
        self.verify_with(archive, false)
    }

    fn verify_with(&self, archive: &Archive, whole: bool) -> Result<(), ArchiveError> {
        for entry in &self.entries {
            if !whole && !entry.sent {
                continue;
            }
            let Some(bytes) = archive.get(&entry.path) else {
                return Err(ArchiveError::MissingFile {
                    path: entry.path.clone(),
                });
            };
            if bytes.len() as u64 != entry.size || Hash::of(bytes) != entry.hash {
                return Err(ArchiveError::WrongFile {
                    path: entry.path.clone(),
                });
            }
        }
        for (path, _) in archive.files() {
            match self
                .entries
                .binary_search_by(|entry| entry.path.as_str().cmp(path))
            {
                Err(_) => {
                    return Err(ArchiveError::UnlistedFile {
                        path: path.to_owned(),
                    });
                }
                Ok(found) if !whole && !self.entries[found].sent => {
                    return Err(ArchiveError::WithheldFileSent {
                        path: path.to_owned(),
                    });
                }
                Ok(_) => {}
            }
        }
        Ok(())
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(length)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }
}

/// The name a pack has in a client's cache: `PACKID-KEY.zip`, the pack's own id (from its `pack.json`) and the first 32 digits of its root hash. The id must
/// be a safe file name (1 to 64 characters of `a-z`, `0-9` and `_`).
pub fn cache_file_name(pack_id: &str, root: &Hash) -> Result<String, ArchiveError> {
    let allowed = !pack_id.is_empty()
        && pack_id.len() <= 64
        && pack_id
            .bytes()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_'));
    if !allowed {
        return Err(ArchiveError::BadPackId {
            id: pack_id.to_owned(),
        });
    }
    Ok(format!("{pack_id}-{}.zip", root.short_key()))
}
