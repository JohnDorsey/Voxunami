//! `/reload`: reading the data packs again while the world runs (`hot-reload-plan.md`, step 5).
//!
//! The rule only asks. A server cannot replace its package from inside a tick, and does not know where the packs are: the host that runs it takes the request
//! between two ticks (`Server::take_reload_request`), reads the packs, and replaces the package everything reads (`game_shell::reload_packs`). Every other rule of
//! Cave follows it (`SharedPackage`).

use namespaced_id::NamespacedId;
use plugin_host::{CommandSpec, Plugin, Registrar};

/// The command `/reload`.
#[derive(Debug, Default)]
pub struct ReloadPlugin;

impl Plugin for ReloadPlugin {
    fn id(&self) -> NamespacedId {
        "voxunami_cave:reload"
            .parse()
            .expect("constant identifier is valid")
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        registrar.command(
            CommandSpec::new(
                "reload",
                "reads the data packs again and replaces what the world is made of, without stopping it",
            ),
            |context, client, _arguments| {
                context.world().request_reload();
                context.reply(
                    client,
                    "the packs are read again when this tick has ended; the server says how it went where it prints",
                );
            },
        );
    }
}
