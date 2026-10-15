//! The game's own message, carried inside the engine's generic game messages.
//!
//! Blocks are named by identifier text, never by number, so a client and server that number their
//! content differently still agree.

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use voxel_space::VoxelCoordinate;

/// Client to server: replace one voxel with a block.
pub fn set_block_kind() -> NamespacedId {
    "voxunami_cave:set_block"
        .parse()
        .expect("constant identifier is valid")
}

/// A request to put `block` at `at`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetBlock {
    pub at: VoxelCoordinate,
    pub block: NamespacedId,
}

pub fn encode_set_block(request: &SetBlock) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    for component in [request.at.x(), request.at.y(), request.at.z()] {
        writer.write_signed_varint(component);
    }
    writer.write_identifier(&request.block);
    writer.into_bytes()
}

pub fn decode_set_block(payload: &[u8]) -> Result<SetBlock, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let at = VoxelCoordinate::try_new(
        reader.read_signed_varint()?,
        reader.read_signed_varint()?,
        reader.read_signed_varint()?,
    )
    .map_err(|_| DecodeError::Invalid("a voxel coordinate is out of range"))?;
    let block = reader.read_identifier()?;
    reader.finish()?;
    Ok(SetBlock { at, block })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn a_request_survives_encoding() {
        let request = SetBlock {
            at: VoxelCoordinate::new(-5, 100, 7),
            block: "base_blocks:stone".parse().unwrap(),
        };
        assert_eq!(decode_set_block(&encode_set_block(&request)), Ok(request));
    }

    #[test]
    fn damaged_requests_are_errors_not_panics() {
        let good = encode_set_block(&SetBlock {
            at: VoxelCoordinate::ORIGIN,
            block: "a:b".parse().unwrap(),
        });
        for length in 0..good.len() {
            assert!(decode_set_block(&good[..length]).is_err(), "{length}");
        }
        let mut trailing = good;
        trailing.push(0);
        assert!(decode_set_block(&trailing).is_err());
    }

    proptest! {
        #[test]
        fn decoding_arbitrary_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..80)) {
            let _ = decode_set_block(&bytes);
        }
    }
}
