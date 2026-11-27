//! The packs a server replicates, as a client takes them in (`docs/specs/pack-archive-spec.md`, step 4).
//!
//! The server tells the client its list of packs; the client keeps in memory every one it has verified, takes what it can from a [`PackStore`] (a folder of
//! cached archives, for a game) and asks the server for the rest, which it is sent in pieces. A pack is held only when its archive is the canonical one, holds
//! exactly the files the index says were sent, and each is as the index lists it. The pack's identity is the root hash of its index, so a pack that is held *is*
//! the pack the server listed. The client does nothing else with them yet: the block table is still the server's resolved package (step 5 loads the packs
//! and cross-checks).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::io;

use pack_archive::{Archive, FileIndex, Hash, Limits};
use protocol::{PackListing, pack_chunks};

use crate::error::ClientError;

/// Where a client keeps the archives of packs between runs. A game's is a folder (`ID-HASH.zip` in a world's folder); a replay has none.
pub trait PackStore: Debug + Send {
    /// The bytes kept for this pack, if any. They are not trusted: the client checks them against the index it was told before using them.
    fn load(&mut self, pack_id: &str, root: &Hash) -> Option<Vec<u8>>;

    /// Keeps the canonical archive of a pack the client has verified.
    fn save(&mut self, pack_id: &str, root: &Hash, archive: &[u8]) -> io::Result<()>;
}

/// A store that has nothing and keeps nothing: what a replay and a test use.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoPackStore;

impl PackStore for NoPackStore {
    fn load(&mut self, _pack_id: &str, _root: &Hash) -> Option<Vec<u8>> {
        None
    }

    fn save(&mut self, _pack_id: &str, _root: &Hash, _archive: &[u8]) -> io::Result<()> {
        Ok(())
    }
}

/// The files of a [`MemoryPackStore`], by pack id and root hash.
type StoredFiles = BTreeMap<(String, Hash), Vec<u8>>;

/// A store in memory, whose copies all share the same files: for a player with nowhere to keep packs, and for tests that look at what a client kept.
#[derive(Clone, Debug, Default)]
pub struct MemoryPackStore {
    files: std::sync::Arc<std::sync::Mutex<StoredFiles>>,
}

impl MemoryPackStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Puts a file in the store as if a client had kept it (whatever it holds: a client checks it before it trusts it).
    pub fn put(&self, pack_id: &str, root: Hash, archive: Vec<u8>) {
        self.files
            .lock()
            .expect("no thread panics holding the store")
            .insert((pack_id.to_owned(), root), archive);
    }

    /// The bytes kept for a pack, if any.
    pub fn get(&self, pack_id: &str, root: &Hash) -> Option<Vec<u8>> {
        self.files
            .lock()
            .expect("no thread panics holding the store")
            .get(&(pack_id.to_owned(), *root))
            .cloned()
    }
}

impl PackStore for MemoryPackStore {
    fn load(&mut self, pack_id: &str, root: &Hash) -> Option<Vec<u8>> {
        self.get(pack_id, root)
    }

    fn save(&mut self, pack_id: &str, root: &Hash, archive: &[u8]) -> io::Result<()> {
        self.put(pack_id, *root, archive.to_vec());
        Ok(())
    }
}

/// A pack the client holds, verified: the archive of the files it was sent, and the index it was checked against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldPack {
    pub id: String,
    pub index: FileIndex,
    pub archive: Archive,
}

impl HeldPack {
    /// The pack's identity.
    pub fn root(&self) -> Hash {
        self.index.root_hash()
    }
}

/// A pack being assembled from the pieces the server sends.
#[derive(Debug)]
struct Incoming {
    bytes: Vec<u8>,
}

/// What a new list of packs asks the client to do.
#[derive(Debug, Default)]
pub(crate) struct ListOutcome {
    /// The packs the client lacks and must ask the server for.
    pub(crate) to_request: Vec<Hash>,
    /// The frames of the packs it took from its store, as the server would have sent them, for a recording to hold.
    pub(crate) from_store: Vec<Vec<u8>>,
}

/// The packs of a server as a client knows them: the list it was told, the ones it holds, and the ones being sent.
#[derive(Debug)]
pub struct ClientPacks {
    store: Box<dyn PackStore>,
    listed: Vec<PackListing>,
    held: BTreeMap<Hash, HeldPack>,
    incoming: BTreeMap<Hash, Incoming>,
    /// The roots asked for and not yet complete.
    requested: BTreeSet<Hash>,
    store_problems: Vec<String>,
    /// Whether the list is every pack the server runs besides the game's own (a server that has said nothing runs only the game's own: a list of none that is whole).
    whole: bool,
}

impl ClientPacks {
    pub(crate) fn new(store: Box<dyn PackStore>) -> Self {
        Self {
            store,
            listed: Vec::new(),
            held: BTreeMap::new(),
            incoming: BTreeMap::new(),
            requested: BTreeSet::new(),
            store_problems: Vec::new(),
            whole: true,
        }
    }

    /// The server's list of packs, in load order, as it was last told.
    pub fn listed(&self) -> &[PackListing] {
        &self.listed
    }

    /// Whether the list is every pack the server runs besides the game's own. Only then do the packs this client holds say all that is behind the package the server
    /// sent, so only then can it load them and check that they make it.
    pub fn whole(&self) -> bool {
        self.whole
    }

    /// The packs held, in the order of the list.
    pub fn held(&self) -> impl Iterator<Item = &HeldPack> {
        self.listed
            .iter()
            .filter_map(|listing| self.held.get(&listing.root()))
    }

    /// The pack with this root hash, if it is held.
    pub fn get(&self, root: &Hash) -> Option<&HeldPack> {
        self.held.get(root)
    }

    /// Whether every pack of the list is held.
    pub fn is_complete(&self) -> bool {
        self.listed
            .iter()
            .all(|listing| self.held.contains_key(&listing.root()))
    }

    /// What went wrong keeping a pack in the store, one line each (a store that cannot save does not stop the client holding the pack).
    pub fn store_problems(&self) -> &[String] {
        &self.store_problems
    }

    /// The server's list replaces the client's. Packs still listed are kept; the others are let go, with what was arriving of them. A pack the client lacks is
    /// taken from its store if the store has the very archive (checked against the index, never trusted), and otherwise asked for.
    pub(crate) fn list(&mut self, listings: Vec<PackListing>, whole: bool) -> ListOutcome {
        self.whole = whole;
        let roots: BTreeSet<Hash> = listings.iter().map(PackListing::root).collect();
        self.held.retain(|root, _| roots.contains(root));
        self.incoming.retain(|root, _| roots.contains(root));
        self.requested.retain(|root| roots.contains(root));
        let mut outcome = ListOutcome::default();
        for listing in &listings {
            let root = listing.root();
            if self.held.contains_key(&root) || self.requested.contains(&root) {
                continue;
            }
            if let Some(bytes) = self.store.load(&listing.id, &root)
                && let Ok(archive) = checked(listing, &bytes)
            {
                // (The recording gets the pieces the server would have sent; the bytes are the canonical archive, so they are the ones the store had.)
                outcome
                    .from_store
                    .extend(pack_chunks(&bytes).map(|(offset, piece)| {
                        protocol::ServerMessage::PackChunk {
                            root,
                            offset,
                            bytes: piece.to_vec(),
                        }
                        .encode()
                    }));
                self.held.insert(
                    root,
                    HeldPack {
                        id: listing.id.clone(),
                        index: listing.index.clone(),
                        archive,
                    },
                );
                continue;
            }
            self.requested.insert(root);
            outcome.to_request.push(root);
        }
        self.listed = listings;
        outcome
    }

    /// A piece of a pack. A pack that is not in the list, or not being asked for, is ignored (a replaced list may have taken it out while it was on its way). A piece
    /// that is not the next, or goes past the size the list says, or a finished archive that is not what the index lists, is an error.
    pub(crate) fn chunk(
        &mut self,
        root: Hash,
        offset: u64,
        piece: &[u8],
    ) -> Result<(), ClientError> {
        if !self.requested.contains(&root) {
            return Ok(());
        }
        let Some(listing) = self
            .listed
            .iter()
            .find(|listing| listing.root() == root)
            .cloned()
        else {
            return Ok(());
        };
        let bad = |reason: String| ClientError::BadPack {
            pack: listing.id.clone(),
            reason,
        };
        let incoming = self
            .incoming
            .entry(root)
            .or_insert_with(|| Incoming { bytes: Vec::new() });
        if offset != incoming.bytes.len() as u64 {
            return Err(bad(format!(
                "a piece starts at offset {offset} where {} was due",
                incoming.bytes.len()
            )));
        }
        if (incoming.bytes.len() + piece.len()) as u64 > listing.archive_size {
            return Err(bad(format!(
                "the pieces go past the {} bytes the list says the archive has",
                listing.archive_size
            )));
        }
        incoming.bytes.extend_from_slice(piece);
        if (incoming.bytes.len() as u64) < listing.archive_size {
            return Ok(());
        }
        let Incoming { bytes, .. } = self.incoming.remove(&root).expect("it was just found");
        self.requested.remove(&root);
        let archive = checked(&listing, &bytes).map_err(bad)?;
        if let Err(error) = self.store.save(&listing.id, &root, &bytes) {
            self.store_problems.push(format!(
                "the pack {} could not be kept in the store: {error}",
                listing.id
            ));
        }
        self.held.insert(
            root,
            HeldPack {
                id: listing.id,
                index: listing.index,
                archive,
            },
        );
        Ok(())
    }
}

/// The archive in `bytes`, if it is the canonical archive of the files `listing` says clients are sent, each as the index lists it.
fn checked(listing: &PackListing, bytes: &[u8]) -> Result<Archive, String> {
    if bytes.len() as u64 != listing.archive_size {
        return Err(format!(
            "the archive has {} bytes and the list says {}",
            bytes.len(),
            listing.archive_size
        ));
    }
    let archive = Archive::from_bytes(bytes, &Limits::default())
        .map_err(|error| format!("it is not the canonical archive: {error}"))?;
    listing
        .index
        .verify_received(&archive)
        .map_err(|error| error.to_string())?;
    Ok(archive)
}
