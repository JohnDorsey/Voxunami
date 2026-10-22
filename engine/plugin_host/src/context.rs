use std::any::{Any, TypeId};
use std::collections::VecDeque;

use entities::{ComponentBag, Entity, EntityError};
use namespaced_id::NamespacedId;
use protocol::{ClientId, EntityId, Message, Tick};
use server::{AccessError, Audience, GridView, ServerGrid, ServerWorld};
use voxel_grid::ChannelIndex;
use voxel_space::{BrickCoordinate, EntityPose, GridId, VoxelCoordinate, WorldPosition};

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
    /// The questions that are waiting for an answer.
    pub prompts: crate::prompt::Prompts,
    /// The forms that are open for players.
    pub forms: crate::forms::Forms,
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

    /// The acceleration of gravity at a place of a grid, in the grid's own axes, in voxels per second per second: the level's, and the pull of
    /// the ships that weigh enough. For a ship it is the world's gravity turned into the ship's axes, so it changes as the ship turns: a rule
    /// never asks where down is in the world, only where it is for the voxels it looks at. `local` is a position in the grid.
    pub fn gravity_at(
        &self,
        grid: GridId,
        local: voxel_space::WorldPosition,
    ) -> Result<[f32; 3], AccessError> {
        self.world.gravity_in(grid, local)
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

    /// Asks a player a question: a title and 1 to 9 options (see [`crate::MAXIMUM_PROMPT_OPTIONS`]). The
    /// player's client shows it and they answer with the number of an option; the handlers of
    /// [`crate::PromptAnswered`] hear which. There is no time limit: the question ends when it is answered,
    /// when [`Context::cancel_prompt`] is called, or when the player leaves. Fails if the question cannot be
    /// asked or the player is not here.
    pub fn prompt(
        &mut self,
        client: ClientId,
        title: &str,
        options: &[&str],
    ) -> Result<crate::prompt::PromptId, crate::prompt::PromptError> {
        crate::prompt::check(title, options)?;
        if self.world.player_key(client).is_none() {
            return Err(crate::prompt::PromptError::NotConnected);
        }
        let id = self.queue.prompts.ask(client, options.len());
        let question = crate::prompt::Prompt {
            id,
            title: title.to_owned(),
            options: options.iter().map(|option| (*option).to_owned()).collect(),
        };
        self.world.send(
            Audience::Client(client),
            crate::prompt::Prompt::kind(),
            question.encode(),
        );
        Ok(id)
    }

    /// Withdraws a question that has not been answered, and tells the player's client to stop showing it.
    /// Returns whether it was waiting.
    pub fn cancel_prompt(&mut self, client: ClientId, id: crate::prompt::PromptId) -> bool {
        let waiting = self.queue.prompts.cancel(client, id);
        if waiting {
            self.world.send(
                Audience::Client(client),
                crate::prompt::PromptClosed::kind(),
                crate::prompt::PromptClosed(id).encode(),
            );
        }
        waiting
    }

    /// Shows a player an interface (see [`ui::Ui`]): a panel their client draws on the screen. A modal form takes their input
    /// and has buttons, whose presses are heard as [`crate::UiResponded`]; one that is not modal is a panel such as a bar, with
    /// no buttons. It stays until [`Context::close_ui`], until the player dismisses a modal one, or until they leave. Fails if
    /// the interface is beyond the limits of [`ui::Ui::check`], the player has too many open, or is not here.
    pub fn show_ui(
        &mut self,
        client: ClientId,
        ui: &ui::Ui,
    ) -> Result<crate::forms::FormId, crate::forms::FormError> {
        if self.world.player_key(client).is_none() {
            return Err(crate::forms::FormError::NotConnected);
        }
        let id = self.queue.forms.show(client, ui)?;
        self.send_form(client, id, ui);
        Ok(id)
    }

    /// Replaces what an open form shows, keeping its number (so the player's keyboard focus and a half-made choice are not
    /// thrown away by a screen that changed under them). Fails if the form is not open for that player or the new
    /// interface is not allowed.
    pub fn update_ui(
        &mut self,
        client: ClientId,
        id: crate::forms::FormId,
        ui: &ui::Ui,
    ) -> Result<(), crate::forms::FormError> {
        self.queue.forms.update(client, id, ui)?;
        self.send_form(client, id, ui);
        Ok(())
    }

    /// Takes a form off a player's screen. Returns whether it was open.
    pub fn close_ui(&mut self, client: ClientId, id: crate::forms::FormId) -> bool {
        let open = self.queue.forms.close(client, id);
        if open {
            self.world.send(
                Audience::Client(client),
                crate::forms::UiClose::kind(),
                crate::forms::UiClose(id).encode(),
            );
        }
        open
    }

    fn send_form(&mut self, client: ClientId, id: crate::forms::FormId, ui: &ui::Ui) {
        self.world.send(
            Audience::Client(client),
            crate::forms::UiShow::kind(),
            crate::forms::UiShow { id, ui: ui.clone() }.encode(),
        );
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

    fn uniform_value(
        &mut self,
        channel: ChannelIndex,
        brick: BrickCoordinate,
    ) -> Result<Option<u32>, AccessError> {
        self.check(channel, false);
        self.inner.uniform_value(channel, brick)
    }

    fn generated(
        &mut self,
        channel: ChannelIndex,
        at: VoxelCoordinate,
    ) -> Result<u32, AccessError> {
        self.check(channel, false);
        self.inner.generated(channel, at)
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
