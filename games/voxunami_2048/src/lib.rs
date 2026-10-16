//! Voxunami 2048: 2048 in an endless landscape.
//!
//! The ground is made of numbered blocks: powers of two, mostly small, in stone, ice and spring
//! ice. Players push and pull blocks with a tool. A pushed block moves according to its material
//! (stone one step; ice and spring ice coast), and a block that meets an equal number merges with
//! it into the next number. Blocks never push each other. Every player on the server shares the
//! same world and the same score.
//!
//! Everything is made of voxels, so it is streamed, saved, recorded and replayed like any world.
//!
//! - [`material`] and [`numbered_block`]: what blocks are, and what their materials do when pushed.
//! - [`motion`]: the rules of moving and merging blocks.
//! - [`landscape`]: the ground.
//! - [`content`]: the blocks as the world stores them, and how worlds are made and loaded.
//! - [`messages`] and [`logic`]: the game's messages, and what the server does with them.
//! - [`game`]: the game as the shell runs it.

pub mod content;
pub mod game;
pub mod landscape;
pub mod logic;
pub mod material;
pub mod messages;
pub mod motion;
pub mod numbered_block;

pub use content::game_id;
pub use game::Game2048;

/// Plays the game. `arguments` are the command line after the command name.
pub fn run_play(arguments: impl IntoIterator<Item = String>) -> Result<(), String> {
    game_shell::run_play::<Game2048>(arguments)
}

/// Runs a dedicated server for the game. `arguments` are the command line after the game's name.
pub fn run_serve(arguments: impl IntoIterator<Item = String>) -> Result<(), String> {
    game_shell::run_serve::<Game2048>(arguments)
}

/// Watches a recording of the game. `arguments` are the command line after the command name.
pub fn run_replay(arguments: impl IntoIterator<Item = String>) -> Result<(), String> {
    game_shell::run_replay::<Game2048>(arguments)
}
