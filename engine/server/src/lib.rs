//! The authoritative game server.
//!
//! A [`Server`] owns the world and runs the game's ticks. Clients connect through
//! [`transport`] connections, say which bricks they want to watch, and are sent those bricks (only the
//! ones that differ from what the client can generate itself) and every change to them. What the
//! game does is supplied as [`GameLogic`]; the server knows nothing about any particular game.
//!
//! Scaling: a client costs the server work in proportion to what it watches, never to the size of the
//! world. Finding the clients an edit concerns is currently a scan of all clients; a spatial index
//! over their watched regions is the step that takes this from dozens to thousands of clients.

mod command_line;
mod config;
mod game_logic;
mod generated_cache;
mod grid_replication;
mod grid_view;
mod replication;
mod server;
mod server_world;
mod ship_settings;
#[cfg(feature = "test-support")]
pub mod test_support;

pub use command_line::CommandLine;
pub use config::ServerConfig;
pub use game_logic::GameLogic;
pub use grid_view::{GridView, ServerGrid};
pub use server::{LeaveReason, Server, ServerError, StepReport};
pub use server_world::{
    AccessError, Audience, ChannelChange, DetachError, GridForce, ServerWorld, ShipStorageError,
};
pub use ship_settings::{LogoutPolicy, ShipSettings, ShipSettingsError};
