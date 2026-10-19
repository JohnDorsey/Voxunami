use std::any::Any;

use namespaced_id::NamespacedId;
use protocol::{ClientId, EntityId};
use voxel_grid::ChannelIndex;
use voxel_space::{GridId, VoxelCoordinate};

pub(crate) fn parse_id(text: &str) -> NamespacedId {
    text.parse()
        .unwrap_or_else(|error| panic!("{text:?} is not an identifier: {error}"))
}

/// A message between plugins, delivered synchronously (see [`crate::PluginHost`] for when and in what
/// order). An event is a Rust type, with an identifier that names its kind: engine events have
/// identifiers that start with `engine:`, a plugin's own start with the plugin's namespace. A plugin
/// that sends an event defines it first, with [`crate::Registrar::event`].
///
/// No event is sent per voxel: a change is a list of voxels, so that a later boundary to code in
/// another language is crossed once for many voxels, not once for each.
pub trait Event: Any + Send + 'static {
    fn id() -> NamespacedId;
}

/// Voxels of a channel changed in a grid. Sent to those who asked for the channel (see
/// [`crate::Registrar::on_channels`]); nothing is recorded for a channel nobody asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelsChanged {
    pub grid: GridId,
    pub channel: ChannelIndex,
    /// In the order they changed.
    pub voxels: Vec<VoxelCoordinate>,
}

impl Event for ChannelsChanged {
    fn id() -> NamespacedId {
        parse_id("engine:channels_changed")
    }
}

/// A client finished its handshake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerJoined {
    pub client: ClientId,
}

impl Event for PlayerJoined {
    fn id() -> NamespacedId {
        parse_id("engine:player_joined")
    }
}

/// A client left, however it went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerLeft {
    pub client: ClientId,
}

impl Event for PlayerLeft {
    fn id() -> NamespacedId {
        parse_id("engine:player_left")
    }
}

/// An entity was made. Sent when it is made, so a handler runs with the entity in the world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntitySpawned {
    pub id: EntityId,
    pub kind: NamespacedId,
}

impl Event for EntitySpawned {
    fn id() -> NamespacedId {
        parse_id("engine:entity_spawned")
    }
}

/// An entity was removed. It is already gone from the world when a handler hears of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityDespawned {
    pub id: EntityId,
    pub kind: NamespacedId,
}

impl Event for EntityDespawned {
    fn id() -> NamespacedId {
        parse_id("engine:entity_despawned")
    }
}

/// What a handler of an event that asks first says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    /// Refuse, saying why.
    Refuse(String),
}
