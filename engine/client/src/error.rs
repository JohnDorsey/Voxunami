use protocol::{ProtocolError, Tick};
use voxel_space::BrickCoordinate;
use world_storage::{LoadError, TranslationError};

/// Why a client could not follow its server's stream. After any of these the client's view cannot be
/// trusted, and the usual response is to disconnect and reconnect.
#[derive(Clone, Debug, thiserror::Error)]
pub enum ClientError {
    #[error("the connection is closed")]
    ConnectionClosed,
    #[error("the server disconnected this client: {0}")]
    DisconnectedByServer(String),
    #[error("a frame from the server could not be understood: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("the server speaks protocol version {0}, which this program does not")]
    UnsupportedProtocolVersion(u32),
    #[error("the server's world description could not be loaded: {0}")]
    WorldDescription(String),
    #[error("the server sent something its own tables do not account for: {0}")]
    Translation(#[from] TranslationError),
    #[error("the server sent {0} at a time the conversation does not allow it")]
    UnexpectedMessage(&'static str),
    #[error("tick {found} arrived where tick {expected} was due")]
    TickOutOfOrder { expected: Tick, found: Tick },
    #[error("the server sent contents for brick {0}, outside the region it was told to watch")]
    BrickOutsideWatchedRegion(BrickCoordinate),
    #[error("the server sent changes to brick {0}, which this client has no contents for")]
    EditToUnknownBrick(BrickCoordinate),
    #[error("brick {0} differs from the server's after applying its changes")]
    OutOfSync(BrickCoordinate),
}

impl From<LoadError> for ClientError {
    fn from(error: LoadError) -> Self {
        Self::WorldDescription(error.to_string())
    }
}
