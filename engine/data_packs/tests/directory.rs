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

    fn write_bytes(&self, relative: &str, bytes: &[u8]) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
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
        "mine/recipes: nothing reads this folder; a pack's folders are \"blocks\", \"patches\", \"bodies\" and \"textures\"\n\
         mine/sounds: nothing reads this folder; a pack's folders are \"blocks\", \"patches\", \"bodies\" and \"textures\""
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
        "mine/block: nothing reads this folder (did you mean \"blocks\"?); a pack's folders are \"blocks\", \"patches\", \"bodies\" and \"textures\""
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

#[test]
fn the_pictures_of_a_pack_are_its_png_files_and_the_png_files_in_folders_named_for_namespaces() {
    let scratch = Scratch::new("textures");
    scratch.write("my_pack/pack.json", "{\"id\":\"my_pack\"}");
    scratch.write_bytes("my_pack/textures/oak_top.png", b"top");
    scratch.write_bytes("my_pack/textures/oak.png", b"plain");
    scratch.write_bytes("my_pack/textures/base_blocks/log_top.png", b"replacement");
    scratch.write("my_pack/textures/notes.txt", "not a picture");
    scratch.write_bytes("my_pack/textures/.hidden.png", b"left alone");
    let packs = read_directory(scratch.path()).unwrap();
    let names: Vec<(&str, &[u8])> = packs[0]
        .textures
        .iter()
        .map(|texture| (texture.name.as_str(), texture.bytes.as_slice()))
        .collect();
    assert_eq!(
        names,
        [
            ("base_blocks/log_top", b"replacement".as_slice()),
            ("oak", b"plain".as_slice()),
            ("oak_top", b"top".as_slice()),
        ]
    );
}

#[test]
fn a_folder_inside_a_folder_of_pictures_is_not_read_and_so_is_an_error() {
    let scratch = Scratch::new("deep_textures");
    scratch.write("my_pack/pack.json", "{\"id\":\"my_pack\"}");
    scratch.write_bytes("my_pack/textures/base_blocks/deeper/log_top.png", b"x");
    let errors = read_directory(scratch.path()).unwrap_err();
    assert_eq!(
        errors.to_string(),
        "my_pack/textures/base_blocks/deeper: nothing reads this folder; pictures are in `textures` or in a folder of it named for a namespace"
    );
}
