//! The packs that already exist in the repository, as folders, go through the archive and come back as the same files.

use std::fs;
use std::path::Path;

use pack_archive::{Archive, FileIndex, Limits};

fn read_folder(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, folder: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((name, fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}

fn check(relative: &str) {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    let files = read_folder(&folder);
    assert!(!files.is_empty(), "{relative} has no files");
    let archive = Archive::from_files(files.clone()).unwrap();
    let bytes = archive.to_bytes();
    let back = Archive::from_bytes(&bytes, &Limits::default()).unwrap();
    assert_eq!(back, archive);
    for (path, content) in &files {
        assert_eq!(back.get(path), Some(content.as_slice()), "{path}");
    }
    let index = FileIndex::of(&back, |_| true);
    index.verify(&back).unwrap();
    assert_eq!(
        index.root_hash(),
        FileIndex::of(&archive, |_| true).root_hash()
    );
}

#[test]
fn the_solar_system_data_pack_round_trips() {
    check("../../games/voxunami_cave/examples/packs/solar_system");
}

#[test]
fn the_example_shader_packs_round_trip() {
    for name in ["cel", "flat", "shadowless"] {
        check(&format!("../renderer/example_shader_packs/{name}"));
    }
}
