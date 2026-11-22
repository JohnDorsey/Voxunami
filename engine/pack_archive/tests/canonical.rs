//! One set of files has one archive: the same bytes whatever the order, and writing then reading gives the files back.

mod common;

use common::*;
use pack_archive::{Archive, Limits};
use proptest::prelude::*;

fn file_set() -> impl Strategy<Value = Vec<(String, Vec<u8>)>> {
    proptest::collection::btree_map(
        "[a-z0-9_]{1,6}(/[a-z0-9_]{1,6}){0,2}(\\.[a-z]{1,3})?",
        proptest::collection::vec(any::<u8>(), 0..300),
        0..8,
    )
    .prop_map(|map| map.into_iter().collect::<Vec<_>>())
    // a name that is the folder of another ("a" and "a/b") cannot both be files in an archive we accept
    .prop_filter("a file is another's folder", |files| {
        !files.iter().any(|(a, _)| {
            files.iter().any(|(b, _)| {
                b.len() > a.len() && b.starts_with(a.as_str()) && b.as_bytes()[a.len()] == b'/'
            })
        })
    })
}

fn shuffle<T>(items: &mut [T], seed: u64) {
    // a deterministic shuffle from the seed (no wall clock, no hidden randomness)
    let mut state = seed | 1;
    for index in (1..items.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        items.swap(index, (state % (index as u64 + 1)) as usize);
    }
}

#[test]
fn an_empty_archive_is_the_end_record_alone() {
    let bytes = Archive::from_files(Vec::new()).unwrap().to_bytes();
    assert_eq!(bytes.len(), 22);
    assert_eq!(&bytes[..4], &0x0605_4b50u32.to_le_bytes());
    let back = Archive::from_bytes(&bytes, &Limits::default()).unwrap();
    assert_eq!(back.files().count(), 0);
}

#[test]
fn the_layout_of_one_file_is_what_the_documentation_says() {
    let bytes = one_file_archive();
    assert_eq!(bytes.len(), 113);
    assert_eq!(&bytes[0..4], &0x0403_4b50u32.to_le_bytes());
    assert_eq!(&bytes[LOCAL_METHOD..LOCAL_METHOD + 2], &[0, 0], "stored");
    assert_eq!(
        &bytes[LOCAL_TIME..LOCAL_TIME + 4],
        &[0, 0, 0x21, 0],
        "1980-01-01 00:00"
    );
    assert_eq!(&bytes[30..35], b"a.txt");
    assert_eq!(&bytes[35..40], b"hello");
    assert_eq!(&bytes[CENTRAL..CENTRAL + 4], &0x0201_4b50u32.to_le_bytes());
    assert_eq!(&bytes[END..END + 4], &0x0605_4b50u32.to_le_bytes());
}

#[test]
fn the_files_are_written_sorted_by_path_bytes() {
    let archive = Archive::from_files([
        ("textures/b.png".to_owned(), vec![1]),
        ("pack.json".to_owned(), vec![2]),
        ("blocks/a.json".to_owned(), vec![3]),
    ])
    .unwrap();
    let paths: Vec<&str> = archive.files().map(|(path, _)| path).collect();
    assert_eq!(paths, ["blocks/a.json", "pack.json", "textures/b.png"]);
}

#[test]
fn the_fields_that_pin_the_format_have_their_values() {
    // a change of one byte of the writer is a new format and must fail here
    let bytes = one_file_archive();
    let expected_crc = crc32fast::hash(b"hello").to_le_bytes();
    assert_eq!(&bytes[LOCAL_CRC..LOCAL_CRC + 4], &expected_crc);
    assert_eq!(&bytes[CENTRAL_CRC..CENTRAL_CRC + 4], &expected_crc);
    assert_eq!(
        &bytes[CENTRAL_MADE_BY..CENTRAL_MADE_BY + 2],
        &[20, 0],
        "made by DOS, version 2.0"
    );
}

#[test]
fn the_sample_round_trips() {
    let archive = Archive::from_files(sample_files()).unwrap();
    let bytes = archive.to_bytes();
    let back = Archive::from_bytes(&bytes, &Limits::default()).unwrap();
    assert_eq!(back, archive);
    assert_eq!(
        back.to_bytes(),
        bytes,
        "reading then writing changes nothing"
    );
    assert_eq!(back.get("empty.txt"), Some(&[][..]));
    assert_eq!(back.get("pack.json"), Some(&br#"{"id":"my_pack"}"#[..]));
    assert_eq!(back.get("nothing"), None);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn any_order_of_the_same_files_is_the_same_bytes(files in file_set(), seed in any::<u64>()) {
        let sorted = Archive::from_files(files.clone()).unwrap().to_bytes();
        let mut shuffled = files;
        shuffle(&mut shuffled, seed);
        prop_assert_eq!(Archive::from_files(shuffled).unwrap().to_bytes(), sorted);
    }

    #[test]
    fn writing_then_reading_gives_the_files_back_and_writing_again_the_same_bytes(files in file_set()) {
        let archive = Archive::from_files(files).unwrap();
        let bytes = archive.to_bytes();
        let back = Archive::from_bytes(&bytes, &Limits::default()).unwrap();
        prop_assert_eq!(&back, &archive);
        prop_assert_eq!(back.to_bytes(), bytes);
    }
}
