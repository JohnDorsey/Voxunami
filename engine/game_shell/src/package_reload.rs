//! Reading the packs again while a server runs on them (`hot-reload-plan.md`, step 5; `specs/package-replacement-spec.md`): the one place that makes a game's content,
//! the package its rules read and the clients' picture of it agree again.
//!
//! The steps, in the order they must be made, between two ticks: the content takes the packs from the game as it was before any pack
//! ([`GameContent::reload_packs`], which changes nothing if it is refused); every rule that reads the blocks is given the new package ([`SharedPackage::replace`]);
//! and the server is told, so that it welcomes the next player with it and tells the players it has ([`Server::replace_package`]).

use data_packs::{PackErrors, RawPack};
use server::ServedPack;
use server::Server;

use crate::definition::GameContent;
use crate::shared_package::SharedPackage;

/// What a reload did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reloaded {
    /// What the packs skipped on purpose, one line each.
    pub warnings: Vec<String>,
    /// How many blocks the package gained (the entries appended to the registry of blocks).
    pub blocks_added: usize,
    /// How many times the package has been replaced, this one included.
    pub version: u64,
}

/// Why a reload did not happen.
#[derive(Debug, thiserror::Error)]
pub enum ReloadError {
    /// The packs are no good, or the game does not take them while it runs. Nothing changed.
    #[error("{0}")]
    Packs(PackErrors),
    /// The packs were good, but the rules or the server would not take what they made. This is a mistake in the program, not in the packs, and what is running
    /// is no longer all of a piece: the content has the new blocks and the server may not. Start the server again.
    #[error(
        "the packs were read but the running server could not take them (a mistake in the program; start the server again): {0}"
    )]
    NotTaken(String),
}

/// Reads `packs` again into `content` and replaces the package of the server that runs on it with what it made: `package` is what the rules of the server read, and
/// `server` is told. Call it between two ticks, on the thread that ticks the server.
///
/// Nothing changes if the packs are refused. A reload that does not change a block changes nothing a player sees (the server still sends the message, with no entries
/// appended).
pub fn reload_packs<C: GameContent>(
    content: &mut C,
    packs: &[RawPack],
    replicated: Vec<ServedPack>,
    package: &SharedPackage,
    server: &mut Server,
) -> Result<Reloaded, ReloadError> {
    let blocks_before = content.package_data().blocks.len();
    let warnings = content.reload_packs(packs).map_err(ReloadError::Packs)?;
    content.set_replicated_packs(replicated);
    let data = content.package_data();
    let blocks_added = data.blocks.len() - blocks_before;
    let encoded = data.encode();
    let version = package
        .replace(data)
        .map_err(|error| ReloadError::NotTaken(error.to_string()))?;
    server
        .replace_package(encoded, &content.registries())
        .map_err(|error| ReloadError::NotTaken(error.to_string()))?;
    // (The list of packs follows the package it was made with: clients are told of it in the same tick, if it changed.)
    server.replace_packs(content.replicated_packs().to_vec());
    Ok(Reloaded {
        warnings,
        blocks_added,
        version,
    })
}
