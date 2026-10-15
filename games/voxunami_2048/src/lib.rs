//! Voxunami 2048: 3D 2048.
//!
//! A 4 x 4 x 4 board of tiles floats in the sky. A player clicks one of six push plates to push
//! every tile toward the opposite side: equal tiles that meet merge into the next value, and after
//! every push that changed anything a new tile appears. The game ends when no push changes the
//! board. Any player on the server can push, so it can be played together.
//!
//! The board is made of voxels, so it is streamed, saved, recorded and replayed like any world:
//! the rules in [`board`] only ever see a plain array, and [`logic`] turns it into voxels.
//!
//! - [`board`]: the rules.
//! - [`layout`]: where the board, frame and plates are in the world.
//! - [`content`]: the blocks, and how worlds are made and loaded.
//! - [`messages`] and [`logic`]: the game's messages, and what the server does with them.
//! - [`game`]: the game as the shell runs it.

pub mod board;
pub mod content;
pub mod game;
pub mod layout;
pub mod logic;
pub mod messages;

pub use content::game_id;
pub use game::Game2048;

/// Plays the game. `arguments` are the command line after the command name.
pub fn run_play(arguments: impl IntoIterator<Item = String>) -> Result<(), String> {
    game_shell::run_play::<Game2048>(arguments)
}

/// Watches a recording of the game. `arguments` are the command line after the command name.
pub fn run_replay(arguments: impl IntoIterator<Item = String>) -> Result<(), String> {
    game_shell::run_replay::<Game2048>(arguments)
}
