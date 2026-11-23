//! Packs stored as canonical zip archives: read by the same code as a folder, so the same files give the same pack.

use std::fs;
use std::path::{Path, PathBuf};

use data_packs::{RawPack, pack_identities, pack_name_of_zip, read_archive, read_directory};
use pack_archive::{Archive, FileIndex};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("zip_packs_{name}_{}", std::process::id()));
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

fn sample_files() -> Vec<(String, Vec<u8>)> {
    [
        ("pack.json", &b"{\"id\":\"mine\"}"[..]),
        ("blocks/b.json", b"B"),
        ("blocks/a.json", b"A"),
        ("blocks/notes.txt", b"not a block"),
        ("patches/p.json", b"P"),
        ("bodies/moon.json", b"M"),
        ("textures/oak.png", b"plain"),
        ("textures/base_blocks/log_top.png", b"replacement"),
        ("readme.md", b"beside pack.json"),
    ]
    .into_iter()
    .map(|(path, bytes)| (path.to_owned(), bytes.to_vec()))
    .collect()
}

fn write_folder(scratch: &Scratch, pack: &str, files: &[(String, Vec<u8>)]) {
    for (path, bytes) in files {
        scratch.write(&format!("{pack}/{path}"), bytes);
    }
}

fn write_zip(scratch: &Scratch, file_name: &str, files: &[(String, Vec<u8>)]) {
    let archive = Archive::from_files(files.to_vec()).unwrap();
    scratch.write(file_name, &archive.to_bytes());
}

fn same_but_named(first: &RawPack, name: &str) -> RawPack {
    RawPack {
        folder: name.to_owned(),
        ..first.clone()
    }
}

#[test]
fn a_zip_gives_the_pack_its_folder_gives() {
    let scratch = Scratch::new("same");
    write_folder(&scratch, "mine", &sample_files());
    let from_folder = read_directory(scratch.path()).unwrap();

    let other = Scratch::new("same_zip");
    write_zip(&other, "mine.zip", &sample_files());
    let from_zip = read_directory(other.path()).unwrap();

    assert_eq!(from_folder, from_zip);
    assert_eq!(from_zip[0].blocks.len(), 2);
    assert_eq!(from_zip[0].textures.len(), 2);
}

#[test]
fn an_archive_in_memory_gives_the_same_pack() {
    let archive = Archive::from_files(sample_files()).unwrap();
    let from_archive = read_archive("mine", &archive).unwrap();

    let scratch = Scratch::new("memory");
    write_folder(&scratch, "mine", &sample_files());
    assert_eq!(read_directory(scratch.path()).unwrap(), [from_archive]);
}

#[test]
fn folders_and_zips_are_read_together_sorted_by_pack_name() {
    let scratch = Scratch::new("mixed");
    write_folder(&scratch, "mid", &[("pack.json".to_owned(), b"{}".to_vec())]);
    write_zip(
        &scratch,
        "zed.zip",
        &[("pack.json".to_owned(), b"{}".to_vec())],
    );
    write_zip(
        &scratch,
        "ant.zip",
        &[("pack.json".to_owned(), b"{}".to_vec())],
    );
    scratch.write("loose.txt", b"ignored");
    let names: Vec<String> = read_directory(scratch.path())
        .unwrap()
        .into_iter()
        .map(|pack| pack.folder)
        .collect();
    assert_eq!(names, ["ant", "mid", "zed"]);
}

#[test]
fn a_cached_name_with_its_hash_is_the_pack_of_that_id() {
    let files = sample_files();
    let archive = Archive::from_files(files.clone()).unwrap();
    let root = FileIndex::of(&archive, |_| true).root_hash();
    let cache_name = pack_archive::cache_file_name("mine", &root).unwrap();

    let scratch = Scratch::new("cached");
    write_zip(&scratch, &cache_name, &files);
    let packs = read_directory(scratch.path()).unwrap();
    assert_eq!(packs.len(), 1);
    assert_eq!(packs[0].folder, "mine");
}

#[test]
fn the_name_of_a_pack_from_its_zip_file_name() {
    let key = "0123456789abcdef0123456789abcdef";
    assert_eq!(pack_name_of_zip("mine.zip").as_deref(), Some("mine"));
    assert_eq!(
        pack_name_of_zip(&format!("mine-{key}.zip")).as_deref(),
        Some("mine")
    );
    // a dash that is not followed by a key is part of the name
    assert_eq!(pack_name_of_zip("my-pack.zip").as_deref(), Some("my-pack"));
    assert_eq!(
        pack_name_of_zip(&format!("a-b-{key}.zip")).as_deref(),
        Some("a-b")
    );
    // a key of upper-case digits or the wrong length is not a key
    assert_eq!(
        pack_name_of_zip("mine-0123456789ABCDEF0123456789ABCDEF.zip").as_deref(),
        Some("mine-0123456789ABCDEF0123456789ABCDEF")
    );
    assert_eq!(
        pack_name_of_zip("mine-abc.zip").as_deref(),
        Some("mine-abc")
    );
    assert_eq!(pack_name_of_zip("mine.json"), None);
}

#[test]
fn problems_in_a_zip_are_told_as_they_are_for_a_folder() {
    let mut files = sample_files();
    files.push(("recipes/plank.json".to_owned(), b"R".to_vec()));
    let scratch = Scratch::new("problems");
    write_zip(&scratch, "mine.zip", &files);
    let errors = read_directory(scratch.path()).unwrap_err();
    assert_eq!(
        errors.to_string(),
        "mine/recipes: nothing reads this folder; a pack's folders are \"blocks\", \"patches\", \"bodies\" and \"textures\""
    );
}

#[test]
fn a_zip_without_a_manifest_is_an_error_naming_the_pack() {
    let scratch = Scratch::new("no_manifest");
    write_zip(
        &scratch,
        "stray.zip",
        &[("blocks/a.json".to_owned(), b"A".to_vec())],
    );
    let errors = read_directory(scratch.path()).unwrap_err();
    assert_eq!(
        errors.to_string(),
        "stray/pack.json: this file is missing: every folder here is a pack and has a pack.json"
    );
}

#[test]
fn an_authors_own_zip_gives_the_pack_its_folder_gives() {
    let scratch = Scratch::new("authors_zip");
    // deflated, inside a wrapper folder, with a folder entry and a Mac's leftover
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    writer.add_directory("mine/", options).unwrap();
    for (path, bytes) in sample_files() {
        writer.start_file(format!("mine/{path}"), options).unwrap();
        std::io::Write::write_all(&mut writer, &bytes).unwrap();
    }
    writer.start_file("__MACOSX/._pack.json", options).unwrap();
    scratch.write("mine.zip", &writer.finish().unwrap().into_inner());

    let folder = Scratch::new("authors_zip_folder");
    write_folder(&folder, "mine", &sample_files());
    assert_eq!(
        read_directory(scratch.path()).unwrap(),
        read_directory(folder.path()).unwrap()
    );
    assert_eq!(
        pack_identities(scratch.path()).unwrap(),
        pack_identities(folder.path()).unwrap(),
        "the identity is that of the canonical rewrite, so it is the folder's too"
    );
}

#[test]
fn a_zip_that_is_no_zip_or_holds_a_file_that_cannot_be_kept_is_refused_with_its_name_and_the_reason()
 {
    let scratch = Scratch::new("bad_zips");
    scratch.write("garbage.zip", b"this is not a zip at all");
    let mut files = sample_files();
    files.push(("Credits.md".to_owned(), b"x".to_vec()));
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, bytes) in &files {
        writer
            .start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut writer, bytes).unwrap();
    }
    scratch.write("upper.zip", &writer.finish().unwrap().into_inner());
    let errors = read_directory(scratch.path()).unwrap_err();
    let described: Vec<(String, bool)> = errors
        .0
        .iter()
        .map(|error| (error.file.clone(), error.message.contains("Credits.md")))
        .collect();
    assert_eq!(
        described,
        [
            ("garbage.zip".to_owned(), false),
            ("upper.zip".to_owned(), true)
        ]
    );
}

#[test]
fn a_folder_and_a_zip_of_one_name_are_an_error() {
    let scratch = Scratch::new("twins");
    write_folder(&scratch, "mine", &sample_files());
    write_zip(&scratch, "mine.zip", &sample_files());
    let errors = read_directory(scratch.path()).unwrap_err();
    assert_eq!(
        errors.to_string(),
        "mine: two packs have this name (a folder and a zip, or two zips)"
    );
}

#[test]
fn the_example_data_pack_gives_the_same_pack_as_folder_and_as_zip() {
    let folder =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games/voxunami_cave/examples/packs");
    let from_folder = read_directory(&folder).unwrap();
    assert_eq!(from_folder.len(), 1);

    // the same files through an archive
    let mut files = Vec::new();
    let pack = folder.join("solar_system");
    for name in [
        "pack.json",
        "bodies/star.json",
        "bodies/star_planet.json",
        "bodies/star_planet_moon.json",
    ] {
        files.push((name.to_owned(), fs::read(pack.join(name)).unwrap()));
    }
    let archive = Archive::from_files(files).unwrap();
    let from_archive = read_archive("solar_system", &archive).unwrap();
    assert_eq!(
        same_but_named(&from_folder[0], "solar_system"),
        from_archive
    );
}

#[test]
fn a_folder_and_its_zip_have_one_identity_and_it_is_the_index_root_of_their_files() {
    let folder = Scratch::new("identity_folder");
    write_folder(&folder, "mine", &sample_files());
    let zip = Scratch::new("identity_zip");
    write_zip(&zip, "mine.zip", &sample_files());

    let from_folder = pack_identities(folder.path()).unwrap();
    let from_zip = pack_identities(zip.path()).unwrap();
    assert_eq!(from_folder, from_zip);

    let archive = Archive::from_files(sample_files()).unwrap();
    let expected = FileIndex::of(&archive, |_| true).root_hash();
    assert_eq!(from_folder, [("mine".to_owned(), Ok(expected))]);
}

#[test]
fn a_folder_that_cannot_be_archived_says_which_file_and_why() {
    let scratch = Scratch::new("identity_upper");
    let mut files = sample_files();
    files.push(("Credits.md".to_owned(), b"x".to_vec()));
    write_folder(&scratch, "mine", &files);
    let errors = pack_identities(scratch.path()).unwrap_err();
    assert_eq!(errors.0.len(), 1);
    assert_eq!(errors.0[0].file, "mine/Credits.md");
}

#[test]
fn reading_a_directory_gives_each_pack_with_the_canonical_archive_of_all_its_files() {
    // (A folder, a zip, and a file the loader does not read, which is in the archive all the same: a pack is replicated whole.)
    let scratch = Scratch::new("with_archives");
    write_folder(&scratch, "folder_pack", &sample_files());
    write_zip(&scratch, "zip_pack.zip", &sample_files());
    let packs = data_packs::read_pack_directory(scratch.path()).unwrap();
    let names: Vec<&str> = packs.iter().map(|pack| pack.raw.folder.as_str()).collect();
    assert_eq!(names, ["folder_pack", "zip_pack"]);
    // the pack is the pack `read_directory` gives
    assert_eq!(
        packs
            .iter()
            .map(|pack| pack.raw.clone())
            .collect::<Vec<_>>(),
        read_directory(scratch.path()).unwrap()
    );
    // and the archive is the canonical one of the files, the same for a folder and for its zip, with the identity `pack_identities` says
    let expected = Archive::from_files(sample_files()).unwrap();
    assert_eq!(packs[0].archive, expected);
    assert_eq!(packs[1].archive, expected);
    let identities = pack_identities(scratch.path()).unwrap();
    for (pack, (name, identity)) in packs.iter().zip(identities) {
        assert_eq!(name, pack.raw.folder);
        assert_eq!(
            Ok(FileIndex::of(&pack.archive, |_| true).root_hash()),
            identity
        );
    }
    assert!(packs[0].archive.get("readme.md").is_some());
}

#[test]
fn a_directory_with_a_bad_pack_gives_the_errors_of_read_directory() {
    let scratch = Scratch::new("with_archives_bad");
    write_folder(&scratch, "mine", &sample_files());
    scratch.write("mine/blocks/Broken.json", b"{}");
    let errors = data_packs::read_pack_directory(scratch.path()).unwrap_err();
    assert_eq!(
        errors.to_string(),
        read_directory(scratch.path()).unwrap_err().to_string()
    );
}
