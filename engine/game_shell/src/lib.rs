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
#[cfg(test)]
mod coarse_view_tests;
mod command_entry;
mod definition;
#[cfg(test)]
mod engine_shader_reload_tests;
pub mod files;
pub mod following;
mod headless;
pub mod host;
pub mod options;
pub mod package;
mod package_data;
mod package_reload;
mod packs;
pub mod paint_bridge;
#[cfg(test)]
mod planet_view_tests;
mod player_identity;
mod pointer_hold;
pub mod reload;
pub mod screen;
pub mod serve;
pub mod session;
pub mod shader_pack_settings;
pub mod shader_packs;
mod shared_package;
pub mod sun;
#[cfg(test)]
mod sun_view_tests;
#[cfg(test)]
mod test_support;
pub mod texture_pack;
pub mod view;
#[cfg(test)]
mod view_reload_tests;
mod windowed;

pub use data_packs::{BodySupport, GameTexture, PackErrors, RawFile, RawPack, RawTexture};
pub use definition::{
    CameraStart, CoarseDetail, GameContent, GameDefinition, unpaired_client_plugins,
};
pub use identity::Identity;
pub use package::{ContentPackage, FamilyFactory, GeneratorFactory, PackageSpec, no_families};
pub use package_data::{PackageData, PackageError, check_materials};
pub use package_reload::{ReloadError, Reloaded, reload_packs};
pub use player_presence::{PlayerPose, Presence};
pub use shared_package::{ReplaceError, SharedPackage};
pub use sun::{Sky, Sun, SunView};
pub use winit::keyboard::KeyCode;

pub use packs::{content_with_packs, run_check_packs, run_pack_schemas};
pub use shader_packs::run_check_shader_packs;

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
        Parsed::Run(mut options) => {
            options
                .view
                .use_default_texture_pack(default_texture_pack_exists());
            if options.view.screenshot.is_some() || options.benchmark {
                headless::play::<G>(&options)
            } else {
                windowed::play::<G>(options)
            }
        }
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
        Parsed::Run(mut options) => {
            options
                .view
                .use_default_texture_pack(default_texture_pack_exists());
            if options.view.screenshot.is_some() {
                headless::replay::<G>(&options)
            } else {
                windowed::replay::<G>(options)
            }
        }
    }
}

/// Whether the folder that is the texture pack by default is there, as seen from the folder the program was started in.
fn default_texture_pack_exists() -> bool {
    std::path::Path::new(options::DEFAULT_TEXTURE_PACK).is_dir()
}
