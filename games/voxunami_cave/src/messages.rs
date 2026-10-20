//! The game's own message, carried inside the engine's generic game messages.
//!
//! Blocks are named by identifier text, never by number, so a client and server that number their
//! content differently still agree. A block's state (which shape it has) is a number the block's
//! own definition gives meaning to.

use crate::items::GameMode;
use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use physics::MoveInput;
use player_presence::{PlayerPose, PoseReport};
use protocol::Message;
use voxel_space::VoxelCoordinate;

use crate::multiscale_light::LightScheme;

fn kind(text: &str) -> NamespacedId {
    text.parse().expect("constant identifier is valid")
}

/// Client to server: a player joins, at this pose. Only the first one from a player counts; after it
/// the server says where the player is (see [`crate::movement`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spawn(pub PlayerPose);

impl Message for Spawn {
    fn kind() -> NamespacedId {
        kind("voxunami_cave:spawn_at")
    }

    fn encode(&self) -> Vec<u8> {
        PoseReport(self.0).encode()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        PoseReport::decode(payload).map(|PoseReport(pose)| Self(pose))
    }

    fn samples() -> Vec<Self> {
        PoseReport::samples()
            .into_iter()
            .map(|PoseReport(pose)| Self(pose))
            .collect()
    }
}

/// What a player asks of their body (the keys held), and which way they look up or down. Client to
/// server.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveRequest {
    pub input: MoveInput,
    /// Radians; positive looks up.
    pub pitch: f32,
}

const JUMP_FLAG: u8 = 1;
const SPRINT_FLAG: u8 = 2;

impl Message for MoveRequest {
    fn kind() -> NamespacedId {
        kind("voxunami_cave:move_input")
    }

    fn encode(&self) -> Vec<u8> {
        let input = &self.input;
        let mut writer = ByteWriter::new();
        for direction in [input.forward, input.strafe, input.vertical] {
            writer.write_u8(direction as u8);
        }
        writer.write_u8(
            if input.jump { JUMP_FLAG } else { 0 } | if input.sprint { SPRINT_FLAG } else { 0 },
        );
        writer.write_u32(input.yaw.to_bits());
        writer.write_u32(self.pitch.to_bits());
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
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
        Ok(Self {
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

    fn samples() -> Vec<Self> {
        let mut samples = Vec::new();
        for direction in -1..=1 {
            for (jump, sprint) in [(false, false), (true, false), (false, true), (true, true)] {
                samples.push(Self {
                    input: MoveInput {
                        forward: direction,
                        strafe: -direction,
                        vertical: 1 - direction.abs(),
                        jump,
                        sprint,
                        yaw: -2.5,
                    },
                    pitch: 0.75,
                });
            }
        }
        samples
    }
}

/// Client to server: replace one voxel with a block. A request to put `block`, in `state`, at `at`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetBlock {
    pub at: VoxelCoordinate,
    pub block: NamespacedId,
    pub state: u32,
}

impl Message for SetBlock {
    fn kind() -> NamespacedId {
        kind("voxunami_cave:set_block")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        for component in [self.at.x(), self.at.y(), self.at.z()] {
            writer.write_signed_varint(component);
        }
        writer.write_identifier(&self.block);
        writer.write_varint(u64::from(self.state));
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
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
        Ok(Self { at, block, state })
    }

    fn samples() -> Vec<Self> {
        let lamp: NamespacedId = kind("base_blocks:lamp");
        vec![
            Self {
                at: VoxelCoordinate::new(-40, 200, 5),
                block: lamp.clone(),
                state: 3,
            },
            Self {
                at: VoxelCoordinate::ORIGIN,
                block: kind("a:b"),
                state: 0,
            },
            Self {
                at: VoxelCoordinate::new(-5, 100, 7),
                block: lamp,
                state: u32::MAX,
            },
        ]
    }
}

/// Server to client: the game mode of the world, which the client needs to show what a player has and what
/// they can place. Sent to a player who joins and to everyone when it changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameModeIs(pub GameMode);

impl Message for GameModeIs {
    fn kind() -> NamespacedId {
        kind("voxunami_cave:game_mode_is")
    }

    fn encode(&self) -> Vec<u8> {
        vec![match self.0 {
            GameMode::Creative => 0,
            GameMode::Survival => 1,
        }]
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        match payload {
            [0] => Ok(Self(GameMode::Creative)),
            [1] => Ok(Self(GameMode::Survival)),
            [_] => Err(DecodeError::Invalid("a game mode is 0 or 1")),
            [] => Err(DecodeError::UnexpectedEnd),
            _ => Err(DecodeError::TrailingBytes(payload.len() - 1)),
        }
    }

    fn samples() -> Vec<Self> {
        vec![Self(GameMode::Creative), Self(GameMode::Survival)]
    }
}

/// Server to client: how the world's light is cut into cells, which the view needs to draw it, or
/// none for a world with the classic light. Sent to a player who joins and to everyone when it
/// changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightSchemeChanged(pub Option<LightScheme>);

impl Message for LightSchemeChanged {
    fn kind() -> NamespacedId {
        kind("voxunami_cave:light_scheme")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        match self.0 {
            None => writer.write_u8(0),
            Some(scheme) => {
                writer.write_u8(scheme.scales() as u8);
                writer.write_u8(scheme.code());
                for &width in scheme.widths() {
                    writer.write_u8(width as u8);
                }
            }
        }
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let scales = usize::from(reader.read_u8()?);
        if scales == 0 {
            reader.finish()?;
            return Ok(Self(None));
        }
        let code = reader.read_u8()?;
        let widths: Vec<u16> = reader
            .read_raw(scales)?
            .iter()
            .map(|&width| u16::from(width))
            .collect();
        reader.finish()?;
        LightScheme::new(32u32.checked_shr(u32::from(code)).unwrap_or(0), &widths)
            .map(|scheme| Self(Some(scheme)))
            .ok_or(DecodeError::Invalid("the light scheme is not one"))
    }

    fn samples() -> Vec<Self> {
        vec![
            Self(None),
            Self(Some(LightScheme::LEVELS_32)),
            Self(Some(LightScheme::LEVELS_16)),
            Self(Some(LightScheme::LEVELS_8)),
            Self(Some(LightScheme::LEVELS_4)),
            Self(Some(LightScheme::new(8, &[1, 3, 7, 17]).expect("a scheme"))),
        ]
    }
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
                assert_eq!(MoveRequest::decode(&request.encode()), Ok(request));
            }
        }
        let good = (MoveRequest {
            input: MoveInput::default(),
            pitch: 0.0,
        })
        .encode();
        for length in 0..good.len() {
            assert!(MoveRequest::decode(&good[..length]).is_err(), "{length}");
        }
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(MoveRequest::decode(&trailing).is_err());
        let mut bad_direction = good.clone();
        bad_direction[1] = 2;
        assert!(MoveRequest::decode(&bad_direction).is_err());
        let mut bad_flags = good.clone();
        bad_flags[3] = 4;
        assert!(MoveRequest::decode(&bad_flags).is_err());
        for angle_at in [4, 8] {
            let mut not_a_number = good.clone();
            not_a_number[angle_at..angle_at + 4].copy_from_slice(&f32::NAN.to_bits().to_le_bytes());
            assert!(MoveRequest::decode(&not_a_number).is_err(), "{angle_at}");
            let mut infinite = good.clone();
            infinite[angle_at..angle_at + 4]
                .copy_from_slice(&f32::INFINITY.to_bits().to_le_bytes());
            assert!(MoveRequest::decode(&infinite).is_err(), "{angle_at}");
        }
    }

    #[test]
    fn the_declared_messages_pass_the_generated_check() {
        assert_eq!(protocol::check_message::<Spawn>(), Ok(()));
        assert_eq!(protocol::check_message::<MoveRequest>(), Ok(()));
        assert_eq!(protocol::check_message::<SetBlock>(), Ok(()));
        assert_eq!(protocol::check_message::<LightSchemeChanged>(), Ok(()));
        assert_eq!(protocol::check_message::<GameModeIs>(), Ok(()));
    }

    #[test]
    fn a_light_scheme_survives_encoding_and_nothing_malformed_is_accepted() {
        let schemes = [
            None,
            Some(LightScheme::LEVELS_32),
            Some(LightScheme::LEVELS_4),
            Some(LightScheme::new(8, &[1, 3, 7, 17]).unwrap()),
            Some(LightScheme::new(4, &[1, 3, 5, 7, 11, 17, 23]).unwrap()),
        ];
        for scheme in schemes {
            assert_eq!(
                LightSchemeChanged::decode(&LightSchemeChanged(scheme).encode()),
                Ok(LightSchemeChanged(scheme))
            );
        }
        let good = LightSchemeChanged(Some(LightScheme::new(8, &[2, 5]).unwrap())).encode();
        for length in 0..good.len() {
            assert!(
                LightSchemeChanged::decode(&good[..length]).is_err(),
                "{length}"
            );
        }
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(LightSchemeChanged::decode(&trailing).is_err());
        assert!(
            LightSchemeChanged::decode(&[0, 0]).is_err(),
            "none, with more"
        );
        // Not a scheme: a level code that does not exist, widths out of order, a width past a brick.
        for bad in [
            vec![2, 4, 1, 2],
            vec![2, 2, 5, 3],
            vec![2, 2, 1, 33],
            vec![8, 2, 1, 2, 3, 4, 5, 6, 7, 8],
            vec![2, 255, 1, 2],
        ] {
            assert!(LightSchemeChanged::decode(&bad).is_err(), "{bad:?}");
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
        assert_eq!(Spawn::decode(&Spawn(pose).encode()), Ok(Spawn(pose)));
        assert!(Spawn::decode(&[1, 2, 3]).is_err());
    }

    #[test]
    fn a_request_survives_encoding() {
        let request = SetBlock {
            at: VoxelCoordinate::new(-5, 100, 7),
            block: "base_blocks:stone".parse().unwrap(),
            state: 5,
        };
        assert_eq!(SetBlock::decode(&request.encode()), Ok(request));
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
            assert_eq!(SetBlock::decode(&request.encode()), Ok(request));
        }
        let mut oversized = ByteWriter::new();
        for _ in 0..3 {
            oversized.write_signed_varint(0);
        }
        oversized.write_identifier(&"a:b".parse().unwrap());
        oversized.write_varint(u64::from(u32::MAX) + 1);
        assert!(SetBlock::decode(&oversized.into_bytes()).is_err());
    }

    #[test]
    fn damaged_requests_are_errors_not_panics() {
        let good = (SetBlock {
            at: VoxelCoordinate::ORIGIN,
            block: "a:b".parse().unwrap(),
            state: 300,
        })
        .encode();
        for length in 0..good.len() {
            assert!(SetBlock::decode(&good[..length]).is_err(), "{length}");
        }
        let mut trailing = good;
        trailing.push(0);
        assert!(SetBlock::decode(&trailing).is_err());
    }

    proptest! {
        #[test]
        fn decoding_arbitrary_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..80)) {
            let _ = SetBlock::decode(&bytes);
            let _ = MoveRequest::decode(&bytes);
            let _ = Spawn::decode(&bytes);
        }
    }
}
