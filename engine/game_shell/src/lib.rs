//! What every Voxunami game shares.
//!
//! A game implements [`GameDefinition`]: what it is called, how its server behaves, and what a
//! player's clicks and keys do. The shell supplies the rest: a window with a fly camera, a
//! singleplayer server running in the same process ([`host`]), joining a server ([`session`]),
//! worlds and recordings on disk ([`files`]), screenshots and benchmarks without a window, and
//! playing recordings back. Everything a player sees arrives as server messages, which is what
//! makes recording and replay work for every game.

pub mod client_commands;
pub mod client_plugin;
mod command_entry;
mod definition;
pub mod files;
pub mod following;
mod headless;
pub mod host;
pub mod options;
pub mod package;
mod package_data;
mod packs;
mod player_identity;
pub mod screen;
pub mod serve;
pub mod session;
#[cfg(test)]
mod test_support;
pub mod view;
mod windowed;

pub use data_packs::{PackErrors, RawFile, RawPack};
pub use definition::{CameraStart, GameContent, GameDefinition};
pub use identity::Identity;
pub use package::{ContentPackage, GeneratorFactory, PackageSpec};
pub use package_data::{PackageData, PackageError, check_materials};
pub use player_presence::{PlayerPose, Presence};
pub use winit::keyboard::KeyCode;

pub use packs::{content_with_packs, run_check_packs, run_pack_schemas};

use options::{Parsed, play_usage, replay_usage, serve_usage};

/// Plays game `G`. `arguments` are the command line after the command name.
pub fn run_play<G: GameDefinition>(
    arguments: impl IntoIterator<Item = String>,
) -> Result<(), String> {
    let parsed = options::parse_play::<G>(arguments)
        .map_err(|message| format!("{message}\n\n{}", play_usage::<G>()))?;
    match parsed {
        Parsed::ShowHelp => {
            println!("{}", play_usage::<G>());
            Ok(())
        }
        Parsed::Run(options) if options.view.screenshot.is_some() || options.benchmark => {
            headless::play::<G>(&options)
        }
        Parsed::Run(options) => windowed::play::<G>(options),
    }
}

/// Runs a dedicated server for game `G`. `arguments` are the command line after the game's name.
pub fn run_serve<G: GameDefinition>(
    arguments: impl IntoIterator<Item = String>,
) -> Result<(), String> {
    let parsed = options::parse_serve::<G>(arguments).map_err(|message| {
        format!(
            "{message}

{}",
            serve_usage::<G>()
        )
    })?;
    match parsed {
        Parsed::ShowHelp => {
            println!("{}", serve_usage::<G>());
            Ok(())
        }
        Parsed::Run(options) => serve::serve::<G>(&options),
    }
}

/// Watches a recording of game `G`. `arguments` are the command line after the command name.
pub fn run_replay<G: GameDefinition>(
    arguments: impl IntoIterator<Item = String>,
) -> Result<(), String> {
    let parsed = options::parse_replay(arguments)
        .map_err(|message| format!("{message}\n\n{}", replay_usage()))?;
    match parsed {
        Parsed::ShowHelp => {
            println!("{}", replay_usage());
            Ok(())
        }
        Parsed::Run(options) if options.view.screenshot.is_some() => {
            headless::replay::<G>(&options)
        }
        Parsed::Run(options) => windowed::replay::<G>(options),
    }
}
