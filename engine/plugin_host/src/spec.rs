use namespaced_id::NamespacedId;
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

/// What a system is: where it runs and which channels it uses.
#[derive(Clone, Debug)]
pub struct SystemSpec {
    pub id: NamespacedId,
    pub phase: Phase,
    pub after: Vec<NamespacedId>,
    pub before: Vec<NamespacedId>,
    pub reads: Vec<ChannelIndex>,
    pub writes: Vec<ChannelIndex>,
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

/// A server command: `usage` is how it is typed (`/tp X Y Z`), `summary` says what it does.
#[derive(Clone, Copy, Debug)]
pub struct CommandSpec {
    pub name: &'static str,
    pub usage: &'static str,
    pub summary: &'static str,
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
