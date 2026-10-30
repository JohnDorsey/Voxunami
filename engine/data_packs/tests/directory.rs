//! Reading packs from folders on disk.

use std::fs;
use std::path::{Path, PathBuf};

use data_packs::{RawFile, read_directory};

/// A folder of the tests' own that goes away when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("data_packs_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn file(name: &str, text: &str) -> RawFile {
    RawFile {
        name: name.to_owned(),
        text: text.to_owned(),
    }
}

#[test]
fn packs_are_read_from_their_folders_sorted_by_name() {
    let scratch = Scratch::new("read");
    scratch.write("zeta/pack.json", "{\"id\":\"zeta\"}");
    scratch.write("zeta/blocks/b.json", "B");
    scratch.write("zeta/blocks/a.json", "A");
    scratch.write("zeta/blocks/notes.txt", "not a block");
    scratch.write("zeta/patches/p.json", "P");
    scratch.write("zeta/bodies/moon.json", "M");
    scratch.write("zeta/bodies/sun.json", "S");
    scratch.write("alpha/pack.json", "{\"id\":\"alpha\"}");
    scratch.write(".hidden/pack.json", "ignored");
    scratch.write("loose_file.json", "a file in the root is not a pack");

    let packs = read_directory(scratch.path()).unwrap();
    let folders: Vec<&str> = packs.iter().map(|pack| pack.folder.as_str()).collect();
    assert_eq!(folders, ["alpha", "zeta"]);
    let zeta = &packs[1];
    assert_eq!(zeta.manifest, "{\"id\":\"zeta\"}");
    assert_eq!(zeta.blocks, [file("a", "A"), file("b", "B")]);
    assert_eq!(zeta.patches, [file("p", "P")]);
    assert_eq!(zeta.bodies, [file("moon", "M"), file("sun", "S")]);
    assert!(
        packs[0].blocks.is_empty() && packs[0].patches.is_empty() && packs[0].bodies.is_empty()
    );
}

#[test]
fn a_folder_in_a_pack_that_nothing_reads_is_an_error_not_a_silence() {
    let scratch = Scratch::new("unread");
    scratch.write("mine/pack.json", "{\"id\":\"mine\"}");
    scratch.write("mine/blocks/a.json", "A");
    scratch.write("mine/recipes/plank.json", "R");
    scratch.write("mine/sounds/hit.ogg", "S");
    scratch.write("mine/.git/config", "left alone");
    scratch.write("mine/README.md", "a file beside pack.json is allowed");
    let errors = read_directory(scratch.path()).unwrap_err();
    assert_eq!(
        errors.to_string(),
        "mine/recipes: nothing reads this folder; a pack's folders are \"blocks\", \"patches\" and \"bodies\"\n\
         mine/sounds: nothing reads this folder; a pack's folders are \"blocks\", \"patches\" and \"bodies\""
    );
}

#[test]
fn a_misspelt_folder_gets_a_suggestion() {
    let scratch = Scratch::new("misspelt");
    scratch.write("mine/pack.json", "{\"id\":\"mine\"}");
    scratch.write("mine/block/a.json", "A");
    let errors = read_directory(scratch.path()).unwrap_err();
    assert_eq!(
        errors.to_string(),
        "mine/block: nothing reads this folder (did you mean \"blocks\"?); a pack's folders are \"blocks\", \"patches\" and \"bodies\""
    );
}

#[test]
fn a_folder_without_a_manifest_and_a_missing_root_are_errors_that_say_where() {
    let scratch = Scratch::new("missing");
    scratch.write("stray/blocks/a.json", "A");
    scratch.write("fine/pack.json", "{}");
    let errors = read_directory(scratch.path()).unwrap_err();
    assert_eq!(
        errors.to_string(),
        "stray/pack.json: this file is missing: every folder here is a pack and has a pack.json"
    );

    let nowhere = scratch.path().join("no_such_folder");
    let errors = read_directory(&nowhere).unwrap_err();
    assert_eq!(errors.0.len(), 1);
    assert!(
        errors.0[0].to_string().contains("cannot read this folder"),
        "{errors}"
    );
}
