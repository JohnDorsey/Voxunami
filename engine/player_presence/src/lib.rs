//! Where players are and which way they look.
//!
//! A client tells the server its pose ([`pose_kind`]); the server keeps a [`PoseBook`] and, when
//! anything changed, tells everyone where everyone is ([`players_kind`]). Games with players in a
//! world use it as it is; what a pose means for the game (reach, line of sight) is theirs.

mod messages;
mod pose_book;

pub use messages::{
    PlayerPose, decode_players, decode_pose, encode_players, encode_pose, players_kind, pose_kind,
};
pub use pose_book::PoseBook;
