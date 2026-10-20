use std::any::TypeId;
use std::cell::RefCell;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use entities::{EntityCatalog, EntityError, EntityKindSpec, ItemSpec};
use namespaced_id::NamespacedId;
use protocol::{ClientId, Message, check_message};
use voxel_grid::ChannelIndex;
use world_storage::{Section, WorldData};

use crate::command::{Arguments, CommandSpec};
use crate::context::{Context, MindContext};
use crate::error::RegistrationError;
use crate::event::{
    ChannelsChanged, EntityDespawned, EntitySpawned, Event, PlayerJoined, PlayerLeft,
    PromptAnswered, UiResponded, Verdict,
};
use crate::order::{Node, order, reachable};
use crate::registry::{
    CommandEntry, HandlerEntry, HandlerKind, MessageEntry, Registry, Runner, SaveEntry, SystemEntry,
};
use crate::spec::{HandlerSpec, Phase, SaveScope, SystemSpec};

/// A unit of a game's server: what it adds to the rules, and nothing it should not touch. In-tree
/// Rust today; the same shape is what a mod loader will fill in.
pub trait Plugin: Send + Sync {
    /// The plugin's identifier, such as `voxunami_cave:fire`.
    fn id(&self) -> NamespacedId;

    /// The plugins that must be registered before this one.
    fn depends_on(&self) -> Vec<NamespacedId> {
        Vec::new()
    }

    /// Registers what the plugin adds. `registrar.world()` is the world the server starts from, which
    /// holds any state the plugin saved last time.
    fn register(&self, registrar: &mut Registrar<'_>);
}

struct PendingSystem {
    plugin: NamespacedId,
    spec: SystemSpec,
    runner: Runner,
}

struct PendingHandler {
    plugin: NamespacedId,
    spec: HandlerSpec,
    event: TypeId,
    event_id: NamespacedId,
    kind: HandlerKind,
    run: crate::registry::HandlerFn,
}

/// What the plugins have registered so far.
#[derive(Default)]
pub(crate) struct Builder {
    events: BTreeMap<TypeId, NamespacedId>,
    systems: Vec<PendingSystem>,
    handlers: Vec<PendingHandler>,
    messages: BTreeMap<NamespacedId, MessageEntry>,
    commands: BTreeMap<String, CommandEntry>,
    catalog: EntityCatalog,
    saves: Vec<SaveEntry>,
    watched: BTreeSet<ChannelIndex>,
    errors: Vec<RegistrationError>,
}

impl Builder {
    pub(crate) fn define_engine_events(&mut self) {
        self.events
            .insert(TypeId::of::<ChannelsChanged>(), ChannelsChanged::id());
        self.events
            .insert(TypeId::of::<PlayerJoined>(), PlayerJoined::id());
        self.events
            .insert(TypeId::of::<PlayerLeft>(), PlayerLeft::id());
        self.events
            .insert(TypeId::of::<PromptAnswered>(), PromptAnswered::id());
        self.events
            .insert(TypeId::of::<UiResponded>(), UiResponded::id());
        self.events
            .insert(TypeId::of::<EntitySpawned>(), EntitySpawned::id());
        self.events
            .insert(TypeId::of::<EntityDespawned>(), EntityDespawned::id());
    }
}

/// What a plugin registers into: the verbs of the server's plugin API.
pub struct Registrar<'a> {
    plugin: NamespacedId,
    world: &'a WorldData,
    builder: &'a mut Builder,
}

impl std::fmt::Debug for Registrar<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Registrar")
            .field("plugin", &self.plugin)
            .finish_non_exhaustive()
    }
}

impl<'a> Registrar<'a> {
    pub(crate) fn new(
        plugin: NamespacedId,
        world: &'a WorldData,
        builder: &'a mut Builder,
    ) -> Self {
        Self {
            plugin,
            world,
            builder,
        }
    }

    /// The plugin being registered.
    pub fn plugin_id(&self) -> &NamespacedId {
        &self.plugin
    }

    /// The world the server starts from.
    pub fn world(&self) -> &WorldData {
        self.world
    }

    /// Defines an event kind this plugin sends. (The engine's own events are defined already.)
    pub fn event<E: Event>(&mut self) {
        if self
            .builder
            .events
            .insert(TypeId::of::<E>(), E::id())
            .is_some()
        {
            self.builder
                .errors
                .push(RegistrationError::DuplicateEvent(E::id()));
        }
    }

    /// Declares a kind of entity: its identifier and the components its entities have.
    pub fn entity_kind(&mut self, spec: EntityKindSpec) {
        if let Err(EntityError::DuplicateKind(kind)) = self.builder.catalog.kinds.register(spec) {
            self.builder
                .errors
                .push(RegistrationError::DuplicateEntityKind(kind));
        }
    }

    /// Declares an item: its identifier and how many fit in a stack.
    pub fn item(&mut self, spec: ItemSpec) {
        if let Err(EntityError::DuplicateItem(item)) = self.builder.catalog.items.register(spec) {
            self.builder
                .errors
                .push(RegistrationError::DuplicateItem(item));
        }
    }

    /// A system: a function run once a tick, in a phase, in the order the specification gives.
    pub fn system(&mut self, spec: SystemSpec, run: impl FnMut(&mut Context<'_>) + Send + 'static) {
        self.builder.systems.push(PendingSystem {
            plugin: self.plugin.clone(),
            spec,
            runner: Runner::Normal(Box::new(run)),
        });
    }

    /// A system of class [`crate::DeterminismClass::Mind`]: it may differ between machines, and so it
    /// can only emit intents.
    pub fn mind_system(
        &mut self,
        spec: SystemSpec,
        run: impl FnMut(&mut MindContext<'_>) + Send + 'static,
    ) {
        self.builder.systems.push(PendingSystem {
            plugin: self.plugin.clone(),
            spec,
            runner: Runner::Mind(Box::new(run)),
        });
    }

    /// A handler told of an event, when it is delivered.
    pub fn on<E: Event>(
        &mut self,
        spec: HandlerSpec,
        mut run: impl FnMut(&E, &mut Context<'_>) + Send + 'static,
    ) {
        self.push_handler::<E>(
            spec,
            HandlerKind::Notify,
            Box::new(move |event, context| {
                let event = event
                    .downcast_ref::<E>()
                    .expect("an event is delivered to handlers of its own type");
                run(event, context);
                Verdict::Allow
            }),
        );
    }

    /// A handler told of changes to voxels of these channels. Nothing is recorded for a channel that
    /// no handler asked for.
    pub fn on_channels(
        &mut self,
        spec: HandlerSpec,
        channels: &[ChannelIndex],
        mut run: impl FnMut(&ChannelsChanged, &mut Context<'_>) + Send + 'static,
    ) {
        self.builder.watched.extend(channels.iter().copied());
        let channels: Vec<ChannelIndex> = channels.to_vec();
        self.on::<ChannelsChanged>(spec, move |event, context| {
            if channels.contains(&event.channel) {
                run(event, context);
            }
        });
    }

    /// A handler asked about an event that asks first (see [`Context::ask`]).
    pub fn on_ask<E: Event>(
        &mut self,
        spec: HandlerSpec,
        mut run: impl FnMut(&E, &mut Context<'_>) -> Verdict + Send + 'static,
    ) {
        self.push_handler::<E>(
            spec,
            HandlerKind::Ask,
            Box::new(move |event, context| {
                let event = event
                    .downcast_ref::<E>()
                    .expect("an event is delivered to handlers of its own type");
                run(event, context)
            }),
        );
    }

    fn push_handler<E: Event>(
        &mut self,
        spec: HandlerSpec,
        kind: HandlerKind,
        run: crate::registry::HandlerFn,
    ) {
        self.builder.handlers.push(PendingHandler {
            plugin: self.plugin.clone(),
            spec,
            event: TypeId::of::<E>(),
            event_id: E::id(),
            kind,
            run,
        });
    }

    /// The handler of a client-to-server message. A kind has one handler, and it is declared by this
    /// call (see [`Registrar::declares_message`]). A payload that does not decode is ignored, and
    /// counted; a message of a kind nobody handles is ignored.
    pub fn message<M: Message>(
        &mut self,
        mut run: impl FnMut(&mut Context<'_>, ClientId, M) + Send + 'static,
    ) {
        self.declare::<M>(Some(RefCell::new(Box::new(
            move |context, client, payload| match M::decode(payload) {
                Ok(message) => {
                    run(context, client, message);
                    true
                }
                Err(_) => false,
            },
        ))));
    }

    /// Declares a message kind this plugin only sends. Every message kind is declared once, so that
    /// [`PluginHost::check_messages`](crate::PluginHost::check_messages) tests its codec.
    pub fn declares_message<M: Message>(&mut self) {
        self.declare::<M>(None);
    }

    fn declare<M: Message>(&mut self, run: Option<RefCell<crate::registry::MessageFn>>) {
        let entry = MessageEntry {
            run,
            check: check_message::<M>,
        };
        match self.builder.messages.entry(M::kind()) {
            Entry::Occupied(taken) => {
                let kind = taken.key().clone();
                self.builder
                    .errors
                    .push(RegistrationError::DuplicateMessage(kind));
            }
            Entry::Vacant(free) => {
                free.insert(entry);
            }
        }
    }

    /// A server command. The host checks a call against the spec's arguments and answers a malformed
    /// one itself; the handler is given the checked arguments. `help` is the host's own command.
    pub fn command(
        &mut self,
        spec: CommandSpec,
        run: impl FnMut(&mut Context<'_>, ClientId, &Arguments<'_>) + Send + 'static,
    ) {
        if spec.name == crate::host::HELP_COMMAND || self.builder.commands.contains_key(spec.name) {
            self.builder
                .errors
                .push(RegistrationError::DuplicateCommand(spec.name.to_owned()));
            return;
        }
        self.builder.commands.insert(
            spec.name.to_owned(),
            CommandEntry {
                spec,
                run: RefCell::new(Box::new(run)),
            },
        );
    }

    /// State this plugin saves with the world: called whenever the world is saved.
    pub fn save_section(
        &mut self,
        scope: SaveScope,
        write: impl Fn() -> Vec<Section> + Send + 'static,
    ) {
        self.builder.saves.push(SaveEntry {
            scope,
            write: Box::new(write),
        });
    }
}

/// The plugins in the order they are registered: dependencies first, and otherwise by identifier,
/// never in the order they were listed.
pub(crate) fn registration_order(
    plugins: &[Box<dyn Plugin>],
) -> Result<Vec<usize>, RegistrationError> {
    let ids: Vec<NamespacedId> = plugins.iter().map(|plugin| plugin.id()).collect();
    for (index, id) in ids.iter().enumerate() {
        if ids[..index].contains(id) {
            return Err(RegistrationError::DuplicatePlugin(id.clone()));
        }
    }
    let mut nodes = Vec::with_capacity(plugins.len());
    for (index, plugin) in plugins.iter().enumerate() {
        let mut after = Vec::new();
        for dependency in plugin.depends_on() {
            let position = ids.iter().position(|id| *id == dependency).ok_or_else(|| {
                RegistrationError::UnknownDependency {
                    plugin: ids[index].clone(),
                    dependency: dependency.clone(),
                }
            })?;
            after.push(position);
        }
        nodes.push(Node {
            key: (ids[index].clone(), ids[index].clone()),
            after,
        });
    }
    order(&nodes).map_err(RegistrationError::DependencyCycle)
}

/// Checks what was registered, puts the systems and handlers in order, and freezes it.
pub(crate) fn finish(builder: Builder) -> Result<Registry, RegistrationError> {
    let Builder {
        events,
        systems,
        handlers,
        messages,
        commands,
        catalog,
        saves,
        watched,
        mut errors,
    } = builder;
    if !errors.is_empty() {
        return Err(errors.remove(0));
    }

    // Systems: unique ids, edges that make sense, an order for each phase, and no unordered pair
    // that uses one channel with a writer among them.
    let mut seen = BTreeSet::new();
    for system in &systems {
        if !seen.insert(system.spec.id.clone()) {
            return Err(RegistrationError::DuplicateSystem(system.spec.id.clone()));
        }
    }
    let mut run_order: [Vec<usize>; 5] = Default::default();
    for phase in Phase::ALL {
        let members: Vec<usize> = (0..systems.len())
            .filter(|&index| systems[index].spec.phase == phase)
            .collect();
        let mut nodes = Vec::with_capacity(members.len());
        for &index in &members {
            let spec = &systems[index].spec;
            let mut after = Vec::new();
            for (relation, targets) in [("after", &spec.after), ("before", &spec.before)] {
                for target in targets {
                    let target_index = systems
                        .iter()
                        .position(|other| other.spec.id == *target)
                        .ok_or_else(|| RegistrationError::UnknownOrderingTarget {
                            item: spec.id.clone(),
                            relation,
                            target: target.clone(),
                        })?;
                    let target_phase = systems[target_index].spec.phase;
                    let wrong_way = match relation {
                        "after" => target_phase > phase,
                        _ => target_phase < phase,
                    };
                    if wrong_way {
                        return Err(RegistrationError::OrderAgainstPhase {
                            item: spec.id.clone(),
                            relation,
                            target: target.clone(),
                        });
                    }
                    if target_phase != phase {
                        continue; // the phases already put them in order
                    }
                    // `after` names what comes before this one. `before` is the other way round, and
                    // is recorded on the target below.
                    if relation == "after" {
                        after.push(
                            members
                                .iter()
                                .position(|&member| member == target_index)
                                .expect("a system of this phase is a member"),
                        );
                    }
                }
            }
            nodes.push(Node {
                key: (systems[index].plugin.clone(), spec.id.clone()),
                after,
            });
        }
        // The `before` edges: a system that must run before another is among that one's predecessors.
        for (position, &index) in members.iter().enumerate() {
            for target in &systems[index].spec.before {
                let target_index = systems
                    .iter()
                    .position(|other| other.spec.id == *target)
                    .expect("checked above");
                if systems[target_index].spec.phase != phase {
                    continue;
                }
                let target_position = members
                    .iter()
                    .position(|&member| member == target_index)
                    .expect("a system of this phase is a member");
                nodes[target_position].after.push(position);
            }
        }
        let sequence = order(&nodes).map_err(RegistrationError::OrderingCycle)?;
        let reach = reachable(&nodes);
        for first in 0..members.len() {
            for second in first + 1..members.len() {
                if reach[first][second] || reach[second][first] {
                    continue;
                }
                let (a, b) = (
                    &systems[members[first]].spec,
                    &systems[members[second]].spec,
                );
                let conflict = a
                    .writes
                    .iter()
                    .find(|channel| b.writes.contains(channel) || b.reads.contains(channel));
                let conflict =
                    conflict.or_else(|| b.writes.iter().find(|channel| a.reads.contains(channel)));
                if let Some(&channel) = conflict {
                    return Err(RegistrationError::AmbiguousOrder {
                        first: a.id.clone(),
                        second: b.id.clone(),
                        channel,
                    });
                }
                let component_conflict = a
                    .component_writes
                    .iter()
                    .find(|component| {
                        b.component_writes.contains(component)
                            || b.component_reads.contains(component)
                    })
                    .or_else(|| {
                        b.component_writes
                            .iter()
                            .find(|component| a.component_reads.contains(component))
                    });
                if let Some(component) = component_conflict {
                    return Err(RegistrationError::AmbiguousComponentOrder {
                        first: a.id.clone(),
                        second: b.id.clone(),
                        component: component.clone(),
                    });
                }
            }
        }
        run_order[phase.index()] = sequence.into_iter().map(|local| members[local]).collect();
    }

    // Handlers: unique ids, a defined event, an order for each event and kind.
    let mut handler_ids = BTreeSet::new();
    for handler in &handlers {
        if !handler_ids.insert(handler.spec.id.clone()) {
            return Err(RegistrationError::DuplicateHandler(handler.spec.id.clone()));
        }
        if !events.contains_key(&handler.event) {
            return Err(RegistrationError::UnknownEvent {
                handler: handler.spec.id.clone(),
                event: handler.event_id.clone(),
            });
        }
    }
    let mut groups: BTreeMap<(TypeId, HandlerKind), Vec<PendingHandler>> = BTreeMap::new();
    for handler in handlers {
        groups
            .entry((handler.event, handler.kind))
            .or_default()
            .push(handler);
    }
    let mut ordered_handlers = BTreeMap::new();
    for (key, group) in groups {
        let mut nodes: Vec<Node> = group
            .iter()
            .map(|handler| Node {
                key: (handler.plugin.clone(), handler.spec.id.clone()),
                after: Vec::new(),
            })
            .collect();
        for (position, handler) in group.iter().enumerate() {
            for (relation, targets) in [
                ("after", &handler.spec.after),
                ("before", &handler.spec.before),
            ] {
                for target in targets {
                    let target_position =
                        group
                            .iter()
                            .position(|other| other.spec.id == *target)
                            .ok_or_else(|| RegistrationError::UnknownOrderingTarget {
                                item: handler.spec.id.clone(),
                                relation,
                                target: target.clone(),
                            })?;
                    if relation == "after" {
                        nodes[position].after.push(target_position);
                    } else {
                        nodes[target_position].after.push(position);
                    }
                }
            }
        }
        let sequence = order(&nodes).map_err(RegistrationError::OrderingCycle)?;
        let mut slots: Vec<Option<PendingHandler>> = group.into_iter().map(Some).collect();
        let entries: Vec<HandlerEntry> = sequence
            .into_iter()
            .map(|position| {
                let handler = slots[position].take().expect("each handler is taken once");
                HandlerEntry {
                    run: RefCell::new(handler.run),
                }
            })
            .collect();
        ordered_handlers.insert(key, entries);
    }

    let systems: Vec<SystemEntry> = systems
        .into_iter()
        .map(|system| SystemEntry {
            spec: system.spec,
            run: RefCell::new(system.runner),
        })
        .collect();
    Ok(Registry {
        systems,
        run_order,
        handlers: ordered_handlers,
        messages,
        malformed_messages: Arc::default(),
        commands,
        catalog: Arc::new(catalog),
        saves,
        watched,
    })
}
