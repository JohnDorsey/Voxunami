use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use entities::EntityCatalog;
use namespaced_id::NamespacedId;
use protocol::ClientId;
use voxel_grid::ChannelIndex;
use world_storage::Section;

use crate::command::{Arguments, CommandSpec};
use crate::context::{Context, MindContext};
use crate::event::Verdict;
use crate::spec::{SaveScope, SystemSpec};

pub(crate) enum Runner {
    Normal(Box<dyn FnMut(&mut Context<'_>) + Send>),
    Mind(Box<dyn FnMut(&mut MindContext<'_>) + Send>),
}

pub(crate) struct SystemEntry {
    pub spec: SystemSpec,
    pub run: RefCell<Runner>,
}

/// Whether a handler is told of an event or is asked about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum HandlerKind {
    Notify,
    Ask,
}

pub(crate) type HandlerFn = Box<dyn FnMut(&dyn Any, &mut Context<'_>) -> Verdict + Send>;

pub(crate) struct HandlerEntry {
    pub run: RefCell<HandlerFn>,
}

/// Decodes a payload and hands it to the handler; false when the payload does not decode.
pub(crate) type MessageFn = Box<dyn FnMut(&mut Context<'_>, ClientId, &[u8]) -> bool + Send>;
pub(crate) type CommandFn = Box<dyn FnMut(&mut Context<'_>, ClientId, &Arguments<'_>) + Send>;

/// A declared message kind: who handles it (nobody, when the plugin only sends it) and how to test its codec.
pub(crate) struct MessageEntry {
    pub run: Option<RefCell<MessageFn>>,
    pub check: fn() -> Result<(), String>,
}

pub(crate) struct CommandEntry {
    pub spec: CommandSpec,
    pub run: RefCell<CommandFn>,
}

pub(crate) struct SaveEntry {
    pub scope: SaveScope,
    pub write: Box<dyn Fn() -> Vec<Section> + Send>,
}

/// Everything the plugins registered, checked and put in order. Built once.
pub(crate) struct Registry {
    pub systems: Vec<SystemEntry>,
    /// For each phase, the positions in `systems` in the order they run.
    pub run_order: [Vec<usize>; 5],
    /// The handlers of each event kind, in the order they run.
    pub handlers: BTreeMap<(TypeId, HandlerKind), Vec<HandlerEntry>>,
    pub messages: BTreeMap<NamespacedId, MessageEntry>,
    /// How many payloads of handled kinds did not decode and were ignored.
    pub malformed_messages: Arc<AtomicU64>,
    pub commands: BTreeMap<String, CommandEntry>,
    /// The kinds of entity and the items the plugins declared.
    pub catalog: Arc<EntityCatalog>,
    pub saves: Vec<SaveEntry>,
    /// The channels some handler asked to hear of.
    pub watched: BTreeSet<ChannelIndex>,
}
