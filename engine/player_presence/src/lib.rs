//! Where players are and which way they look.
//!
//! A player is an entity ([`player_kind`]) whose pose is where they are and whose components say whose
//! it is, which way they look, and whether they are away. Clients learn of players the way they learn of
//! any entity, by the replication of what is in the region they watch. A client tells the server its pose
//! ([`PoseReport`]); [`Presence`] keeps the table from connections to entities and reads and moves players
//! through them. Games with players in a world use it as it is; what a pose means for the game (reach,
//! line of sight) is theirs.
//!
//! The server also decides which level each player is in (the level is the root grid of their pose). A
//! player moves with the `/level go` command, which [`Presence`] answers by telling that client
//! ([`LevelChange`]); a later way of moving, such as a portal, would use the same message.
//!
//! [`Players`] is the message that told every client where everyone was before protocol version 4. It is
//! read, never sent, so that older recordings still show their players.

mod messages;
mod plugin;
mod presence;

pub use messages::{
    LevelChange, PlayerClient, PlayerKeyComponent, PlayerLook, PlayerPose, PlayerState, Players,
    PoseReport,
};
pub use plugin::{PlayersPlugin, SharedPresence, lock};
pub use presence::{PLAYER_INVENTORY_SLOTS, Presence, player_kind, player_kind_spec};
