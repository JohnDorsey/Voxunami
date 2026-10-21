//! Which ships move, which wait, and which are in files: [`GridLifecyclePlugin`] keeps the ships of a world as the host of the server wants them
//! kept (see [`server::ShipSettings`]), by how near the players are.
//!
//! A ship moves, runs its rules and is edited while a player is within the *simulation distance* of it. Farther off it is *frozen*: where
//! it is, asleep, its rules waiting. A frozen ship that is farther than the *unload distance* from every player is *parked*: written to
//! a file and out of memory, with the world knowing where it is. A parked ship comes back when a player is within the simulation distance
//! of where it is, or when a player who was aboard it joins. Distances are to the nearest brick of the ship, and each limit has a tenth
//! more to leave it by than to come to it, so that a ship at the edge does not change state every tick.
//!
//! A ship can also be put in a file by a game, to wait for those aboard it (a pilot's ship that goes away with them): it is not brought back
//! for a player who comes near, only for one who was aboard. When that player joins, the plugin looks at where the ship was left (the
//! [`arrival`] module: its hull, the terrain and the other ships) and, if something is there, asks the player what to do. A ship whose file
//! cannot be found asks them too. A game that wants the look gives the plugin what its blocks are to a body ([`ArrivalConfig`]); every game
//! gives it the place a player goes who chooses the world's spawn.

pub mod arrival;
mod distance;
mod plugin;

pub use distance::{squared_distance, squared_distance_to_bricks};
pub use plugin::{ArrivalConfig, GridLifecyclePlugin, LifecycleReport};
