//! The sections the engine itself defines. Every other section in a world file belongs to a plugin
//! and is carried as opaque bytes.
//!
//! All of them are at version 1, except `engine:generators`, which is at version 2 (version 1 files still
//! load). A file has either `engine:levels`, or (every file written before levels existed) a
//! `engine:generators` and an `engine:override_bricks`, which describe its one level. They are listed
//! here in the order they are written.
//!
//! - `engine:header`: world seed (`u64`), engine version text.
//! - `engine:registries`: for each registry kind, the identifiers of its entries in file-id order.
//!   Everything else in the file refers to content by file id, an index into one of these tables.
//! - `engine:channels`: the grid layout: for each channel its identifier, default value and what its
//!   values mean (a plain number, or an id from a named registry).
//! - `engine:generators`: the default terrain generator, the assignment cube level and the leaves of
//!   the generator-assignment octree. Version 2 lists each leaf as one number (generator file id plus
//!   one, 0 = none, times 64, plus the cube level), in Morton order: the leaves tile the whole space, so
//!   where each one is follows from the levels before it. Version 1 also gave each leaf's origin.
//! - `engine:levels`: the world's levels in order, the first being the main level where players start.
//!   Per level: its identifier, its generator setup (as the payload of `engine:generators`, version 2) and
//!   its edited bricks (as the payload of `engine:override_bricks`).
//! - `engine:override_bricks`: the bricks that replace generated terrain, sorted by coordinate. (Files
//!   written before levels existed; newer files keep them inside `engine:levels`.)

use namespaced_id::NamespacedId;
use region_octree::BrickCube;
use voxel_grid::{Brick, ChannelValueKind};
use voxel_space::BrickCoordinate;

use binary_codec::{ByteReader, ByteWriter, DecodeError};

pub const SUPPORTED_VERSION: u32 = 1;

/// The version of `engine:generators` this program writes.
pub const GENERATORS_VERSION: u32 = 2;

/// Whether this program can read `engine:generators` at this version.
pub fn reads_generators_version(version: u32) -> bool {
    (1..=GENERATORS_VERSION).contains(&version)
}

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
pub fn levels_id() -> NamespacedId {
    engine_id("levels")
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
    /// Leaves of the assignment octree in Morton order, so they tile the whole space: the cube and
    /// the generator's file id, if assigned.
    pub leaves: Vec<(BrickCube, Option<u32>)>,
}

/// Version 2 of the section. A world's own leaves tile the space; a description of a world's shape has none.
/// Panics for any other list.
pub fn encode_generators(record: &GeneratorsRecord) -> Vec<u8> {
    let levels: Vec<u8> = record.leaves.iter().map(|(cube, _)| cube.level()).collect();
    assert!(
        levels.is_empty()
            || region_octree::tile_from_levels(&levels)
                .is_ok_and(|cubes| cubes.iter().eq(record.leaves.iter().map(|(cube, _)| cube))),
        "assignment leaves must be the leaves of an octree, in order"
    );
    let mut writer = ByteWriter::new();
    writer.write_identifier(&record.default_generator);
    writer.write_u8(record.assignment_cube_level);
    writer.write_varint(record.leaves.len() as u64);
    for (cube, generator) in &record.leaves {
        let generator_plus_one = generator.map_or(0, |id| u64::from(id) + 1);
        writer.write_varint(generator_plus_one << LEVEL_BITS | u64::from(cube.level()));
    }
    writer.into_bytes()
}

/// How many low bits of a leaf's number hold its level (levels run to 56).
const LEVEL_BITS: u32 = 6;

/// Reads the section at the given version (1 or 2; see [`reads_generators_version`]).
pub fn decode_generators(payload: &[u8], version: u32) -> Result<GeneratorsRecord, DecodeError> {
    match version {
        1 => decode_generators_version_1(payload),
        2 => decode_generators_version_2(payload),
        _ => Err(DecodeError::Invalid("unknown generators section version")),
    }
}

fn decode_generators_version_2(payload: &[u8]) -> Result<GeneratorsRecord, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let default_generator = reader.read_identifier()?;
    let assignment_cube_level = reader.read_u8()?;
    let count = reader.read_count(1)?;
    let mut levels = Vec::with_capacity(count);
    let mut generators = Vec::with_capacity(count);
    for _ in 0..count {
        let number = reader.read_varint()?;
        levels
            .push(u8::try_from(number & ((1 << LEVEL_BITS) - 1)).expect("six bits fit in a byte"));
        generators.push(match number >> LEVEL_BITS {
            0 => None,
            plus_one => Some(
                u32::try_from(plus_one - 1)
                    .map_err(|_| DecodeError::Invalid("a generator file id is too large"))?,
            ),
        });
    }
    reader.finish()?;
    if levels.is_empty() {
        return Ok(GeneratorsRecord {
            default_generator,
            assignment_cube_level,
            leaves: Vec::new(),
        });
    }
    let cubes = region_octree::tile_from_levels(&levels)
        .map_err(|_| DecodeError::Invalid("the assignment cubes do not tile the world"))?;
    Ok(GeneratorsRecord {
        default_generator,
        assignment_cube_level,
        leaves: cubes.into_iter().zip(generators).collect(),
    })
}

fn decode_generators_version_1(payload: &[u8]) -> Result<GeneratorsRecord, DecodeError> {
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

/// One level as it is written to a file.
#[derive(Debug)]
pub struct LevelToWrite<'a> {
    pub id: &'a NamespacedId,
    pub generators: GeneratorsRecord,
    /// Sorted by coordinate.
    pub bricks: Vec<(BrickCoordinate, &'a Brick)>,
}

/// One level as it is read from a file.
#[derive(Debug)]
pub struct LevelRecord {
    pub id: NamespacedId,
    pub generators: GeneratorsRecord,
    pub bricks: Vec<(BrickCoordinate, Brick)>,
}

pub fn encode_levels(levels: &[LevelToWrite<'_>]) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    writer.write_varint(levels.len() as u64);
    for level in levels {
        writer.write_identifier(level.id);
        writer.write_bytes(&encode_generators(&level.generators));
        writer.write_bytes(&encode_override_bricks(&level.bricks));
    }
    writer.into_bytes()
}

pub fn decode_levels(payload: &[u8]) -> Result<Vec<LevelRecord>, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let count = reader.read_count(3)?;
    if count == 0 {
        return Err(DecodeError::Invalid("a world has at least one level"));
    }
    let mut levels: Vec<LevelRecord> = Vec::with_capacity(count);
    for _ in 0..count {
        let id = reader.read_identifier()?;
        if levels.iter().any(|level| level.id == id) {
            return Err(DecodeError::Invalid("a level is listed twice"));
        }
        let generators = decode_generators(reader.read_bytes()?, GENERATORS_VERSION)?;
        let bricks = decode_override_bricks(reader.read_bytes()?)?;
        levels.push(LevelRecord {
            id,
            generators,
            bricks,
        });
    }
    reader.finish()?;
    Ok(levels)
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
    use region_octree::RegionOctree;
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

    /// The leaves of a tree in which two cubes are assigned generators.
    fn sample_generators() -> GeneratorsRecord {
        let cube = |x, y, z, level| BrickCube::new(BrickCoordinate::new(x, y, z), level).unwrap();
        let mut tree = RegionOctree::new(None);
        tree.set_cube(cube(-64, 0, 32, 5), Some(0u32));
        tree.set_cube(cube(8, 0, 0, 3), Some(41));
        tree.set_cube(cube(0, 0, 0, 3), Some(0));
        GeneratorsRecord {
            default_generator: id("base_terrain:hills_v1"),
            assignment_cube_level: 3,
            leaves: tree
                .leaves()
                .into_iter()
                .map(|(cube, generator)| (cube, *generator))
                .collect(),
        }
    }

    /// The section as version 1 wrote it, which still has to load.
    fn encode_generators_version_1(record: &GeneratorsRecord) -> Vec<u8> {
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

    #[test]
    fn generator_assignments_round_trip_in_both_versions() {
        let record = sample_generators();
        assert!(
            record
                .leaves
                .iter()
                .any(|(_, generator)| *generator == Some(41))
        );
        assert_eq!(
            decode_generators(&encode_generators(&record), 2),
            Ok(record.clone())
        );
        assert_eq!(
            decode_generators(&encode_generators_version_1(&record), 1),
            Ok(record)
        );
        assert!(reads_generators_version(1) && reads_generators_version(2));
        assert!(!reads_generators_version(0) && !reads_generators_version(3));
        assert!(decode_generators(&[], 3).is_err());
    }

    #[test]
    fn the_new_version_is_several_times_smaller() {
        let record = sample_generators();
        let version_1 = encode_generators_version_1(&record).len();
        let version_2 = encode_generators(&record).len();
        // The identifier and level take the same space in both; the leaves take 1 byte, not about 10.
        assert!(
            version_2 * 3 < version_1 * 2,
            "{version_2} against {version_1}"
        );
        let identifier_bytes = 1 + "base_terrain:hills_v1".len() + 1 + 2;
        assert!(
            version_2 <= identifier_bytes + record.leaves.len() + record.leaves.len() / 8,
            "{version_2} bytes for {} leaves",
            record.leaves.len()
        );
    }

    #[test]
    fn misaligned_or_non_tiling_assignments_are_refused() {
        // Version 1: the first leaf's level made large enough that its origin is not aligned.
        let mut writer = ByteWriter::new();
        writer.write_identifier(&id("base_terrain:hills_v1"));
        writer.write_u8(5);
        writer.write_varint(1);
        for value in [-64i64, 0, 32] {
            writer.write_signed_varint(value);
        }
        writer.write_u8(40);
        writer.write_varint(0);
        assert_eq!(
            decode_generators(&writer.into_bytes(), 1),
            Err(DecodeError::Invalid(
                "an assignment cube is misaligned or too large"
            ))
        );

        // Version 2: levels that do not cover the space exactly.
        let version_2 = |numbers: &[u64]| {
            let mut writer = ByteWriter::new();
            writer.write_identifier(&id("base_terrain:hills_v1"));
            writer.write_u8(3);
            writer.write_varint(numbers.len() as u64);
            for number in numbers {
                writer.write_varint(*number);
            }
            decode_generators(&writer.into_bytes(), 2)
        };
        let refused = Err(DecodeError::Invalid(
            "the assignment cubes do not tile the world",
        ));
        assert_eq!(version_2(&[56]).map(|record| record.leaves.len()), Ok(1));
        assert_eq!(
            version_2(&[]).map(|record| record.leaves.len()),
            Ok(0),
            "a description"
        );
        assert_eq!(version_2(&[55]), refused, "half the space is missing");
        assert_eq!(version_2(&[56, 56]), refused);
        assert_eq!(version_2(&[57]), refused, "bigger than the whole space");
        assert_eq!(version_2(&[63]), refused);
    }

    #[test]
    #[should_panic(expected = "leaves of an octree")]
    fn leaves_that_do_not_tile_the_space_cannot_be_written() {
        let mut record = sample_generators();
        record.leaves.pop();
        encode_generators(&record);
    }

    #[test]
    fn levels_round_trip_in_order_and_refuse_duplicates_and_emptiness() {
        let fixture = StandardTestLayout::default();
        let mut brick = Brick::new(&fixture.layout);
        brick.set(fixture.material, LocalVoxelIndex::new(4, 5, 6), 3);
        let main = id("game:main");
        let nether = id("game:nether");
        let written = encode_levels(&[
            LevelToWrite {
                id: &main,
                generators: sample_generators(),
                bricks: vec![(BrickCoordinate::new(1, 2, 3), &brick)],
            },
            LevelToWrite {
                id: &nether,
                generators: GeneratorsRecord {
                    default_generator: id("game:lava_v1"),
                    assignment_cube_level: 4,
                    leaves: Vec::new(),
                },
                bricks: Vec::new(),
            },
        ]);
        let read = decode_levels(&written).unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!((read[0].id.clone(), read[1].id.clone()), (main, nether));
        assert_eq!(read[0].generators, sample_generators());
        assert_eq!(read[0].bricks, vec![(BrickCoordinate::new(1, 2, 3), brick)]);
        assert_eq!(read[1].generators.default_generator, id("game:lava_v1"));
        assert_eq!(read[1].generators.assignment_cube_level, 4);
        assert!(read[1].bricks.is_empty());

        for length in 0..written.len() {
            assert!(decode_levels(&written[..length]).is_err(), "{length}");
        }
        let mut trailing = written.clone();
        trailing.push(0);
        assert!(decode_levels(&trailing).is_err());

        assert_eq!(
            decode_levels(&encode_levels(&[])).unwrap_err(),
            DecodeError::Invalid("a world has at least one level")
        );
        let twice = id("game:main");
        let level = || LevelToWrite {
            id: &twice,
            generators: sample_generators(),
            bricks: Vec::new(),
        };
        assert_eq!(
            decode_levels(&encode_levels(&[level(), level()])).unwrap_err(),
            DecodeError::Invalid("a level is listed twice")
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
