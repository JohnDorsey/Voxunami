//! Voxunami 2048: 2048 in an endless landscape.
//!
//! The ground is made of numbered blocks: powers of two, mostly small, in stone, ice and spring
//! ice. A block is its material's block and, as the state of its voxel, its number, so numbers are
//! not limited to a list of blocks. Players push and pull blocks with a tool. A pushed block moves
//! according to its material (stone one step, pushing the row of blocks in front of it; ice and
//! spring ice coast), and what a block does to a block it meets is up to the tool: the double tool
//! merges equal numbers into their double, the add tool adds, the subtract tool subtracts. Every
//! player on the server shares the same world and the same score.
//!
//! Everything is made of voxels, so it is streamed, saved, recorded and replayed like any world.
//!
//! - [`material`] and [`numbered_block`]: what blocks are, and what their materials do when pushed.
//! - [`tool`]: the tools that push blocks, and the way each combines the blocks it meets.
//! - [`motion`]: the rules of moving and merging blocks.
//! - [`landscape`]: the ground.
//! - [`content`]: the blocks as the world stores them, and how worlds are made and loaded.
//! - [`messages`] and [`logic`]: the game's messages, and what the server does with them.
//! - [`game`]: the game as the shell runs it.

pub mod client;
pub mod content;
pub mod game;
pub mod landscape;
pub mod logic;
pub mod material;
pub mod messages;
pub mod motion;
pub mod numbered_block;
pub mod tool;

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
