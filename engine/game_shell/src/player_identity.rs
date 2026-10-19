//! Who the player is to the servers they join: one keypair per install, made the first time it is needed.

use std::path::Path;

use identity::Identity;

/// The player's identity: the key file named, or the one in the game data folder. A missing file is made
/// (and kept: it is the player's name from then on).
pub(crate) fn load(file: Option<&Path>) -> Result<Identity, String> {
    let path = match file {
        Some(path) => path.to_path_buf(),
        None => Identity::default_path().ok_or_else(|| {
            "there is no game data folder to keep your identity in; name a file with --identity"
                .to_owned()
        })?,
    };
    Identity::load_or_create(&path)
        .map_err(|error| format!("the identity file {}: {error}", path.display()))
}

/// An identity for watching a recording, which proves nothing to anyone and is not kept.
pub(crate) fn for_replay() -> Identity {
    Identity::generate()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_file(name: &str) -> std::path::PathBuf {
        let directory =
            std::env::temp_dir().join(format!("voxunami_identity_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        directory.join("identity")
    }

    #[test]
    fn the_named_file_is_made_once_and_is_the_same_player_afterwards() {
        let file = temporary_file("same");
        let first = load(Some(&file)).unwrap();
        assert!(file.exists(), "the key is kept");
        let second = load(Some(&file)).unwrap();
        assert_eq!(first.player_key(), second.player_key());
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn a_file_that_is_not_a_key_is_an_error_naming_it_and_is_left_alone() {
        let file = temporary_file("damaged");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"not a key").unwrap();
        let error = load(Some(&file)).unwrap_err();
        assert!(error.contains("identity"), "{error}");
        assert_eq!(std::fs::read(&file).unwrap(), b"not a key");
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn two_replays_are_not_the_same_player() {
        assert_ne!(for_replay().player_key(), for_replay().player_key());
    }
}
