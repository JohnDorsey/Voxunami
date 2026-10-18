use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use namespaced_id::NamespacedId;
use protocol::ClientId;
use voxel_grid::ChannelIndex;
use world_storage::Section;

use crate::context::{Context, MindContext};
use crate::event::Verdict;
use crate::spec::{CommandSpec, SaveScope, SystemSpec};

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

pub(crate) type MessageFn = Box<dyn FnMut(&mut Context<'_>, ClientId, &[u8]) + Send>;
pub(crate) type CommandFn = Box<dyn FnMut(&mut Context<'_>, ClientId, &[&str]) + Send>;

pub(crate) struct MessageEntry {
    pub run: RefCell<MessageFn>,
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
    pub commands: BTreeMap<String, CommandEntry>,
    pub saves: Vec<SaveEntry>,
    /// The channels some handler asked to hear of.
    pub watched: BTreeSet<ChannelIndex>,
}
