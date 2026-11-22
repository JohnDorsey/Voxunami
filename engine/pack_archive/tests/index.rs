//! The file index and the root hash: what a pack's identity is made of.

mod common;

use common::*;
use pack_archive::{Archive, ArchiveError, FileIndex, Hash, cache_file_name};
use proptest::prelude::*;

fn sample() -> Archive {
    Archive::from_files(sample_files()).unwrap()
}

fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut state = seed | 1;
    for index in (1..items.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        items.swap(index, (state % (index as u64 + 1)) as usize);
    }
}

#[test]
fn the_index_lists_every_file_sorted_with_its_size_and_hash() {
    let index = FileIndex::of(&sample(), |_| true);
    let paths: Vec<&str> = index
        .entries()
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    assert_eq!(
        paths,
        [
            "blocks/lava.json",
            "empty.txt",
            "pack.json",
            "textures/lava.png"
        ]
    );
    let picture = &index.entries()[3];
    assert_eq!(picture.size, 1190);
    assert!(picture.sent);
    assert_eq!(
        picture.hash,
        Hash::of(&(0..=255u8).cycle().take(1190).collect::<Vec<u8>>())
    );
}

#[test]
fn the_hash_of_known_bytes_is_sha_256() {
    // the empty input and "abc" are the standard test vectors of SHA-256
    assert_eq!(
        Hash::of(b"").to_hex(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        Hash::of(b"abc").to_hex(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn a_hash_round_trips_through_hex_and_refuses_bad_hex() {
    let hash = Hash::of(b"x");
    assert_eq!(Hash::from_hex(&hash.to_hex()).unwrap(), hash);
    assert!(Hash::from_hex("abc").is_err());
    assert!(Hash::from_hex(&"g".repeat(64)).is_err());
    assert!(
        Hash::from_hex(&hash.to_hex().to_uppercase()).is_err(),
        "one spelling only"
    );
}

#[test]
fn the_short_key_is_the_first_thirty_two_hex_characters() {
    let root = FileIndex::of(&sample(), |_| true).root_hash();
    assert_eq!(root.short_key(), root.to_hex()[..32]);
}

#[test]
fn the_root_hash_is_the_hash_of_the_index_bytes() {
    let index = FileIndex::of(&sample(), |_| true);
    assert_eq!(index.root_hash(), Hash::of(&index.to_bytes()));
}

#[test]
fn the_index_bytes_round_trip_and_the_reader_refuses_a_short_or_long_one() {
    let index = FileIndex::of(&sample(), |path| path != "empty.txt");
    let bytes = index.to_bytes();
    assert_eq!(FileIndex::from_bytes(&bytes).unwrap(), index);
    for length in 0..bytes.len() {
        assert!(
            FileIndex::from_bytes(&bytes[..length]).is_err(),
            "accepted {length} bytes"
        );
    }
    let mut longer = bytes;
    longer.push(0);
    assert!(FileIndex::from_bytes(&longer).is_err());
}

#[test]
fn an_index_made_of_entries_must_be_sorted_without_duplicates_and_with_valid_paths() {
    let index = FileIndex::of(&sample(), |_| true);
    let mut entries = index.entries().to_vec();
    entries.swap(0, 1);
    assert!(matches!(
        FileIndex::from_entries(entries),
        Err(ArchiveError::NotSorted { .. })
    ));

    let mut entries = index.entries().to_vec();
    entries[1] = entries[0].clone();
    assert!(matches!(
        FileIndex::from_entries(entries),
        Err(ArchiveError::Duplicate { .. })
    ));

    let mut entries = index.entries().to_vec();
    entries[0].path = "Bad".to_owned();
    assert!(matches!(
        FileIndex::from_entries(entries),
        Err(ArchiveError::InvalidPath { .. })
    ));
}

#[test]
fn a_withheld_file_changes_the_root_when_it_changes_though_the_client_never_holds_it() {
    let mut files = sample_files();
    files.push(("scripts/rules.py".to_owned(), b"print(1)".to_vec()));
    let server = Archive::from_files(files.clone()).unwrap();
    let sent = |path: &str| !path.starts_with("scripts/");
    let before = FileIndex::of(&server, sent).root_hash();

    files.last_mut().unwrap().1 = b"print(2)".to_vec();
    let changed = Archive::from_files(files).unwrap();
    let after = FileIndex::of(&changed, sent).root_hash();
    assert_ne!(
        before, after,
        "one byte of withheld code is a different pack"
    );
}

#[test]
fn the_sent_flag_is_part_of_the_identity() {
    let all = FileIndex::of(&sample(), |_| true).root_hash();
    let some = FileIndex::of(&sample(), |path| path != "empty.txt").root_hash();
    assert_ne!(all, some);
}

#[test]
fn the_client_archive_of_the_sent_files_verifies_against_the_index() {
    let mut files = sample_files();
    files.push(("scripts/rules.py".to_owned(), b"print(1)".to_vec()));
    let server = Archive::from_files(files).unwrap();
    let sent = |path: &str| !path.starts_with("scripts/");
    let index = FileIndex::of(&server, sent);
    let client = server.only(sent);
    assert_eq!(client.files().count(), 4);
    index.verify_received(&client).unwrap();
    // the server's own whole archive verifies too: it holds every file, as the index says
    index.verify(&server).unwrap();
    // but a client archive is not a whole one
    assert!(matches!(
        index.verify(&client),
        Err(ArchiveError::MissingFile { path }) if path == "scripts/rules.py"
    ));
}

#[test]
fn verification_names_each_kind_of_difference() {
    let archive = sample();
    let index = FileIndex::of(&archive, |_| true);

    let missing = archive.only(|path| path != "pack.json");
    assert!(
        matches!(index.verify(&missing), Err(ArchiveError::MissingFile { path }) if path == "pack.json")
    );
    assert!(
        matches!(index.verify_received(&missing), Err(ArchiveError::MissingFile { path }) if path == "pack.json")
    );

    let mut files = sample_files();
    files[0].1 = b"{\"id\":\"other_pack\"}".to_vec();
    let changed = Archive::from_files(files).unwrap();
    assert!(
        matches!(index.verify(&changed), Err(ArchiveError::WrongFile { path }) if path == "pack.json")
    );

    let mut files = sample_files();
    files.push(("extra.txt".to_owned(), vec![1]));
    let extra = Archive::from_files(files).unwrap();
    assert!(
        matches!(index.verify(&extra), Err(ArchiveError::UnlistedFile { path }) if path == "extra.txt")
    );
}

#[test]
fn a_file_the_index_withholds_is_refused_if_the_client_was_sent_it_anyway() {
    let archive = sample();
    let index = FileIndex::of(&archive, |path| path != "empty.txt");
    index.verify(&archive).unwrap();
    assert!(matches!(
        index.verify_received(&archive),
        Err(ArchiveError::WithheldFileSent { path }) if path == "empty.txt"
    ));
    index
        .verify_received(&archive.only(|path| path != "empty.txt"))
        .unwrap();
}

#[test]
fn the_cache_file_name_has_the_pack_id_and_the_short_key() {
    let root = FileIndex::of(&sample(), |_| true).root_hash();
    assert_eq!(
        cache_file_name("my_pack", &root).unwrap(),
        format!("my_pack-{}.zip", root.short_key())
    );
}

#[test]
fn a_pack_id_that_is_not_a_safe_file_name_is_refused() {
    let root = Hash::of(b"x");
    for id in [
        "",
        "My_Pack",
        "a b",
        "a/b",
        "..",
        "a-b",
        "a.b",
        "con\0",
        &"a".repeat(65),
    ] {
        assert!(
            matches!(
                cache_file_name(id, &root),
                Err(ArchiveError::BadPackId { .. })
            ),
            "accepted {id:?}"
        );
    }
    assert!(cache_file_name("a", &root).is_ok());
    assert!(cache_file_name(&"a".repeat(64), &root).is_ok());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn changing_one_byte_of_any_file_changes_the_root(
        file in 0usize..4,
        byte in 0usize..2000,
    ) {
        let mut files = sample_files();
        prop_assume!(!files[file].1.is_empty());
        let before = FileIndex::of(&Archive::from_files(files.clone()).unwrap(), |_| true).root_hash();
        let at = byte % files[file].1.len();
        files[file].1[at] ^= 1;
        let after = FileIndex::of(&Archive::from_files(files).unwrap(), |_| true).root_hash();
        prop_assert_ne!(before, after);
    }

    #[test]
    fn the_root_does_not_depend_on_the_order_the_files_were_given(seed in any::<u64>()) {
        let mut files = sample_files();
        let straight = FileIndex::of(&Archive::from_files(files.clone()).unwrap(), |_| true).root_hash();
        shuffle(&mut files, seed);
        let shuffled = FileIndex::of(&Archive::from_files(files).unwrap(), |_| true).root_hash();
        prop_assert_eq!(straight, shuffled);
    }
}
