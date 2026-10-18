use std::sync::{Arc, Mutex, MutexGuard};

use namespaced_id::NamespacedId;
use plugin_host::{
    Argument, CommandSpec, HandlerSpec, Phase, PlayerLeft, Plugin, Registrar, SystemSpec,
};

use crate::messages::{LevelChange, Players, PoseReport};
use crate::pose_book::PoseBook;

/// The record of where players are, shared by the plugins of a game that need it (a rule that asks
/// how far a player is, a body that moves them). The game makes the [`PlayersPlugin`], takes its
/// book, and hands it to the plugins that use it; a plugin that wants it declares that it
/// depends on `player_presence:players`.
pub type SharedPoseBook = Arc<Mutex<PoseBook>>;

/// Locks a shared book. (A panic while it is held does not make the book unusable: it is plain data.)
pub fn lock(book: &SharedPoseBook) -> MutexGuard<'_, PoseBook> {
    book.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Where players are and which level each is in: takes in pose messages (if the game believes what
/// players say about where they are), answers `/level`, forgets players who leave, and once a
/// tick (in the `Settle` phase, after whatever moved them) tells everyone where everyone is.
#[derive(Debug)]
pub struct PlayersPlugin {
    book: SharedPoseBook,
    accept_reported_poses: bool,
}

impl PlayersPlugin {
    /// `accept_reported_poses` is whether the game believes a client's message about where it is. A
    /// game whose server moves the players says no, and such messages are ignored.
    pub fn new(accept_reported_poses: bool) -> Self {
        Self {
            book: Arc::new(Mutex::new(PoseBook::new())),
            accept_reported_poses,
        }
    }

    /// The book, to give to the plugins that use it.
    pub fn book(&self) -> SharedPoseBook {
        Arc::clone(&self.book)
    }
}

impl Plugin for PlayersPlugin {
    fn id(&self) -> NamespacedId {
        "player_presence:players"
            .parse()
            .expect("constant identifier is valid")
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        if self.accept_reported_poses {
            let book = Arc::clone(&self.book);
            registrar.message(move |_, client, PoseReport(pose)| {
                lock(&book).set_pose(client, pose);
            });
        }
        registrar.declares_message::<Players>();
        registrar.declares_message::<LevelChange>();
        let book = Arc::clone(&self.book);
        registrar.command(
            CommandSpec::new(
                "level",
                "which level you are in; /level list; /level go NAME",
            )
            .argument(Argument::rest("subcommand").optional()),
            move |context, client, arguments| {
                lock(&book).level_command(client, arguments.rest(0), context.world());
            },
        );
        let book = Arc::clone(&self.book);
        registrar.on::<PlayerLeft>(
            HandlerSpec::new("player_presence:forget_players_who_leave"),
            move |event, _| lock(&book).client_left(event.client),
        );
        let book = Arc::clone(&self.book);
        registrar.system(
            SystemSpec::new("player_presence:tell_everyone", Phase::Settle),
            move |context| lock(&book).tell_everyone(context.world()),
        );
    }
}
