//! Makes the canonical archive of the plugin's own pack: every file of the `data` folder (`pack.json`, `blocks/*.json`) and every `.png` of the `textures`
//! folder, which is the painter's working folder (VirtualAtlas, scripts and notes sit beside the pictures, and are not part of the pack). The archive is
//! written to `OUT_DIR` and included by the crate (`docs/specs/pack-archive-spec.md`, step 3), so a block or a picture put in the folders is in the pack
//! with no list to forget it in.

use std::env;
use std::fs;
use std::path::PathBuf;

use pack_archive::{Archive, files_of_folder};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets it"));
    // (A folder named here is looked at whole: a file added to it or changed in it builds the archive again.)
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=data");
    println!("cargo:rerun-if-changed=textures");

    let mut files = files_of_folder(&manifest_dir.join("data"), "", &|_| true)
        .unwrap_or_else(|error| panic!("cannot read the data folder: {error}"));
    // the pictures lie directly in the folder; a `.png` in a folder of it would be a different name in the pack
    let pictures = files_of_folder(&manifest_dir.join("textures"), "textures/", &|path| {
        path.ends_with(".png") && !path.contains('/')
    })
    .unwrap_or_else(|error| panic!("cannot read the textures folder: {error}"));
    files.extend(pictures);
    let archive = Archive::from_files(files)
        .unwrap_or_else(|error| panic!("the files of base_blocks cannot make a pack: {error}"));
    fs::write(out_dir.join("base_blocks.pack.zip"), archive.to_bytes())
        .unwrap_or_else(|error| panic!("cannot write the pack of base_blocks: {error}"));
}
