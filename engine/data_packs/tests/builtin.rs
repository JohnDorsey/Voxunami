//! The pack a program carries in itself (the archive its build script made): read like any pack, and a fault in it is the program's own, which stops it.

use data_packs::{read_archive, read_builtin};
use pack_archive::Archive;

fn archive_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    Archive::from_files(
        files
            .iter()
            .map(|(path, bytes)| ((*path).to_owned(), bytes.to_vec())),
    )
    .unwrap()
    .to_bytes()
}

const MANIFEST: &[u8] = br#"{"id": "inner"}"#;

#[test]
fn a_good_archive_gives_the_pack_any_reader_of_it_gives() {
    let bytes = archive_of(&[
        ("pack.json", MANIFEST),
        ("blocks/a.json", b"A"),
        ("textures/leaf.png", b"picture"),
    ]);
    let expected = read_archive(
        "inner",
        &Archive::from_bytes(&bytes, &Default::default()).unwrap(),
    )
    .unwrap();
    assert_eq!(read_builtin("inner", &bytes), expected);
    assert_eq!(expected.blocks.len(), 1);
    assert_eq!(expected.textures.len(), 1);
}

#[test]
#[should_panic(expected = "the built-in pack inner")]
fn bytes_that_are_not_the_canonical_archive_stop_the_program() {
    read_builtin("inner", b"this is not an archive");
}

#[test]
#[should_panic(expected = "the built-in pack inner")]
fn a_folder_nothing_reads_stops_the_program_and_is_named() {
    read_builtin(
        "inner",
        &archive_of(&[("pack.json", MANIFEST), ("recipes/a.json", b"A")]),
    );
}

#[test]
#[should_panic(expected = "the built-in pack inner")]
fn an_archive_without_a_manifest_stops_the_program() {
    read_builtin("inner", &archive_of(&[("blocks/a.json", b"A")]));
}

#[test]
#[should_panic(expected = "says its id is \"other\"")]
fn a_manifest_that_names_another_pack_stops_the_program() {
    read_builtin(
        "inner",
        &archive_of(&[("pack.json", br#"{"id": "other"}"#)]),
    );
}
