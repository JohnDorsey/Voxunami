use std::any::TypeId;
use std::fmt;

use namespaced_id::NamespacedId;
use protocol::{ClientId, Tick};
use server::{CommandLine, GameLogic, ServerWorld};
use voxel_grid::ChannelIndex;
use voxel_space::{GridId, VoxelCoordinate};
use world_storage::{Section, WorldData};

use crate::context::{Context, Intent, MindContext, Queue, Queued};
use crate::error::RegistrationError;
use crate::event::{ChannelsChanged, PlayerJoined, PlayerLeft};
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

    /// The usage of every command, by name.
    pub fn command_usages(&self) -> Vec<&'static str> {
        self.registry
            .commands
            .values()
            .map(|entry| entry.spec.usage)
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

fn dispatch_message(
    registry: &Registry,
    queue: &mut Queue,
    world: &mut ServerWorld,
    tick: Tick,
    client: ClientId,
    kind: &NamespacedId,
    payload: &[u8],
) {
    let Some(entry) = registry.messages.get(kind) else {
        return; // another game's, or malformed: ignored
    };
    {
        let mut run = entry
            .run
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
        run(&mut context, client, payload);
    }
    drain(registry, queue, world, tick);
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
        let known = CommandLine::parse(text)
            .and_then(|line| registry.commands.get(line.name).map(|entry| (line, entry)));
        let Some((line, entry)) = known else {
            let usages: Vec<&str> = registry
                .commands
                .values()
                .map(|entry| entry.spec.usage)
                .collect();
            let reply = if usages.is_empty() {
                "this game has no commands".to_owned()
            } else {
                format!("unknown command; try {}", usages.join(", "))
            };
            world.reply(client, &reply);
            return;
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
            run(&mut context, client, &line.arguments);
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

    fn save_sections(&self) -> Vec<Section> {
        self.registry
            .saves
            .iter()
            .flat_map(|entry| (entry.write)())
            .collect()
    }
}
