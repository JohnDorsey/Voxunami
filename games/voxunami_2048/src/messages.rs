//! The game's own messages, carried inside the engine's generic game messages, and the part of its
//! state that is saved with the world.

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use protocol::{Message, read_voxel_coordinate, write_voxel_coordinate};
use voxel_space::{FaceDirection, VoxelCoordinate};
use world_storage::Section;

use crate::tool::Tool;

fn kind(text: &str) -> NamespacedId {
    text.parse().expect("constant identifier is valid")
}

/// A request to push the block at `voxel` toward `direction` with `tool`, which decides what the
/// block does to the blocks it meets. Pulling is pushing toward the puller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PushRequest {
    pub voxel: VoxelCoordinate,
    pub direction: FaceDirection,
    pub tool: Tool,
}

fn read_direction(reader: &mut ByteReader<'_>) -> Result<FaceDirection, DecodeError> {
    FaceDirection::from_index(reader.read_u8()?).ok_or(DecodeError::Invalid("no such direction"))
}

fn read_tool(reader: &mut ByteReader<'_>) -> Result<Tool, DecodeError> {
    Tool::from_index(reader.read_u8()?).ok_or(DecodeError::Invalid("no such tool"))
}

impl Message for PushRequest {
    fn kind() -> NamespacedId {
        kind("voxunami_2048:push_block")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        write_voxel_coordinate(&mut writer, self.voxel);
        writer.write_u8(self.direction.index());
        writer.write_u8(self.tool.index());
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let request = Self {
            voxel: read_voxel_coordinate(&mut reader)?,
            direction: read_direction(&mut reader)?,
            tool: read_tool(&mut reader)?,
        };
        reader.finish()?;
        Ok(request)
    }

    fn samples() -> Vec<Self> {
        let mut samples = vec![Self {
            voxel: VoxelCoordinate::ORIGIN,
            direction: FaceDirection::PositiveY,
            tool: Tool::Add,
        }];
        for (direction, tool) in FaceDirection::ALL
            .into_iter()
            .zip(Tool::ALL.into_iter().cycle())
        {
            samples.push(Self {
                voxel: VoxelCoordinate::new(-5, 100, 7_000_000_000),
                direction,
                tool,
            });
        }
        samples
    }
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

impl Message for Progress {
    fn kind() -> NamespacedId {
        kind("voxunami_2048:progress")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        write_progress(&mut writer, self);
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let progress = read_progress(&mut reader)?;
        reader.finish()?;
        Ok(progress)
    }

    fn samples() -> Vec<Self> {
        vec![
            Self::default(),
            Self {
                score: 123_456,
                pushes: 99,
                largest_merge: 4096,
            },
            Self {
                score: u64::MAX,
                pushes: u64::MAX,
                largest_merge: u32::MAX,
            },
        ]
    }
}

/// A block that was moving when the world was saved. The level is named by identifier, since level
/// numbers belong to one running server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedMover {
    pub level: NamespacedId,
    pub position: VoxelCoordinate,
    pub direction: FaceDirection,
    pub tool: Tool,
    pub steps_left: u32,
}

/// What the server remembers that the voxels do not show: the progress, and which blocks were
/// moving (the blocks themselves are in the voxels).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SavedState {
    pub progress: Progress,
    pub movers: Vec<SavedMover>,
}

/// The version of the saved state this program writes, and the only one it reads: 5, in which the position of a moving block is three wide integers (up to 128
/// bits each). Nothing was released and nothing older is kept readable: a state of an earlier version counts as none, and is kept in the file as it is.
const SAVED_STATE_VERSION: u32 = 5;

pub fn saved_state_section_id() -> NamespacedId {
    kind("voxunami_2048:progress")
}

pub fn saved_state_section(state: &SavedState) -> Section {
    let mut writer = ByteWriter::new();
    write_progress(&mut writer, &state.progress);
    writer.write_varint(state.movers.len() as u64);
    for mover in &state.movers {
        writer.write_identifier(&mover.level);
        write_voxel_coordinate(&mut writer, mover.position);
        writer.write_u8(mover.direction.index());
        writer.write_u8(mover.tool.index());
        writer.write_varint(u64::from(mover.steps_left));
    }
    Section {
        identifier: saved_state_section_id(),
        version: SAVED_STATE_VERSION,
        payload: writer.into_bytes(),
    }
}

fn read_saved_state_payload(payload: &[u8]) -> Result<SavedState, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let progress = read_progress(&mut reader)?;
    // A moving block takes a level's name of at least three bytes, at least three bytes of position, one of direction, one of its tool and one of steps.
    let count = reader.read_count(9)?;
    let mut movers = Vec::with_capacity(count);
    for _ in 0..count {
        movers.push(SavedMover {
            level: reader.read_identifier()?,
            position: read_voxel_coordinate(&mut reader)?,
            direction: read_direction(&mut reader)?,
            tool: read_tool(&mut reader)?,
            steps_left: u32::try_from(reader.read_varint()?)
                .map_err(|_| DecodeError::Invalid("a block has too many steps left"))?,
        });
    }
    reader.finish()?;
    Ok(SavedState { progress, movers })
}

/// The saved state in a world's sections, if the world has some this program can read. A section of another version (a newer program's, or an older one's) is left
/// alone (and so kept in the file) and counts as no state.
pub fn read_saved_state(sections: &[Section]) -> Option<SavedState> {
    let section = sections.iter().find(|section| {
        section.identifier == saved_state_section_id() && section.version == SAVED_STATE_VERSION
    })?;
    read_saved_state_payload(&section.payload).ok()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn level(name: &str) -> NamespacedId {
        format!("voxunami_2048:{name}").parse().unwrap()
    }

    fn mover(x: i128, direction: FaceDirection, steps_left: u32) -> SavedMover {
        SavedMover {
            level: level("landscape"),
            position: VoxelCoordinate::new(x, -40, 7),
            direction,
            tool: Tool::Subtract,
            steps_left,
        }
    }

    #[test]
    fn a_push_names_a_voxel_a_direction_and_a_tool_and_nothing_else_is_accepted() {
        for direction in FaceDirection::ALL {
            for tool in Tool::ALL {
                let request = PushRequest {
                    voxel: VoxelCoordinate::new(-5, 100, 7_000_000_000),
                    direction,
                    tool,
                };
                assert_eq!(PushRequest::decode(&request.encode()), Ok(request));
            }
        }
        let good = PushRequest {
            voxel: VoxelCoordinate::ORIGIN,
            direction: FaceDirection::PositiveY,
            tool: Tool::Add,
        }
        .encode();
        for length in 0..good.len() {
            assert!(PushRequest::decode(&good[..length]).is_err(), "{length}");
        }
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(PushRequest::decode(&trailing).is_err());
        let mut bad_direction = good.clone();
        let length = bad_direction.len();
        bad_direction[length - 2] = 6;
        assert!(PushRequest::decode(&bad_direction).is_err());
        let mut bad_tool = good;
        *bad_tool.last_mut().unwrap() = 3;
        assert!(PushRequest::decode(&bad_tool).is_err());
        // A coordinate beyond the world's edge.
        let mut writer = ByteWriter::new();
        writer.write_wide_signed(voxel_space::VOXEL_COORDINATE_MAXIMUM + 1);
        writer.write_wide_signed(0);
        writer.write_wide_signed(0);
        writer.write_u8(0);
        writer.write_u8(0);
        assert!(PushRequest::decode(&writer.into_bytes()).is_err());
    }

    #[test]
    fn the_declared_messages_pass_the_generated_check() {
        assert_eq!(protocol::check_message::<PushRequest>(), Ok(()));
        assert_eq!(protocol::check_message::<Progress>(), Ok(()));
    }

    #[test]
    fn progress_survives_encoding_and_rejects_damage() {
        let progress = Progress {
            score: 123_456,
            pushes: 300,
            largest_merge: 3_000_000_001,
        };
        assert_eq!(Progress::decode(&progress.encode()), Ok(progress));
        let good = progress.encode();
        for length in 0..good.len() {
            assert!(Progress::decode(&good[..length]).is_err());
        }
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(Progress::decode(&trailing).is_err());
        let mut too_large = ByteWriter::new();
        too_large.write_varint(1);
        too_large.write_varint(1);
        too_large.write_varint(u64::from(u32::MAX) + 1);
        assert!(Progress::decode(&too_large.into_bytes()).is_err());
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
                mover(1 << 100, FaceDirection::PositiveY, 2),
            ],
        };
        let section = saved_state_section(&state);
        assert_eq!(
            read_saved_state(std::slice::from_ref(&section)),
            Some(state)
        );
        assert_eq!(read_saved_state(&[]), None);
        let newer = Section {
            version: SAVED_STATE_VERSION + 1,
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
    fn a_saved_state_of_any_other_version_is_not_read() {
        let state = SavedState {
            progress: Progress {
                score: 1,
                pushes: 2,
                largest_merge: 4,
            },
            movers: vec![mover(3, FaceDirection::PositiveX, 40)],
        };
        let section = saved_state_section(&state);
        for version in [0, 1, 2, 3, 4, SAVED_STATE_VERSION + 1] {
            let other = Section {
                version,
                ..section.clone()
            };
            assert_eq!(read_saved_state(&[other]), None, "version {version}");
        }
    }

    #[test]
    fn a_saved_state_with_no_movers_is_small_and_reads_back() {
        let state = SavedState::default();
        let section = saved_state_section(&state);
        assert_eq!(section.payload.len(), 4);
        assert_eq!(section.version, SAVED_STATE_VERSION);
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
        bad_direction.payload[length - 3] = 9;
        assert_eq!(read_saved_state(&[bad_direction]), None);
        let mut bad_tool = good.clone();
        bad_tool.payload[length - 2] = 7;
        assert_eq!(read_saved_state(&[bad_tool]), None);
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
            let _ = PushRequest::decode(&bytes);
            let _ = Progress::decode(&bytes);
            let _ = read_saved_state(&[Section {
                identifier: saved_state_section_id(),
                version: 1,
                payload: bytes,
            }]);
        }
    }
}
