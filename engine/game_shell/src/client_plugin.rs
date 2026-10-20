//! The client's side of a plugin: what it hears from the server, how it adjusts what is drawn, what it
//! says in the HUD, and what a key, a mouse button or a typed `.`-command does.
//!
//! A [`ClientPlugin`] is separate from the server plugin it belongs to (they share an identifier, so
//! that a mod can ship both). Its hooks are pure functions of the server's message stream and the
//! plugin's own state, so that a recording played back shows the same HUD and the same picture: the
//! hooks that heard the stream (`on_message`, `view`, `hud_line`) run in a replay, and the ones that
//! ask for something (keys, mouse buttons, commands) only while playing.

use std::any::Any;
use std::collections::BTreeMap;

use block_models::BlockTable;
use client::{ClientEntities, ClientEntity};
use namespaced_id::NamespacedId;
use player_presence::PlayerState;
use plugin_host::{Arguments, CommandSpec, Prompt};
use protocol::{ClientId, Message, Tick, check_message};
use renderer::Camera;
use winit::keyboard::KeyCode;
use world_view::{RayHit, ViewChannels};

use crate::client_commands::ClientSettings;
use crate::package_data::PackageData;
use crate::session::Mode;

/// The client half of a plugin.
pub trait ClientPlugin: Any + Send + std::fmt::Debug {
    /// The identifier, the same as the server plugin's it belongs to (such as `voxunami_cave:light`).
    fn id(&self) -> NamespacedId;

    /// Registers what the plugin hears, adjusts and answers.
    fn register(&self, registrar: &mut ClientRegistrar<'_>);
}

/// The mouse buttons that do something in a game, while the mouse is captured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    /// The left button.
    Primary,
    /// The right button.
    Secondary,
}

/// What an action is: an identifier (unique among all the actions, so that a later phase can rebind it by
/// that name) and a line saying what it does.
#[derive(Clone, Debug)]
pub struct ActionSpec {
    id: NamespacedId,
    summary: &'static str,
}

impl ActionSpec {
    /// `id` is `plugin:name`, such as `voxunami_cave:break_block`.
    pub fn new(id: &str, summary: &'static str) -> Self {
        Self {
            id: id.parse().unwrap_or_else(|error| {
                panic!("the action identifier {id:?} is not valid: {error}")
            }),
            summary,
        }
    }

    pub fn id(&self) -> &NamespacedId {
        &self.id
    }

    pub fn summary(&self) -> &'static str {
        self.summary
    }
}

/// A question as the screen shows it: `Delete the level? 1) Keep it  2) Delete it  (.choose N)`, and how many
/// more are waiting after it.
pub(crate) fn describe_prompt(prompt: &Prompt, waiting: usize) -> String {
    let options = prompt
        .options
        .iter()
        .enumerate()
        .map(|(index, option)| format!("{}) {option}", index + 1))
        .collect::<Vec<_>>()
        .join("  ");
    let title = if prompt.title.is_empty() {
        String::new()
    } else {
        format!("{} ", prompt.title)
    };
    let more = if waiting > 1 {
        format!(" [{} more]", waiting - 1)
    } else {
        String::new()
    };
    format!("{title}{options}  (.choose N){more}")
}

/// What the client does in answer to an action, after the hook has run.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outgoing {
    Command(String),
    Message(NamespacedId, Vec<u8>),
}

/// What a hook can see of the player's session and ask of the server. A narrow view: it has no
/// connection and no mesher.
pub struct ClientContext<'a> {
    view: SessionView<'a>,
    outgoing: Vec<Outgoing>,
}

/// What a context shows of the session.
pub(crate) struct SessionView<'a> {
    pub camera: &'a Camera,
    pub block_table: &'a BlockTable,
    /// What the server said the world looks like (the table above is made from it).
    pub package: &'a PackageData,
    pub players: &'a BTreeMap<ClientId, PlayerState>,
    /// Every entity the client has been told of.
    pub entities: &'a ClientEntities,
    /// The questions the server is waiting for the player to answer, the oldest first.
    pub prompts: &'a [Prompt],
    pub settings: &'a mut ClientSettings,
    pub target: Option<RayHit>,
    /// The block and state in the targeted voxel.
    pub target_content: Option<(u32, u32)>,
    pub tick: Tick,
    pub mode: Mode,
    pub own_client: ClientId,
}

impl std::fmt::Debug for ClientContext<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ClientContext")
            .field("tick", &self.view.tick)
            .field("mode", &self.view.mode)
            .finish_non_exhaustive()
    }
}

impl<'a> ClientContext<'a> {
    pub(crate) fn new(view: SessionView<'a>) -> Self {
        Self {
            view,
            outgoing: Vec::new(),
        }
    }

    pub(crate) fn into_outgoing(self) -> Vec<Outgoing> {
        self.outgoing
    }

    /// Asks the server to run a command, as if the player had typed it (`/fly`).
    pub fn send_command(&mut self, text: &str) {
        self.outgoing.push(Outgoing::Command(text.to_owned()));
    }

    /// Sends the server a message of the game.
    pub fn send_message<M: Message>(&mut self, message: &M) {
        self.outgoing
            .push(Outgoing::Message(M::kind(), message.encode()));
    }

    /// The solid voxel under the crosshair, within the game's reach.
    pub fn target(&self) -> Option<RayHit> {
        self.view.target
    }

    /// The block and its state in the voxel under the crosshair, as the picture has them.
    pub fn target_block(&self) -> Option<(u32, u32)> {
        self.view.target_content
    }

    pub fn camera(&self) -> &Camera {
        self.view.camera
    }

    /// The last tick the picture is complete for.
    pub fn tick(&self) -> Tick {
        self.view.tick
    }

    pub fn block_table(&self) -> &BlockTable {
        self.view.block_table
    }

    /// The server's definition of a block, if it has one: what colour it is and so on. A block of a data pack the client has never
    /// heard of is here, as is any block of the game.
    pub fn block_definition(&self, block: &NamespacedId) -> Option<&block_models::BlockDefinition> {
        self.view
            .package
            .blocks
            .iter()
            .find(|(identifier, _)| identifier == block)
            .and_then(|(_, definition)| definition.as_ref())
    }

    /// The blocks the server's definitions give a true value for the property `key`, such as a flag a game
    /// reads to know which blocks a player can place, in the order of the blocks. Read from the server's
    /// package, so a block of a pack the client has never heard of is in it.
    pub fn blocks_with(&self, key: &block_models::PropertyKey<bool>) -> Vec<NamespacedId> {
        self.view.package.blocks_with(key)
    }

    /// Where every player is, as far as the server has said.
    pub fn players(&self) -> &BTreeMap<ClientId, PlayerState> {
        self.view.players
    }

    /// The entities the client has been told of: those in the region it watches (the players among them,
    /// the one that is this player with their inventory).
    pub fn entities(&self) -> &ClientEntities {
        self.view.entities
    }

    /// The question the server is waiting for the player to answer (the oldest, if there are several).
    pub fn prompt(&self) -> Option<&Prompt> {
        self.view.prompts.first()
    }

    /// How many questions are waiting, the one shown among them.
    pub fn prompts_waiting(&self) -> usize {
        self.view.prompts.len()
    }

    /// This client's identifier on the server.
    pub fn own_client(&self) -> ClientId {
        self.view.own_client
    }

    pub fn mode(&self) -> Mode {
        self.view.mode
    }

    /// What the window's own commands change (see `.ambient`).
    pub fn settings(&self) -> &ClientSettings {
        self.view.settings
    }

    pub fn settings_mut(&mut self) -> &mut ClientSettings {
        self.view.settings
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ClientRegistrationError {
    #[error("message kind {0} is handled twice")]
    DuplicateMessage(NamespacedId),
    #[error("the action {0} is registered twice")]
    DuplicateAction(NamespacedId),
    #[error("the client command .{0} is registered twice")]
    DuplicateCommand(String),
    #[error("a message does not round-trip: {0}")]
    BadMessage(String),
    #[error("the entity kind {0} is given an appearance twice")]
    DuplicateAppearance(NamespacedId),
}

/// How an entity is drawn until it has a model: as a box. The box is centred `centre_offset` voxels from the
/// entity's position, so that a thing that rests on the ground can be drawn on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Appearance {
    /// How big the box is along each axis, in eighths of a voxel (each is made even and from 2 to 8).
    pub size: [u8; 3],
    /// The palette value it is drawn in: a block's number, to look like that block.
    pub material: u32,
    pub centre_offset: [f32; 3],
}

type MessageHook = Box<dyn FnMut(&[u8]) -> bool + Send>;
type ViewHook = Box<dyn FnMut(ViewChannels) -> ViewChannels + Send>;
type HudHook = Box<dyn FnMut(&ClientContext<'_>) -> Option<String> + Send>;
type OverlayHook = Box<dyn FnMut(&ClientContext<'_>) -> Option<ui::Ui> + Send>;
type AppearanceHook = Box<dyn Fn(&ClientEntity) -> Option<Appearance> + Send>;
type ActionHook = Box<dyn FnMut(&mut ClientContext<'_>) + Send>;
type CommandHook = Box<dyn FnMut(&mut ClientContext<'_>, &Arguments<'_>) -> String + Send>;

struct KeyBinding {
    keys: Vec<KeyCode>,
    action: ActionSpec,
    run: ActionHook,
}

struct MouseBinding {
    button: MouseButton,
    action: ActionSpec,
    run: ActionHook,
}

struct CommandEntry {
    spec: CommandSpec,
    run: CommandHook,
}

/// Everything the client plugins registered, checked and put in order.
#[derive(Default)]
pub(crate) struct ClientHost {
    messages: BTreeMap<NamespacedId, MessageHook>,
    checks: Vec<fn() -> Result<(), String>>,
    views: Vec<ViewHook>,
    huds: Vec<HudHook>,
    overlays: Vec<OverlayHook>,
    appearances: BTreeMap<NamespacedId, AppearanceHook>,
    keys: Vec<KeyBinding>,
    mice: Vec<MouseBinding>,
    commands: BTreeMap<String, CommandEntry>,
    actions: Vec<NamespacedId>,
    malformed_messages: u64,
    errors: Vec<ClientRegistrationError>,
}

impl std::fmt::Debug for ClientHost {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ClientHost")
            .field("messages", &self.messages.len())
            .field("commands", &self.commands.len())
            .finish_non_exhaustive()
    }
}

/// What a client plugin registers into.
pub struct ClientRegistrar<'a> {
    host: &'a mut ClientHost,
}

impl std::fmt::Debug for ClientRegistrar<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ClientRegistrar")
            .finish_non_exhaustive()
    }
}

impl ClientRegistrar<'_> {
    /// Hears a message of the game from the server. A kind has one handler; a payload that does not decode
    /// is ignored (and counted). Runs while playing and while replaying: it is the stream.
    pub fn on_message<M: Message>(&mut self, mut run: impl FnMut(M) + Send + 'static) {
        let hook: MessageHook = Box::new(move |payload| match M::decode(payload) {
            Ok(message) => {
                run(message);
                true
            }
            Err(_) => false,
        });
        if self.host.messages.insert(M::kind(), hook).is_some() {
            self.host
                .errors
                .push(ClientRegistrationError::DuplicateMessage(M::kind()));
        }
        self.host.checks.push(check_message::<M>);
    }

    /// Adjusts what the view draws from, given what the game and the plugins before this one made of it.
    /// Called whenever the view is looked at; a hook that gives the view another light hook returns the
    /// same one while nothing has changed (the view compares them by identity).
    pub fn view(&mut self, run: impl FnMut(ViewChannels) -> ViewChannels + Send + 'static) {
        self.host.views.push(Box::new(run));
    }

    /// How entities of a kind are drawn: a box, from what the client knows of the entity, or none to draw
    /// nothing. Players are drawn by the shell and need none. A kind has one appearance.
    pub fn entity_appearance(
        &mut self,
        kind: NamespacedId,
        run: impl Fn(&ClientEntity) -> Option<Appearance> + Send + 'static,
    ) {
        if self
            .host
            .appearances
            .insert(kind.clone(), Box::new(run))
            .is_some()
        {
            self.host
                .errors
                .push(ClientRegistrationError::DuplicateAppearance(kind));
        }
    }

    /// A line of the HUD, or none. Runs every frame, while playing and replaying.
    pub fn hud_line(
        &mut self,
        run: impl FnMut(&ClientContext<'_>) -> Option<String> + Send + 'static,
    ) {
        self.host.huds.push(Box::new(run));
    }

    /// A panel on the screen, or none: an interface (see [`ui::Ui`]) built from what the client knows, such as the hotbar. Runs every
    /// frame, while playing and replaying, so it must be a pure function of the stream and the plugin's own state, as a HUD
    /// line is. It is shown as a panel of the screen, never a modal one: the player's input stays with the world, so
    /// buttons in it do nothing (a server's form is how a plugin asks for a choice). One that is beyond the limits of an
    /// interface is not shown.
    pub fn overlay(
        &mut self,
        run: impl FnMut(&ClientContext<'_>) -> Option<ui::Ui> + Send + 'static,
    ) {
        self.host.overlays.push(Box::new(run));
    }

    /// An action that is by default on a key. Any number of actions may be on one key; they all run, in
    /// order of plugin identifier and then registration. Runs while playing only.
    pub fn key(
        &mut self,
        key: KeyCode,
        action: ActionSpec,
        run: impl FnMut(&mut ClientContext<'_>) + Send + 'static,
    ) {
        self.keys(&[key], action, run);
    }

    /// An action that is by default on any of several keys (such as `A` and the left arrow).
    pub fn keys(
        &mut self,
        keys: &[KeyCode],
        action: ActionSpec,
        run: impl FnMut(&mut ClientContext<'_>) + Send + 'static,
    ) {
        self.claim(&action);
        self.host.keys.push(KeyBinding {
            keys: keys.to_vec(),
            action,
            run: Box::new(run),
        });
    }

    /// An action that is by default on a mouse button, while the mouse is captured. As for keys.
    pub fn mouse(
        &mut self,
        button: MouseButton,
        action: ActionSpec,
        run: impl FnMut(&mut ClientContext<'_>) + Send + 'static,
    ) {
        self.claim(&action);
        self.host.mice.push(MouseBinding {
            button,
            action,
            run: Box::new(run),
        });
    }

    /// A command the player types with a period (`.ambient`). The host checks the call against the spec,
    /// which should have been made `with_prefix('.')`; the handler answers with the line to show.
    pub fn command(
        &mut self,
        spec: CommandSpec,
        run: impl FnMut(&mut ClientContext<'_>, &Arguments<'_>) -> String + Send + 'static,
    ) {
        let name = spec.name().to_owned();
        let entry = CommandEntry {
            spec,
            run: Box::new(run),
        };
        if self.host.commands.insert(name.clone(), entry).is_some() {
            self.host
                .errors
                .push(ClientRegistrationError::DuplicateCommand(name));
        }
    }

    fn claim(&mut self, action: &ActionSpec) {
        if self.host.actions.contains(&action.id) {
            self.host
                .errors
                .push(ClientRegistrationError::DuplicateAction(action.id.clone()));
        } else {
            self.host.actions.push(action.id.clone());
        }
    }
}

impl ClientHost {
    /// Registers the shell's own client commands, then every plugin's, in order of plugin identifier.
    pub(crate) fn new(plugins: &[Box<dyn ClientPlugin>]) -> Result<Self, ClientRegistrationError> {
        let mut host = Self::default();
        crate::client_commands::register(&mut ClientRegistrar { host: &mut host });
        let mut order: Vec<usize> = (0..plugins.len()).collect();
        order.sort_by_key(|&index| plugins[index].id());
        for index in order {
            plugins[index].register(&mut ClientRegistrar { host: &mut host });
        }
        if !host.errors.is_empty() {
            return Err(host.errors.remove(0));
        }
        for check in &host.checks {
            check().map_err(ClientRegistrationError::BadMessage)?;
        }
        Ok(host)
    }

    /// How an entity is drawn, if the plugins say.
    pub(crate) fn appearance_of(&self, entity: &ClientEntity) -> Option<Appearance> {
        self.appearances.get(&entity.kind)?(entity)
    }

    /// Hands a game message to the plugin that handles its kind. Returns whether one did.
    pub(crate) fn message(&mut self, kind: &NamespacedId, payload: &[u8]) -> bool {
        let Some(hook) = self.messages.get_mut(kind) else {
            return false;
        };
        if !hook(payload) {
            self.malformed_messages += 1;
        }
        true
    }

    /// How many payloads of handled kinds did not decode.
    pub(crate) fn malformed_messages(&self) -> u64 {
        self.malformed_messages
    }

    /// What the view draws from, after every plugin has had its say.
    pub(crate) fn view(&mut self, base: ViewChannels) -> ViewChannels {
        self.views
            .iter_mut()
            .fold(base, |channels, hook| hook(channels))
    }

    /// The panels the plugins want on the screen, in order: those within the limits, made not modal.
    pub(crate) fn overlays(&mut self, context: &ClientContext<'_>) -> Vec<ui::Ui> {
        self.overlays
            .iter_mut()
            .filter_map(|hook| hook(context))
            .map(|mut panel| {
                panel.modal = false;
                panel
            })
            .filter(|panel| panel.check().is_ok())
            .collect()
    }

    /// The HUD lines of the plugins, in order.
    pub(crate) fn hud_lines(&mut self, context: &ClientContext<'_>) -> Vec<String> {
        self.huds
            .iter_mut()
            .filter_map(|hook| hook(context))
            .filter(|line| !line.is_empty())
            .collect()
    }

    /// The question the server is waiting for, if there is one, then the HUD lines of the plugins, joined by ` | `.
    pub(crate) fn hud(&mut self, context: &ClientContext<'_>) -> String {
        // The shell's own line comes first: a question must not be missed.
        let question = context
            .prompt()
            .map(|prompt| describe_prompt(prompt, context.prompts_waiting()));
        question
            .into_iter()
            .chain(self.hud_lines(context))
            .collect::<Vec<_>>()
            .join(" | ")
    }

    pub(crate) fn press_key(&mut self, key: KeyCode, context: &mut ClientContext<'_>) {
        for binding in self
            .keys
            .iter_mut()
            .filter(|binding| binding.keys.contains(&key))
        {
            (binding.run)(context);
        }
    }

    pub(crate) fn press_mouse(&mut self, button: MouseButton, context: &mut ClientContext<'_>) {
        for binding in self
            .mice
            .iter_mut()
            .filter(|binding| binding.button == button)
        {
            (binding.run)(context);
        }
    }

    /// What `.keys` says: each action, where it is by default and what it does, in the order they run.
    fn bindings(&self) -> String {
        let keys = self.keys.iter().map(|binding| {
            let keys: Vec<String> = binding.keys.iter().map(|key| format!("{key:?}")).collect();
            (keys.join("/"), &binding.action)
        });
        let mice = self.mice.iter().map(|binding| {
            (
                format!("{:?} mouse button", binding.button),
                &binding.action,
            )
        });
        let lines: Vec<String> = keys
            .chain(mice)
            .map(|(on, action)| format!("{on}: {} ({})", action.summary, action.id))
            .collect();
        if lines.is_empty() {
            "this game has no actions on keys or mouse buttons".to_owned()
        } else {
            lines.join("; ")
        }
    }

    /// Runs a typed line that starts with a period and returns what to tell the player.
    pub(crate) fn command(&mut self, line: &str, context: &mut ClientContext<'_>) -> String {
        let text = line.trim().trim_start_matches('.');
        let mut words = text.split_whitespace();
        let Some(name) = words.next() else {
            return self.usages();
        };
        let words: Vec<&str> = words.collect();
        let parsed = match self.commands.get(name) {
            Some(entry) => entry.spec.parse(&words),
            None => return format!("unknown client command .{name}; {}", self.usages()),
        };
        let arguments = match parsed {
            Ok(arguments) => arguments,
            Err(reply) => return reply,
        };
        if name == "help" {
            return self.help(arguments.present(0).then(|| arguments.word(0)));
        }
        if name == "keys" {
            return self.bindings();
        }
        let entry = self
            .commands
            .get_mut(name)
            .expect("the command was just found");
        (entry.run)(context, &arguments)
    }

    /// `client commands: .ambient [LEVEL], .help [COMMAND]`.
    pub(crate) fn usages(&self) -> String {
        let usages: Vec<String> = self
            .commands
            .values()
            .map(|entry| entry.spec.usage())
            .collect();
        format!("client commands: {}", usages.join(", "))
    }

    /// What `.help` says: every command with its summary, or the one asked about.
    pub(crate) fn help(&self, wanted: Option<&str>) -> String {
        let describe =
            |entry: &CommandEntry| format!("{}: {}", entry.spec.usage(), entry.spec.summary());
        match wanted {
            None => self.usages(),
            Some(name) => {
                let name = name.trim_start_matches('.');
                match self.commands.get(name) {
                    Some(entry) => describe(entry),
                    None => format!("there is no client command .{name}; {}", self.usages()),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use binary_codec::{ByteReader, ByteWriter, DecodeError};
    use plugin_host::Argument;
    use voxel_grid::{ChannelDefinition, ChannelIndex, GridLayout};

    use super::*;
    use crate::view::camera_at;

    type Log = Arc<Mutex<Vec<String>>>;

    fn note(log: &Log, text: impl Into<String>) {
        log.lock().unwrap().push(text.into());
    }

    fn lines(log: &Log) -> Vec<String> {
        log.lock().unwrap().clone()
    }

    fn id(text: &str) -> NamespacedId {
        text.parse().unwrap()
    }

    /// A client plugin that is whatever a closure registers.
    struct Scripted {
        id: NamespacedId,
        register: Box<dyn Fn(&mut ClientRegistrar<'_>) + Send>,
    }

    impl std::fmt::Debug for Scripted {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(formatter, "Scripted({})", self.id)
        }
    }

    impl ClientPlugin for Scripted {
        fn id(&self) -> NamespacedId {
            self.id.clone()
        }

        fn register(&self, registrar: &mut ClientRegistrar<'_>) {
            (self.register)(registrar);
        }
    }

    fn plugin(
        name: &str,
        register: impl Fn(&mut ClientRegistrar<'_>) + Send + 'static,
    ) -> Box<dyn ClientPlugin> {
        Box::new(Scripted {
            id: id(name),
            register: Box::new(register),
        })
    }

    /// A message that is exactly one number.
    #[derive(Debug, PartialEq)]
    struct Ping(u32);

    impl Message for Ping {
        fn kind() -> NamespacedId {
            id("test:ping")
        }
        fn encode(&self) -> Vec<u8> {
            let mut writer = ByteWriter::new();
            writer.write_u32(self.0);
            writer.into_bytes()
        }
        fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
            let mut reader = ByteReader::new(payload);
            let number = reader.read_u32()?;
            reader.finish()?;
            Ok(Self(number))
        }
        fn samples() -> Vec<Self> {
            vec![Self(0), Self(u32::MAX)]
        }
    }

    /// A message whose decoder takes whatever follows the number.
    #[derive(Debug, PartialEq)]
    struct Sloppy(u32);

    impl Message for Sloppy {
        fn kind() -> NamespacedId {
            id("test:sloppy")
        }
        fn encode(&self) -> Vec<u8> {
            self.0.to_le_bytes().to_vec()
        }
        fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
            Ok(Self(ByteReader::new(payload).read_u32()?))
        }
        fn samples() -> Vec<Self> {
            vec![Self(7)]
        }
    }

    /// Runs `with` on a host made of these plugins, in a context of a player at rest, and returns what the
    /// hooks asked the server for.
    fn with_host<R>(
        plugins: Vec<Box<dyn ClientPlugin>>,
        with: impl FnOnce(&mut ClientHost, &mut ClientContext<'_>) -> R,
    ) -> (R, Vec<Outgoing>) {
        with_host_asked(plugins, &[], with)
    }

    /// The same, with these questions waiting for the player.
    fn with_host_asked<R>(
        plugins: Vec<Box<dyn ClientPlugin>>,
        prompts: &[Prompt],
        with: impl FnOnce(&mut ClientHost, &mut ClientContext<'_>) -> R,
    ) -> (R, Vec<Outgoing>) {
        let camera = camera_at([0.0; 3], 0.0, 0.0, 3);
        let table = BlockTable::of_cubes(&[false]);
        let package = PackageData::for_tests();
        let players = BTreeMap::new();
        let entities = ClientEntities::default();
        let mut settings = ClientSettings::new(0);
        let mut host = ClientHost::new(&plugins).expect("the plugins register");
        let mut context = ClientContext::new(SessionView {
            camera: &camera,
            block_table: &table,
            package: &package,
            players: &players,
            entities: &entities,
            prompts,
            settings: &mut settings,
            target: None,
            target_content: None,
            tick: Tick::ZERO,
            mode: Mode::Playing,
            own_client: ClientId::new(1),
        });
        let result = with(&mut host, &mut context);
        (result, context.into_outgoing())
    }

    fn registration_error(plugins: Vec<Box<dyn ClientPlugin>>) -> Option<ClientRegistrationError> {
        ClientHost::new(&plugins).err()
    }

    #[test]
    fn any_number_of_actions_may_be_on_one_key_and_all_run_in_order_of_plugin_then_registration() {
        let log: Log = Arc::default();
        let make = |name: &'static str, log: &Log| {
            let log = Arc::clone(log);
            plugin(name, move |registrar| {
                for action in ["first", "second"] {
                    let log = Arc::clone(&log);
                    registrar.key(
                        KeyCode::KeyF,
                        ActionSpec::new(&format!("{name}_{action}"), "does something"),
                        move |_| note(&log, format!("{name} {action}")),
                    );
                }
            })
        };
        // Listed in the opposite order to their identifiers.
        let mut plugins = vec![make("test_b:p", &log), make("test_a:p", &log)];
        let other = Arc::clone(&log);
        plugins.push(plugin("test_c:p", move |registrar| {
            let other = Arc::clone(&other);
            registrar.key(KeyCode::KeyG, ActionSpec::new("test_c:g", "g"), move |_| {
                note(&other, "c on g");
            });
        }));
        with_host(plugins, |host, context| {
            host.press_key(KeyCode::KeyF, context);
            host.press_key(KeyCode::KeyH, context);
        });
        assert_eq!(
            lines(&log),
            [
                "test_a:p first",
                "test_a:p second",
                "test_b:p first",
                "test_b:p second"
            ]
        );
    }

    #[test]
    fn mouse_buttons_are_separate_from_keys_and_from_each_other() {
        let log: Log = Arc::default();
        let plugins = vec![plugin("test:p", {
            let log = Arc::clone(&log);
            move |registrar| {
                for (button, action) in [
                    (MouseButton::Primary, "test:left"),
                    (MouseButton::Secondary, "test:right"),
                ] {
                    let log = Arc::clone(&log);
                    registrar.mouse(button, ActionSpec::new(action, "clicks"), move |_| {
                        note(&log, action);
                    });
                }
            }
        })];
        with_host(plugins, |host, context| {
            host.press_mouse(MouseButton::Secondary, context);
            host.press_key(KeyCode::KeyF, context);
            host.press_mouse(MouseButton::Primary, context);
        });
        assert_eq!(lines(&log), ["test:right", "test:left"]);
    }

    #[test]
    fn two_actions_with_one_identifier_or_two_commands_with_one_name_are_refused() {
        let action = |registrar: &mut ClientRegistrar<'_>| {
            registrar.key(KeyCode::KeyF, ActionSpec::new("test:act", "acts"), |_| {});
        };
        assert_eq!(
            registration_error(vec![plugin("test:a", action), plugin("test:b", action)]),
            Some(ClientRegistrationError::DuplicateAction(id("test:act")))
        );
        let command = |registrar: &mut ClientRegistrar<'_>| {
            registrar.command(CommandSpec::new("go", "goes").with_prefix('.'), |_, _| {
                String::new()
            });
        };
        assert_eq!(
            registration_error(vec![plugin("test:a", command), plugin("test:b", command)]),
            Some(ClientRegistrationError::DuplicateCommand("go".to_owned()))
        );
        // The shell's own commands are taken.
        let ambient = |registrar: &mut ClientRegistrar<'_>| {
            registrar.command(
                CommandSpec::new("ambient", "mine").with_prefix('.'),
                |_, _| String::new(),
            );
        };
        assert_eq!(
            registration_error(vec![plugin("test:a", ambient)]),
            Some(ClientRegistrationError::DuplicateCommand(
                "ambient".to_owned()
            ))
        );
        let message = |registrar: &mut ClientRegistrar<'_>| registrar.on_message::<Ping>(|_| {});
        assert_eq!(
            registration_error(vec![plugin("test:a", message), plugin("test:b", message)]),
            Some(ClientRegistrationError::DuplicateMessage(Ping::kind()))
        );
    }

    #[test]
    fn a_message_is_decoded_for_its_handler_and_a_payload_that_does_not_decode_is_counted() {
        let log: Log = Arc::default();
        let plugins = vec![plugin("test:p", {
            let log = Arc::clone(&log);
            move |registrar| {
                let log = Arc::clone(&log);
                registrar.on_message(move |Ping(number)| note(&log, format!("ping {number}")));
            }
        })];
        with_host(plugins, |host, _| {
            assert!(host.message(&Ping::kind(), &Ping(41).encode()));
            assert!(host.message(&Ping::kind(), &[1, 2]));
            assert!(host.message(&Ping::kind(), &[1, 0, 0, 0, 0]));
            assert!(!host.message(&id("test:other"), &Ping(1).encode()));
            assert_eq!(host.malformed_messages(), 2);
        });
        assert_eq!(lines(&log), ["ping 41"]);
    }

    #[test]
    fn a_message_whose_codec_does_not_round_trip_is_refused_when_the_host_is_made() {
        let error = registration_error(vec![plugin("test:p", |registrar| {
            registrar.on_message::<Sloppy>(|_| {});
        })]);
        assert!(
            matches!(&error, Some(ClientRegistrationError::BadMessage(text)) if text.contains("trailing byte")),
            "{error:?}"
        );
    }

    fn two_channels() -> (ChannelIndex, ChannelIndex) {
        let layout = GridLayout::new(
            ["blocks", "light"]
                .iter()
                .map(|name| ChannelDefinition::number(id(&format!("test:{name}")), 0))
                .collect(),
        )
        .unwrap();
        (
            layout.channel_index(&id("test:blocks")).unwrap(),
            layout.channel_index(&id("test:light")).unwrap(),
        )
    }

    #[test]
    fn the_view_is_what_each_plugin_made_of_what_the_one_before_made_in_order_of_plugin() {
        let (blocks, light) = two_channels();
        let plugins = vec![
            plugin("test_b:p", |registrar| {
                registrar.view(|mut channels| {
                    channels.states = channels.light;
                    channels
                });
            }),
            plugin("test_a:p", move |registrar| {
                registrar.view(move |mut channels| {
                    channels.light = Some(light);
                    channels
                });
            }),
        ];
        let base = ViewChannels::new(blocks, None, None);
        let (made, _) = with_host(plugins, |host, _| host.view(base));
        // test_a runs first and sets the light, then test_b copies the light to the states.
        assert_eq!(made.light, Some(light));
        assert_eq!(made.states, Some(light));
    }

    #[test]
    fn hud_lines_are_joined_in_order_of_plugin_and_a_plugin_with_nothing_to_say_adds_nothing() {
        let plugins = vec![
            plugin("test_b:p", |registrar| {
                registrar.hud_line(|context| Some(format!("tick {}", context.tick().get())));
            }),
            plugin("test_a:p", |registrar| {
                registrar.hud_line(|_| Some("placing stone".to_owned()));
                registrar.hud_line(|_| None);
                registrar.hud_line(|_| Some(String::new()));
            }),
        ];
        let (hud, _) = with_host(plugins, |host, context| host.hud(context));
        assert_eq!(hud, "placing stone | tick 0");
    }

    fn panel_of(text: &str, modal: bool) -> ui::Ui {
        ui::Ui {
            modal,
            anchor: ui::Anchor::Bottom,
            root: ui::Panel {
                title: None,
                color: ui::Color::BLACK,
                direction: ui::Direction::Row,
                children: vec![ui::Node::Label {
                    text: text.to_owned(),
                    color: ui::Color::WHITE,
                }],
            },
        }
    }

    #[test]
    fn the_panels_of_plugins_come_in_order_of_plugin_never_modal_and_those_beyond_the_limits_are_left_out()
     {
        let plugins = vec![
            plugin("test_b:p", |registrar| {
                registrar.overlay(|context| {
                    Some(panel_of(&format!("tick {}", context.tick().get()), false))
                });
            }),
            plugin("test_a:p", |registrar| {
                // One that wants the input: it is shown, but does not take it. One with nothing to show. One too long to be one.
                registrar.overlay(|_| Some(panel_of("first", true)));
                registrar.overlay(|_| None);
                registrar.overlay(|_| {
                    Some(panel_of(
                        &"x".repeat(ui::MAXIMUM_TEXT_CHARACTERS + 1),
                        false,
                    ))
                });
            }),
        ];
        let (panels, _) = with_host(plugins, |host, context| host.overlays(context));
        assert_eq!(
            panels,
            [panel_of("first", false), panel_of("tick 0", false)]
        );
    }

    #[test]
    fn a_plugin_can_ask_what_colour_the_server_says_a_block_is() {
        let mut package = PackageData::for_tests();
        package.blocks = vec![
            (
                id("a:red"),
                Some(block_models::BlockDefinition::cube([1.0, 0.0, 0.0])),
            ),
            (id("b:gone"), None),
        ];
        let camera = camera_at([0.0; 3], 0.0, 0.0, 3);
        let table = BlockTable::of_cubes(&[false]);
        let (players, entities) = (BTreeMap::new(), ClientEntities::default());
        let mut settings = ClientSettings::new(0);
        let context = ClientContext::new(SessionView {
            camera: &camera,
            block_table: &table,
            package: &package,
            players: &players,
            entities: &entities,
            prompts: &[],
            settings: &mut settings,
            target: None,
            target_content: None,
            tick: Tick::ZERO,
            mode: Mode::Playing,
            own_client: ClientId::new(1),
        });
        assert_eq!(
            context
                .block_definition(&id("a:red"))
                .map(|definition| definition.color),
            Some([1.0, 0.0, 0.0])
        );
        assert!(
            context.block_definition(&id("b:gone")).is_none(),
            "no definition"
        );
        assert!(context.block_definition(&id("c:unknown")).is_none());
    }

    fn question(id: u64, title: &str, options: &[&str]) -> Prompt {
        Prompt {
            id: plugin_host::PromptId::new(id),
            title: title.to_owned(),
            options: options.iter().map(|option| (*option).to_owned()).collect(),
        }
    }

    #[test]
    fn a_question_is_the_first_thing_on_the_screen_with_its_options_and_how_many_more_wait() {
        let plugins = || {
            vec![plugin("test:p", |registrar| {
                registrar.hud_line(|_| Some("placing stone".to_owned()));
            })]
        };
        let (none, _) = with_host_asked(plugins(), &[], |host, context| host.hud(context));
        assert_eq!(none, "placing stone", "no question, no line of it");

        let delete = question(1, "Delete the level?", &["Keep it", "Delete it"]);
        let (one, _) =
            with_host_asked(plugins(), std::slice::from_ref(&delete), |host, context| {
                host.hud(context)
            });
        assert_eq!(
            one,
            "Delete the level? 1) Keep it  2) Delete it  (.choose N) | placing stone"
        );

        let other = question(2, "", &["Yes"]);
        let (two, _) = with_host_asked(plugins(), &[delete, other], |host, context| {
            host.hud(context)
        });
        assert_eq!(
            two, "Delete the level? 1) Keep it  2) Delete it  (.choose N) [1 more] | placing stone",
            "the oldest is shown"
        );
        let (untitled, _) = with_host_asked(
            Vec::new(),
            &[question(3, "", &["Yes", "No"])],
            |host, context| host.hud(context),
        );
        assert_eq!(untitled, "1) Yes  2) No  (.choose N)");
    }

    #[test]
    fn what_an_action_asks_of_the_server_is_collected_for_the_session_to_send() {
        let plugins = vec![plugin("test:p", |registrar| {
            registrar.key(
                KeyCode::KeyF,
                ActionSpec::new("test:fly", "flies"),
                |context| {
                    context.send_command("/fly");
                    context.send_message(&Ping(5));
                },
            );
        })];
        let ((), outgoing) = with_host(plugins, |host, context| {
            host.press_key(KeyCode::KeyF, context);
        });
        assert_eq!(
            outgoing,
            [
                Outgoing::Command("/fly".to_owned()),
                Outgoing::Message(Ping::kind(), Ping(5).encode())
            ]
        );
    }

    #[test]
    fn a_plugins_command_gets_typed_arguments_and_a_malformed_call_is_answered_with_the_usage() {
        let plugins = vec![plugin("test:p", |registrar| {
            registrar.command(
                CommandSpec::new("pick", "picks a block")
                    .with_prefix('.')
                    .argument(Argument::integer("slot")),
                |_, arguments| format!("picked {}", arguments.integer(0)),
            );
        })];
        let (replies, _) = with_host(plugins, |host, context| {
            [".pick 3", ".pick", ".pick x", ".pick 1 2", "pick 4"]
                .map(|line| host.command(line, context))
        });
        assert_eq!(
            replies,
            [
                "picked 3",
                "usage: .pick SLOT",
                "usage: .pick SLOT; SLOT must be a whole number",
                "usage: .pick SLOT",
                "picked 4"
            ]
        );
    }

    #[test]
    fn keys_lists_every_action_with_where_it_is_and_what_it_does() {
        let plugins = vec![plugin("test:p", |registrar| {
            registrar.key(KeyCode::KeyF, ActionSpec::new("test:fly", "flies"), |_| {});
            registrar.mouse(
                MouseButton::Primary,
                ActionSpec::new("test:hit", "hits"),
                |_| {},
            );
        })];
        let (keys, _) = with_host(plugins, |host, context| host.command(".keys", context));
        assert_eq!(
            keys,
            "KeyF: flies (test:fly); Primary mouse button: hits (test:hit)"
        );
        let (none, _) = with_host(Vec::new(), |host, context| host.command(".keys", context));
        assert_eq!(none, "this game has no actions on keys or mouse buttons");
    }

    fn an_entity_of(kind: &str) -> ClientEntity {
        ClientEntity {
            id: protocol::EntityId::new(1),
            kind: id(kind),
            pose: voxel_space::EntityPose::new(
                voxel_space::GridId::MAIN,
                voxel_space::WorldPosition::new(voxel_space::BrickCoordinate::ORIGIN, [1.0; 3]),
                voxel_space::Orientation::IDENTITY,
            ),
            components: entities::ComponentBag::new(),
        }
    }

    #[test]
    fn an_appearance_is_given_to_the_entities_of_its_kind_and_to_no_others() {
        let plugins = vec![plugin("test:a", |registrar| {
            registrar.entity_appearance(id("test:rock"), |entity| {
                Some(Appearance {
                    size: [4, 4, 4],
                    material: entity.id.get() as u32 + 2,
                    centre_offset: [0.0, -0.25, 0.0],
                })
            });
            // One that says nothing for an entity it will not draw.
            registrar.entity_appearance(id("test:hidden"), |_| None);
        })];
        let host = ClientHost::new(&plugins).unwrap();
        assert_eq!(
            host.appearance_of(&an_entity_of("test:rock")),
            Some(Appearance {
                size: [4, 4, 4],
                material: 3,
                centre_offset: [0.0, -0.25, 0.0],
            })
        );
        assert_eq!(host.appearance_of(&an_entity_of("test:hidden")), None);
        assert_eq!(host.appearance_of(&an_entity_of("test:other")), None);
    }

    #[test]
    fn a_kind_is_given_an_appearance_once() {
        let appearance = |registrar: &mut ClientRegistrar<'_>| {
            registrar.entity_appearance(id("test:rock"), |_| None);
        };
        assert_eq!(
            registration_error(vec![
                plugin("test:a", appearance),
                plugin("test:b", appearance)
            ]),
            Some(ClientRegistrationError::DuplicateAppearance(id(
                "test:rock"
            )))
        );
    }

    #[test]
    fn a_plugin_can_ask_which_blocks_the_servers_definitions_give_a_flag_to() {
        use block_models::{BlockDefinition, PropertyKey};
        const FLAG: PropertyKey<bool> = PropertyKey::new("test:flagged", false, "a flag");
        let mut package = PackageData::for_tests();
        package.properties.declare(FLAG.declaration()).unwrap();
        let plain = BlockDefinition::cube([0.0; 3]);
        package.blocks = vec![
            (id("a:one"), Some(plain.clone().with_property(&FLAG, true))),
            (id("a:two"), Some(plain.clone())),
            (id("b:three"), None),
            (id("b:four"), Some(plain.clone().with_property(&FLAG, true))),
            (id("b:five"), Some(plain.with_property(&FLAG, false))),
        ];
        let camera = camera_at([0.0; 3], 0.0, 0.0, 3);
        let table = BlockTable::of_cubes(&[false]);
        let (players, entities) = (BTreeMap::new(), ClientEntities::default());
        let mut settings = ClientSettings::new(0);
        let context = ClientContext::new(SessionView {
            camera: &camera,
            block_table: &table,
            package: &package,
            players: &players,
            entities: &entities,
            prompts: &[],
            settings: &mut settings,
            target: None,
            target_content: None,
            tick: Tick::ZERO,
            mode: Mode::Playing,
            own_client: ClientId::new(1),
        });
        assert_eq!(context.blocks_with(&FLAG), [id("a:one"), id("b:four")]);
    }
}
