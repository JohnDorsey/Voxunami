//! Voxunami Cave: the Minecraft-like game.
//!
//! For now the game is a world of rolling hills to fly through, whose blocks any player can break
//! and place, alone (the game runs its own server inside the process) or together. Everything a
//! player sees reaches them as server messages, so what they saw can be recorded and replayed.
//!
//! - [`content`]: the plugins the game uses, and how worlds are made and loaded from them.
//! - [`messages`] and [`logic`]: the game's own message, and what the server does with it.
//! - [`game`]: the game as the shell runs it: what clicks and keys do.
//!
//! Window, camera, singleplayer server, screenshots, recording and replay are the shell's.

pub mod content;
pub mod game;
pub mod light_state;
pub mod lighting;
pub mod logic;
pub mod messages;

pub use content::game_id;
pub use game::Cave;

/// Plays the game. `arguments` are the command line after the command name.
pub fn run_play(arguments: impl IntoIterator<Item = String>) -> Result<(), String> {
    game_shell::run_play::<Cave>(arguments)
}

/// Runs a dedicated server for the game. `arguments` are the command line after the game's name.
pub fn run_serve(arguments: impl IntoIterator<Item = String>) -> Result<(), String> {
    game_shell::run_serve::<Cave>(arguments)
}

/// Watches a recording of the game. `arguments` are the command line after the command name.
pub fn run_replay(arguments: impl IntoIterator<Item = String>) -> Result<(), String> {
    game_shell::run_replay::<Cave>(arguments)
}
