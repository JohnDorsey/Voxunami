use std::any::TypeId;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use namespaced_id::NamespacedId;
use protocol::{ClientId, Message, Tick};
use server::{Audience, CommandLine, GameLogic, ServerWorld};
use voxel_grid::ChannelIndex;
use voxel_space::{GridId, VoxelCoordinate};
use world_storage::{Section, WorldData};

use entities::EntityCatalog;

use crate::command::{Argument, Arguments, CommandSpec};
use crate::context::{Context, Intent, MindContext, Queue, Queued};
use crate::error::RegistrationError;
use crate::event::{ChannelsChanged, PlayerJoined, PlayerLeft, PromptAnswered, UiResponded};
use crate::forms::{UiAction, UiClose, UiResponse};
use crate::prompt::{PromptAnswer, PromptClosed};
use crate::registrar::{Builder, Plugin, Registrar, finish, registration_order};
use crate::registry::{HandlerKind, Registry, Runner};
use crate::spec::Phase;

/// The most events delivered in one tick. A plugin whose handlers send events that make the same
/// handlers send more has a loop; delivery stops for the rest of that tick, and it is counted (see
/// [`PluginHost::cascade_overflows`]). It is deterministic: the same loop stops at the same event.
pub const CASCADE_LIMIT: usize = 4096;

/// Runs a game's plugins as the server's [`GameLogic`].
///
/// A tick, after the engine has taken in the clients' messages, runs the phases in order and the
/// systems of each in the order the plugins gave. **Events** are delivered synchronously: when a
/// system, a handler, a message handler or a command handler returns, the events it sent and one
/// `ChannelsChanged` for each channel it wrote that someone asked about are put in a first-in
/// first-out queue and delivered, each to all its handlers in their order, until none are left. What a
/// handler sends or writes joins the back of the queue (after the other events waiting), so it is
/// delivered once every handler of the event being delivered has run.
pub struct PluginHost {
    registry: Registry,
    queue: Queue,
    tick: Tick,
    prepared: bool,
}

impl fmt::Debug for PluginHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PluginHost")
            .field("systems", &self.registry.systems.len())
            .field("commands", &self.registry.commands.len())
            .finish_non_exhaustive()
    }
}

impl PluginHost {
    /// Registers the plugins, in dependency order and otherwise by identifier (the order of `plugins`
    /// does not matter), and checks and orders what they registered. `world` is the world the server
    /// starts from.
    pub fn new(
        plugins: Vec<Box<dyn Plugin>>,
        world: &WorldData,
    ) -> Result<Self, RegistrationError> {
        let sequence = registration_order(&plugins)?;
        let mut builder = Builder::default();
        builder.define_engine_events();
        for index in sequence {
            let plugin = &plugins[index];
            let mut registrar = Registrar::new(plugin.id(), world, &mut builder);
            plugin.register(&mut registrar);
        }
        Ok(Self {
            registry: finish(builder)?,
            queue: Queue::default(),
            tick: Tick::ZERO,
            prepared: false,
        })
    }

    /// How many ticks delivered more than [`CASCADE_LIMIT`] events.
    pub fn cascade_overflows(&self) -> u32 {
        self.queue.overflows
    }

    /// Tests the codec of every declared message kind on its samples
    /// ([`protocol::check_message`]); the errors say which kind failed and how.
    pub fn check_messages(&self) -> Result<(), Vec<String>> {
        let errors: Vec<String> = self
            .registry
            .messages
            .values()
            .filter_map(|entry| (entry.check)().err())
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// How many payloads of kinds the plugins handle did not decode, and were ignored.
    pub fn malformed_messages(&self) -> u64 {
        self.registry.malformed_messages.load(Ordering::Relaxed)
    }

    /// The count of [`malformed_messages`](Self::malformed_messages) as a handle that outlives the
    /// host's move into the server, for whatever reports on it.
    pub fn malformed_messages_counter(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.registry.malformed_messages)
    }

    /// The kinds of every declared message, in order.
    pub fn declared_messages(&self) -> Vec<NamespacedId> {
        self.registry.messages.keys().cloned().collect()
    }

    /// The usage of every command, by name; `/help` is among them when there are any others.
    pub fn command_usages(&self) -> Vec<String> {
        command_list(&self.registry)
            .iter()
            .map(CommandSpec::usage)
            .collect()
    }

    /// The scope of each piece of saved plugin state, in the order the sections are written.
    pub fn saved_state_scopes(&self) -> Vec<crate::spec::SaveScope> {
        self.registry
            .saves
            .iter()
            .map(|entry| entry.scope)
            .collect()
    }

    fn prepare(&mut self, world: &mut ServerWorld) {
        if !self.prepared {
            world.watch_channels(self.registry.watched.clone());
            self.prepared = true;
        }
    }
}

/// Turns what was written to watched channels into events, one for each grid and channel, in the
/// order they first appear.
fn collect_changes(queue: &mut Queue, world: &mut ServerWorld) {
    let changes = world.take_changes();
    if changes.is_empty() || queue.halted {
        return;
    }
    let mut groups: Vec<(GridId, ChannelIndex, Vec<VoxelCoordinate>)> = Vec::new();
    for change in changes {
        match groups
            .iter_mut()
            .find(|group| group.0 == change.grid && group.1 == change.channel)
        {
            Some(group) => group.2.push(change.voxel),
            None => groups.push((change.grid, change.channel, vec![change.voxel])),
        }
    }
    for (grid, channel, voxels) in groups {
        queue.events.push_back(Queued {
            type_id: TypeId::of::<ChannelsChanged>(),
            payload: Box::new(ChannelsChanged {
                grid,
                channel,
                voxels,
            }),
        });
    }
}

/// Delivers every event waiting, and those that delivering them causes.
fn drain(registry: &Registry, queue: &mut Queue, world: &mut ServerWorld, tick: Tick) {
    loop {
        collect_changes(queue, world);
        let Some(event) = queue.events.pop_front() else {
            return;
        };
        if queue.halted {
            queue.events.clear();
            return;
        }
        queue.delivered += 1;
        if queue.delivered > CASCADE_LIMIT {
            queue.halted = true;
            queue.overflows += 1;
            queue.events.clear();
            let _ = world.take_changes();
            return;
        }
        if let Some(handlers) = registry.handlers.get(&(event.type_id, HandlerKind::Notify)) {
            for handler in handlers {
                {
                    let mut run = handler
                        .run
                        .try_borrow_mut()
                        .expect("an event reached a handler that was still running");
                    let mut context = Context {
                        world: &mut *world,
                        registry,
                        queue: &mut *queue,
                        tick,
                        client: None,
                        declared: None,
                    };
                    run(&*event.payload, &mut context);
                }
                collect_changes(queue, world);
            }
        }
    }
}

/// The name of the command the host provides itself.
pub(crate) const HELP_COMMAND: &str = "help";

fn help_spec() -> CommandSpec {
    CommandSpec::new(
        HELP_COMMAND,
        "lists the commands; /help NAME says what one does",
    )
    .argument(Argument::word("command").optional())
}

/// Every command, the host's own `/help` among them (when there are others), in order of name.
fn command_list(registry: &Registry) -> Vec<CommandSpec> {
    if registry.commands.is_empty() {
        return Vec::new();
    }
    let mut specs: Vec<CommandSpec> = registry
        .commands
        .values()
        .map(|entry| entry.spec.clone())
        .collect();
    specs.push(help_spec());
    specs.sort_by_key(CommandSpec::name);
    specs
}

/// The replies to `/help`: a line for every command, or for the one named.
fn help(registry: &Registry, words: &[&str]) -> Vec<String> {
    let specs = command_list(registry);
    let describe = |spec: &CommandSpec| format!("{}: {}", spec.usage(), spec.summary());
    match words {
        [] => specs.iter().map(describe).collect(),
        [wanted] => {
            let wanted = wanted.strip_prefix('/').unwrap_or(wanted);
            match specs.iter().find(|spec| spec.name() == wanted) {
                Some(spec) => vec![describe(spec)],
                None => vec![format!("there is no command {wanted}; /help lists them")],
            }
        }
        _ => vec![describe(&help_spec())],
    }
}

fn dispatch_message(
    registry: &Registry,
    queue: &mut Queue,
    world: &mut ServerWorld,
    tick: Tick,
    client: ClientId,
    kind: &NamespacedId,
    payload: &[u8],
) {
    // The engine's own message: an answer to a question.
    if *kind == PromptAnswer::kind() {
        answer_prompt(registry, queue, world, tick, client, payload);
        return;
    }
    if *kind == UiResponse::kind() {
        respond_to_form(registry, queue, world, tick, client, payload);
        return;
    }
    let Some(run) = registry
        .messages
        .get(kind)
        .and_then(|entry| entry.run.as_ref())
    else {
        return; // another game's, or one the server only sends: ignored
    };
    {
        let mut run = run
            .try_borrow_mut()
            .expect("a message reached a handler that was still running");
        let mut context = Context {
            world: &mut *world,
            registry,
            queue: &mut *queue,
            tick,
            client: Some(client),
            declared: None,
        };
        if !run(&mut context, client, payload) {
            registry.malformed_messages.fetch_add(1, Ordering::Relaxed);
        }
    }
    drain(registry, queue, world, tick);
}

/// A player answers a question: if it is one that was asked of them and the option is one it has, it is over,
/// their client is told so, and the plugins hear the answer. Anything else is ignored (a message that cannot
/// be read is counted, like any other).
fn answer_prompt(
    registry: &Registry,
    queue: &mut Queue,
    world: &mut ServerWorld,
    tick: Tick,
    client: ClientId,
    payload: &[u8],
) {
    let Ok(answer) = PromptAnswer::decode(payload) else {
        registry.malformed_messages.fetch_add(1, Ordering::Relaxed);
        return;
    };
    if !queue.prompts.answer(client, answer.id, answer.choice) {
        return;
    }
    world.send(
        Audience::Client(client),
        PromptClosed::kind(),
        PromptClosed(answer.id).encode(),
    );
    emit_and_drain(
        registry,
        queue,
        world,
        tick,
        PromptAnswered {
            client,
            prompt: answer.id,
            choice: answer.choice as usize,
        },
    );
}

/// A player does something with a form: if the form is open for them and the button is one it has (or the form is modal and
/// they put it away), the plugins hear of it, and a form that was put away is closed on their screen too. Anything else
/// is ignored (a message that cannot be read is counted, like any other).
fn respond_to_form(
    registry: &Registry,
    queue: &mut Queue,
    world: &mut ServerWorld,
    tick: Tick,
    client: ClientId,
    payload: &[u8],
) {
    let Ok(response) = UiResponse::decode(payload) else {
        registry.malformed_messages.fetch_add(1, Ordering::Relaxed);
        return;
    };
    if !queue.forms.respond(client, response.id, response.action) {
        return;
    }
    if response.action == UiAction::Dismissed {
        world.send(
            Audience::Client(client),
            UiClose::kind(),
            UiClose(response.id).encode(),
        );
    }
    emit_and_drain(
        registry,
        queue,
        world,
        tick,
        UiResponded {
            client,
            form: response.id,
            action: response.action,
        },
    );
}

fn emit_and_drain<E: crate::event::Event>(
    registry: &Registry,
    queue: &mut Queue,
    world: &mut ServerWorld,
    tick: Tick,
    event: E,
) {
    queue.events.push_back(Queued {
        type_id: TypeId::of::<E>(),
        payload: Box::new(event),
    });
    drain(registry, queue, world, tick);
}

impl GameLogic for PluginHost {
    fn entity_catalog(&self) -> Arc<EntityCatalog> {
        Arc::clone(&self.registry.catalog)
    }

    fn client_joined(&mut self, client: ClientId, world: &mut ServerWorld) {
        self.prepare(world);
        let tick = world.tick();
        emit_and_drain(
            &self.registry,
            &mut self.queue,
            world,
            tick,
            PlayerJoined { client },
        );
    }

    fn client_left(&mut self, client: ClientId, world: &mut ServerWorld) {
        self.prepare(world);
        self.queue.prompts.forget(client);
        self.queue.forms.forget(client);
        let tick = world.tick();
        emit_and_drain(
            &self.registry,
            &mut self.queue,
            world,
            tick,
            PlayerLeft { client },
        );
    }

    fn message_received(
        &mut self,
        client: ClientId,
        kind: &NamespacedId,
        payload: &[u8],
        world: &mut ServerWorld,
    ) {
        self.prepare(world);
        let tick = world.tick();
        dispatch_message(
            &self.registry,
            &mut self.queue,
            world,
            tick,
            client,
            kind,
            payload,
        );
    }

    fn command_received(&mut self, client: ClientId, text: &str, world: &mut ServerWorld) {
        self.prepare(world);
        let tick = world.tick();
        let Self {
            registry, queue, ..
        } = self;
        let line = CommandLine::parse(text);
        if let Some(line) = &line
            && line.name == HELP_COMMAND
            && !registry.commands.is_empty()
        {
            for reply in help(registry, &line.arguments) {
                world.reply(client, &reply);
            }
            return;
        }
        let known =
            line.and_then(|line| registry.commands.get(line.name).map(|entry| (line, entry)));
        let Some((line, entry)) = known else {
            let usages: Vec<String> = command_list(registry)
                .iter()
                .map(CommandSpec::usage)
                .collect();
            let reply = if usages.is_empty() {
                "this game has no commands".to_owned()
            } else {
                format!("unknown command; try {}", usages.join(", "))
            };
            world.reply(client, &reply);
            return;
        };
        let arguments = match Arguments::parse(&entry.spec, &line.arguments) {
            Ok(arguments) => arguments,
            Err(reply) => {
                world.reply(client, &reply);
                return;
            }
        };
        {
            let mut run = entry
                .run
                .try_borrow_mut()
                .expect("a command reached a handler that was still running");
            let mut context = Context {
                world: &mut *world,
                registry,
                queue: &mut *queue,
                tick,
                client: Some(client),
                declared: None,
            };
            run(&mut context, client, &arguments);
        }
        drain(registry, queue, world, tick);
    }

    fn tick(&mut self, tick: Tick, world: &mut ServerWorld) {
        self.prepare(world);
        self.tick = tick;
        let Self {
            registry, queue, ..
        } = self;
        for phase in Phase::ALL {
            for &index in &registry.run_order[phase.index()] {
                let entry = &registry.systems[index];
                let mut runner = entry
                    .run
                    .try_borrow_mut()
                    .expect("a system was started while it was still running");
                match &mut *runner {
                    Runner::Normal(run) => {
                        let mut context = Context {
                            world: &mut *world,
                            registry,
                            queue: &mut *queue,
                            tick,
                            client: None,
                            declared: Some(&entry.spec),
                        };
                        run(&mut context);
                    }
                    Runner::Mind(run) => {
                        let mut intents: Vec<Intent> = Vec::new();
                        run(&mut MindContext {
                            tick,
                            intents: &mut intents,
                        });
                        drop(runner);
                        for intent in intents {
                            dispatch_message(
                                registry,
                                queue,
                                world,
                                tick,
                                intent.client,
                                &intent.kind,
                                &intent.payload,
                            );
                        }
                        drain(registry, queue, world, tick);
                        continue;
                    }
                }
                drop(runner);
                drain(registry, queue, world, tick);
            }
        }
        queue.delivered = 0;
        queue.halted = false;
    }

    /// What the plugins save, in the order of the sections' identifiers: the bytes of a saved world must
    /// not depend on the order the plugins were listed or registered in.
    fn save_sections(&self) -> Vec<Section> {
        let mut sections: Vec<Section> = self
            .registry
            .saves
            .iter()
            .flat_map(|entry| (entry.write)())
            .collect();
        sections.sort_by(|first, second| first.identifier.cmp(&second.identifier));
        sections
    }
}
