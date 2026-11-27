//! An author's own zip (deflated, with folders, a wrapper folder, a Mac's leftovers) is rewritten into the canonical archive; whatever cannot be is refused
//! with the reason. Needs the `import` feature.

mod common;

use std::io::{Cursor, Write};

use common::{sample_files, unchecked_zip};
use pack_archive::{Archive, ArchiveError, Limits, import_zip};
use proptest::prelude::*;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// A zip as an ordinary tool writes it: deflated entries, in the given order, with the given names as they are (no checking).
fn author_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in entries {
        if name.ends_with('/') {
            writer.add_directory(*name, options).unwrap();
        } else {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
    }
    writer.finish().unwrap().into_inner()
}

fn import(bytes: &[u8]) -> Result<Archive, ArchiveError> {
    import_zip(bytes, &Limits::default())
}

fn sample_entries() -> Vec<(String, Vec<u8>)> {
    sample_files()
}

fn borrowed(entries: &[(String, Vec<u8>)]) -> Vec<(&str, &[u8])> {
    entries
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect()
}

#[test]
fn a_deflated_zip_becomes_the_canonical_archive_of_the_same_files() {
    let entries = sample_entries();
    let imported = import(&author_zip(&borrowed(&entries))).unwrap();
    let expected = Archive::from_files(entries).unwrap();
    assert_eq!(imported, expected);
    assert_eq!(imported.to_bytes(), expected.to_bytes());
}

#[test]
fn the_order_the_author_put_the_files_in_does_not_matter() {
    let mut entries = sample_entries();
    let straight = import(&author_zip(&borrowed(&entries))).unwrap().to_bytes();
    entries.reverse();
    let reversed = import(&author_zip(&borrowed(&entries))).unwrap().to_bytes();
    assert_eq!(straight, reversed);
}

#[test]
fn a_canonical_archive_imports_to_itself() {
    let archive = Archive::from_files(sample_entries()).unwrap();
    assert_eq!(import(&archive.to_bytes()).unwrap(), archive);
}

#[test]
fn folder_entries_are_dropped() {
    let mut entries = vec![("blocks/", &b""[..]), ("textures/", b"")];
    entries.push(("pack.json", b"{}"));
    entries.push(("blocks/a.json", b"A"));
    let imported = import(&author_zip(&entries)).unwrap();
    let paths: Vec<&str> = imported.files().map(|(path, _)| path).collect();
    assert_eq!(paths, ["blocks/a.json", "pack.json"]);
}

#[test]
fn the_folder_a_pack_was_zipped_inside_is_taken_off() {
    let imported = import(&author_zip(&[
        ("my_pack/", b""),
        ("my_pack/pack.json", b"{}"),
        ("my_pack/blocks/a.json", b"A"),
    ]))
    .unwrap();
    let paths: Vec<&str> = imported.files().map(|(path, _)| path).collect();
    assert_eq!(paths, ["blocks/a.json", "pack.json"]);
}

#[test]
fn a_folder_that_is_not_a_wrapper_is_left_where_it_is() {
    // pack.json is at the root already: a lone `blocks` folder beside it is a folder of the pack, not a wrapper
    let imported = import(&author_zip(&[
        ("pack.json", b"{}"),
        ("blocks/a.json", b"A"),
    ]))
    .unwrap();
    assert!(imported.get("blocks/a.json").is_some());
    // two top-level folders and no pack.json: nothing is taken off
    let imported = import(&author_zip(&[("one/a.json", b"A"), ("two/b.json", b"B")])).unwrap();
    assert!(imported.get("one/a.json").is_some() && imported.get("two/b.json").is_some());
    // two top-level folders, one of them with a pack.json: it is not a wrapper, there is something beside it
    let imported = import(&author_zip(&[
        ("one/pack.json", b"{}"),
        ("two/b.json", b"B"),
    ]))
    .unwrap();
    assert!(imported.get("one/pack.json").is_some() && imported.get("two/b.json").is_some());
    // one folder without a pack.json in it: nothing is taken off either (the pack is what is wrong, and says so later)
    let imported = import(&author_zip(&[("one/a.json", b"A")])).unwrap();
    assert!(imported.get("one/a.json").is_some());
}

#[test]
fn the_leftovers_of_a_mac_and_hidden_files_are_dropped() {
    let imported = import(&author_zip(&[
        ("pack.json", b"{}"),
        ("__MACOSX/._pack.json", b"junk"),
        (".DS_Store", b"junk"),
        (".git/config", b"junk"),
        ("textures/.hidden.png", b"junk"),
    ]))
    .unwrap();
    let paths: Vec<&str> = imported.files().map(|(path, _)| path).collect();
    assert_eq!(paths, ["pack.json"]);
}

#[test]
fn backslashes_and_a_leading_dot_slash_are_the_same_path() {
    let imported = import(&unchecked_zip(&[
        ("blocks\\a.json", b"A"),
        ("./pack.json", b"{}"),
    ]))
    .unwrap();
    let paths: Vec<&str> = imported.files().map(|(path, _)| path).collect();
    assert_eq!(paths, ["blocks/a.json", "pack.json"]);
}

#[test]
fn an_upper_case_name_is_refused_naming_the_file_and_is_never_changed() {
    let error = import(&author_zip(&[("pack.json", b"{}"), ("README.md", b"x")])).unwrap_err();
    assert!(
        matches!(&error, ArchiveError::InvalidPath { path, .. } if path == "README.md"),
        "{error}"
    );
}

#[test]
fn a_path_that_leaves_the_pack_is_refused() {
    for name in ["../up.json", "a/../../b.json", "/abs.json", "C:/x.json"] {
        let error = import(&unchecked_zip(&[(name, b"x")])).unwrap_err();
        assert!(
            matches!(error, ArchiveError::InvalidPath { .. }),
            "{name}: {error}"
        );
    }
}

#[test]
fn two_entries_of_one_path_are_refused() {
    // exactly one name: the zip crate would keep one of them and say nothing, so this is found from the count the zip declares
    let error = import(&unchecked_zip(&[("a.txt", b"1"), ("a.txt", b"2")])).unwrap_err();
    assert!(
        matches!(&error, ArchiveError::Import { reason } if reason.contains("same name")),
        "{error}"
    );
    // names that differ in spelling and are one path
    for names in [("a/b.txt", r"a\b.txt"), ("a.txt", "./a.txt")] {
        let error = import(&unchecked_zip(&[(names.0, b"1"), (names.1, b"2")])).unwrap_err();
        assert!(
            matches!(error, ArchiveError::Duplicate { .. }),
            "{names:?}: {error}"
        );
    }
}

#[test]
fn a_symbolic_link_is_refused() {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .add_symlink("link", "/etc/passwd", SimpleFileOptions::default())
        .unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    let error = import(&bytes).unwrap_err();
    assert!(
        matches!(&error, ArchiveError::Import { reason } if reason.contains("symbolic link")),
        "{error}"
    );
}

#[test]
fn a_file_larger_than_the_limit_is_refused_without_inflating_it() {
    // 20 MB of zeros deflates to a few kilobytes: the declared size is what is checked, before any inflating
    let zeros = vec![0u8; 20_000_000];
    let bytes = author_zip(&[("pack.json", b"{}"), ("big.bin", &zeros)]);
    assert!(bytes.len() < 100_000);
    let limits = Limits {
        max_files: 10,
        max_file_bytes: 1_000_000,
        max_total_bytes: 100_000_000,
    };
    assert!(matches!(
        import_zip(&bytes, &limits),
        Err(ArchiveError::FileTooLarge { .. })
    ));
    let limits = Limits {
        max_files: 10,
        max_file_bytes: 100_000_000,
        max_total_bytes: 1_000_000,
    };
    assert!(matches!(
        import_zip(&bytes, &limits),
        Err(ArchiveError::TooLarge { .. })
    ));
    let limits = Limits {
        max_files: 1,
        max_file_bytes: 100_000_000,
        max_total_bytes: 100_000_000,
    };
    assert!(matches!(
        import_zip(&bytes, &limits),
        Err(ArchiveError::TooManyFiles { .. })
    ));
}

#[test]
fn a_damaged_entry_is_refused() {
    let entries = sample_entries();
    let bytes = author_zip(&borrowed(&entries));
    // flip a byte in the middle of the data of the binary picture, which is the largest entry
    let mut broken = bytes.clone();
    let middle = bytes.len() / 2;
    broken[middle] ^= 0xff;
    assert!(import(&broken).is_err());
}

#[test]
fn bytes_that_are_not_a_zip_are_refused_at_every_cut() {
    assert!(matches!(
        import(b"this is not a zip"),
        Err(ArchiveError::Import { .. })
    ));
    let bytes = author_zip(&borrowed(&sample_entries()));
    for length in 0..bytes.len() {
        assert!(import(&bytes[..length]).is_err(), "accepted {length} bytes");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
        let _ = import(&bytes);
    }

    #[test]
    fn one_changed_byte_is_refused_or_gives_an_archive_that_is_canonical(
        position in 0usize..5000,
        value in any::<u8>(),
    ) {
        let bytes = author_zip(&borrowed(&sample_entries()));
        let mut changed = bytes.clone();
        let position = position % changed.len();
        changed[position] = value;
        if let Ok(archive) = import(&changed) {
            let canonical = archive.to_bytes();
            prop_assert_eq!(Archive::from_bytes(&canonical, &Limits::default()).unwrap(), archive);
        }
    }
}
