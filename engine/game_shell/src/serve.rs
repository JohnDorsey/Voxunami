//! A dedicated server: no window, no player at this computer, just a world that others join.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use transport::tcp::TcpServerListener;

use crate::definition::GameDefinition;
use crate::files::{open_world, save_world};
use crate::host::{Autosave, HostSettings, ServerThread, listen_on};
use crate::options::ServeOptions;

/// Runs a server until it is told to stop (Ctrl-C), then saves the world if a file was named.
pub fn serve<G: GameDefinition>(options: &ServeOptions) -> Result<(), String> {
    let stop = Arc::new(AtomicBool::new(false));
    let signalled = Arc::clone(&stop);
    ctrlc::set_handler(move || signalled.store(true, Ordering::Relaxed))
        .map_err(|error| format!("cannot watch for Ctrl-C: {error}"))?;
    let listener = listen_on(&options.listen)?;
    serve_until::<G>(options, listener, &stop)
}

/// Like [`serve`], but on a listener the caller has made (to pick a free port, say), and stopping
/// when `stop` becomes true. For tests and programs with their own way to be told.
pub fn serve_until<G: GameDefinition>(
    options: &ServeOptions,
    listener: TcpServerListener,
    stop: &AtomicBool,
) -> Result<(), String> {
    let mut content = crate::packs::content_with_packs::<G>(options.packs.as_deref())?;
    let world = open_world(&mut content, options.world_file.as_deref(), options.seed)?;
    let mut settings = HostSettings::local(options.view_radius);
    settings.ships = options.ships;
    settings.network = Some(Box::new(listener));
    settings.announce_players = true;
    if let Some(path) = &options.world_file {
        settings.world_file = Some(path.clone());
        settings.autosave = Some(Autosave {
            interval: Duration::from_secs_f32(options.autosave_seconds),
        });
    }
    let host =
        ServerThread::<G>::start(content, world, settings).map_err(|error| error.to_string())?;
    println!("{} is running; press Ctrl-C to stop", G::title());
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("stopping");
    let (mut world, content) = host.stop();
    if let Some(path) = &options.world_file {
        save_world(&content, &mut world, path)?;
        println!("saved the world to {}", path.display());
    }
    Ok(())
}
