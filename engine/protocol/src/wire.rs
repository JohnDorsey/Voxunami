//! Encoding of the values messages are built from.

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use voxel_space::{BrickBox, BrickCoordinate, LevelIndex};

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
