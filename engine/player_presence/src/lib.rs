//! Where players are and which way they look.
//!
//! A client tells the server its pose ([`pose_kind`]); the server keeps a [`PoseBook`] and, when
//! anything changed, tells everyone where everyone is ([`players_kind`]). Games with players in a
//! world use it as it is; what a pose means for the game (reach, line of sight) is theirs.
//!
//! The server also decides which level each player is in. A player moves with the `/level go`
//! command, which the book answers ([`PoseBook::handle_command`]) by telling that client
//! ([`level_kind`]); a later way of moving, such as a portal, would use the same message.

mod messages;
mod plugin;
mod pose_book;

pub use messages::{
    PlayerPose, PlayerState, decode_level_change, decode_players, decode_pose, encode_level_change,
    encode_players, encode_pose, level_kind, players_kind, pose_kind,
};
pub use plugin::{PlayersPlugin, SharedPoseBook, lock};
pub use pose_book::PoseBook;
