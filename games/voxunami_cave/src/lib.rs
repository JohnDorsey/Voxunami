//! Voxunami Cave: the Minecraft-like game.
//!
//! For now the game is a world of rolling caverns to fly through, whose blocks any player can break
//! and place, alone (the game runs its own server inside the process) or together. Everything a
//! player sees reaches them as server messages, so what they saw can be recorded and replayed.
//!
//! - [`content`]: the plugins the game uses, and how worlds are made and loaded from them.
//! - [`messages`] and [`logic`]: the game's own messages, and what the server does with them.
//! - [`host`]: running the server in this process.
//! - [`session`]: a player's side, without a window; windows and screenshots are built on it.

pub mod content;
pub mod files;
mod headless;
pub mod host;
pub mod logic;
pub mod messages;
pub mod options;
pub mod session;
pub mod view;
mod windowed;

use options::{PLAY_USAGE, Parsed, REPLAY_USAGE};

pub use content::game_id;

/// Plays the game. `arguments` are the command line after the command name.
pub fn run_play(arguments: impl IntoIterator<Item = String>) -> Result<(), String> {
    match options::parse_play(arguments).map_err(with_usage(PLAY_USAGE))? {
        Parsed::ShowHelp => {
            println!("{PLAY_USAGE}");
            Ok(())
        }
        Parsed::Run(options) if options.view.screenshot.is_some() || options.benchmark => {
            headless::play(&options)
        }
        Parsed::Run(options) => windowed::play(options),
    }
}

/// Watches a recording of the game. `arguments` are the command line after the command name.
pub fn run_replay(arguments: impl IntoIterator<Item = String>) -> Result<(), String> {
    match options::parse_replay(arguments).map_err(with_usage(REPLAY_USAGE))? {
        Parsed::ShowHelp => {
            println!("{REPLAY_USAGE}");
            Ok(())
        }
        Parsed::Run(options) if options.view.screenshot.is_some() => headless::replay(&options),
        Parsed::Run(options) => windowed::replay(options),
    }
}

fn with_usage(usage: &'static str) -> impl Fn(String) -> String {
    move |message| format!("{message}\n\n{usage}")
}
