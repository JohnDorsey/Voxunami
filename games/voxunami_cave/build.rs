//! Makes the canonical archive of Cave's own pack: every file of the `data` folder (`pack.json`, `blocks/*.json`, `bodies/*.json`). The archive is written to
//! `OUT_DIR` and included by the crate (`docs/specs/pack-archive-spec.md`, step 3), so a file put in the folder is in the pack with no list to forget it in.

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

    let files = files_of_folder(&manifest_dir.join("data"), "", &|_| true)
        .unwrap_or_else(|error| panic!("cannot read the data folder: {error}"));
    let archive = Archive::from_files(files)
        .unwrap_or_else(|error| panic!("the files of Cave cannot make a pack: {error}"));
    fs::write(out_dir.join("voxunami_cave.pack.zip"), archive.to_bytes())
        .unwrap_or_else(|error| panic!("cannot write the pack of Cave: {error}"));
}
