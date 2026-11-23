//! The folder a client keeps for a server it has joined (`docs/specs/pack-archive-spec.md`, "The client's folder for a server"): shaped like a world folder, so that
//! what a client keeps of a server is in one place that can later be a world of its own.
//!
//! ```text
//! DATA/worlds/ADDRESS/               one folder for each server, named by its address
//!     remote.json                    which server it is
//!     packs/my_pack-HASH.zip         the packs the server replicates, each the canonical archive, named by its id and the start of its root hash
//! ```
//!
//! A pack in the folder is a pack the client can host itself: `data_packs::read_pack_directory` reads that folder as it reads a server's.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use client::PackStore;
use namespaced_id::NamespacedId;
use pack_archive::{Hash, cache_file_name};
use world_storage::file::write_atomically;

/// The name of the folder for the server at `address` (`host:port`): the address with every character that a file name may not have, and the colon of the port,
/// made a `-` or `_`.
pub(crate) fn folder_name(address: &str) -> String {
    address
        .chars()
        .map(|character| match character {
            ':' => '-',
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' => character,
            _ => '_',
        })
        .collect()
}

/// The folder for the server at `address` in the game data folder `data`.
pub(crate) fn folder_in(data: &Path, address: &str) -> PathBuf {
    data.join("worlds").join(folder_name(address))
}

/// Packs kept as `ID-HASH.zip` files in a folder.
#[derive(Debug)]
pub(crate) struct FolderPackStore {
    folder: PathBuf,
}

impl FolderPackStore {
    pub(crate) fn new(folder: PathBuf) -> Self {
        Self { folder }
    }

    fn path_of(&self, pack_id: &str, root: &Hash) -> Option<PathBuf> {
        cache_file_name(pack_id, root)
            .ok()
            .map(|name| self.folder.join(name))
    }
}

impl PackStore for FolderPackStore {
    fn load(&mut self, pack_id: &str, root: &Hash) -> Option<Vec<u8>> {
        // (Whatever is wrong with the file, the client checks what it is given against the index and asks the server for the pack if it is not good.)
        fs::read(self.path_of(pack_id, root)?).ok()
    }

    fn save(&mut self, pack_id: &str, root: &Hash, archive: &[u8]) -> io::Result<()> {
        let path = self.path_of(pack_id, root).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{pack_id:?} is not an id a cached pack can be named by"),
            )
        })?;
        fs::create_dir_all(&self.folder)?;
        write_atomically(&path, archive)
    }
}

/// Opens the folder for the server at `address` in `data` (made if it is not there, with `remote.json` saying which server it is) and gives the store of its packs.
pub(crate) fn open_pack_store(
    data: &Path,
    game: &NamespacedId,
    address: &str,
) -> io::Result<Box<dyn PackStore>> {
    let folder = folder_in(data, address);
    fs::create_dir_all(&folder)?;
    let remote = folder.join("remote.json");
    if !remote.exists() {
        let text = serde_json::json!({ "address": address, "game": game.to_string() });
        write_atomically(&remote, format!("{text:#}\n").as_bytes())?;
    }
    Ok(Box::new(FolderPackStore::new(folder.join("packs"))))
}

/// The store for the packs of the server at `address` that a player is about to join: in the game data folder, if there is one and the folder for the server can be
/// made, else a store that keeps nothing (the packs are sent again next time, and a line on the error stream says why).
pub(crate) fn pack_store_for_joining(game: &NamespacedId, address: &str) -> Box<dyn PackStore> {
    let Some(data) =
        identity::Identity::default_path().and_then(|path| path.parent().map(Path::to_path_buf))
    else {
        return Box::new(client::NoPackStore);
    };
    match open_pack_store(&data, game, address) {
        Ok(store) => store,
        Err(error) => {
            eprintln!(
                "warning: the packs of {address} cannot be kept in {}: {error}",
                data.display()
            );
            Box::new(client::NoPackStore)
        }
    }
}

#[cfg(test)]
mod tests {
    use data_packs::read_pack_directory;
    use pack_archive::{Archive, FileIndex};

    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "voxunami_remote_world_{name}_{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn a_pack(pack_id: &str) -> (Hash, Vec<u8>) {
        let archive = Archive::from_files([
            (
                "pack.json".to_owned(),
                format!("{{\"id\":\"{pack_id}\"}}").into_bytes(),
            ),
            (
                "blocks/marble.json".to_owned(),
                br#"{ "color": [0.9, 0.9, 0.95] }"#.to_vec(),
            ),
        ])
        .unwrap();
        (
            FileIndex::of(&archive, |_| true).root_hash(),
            archive.to_bytes(),
        )
    }

    #[test]
    fn the_folder_of_a_server_is_named_by_its_address_with_nothing_a_file_name_cannot_have() {
        assert_eq!(folder_name("192.168.1.5:24816"), "192.168.1.5-24816");
        assert_eq!(folder_name("play.example.com"), "play.example.com");
        assert_eq!(folder_name("[::1]:24816"), "_--1_-24816");
        assert_eq!(folder_name("a/b\\c d"), "a_b_c_d");
        assert_eq!(
            folder_in(Path::new("data"), "host:1"),
            Path::new("data").join("worlds").join("host-1")
        );
    }

    #[test]
    fn a_pack_saved_is_loaded_back_under_its_id_and_hash_and_not_under_another() {
        let scratch = Scratch::new("round_trip");
        let mut store = FolderPackStore::new(scratch.0.join("packs"));
        let (root, bytes) = a_pack("my_pack");
        assert_eq!(store.load("my_pack", &root), None);
        store.save("my_pack", &root, &bytes).unwrap();
        assert_eq!(store.load("my_pack", &root), Some(bytes.clone()));
        assert_eq!(store.load("other", &root), None);
        assert_eq!(store.load("my_pack", &Hash::of(b"another")), None);
        // the file is named by the id and the first 32 digits of the hash
        let name = format!("my_pack-{}.zip", root.short_key());
        assert_eq!(fs::read(scratch.0.join("packs").join(name)).unwrap(), bytes);
        // an id no file can be named by is an error to save, and loads nothing
        assert!(store.save("My Pack", &root, &bytes).is_err());
        assert_eq!(store.load("My Pack", &root), None);
    }

    #[test]
    fn saving_again_replaces_the_file_whole_and_leaves_no_temporary_file() {
        let scratch = Scratch::new("replace");
        let mut store = FolderPackStore::new(scratch.0.join("packs"));
        let (root, bytes) = a_pack("my_pack");
        store.save("my_pack", &root, b"not the pack").unwrap();
        store.save("my_pack", &root, &bytes).unwrap();
        assert_eq!(store.load("my_pack", &root), Some(bytes));
        let names: Vec<String> = fs::read_dir(scratch.0.join("packs"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
    }

    #[test]
    fn what_a_client_kept_of_a_server_is_a_folder_of_packs_it_can_host_itself() {
        let scratch = Scratch::new("hostable");
        let game: NamespacedId = "test_game:game".parse().unwrap();
        let mut store = open_pack_store(&scratch.0, &game, "host.example:24816").unwrap();
        let (root, bytes) = a_pack("my_pack");
        store.save("my_pack", &root, &bytes).unwrap();
        let folder = folder_in(&scratch.0, "host.example:24816");
        // it says which server it is, and is made once
        let remote: serde_json::Value =
            serde_json::from_slice(&fs::read(folder.join("remote.json")).unwrap()).unwrap();
        assert_eq!(remote["address"], "host.example:24816");
        assert_eq!(remote["game"], "test_game:game");
        // the packs in it are read as any directory of packs is: the pack's name is its id, and its identity is the root hash the server told
        let packs = read_pack_directory(&folder.join("packs")).unwrap();
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].raw.folder, "my_pack");
        assert_eq!(FileIndex::of(&packs[0].archive, |_| true).root_hash(), root);
        // opening it again keeps what it had, and `remote.json` as it was
        fs::write(folder.join("remote.json"), b"{\"address\":\"edited\"}").unwrap();
        let mut again = open_pack_store(&scratch.0, &game, "host.example:24816").unwrap();
        assert_eq!(again.load("my_pack", &root), Some(bytes));
        assert_eq!(
            fs::read(folder.join("remote.json")).unwrap(),
            b"{\"address\":\"edited\"}"
        );
    }
}
