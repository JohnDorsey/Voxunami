use std::any::{Any, TypeId};
use std::collections::VecDeque;

use entities::{ComponentBag, Entity, EntityError};
use namespaced_id::NamespacedId;
use protocol::{ClientId, EntityId, Message, Tick};
use server::{AccessError, Audience, GridView, ServerGrid, ServerWorld};
use voxel_grid::ChannelIndex;
use voxel_space::{EntityPose, GridId, VoxelCoordinate, WorldPosition};

use crate::event::{EntityDespawned, EntitySpawned, Event, Verdict};
use crate::registry::{HandlerKind, Registry};
use crate::spec::{SystemSpec, entities_resource};

/// An event waiting to be delivered.
pub(crate) struct Queued {
    pub type_id: TypeId,
    pub payload: Box<dyn Any + Send>,
}

/// The events waiting, and how many have been delivered this tick.
#[derive(Default)]
pub(crate) struct Queue {
    pub events: VecDeque<Queued>,
    pub delivered: usize,
    /// Set when a tick delivered more events than [`crate::host::CASCADE_LIMIT`]: nothing more is
    /// delivered until the tick ends.
    pub halted: bool,
    pub overflows: u32,
}

/// What a system, handler, message handler or command handler is given: the world, and the means to
/// send and ask events.
///
/// Rules read and write the world through [`Context::grid`], which sees one grid in its own
/// coordinates. [`Context::world`] is the whole server world, for what is not about a grid
/// (messages, replies, who is connected); it also reaches voxels by level and bypasses the checks of
/// declared channels, so a rule does not use it for voxels.
pub struct Context<'a> {
    pub(crate) world: &'a mut ServerWorld,
    pub(crate) registry: &'a Registry,
    pub(crate) queue: &'a mut Queue,
    pub(crate) tick: Tick,
    pub(crate) client: Option<ClientId>,
    /// The system running, if one is: what it declared is checked in tests and debug builds.
    pub(crate) declared: Option<&'a SystemSpec>,
}

impl<'a> Context<'a> {
    /// The tick being run.
    pub fn tick(&self) -> Tick {
        self.tick
    }

    /// The client whose message or command is being handled, if one is.
    pub fn client(&self) -> Option<ClientId> {
        self.client
    }

    /// The server's world, for everything that is not the voxels of a grid. See the type's notes.
    pub fn world(&mut self) -> &mut ServerWorld {
        self.world
    }

    /// One grid, read and written in its own coordinates. Fails for a grid the world does not have.
    pub fn grid(&mut self, grid: GridId) -> Result<CheckedGrid<'_>, AccessError> {
        let inner = self.world.grid(grid)?;
        Ok(CheckedGrid {
            inner,
            declared: self.declared,
        })
    }

    /// Makes an entity of `kind` at `pose` with the kind's components, those in `components` taking the
    /// place of its defaults. The handlers of [`EntitySpawned`] hear of it when this system or handler
    /// returns.
    pub fn spawn(
        &mut self,
        kind: &NamespacedId,
        pose: EntityPose,
        components: ComponentBag,
    ) -> Result<EntityId, EntityError> {
        self.check_component(&entities_resource(), true);
        let id = self.world.spawn_entity(kind, pose, components)?;
        self.emit(EntitySpawned {
            id,
            kind: kind.clone(),
        });
        Ok(id)
    }

    /// Removes an entity and returns it, if there was one. The handlers of [`EntityDespawned`] hear of it.
    pub fn despawn(&mut self, id: EntityId) -> Option<Entity> {
        self.check_component(&entities_resource(), true);
        let entity = self.world.despawn_entity(id)?;
        self.emit(EntityDespawned {
            id,
            kind: entity.kind.clone(),
        });
        Some(entity)
    }

    /// An entity, with its kind, pose and every component as bytes.
    pub fn entity(&self, id: EntityId) -> Option<&Entity> {
        self.check_component(&entities_resource(), false);
        self.world.entities().get(id)
    }

    /// A component of an entity: `Ok(None)` if its kind has none of this type.
    pub fn component<C: Message>(&self, id: EntityId) -> Result<Option<C>, EntityError> {
        self.check_component(&C::kind(), false);
        self.world.entities().component::<C>(id)
    }

    /// Sets a component the entity's kind declares.
    pub fn set_component<C: Message>(
        &mut self,
        id: EntityId,
        value: &C,
    ) -> Result<(), EntityError> {
        self.check_component(&C::kind(), true);
        self.world.set_entity_component(id, value)
    }

    /// Moves an entity.
    pub fn set_pose(&mut self, id: EntityId, pose: EntityPose) -> Result<(), EntityError> {
        self.check_component(&entities_resource(), true);
        self.world.set_entity_pose(id, pose)
    }

    /// The entities of a grid within `radius` voxels of a position, in order of identifier.
    pub fn entities_near(
        &self,
        grid: GridId,
        position: WorldPosition,
        radius: f32,
    ) -> Vec<EntityId> {
        self.check_component(&entities_resource(), false);
        self.world.entities_near(grid, position, radius)
    }

    /// In tests and debug builds a system that uses a component it did not declare panics.
    fn check_component(&self, component: &NamespacedId, writing: bool) {
        if !cfg!(debug_assertions) {
            return;
        }
        let Some(spec) = self.declared else { return };
        let allowed = spec.component_writes.contains(component)
            || (!writing && spec.component_reads.contains(component));
        assert!(
            allowed,
            "system {} {} the component {component}, which it did not declare",
            spec.id,
            if writing { "wrote" } else { "read" },
        );
    }

    /// Queues a message for clients, delivered at the end of this tick.
    pub fn send(&mut self, audience: Audience, kind: NamespacedId, payload: Vec<u8>) {
        self.world.send(audience, kind, payload);
    }

    /// Queues text for one client as the answer to a command.
    pub fn reply(&mut self, client: ClientId, text: &str) {
        self.world.reply(client, text);
    }

    /// Sends an event. It is delivered when the current system or handler returns, after the events
    /// already waiting.
    pub fn emit<E: Event>(&mut self, event: E) {
        self.queue.events.push_back(Queued {
            type_id: TypeId::of::<E>(),
            payload: Box::new(event),
        });
    }

    /// Asks the handlers of an event that asks first, right now, in their order. The first to refuse
    /// wins; if none does, the answer is [`Verdict::Allow`].
    pub fn ask<E: Event>(&mut self, event: &E) -> Verdict {
        let Some(handlers) = self
            .registry
            .handlers
            .get(&(TypeId::of::<E>(), HandlerKind::Ask))
        else {
            return Verdict::Allow;
        };
        for handler in handlers {
            let mut run = handler
                .run
                .try_borrow_mut()
                .expect("a handler asked for an event that reaches the handler itself");
            let mut inner = Context {
                world: &mut *self.world,
                registry: self.registry,
                queue: &mut *self.queue,
                tick: self.tick,
                client: self.client,
                declared: None,
            };
            if let Verdict::Refuse(reason) = run(event as &dyn Any, &mut inner) {
                return Verdict::Refuse(reason);
            }
        }
        Verdict::Allow
    }
}

impl std::fmt::Debug for Context<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Context")
            .field("tick", &self.tick)
            .field("client", &self.client)
            .finish_non_exhaustive()
    }
}

/// One grid as a system sees it: reads and writes are checked against what the system declared (in
/// tests and debug builds; a system that uses a channel it did not declare panics).
pub struct CheckedGrid<'a> {
    inner: ServerGrid<'a>,
    declared: Option<&'a SystemSpec>,
}

impl std::fmt::Debug for CheckedGrid<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CheckedGrid")
            .field("grid", &self.inner.id())
            .finish_non_exhaustive()
    }
}

impl CheckedGrid<'_> {
    fn check(&self, channel: ChannelIndex, writing: bool) {
        if !cfg!(debug_assertions) {
            return;
        }
        let Some(spec) = self.declared else { return };
        let allowed = spec.writes.contains(&channel) || (!writing && spec.reads.contains(&channel));
        assert!(
            allowed,
            "system {} {} channel {channel:?}, which it did not declare",
            spec.id,
            if writing { "wrote" } else { "read" },
        );
    }
}

impl GridView for CheckedGrid<'_> {
    fn id(&self) -> GridId {
        self.inner.id()
    }

    fn get(&mut self, channel: ChannelIndex, at: VoxelCoordinate) -> Result<u32, AccessError> {
        self.check(channel, false);
        self.inner.get(channel, at)
    }

    fn set(
        &mut self,
        channel: ChannelIndex,
        at: VoxelCoordinate,
        value: u32,
    ) -> Result<bool, AccessError> {
        self.check(channel, true);
        self.inner.set(channel, at, value)
    }
}

/// What a brain asked for: the same thing a player's message would ask, in the same form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Intent {
    pub client: ClientId,
    pub kind: NamespacedId,
    pub payload: Vec<u8>,
}

/// What a `Mind` system is given: the tick, and a place to put intents. It cannot reach the world, so
/// whatever such a system does, nothing it does can differ between machines in a way that matters:
/// the rules that carry out its intents are the same everywhere.
///
/// ```
/// fn decide(context: &mut plugin_host::MindContext) -> u64 {
///     context.tick().get()
/// }
/// ```
///
/// There is no way to write the world from it:
///
/// ```compile_fail
/// fn cannot_write(context: &mut plugin_host::MindContext) {
///     context.world();
/// }
/// ```
pub struct MindContext<'a> {
    pub(crate) tick: Tick,
    pub(crate) intents: &'a mut Vec<Intent>,
}

impl std::fmt::Debug for MindContext<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MindContext")
            .field("tick", &self.tick)
            .finish_non_exhaustive()
    }
}

impl MindContext<'_> {
    pub fn tick(&self) -> Tick {
        self.tick
    }

    /// Asks for something to be done for `client`, as if their client had sent `kind` and `payload`.
    pub fn push_intent(&mut self, client: ClientId, kind: NamespacedId, payload: Vec<u8>) {
        self.intents.push(Intent {
            client,
            kind,
            payload,
        });
    }
}
