//! What a Voxunami server and its clients say to each other.
//!
//! The server owns the world and the clients only ever see it through the messages defined here;
//! a recording of the messages one client received is therefore a complete replay of what that
//! client saw. Messages travel one per *frame*: a frame is one encoded message, and carrying frames
//! (framing them on a byte stream, ordering them, delivering them) is the transport's job.
//!
//! The conversation:
//!
//! 1. The client sends [`ClientMessage::Hello`]; the server answers with [`ServerMessage::Welcome`],
//!    which describes the world: its seed, registry tables, channels and terrain generators. Clients
//!    generate terrain themselves, so a world description holds no terrain.
//! 2. The client says which bricks of which level it wants to watch ([`ClientMessage::SetInterest`]).
//!    A world has several levels, each with its own grid, named in messages by a
//!    [`LevelIndex`](voxel_space::LevelIndex). The server
//!    answers with [`ServerMessage::InterestChanged`] (the region now watched, and which terrain
//!    generator produced each part of it), then the contents of every edited brick in the newly
//!    watched part. Unedited terrain is not sent: the client generates it.
//! 3. Every server tick, the server sends the changes inside the watched region (edited bricks,
//!    [`ServerMessage::VoxelEdits`]), the games' own messages ([`ServerMessage::GameMessage`]), and
//!    finally [`ServerMessage::TickCompleted`]. The client's view is consistent at those boundaries.
//!
//! A client can also send text commands ([`ClientMessage::Command`]), which the game's server logic
//! answers with [`ServerMessage::CommandReply`].
//!
//! Games and plugins define their own messages (entities, chat, inventory) and carry them as
//! [`GameMessage`](ServerMessage::GameMessage)s: an identifier saying whose message it is, and
//! bytes only that game understands. Anything that names content (a block type) in such a message
//! should use its identifier text, since two programs may number content differently.

mod client_message;
mod error;
mod message;
mod server_message;
mod types;
mod wire;

pub use wire::{
    read_brick_box, read_brick_coordinate, read_entity_pose, read_grid, read_grid_pose,
    read_voxel_coordinate, write_brick_box, write_brick_coordinate, write_entity_pose, write_grid,
    write_grid_pose, write_voxel_coordinate,
};

pub use client_message::ClientMessage;
pub use error::ProtocolError;
pub use message::{Message, check_message};
pub use server_message::{
    EntityChange, EntityState, GeneratorAssignment, GridChange, GridState, GridTerrainState,
    ServerMessage, VoxelChange, Welcome, is_tick_boundary,
};
pub use types::{ClientId, EntityId, PlayerKey, Tick};

/// Bumped whenever a message changes. A recording stores the version it was made with, and a program
/// plays only recordings of its own version (nothing is kept to read older ones before a first release).
/// (10: the welcome has the clock of the world; 9: a brick coordinate is three wide integers, up to 128 bits each; 8, Phase 39c: a ship may have terrain, which `GridsSpawned` says; 7: a pose has four `f64`.)
pub const PROTOCOL_VERSION: u32 = 10;

/// The most bytes of text a command may have; a longer one is a protocol violation.
pub const MAXIMUM_COMMAND_LENGTH: usize = 1024;
