//! The stored form of bricks, used by save files and (later) the network.
//!
//! Layout (all integers little-endian):
//!
//! - A **channel storage** starts with a tag byte. `0` = uniform: one `u32`. `1` = paletted: the
//!   palette length (`u16`, 2 to 256), the index width in bits (`u8`: 1, 2, 4 or 8), the palette
//!   (`u32` each), then `512 * width` packed index words (`u64` each). `2` = dense: 32768 `u32`s.
//! - A **brick** is its channel count (`u16`) followed by that many channel storages.
//!
//! Encoding always writes the minimal ("compact") form, so equal contents give identical bytes
//! whatever representation the storage happened to be in. Decoding validates everything and never
//! panics on malformed input.

use voxel_space::BRICK_VOXEL_COUNT;

use crate::brick::Brick;
use crate::channel_storage::{ChannelStorage, Representation};
use crate::packed_indices::PackedIndices;

const TAG_UNIFORM: u8 = 0;
const TAG_PALETTED: u8 = 1;
const TAG_DENSE: u8 = 2;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StorageDecodeError {
    #[error("the data ends in the middle of a channel")]
    UnexpectedEnd,
    #[error("unknown channel storage tag {0}")]
    UnknownTag(u8),
    #[error("a palette of {0} entries is not allowed (2 to 256)")]
    InvalidPaletteLength(usize),
    #[error("index width {0} is not allowed (1, 2, 4 or 8 bits)")]
    InvalidIndexWidth(u8),
    #[error("a palette of {length} entries does not fit {bits}-bit indices")]
    PaletteTooLargeForIndexWidth { length: usize, bits: u8 },
    #[error("the palette lists the same value twice")]
    DuplicatePaletteValue,
    #[error("a voxel refers to a palette entry that does not exist")]
    IndexOutsidePalette,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BrickDecodeError {
    #[error("channel {channel}: {source}")]
    Channel {
        channel: usize,
        source: StorageDecodeError,
    },
    #[error("the data ends before the brick's channel count")]
    UnexpectedEnd,
}

fn take<'a>(input: &mut &'a [u8], count: usize) -> Result<&'a [u8], StorageDecodeError> {
    if input.len() < count {
        return Err(StorageDecodeError::UnexpectedEnd);
    }
    let (taken, rest) = input.split_at(count);
    *input = rest;
    Ok(taken)
}

fn take_u32(input: &mut &[u8]) -> Result<u32, StorageDecodeError> {
    let bytes = take(input, 4)?;
    Ok(u32::from_le_bytes(
        bytes.try_into().expect("four bytes were taken"),
    ))
}

impl ChannelStorage {
    /// Appends the canonical stored form of this channel to `output`.
    pub fn encode(&self, output: &mut Vec<u8>) {
        let mut canonical = self.clone();
        canonical.compact();
        match &canonical.representation {
            Representation::Uniform(value) => {
                output.push(TAG_UNIFORM);
                output.extend_from_slice(&value.to_le_bytes());
            }
            Representation::Paletted { palette, indices } => {
                output.push(TAG_PALETTED);
                output.extend_from_slice(&(palette.len() as u16).to_le_bytes());
                output.push(indices.bits_per_index());
                for value in palette {
                    output.extend_from_slice(&value.to_le_bytes());
                }
                for word in indices.words() {
                    output.extend_from_slice(&word.to_le_bytes());
                }
            }
            Representation::Dense(values) => {
                output.push(TAG_DENSE);
                for value in values.iter() {
                    output.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
    }

    /// Reads one channel from the front of `input`, advancing it past the bytes used.
    pub fn decode(input: &mut &[u8]) -> Result<Self, StorageDecodeError> {
        match take(input, 1)?[0] {
            TAG_UNIFORM => Ok(Self::uniform(take_u32(input)?)),
            TAG_PALETTED => {
                let length = usize::from(u16::from_le_bytes(
                    take(input, 2)?.try_into().expect("two bytes were taken"),
                ));
                let bits = take(input, 1)?[0];
                if !(2..=256).contains(&length) {
                    return Err(StorageDecodeError::InvalidPaletteLength(length));
                }
                if !matches!(bits, 1 | 2 | 4 | 8) {
                    return Err(StorageDecodeError::InvalidIndexWidth(bits));
                }
                if length > 1usize << bits {
                    return Err(StorageDecodeError::PaletteTooLargeForIndexWidth { length, bits });
                }
                let mut palette = Vec::with_capacity(length);
                for _ in 0..length {
                    let value = take_u32(input)?;
                    if palette.contains(&value) {
                        return Err(StorageDecodeError::DuplicatePaletteValue);
                    }
                    palette.push(value);
                }
                let word_count = BRICK_VOXEL_COUNT * usize::from(bits) / 64;
                let word_bytes = take(input, word_count * 8)?;
                let words = word_bytes
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|chunk| u64::from_le_bytes(*chunk))
                    .collect();
                let indices = PackedIndices::from_words(bits, words)
                    .expect("the width and word count were validated");
                if !indices.all_slots_below(length) {
                    return Err(StorageDecodeError::IndexOutsidePalette);
                }
                Ok(Self {
                    representation: Representation::Paletted { palette, indices },
                })
            }
            TAG_DENSE => {
                let bytes = take(input, BRICK_VOXEL_COUNT * 4)?;
                let values: Box<[u32]> = bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|chunk| u32::from_le_bytes(*chunk))
                    .collect();
                Ok(Self {
                    representation: Representation::Dense(values),
                })
            }
            tag => Err(StorageDecodeError::UnknownTag(tag)),
        }
    }
}

impl Brick {
    /// Appends the canonical stored form of this brick to `output`.
    pub fn encode(&self, output: &mut Vec<u8>) {
        output.extend_from_slice(&(self.channel_count() as u16).to_le_bytes());
        for channel in self.channels() {
            channel.encode(output);
        }
    }

    /// Reads one brick from the front of `input`, advancing it past the bytes used.
    pub fn decode(input: &mut &[u8]) -> Result<Self, BrickDecodeError> {
        let count = take(input, 2)
            .map_err(|_| BrickDecodeError::UnexpectedEnd)?
            .try_into()
            .map(u16::from_le_bytes)
            .expect("two bytes were taken");
        let mut channels = Vec::with_capacity(usize::from(count));
        for channel in 0..usize::from(count) {
            channels.push(
                ChannelStorage::decode(input)
                    .map_err(|source| BrickDecodeError::Channel { channel, source })?,
            );
        }
        Ok(Self::from_channels(channels))
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use voxel_space::LocalVoxelIndex;

    use super::*;
    use crate::test_support::StandardTestLayout;

    fn voxel(index: usize) -> LocalVoxelIndex {
        LocalVoxelIndex::from_index(index).unwrap()
    }

    fn encoded(storage: &ChannelStorage) -> Vec<u8> {
        let mut bytes = Vec::new();
        storage.encode(&mut bytes);
        bytes
    }

    fn decoded(bytes: &[u8]) -> Result<ChannelStorage, StorageDecodeError> {
        let mut input = bytes;
        let storage = ChannelStorage::decode(&mut input)?;
        assert!(input.is_empty(), "decoding left {} bytes", input.len());
        Ok(storage)
    }

    fn storage_with_distinct_values(count: u32) -> ChannelStorage {
        let mut storage = ChannelStorage::uniform(0);
        for k in 1..=count {
            storage.set(voxel(k as usize), k);
        }
        storage
    }

    #[test]
    fn the_stored_layout_is_exactly_as_documented() {
        assert_eq!(
            encoded(&ChannelStorage::uniform(0x0403_0201)),
            [0, 1, 2, 3, 4]
        );

        let mut two_values = ChannelStorage::uniform(7);
        two_values.set(voxel(0), 9);
        two_values.set(voxel(65), 9);
        let bytes = encoded(&two_values);
        // tag, palette length 2, width 1 bit, palette [9, 7] (first occurrence is voxel 0), 512 words.
        assert_eq!(&bytes[..3], [1, 2, 0]);
        assert_eq!(bytes[3], 1);
        assert_eq!(&bytes[4..8], 9u32.to_le_bytes());
        assert_eq!(&bytes[8..12], 7u32.to_le_bytes());
        assert_eq!(bytes.len(), 1 + 2 + 1 + 8 + 4096);
        // Voxels 0 and 65 use palette entry 0, all others entry 1: word 0 is all ones except bit 0,
        // word 1 is all ones except bit 1 (voxel 65 is bit 1 of word 1).
        assert_eq!(&bytes[12..20], (!1u64).to_le_bytes());
        assert_eq!(&bytes[20..28], (!2u64).to_le_bytes());

        let dense = storage_with_distinct_values(300);
        assert_eq!(encoded(&dense).len(), 1 + 32768 * 4);
    }

    #[test]
    fn every_representation_round_trips() {
        for storage in [
            ChannelStorage::uniform(42),
            storage_with_distinct_values(1),
            storage_with_distinct_values(3),
            storage_with_distinct_values(20),
            storage_with_distinct_values(255),
            storage_with_distinct_values(300),
        ] {
            assert_eq!(decoded(&encoded(&storage)).unwrap(), storage);
        }
    }

    #[test]
    fn equal_contents_give_identical_bytes_whatever_the_representation() {
        let mut stale = ChannelStorage::uniform(5);
        stale.set(voxel(3), 8);
        stale.set(voxel(3), 5);
        assert_eq!(encoded(&stale), encoded(&ChannelStorage::uniform(5)));

        let mut dense_but_few_values = storage_with_distinct_values(300);
        for k in 3..=300 {
            dense_but_few_values.set(voxel(k), 0);
        }
        let compact = ChannelStorage::from_values(&dense_but_few_values.iter().collect::<Vec<_>>());
        assert_eq!(encoded(&dense_but_few_values), encoded(&compact));
    }

    #[test]
    fn decoding_advances_past_exactly_the_bytes_it_used() {
        let mut bytes = encoded(&ChannelStorage::uniform(1));
        bytes.extend(encoded(&storage_with_distinct_values(3)));
        let mut input = &bytes[..];
        let first = ChannelStorage::decode(&mut input).unwrap();
        let second = ChannelStorage::decode(&mut input).unwrap();
        assert_eq!(first, ChannelStorage::uniform(1));
        assert_eq!(second, storage_with_distinct_values(3));
        assert!(input.is_empty());
    }

    /// A paletted channel built by hand so malformed variants can be derived from it.
    fn paletted_bytes(palette: &[u32], bits: u8, fill_slot: u8) -> Vec<u8> {
        let mut bytes = vec![TAG_PALETTED];
        bytes.extend((palette.len() as u16).to_le_bytes());
        bytes.push(bits);
        for value in palette {
            bytes.extend(value.to_le_bytes());
        }
        let word_count = BRICK_VOXEL_COUNT * usize::from(bits) / 64;
        let pattern = u64::from(fill_slot) * (u64::MAX / ((1u64 << bits) - 1));
        for _ in 0..word_count {
            bytes.extend(pattern.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn malformed_channels_are_rejected_with_a_specific_error() {
        assert_eq!(decoded(&[]), Err(StorageDecodeError::UnexpectedEnd));
        assert_eq!(decoded(&[0, 1, 2]), Err(StorageDecodeError::UnexpectedEnd));
        assert_eq!(decoded(&[9]), Err(StorageDecodeError::UnknownTag(9)));
        assert_eq!(
            decoded(&paletted_bytes(&[1], 1, 0)),
            Err(StorageDecodeError::InvalidPaletteLength(1))
        );
        assert_eq!(
            decoded(&paletted_bytes(&[1, 2], 3, 0)),
            Err(StorageDecodeError::InvalidIndexWidth(3))
        );
        assert_eq!(
            decoded(&paletted_bytes(&[1, 2, 3], 1, 0)),
            Err(StorageDecodeError::PaletteTooLargeForIndexWidth { length: 3, bits: 1 })
        );
        assert_eq!(
            decoded(&paletted_bytes(&[5, 5], 1, 0)),
            Err(StorageDecodeError::DuplicatePaletteValue)
        );
        // Two entries but every voxel points at entry 3 (2-bit width allows it to be stored).
        assert_eq!(
            decoded(&paletted_bytes(&[1, 2], 2, 3)),
            Err(StorageDecodeError::IndexOutsidePalette)
        );
        // The same palette with valid indices is accepted.
        assert!(decoded(&paletted_bytes(&[1, 2], 2, 1)).is_ok());
        // Truncating a valid palette or dense channel at every interesting point fails cleanly.
        for bytes in [
            paletted_bytes(&[1, 2], 1, 0),
            encoded(&storage_with_distinct_values(300)),
        ] {
            for cut in [1, 3, 4, 10, bytes.len() / 2, bytes.len() - 1] {
                assert_eq!(
                    decoded(&bytes[..cut]),
                    Err(StorageDecodeError::UnexpectedEnd),
                    "cut at {cut}"
                );
            }
        }
    }

    #[test]
    fn bricks_round_trip_with_their_channel_count() {
        let fixture = StandardTestLayout::default();
        let mut brick = Brick::new(&fixture.layout);
        brick.set(fixture.material, voxel(10), 3);
        brick.set(fixture.temperature, voxel(11), 99);
        let mut bytes = Vec::new();
        brick.encode(&mut bytes);
        assert_eq!(&bytes[..2], [2, 0]);

        let mut input = &bytes[..];
        assert_eq!(Brick::decode(&mut input).unwrap(), brick);
        assert!(input.is_empty());

        let untouched = Brick::new(&fixture.layout);
        let mut bytes = Vec::new();
        untouched.encode(&mut bytes);
        assert_eq!(
            bytes.len(),
            2 + 5 + 5,
            "an untouched two-channel brick is 12 bytes"
        );
    }

    #[test]
    fn a_bad_channel_inside_a_brick_is_reported_by_position() {
        let mut bytes = vec![2, 0];
        encoded(&ChannelStorage::uniform(1))
            .iter()
            .for_each(|b| bytes.push(*b));
        bytes.push(77);
        let mut input = &bytes[..];
        assert_eq!(
            Brick::decode(&mut input),
            Err(BrickDecodeError::Channel {
                channel: 1,
                source: StorageDecodeError::UnknownTag(77)
            })
        );
        assert_eq!(
            Brick::decode(&mut &[1u8][..]),
            Err(BrickDecodeError::UnexpectedEnd)
        );
    }

    proptest! {
        /// Whatever bytes arrive, decoding returns an error or a storage that re-encodes to bytes
        /// that decode to the same storage. It never panics.
        #[test]
        fn arbitrary_bytes_never_panic_and_accepted_ones_are_stable(
            bytes in proptest::collection::vec(any::<u8>(), 0..2000),
            prefix in prop_oneof![Just(None), Just(Some(TAG_UNIFORM)), Just(Some(TAG_PALETTED)), Just(Some(TAG_DENSE))],
        ) {
            let mut input_bytes = bytes;
            if let Some(tag) = prefix {
                input_bytes.insert(0, tag);
            }
            let mut input = &input_bytes[..];
            if let Ok(storage) = ChannelStorage::decode(&mut input) {
                let again = decoded(&encoded(&storage)).unwrap();
                prop_assert!(again == storage);
            }
        }

        #[test]
        fn random_contents_round_trip_to_identical_bytes(
            seed in any::<u64>(),
            distinct in prop_oneof![Just(1usize), Just(2), Just(7), Just(100), Just(400)],
        ) {
            let mut state = seed | 1;
            let values: Vec<u32> = (0..BRICK_VOXEL_COUNT)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    (state % distinct as u64) as u32 * 31
                })
                .collect();
            let storage = ChannelStorage::from_values(&values);
            let bytes = encoded(&storage);
            let round_tripped = decoded(&bytes).unwrap();
            prop_assert!(round_tripped.iter().eq(values.iter().copied()));
            prop_assert_eq!(encoded(&round_tripped), bytes);
        }
    }
}
