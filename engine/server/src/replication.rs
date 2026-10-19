//! Telling clients about entities: for each client, what came into the region it watches, what moved or
//! changed, and what left, as the difference between what it was last told and what is now.
//!
//! What a client is told of an entity is its *view*: the kind, the pose and the components its kind says
//! to replicate. A view is made once for all clients and kept until the entity changes, so a client
//! whose entity did not change costs a pointer comparison.

use std::collections::BTreeMap;
use std::sync::Arc;

use entities::{Entity, EntityCatalog};
use namespaced_id::NamespacedId;
use protocol::{EntityChange, EntityId, EntityState, ServerMessage, write_entity_pose};
use voxel_space::{BrickBox, EntityPose, GridId, LevelIndex};

use crate::server_world::ServerWorld;

/// What clients are told of one entity.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct EntityView {
    kind: NamespacedId,
    pose: EntityPose,
    /// The pose as it is sent, so that two poses are the same only if they would be sent the same.
    pose_bytes: Vec<u8>,
    /// The replicated components, in order of identifier.
    components: Vec<(NamespacedId, Vec<u8>)>,
}

impl EntityView {
    /// The view of an entity, or none if its kind is not one this game has (it is kept, but not shown).
    fn of(entity: &Entity, catalog: &EntityCatalog) -> Option<Self> {
        let spec = catalog.kinds.get(&entity.kind)?;
        let mut components: Vec<(NamespacedId, Vec<u8>)> = spec
            .components()
            .iter()
            .filter(|declaration| declaration.replicated)
            .map(|declaration| {
                let bytes = entity
                    .components
                    .raw(&declaration.id)
                    .unwrap_or(&declaration.default);
                (declaration.id.clone(), bytes.to_vec())
            })
            .collect();
        components.sort_by(|first, second| first.0.cmp(&second.0));
        let mut pose_bytes = binary_codec::ByteWriter::new();
        write_entity_pose(&mut pose_bytes, &entity.pose);
        Some(Self {
            kind: entity.kind.clone(),
            pose: entity.pose,
            pose_bytes: pose_bytes.into_bytes(),
            components,
        })
    }
}

/// The views of entities, kept from tick to tick and made again for the entities that changed.
#[derive(Debug, Default)]
pub(crate) struct ViewCache {
    views: BTreeMap<EntityId, Option<Arc<EntityView>>>,
}

impl ViewCache {
    /// Forgets the views of entities that were made, removed or changed.
    pub(crate) fn invalidate(&mut self, changed: impl IntoIterator<Item = EntityId>) {
        for id in changed {
            self.views.remove(&id);
        }
    }

    fn view(&mut self, world: &ServerWorld, id: EntityId) -> Option<Arc<EntityView>> {
        self.views
            .entry(id)
            .or_insert_with(|| {
                world
                    .entities()
                    .get(id)
                    .and_then(|entity| EntityView::of(entity, world.catalog()))
                    .map(Arc::new)
            })
            .clone()
    }
}

/// What one client has been told of the entities, by id.
pub(crate) type KnownEntities = BTreeMap<EntityId, Arc<EntityView>>;

/// The messages that bring a client's picture of the entities up to date with the world, for a client
/// watching `watched`: those that left or are gone, then those that came in, then those that changed.
/// `known` becomes what the client now knows.
pub(crate) fn entity_messages(
    world: &ServerWorld,
    cache: &mut ViewCache,
    known: &mut KnownEntities,
    watched: Option<(LevelIndex, BrickBox)>,
) -> Vec<ServerMessage> {
    let wanted: Vec<EntityId> = watched
        .map(|(level, region)| world.entities().in_bricks(GridId::root(level), region))
        .unwrap_or_default();
    let mut now: KnownEntities = BTreeMap::new();
    for id in wanted {
        if let Some(view) = cache.view(world, id) {
            now.insert(id, view);
        }
    }

    let gone: Vec<EntityId> = known
        .keys()
        .filter(|id| !now.contains_key(id))
        .copied()
        .collect();
    let mut spawned = Vec::new();
    let mut changed = Vec::new();
    for (&id, view) in &now {
        match known.get(&id) {
            None => spawned.push(EntityState {
                id,
                kind: view.kind.clone(),
                pose: view.pose,
                components: view.components.clone(),
            }),
            Some(before) if Arc::ptr_eq(before, view) => {}
            Some(before) => {
                let pose = (before.pose_bytes != view.pose_bytes).then_some(view.pose);
                let components: Vec<(NamespacedId, Vec<u8>)> = view
                    .components
                    .iter()
                    .filter(|component| !before.components.contains(component))
                    .cloned()
                    .collect();
                if pose.is_some() || !components.is_empty() {
                    changed.push(EntityChange {
                        id,
                        pose,
                        components,
                    });
                }
            }
        }
    }
    *known = now;

    let mut messages = Vec::new();
    if !gone.is_empty() {
        messages.push(ServerMessage::EntitiesDespawned(gone));
    }
    if !spawned.is_empty() {
        messages.push(ServerMessage::EntitiesSpawned(spawned));
    }
    if !changed.is_empty() {
        messages.push(ServerMessage::EntitiesChanged(changed));
    }
    messages
}
