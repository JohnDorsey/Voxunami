use binary_codec::DecodeError;
use voxel_grid::BrickDecodeError;

/// A frame could not be understood. Never a panic: frames come from the network and from files.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    #[error("the frame is empty")]
    EmptyFrame,
    #[error("unknown message kind {0}")]
    UnknownMessageKind(u8),
    #[error("malformed message: {0}")]
    Malformed(#[from] DecodeError),
    #[error("a brick in the message is malformed: {0}")]
    MalformedBrick(#[from] BrickDecodeError),
}
