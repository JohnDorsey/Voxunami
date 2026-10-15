//! The outer envelope of a world file: a list of checksummed, length-prefixed sections.
//!
//! ```text
//! magic "VOXUNAMI" | container version (u32) | section count (varint)
//! per section: identifier (string) | section version (varint) | payload length (varint)
//!              | CRC-32 of identifier, version and payload (u32) | payload
//! ```
//!
//! The container knows nothing about what sections mean. That is what lets a world written with
//! plugins that are not installed be loaded and saved again without losing their data: their
//! sections are carried along untouched.

use namespaced_id::NamespacedId;

use binary_codec::{ByteReader, ByteWriter, DecodeError};

pub const MAGIC: [u8; 8] = *b"VOXUNAMI";
pub const CONTAINER_VERSION: u32 = 1;

/// One named, versioned blob of a world file. The meaning of the payload belongs to whoever owns the
/// identifier's namespace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub identifier: NamespacedId,
    pub version: u32,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ContainerError {
    #[error("this is not a Voxunami world file")]
    NotAWorldFile,
    #[error("the file uses container version {0}, which this program cannot read")]
    UnsupportedContainerVersion(u32),
    #[error("the file is damaged: {0}")]
    Malformed(#[from] DecodeError),
    #[error("section {section} is corrupt (its checksum does not match)")]
    CorruptSection { section: NamespacedId },
    #[error("section {0} appears more than once")]
    DuplicateSection(NamespacedId),
}

/// Covers everything that gives a payload its meaning, so damage to the identifier or version is
/// detected as well as damage to the payload.
fn checksum(identifier: &NamespacedId, version: u32, payload: &[u8]) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(identifier.as_str().as_bytes());
    hasher.update(&version.to_le_bytes());
    hasher.update(payload);
    hasher.finalize()
}

pub fn write_container(sections: &[Section]) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    writer.write_raw(&MAGIC);
    writer.write_u32(CONTAINER_VERSION);
    writer.write_varint(sections.len() as u64);
    for section in sections {
        writer.write_identifier(&section.identifier);
        writer.write_varint(u64::from(section.version));
        writer.write_varint(section.payload.len() as u64);
        writer.write_u32(checksum(
            &section.identifier,
            section.version,
            &section.payload,
        ));
        writer.write_raw(&section.payload);
    }
    writer.into_bytes()
}

pub fn read_container(bytes: &[u8]) -> Result<Vec<Section>, ContainerError> {
    let mut reader = ByteReader::new(bytes);
    if reader
        .read_raw(MAGIC.len())
        .map_err(|_| ContainerError::NotAWorldFile)?
        != MAGIC
    {
        return Err(ContainerError::NotAWorldFile);
    }
    let version = reader.read_u32()?;
    if version != CONTAINER_VERSION {
        return Err(ContainerError::UnsupportedContainerVersion(version));
    }
    // A section takes at least an identifier, version, length and checksum.
    let count = reader.read_count(8)?;
    let mut sections: Vec<Section> = Vec::with_capacity(count);
    for _ in 0..count {
        let identifier = reader.read_identifier()?;
        let section_version = u32::try_from(reader.read_varint()?)
            .map_err(|_| DecodeError::Invalid("section version is too large"))?;
        let length = reader.read_count(1)?;
        let expected_checksum = reader.read_u32()?;
        let payload = reader.read_raw(length)?;
        if checksum(&identifier, section_version, payload) != expected_checksum {
            return Err(ContainerError::CorruptSection {
                section: identifier,
            });
        }
        if sections
            .iter()
            .any(|section| section.identifier == identifier)
        {
            return Err(ContainerError::DuplicateSection(identifier));
        }
        sections.push(Section {
            identifier,
            version: section_version,
            payload: payload.to_vec(),
        });
    }
    reader.finish()?;
    Ok(sections)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn section(identifier: &str, version: u32, payload: &[u8]) -> Section {
        Section {
            identifier: identifier.parse().unwrap(),
            version,
            payload: payload.to_vec(),
        }
    }

    fn sample() -> Vec<Section> {
        vec![
            section("engine:header", 1, &[1, 2, 3]),
            section("magic:spellbook", 7, b"fireball"),
            section("empty:section", 0, &[]),
        ]
    }

    #[test]
    fn sections_round_trip_in_order() {
        let bytes = write_container(&sample());
        assert_eq!(read_container(&bytes), Ok(sample()));
    }

    #[test]
    fn the_file_starts_with_the_magic_and_version() {
        let bytes = write_container(&[]);
        assert_eq!(&bytes[..8], b"VOXUNAMI");
        assert_eq!(&bytes[8..12], [1, 0, 0, 0]);
        assert_eq!(bytes.len(), 13);
    }

    #[test]
    fn files_that_are_not_worlds_or_are_too_new_are_refused() {
        assert_eq!(read_container(b""), Err(ContainerError::NotAWorldFile));
        assert_eq!(
            read_container(b"PNG\x89 not a world at all"),
            Err(ContainerError::NotAWorldFile)
        );
        let mut bytes = write_container(&sample());
        bytes[8] = 2;
        assert_eq!(
            read_container(&bytes),
            Err(ContainerError::UnsupportedContainerVersion(2))
        );
    }

    #[test]
    fn flipping_any_payload_byte_names_the_damaged_section() {
        let bytes = write_container(&sample());
        let position = bytes
            .windows(8)
            .position(|window| window == b"fireball")
            .unwrap();
        for offset in 0..8 {
            let mut damaged = bytes.clone();
            damaged[position + offset] ^= 0x10;
            assert_eq!(
                read_container(&damaged),
                Err(ContainerError::CorruptSection {
                    section: "magic:spellbook".parse().unwrap()
                })
            );
        }
    }

    #[test]
    fn truncation_and_trailing_bytes_are_errors_at_every_length() {
        let bytes = write_container(&sample());
        for length in 0..bytes.len() {
            assert!(
                read_container(&bytes[..length]).is_err(),
                "length {length} was accepted"
            );
        }
        let mut with_extra = bytes;
        with_extra.push(0);
        assert_eq!(
            read_container(&with_extra),
            Err(ContainerError::Malformed(DecodeError::TrailingBytes(1)))
        );
    }

    #[test]
    fn a_repeated_section_identifier_is_refused() {
        let bytes = write_container(&[section("a:b", 1, &[1]), section("a:b", 1, &[2])]);
        assert_eq!(
            read_container(&bytes),
            Err(ContainerError::DuplicateSection("a:b".parse().unwrap()))
        );
    }

    /// Every byte of a small valid file, changed in every possible way: the result must be an error
    /// or exactly the original data. Exhaustive, because a random sample can miss one field.
    #[test]
    fn every_single_byte_change_is_detected_or_harmless() {
        let original = write_container(&sample());
        for position in 0..original.len() {
            for flip in 1..=255u8 {
                let mut damaged = original.clone();
                damaged[position] ^= flip;
                if let Ok(sections) = read_container(&damaged) {
                    assert_eq!(
                        sections,
                        sample(),
                        "byte {position} xor {flip:#04x} changed the data unnoticed"
                    );
                }
            }
        }
    }

    proptest! {
        #[test]
        fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
            let _ = read_container(&bytes);
            let mut with_magic = MAGIC.to_vec();
            with_magic.extend([1, 0, 0, 0]);
            with_magic.extend(&bytes);
            let _ = read_container(&with_magic);
        }
    }
}
