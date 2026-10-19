//! Runs a game's server rules as plugins.
//!
//! A game is a list of [`Plugin`]s. Each registers what it adds into a [`Registrar`]: systems that
//! run once a tick in a [`Phase`], handlers of events, handlers of client messages, commands, and
//! the state it saves. [`PluginHost`] checks the lot when it is built (a cycle in the ordering, two
//! handlers for one message, two systems that write one channel with no order between them are
//! errors found before a tick runs) and then is the server's `GameLogic`.
//!
//! Order is part of the replay contract: within a phase, systems run in one sequential order given by
//! the `after` and `before` they declare and, for ties, by plugin identifier then their own; never by
//! hash order, thread timing or the order the plugins were listed. See `docs/plugin-api-spec.md`.

mod command;
mod context;
mod error;
mod event;
mod host;
mod order;
mod registrar;
mod registry;
mod spec;

pub use command::{Argument, ArgumentKind, Arguments, CommandSpec};
pub use context::{CheckedGrid, Context, Intent, MindContext};
pub use error::RegistrationError;
pub use event::{
    ChannelsChanged, EntityDespawned, EntitySpawned, Event, PlayerJoined, PlayerLeft, Verdict,
};
pub use host::{CASCADE_LIMIT, PluginHost};
pub use registrar::{Plugin, Registrar};
pub use spec::{DeterminismClass, HandlerSpec, Phase, SaveScope, SystemSpec};
