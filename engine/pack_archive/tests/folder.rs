//! The files of a folder on disk, for a build script that makes the archive of a program's own pack.

use std::fs;
use std::path::{Path, PathBuf};

use pack_archive::{Archive, files_of_folder};

/// A folder of the tests' own that goes away when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("pack_archive_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, relative: &str, bytes: &[u8]) {
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

fn everything(_: &str) -> bool {
    true
}

#[test]
fn the_files_come_with_slash_paths_in_the_order_of_the_paths() {
    let folder = Scratch::new("order");
    folder.write("pack.json", b"{}");
    folder.write("blocks/b.json", b"B");
    folder.write("blocks/a.json", b"A");
    folder.write("textures/deep/er.png", b"P");
    let files = files_of_folder(folder.path(), "", &everything).unwrap();
    let paths: Vec<&str> = files.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "blocks/a.json",
            "blocks/b.json",
            "pack.json",
            "textures/deep/er.png"
        ]
    );
    assert_eq!(files[0].1, b"A");
}

#[test]
fn the_prefix_is_put_before_every_path() {
    let folder = Scratch::new("prefix");
    folder.write("a.png", b"A");
    folder.write("sub/b.png", b"B");
    let files = files_of_folder(folder.path(), "textures/", &everything).unwrap();
    let paths: Vec<&str> = files.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(paths, ["textures/a.png", "textures/sub/b.png"]);
}

#[test]
fn names_that_start_with_a_dot_are_left_alone_and_so_are_folders_of_them() {
    let folder = Scratch::new("dots");
    folder.write("pack.json", b"{}");
    folder.write(".DS_Store", b"junk");
    folder.write(".git/config", b"junk");
    folder.write("textures/.hidden.png", b"junk");
    let files = files_of_folder(folder.path(), "", &everything).unwrap();
    let paths: Vec<&str> = files.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(paths, ["pack.json"]);
}

#[test]
fn the_filter_is_given_the_path_from_the_folder_not_the_prefix() {
    let folder = Scratch::new("filter");
    folder.write("a.png", b"A");
    folder.write("a.py", b"script");
    folder.write("README.md", b"notes");
    let only_pictures = |path: &str| path.ends_with(".png");
    let files = files_of_folder(folder.path(), "textures/", &only_pictures).unwrap();
    let paths: Vec<&str> = files.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(paths, ["textures/a.png"]);
}

#[test]
fn a_folder_that_is_not_there_is_an_error() {
    let folder = Scratch::new("missing");
    assert!(files_of_folder(&folder.path().join("nothing"), "", &everything).is_err());
}

#[test]
fn what_is_read_makes_the_archive_of_the_same_files_written_by_hand() {
    let folder = Scratch::new("archive");
    folder.write("pack.json", b"{}");
    folder.write("blocks/a.json", b"A");
    let from_disk = Archive::from_files(files_of_folder(folder.path(), "", &everything).unwrap())
        .unwrap()
        .to_bytes();
    let by_hand = Archive::from_files([
        ("pack.json".to_owned(), b"{}".to_vec()),
        ("blocks/a.json".to_owned(), b"A".to_vec()),
    ])
    .unwrap()
    .to_bytes();
    assert_eq!(from_disk, by_hand);
}
