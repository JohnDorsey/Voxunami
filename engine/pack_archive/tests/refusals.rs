//! The reader accepts the canonical form and nothing else: each way a zip can differ is refused, with the reason that names it.

mod common;

use common::*;
use pack_archive::{Archive, ArchiveError, Limits};
use proptest::prelude::*;

fn read(bytes: &[u8]) -> Result<Archive, ArchiveError> {
    Archive::from_bytes(bytes, &Limits::default())
}

#[track_caller]
fn refused_as(bytes: &[u8], expected: fn(&ArchiveError) -> bool) {
    match read(bytes) {
        Ok(_) => panic!("accepted a zip that is not canonical"),
        Err(error) => assert!(expected(&error), "refused for the wrong reason: {error}"),
    }
}

#[test]
fn the_canonical_one_file_archive_is_accepted() {
    read(&one_file_archive()).unwrap();
}

#[test]
fn a_compressed_entry_is_refused() {
    let mut bytes = one_file_archive();
    set(&mut bytes, LOCAL_METHOD, &[8, 0]);
    set(&mut bytes, CENTRAL_METHOD, &[8, 0]);
    refused_as(
        &bytes,
        |e| matches!(e, ArchiveError::Compressed { path } if path == "a.txt"),
    );
}

#[test]
fn an_entry_with_an_extra_field_is_refused() {
    let mut bytes = one_file_archive();
    set(&mut bytes, CENTRAL_EXTRA_LENGTH, &[4, 0]);
    refused_as(&bytes, |e| matches!(e, ArchiveError::ExtraField { .. }));
    let mut bytes = one_file_archive();
    set(&mut bytes, LOCAL_EXTRA_LENGTH, &[4, 0]);
    refused_as(&bytes, |e| matches!(e, ArchiveError::ExtraField { .. }));
}

#[test]
fn an_entry_with_a_comment_is_refused() {
    let mut bytes = one_file_archive();
    set(&mut bytes, CENTRAL_COMMENT_LENGTH, &[1, 0]);
    refused_as(&bytes, |e| matches!(e, ArchiveError::Comment { .. }));
}

#[test]
fn an_archive_comment_is_refused() {
    let mut bytes = one_file_archive();
    set(&mut bytes, END_COMMENT_LENGTH, &[3, 0]);
    bytes.extend_from_slice(b"hey");
    refused_as(&bytes, |e| matches!(e, ArchiveError::Malformed { .. }));
}

#[test]
fn a_data_descriptor_flag_is_refused() {
    let mut bytes = one_file_archive();
    set(&mut bytes, LOCAL_FLAGS, &[8, 0]);
    set(&mut bytes, CENTRAL_FLAGS, &[8, 0]);
    refused_as(&bytes, |e| matches!(e, ArchiveError::Flags { .. }));
}

#[test]
fn the_utf8_flag_is_refused_because_names_are_ascii() {
    let mut bytes = one_file_archive();
    set(&mut bytes, LOCAL_FLAGS, &[0, 8]);
    set(&mut bytes, CENTRAL_FLAGS, &[0, 8]);
    refused_as(&bytes, |e| matches!(e, ArchiveError::Flags { .. }));
}

#[test]
fn a_date_other_than_the_fixed_one_is_refused() {
    let mut bytes = one_file_archive();
    set(&mut bytes, LOCAL_TIME, &[1, 0]);
    refused_as(&bytes, |e| matches!(e, ArchiveError::NotCanonical { .. }));
}

#[test]
fn file_permissions_in_the_external_attributes_are_refused() {
    let mut bytes = one_file_archive();
    set(&mut bytes, CENTRAL_EXTERNAL_ATTRIBUTES, &[0, 0, 0xa4, 0x81]);
    refused_as(&bytes, |e| matches!(e, ArchiveError::NotCanonical { .. }));
}

#[test]
fn a_wrong_checksum_is_refused() {
    let mut bytes = one_file_archive();
    set(&mut bytes, LOCAL_CRC, &[1, 2, 3, 4]);
    set(&mut bytes, CENTRAL_CRC, &[1, 2, 3, 4]);
    refused_as(&bytes, |e| matches!(e, ArchiveError::Checksum { .. }));
}

#[test]
fn an_entry_larger_than_it_says_is_refused() {
    // every size field says 4 while 5 bytes are there: the four bytes read do not match the checksum of the five
    let mut bytes = one_file_archive();
    for at in [
        LOCAL_COMPRESSED_SIZE,
        LOCAL_SIZE,
        CENTRAL_COMPRESSED_SIZE,
        CENTRAL_SIZE,
    ] {
        set(&mut bytes, at, &[4, 0, 0, 0]);
    }
    refused_as(&bytes, |e| matches!(e, ArchiveError::Checksum { .. }));
}

#[test]
fn an_entry_that_says_it_is_larger_than_the_limit_is_refused_without_allocating_it() {
    let mut bytes = one_file_archive();
    set(&mut bytes, CENTRAL_SIZE, &[0xff, 0xff, 0xff, 0x7f]);
    set(
        &mut bytes,
        CENTRAL_COMPRESSED_SIZE,
        &[0xff, 0xff, 0xff, 0x7f],
    );
    refused_as(&bytes, |e| matches!(e, ArchiveError::FileTooLarge { .. }));
}

#[test]
fn a_zip64_size_is_refused() {
    let mut bytes = one_file_archive();
    set(&mut bytes, CENTRAL_SIZE, &[0xff; 4]);
    set(&mut bytes, CENTRAL_COMPRESSED_SIZE, &[0xff; 4]);
    refused_as(&bytes, |e| matches!(e, ArchiveError::Zip64));
}

#[test]
fn an_offset_outside_the_archive_is_refused() {
    let mut bytes = one_file_archive();
    set(&mut bytes, CENTRAL_OFFSET, &[0xf0, 0xff, 0, 0]);
    refused_as(&bytes, |e| matches!(e, ArchiveError::Malformed { .. }));
}

#[test]
fn a_short_archive_is_refused_at_every_length() {
    let bytes = one_file_archive();
    for length in 0..bytes.len() {
        assert!(
            read(&bytes[..length]).is_err(),
            "accepted the first {length} bytes"
        );
    }
}

#[test]
fn bytes_after_the_end_record_are_refused() {
    let mut bytes = one_file_archive();
    bytes.push(0);
    refused_as(&bytes, |e| matches!(e, ArchiveError::Malformed { .. }));
}

#[test]
fn bytes_before_the_first_entry_are_refused() {
    let mut bytes = vec![0u8; 4];
    bytes.extend(one_file_archive());
    refused_as(&bytes, |e| matches!(e, ArchiveError::Malformed { .. }));
}

#[test]
fn the_count_of_entries_must_match() {
    let mut bytes = one_file_archive();
    set(&mut bytes, END_ENTRIES, &[2, 0]);
    refused_as(&bytes, |e| matches!(e, ArchiveError::Malformed { .. }));
}

#[test]
fn a_duplicate_path_is_refused() {
    let bytes = unchecked_zip(&[("a.txt", b"1"), ("a.txt", b"2")]);
    refused_as(
        &bytes,
        |e| matches!(e, ArchiveError::Duplicate { path } if path == "a.txt"),
    );
}

#[test]
fn entries_out_of_order_are_refused() {
    let bytes = unchecked_zip(&[("b.txt", b"1"), ("a.txt", b"2")]);
    refused_as(&bytes, |e| matches!(e, ArchiveError::NotSorted { .. }));
}

#[test]
fn the_unchecked_helper_agrees_with_the_writer_on_sorted_input() {
    let by_hand = unchecked_zip(&[("a.txt", b"hello")]);
    assert_eq!(by_hand, one_file_archive());
}

#[test]
fn bad_paths_are_refused_by_the_reader_and_by_the_writer() {
    let bad = [
        "",
        "/abs.txt",
        "a//b.txt",
        "a/",
        "../up.txt",
        "a/../b.txt",
        "./a.txt",
        "a/./b.txt",
        "C:/x.txt",
        "a\\b.txt",
        "Upper.txt",
        "space here.txt",
        "tab\t.txt",
        "nul\0.txt",
        "é.txt",
        "dash-name.txt",
    ];
    for path in bad {
        let bytes = unchecked_zip(&[(path, b"x")]);
        assert!(
            matches!(read(&bytes), Err(ArchiveError::InvalidPath { .. })),
            "the reader did not refuse {path:?} as a bad path"
        );
        assert!(
            matches!(
                Archive::from_files([(path.to_owned(), vec![1])]),
                Err(ArchiveError::InvalidPath { .. })
            ),
            "the writer did not refuse {path:?} as a bad path"
        );
    }
}

#[test]
fn a_path_longer_than_the_limit_is_refused() {
    let long = "a".repeat(256);
    assert!(matches!(
        Archive::from_files([(long, vec![])]),
        Err(ArchiveError::InvalidPath { .. })
    ));
}

#[test]
fn a_file_that_is_another_files_folder_is_refused() {
    let bytes = unchecked_zip(&[("a", b"1"), ("a/b", b"2")]);
    refused_as(&bytes, |e| matches!(e, ArchiveError::FileAndFolder { .. }));
    assert!(matches!(
        Archive::from_files([("a".to_owned(), vec![]), ("a/b".to_owned(), vec![])]),
        Err(ArchiveError::FileAndFolder { .. })
    ));
}

#[test]
fn the_writer_refuses_a_duplicate_path() {
    assert!(matches!(
        Archive::from_files([("a.txt".to_owned(), vec![1]), ("a.txt".to_owned(), vec![2])]),
        Err(ArchiveError::Duplicate { .. })
    ));
}

#[test]
fn the_limits_are_enforced_before_anything_is_held() {
    let bytes = Archive::from_files(sample_files()).unwrap().to_bytes();
    let limits = |files, file_bytes, total| Limits {
        max_files: files,
        max_file_bytes: file_bytes,
        max_total_bytes: total,
    };
    assert!(Archive::from_bytes(&bytes, &limits(4, 1190, 2000)).is_ok());
    assert!(matches!(
        Archive::from_bytes(&bytes, &limits(3, 1190, 2000)),
        Err(ArchiveError::TooManyFiles { .. })
    ));
    assert!(matches!(
        Archive::from_bytes(&bytes, &limits(4, 1189, 2000)),
        Err(ArchiveError::FileTooLarge { .. })
    ));
    assert!(matches!(
        Archive::from_bytes(&bytes, &limits(4, 1190, 1000)),
        Err(ArchiveError::TooLarge { .. })
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Whatever one changed byte does, what is accepted writes back to the very bytes read: the reader never takes a second spelling of an archive.
    #[test]
    fn what_is_accepted_after_one_changed_byte_is_still_canonical(
        position in 0usize..5000,
        value in any::<u8>(),
    ) {
        let bytes = Archive::from_files(sample_files()).unwrap().to_bytes();
        let position = position % bytes.len();
        prop_assume!(bytes[position] != value);
        let mut changed = bytes.clone();
        changed[position] = value;
        if let Ok(archive) = read(&changed) {
            prop_assert_eq!(archive.to_bytes(), changed);
        }
    }

    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
        let _ = read(&bytes);
    }
}
