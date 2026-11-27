//! Shared helpers of the tests: a zip writer that does not check anything (so a test can build a bad archive on purpose) and small file sets.

#![allow(dead_code)] // each test binary uses some of these

use pack_archive::Archive;

/// A zip written by hand in the canonical layout but without any check, in the order given: duplicates, unsorted or badly named entries can be made with it.
pub fn unchecked_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let offset = out.len() as u32;
        let crc = crc32fast::hash(data);
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]); // flags, method
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&0x0021u16.to_le_bytes()); // date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // made by
        central.extend_from_slice(&20u16.to_le_bytes()); // needed
        central.extend_from_slice(&[0, 0, 0, 0]); // flags, method
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0x0021u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0; 2 + 2 + 2 + 2 + 4]); // extra, comment, disk, internal, external
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let directory_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]); // disks
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&directory_offset.to_le_bytes());
    out.extend_from_slice(&[0, 0]); // comment length
    out
}

/// The canonical archive of one file `a.txt` holding `hello`: 113 bytes. Offsets: local header 0, data 35, central entry 40, end record 91.
pub fn one_file_archive() -> Vec<u8> {
    Archive::from_files([("a.txt".to_owned(), b"hello".to_vec())])
        .expect("valid")
        .to_bytes()
}

pub const LOCAL_FLAGS: usize = 6;
pub const LOCAL_METHOD: usize = 8;
pub const LOCAL_TIME: usize = 10;
pub const LOCAL_CRC: usize = 14;
pub const LOCAL_COMPRESSED_SIZE: usize = 18;
pub const LOCAL_SIZE: usize = 22;
pub const LOCAL_EXTRA_LENGTH: usize = 28;
pub const CENTRAL: usize = 40;
pub const CENTRAL_MADE_BY: usize = CENTRAL + 4;
pub const CENTRAL_FLAGS: usize = CENTRAL + 8;
pub const CENTRAL_METHOD: usize = CENTRAL + 10;
pub const CENTRAL_CRC: usize = CENTRAL + 16;
pub const CENTRAL_COMPRESSED_SIZE: usize = CENTRAL + 20;
pub const CENTRAL_SIZE: usize = CENTRAL + 24;
pub const CENTRAL_EXTRA_LENGTH: usize = CENTRAL + 30;
pub const CENTRAL_COMMENT_LENGTH: usize = CENTRAL + 32;
pub const CENTRAL_EXTERNAL_ATTRIBUTES: usize = CENTRAL + 38;
pub const CENTRAL_OFFSET: usize = CENTRAL + 42;
pub const END: usize = 91;
pub const END_ENTRIES: usize = END + 10;
pub const END_COMMENT_LENGTH: usize = END + 20;

pub fn set(bytes: &mut [u8], at: usize, value: &[u8]) {
    bytes[at..at + value.len()].copy_from_slice(value);
}

/// A small set of files with a folder, a binary file and an empty file.
pub fn sample_files() -> Vec<(String, Vec<u8>)> {
    vec![
        ("pack.json".to_owned(), br#"{"id":"my_pack"}"#.to_vec()),
        ("blocks/lava.json".to_owned(), b"{}".to_vec()),
        (
            "textures/lava.png".to_owned(),
            (0..=255u8).cycle().take(1190).collect(),
        ),
        ("empty.txt".to_owned(), Vec::new()),
    ]
}
