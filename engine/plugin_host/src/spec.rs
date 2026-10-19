use namespaced_id::NamespacedId;
use protocol::Message;
use voxel_grid::ChannelIndex;

use crate::event::parse_id;

/// A stage of the tick, in the order they run. The engine receives messages before the first and
/// sends what changed after the last; systems run in between.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Phase {
    /// Brains decide what to do (they may only emit intents).
    Decide,
    /// Systems that turn input into forces on bodies.
    Controllers,
    /// Bodies move.
    Physics,
    /// What movement settled is announced, and what depends on it follows.
    Settle,
    /// The rules of the world: light, fire, and so on.
    Rules,
}

impl Phase {
    pub const ALL: [Self; 5] = [
        Self::Decide,
        Self::Controllers,
        Self::Physics,
        Self::Settle,
        Self::Rules,
    ];

    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

/// Whether a system may differ from machine to machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeterminismClass {
    /// Bit-identical everywhere: covered by the golden sessions and the partition-invariance tests.
    Deterministic,
    /// May differ between machines (a learning brain). It cannot write the world: it can only emit
    /// intents, which go through the same path as a player's messages.
    Mind,
}

/// What stands for the existence and the poses of entities in the components a system declares: a
/// system that makes, removes, moves or looks for entities uses this, besides the components it reads and
/// writes.
pub fn entities_resource() -> NamespacedId {
    parse_id("engine:entities")
}

/// What a system is: where it runs and which channels and components it uses.
#[derive(Clone, Debug)]
pub struct SystemSpec {
    pub id: NamespacedId,
    pub phase: Phase,
    pub after: Vec<NamespacedId>,
    pub before: Vec<NamespacedId>,
    pub reads: Vec<ChannelIndex>,
    pub writes: Vec<ChannelIndex>,
    /// The components of entities it reads, by identifier (see [`entities_resource`]).
    pub component_reads: Vec<NamespacedId>,
    pub component_writes: Vec<NamespacedId>,
}

impl SystemSpec {
    /// A system called `id` that runs in `phase`.
    pub fn new(id: &str, phase: Phase) -> Self {
        Self {
            id: parse_id(id),
            phase,
            after: Vec::new(),
            before: Vec::new(),
            reads: Vec::new(),
            writes: Vec::new(),
            component_reads: Vec::new(),
            component_writes: Vec::new(),
        }
    }

    /// Runs after the system called `id`.
    pub fn after(mut self, id: &str) -> Self {
        self.after.push(parse_id(id));
        self
    }

    /// Runs before the system called `id`.
    pub fn before(mut self, id: &str) -> Self {
        self.before.push(parse_id(id));
        self
    }

    /// The channels it reads. In tests and debug builds a system that reads another fails.
    pub fn reads(mut self, channels: &[ChannelIndex]) -> Self {
        self.reads.extend_from_slice(channels);
        self
    }

    /// The channels it writes (and may read). In tests and debug builds a system that writes another
    /// fails.
    pub fn writes(mut self, channels: &[ChannelIndex]) -> Self {
        self.writes.extend_from_slice(channels);
        self
    }

    /// Reads this component of entities. In tests and debug builds a system that reads another fails.
    pub fn reads_component<C: Message>(mut self) -> Self {
        self.component_reads.push(C::kind());
        self
    }

    /// Writes (and may read) this component of entities.
    pub fn writes_component<C: Message>(mut self) -> Self {
        self.component_writes.push(C::kind());
        self
    }

    /// Looks for entities or reads their poses and kinds.
    pub fn reads_entities(mut self) -> Self {
        self.component_reads.push(entities_resource());
        self
    }

    /// Makes, removes or moves entities (and may look for them).
    pub fn writes_entities(mut self) -> Self {
        self.component_writes.push(entities_resource());
        self
    }
}

/// What a handler of an event is: its name and where it stands among the handlers of that event.
#[derive(Clone, Debug)]
pub struct HandlerSpec {
    pub id: NamespacedId,
    pub after: Vec<NamespacedId>,
    pub before: Vec<NamespacedId>,
}

impl HandlerSpec {
    pub fn new(id: &str) -> Self {
        Self {
            id: parse_id(id),
            after: Vec::new(),
            before: Vec::new(),
        }
    }

    /// Runs after the handler called `id` (of the same event).
    pub fn after(mut self, id: &str) -> Self {
        self.after.push(parse_id(id));
        self
    }

    /// Runs before the handler called `id` (of the same event).
    pub fn before(mut self, id: &str) -> Self {
        self.before.push(parse_id(id));
        self
    }
}

/// Whether saved plugin state is about the whole world or about one grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveScope {
    Global,
    /// Belongs to a grid. `movable` says the state is keyed by voxels, so that a merge or a split can
    /// carry it along; otherwise it is a cache the plugin can rebuild from the voxels.
    PerGrid {
        movable: bool,
    },
}
