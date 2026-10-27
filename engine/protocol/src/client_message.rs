use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use voxel_space::{BrickBox, LevelIndex};

use crate::MAXIMUM_COMMAND_LENGTH;
use crate::error::ProtocolError;
use crate::types::PlayerKey;
use crate::wire::{read_brick_box, read_level, write_brick_box, write_level};

const KIND_HELLO: u8 = 1;
const KIND_SET_INTEREST: u8 = 2;
const KIND_GAME_MESSAGE: u8 = 3;
const KIND_GOODBYE: u8 = 4;
const KIND_COMMAND: u8 = 5;
const KIND_PROVE: u8 = 6;

/// Sent from a client to the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientMessage {
    /// The first message on a connection.
    Hello {
        protocol_version: u32,
        /// Which game the client is running, such as `voxunami_cave:game`. A server refuses clients
        /// of another game.
        game: NamespacedId,
        /// Who the player is. The server answers with a [`ServerMessage::Challenge`](crate::ServerMessage::Challenge)
        /// and does not go on until the player has proved they hold the private key.
        player_key: PlayerKey,
    },
    /// The answer to a challenge: the signature, by the key in the `Hello`, of the login the challenge
    /// names (see the `identity` crate for what is signed).
    Prove { signature: [u8; 64] },
    /// The region of bricks of one level the client wants to watch. The server sends what is in it,
    /// and keeps the client up to date on it, until the next `SetInterest`.
    SetInterest { level: LevelIndex, region: BrickBox },
    /// A message for the game or one of its plugins.
    GameMessage {
        kind: NamespacedId,
        payload: Vec<u8>,
    },
    /// The client is leaving. Closing the connection means the same.
    Goodbye,
    /// A line of text the player typed as a command, such as `/level go nether`. At most
    /// [`MAXIMUM_COMMAND_LENGTH`] bytes. The game's server logic decides what it means and answers
    /// with a `CommandReply`.
    Command { text: String },
}

impl ClientMessage {
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        match self {
            Self::Hello {
                protocol_version,
                game,
                player_key,
            } => {
                writer.write_u8(KIND_HELLO);
                writer.write_u32(*protocol_version);
                writer.write_identifier(game);
                writer.write_raw(player_key.bytes());
            }
            Self::Prove { signature } => {
                writer.write_u8(KIND_PROVE);
                writer.write_raw(signature);
            }
            Self::SetInterest { level, region } => {
                writer.write_u8(KIND_SET_INTEREST);
                write_level(&mut writer, *level);
                write_brick_box(&mut writer, *region);
            }
            Self::GameMessage { kind, payload } => {
                writer.write_u8(KIND_GAME_MESSAGE);
                writer.write_identifier(kind);
                writer.write_bytes(payload);
            }
            Self::Goodbye => writer.write_u8(KIND_GOODBYE),
            Self::Command { text } => {
                writer.write_u8(KIND_COMMAND);
                writer.write_string(text);
            }
        }
        writer.into_bytes()
    }

    pub fn decode(frame: &[u8]) -> Result<Self, ProtocolError> {
        let mut reader = ByteReader::new(frame);
        let message = match reader.read_u8().map_err(|_| ProtocolError::EmptyFrame)? {
            KIND_HELLO => Self::Hello {
                protocol_version: reader.read_u32()?,
                game: reader.read_identifier()?,
                player_key: PlayerKey::new(
                    reader.read_raw(32)?.try_into().expect("32 bytes were read"),
                ),
            },
            KIND_PROVE => Self::Prove {
                signature: reader.read_raw(64)?.try_into().expect("64 bytes were read"),
            },
            KIND_SET_INTEREST => Self::SetInterest {
                level: read_level(&mut reader)?,
                region: read_brick_box(&mut reader)?,
            },
            KIND_GAME_MESSAGE => Self::GameMessage {
                kind: reader.read_identifier()?,
                payload: reader.read_bytes()?.to_vec(),
            },
            KIND_GOODBYE => Self::Goodbye,
            KIND_COMMAND => {
                let text = reader.read_string()?;
                if text.len() > MAXIMUM_COMMAND_LENGTH {
                    return Err(DecodeError::Invalid("a command is too long").into());
                }
                Self::Command {
                    text: text.to_owned(),
                }
            }
            unknown => return Err(ProtocolError::UnknownMessageKind(unknown)),
        };
        reader.finish()?;
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use voxel_space::BrickCoordinate;

    use super::*;

    fn id(text: &str) -> NamespacedId {
        text.parse().unwrap()
    }

    fn samples() -> Vec<ClientMessage> {
        vec![
            ClientMessage::Hello {
                protocol_version: 7,
                game: id("voxunami_cave:game"),
                player_key: PlayerKey::new([9; 32]),
            },
            ClientMessage::Prove {
                signature: [200; 64],
            },
            ClientMessage::SetInterest {
                level: LevelIndex::new(2),
                region: BrickBox::around(BrickCoordinate::new(-3, 0, 12), 4),
            },
            ClientMessage::GameMessage {
                kind: id("voxunami_cave:place_block"),
                payload: vec![1, 2, 3, 255],
            },
            ClientMessage::GameMessage {
                kind: id("x:y"),
                payload: Vec::new(),
            },
            ClientMessage::Goodbye,
            ClientMessage::Command {
                text: "/level go nether".to_owned(),
            },
            ClientMessage::Command {
                text: String::new(),
            },
        ]
    }

    #[test]
    fn every_message_survives_encoding() {
        for message in samples() {
            assert_eq!(ClientMessage::decode(&message.encode()), Ok(message));
        }
    }

    #[test]
    fn the_wire_layout_is_pinned() {
        // Changing this layout needs a new protocol version.
        assert_eq!(ClientMessage::Goodbye.encode(), [4]);
        let mut hello = vec![1, 1, 0, 0, 0, 3, b'a', b':', b'b'];
        hello.extend([7; 32]);
        assert_eq!(
            ClientMessage::Hello {
                protocol_version: 1,
                game: id("a:b"),
                player_key: PlayerKey::new([7; 32]),
            }
            .encode(),
            hello
        );
        let mut prove = vec![6];
        prove.extend([5; 64]);
        assert_eq!(ClientMessage::Prove { signature: [5; 64] }.encode(), prove);
        assert_eq!(
            ClientMessage::SetInterest {
                level: LevelIndex::new(3),
                region: BrickBox::new(
                    BrickCoordinate::new(-1, 0, 2),
                    BrickCoordinate::new(3, 4, 5)
                )
                .unwrap()
            }
            .encode(),
            [2, 3, 1, 1, 0, 1, 4, 1, 6, 1, 8, 1, 10]
        );
        assert_eq!(
            ClientMessage::Command {
                text: "/x".to_owned()
            }
            .encode(),
            [5, 2, b'/', b'x']
        );
        assert_eq!(
            ClientMessage::GameMessage {
                kind: id("a:b"),
                payload: vec![9]
            }
            .encode(),
            [3, 3, b'a', b':', b'b', 1, 9]
        );
    }

    #[test]
    fn damaged_frames_are_errors_not_panics() {
        assert_eq!(ClientMessage::decode(&[]), Err(ProtocolError::EmptyFrame));
        assert_eq!(
            ClientMessage::decode(&[99]),
            Err(ProtocolError::UnknownMessageKind(99))
        );
        let mut trailing = ClientMessage::Goodbye.encode();
        trailing.push(0);
        assert_eq!(
            ClientMessage::decode(&trailing),
            Err(ProtocolError::Malformed(DecodeError::TrailingBytes(1)))
        );
        // A box whose corners are the wrong way round.
        assert!(ClientMessage::decode(&[2, 0, 6, 0, 0, 0, 0, 0]).is_err());
        // A level number too large for a level, and a command that is too long or not text.
        assert!(
            ClientMessage::decode(&[2, 0xFF, 0xFF, 0xFF, 0xFF, 0x1F, 0, 0, 0, 0, 0, 0]).is_err()
        );
        let longest = ClientMessage::Command {
            text: "a".repeat(MAXIMUM_COMMAND_LENGTH),
        };
        assert_eq!(ClientMessage::decode(&longest.encode()), Ok(longest));
        let too_long = ClientMessage::Command {
            text: "a".repeat(MAXIMUM_COMMAND_LENGTH + 1),
        };
        assert_eq!(
            ClientMessage::decode(&too_long.encode()),
            Err(ProtocolError::Malformed(DecodeError::Invalid(
                "a command is too long"
            )))
        );
        assert!(ClientMessage::decode(&[5, 1, 0xFF]).is_err());
        for message in samples() {
            let frame = message.encode();
            for length in 0..frame.len() {
                assert!(
                    ClientMessage::decode(&frame[..length]).is_err(),
                    "{message:?} truncated to {length}"
                );
            }
        }
    }

    proptest! {
        #[test]
        fn decoding_arbitrary_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..80)) {
            let _ = ClientMessage::decode(&bytes);
        }

        #[test]
        fn game_messages_round_trip(payload in proptest::collection::vec(any::<u8>(), 0..200)) {
            let message = ClientMessage::GameMessage { kind: id("game:kind"), payload };
            prop_assert_eq!(ClientMessage::decode(&message.encode()), Ok(message));
        }
    }
}
