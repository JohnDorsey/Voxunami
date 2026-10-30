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
    while !stop.load(Ordering::Relaxed) && !host.has_failed() {
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("stopping");
    let (mut world, content) = host.stop().map_err(|failure| {
        let saved = match &options.world_file {
            Some(_) => "; the world file was not written, so the last autosave is the world",
            None => "; there was no world file, so the world is lost",
        };
        format!("{failure}{saved}")
    })?;
    if let Some(path) = &options.world_file {
        save_world(&content, &mut world, path)?;
        println!("saved the world to {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ExamplePanickingGame, PANIC_MESSAGE};

    #[test]
    fn a_server_that_fails_ends_the_command_with_the_reason_and_leaves_the_world_file_alone() {
        let folder = std::env::temp_dir().join(format!("serve_fails_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let world_file = folder.join("world.vox");
        let options = ServeOptions {
            listen: "127.0.0.1:0".to_owned(),
            seed: 1,
            world_file: Some(world_file.clone()),
            packs: None,
            autosave_seconds: 3600.0,
            view_radius: 2,
            ships: server::ShipSettings::default(),
        };
        let listener = listen_on(&options.listen).unwrap();
        // Nobody presses Ctrl-C: the command must end by itself because its server died.
        let never = AtomicBool::new(false);
        let error = serve_until::<ExamplePanickingGame>(&options, listener, &never)
            .expect_err("a server that failed is an error");
        assert!(error.contains(PANIC_MESSAGE), "{error}");
        assert!(
            error.contains("autosave"),
            "the message tells the owner what is safe: {error}"
        );
        assert!(
            !world_file.exists(),
            "what a failed server held may be half a tick; the file is not written from it"
        );
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
