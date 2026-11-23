//! The packs a server tells its clients of and sends to the ones that ask (`docs/specs/pack-archive-spec.md`, step 4).

use std::collections::VecDeque;
use std::sync::Arc;

use pack_archive::{Archive, ArchiveError, FileIndex, Hash, cache_file_name};
use protocol::{PackListing, maximum_pack_bytes};

/// A pack as the server serves it: what clients are told of it, and the canonical archive of the files they are sent.
///
/// The server does not read a pack's meaning; it knows its files. Which of them clients are sent is the caller's say (a file that is code and not wasm is
/// withheld, and a client sees it as a line of the index it cannot ask for).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServedPack {
    listing: PackListing,
    /// The archive of the files that are sent: what `PackChunk`s carry, from first byte to last.
    bytes: Arc<[u8]>,
}

impl ServedPack {
    /// The pack `id` made of `archive`, `sent` saying for each path whether clients are sent the file. Refused if the id is not one a cached file can be named by
    /// or the archive of the sent files is larger than a client holds.
    pub fn new(
        id: &str,
        archive: &Archive,
        sent: impl Fn(&str) -> bool,
    ) -> Result<ServedPack, ArchiveError> {
        let index = FileIndex::of(archive, &sent);
        cache_file_name(id, &index.root_hash())?;
        let bytes = archive.only(&sent).to_bytes();
        if bytes.len() as u64 > maximum_pack_bytes() {
            return Err(ArchiveError::TooLarge {
                max: maximum_pack_bytes(),
            });
        }
        Ok(ServedPack {
            listing: PackListing {
                id: id.to_owned(),
                index,
                archive_size: bytes.len() as u64,
            },
            bytes: bytes.into(),
        })
    }

    /// What clients are told of this pack.
    pub fn listing(&self) -> &PackListing {
        &self.listing
    }

    /// The pack's identity: the root hash of its index, withheld files included.
    pub fn root(&self) -> Hash {
        self.listing.root()
    }

    /// The canonical archive of the files clients are sent.
    pub fn archive_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// A pack being sent to one client: the archive (kept as it was when asked for, whatever is replaced meanwhile) and how much of it has gone.
#[derive(Debug)]
pub(crate) struct PackTransfer {
    root: Hash,
    bytes: Arc<[u8]>,
    sent: usize,
}

/// The packs one client has asked for and is being sent, in the order asked.
#[derive(Debug, Default)]
pub(crate) struct PackTransfers {
    queue: VecDeque<PackTransfer>,
}

impl PackTransfers {
    /// Starts sending `pack` unless it is being sent already.
    pub(crate) fn ask_for(&mut self, pack: &ServedPack) {
        let root = pack.root();
        if self.queue.iter().any(|transfer| transfer.root == root) {
            return;
        }
        self.queue.push_back(PackTransfer {
            root,
            bytes: Arc::clone(&pack.bytes),
            sent: 0,
        });
    }

    /// The `PackChunk` frames for this tick: whole pieces, in order, until `budget` bytes are used (at least one piece if anything is waiting, so that a small
    /// budget slows a pack down and never stops it).
    pub(crate) fn frames(&mut self, budget: usize) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        let mut used = 0;
        while used < budget {
            let Some(transfer) = self.queue.front_mut() else {
                break;
            };
            let end = (transfer.sent + protocol::MAXIMUM_PACK_CHUNK).min(transfer.bytes.len());
            let piece = &transfer.bytes[transfer.sent..end];
            used += piece.len();
            frames.push(
                protocol::ServerMessage::PackChunk {
                    root: transfer.root,
                    offset: transfer.sent as u64,
                    bytes: piece.to_vec(),
                }
                .encode(),
            );
            transfer.sent = end;
            if end == transfer.bytes.len() {
                self.queue.pop_front();
            }
        }
        frames
    }
}
