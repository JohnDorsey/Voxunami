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

pub use wire::{read_entity_pose, write_entity_pose};

pub use client_message::ClientMessage;
pub use error::ProtocolError;
pub use message::{Message, check_message};
pub use server_message::{
    EntityChange, EntityState, GeneratorAssignment, ServerMessage, VoxelChange, Welcome,
    is_tick_boundary,
};
pub use types::{ClientId, EntityId, PlayerKey, Tick};

/// Bumped whenever a message changes in a way older programs cannot read. A recording stores the
/// version it was made with, so replaying needs a program that understands it.
pub const PROTOCOL_VERSION: u32 = 4;

/// The oldest protocol version this program can still read. Version 3 only added messages (those about
/// entities), so a recording made at version 2 plays as it did: a recording must stay watchable, which is
/// why a version is bumped only for changes older programs cannot read and the newer ones read every
/// version back to this one. A live connection still needs the same version on both ends.
pub const OLDEST_READABLE_PROTOCOL_VERSION: u32 = 2;

/// The most bytes of text a command may have; a longer one is a protocol violation.
pub const MAXIMUM_COMMAND_LENGTH: usize = 1024;
