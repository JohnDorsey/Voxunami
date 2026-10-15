use binary_codec::{ByteReader, ByteWriter};
use namespaced_id::NamespacedId;
use voxel_space::BrickBox;

use crate::error::ProtocolError;
use crate::wire::{read_brick_box, write_brick_box};

const KIND_HELLO: u8 = 1;
const KIND_SET_INTEREST: u8 = 2;
const KIND_GAME_MESSAGE: u8 = 3;
const KIND_GOODBYE: u8 = 4;

/// Sent from a client to the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientMessage {
    /// The first message on a connection.
    Hello {
        protocol_version: u32,
        /// Which game the client is running, such as `voxunami_cave:game`. A server refuses clients
        /// of another game.
        game: NamespacedId,
    },
    /// The region of bricks the client wants to watch. The server sends what is in it, and
    /// keeps the client up to date on it, until the next `SetInterest`.
    SetInterest { region: BrickBox },
    /// A message for the game or one of its plugins.
    GameMessage {
        kind: NamespacedId,
        payload: Vec<u8>,
    },
    /// The client is leaving. Closing the connection means the same.
    Goodbye,
}

impl ClientMessage {
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        match self {
            Self::Hello {
                protocol_version,
                game,
            } => {
                writer.write_u8(KIND_HELLO);
                writer.write_u32(*protocol_version);
                writer.write_identifier(game);
            }
            Self::SetInterest { region } => {
                writer.write_u8(KIND_SET_INTEREST);
                write_brick_box(&mut writer, *region);
            }
            Self::GameMessage { kind, payload } => {
                writer.write_u8(KIND_GAME_MESSAGE);
                writer.write_identifier(kind);
                writer.write_bytes(payload);
            }
            Self::Goodbye => writer.write_u8(KIND_GOODBYE),
        }
        writer.into_bytes()
    }

    pub fn decode(frame: &[u8]) -> Result<Self, ProtocolError> {
        let mut reader = ByteReader::new(frame);
        let message = match reader.read_u8().map_err(|_| ProtocolError::EmptyFrame)? {
            KIND_HELLO => Self::Hello {
                protocol_version: reader.read_u32()?,
                game: reader.read_identifier()?,
            },
            KIND_SET_INTEREST => Self::SetInterest {
                region: read_brick_box(&mut reader)?,
            },
            KIND_GAME_MESSAGE => Self::GameMessage {
                kind: reader.read_identifier()?,
                payload: reader.read_bytes()?.to_vec(),
            },
            KIND_GOODBYE => Self::Goodbye,
            unknown => return Err(ProtocolError::UnknownMessageKind(unknown)),
        };
        reader.finish()?;
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use binary_codec::DecodeError;
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
            },
            ClientMessage::SetInterest {
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
        assert_eq!(
            ClientMessage::Hello {
                protocol_version: 1,
                game: id("a:b")
            }
            .encode(),
            [1, 1, 0, 0, 0, 3, b'a', b':', b'b']
        );
        assert_eq!(
            ClientMessage::SetInterest {
                region: BrickBox::new(
                    BrickCoordinate::new(-1, 0, 2),
                    BrickCoordinate::new(3, 4, 5)
                )
                .unwrap()
            }
            .encode(),
            [2, 1, 0, 4, 6, 8, 10]
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
        assert!(ClientMessage::decode(&[2, 6, 0, 0, 0, 0, 0]).is_err());
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
