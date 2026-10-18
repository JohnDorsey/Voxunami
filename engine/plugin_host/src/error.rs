use namespaced_id::NamespacedId;
use voxel_grid::ChannelIndex;

/// Why a set of plugins cannot be started. Every one is a mistake in the plugins, found when the
/// host is built, before a tick runs.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RegistrationError {
    #[error("two plugins are called {0}")]
    DuplicatePlugin(NamespacedId),
    #[error("plugin {plugin} depends on {dependency}, which is not in the list")]
    UnknownDependency {
        plugin: NamespacedId,
        dependency: NamespacedId,
    },
    #[error("these plugins depend on each other in a cycle: {0:?}")]
    DependencyCycle(Vec<NamespacedId>),
    #[error("two systems are called {0}")]
    DuplicateSystem(NamespacedId),
    #[error("two handlers are called {0}")]
    DuplicateHandler(NamespacedId),
    #[error("message kind {0} is declared twice")]
    DuplicateMessage(NamespacedId),
    #[error("the command {0} is registered twice")]
    DuplicateCommand(String),
    #[error(
        "{item} is ordered {relation} {target}, which is not registered (or is not of the same kind)"
    )]
    UnknownOrderingTarget {
        item: NamespacedId,
        relation: &'static str,
        target: NamespacedId,
    },
    #[error("{item} is ordered {relation} {target}, but their phases run the other way round")]
    OrderAgainstPhase {
        item: NamespacedId,
        relation: &'static str,
        target: NamespacedId,
    },
    #[error("these are ordered in a cycle: {0:?}")]
    OrderingCycle(Vec<NamespacedId>),
    #[error(
        "systems {first} and {second} both use channel {channel:?} (at least one writes it) with no order between them"
    )]
    AmbiguousOrder {
        first: NamespacedId,
        second: NamespacedId,
        channel: ChannelIndex,
    },
    #[error("handler {handler} listens for event {event}, which no plugin defines")]
    UnknownEvent {
        handler: NamespacedId,
        event: NamespacedId,
    },
    #[error("the event {0} is defined twice")]
    DuplicateEvent(NamespacedId),
}
