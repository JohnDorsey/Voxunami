//! The game's own messages, carried inside the engine's generic game messages, and the part of its
//! state that is saved with the world.

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use voxel_space::FaceDirection;
use world_storage::Section;

fn kind(text: &str) -> NamespacedId {
    text.parse().expect("constant identifier is valid")
}

/// Client to server: push every tile toward a side. The body is the side's face index.
pub fn push_kind() -> NamespacedId {
    kind("voxunami_2048:push")
}

/// Client to server: start a new game. No body.
pub fn new_game_kind() -> NamespacedId {
    kind("voxunami_2048:new_game")
}

/// Server to everyone: the score and whether the game goes on.
pub fn status_kind() -> NamespacedId {
    kind("voxunami_2048:status")
}

pub fn encode_push(toward: FaceDirection) -> Vec<u8> {
    vec![toward.index()]
}

pub fn decode_push(payload: &[u8]) -> Result<FaceDirection, DecodeError> {
    let [index] = payload else {
        return Err(DecodeError::Invalid("a push is one byte"));
    };
    FaceDirection::from_index(*index).ok_or(DecodeError::Invalid("no such direction"))
}

/// How the game stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub score: u64,
    pub moves: u64,
    /// The exponent of the largest tile.
    pub highest: u8,
    /// Whether no push can change the board.
    pub game_over: bool,
    /// How many games have been started in this world, counting from 0.
    pub game_number: u64,
}

impl Status {
    /// Whether a 2048 tile has been made.
    pub fn won(&self) -> bool {
        self.highest >= crate::board::WINNING_EXPONENT
    }
}

pub fn encode_status(status: &Status) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    writer.write_varint(status.score);
    writer.write_varint(status.moves);
    writer.write_u8(status.highest);
    writer.write_u8(u8::from(status.game_over));
    writer.write_varint(status.game_number);
    writer.into_bytes()
}

pub fn decode_status(payload: &[u8]) -> Result<Status, DecodeError> {
    let mut reader = ByteReader::new(payload);
    let status = Status {
        score: reader.read_varint()?,
        moves: reader.read_varint()?,
        highest: reader.read_u8()?,
        game_over: match reader.read_u8()? {
            0 => false,
            1 => true,
            _ => return Err(DecodeError::Invalid("a flag is 0 or 1")),
        },
        game_number: reader.read_varint()?,
    };
    reader.finish()?;
    Ok(status)
}

/// What the server remembers that the voxels do not show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SavedState {
    pub score: u64,
    pub moves: u64,
    pub game_number: u64,
}

pub fn saved_state_section_id() -> NamespacedId {
    kind("voxunami_2048:state")
}

pub fn saved_state_section(state: &SavedState) -> Section {
    let mut writer = ByteWriter::new();
    writer.write_varint(state.score);
    writer.write_varint(state.moves);
    writer.write_varint(state.game_number);
    Section {
        identifier: saved_state_section_id(),
        version: 1,
        payload: writer.into_bytes(),
    }
}

/// The saved state in a world's sections, if the world has some this program can read. A newer
/// version of the section is left alone (and so kept in the file) and counts as no state.
pub fn read_saved_state(sections: &[Section]) -> Option<SavedState> {
    let section = sections
        .iter()
        .find(|section| section.identifier == saved_state_section_id() && section.version == 1)?;
    let mut reader = ByteReader::new(&section.payload);
    let state = SavedState {
        score: reader.read_varint().ok()?,
        moves: reader.read_varint().ok()?,
        game_number: reader.read_varint().ok()?,
    };
    reader.finish().ok()?;
    Some(state)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn pushes_name_a_direction_and_nothing_else_is_accepted() {
        for toward in FaceDirection::ALL {
            assert_eq!(decode_push(&encode_push(toward)), Ok(toward));
        }
        assert!(decode_push(&[]).is_err());
        assert!(decode_push(&[6]).is_err());
        assert!(decode_push(&[0, 0]).is_err());
    }

    #[test]
    fn status_survives_encoding_and_rejects_damage() {
        let status = Status {
            score: 123_456,
            moves: 300,
            highest: 11,
            game_over: true,
            game_number: 2,
        };
        assert_eq!(decode_status(&encode_status(&status)), Ok(status));
        assert!(status.won());
        assert!(!Status::default().won());
        let good = encode_status(&status);
        for length in 0..good.len() {
            assert!(decode_status(&good[..length]).is_err());
        }
        let mut bad_flag = good.clone();
        let flag_position = good.len() - 2;
        bad_flag[flag_position] = 7;
        assert!(decode_status(&bad_flag).is_err());
        let mut trailing = good;
        trailing.push(0);
        assert!(decode_status(&trailing).is_err());
    }

    #[test]
    fn saved_state_round_trips_and_ignores_what_it_cannot_read() {
        let state = SavedState {
            score: 99,
            moves: 7,
            game_number: 3,
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

    proptest! {
        #[test]
        fn decoding_arbitrary_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..40)) {
            let _ = decode_push(&bytes);
            let _ = decode_status(&bytes);
            let _ = read_saved_state(&[Section {
                identifier: saved_state_section_id(),
                version: 1,
                payload: bytes,
            }]);
        }
    }
}
