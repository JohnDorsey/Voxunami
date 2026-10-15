//! The files a game reads and writes: worlds and recordings.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

use namespaced_id::NamespacedId;
use replay::RecordingConnection;
use transport::Connection;
use world_storage::file::write_atomically;
use world_storage::{ParsedWorld, WorldData};

use crate::definition::GameContent;

/// The world in `path` if the file exists, else a new world with `seed`.
pub fn open_world<Content: GameContent>(
    content: &mut Content,
    path: Option<&Path>,
    seed: u64,
) -> Result<WorldData, String> {
    let Some(path) = path.filter(|path| path.exists()) else {
        return Ok(content.new_world(seed));
    };
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let parsed =
        ParsedWorld::parse(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    let world_seed = parsed.world_seed();
    let (world, report) = parsed
        .load(content.load_context(world_seed))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let missing = report.missing_namespaces();
    if !missing.is_empty() {
        println!(
            "note: the world uses content from plugins that are not loaded ({}); it is kept as it is",
            missing.into_iter().collect::<Vec<_>>().join(", ")
        );
    }
    Ok(world)
}

/// Saves the world to `path`, all or nothing.
pub fn save_world<Content: GameContent>(
    content: &Content,
    world: &mut WorldData,
    path: &Path,
) -> Result<(), String> {
    world.prune_unmodified_overrides();
    let bytes = world
        .save(&content.registries())
        .map_err(|error| format!("{}: {error}", path.display()))?;
    write_atomically(path, &bytes).map_err(|error| format!("{}: {error}", path.display()))
}

/// Wraps `connection` so what it receives is recorded to `path`.
pub fn record_connection<C: Connection + 'static>(
    connection: C,
    path: &Path,
) -> Result<Box<dyn Connection>, String> {
    let file = File::create(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let recording = RecordingConnection::new(connection, BufWriter::new(file))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(Box::new(recording))
}

/// Opens a recording for playback.
pub fn open_recording(path: &Path) -> Result<replay::ReplayConnection<BufReader<File>>, String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    replay::ReplayConnection::new(BufReader::new(file))
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// The game a recording was made by, from its welcome.
pub fn recorded_game(path: &Path) -> Result<NamespacedId, String> {
    replay::read_welcome(path)
        .map(|welcome| welcome.game)
        .map_err(|error| format!("{}: {error}", path.display()))
}
