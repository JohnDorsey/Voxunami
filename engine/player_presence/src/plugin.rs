use std::sync::{Arc, Mutex, MutexGuard};

use namespaced_id::NamespacedId;
use plugin_host::{
    Argument, CommandSpec, HandlerSpec, PlayerJoined, PlayerLeft, Plugin, Registrar,
};

use crate::messages::{LevelChange, PlayerPlaced, PoseReport};
use crate::presence::{Presence, player_kind_spec};

/// The record of which entity is which player, shared by the plugins of a game that need it (a rule that
/// asks how far a player is, a body that moves them). The game makes the [`PlayersPlugin`], takes its
/// presence, and hands it to the plugins that use it; a plugin that wants it declares that it depends on
/// `player_presence:players`.
pub type SharedPresence = Arc<Mutex<Presence>>;

/// Locks shared presence. (A panic while it is held does not make it unusable: it is a table.)
pub fn lock(presence: &SharedPresence) -> MutexGuard<'_, Presence> {
    presence
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Players as entities: registers their kind, gives a joining player the entity their key had before (or
/// makes one at their first pose), takes in pose messages (if the game believes what players say about where
/// they are), answers `/level`, and hides the entity of a player who leaves. Clients are told of players
/// by the entity replication, like any other entity.
#[derive(Debug)]
pub struct PlayersPlugin {
    presence: SharedPresence,
    accept_reported_poses: bool,
}

impl PlayersPlugin {
    /// `accept_reported_poses` is whether the game believes a client's message about where it is. A
    /// game whose server moves the players says no, and such messages are ignored.
    pub fn new(accept_reported_poses: bool) -> Self {
        Self {
            presence: Arc::new(Mutex::new(Presence::new())),
            accept_reported_poses,
        }
    }

    /// The same, where players arrive in a level they are sent to (`/level go`) being what `rule` says of the level they come from and the one they are sent to: a
    /// place in the world's axes, or `None` for the place they were at (see [`Presence::set_level_arrival`]).
    pub fn with_level_arrival(
        self,
        rule: impl Fn(
            &server::ServerWorld,
            voxel_space::LevelIndex,
            voxel_space::LevelIndex,
        ) -> Option<voxel_space::WorldPosition>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        lock(&self.presence).set_level_arrival(rule);
        self
    }

    /// The presence, to give to the plugins that use it.
    pub fn presence(&self) -> SharedPresence {
        Arc::clone(&self.presence)
    }
}

impl Plugin for PlayersPlugin {
    fn id(&self) -> NamespacedId {
        "player_presence:players"
            .parse()
            .expect("constant identifier is valid")
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        registrar.entity_kind(player_kind_spec());
        if self.accept_reported_poses {
            let presence = Arc::clone(&self.presence);
            registrar.message(move |context, client, PoseReport(pose)| {
                lock(&presence).set_pose(context.world(), client, pose);
            });
        }
        registrar.declares_message::<LevelChange>();
        registrar.declares_message::<PlayerPlaced>();
        let presence = Arc::clone(&self.presence);
        registrar.command(
            CommandSpec::new(
                "level",
                "which level you are in; /level list; /level go NAME",
            )
            .argument(Argument::rest("subcommand").optional()),
            move |context, client, arguments| {
                lock(&presence).level_command(client, arguments.rest(0), context.world());
            },
        );
        let presence = Arc::clone(&self.presence);
        registrar.on::<PlayerJoined>(
            HandlerSpec::new("player_presence:take_over_the_saved_player"),
            move |event, context| lock(&presence).client_joined(event.client, context.world()),
        );
        let presence = Arc::clone(&self.presence);
        registrar.on::<PlayerLeft>(
            HandlerSpec::new("player_presence:hide_players_who_leave"),
            move |event, context| lock(&presence).client_left(event.client, context.world()),
        );
    }
}
