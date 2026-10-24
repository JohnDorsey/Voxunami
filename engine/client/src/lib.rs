//! A client's view of a world.
//!
//! The only source of what a client knows about the world is the stream of messages its server sends
//! ([`protocol::ServerMessage`]): that rule is what lets a recording of the stream stand in for the
//! server. [`ClientWorld`] applies messages one by one; [`Client`] adds a [`transport::Connection`],
//! applies them a whole tick at a time, and sends the player's requests.
//!
//! The client generates terrain itself wherever the server says which generator made it, so the
//! stream carries only what differs from generated terrain.

mod client;
mod client_entities;
mod client_grids;
mod client_world;
mod error;

pub use client::{Client, Handshake, HandshakeProgress, TickUpdate, Welcomed};
pub use client_entities::{ClientEntities, ClientEntity};
pub use client_grids::{ClientGrid, ClientGridTerrain, ClientGrids};
pub use client_world::{
    ClientEvent, ClientWorld, GridSummary, LevelSummary, ServerInfo, ViewSummary,
};
pub use error::ClientError;
