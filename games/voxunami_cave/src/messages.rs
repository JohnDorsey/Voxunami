//! The game's own message, carried inside the engine's generic game messages.
//!
//! Blocks are named by identifier text, never by number, so a client and server that number their
//! content differently still agree. A block's state (which shape it has) is a number the block's
//! own definition gives meaning to.

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use physics::MoveInput;
use player_presence::{PlayerPose, decode_pose, encode_pose};
use voxel_space::VoxelCoordinate;

/// Client to server: a player joins, at this pose. Only the first one from a player counts; after it
/// the server says where the player is (see [`crate::movement`]).
pub fn spawn_kind() -> NamespacedId {
    "voxunami_cave:spawn_at"
        .parse()
        .expect("constant identifier is valid")
}

pub fn encode_spawn(pose: &PlayerPose) -> Vec<u8> {
    encode_pose(pose)
}

pub fn decode_spawn(payload: &[u8]) -> Result<PlayerPose, DecodeError> {
    decode_pose(payload)
}

/// Client to server: what the player asks of their body, and which way they face.
pub fn move_input_kind() -> NamespacedId {
    "voxunami_cave:move_input"
        .parse()
        .expect("constant identifier is valid")
}

/// What a player asks of their body (the keys held), and which way they look up or down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveRequest {
    pub input: MoveInput,
    /// Radians; positive looks up.
    pub pitch: f32,
}

const JUMP_FLAG: u8 = 1;
const SPRINT_FLAG: u8 = 2;

pub fn encode_move_request(request: &MoveRequest) -> Vec<u8> {
    let input = &request.input;
    let mut writer = ByteWriter::new();
    for direction in [input.forward, input.strafe, input.vertical] {
        writer.write_u8(direction as u8);
    }
    writer.write_u8(
        if input.jump { JUMP_FLAG } else { 0 } | if input.sprint { SPRINT_FLAG } else { 0 },
    );
    writer.write_u32(input.yaw.to_bits());
    writer.write_u32(request.pitch.to_bits());
    writer.into_bytes()
}

pub fn decode_move_request(payload: &[u8]) -> Result<MoveRequest, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let mut directions = [0i8; 3];
    for direction in &mut directions {
        *direction = reader.read_u8()? as i8;
        if !(-1..=1).contains(direction) {
            return Err(DecodeError::Invalid("a direction is not -1, 0 or 1"));
        }
    }
    let flags = reader.read_u8()?;
    if flags & !(JUMP_FLAG | SPRINT_FLAG) != 0 {
        return Err(DecodeError::Invalid("unknown movement flags"));
    }
    let yaw = f32::from_bits(reader.read_u32()?);
    let pitch = f32::from_bits(reader.read_u32()?);
    if !yaw.is_finite() || !pitch.is_finite() {
        return Err(DecodeError::Invalid("an angle is not a number"));
    }
    reader.finish()?;
    Ok(MoveRequest {
        input: MoveInput {
            forward: directions[0],
            strafe: directions[1],
            vertical: directions[2],
            jump: flags & JUMP_FLAG != 0,
            sprint: flags & SPRINT_FLAG != 0,
            yaw,
        },
        pitch,
    })
}

/// Client to server: replace one voxel with a block.
pub fn set_block_kind() -> NamespacedId {
    "voxunami_cave:set_block"
        .parse()
        .expect("constant identifier is valid")
}

/// A request to put `block`, in `state`, at `at`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetBlock {
    pub at: VoxelCoordinate,
    pub block: NamespacedId,
    pub state: u32,
}

pub fn encode_set_block(request: &SetBlock) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    for component in [request.at.x(), request.at.y(), request.at.z()] {
        writer.write_signed_varint(component);
    }
    writer.write_identifier(&request.block);
    writer.write_varint(u64::from(request.state));
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
    let state = u32::try_from(reader.read_varint()?)
        .map_err(|_| DecodeError::Invalid("a block state is out of range"))?;
    reader.finish()?;
    Ok(SetBlock { at, block, state })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn a_movement_request_survives_encoding_and_nothing_malformed_is_accepted() {
        for forward in -1..=1 {
            for flags in [(false, false), (true, false), (false, true), (true, true)] {
                let request = MoveRequest {
                    input: MoveInput {
                        forward,
                        strafe: -forward,
                        vertical: 1 - forward.abs(),
                        jump: flags.0,
                        sprint: flags.1,
                        yaw: -2.5,
                    },
                    pitch: 0.75,
                };
                assert_eq!(
                    decode_move_request(&encode_move_request(&request)),
                    Ok(request)
                );
            }
        }
        let good = encode_move_request(&MoveRequest {
            input: MoveInput::default(),
            pitch: 0.0,
        });
        for length in 0..good.len() {
            assert!(decode_move_request(&good[..length]).is_err(), "{length}");
        }
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(decode_move_request(&trailing).is_err());
        let mut bad_direction = good.clone();
        bad_direction[1] = 2;
        assert!(decode_move_request(&bad_direction).is_err());
        let mut bad_flags = good.clone();
        bad_flags[3] = 4;
        assert!(decode_move_request(&bad_flags).is_err());
        for angle_at in [4, 8] {
            let mut not_a_number = good.clone();
            not_a_number[angle_at..angle_at + 4].copy_from_slice(&f32::NAN.to_bits().to_le_bytes());
            assert!(decode_move_request(&not_a_number).is_err(), "{angle_at}");
            let mut infinite = good.clone();
            infinite[angle_at..angle_at + 4]
                .copy_from_slice(&f32::INFINITY.to_bits().to_le_bytes());
            assert!(decode_move_request(&infinite).is_err(), "{angle_at}");
        }
    }

    #[test]
    fn a_spawn_is_a_pose() {
        use voxel_space::{BrickCoordinate, WorldPosition};
        let pose = PlayerPose {
            position: WorldPosition::new(BrickCoordinate::new(2, -1, 7), [1.5, 20.25, 31.0]),
            yaw: 0.5,
            pitch: -0.25,
        };
        assert_eq!(decode_spawn(&encode_spawn(&pose)), Ok(pose));
        assert!(decode_spawn(&[1, 2, 3]).is_err());
    }

    #[test]
    fn a_request_survives_encoding() {
        let request = SetBlock {
            at: VoxelCoordinate::new(-5, 100, 7),
            block: "base_blocks:stone".parse().unwrap(),
            state: 5,
        };
        assert_eq!(decode_set_block(&encode_set_block(&request)), Ok(request));
    }

    #[test]
    fn every_state_up_to_the_largest_survives_and_larger_ones_are_errors() {
        let request = |state| SetBlock {
            at: VoxelCoordinate::ORIGIN,
            block: "a:b".parse().unwrap(),
            state,
        };
        for state in [0, 1, 127, 128, 70_000, u32::MAX] {
            let request = request(state);
            assert_eq!(decode_set_block(&encode_set_block(&request)), Ok(request));
        }
        let mut oversized = ByteWriter::new();
        for _ in 0..3 {
            oversized.write_signed_varint(0);
        }
        oversized.write_identifier(&"a:b".parse().unwrap());
        oversized.write_varint(u64::from(u32::MAX) + 1);
        assert!(decode_set_block(&oversized.into_bytes()).is_err());
    }

    #[test]
    fn damaged_requests_are_errors_not_panics() {
        let good = encode_set_block(&SetBlock {
            at: VoxelCoordinate::ORIGIN,
            block: "a:b".parse().unwrap(),
            state: 300,
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
            let _ = decode_move_request(&bytes);
            let _ = decode_spawn(&bytes);
        }
    }
}
