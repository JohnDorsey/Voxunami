//! The presence messages, carried inside the engine's generic game messages.

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use protocol::{ClientId, Message, PlayerKey};
use voxel_space::{BRICK_EDGE_LENGTH, BrickCoordinate, LevelIndex, VoxelCoordinate, WorldPosition};

fn kind(text: &str) -> NamespacedId {
    text.parse().expect("constant identifier is valid")
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

    /// The distance in voxels from the player to a position in the same grid.
    pub fn distance_to_position(&self, position: WorldPosition) -> f64 {
        let (brick, offset) = (self.position.brick(), self.position.offset());
        let (other_brick, other_offset) = (position.brick(), position.offset());
        let along = |other: i64, this: i64, other_offset: f32, offset: f32| {
            ((other - this) * BRICK_EDGE_LENGTH as i64) as f64
                + (f64::from(other_offset) - f64::from(offset))
        };
        let (dx, dy, dz) = (
            along(other_brick.x(), brick.x(), other_offset[0], offset[0]),
            along(other_brick.y(), brick.y(), other_offset[1], offset[1]),
            along(other_brick.z(), brick.z(), other_offset[2], offset[2]),
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

/// Client to server: where the player is and which way they look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoseReport(pub PlayerPose);

impl Message for PoseReport {
    fn kind() -> NamespacedId {
        kind("player_presence:pose")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        write_pose(&mut writer, &self.0);
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let pose = read_pose(&mut reader)?;
        reader.finish()?;
        Ok(Self(pose))
    }

    fn samples() -> Vec<Self> {
        let pose = |x: i64, offset: [f32; 3], yaw: f32, pitch: f32| {
            Self(PlayerPose {
                position: WorldPosition::new(BrickCoordinate::new(x, -2, 7), offset),
                yaw,
                pitch,
            })
        };
        vec![
            pose(3, [1.5, 31.99, 0.0], 0.25, -1.0),
            pose(-9, [0.0, 0.5, 16.0], -3.0, 1.5),
        ]
    }
}

/// Server to a client: the player has moved to another level. The body is the level's identifier,
/// not its number, so it means the same to a client numbering levels differently.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelChange(pub NamespacedId);

impl Message for LevelChange {
    fn kind() -> NamespacedId {
        kind("player_presence:level")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.write_identifier(&self.0);
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let level = reader.read_identifier()?;
        reader.finish()?;
        Ok(Self(level))
    }

    fn samples() -> Vec<Self> {
        vec![Self(kind("test_content:second")), Self(kind("game:nether"))]
    }
}

/// A player entity's component: the public key of the install that plays it. Only the server has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerKeyComponent(pub PlayerKey);

impl Message for PlayerKeyComponent {
    fn kind() -> NamespacedId {
        kind("player_presence:key")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.write_raw(self.0.bytes());
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let bytes: [u8; 32] = reader
            .read_raw(32)?
            .try_into()
            .expect("read_raw gave 32 bytes");
        reader.finish()?;
        Ok(Self(PlayerKey::new(bytes)))
    }

    fn samples() -> Vec<Self> {
        vec![
            Self(PlayerKey::new([0; 32])),
            Self(PlayerKey::new([0xA5; 32])),
        ]
    }
}

/// A player entity's component: the number of the connection that plays it, which is how a client tells
/// which entity is which player. The number of the last connection if the player is away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerClient(pub ClientId);

impl Message for PlayerClient {
    fn kind() -> NamespacedId {
        kind("player_presence:client")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.write_u32(self.0.get());
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let client = ClientId::new(reader.read_u32()?);
        reader.finish()?;
        Ok(Self(client))
    }

    fn samples() -> Vec<Self> {
        vec![Self(ClientId::new(1)), Self(ClientId::new(70_000))]
    }
}

/// A player entity's component: which way the player looks, as the angles they reported (the entity's
/// orientation is the same turn as a quaternion; the angles are kept so that nothing recovers them from it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerLook {
    pub yaw: f32,
    pub pitch: f32,
}

impl Message for PlayerLook {
    fn kind() -> NamespacedId {
        kind("player_presence:look")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.write_u32(self.yaw.to_bits());
        writer.write_u32(self.pitch.to_bits());
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let yaw = f32::from_bits(reader.read_u32()?);
        let pitch = f32::from_bits(reader.read_u32()?);
        reader.finish()?;
        if !yaw.is_finite() || !pitch.is_finite() {
            return Err(DecodeError::Invalid("a view angle is not a number"));
        }
        Ok(Self { yaw, pitch })
    }

    fn samples() -> Vec<Self> {
        vec![
            Self {
                yaw: 0.0,
                pitch: 0.0,
            },
            Self {
                yaw: -3.0,
                pitch: 1.5,
            },
        ]
    }
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
    fn poses_survive_encoding() {
        let first = pose(3, [1.5, 31.99, 0.0], 0.25, -1.0);
        assert_eq!(
            PoseReport::decode(&PoseReport(first).encode()),
            Ok(PoseReport(first))
        );
    }

    #[test]
    fn a_level_change_names_the_level_by_identifier_and_nothing_else() {
        let level: NamespacedId = "game:nether".parse().unwrap();
        let change = LevelChange(level);
        assert_eq!(LevelChange::decode(&change.encode()), Ok(change.clone()));
        let good = change.encode();
        for length in 0..good.len() {
            assert!(LevelChange::decode(&good[..length]).is_err(), "{length}");
        }
        let mut trailing = good;
        trailing.push(0);
        assert!(LevelChange::decode(&trailing).is_err());
    }

    #[test]
    fn the_declared_messages_pass_the_generated_check() {
        assert_eq!(protocol::check_message::<PoseReport>(), Ok(()));
        assert_eq!(protocol::check_message::<LevelChange>(), Ok(()));
        assert_eq!(protocol::check_message::<PlayerKeyComponent>(), Ok(()));
        assert_eq!(protocol::check_message::<PlayerClient>(), Ok(()));
        assert_eq!(protocol::check_message::<PlayerLook>(), Ok(()));
    }

    #[test]
    fn the_distance_to_a_position_is_the_distance_to_a_voxel_when_the_position_is_its_centre() {
        let standing = pose(1, [4.0, 6.0, 8.0], 0.0, 0.0);
        for voxel in [
            VoxelCoordinate::new(36, -58, 232),
            VoxelCoordinate::new(-100, 500, 7),
            VoxelCoordinate::new(31, -64, 224),
        ] {
            let (brick, local) = voxel.split();
            let centre = WorldPosition::new(
                brick,
                [
                    local.x() as f32 + 0.5,
                    local.y() as f32 + 0.5,
                    local.z() as f32 + 0.5,
                ],
            );
            assert_eq!(
                standing.distance_to_position(centre),
                standing.distance_to_voxel(voxel),
                "{voxel:?}"
            );
        }
        // And a position that is not a centre: a quarter of a voxel along from the player's own.
        let beside = standing.position.translated([0.25, 0.0, 0.0]);
        assert_eq!(standing.distance_to_position(beside), 0.25);
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
        let good = PoseReport(pose(0, [1.0; 3], 0.0, 0.0)).encode();
        for length in 0..good.len() {
            assert!(
                PoseReport::decode(&good[..length]).is_err(),
                "truncated to {length}"
            );
        }
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(PoseReport::decode(&trailing).is_err());
        // Offsets outside their brick, and angles that are not numbers.
        for bad in [-1.0f32, 32.0, f32::NAN, f32::INFINITY] {
            assert!(
                PoseReport::decode(&encode_raw_pose([bad, 0.0, 0.0], 0.0, 0.0)).is_err(),
                "{bad}"
            );
        }
        for bad in [f32::NAN, f32::INFINITY] {
            assert!(PoseReport::decode(&encode_raw_pose([1.0; 3], bad, 0.0)).is_err());
            assert!(PoseReport::decode(&encode_raw_pose([1.0; 3], 0.0, bad)).is_err());
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
            let _ = PoseReport::decode(&bytes);
            let _ = LevelChange::decode(&bytes);
            let _ = PlayerKeyComponent::decode(&bytes);
            let _ = PlayerClient::decode(&bytes);
            let _ = PlayerLook::decode(&bytes);
        }
    }
}
