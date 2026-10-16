//! The presence messages, carried inside the engine's generic game messages.

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use protocol::ClientId;
use voxel_space::{BRICK_EDGE_LENGTH, BrickCoordinate, LevelIndex, VoxelCoordinate, WorldPosition};

fn kind(text: &str) -> NamespacedId {
    text.parse().expect("constant identifier is valid")
}

/// Client to server: where the player is and which way they look.
pub fn pose_kind() -> NamespacedId {
    kind("player_presence:pose")
}

/// Server to everyone: where every player is.
pub fn players_kind() -> NamespacedId {
    kind("player_presence:players")
}

/// Server to a client: the player has moved to another level. The body is the level's identifier,
/// not its number, so it means the same to a client numbering levels differently.
pub fn level_kind() -> NamespacedId {
    kind("player_presence:level")
}

/// Where a player is and which way they look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerPose {
    pub position: WorldPosition,
    pub yaw: f32,
    pub pitch: f32,
}

/// What the server knows of a player: where they are, and which level they are in. The server
/// decides the level; players only say where they are in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerState {
    pub pose: PlayerPose,
    pub level: LevelIndex,
}

impl PlayerPose {
    /// The distance in voxels from the player to the centre of a voxel.
    pub fn distance_to_voxel(&self, voxel: VoxelCoordinate) -> f64 {
        let brick = self.position.brick();
        let offset = self.position.offset();
        let along = |voxel: i64, brick: i64, offset: f32| {
            (voxel - brick * BRICK_EDGE_LENGTH as i64) as f64 + 0.5 - f64::from(offset)
        };
        let (dx, dy, dz) = (
            along(voxel.x(), brick.x(), offset[0]),
            along(voxel.y(), brick.y(), offset[1]),
            along(voxel.z(), brick.z(), offset[2]),
        );
        (dx * dx + dy * dy + dz * dz).sqrt()
    }
}

fn write_pose(writer: &mut ByteWriter, pose: &PlayerPose) {
    let brick = pose.position.brick();
    for component in [brick.x(), brick.y(), brick.z()] {
        writer.write_signed_varint(component);
    }
    for component in pose.position.offset() {
        writer.write_u32(component.to_bits());
    }
    writer.write_u32(pose.yaw.to_bits());
    writer.write_u32(pose.pitch.to_bits());
}

fn read_pose(reader: &mut ByteReader<'_>) -> Result<PlayerPose, DecodeError> {
    let brick = BrickCoordinate::try_new(
        reader.read_signed_varint()?,
        reader.read_signed_varint()?,
        reader.read_signed_varint()?,
    )
    .map_err(|_| DecodeError::Invalid("a brick coordinate is out of range"))?;
    let mut offset = [0f32; 3];
    for component in &mut offset {
        *component = f32::from_bits(reader.read_u32()?);
        if !(0.0..32.0).contains(component) {
            return Err(DecodeError::Invalid(
                "a position offset is outside its brick",
            ));
        }
    }
    let yaw = f32::from_bits(reader.read_u32()?);
    let pitch = f32::from_bits(reader.read_u32()?);
    if !yaw.is_finite() || !pitch.is_finite() {
        return Err(DecodeError::Invalid("a view angle is not a number"));
    }
    Ok(PlayerPose {
        position: WorldPosition::new(brick, offset),
        yaw,
        pitch,
    })
}

pub fn encode_pose(pose: &PlayerPose) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    write_pose(&mut writer, pose);
    writer.into_bytes()
}

pub fn decode_pose(payload: &[u8]) -> Result<PlayerPose, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let pose = read_pose(&mut reader)?;
    reader.finish()?;
    Ok(pose)
}

/// The level a player is moving to, by identifier.
pub fn encode_level_change(level: &NamespacedId) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    writer.write_identifier(level);
    writer.into_bytes()
}

pub fn decode_level_change(payload: &[u8]) -> Result<NamespacedId, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let level = reader.read_identifier()?;
    reader.finish()?;
    Ok(level)
}

/// Every player's state, in client id order.
pub fn encode_players(players: &[(ClientId, PlayerState)]) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    writer.write_varint(players.len() as u64);
    for (client, state) in players {
        writer.write_u32(client.get());
        writer.write_varint(u64::from(state.level.get()));
        write_pose(&mut writer, &state.pose);
    }
    writer.into_bytes()
}

pub fn decode_players(payload: &[u8]) -> Result<Vec<(ClientId, PlayerState)>, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let count = reader.read_count(4 + 1 + 3 + 20)?;
    let mut players = Vec::with_capacity(count);
    for _ in 0..count {
        let client = ClientId::new(reader.read_u32()?);
        let level = u32::try_from(reader.read_varint()?)
            .map(LevelIndex::new)
            .map_err(|_| DecodeError::Invalid("a level number is too large"))?;
        let pose = read_pose(&mut reader)?;
        players.push((client, PlayerState { pose, level }));
    }
    reader.finish()?;
    Ok(players)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn pose(x: i64, offset: [f32; 3], yaw: f32, pitch: f32) -> PlayerPose {
        PlayerPose {
            position: WorldPosition::new(BrickCoordinate::new(x, -2, 7), offset),
            yaw,
            pitch,
        }
    }

    #[test]
    fn poses_and_player_lists_survive_encoding() {
        let first = pose(3, [1.5, 31.99, 0.0], 0.25, -1.0);
        assert_eq!(decode_pose(&encode_pose(&first)), Ok(first));
        let players = vec![
            (
                ClientId::new(1),
                PlayerState {
                    pose: first,
                    level: LevelIndex::MAIN,
                },
            ),
            (
                ClientId::new(4),
                PlayerState {
                    pose: pose(-9, [0.0; 3], 3.0, 0.5),
                    level: LevelIndex::new(300),
                },
            ),
        ];
        assert_eq!(decode_players(&encode_players(&players)), Ok(players));
        assert_eq!(decode_players(&encode_players(&[])), Ok(vec![]));
    }

    #[test]
    fn a_level_change_names_the_level_by_identifier_and_nothing_else() {
        let level: NamespacedId = "game:nether".parse().unwrap();
        assert_eq!(
            decode_level_change(&encode_level_change(&level)),
            Ok(level.clone())
        );
        let good = encode_level_change(&level);
        for length in 0..good.len() {
            assert!(decode_level_change(&good[..length]).is_err(), "{length}");
        }
        let mut trailing = good;
        trailing.push(0);
        assert!(decode_level_change(&trailing).is_err());
    }

    #[test]
    fn distance_is_measured_to_the_centre_of_the_voxel_across_bricks() {
        let standing = pose(1, [4.0, 6.0, 8.0], 0.0, 0.0); // brick (1, -2, 7)
        let at = |x: i64, y: i64, z: i64| standing.distance_to_voxel(VoxelCoordinate::new(x, y, z));
        // The voxel holding the player is half a voxel from them along each axis.
        assert_eq!(at(32 + 4, -64 + 6, 224 + 8), 0.75_f64.sqrt());
        assert_eq!(
            at(32 + 4 + 3, -64 + 6, 224 + 8),
            (3.5_f64 * 3.5 + 0.5).sqrt()
        );
        assert_eq!(
            at(32 + 4, -64 + 6 + 3, 224 + 8),
            (3.5_f64 * 3.5 + 0.5).sqrt()
        );
        // Negative voxel coordinates are measured the same way.
        let origin = pose(-1, [31.5, 0.5, 0.5], 0.0, 0.0);
        assert_eq!(
            origin.distance_to_voxel(VoxelCoordinate::new(-1, -64, 224)),
            0.0
        );
    }

    #[test]
    fn damaged_or_dishonest_messages_are_errors_not_panics() {
        let good = encode_pose(&pose(0, [1.0; 3], 0.0, 0.0));
        for length in 0..good.len() {
            assert!(
                decode_pose(&good[..length]).is_err(),
                "truncated to {length}"
            );
        }
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(decode_pose(&trailing).is_err());
        // Offsets outside their brick, and angles that are not numbers.
        for bad in [-1.0f32, 32.0, f32::NAN, f32::INFINITY] {
            assert!(
                decode_pose(&encode_raw_pose([bad, 0.0, 0.0], 0.0, 0.0)).is_err(),
                "{bad}"
            );
        }
        for bad in [f32::NAN, f32::INFINITY] {
            assert!(decode_pose(&encode_raw_pose([1.0; 3], bad, 0.0)).is_err());
            assert!(decode_pose(&encode_raw_pose([1.0; 3], 0.0, bad)).is_err());
        }
    }

    /// A pose message built by hand, so values `WorldPosition` would refuse can be sent.
    fn encode_raw_pose(offset: [f32; 3], yaw: f32, pitch: f32) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        for component in [0i64, 0, 0] {
            writer.write_signed_varint(component);
        }
        for component in offset {
            writer.write_u32(component.to_bits());
        }
        writer.write_u32(yaw.to_bits());
        writer.write_u32(pitch.to_bits());
        writer.into_bytes()
    }

    proptest! {
        #[test]
        fn decoding_arbitrary_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..80)) {
            let _ = decode_pose(&bytes);
            let _ = decode_players(&bytes);
            let _ = decode_level_change(&bytes);
        }
    }
}
