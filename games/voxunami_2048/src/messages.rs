//! The game's own messages, carried inside the engine's generic game messages, and the part of its
//! state that is saved with the world.

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use voxel_space::{FaceDirection, VoxelCoordinate};
use world_storage::Section;

use crate::motion::Mover;

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
    /// The exponent of the largest block merging has made; 0 before the first merge.
    pub largest_merge: u8,
}

impl Progress {
    /// The number of the largest block merging has made, if there has been a merge.
    pub fn largest_merged_value(&self) -> Option<u64> {
        (self.largest_merge > 0).then(|| 1 << self.largest_merge)
    }
}

fn write_progress(writer: &mut ByteWriter, progress: &Progress) {
    writer.write_varint(progress.score);
    writer.write_varint(progress.pushes);
    writer.write_u8(progress.largest_merge);
}

fn read_progress(reader: &mut ByteReader<'_>) -> Result<Progress, DecodeError> {
    let progress = Progress {
        score: reader.read_varint()?,
        pushes: reader.read_varint()?,
        largest_merge: reader.read_u8()?,
    };
    if progress.largest_merge > crate::numbered_block::MAXIMUM_EXPONENT {
        return Err(DecodeError::Invalid(
            "a block is numbered beyond the largest",
        ));
    }
    Ok(progress)
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

/// What the server remembers that the voxels do not show: the progress, and which blocks were
/// moving (the blocks themselves are in the voxels).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SavedState {
    pub progress: Progress,
    pub movers: Vec<Mover>,
}

pub fn saved_state_section_id() -> NamespacedId {
    kind("voxunami_2048:progress")
}

pub fn saved_state_section(state: &SavedState) -> Section {
    let mut writer = ByteWriter::new();
    write_progress(&mut writer, &state.progress);
    writer.write_varint(state.movers.len() as u64);
    for mover in &state.movers {
        write_voxel(&mut writer, mover.position);
        writer.write_u8(mover.direction.index());
        writer.write_varint(u64::from(mover.steps_left));
    }
    Section {
        identifier: saved_state_section_id(),
        version: 1,
        payload: writer.into_bytes(),
    }
}

fn read_saved_state_payload(payload: &[u8]) -> Result<SavedState, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let progress = read_progress(&mut reader)?;
    // A moving block takes at least three bytes of position, one of direction, one of steps.
    let count = reader.read_count(5)?;
    let mut movers = Vec::with_capacity(count);
    for _ in 0..count {
        movers.push(Mover {
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
pub fn read_saved_state(sections: &[Section]) -> Option<SavedState> {
    let section = sections
        .iter()
        .find(|section| section.identifier == saved_state_section_id() && section.version == 1)?;
    read_saved_state_payload(&section.payload).ok()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn mover(x: i64, direction: FaceDirection, steps_left: u32) -> Mover {
        Mover {
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
            largest_merge: 11,
        };
        assert_eq!(decode_progress(&encode_progress(&progress)), Ok(progress));
        assert_eq!(progress.largest_merged_value(), Some(2048));
        assert_eq!(Progress::default().largest_merged_value(), None);
        let good = encode_progress(&progress);
        for length in 0..good.len() {
            assert!(decode_progress(&good[..length]).is_err());
        }
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(decode_progress(&trailing).is_err());
        let mut too_large = good;
        *too_large.last_mut().unwrap() = crate::numbered_block::MAXIMUM_EXPONENT + 1;
        assert!(decode_progress(&too_large).is_err());
    }

    #[test]
    fn saved_state_round_trips_and_ignores_what_it_cannot_read() {
        let state = SavedState {
            progress: Progress {
                score: 99,
                pushes: 7,
                largest_merge: 4,
            },
            movers: vec![
                mover(3, FaceDirection::PositiveX, 40),
                mover(-1_000_000_000_000, FaceDirection::NegativeZ, 1),
            ],
        };
        let section = saved_state_section(&state);
        assert_eq!(
            read_saved_state(std::slice::from_ref(&section)),
            Some(state)
        );
        assert_eq!(read_saved_state(&[]), None);
        let newer = Section {
            version: 2,
            ..section.clone()
        };
        assert_eq!(read_saved_state(&[newer]), None);
        let damaged = Section {
            payload: vec![0x80],
            ..section
        };
        assert_eq!(read_saved_state(&[damaged]), None);
    }

    #[test]
    fn a_saved_state_with_no_movers_is_small_and_reads_back() {
        let state = SavedState::default();
        let section = saved_state_section(&state);
        assert_eq!(section.payload.len(), 4);
        assert_eq!(read_saved_state(&[section]), Some(state));
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
        assert_eq!(read_saved_state(&[bad_direction]), None);
        let mut too_many = good.payload[..length - 1].to_vec();
        too_many.extend([0xFF, 0xFF, 0xFF, 0xFF, 0x1F]); // 2^33 - 1 steps
        assert_eq!(
            read_saved_state(&[Section {
                payload: too_many,
                ..good
            }]),
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
            }]);
        }
    }
}
