//! The sections the engine itself defines. Every other section in a world file belongs to a plugin
//! and is carried as opaque bytes.
//!
//! All five are at version 1. They are listed here in the order they are written.
//!
//! - `engine:header`: world seed (`u64`), engine version text.
//! - `engine:registries`: for each registry kind, the identifiers of its entries in file-id order.
//!   Everything else in the file refers to content by file id, an index into one of these tables.
//! - `engine:channels`: the grid layout: for each channel its identifier, default value and what its
//!   values mean (a plain number, or an id from a named registry).
//! - `engine:generators`: the default terrain generator, the assignment cube level and the leaves of
//!   the generator-assignment octree (cube origin, level, generator file id plus one, 0 = none).
//! - `engine:override_bricks`: the bricks that replace generated terrain, sorted by coordinate.

use namespaced_id::NamespacedId;
use region_octree::BrickCube;
use voxel_grid::{Brick, ChannelValueKind};
use voxel_space::BrickCoordinate;

use binary_codec::{ByteReader, ByteWriter, DecodeError};

pub const SUPPORTED_VERSION: u32 = 1;

pub fn header_id() -> NamespacedId {
    engine_id("header")
}
pub fn registries_id() -> NamespacedId {
    engine_id("registries")
}
pub fn channels_id() -> NamespacedId {
    engine_id("channels")
}
pub fn generators_id() -> NamespacedId {
    engine_id("generators")
}
pub fn override_bricks_id() -> NamespacedId {
    engine_id("override_bricks")
}

/// The registry kind that holds terrain generator versions.
pub fn terrain_generator_registry_kind() -> NamespacedId {
    engine_id("terrain_generator")
}

fn engine_id(name: &str) -> NamespacedId {
    NamespacedId::new("engine", name).expect("engine section names are valid identifiers")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub world_seed: u64,
    pub engine_version: String,
}

pub fn encode_header(header: &Header) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    writer.write_u64(header.world_seed);
    writer.write_string(&header.engine_version);
    writer.into_bytes()
}

pub fn decode_header(payload: &[u8]) -> Result<Header, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let header = Header {
        world_seed: reader.read_u64()?,
        engine_version: reader.read_string()?.to_owned(),
    };
    reader.finish()?;
    Ok(header)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryTable {
    pub kind: NamespacedId,
    pub entries: Vec<NamespacedId>,
}

pub fn encode_registries(tables: &[RegistryTable]) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    writer.write_varint(tables.len() as u64);
    for table in tables {
        writer.write_identifier(&table.kind);
        writer.write_varint(table.entries.len() as u64);
        for entry in &table.entries {
            writer.write_identifier(entry);
        }
    }
    writer.into_bytes()
}

pub fn decode_registries(payload: &[u8]) -> Result<Vec<RegistryTable>, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let table_count = reader.read_count(2)?;
    let mut tables = Vec::with_capacity(table_count);
    for _ in 0..table_count {
        let kind = reader.read_identifier()?;
        let entry_count = reader.read_count(2)?;
        let mut entries = Vec::with_capacity(entry_count);
        for _ in 0..entry_count {
            entries.push(reader.read_identifier()?);
        }
        tables.push(RegistryTable { kind, entries });
    }
    reader.finish()?;
    Ok(tables)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelRecord {
    pub identifier: NamespacedId,
    pub default_value: u32,
    pub value_kind: ChannelValueKind,
}

pub fn encode_channels(channels: &[ChannelRecord]) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    writer.write_varint(channels.len() as u64);
    for channel in channels {
        writer.write_identifier(&channel.identifier);
        writer.write_u32(channel.default_value);
        match &channel.value_kind {
            ChannelValueKind::Number => writer.write_u8(0),
            ChannelValueKind::Content { registry } => {
                writer.write_u8(1);
                writer.write_identifier(registry);
            }
        }
    }
    writer.into_bytes()
}

pub fn decode_channels(payload: &[u8]) -> Result<Vec<ChannelRecord>, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let count = reader.read_count(7)?;
    let mut channels = Vec::with_capacity(count);
    for _ in 0..count {
        let identifier = reader.read_identifier()?;
        let default_value = reader.read_u32()?;
        let value_kind = match reader.read_u8()? {
            0 => ChannelValueKind::Number,
            1 => ChannelValueKind::Content {
                registry: reader.read_identifier()?,
            },
            _ => return Err(DecodeError::Invalid("unknown channel value kind")),
        };
        channels.push(ChannelRecord {
            identifier,
            default_value,
            value_kind,
        });
    }
    reader.finish()?;
    Ok(channels)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratorsRecord {
    pub default_generator: NamespacedId,
    pub assignment_cube_level: u8,
    /// Leaves of the assignment octree: the cube and the generator's file id, if assigned.
    pub leaves: Vec<(BrickCube, Option<u32>)>,
}

pub fn encode_generators(record: &GeneratorsRecord) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    writer.write_identifier(&record.default_generator);
    writer.write_u8(record.assignment_cube_level);
    writer.write_varint(record.leaves.len() as u64);
    for (cube, generator) in &record.leaves {
        let origin = cube.origin();
        writer.write_signed_varint(origin.x());
        writer.write_signed_varint(origin.y());
        writer.write_signed_varint(origin.z());
        writer.write_u8(cube.level());
        writer.write_varint(generator.map_or(0, |id| u64::from(id) + 1));
    }
    writer.into_bytes()
}

pub fn decode_generators(payload: &[u8]) -> Result<GeneratorsRecord, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let default_generator = reader.read_identifier()?;
    let assignment_cube_level = reader.read_u8()?;
    let count = reader.read_count(5)?;
    let mut leaves = Vec::with_capacity(count);
    for _ in 0..count {
        let origin = read_brick_coordinate(&mut reader)?;
        let level = reader.read_u8()?;
        let cube = BrickCube::new(origin, level)
            .map_err(|_| DecodeError::Invalid("an assignment cube is misaligned or too large"))?;
        let generator = match reader.read_varint()? {
            0 => None,
            plus_one => Some(
                u32::try_from(plus_one - 1)
                    .map_err(|_| DecodeError::Invalid("a generator file id is too large"))?,
            ),
        };
        leaves.push((cube, generator));
    }
    reader.finish()?;
    Ok(GeneratorsRecord {
        default_generator,
        assignment_cube_level,
        leaves,
    })
}

fn read_brick_coordinate(reader: &mut ByteReader<'_>) -> Result<BrickCoordinate, DecodeError> {
    let (x, y, z) = (
        reader.read_signed_varint()?,
        reader.read_signed_varint()?,
        reader.read_signed_varint()?,
    );
    BrickCoordinate::try_new(x, y, z)
        .map_err(|_| DecodeError::Invalid("a brick coordinate is out of range"))
}

/// Bricks must be given sorted by coordinate so equal worlds produce equal bytes.
pub fn encode_override_bricks(bricks: &[(BrickCoordinate, &Brick)]) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    writer.write_varint(bricks.len() as u64);
    let mut encoded = Vec::new();
    for (coordinate, brick) in bricks {
        writer.write_signed_varint(coordinate.x());
        writer.write_signed_varint(coordinate.y());
        writer.write_signed_varint(coordinate.z());
        encoded.clear();
        brick.encode(&mut encoded);
        writer.write_bytes(&encoded);
    }
    writer.into_bytes()
}

pub fn decode_override_bricks(
    payload: &[u8],
) -> Result<Vec<(BrickCoordinate, Brick)>, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let count = reader.read_count(5)?;
    let mut bricks: Vec<(BrickCoordinate, Brick)> = Vec::with_capacity(count);
    for _ in 0..count {
        let coordinate = read_brick_coordinate(&mut reader)?;
        let mut brick_bytes = reader.read_bytes()?;
        let brick = Brick::decode(&mut brick_bytes)
            .map_err(|_| DecodeError::Invalid("a brick is malformed"))?;
        if !brick_bytes.is_empty() {
            return Err(DecodeError::Invalid("a brick has unused bytes after it"));
        }
        if bricks
            .last()
            .is_some_and(|(previous, _)| *previous >= coordinate)
        {
            return Err(DecodeError::Invalid(
                "bricks are not in strictly increasing order",
            ));
        }
        bricks.push((coordinate, brick));
    }
    reader.finish()?;
    Ok(bricks)
}

#[cfg(test)]
mod tests {
    use voxel_grid::test_support::StandardTestLayout;
    use voxel_space::LocalVoxelIndex;

    use super::*;

    fn id(text: &str) -> NamespacedId {
        text.parse().unwrap()
    }

    #[test]
    fn the_header_round_trips_and_rejects_extra_bytes() {
        let header = Header {
            world_seed: 0xDEAD_BEEF_0123,
            engine_version: "0.1.0".to_owned(),
        };
        let mut bytes = encode_header(&header);
        assert_eq!(decode_header(&bytes), Ok(header));
        bytes.push(0);
        assert_eq!(decode_header(&bytes), Err(DecodeError::TrailingBytes(1)));
    }

    #[test]
    fn registry_tables_round_trip_in_order() {
        let tables = vec![
            RegistryTable {
                kind: id("voxel_content:block"),
                entries: vec![id("base:air"), id("gone:ghost")],
            },
            RegistryTable {
                kind: id("engine:terrain_generator"),
                entries: vec![],
            },
        ];
        assert_eq!(decode_registries(&encode_registries(&tables)), Ok(tables));
    }

    #[test]
    fn channels_round_trip_with_both_value_kinds() {
        let channels = vec![
            ChannelRecord {
                identifier: id("core:block"),
                default_value: 0,
                value_kind: ChannelValueKind::Content {
                    registry: id("voxel_content:block"),
                },
            },
            ChannelRecord {
                identifier: id("core:temperature"),
                default_value: 20,
                value_kind: ChannelValueKind::Number,
            },
        ];
        assert_eq!(decode_channels(&encode_channels(&channels)), Ok(channels));
        // Tag 7 is not a value kind.
        let mut bad = encode_channels(&[ChannelRecord {
            identifier: id("a:b"),
            default_value: 1,
            value_kind: ChannelValueKind::Number,
        }]);
        *bad.last_mut().unwrap() = 7;
        assert_eq!(
            decode_channels(&bad),
            Err(DecodeError::Invalid("unknown channel value kind"))
        );
    }

    #[test]
    fn generator_assignments_round_trip_and_misaligned_cubes_are_refused() {
        let cube = |x, y, z, level| BrickCube::new(BrickCoordinate::new(x, y, z), level).unwrap();
        let record = GeneratorsRecord {
            default_generator: id("base_terrain:hills_v1"),
            assignment_cube_level: 5,
            leaves: vec![
                (cube(-64, 0, 32, 5), Some(0)),
                (cube(0, 0, 0, 3), None),
                (cube(8, 0, 0, 3), Some(41)),
            ],
        };
        assert_eq!(
            decode_generators(&encode_generators(&record)),
            Ok(record.clone())
        );

        // Corrupt the first leaf's level so its origin (-64, 0, 32) is no longer aligned to 2^40.
        let mut writer = ByteWriter::new();
        writer.write_identifier(&record.default_generator);
        writer.write_u8(5);
        writer.write_varint(1);
        for value in [-64i64, 0, 32] {
            writer.write_signed_varint(value);
        }
        writer.write_u8(40);
        writer.write_varint(0);
        assert_eq!(
            decode_generators(&writer.into_bytes()),
            Err(DecodeError::Invalid(
                "an assignment cube is misaligned or too large"
            ))
        );
    }

    #[test]
    fn override_bricks_round_trip_and_must_be_sorted() {
        let fixture = StandardTestLayout::default();
        let mut first = Brick::new(&fixture.layout);
        first.set(fixture.material, LocalVoxelIndex::new(1, 2, 3), 9);
        let mut second = Brick::new(&fixture.layout);
        second.set(fixture.temperature, LocalVoxelIndex::new(0, 0, 0), 21);
        let a = BrickCoordinate::new(-5, 0, 2);
        let b = BrickCoordinate::new(-5, 1, 0);
        let bytes = encode_override_bricks(&[(a, &first), (b, &second)]);
        let decoded = decode_override_bricks(&bytes).unwrap();
        assert_eq!(decoded.iter().map(|(c, _)| *c).collect::<Vec<_>>(), [a, b]);
        assert_eq!(decoded[0].1, first);
        assert_eq!(decoded[1].1, second);

        let unsorted = encode_override_bricks(&[(b, &second), (a, &first)]);
        assert_eq!(
            decode_override_bricks(&unsorted),
            Err(DecodeError::Invalid(
                "bricks are not in strictly increasing order"
            ))
        );
        let duplicate = encode_override_bricks(&[(a, &first), (a, &second)]);
        assert!(decode_override_bricks(&duplicate).is_err());
    }
}
