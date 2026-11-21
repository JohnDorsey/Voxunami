//! Preparing the ships of a world to be seen from afar (`docs/specs/far-nodes-spec.md`, 8.5 (d) and 8.14): a ship that is in a file is drawn from nodes that were made from its bricks
//! while it was in memory, and a ship that was put away before they were made, or by a program that did not make them, has none. This brings every ship into memory in a server
//! that has no players, lets the game's plugins make what they make of them, and saves the world: a save writes the nodes beside the ship files (and the box of each ship's own
//! bricks into the world file), so the world that comes out has its ships in files with their far views.
//!
//! **Nothing in the world moves while it is done**: with no player, every ship is frozen, none is put away by distance, and the world's clock is put back as it was (what is on a
//! rail is where its rail says at the time the world is started again, whatever place was saved). A ship that waits for its pilot is left in its file: it is not brought in by
//! anyone but its pilot, and a save of it in memory would forget that it waits.

use std::path::PathBuf;

use server::ShipSettings;
use voxel_space::GridId;
use world_storage::{DirectoryShipFiles, ShipFiles};

use crate::definition::{GameContent, GameDefinition};
use crate::files::{open_world, save_world};
use crate::host::build_server_with;

/// How many ticks the server may run to make nodes before it is given up on: far more than a world of any size takes (thirty-two nodes a tick).
const MAXIMUM_TICKS: u64 = 5_000_000;

/// What preparing the ships of a world was asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct PrepareOptions {
    /// The world file (its ships' files are in the folder beside it, as a server keeps them).
    pub world_file: PathBuf,
    /// A directory of data packs, if the world uses any.
    pub packs: Option<PathBuf>,
}

/// What preparing the ships did.
#[derive(Clone, Debug, PartialEq)]
pub struct PrepareReport {
    /// The ships that were brought into memory, and saved again from there.
    pub brought_in: usize,
    /// The ships that were left in their files, and why: they wait for those aboard, or their files could not be read.
    pub left_in_files: Vec<(GridId, String)>,
    /// How many ticks the server ran.
    pub ticks: u64,
    /// How many far files (sets of nodes of a ship) the world has now.
    pub far_files: usize,
}

/// Prepares the ships of the world in `options.world_file` for game `G`. The world file must exist.
pub fn prepare_ships<G: GameDefinition>(options: &PrepareOptions) -> Result<PrepareReport, String> {
    let path = &options.world_file;
    if !path.exists() {
        return Err(format!("{}: there is no such world", path.display()));
    }
    let mut content = crate::packs::content_with_packs::<G>(options.packs.as_deref())?;
    let mut world = open_world(&mut content, Some(path), 0)?;
    let clock = world.clock();
    let files = DirectoryShipFiles::beside(path);
    let format = world
        .ship_format(&content.registries())
        .map_err(|error| format!("{}: {error}", path.display()))?;

    // Every ship that can come in does, before the server starts: it finds them in memory, which is when it makes their nodes.
    let mut brought_in = 0;
    let mut left_in_files = Vec::new();
    let parked: Vec<(GridId, bool)> = world
        .grids()
        .parked()
        .map(|(id, parked)| (id, parked.waits_for_those_aboard))
        .collect();
    for (id, waits_for_those_aboard) in parked {
        if waits_for_those_aboard {
            left_in_files.push((id, "it waits for those aboard".to_owned()));
            continue;
        }
        match world.load_ship(id, &files, &format) {
            Ok(()) => brought_in += 1,
            Err(error) => left_in_files.push((id, error.to_string())),
        }
    }

    // A server with no player: the ships are frozen where they are, and none is put away.
    let plugins = G::plugins(&content);
    let (mut server, _connector) =
        build_server_with::<G>(&content, world, 1, plugins, Box::new(files.clone()))
            .map_err(|error| error.to_string())?;
    server.set_ship_settings(
        ShipSettings::new(0.0, f64::INFINITY).expect("distances that fit one another"),
    );
    let mut ticks = 0;
    // (The plugins mark what they find in memory in their first tick, so there is work to be seen after it, and none is looked for before.)
    loop {
        server.step();
        ticks += 1;
        if !server.work_pending() {
            break;
        }
        if ticks >= MAXIMUM_TICKS {
            return Err(format!(
                "the nodes were still being made after {MAXIMUM_TICKS} ticks; the world file was not written"
            ));
        }
    }
    let mut world = server.into_world();
    world.set_clock(clock);
    save_world(&content, &mut world, path)?;
    let far_files = {
        let mut files = files;
        files
            .far_files_mut()
            .names()
            .map_err(|error| format!("{}: {error}", path.display()))?
            .len()
    };
    Ok(PrepareReport {
        brought_in,
        left_in_files,
        ticks,
        far_files,
    })
}
