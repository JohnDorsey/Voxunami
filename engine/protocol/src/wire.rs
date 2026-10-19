//! Encoding of the values messages are built from.

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use voxel_space::{
    BrickBox, BrickCoordinate, EntityPose, GridId, LevelIndex, Orientation, WorldPosition,
};

pub fn write_level(writer: &mut ByteWriter, level: LevelIndex) {
    writer.write_varint(u64::from(level.get()));
}

pub fn read_level(reader: &mut ByteReader<'_>) -> Result<LevelIndex, DecodeError> {
    u32::try_from(reader.read_varint()?)
        .map(LevelIndex::new)
        .map_err(|_| DecodeError::Invalid("a level number is too large"))
}

pub fn write_brick_coordinate(writer: &mut ByteWriter, coordinate: BrickCoordinate) {
    writer.write_signed_varint(coordinate.x());
    writer.write_signed_varint(coordinate.y());
    writer.write_signed_varint(coordinate.z());
}

pub fn read_brick_coordinate(reader: &mut ByteReader<'_>) -> Result<BrickCoordinate, DecodeError> {
    let (x, y, z) = (
        reader.read_signed_varint()?,
        reader.read_signed_varint()?,
        reader.read_signed_varint()?,
    );
    BrickCoordinate::try_new(x, y, z)
        .map_err(|_| DecodeError::Invalid("a brick coordinate is out of range"))
}

pub fn write_brick_box(writer: &mut ByteWriter, region: BrickBox) {
    write_brick_coordinate(writer, region.min());
    write_brick_coordinate(writer, region.max());
}

pub fn read_brick_box(reader: &mut ByteReader<'_>) -> Result<BrickBox, DecodeError> {
    let (min, max) = (
        read_brick_coordinate(reader)?,
        read_brick_coordinate(reader)?,
    );
    BrickBox::new(min, max).ok_or(DecodeError::Invalid(
        "a brick box has its minimum corner above its maximum",
    ))
}

/// An entity's pose: its grid, the brick and the offset in it, and the four components of its orientation.
pub fn write_entity_pose(writer: &mut ByteWriter, pose: &EntityPose) {
    writer.write_varint(pose.grid.get());
    write_brick_coordinate(writer, pose.position.brick());
    for component in pose.position.offset() {
        writer.write_u32(component.to_bits());
    }
    for component in pose.orientation.components() {
        writer.write_u32(component.to_bits());
    }
}

pub fn read_entity_pose(reader: &mut ByteReader<'_>) -> Result<EntityPose, DecodeError> {
    let grid = GridId::new(reader.read_varint()?);
    let brick = read_brick_coordinate(reader)?;
    let mut offset = [0f32; 3];
    for component in &mut offset {
        *component = f32::from_bits(reader.read_u32()?);
        if !(0.0..32.0).contains(component) {
            return Err(DecodeError::Invalid(
                "a position offset is outside its brick",
            ));
        }
    }
    let mut components = [0f32; 4];
    for component in &mut components {
        *component = f32::from_bits(reader.read_u32()?);
    }
    let orientation = Orientation::from_components(components)
        .ok_or(DecodeError::Invalid("an orientation is not a rotation"))?;
    Ok(EntityPose::new(
        grid,
        WorldPosition::new(brick, offset),
        orientation,
    ))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use voxel_space::test_support::entity_pose;

    use super::*;

    fn encoded(pose: &EntityPose) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        write_entity_pose(&mut writer, pose);
        writer.into_bytes()
    }

    fn decoded(bytes: &[u8]) -> Result<EntityPose, DecodeError> {
        let mut reader = ByteReader::new(bytes);
        let pose = read_entity_pose(&mut reader)?;
        reader.finish()?;
        Ok(pose)
    }

    proptest! {
        #[test]
        fn a_pose_survives_encoding(pose in entity_pose()) {
            prop_assert_eq!(decoded(&encoded(&pose)), Ok(pose));
        }

        #[test]
        fn a_pose_cut_short_or_followed_by_more_is_refused(pose in entity_pose()) {
            let bytes = encoded(&pose);
            for length in 0..bytes.len() {
                prop_assert!(decoded(&bytes[..length]).is_err(), "cut at {}", length);
            }
            let mut longer = bytes;
            longer.push(0);
            prop_assert!(decoded(&longer).is_err());
        }

        #[test]
        fn decoding_arbitrary_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..80)) {
            let _ = decoded(&bytes);
        }
    }

    #[test]
    fn the_layout_of_a_pose_is_pinned() {
        let pose = EntityPose::new(
            GridId::root(voxel_space::LevelIndex::new(2)),
            WorldPosition::new(BrickCoordinate::new(3, -2, 7), [1.5, 31.5, 0.0]),
            Orientation::IDENTITY,
        );
        let hex: String = encoded(&pose)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(
            hex,
            concat!(
                "0206030e", "0000c03f", "0000fc41", "00000000", "00000000", "00000000", "00000000",
                "0000803f"
            )
        );
    }

    #[test]
    fn an_offset_outside_its_brick_or_a_non_rotation_is_refused() {
        let good = encoded(&EntityPose::new(
            GridId::MAIN,
            WorldPosition::new(BrickCoordinate::ORIGIN, [1.0, 2.0, 3.0]),
            Orientation::IDENTITY,
        ));
        // The grid is one byte and the brick three, so the first offset starts at byte 4.
        for bad in [-1.0f32, 32.0, f32::NAN, f32::INFINITY] {
            let mut bytes = good.clone();
            bytes[4..8].copy_from_slice(&bad.to_bits().to_le_bytes());
            assert!(decoded(&bytes).is_err(), "{bad}");
        }
        let mut bytes = good;
        let at = bytes.len() - 4;
        bytes[at..].copy_from_slice(&2.0f32.to_bits().to_le_bytes());
        assert!(decoded(&bytes).is_err(), "not a unit quaternion");
    }
}
