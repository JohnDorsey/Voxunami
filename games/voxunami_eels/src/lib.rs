//! Voxunami Eels: 3D Tron.
//!
//! Flat eels race through a cube-shaped arena, each leaving a wall behind it. An eel is a ribbon
//! three cells wide and one thick, and rolling it a quarter turn changes which way it is wide. An
//! eel that touches the arena's wall, any eel's body or another eel dies; the last one swimming
//! wins the round. Players see the arena from behind their eel.
//!
//! Everything is made of voxels, so it is streamed, saved, recorded and replayed like any world:
//! the server runs the eels and the players' cameras, and a recording is a replay of the match.
//!
//! - [`arena`]: the arena and its wall, a terrain generator.
//! - [`eel`]: how an eel is steered and which cells its body covers.
//! - [`round`]: the rules of a match, on any arena.
//! - [`view`]: where each player's camera is.
//! - [`content`]: the blocks as the world stores them, and how worlds are made and loaded.
//! - [`messages`] and [`logic`]: the game's messages, and what the server does with them.
//! - [`game`]: the game as the shell runs it.

pub mod arena;
pub mod content;
pub mod eel;
pub mod game;
pub mod logic;
pub mod messages;
pub mod round;
pub mod view;

pub use content::game_id;
pub use game::Eels;

/// Plays the game. `arguments` are the command line after the command name.
pub fn run_play(arguments: impl IntoIterator<Item = String>) -> Result<(), String> {
    game_shell::run_play::<Eels>(arguments)
}

/// Runs a dedicated server for the game. `arguments` are the command line after the game's name.
pub fn run_serve(arguments: impl IntoIterator<Item = String>) -> Result<(), String> {
    game_shell::run_serve::<Eels>(arguments)
}

/// Watches a recording of the game. `arguments` are the command line after the command name.
pub fn run_replay(arguments: impl IntoIterator<Item = String>) -> Result<(), String> {
    game_shell::run_replay::<Eels>(arguments)
}
