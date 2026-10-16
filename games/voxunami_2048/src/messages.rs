//! The game's own messages, carried inside the engine's generic game messages, and the part of its
//! state that is saved with the world.

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use voxel_space::{FaceDirection, VoxelCoordinate};
use world_storage::Section;

fn kind(text: &str) -> NamespacedId {
    text.parse().expect("constant identifier is valid")
}

/// Client to server: push the block at a voxel.
pub fn push_kind() -> NamespacedId {
    kind("voxunami_2048:push_block")
}

/// Server to everyone: how the game has gone so far.
pub fn progress_kind() -> NamespacedId {
    kind("voxunami_2048:progress")
}

/// A request to push the block at `voxel` toward `direction`. Pulling is pushing toward the puller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PushRequest {
    pub voxel: VoxelCoordinate,
    pub direction: FaceDirection,
}

fn write_voxel(writer: &mut ByteWriter, voxel: VoxelCoordinate) {
    for component in [voxel.x(), voxel.y(), voxel.z()] {
        writer.write_signed_varint(component);
    }
}

fn read_voxel(reader: &mut ByteReader<'_>) -> Result<VoxelCoordinate, DecodeError> {
    VoxelCoordinate::try_new(
        reader.read_signed_varint()?,
        reader.read_signed_varint()?,
        reader.read_signed_varint()?,
    )
    .map_err(|_| DecodeError::Invalid("a voxel coordinate is out of range"))
}

fn read_direction(reader: &mut ByteReader<'_>) -> Result<FaceDirection, DecodeError> {
    FaceDirection::from_index(reader.read_u8()?).ok_or(DecodeError::Invalid("no such direction"))
}

pub fn encode_push(request: &PushRequest) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    write_voxel(&mut writer, request.voxel);
    writer.write_u8(request.direction.index());
    writer.into_bytes()
}

pub fn decode_push(payload: &[u8]) -> Result<PushRequest, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let request = PushRequest {
        voxel: read_voxel(&mut reader)?,
        direction: read_direction(&mut reader)?,
    };
    reader.finish()?;
    Ok(request)
}

/// How the game has gone, for everyone who plays it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// The sum of the numbers of every block that merging has made.
    pub score: u64,
    /// How many blocks players have pushed.
    pub pushes: u64,
    /// The number of the largest block merging has made; 0 before the first merge.
    pub largest_merge: u32,
}

fn write_progress(writer: &mut ByteWriter, progress: &Progress) {
    writer.write_varint(progress.score);
    writer.write_varint(progress.pushes);
    writer.write_varint(u64::from(progress.largest_merge));
}

fn read_progress(reader: &mut ByteReader<'_>) -> Result<Progress, DecodeError> {
    Ok(Progress {
        score: reader.read_varint()?,
        pushes: reader.read_varint()?,
        largest_merge: u32::try_from(reader.read_varint()?)
            .map_err(|_| DecodeError::Invalid("a block is numbered beyond the largest"))?,
    })
}

/// The progress as saved before numbers were unbounded: the largest merge was the exponent of a power
/// of two, in a byte.
fn read_progress_with_exponent(reader: &mut ByteReader<'_>) -> Result<Progress, DecodeError> {
    let score = reader.read_varint()?;
    let pushes = reader.read_varint()?;
    let exponent = reader.read_u8()?;
    let largest_merge = match exponent {
        0 => 0,
        1..=31 => 1 << exponent,
        _ => {
            return Err(DecodeError::Invalid(
                "a block is numbered beyond the largest",
            ));
        }
    };
    Ok(Progress {
        score,
        pushes,
        largest_merge,
    })
}

pub fn encode_progress(progress: &Progress) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    write_progress(&mut writer, progress);
    writer.into_bytes()
}

pub fn decode_progress(payload: &[u8]) -> Result<Progress, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let progress = read_progress(&mut reader)?;
    reader.finish()?;
    Ok(progress)
}

/// A block that was moving when the world was saved. The level is named by identifier, since level
/// numbers belong to one running server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedMover {
    pub level: NamespacedId,
    pub position: VoxelCoordinate,
    pub direction: FaceDirection,
    pub steps_left: u32,
}

/// What the server remembers that the voxels do not show: the progress, and which blocks were
/// moving (the blocks themselves are in the voxels).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SavedState {
    pub progress: Progress,
    pub movers: Vec<SavedMover>,
}

/// The version of the saved state this program writes. Version 1, from before worlds had levels,
/// did not say which level a moving block was in; versions 1 and 2 kept the largest merge as the
/// exponent of a power of two. Both are still read.
const SAVED_STATE_VERSION: u32 = 3;

pub fn saved_state_section_id() -> NamespacedId {
    kind("voxunami_2048:progress")
}

pub fn saved_state_section(state: &SavedState) -> Section {
    let mut writer = ByteWriter::new();
    write_progress(&mut writer, &state.progress);
    writer.write_varint(state.movers.len() as u64);
    for mover in &state.movers {
        writer.write_identifier(&mover.level);
        write_voxel(&mut writer, mover.position);
        writer.write_u8(mover.direction.index());
        writer.write_varint(u64::from(mover.steps_left));
    }
    Section {
        identifier: saved_state_section_id(),
        version: SAVED_STATE_VERSION,
        payload: writer.into_bytes(),
    }
}

fn read_saved_state_payload(
    payload: &[u8],
    version: u32,
    legacy_level: &NamespacedId,
) -> Result<SavedState, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let progress = if version <= 2 {
        read_progress_with_exponent(&mut reader)?
    } else {
        read_progress(&mut reader)?
    };
    // A moving block takes at least three bytes of position, one of direction, one of steps, and
    // from version 2 a level's name of at least three.
    let count = reader.read_count(if version == 1 { 5 } else { 8 })?;
    let mut movers = Vec::with_capacity(count);
    for _ in 0..count {
        let level = if version == 1 {
            legacy_level.clone()
        } else {
            reader.read_identifier()?
        };
        movers.push(SavedMover {
            level,
            position: read_voxel(&mut reader)?,
            direction: read_direction(&mut reader)?,
            steps_left: u32::try_from(reader.read_varint()?)
                .map_err(|_| DecodeError::Invalid("a block has too many steps left"))?,
        });
    }
    reader.finish()?;
    Ok(SavedState { progress, movers })
}

/// The saved state in a world's sections, if the world has some this program can read. A newer
/// version of the section is left alone (and so kept in the file) and counts as no state.
/// `legacy_level` is the level that blocks saved before worlds had levels were in.
pub fn read_saved_state(sections: &[Section], legacy_level: &NamespacedId) -> Option<SavedState> {
    let section = sections.iter().find(|section| {
        section.identifier == saved_state_section_id()
            && (1..=SAVED_STATE_VERSION).contains(&section.version)
    })?;
    read_saved_state_payload(&section.payload, section.version, legacy_level).ok()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn level(name: &str) -> NamespacedId {
        format!("voxunami_2048:{name}").parse().unwrap()
    }

    fn mover(x: i64, direction: FaceDirection, steps_left: u32) -> SavedMover {
        SavedMover {
            level: level("landscape"),
            position: VoxelCoordinate::new(x, -40, 7),
            direction,
            steps_left,
        }
    }

    #[test]
    fn a_push_names_a_voxel_and_a_direction_and_nothing_else_is_accepted() {
        for direction in FaceDirection::ALL {
            let request = PushRequest {
                voxel: VoxelCoordinate::new(-5, 100, 7_000_000_000),
                direction,
            };
            assert_eq!(decode_push(&encode_push(&request)), Ok(request));
        }
        let good = encode_push(&PushRequest {
            voxel: VoxelCoordinate::ORIGIN,
            direction: FaceDirection::PositiveY,
        });
        for length in 0..good.len() {
            assert!(decode_push(&good[..length]).is_err(), "{length}");
        }
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(decode_push(&trailing).is_err());
        let mut bad_direction = good;
        *bad_direction.last_mut().unwrap() = 6;
        assert!(decode_push(&bad_direction).is_err());
        // A coordinate beyond the world's edge.
        let mut writer = ByteWriter::new();
        writer.write_signed_varint(i64::MAX);
        writer.write_signed_varint(0);
        writer.write_signed_varint(0);
        writer.write_u8(0);
        assert!(decode_push(&writer.into_bytes()).is_err());
    }

    #[test]
    fn progress_survives_encoding_and_rejects_damage() {
        let progress = Progress {
            score: 123_456,
            pushes: 300,
            largest_merge: 3_000_000_001,
        };
        assert_eq!(decode_progress(&encode_progress(&progress)), Ok(progress));
        let good = encode_progress(&progress);
        for length in 0..good.len() {
            assert!(decode_progress(&good[..length]).is_err());
        }
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(decode_progress(&trailing).is_err());
        let mut too_large = ByteWriter::new();
        too_large.write_varint(1);
        too_large.write_varint(1);
        too_large.write_varint(u64::from(u32::MAX) + 1);
        assert!(decode_progress(&too_large.into_bytes()).is_err());
    }

    #[test]
    fn saved_state_round_trips_and_ignores_what_it_cannot_read() {
        let state = SavedState {
            progress: Progress {
                score: 99,
                pushes: 7,
                largest_merge: 16,
            },
            movers: vec![
                mover(3, FaceDirection::PositiveX, 40),
                mover(-1_000_000_000_000, FaceDirection::NegativeZ, 1),
            ],
        };
        let section = saved_state_section(&state);
        assert_eq!(
            read_saved_state(std::slice::from_ref(&section), &level("landscape")),
            Some(state)
        );
        assert_eq!(read_saved_state(&[], &level("landscape")), None);
        let newer = Section {
            version: SAVED_STATE_VERSION + 1,
            ..section.clone()
        };
        assert_eq!(read_saved_state(&[newer], &level("landscape")), None);
        let damaged = Section {
            payload: vec![0x80],
            ..section
        };
        assert_eq!(read_saved_state(&[damaged], &level("landscape")), None);
    }

    #[test]
    fn a_version_1_saved_state_has_movers_with_no_level_and_gives_them_the_one_asked_for() {
        let mut writer = ByteWriter::new();
        writer.write_varint(40); // score
        writer.write_varint(3); // pushes
        writer.write_u8(2); // largest merge
        writer.write_varint(1); // one mover
        write_voxel(&mut writer, VoxelCoordinate::new(5, 6, 7));
        writer.write_u8(FaceDirection::NegativeX.index());
        writer.write_varint(9);
        let section = Section {
            identifier: saved_state_section_id(),
            version: 1,
            payload: writer.into_bytes(),
        };
        let state = read_saved_state(&[section], &level("old")).unwrap();
        assert_eq!(state.progress.score, 40);
        assert_eq!(
            state.progress.largest_merge, 4,
            "an exponent of 2 is the number 4"
        );
        assert_eq!(
            state.movers,
            [SavedMover {
                level: level("old"),
                position: VoxelCoordinate::new(5, 6, 7),
                direction: FaceDirection::NegativeX,
                steps_left: 9,
            }]
        );
    }

    #[test]
    fn a_version_2_saved_state_has_the_largest_merge_as_an_exponent_and_it_becomes_a_number() {
        let version_2 = |exponent: u8| {
            let mut writer = ByteWriter::new();
            writer.write_varint(500); // score
            writer.write_varint(9); // pushes
            writer.write_u8(exponent); // largest merge
            writer.write_varint(1); // one mover
            writer.write_identifier(&level("landscape"));
            write_voxel(&mut writer, VoxelCoordinate::new(5, 6, 7));
            writer.write_u8(FaceDirection::NegativeX.index());
            writer.write_varint(9);
            Section {
                identifier: saved_state_section_id(),
                version: 2,
                payload: writer.into_bytes(),
            }
        };
        for (exponent, number) in [(0, 0), (1, 2), (11, 2048), (20, 1 << 20), (31, 1 << 31)] {
            let state = read_saved_state(&[version_2(exponent)], &level("landscape")).unwrap();
            assert_eq!(state.progress.largest_merge, number, "exponent {exponent}");
            assert_eq!(state.progress.score, 500);
            assert_eq!(state.movers.len(), 1);
        }
        // An exponent whose number does not fit is damage, not a number.
        assert_eq!(
            read_saved_state(&[version_2(32)], &level("landscape")),
            None
        );
    }

    #[test]
    fn a_saved_state_with_no_movers_is_small_and_reads_back() {
        let state = SavedState::default();
        let section = saved_state_section(&state);
        assert_eq!(section.payload.len(), 4);
        assert_eq!(section.version, SAVED_STATE_VERSION);
        assert_eq!(
            read_saved_state(&[section], &level("landscape")),
            Some(state)
        );
    }

    #[test]
    fn a_saved_mover_with_a_wrong_direction_or_too_many_steps_is_unreadable() {
        let good = saved_state_section(&SavedState {
            progress: Progress::default(),
            movers: vec![mover(0, FaceDirection::PositiveX, 5)],
        });
        let length = good.payload.len();
        let mut bad_direction = good.clone();
        bad_direction.payload[length - 2] = 9;
        assert_eq!(
            read_saved_state(&[bad_direction], &level("landscape")),
            None
        );
        let mut too_many = good.payload[..length - 1].to_vec();
        too_many.extend([0xFF, 0xFF, 0xFF, 0xFF, 0x1F]); // 2^33 - 1 steps
        assert_eq!(
            read_saved_state(
                &[Section {
                    payload: too_many,
                    ..good
                }],
                &level("landscape")
            ),
            None
        );
    }

    proptest! {
        #[test]
        fn decoding_arbitrary_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..60)) {
            let _ = decode_push(&bytes);
            let _ = decode_progress(&bytes);
            let _ = read_saved_state(&[Section {
                identifier: saved_state_section_id(),
                version: 1,
                payload: bytes,
            }], &level("landscape"));
        }
    }
}
