use std::collections::{BTreeMap, BTreeSet};

use namespaced_id::NamespacedId;
use protocol::{EntityId, Message};
use voxel_space::{
    BRICK_COORDINATE_MAXIMUM, BRICK_COORDINATE_MINIMUM, BRICK_EDGE_LENGTH, BrickBox,
    BrickCoordinate, EntityPose, GridId, WorldPosition,
};

use crate::{ComponentBag, EntityError, EntityKinds};

/// A thing in the world that is not a voxel: it has an identity, a kind, a pose, and typed components.
#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    pub id: EntityId,
    pub kind: NamespacedId,
    pub pose: EntityPose,
    pub components: ComponentBag,
}

/// Every entity of a world, by identifier.
///
/// Iteration is always in order of identifier, never of memory or hash, so what a rule does with
/// entities does not depend on the machine. Identifiers come from a counter, never reuse one, and
/// are saved with the world (see [`EntityStore::next_id`]). A spatial index by brick answers "what is
/// near here" without a scan of everything.
#[derive(Clone, Debug)]
pub struct EntityStore {
    entities: BTreeMap<EntityId, Entity>,
    next_id: u64,
    bricks: BTreeMap<(GridId, BrickCoordinate), BTreeSet<EntityId>>,
    /// Entities made, removed or changed since [`EntityStore::take_changed`] was last called.
    changed: BTreeSet<EntityId>,
}

impl Default for EntityStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EntityStore {
    /// An empty store. Identifiers start at 1, so that 0 can mean "none" where a number is needed.
    pub fn new() -> Self {
        Self {
            entities: BTreeMap::new(),
            next_id: 1,
            bricks: BTreeMap::new(),
            changed: BTreeSet::new(),
        }
    }

    /// A store with these entities and this counter, as a save kept them. The entities must be in strictly
    /// increasing order of id, each below the counter, so that an id is never given twice. Nothing is
    /// checked against a kind: an entity of a kind nobody has is kept like any other.
    pub fn restore(next_id: u64, entities: Vec<Entity>) -> Result<Self, EntityError> {
        let mut store = Self::new();
        store.next_id = next_id.max(1);
        let mut previous: Option<EntityId> = None;
        for entity in entities {
            if previous.is_some_and(|last| last >= entity.id) {
                return Err(EntityError::UnorderedEntities);
            }
            if entity.id.get() >= next_id {
                return Err(EntityError::IdBeyondCounter {
                    id: entity.id,
                    next_id,
                });
            }
            previous = Some(entity.id);
            store.index(entity.id, entity.pose);
            store.entities.insert(entity.id, entity);
        }
        Ok(store)
    }

    /// The identifier the next entity will get: what a save keeps.
    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Makes an entity of `kind` at `pose` with the kind's components, those in `overrides` taking
    /// the place of the kind's defaults. The pose's grid is the frame the entity is in: a level's root grid, or a ship (the store does not
    /// know which exist; the world checks that before it asks).
    pub fn spawn(
        &mut self,
        kinds: &EntityKinds,
        kind: &NamespacedId,
        pose: EntityPose,
        overrides: ComponentBag,
    ) -> Result<EntityId, EntityError> {
        let spec = kinds
            .get(kind)
            .ok_or_else(|| EntityError::UnknownKind(kind.clone()))?;
        for (component, _) in overrides.iter() {
            if spec.declaration(component).is_none() {
                return Err(EntityError::UndeclaredComponent {
                    kind: kind.clone(),
                    component: component.clone(),
                });
            }
        }
        let mut components = ComponentBag::new();
        for declaration in spec.components() {
            let bytes = overrides
                .raw(&declaration.id)
                .unwrap_or(&declaration.default);
            components.set_raw(declaration.id.clone(), bytes.to_vec());
        }
        let id = EntityId::new(self.next_id);
        self.next_id = self.next_id.checked_add(1).expect("entity ids ran out");
        self.index(id, pose);
        self.changed.insert(id);
        self.entities.insert(
            id,
            Entity {
                id,
                kind: kind.clone(),
                pose,
                components,
            },
        );
        Ok(id)
    }

    /// Removes an entity and returns it, if there was one. Its identifier is not used again.
    pub fn despawn(&mut self, id: EntityId) -> Option<Entity> {
        let entity = self.entities.remove(&id)?;
        self.unindex(id, entity.pose);
        self.changed.insert(id);
        Some(entity)
    }

    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(&id)
    }

    /// Every entity, in order of identifier.
    pub fn iter(&self) -> impl Iterator<Item = &Entity> {
        self.entities.values()
    }

    /// A component of an entity: `Ok(None)` if the entity does not have one of this type (a kind
    /// without it), an error if there is no such entity or its bytes are not this type.
    pub fn component<C: Message>(&self, id: EntityId) -> Result<Option<C>, EntityError> {
        let entity = self.get(id).ok_or(EntityError::UnknownEntity(id))?;
        entity
            .components
            .get::<C>()
            .map_err(|error| EntityError::UndecodableComponent {
                component: C::kind(),
                reason: error.to_string(),
            })
    }

    /// Sets a component the entity's kind declares.
    pub fn set_component<C: Message>(
        &mut self,
        kinds: &EntityKinds,
        id: EntityId,
        value: &C,
    ) -> Result<(), EntityError> {
        let entity = self
            .entities
            .get_mut(&id)
            .ok_or(EntityError::UnknownEntity(id))?;
        let declared = kinds
            .get(&entity.kind)
            .is_some_and(|spec| spec.declaration(&C::kind()).is_some());
        if !declared {
            return Err(EntityError::UndeclaredComponent {
                kind: entity.kind.clone(),
                component: C::kind(),
            });
        }
        entity.components.set(value);
        self.changed.insert(id);
        Ok(())
    }

    /// Moves an entity, to another place in its frame or into another frame (a level's root grid or a ship).
    pub fn set_pose(&mut self, id: EntityId, pose: EntityPose) -> Result<(), EntityError> {
        let entity = self
            .entities
            .get_mut(&id)
            .ok_or(EntityError::UnknownEntity(id))?;
        let old = std::mem::replace(&mut entity.pose, pose);
        self.changed.insert(id);
        if (old.grid, old.position.brick()) != (pose.grid, pose.position.brick()) {
            self.unindex(id, old);
            self.index(id, pose);
        }
        Ok(())
    }

    /// The entities of `grid` within `radius` voxels of `position`, in order of identifier.
    pub fn near(&self, grid: GridId, position: WorldPosition, radius: f32) -> Vec<EntityId> {
        if !radius.is_finite() || radius < 0.0 {
            return Vec::new();
        }
        let edge = BRICK_EDGE_LENGTH as f32;
        let offset = position.offset();
        let reach = |axis: usize| {
            (
                ((offset[axis] - radius) / edge).floor() as i64,
                ((offset[axis] + radius) / edge).floor() as i64,
            )
        };
        let (x, y, z) = (reach(0), reach(1), reach(2));
        let cells = [x, y, z]
            .iter()
            .map(|(low, high)| u128::try_from(high - low + 1).unwrap_or(0))
            .product::<u128>();
        let within = |entity: &Entity| {
            let to = entity.pose.position.relative_to(position);
            to[0] * to[0] + to[1] * to[1] + to[2] * to[2] <= radius * radius
        };
        let mut found: Vec<EntityId> = Vec::new();
        if cells > self.entities.len() as u128 {
            // A radius reaching more bricks than there are entities: asking each entity is cheaper.
            found.extend(
                self.entities
                    .values()
                    .filter(|entity| entity.pose.grid == grid && within(entity))
                    .map(|entity| entity.id),
            );
        } else {
            for dx in x.0..=x.1 {
                for dy in y.0..=y.1 {
                    for dz in z.0..=z.1 {
                        let Some(brick) = position.brick().checked_offset(dx, dy, dz) else {
                            continue;
                        };
                        let Some(ids) = self.bricks.get(&(grid, brick)) else {
                            continue;
                        };
                        found.extend(ids.iter().filter(|id| within(&self.entities[id])));
                    }
                }
            }
        }
        found.sort();
        found
    }

    /// The entities made, removed or changed (a component set, a pose set) since this was last called,
    /// in order of identifier. What replication uses to know what to look at again.
    pub fn take_changed(&mut self) -> BTreeSet<EntityId> {
        std::mem::take(&mut self.changed)
    }

    /// The entities of `grid` whose bricks are in `region`, in order of identifier. This is what a client
    /// that watches the region is told of.
    pub fn in_bricks(&self, grid: GridId, region: BrickBox) -> Vec<EntityId> {
        // Bricks are ordered by x, then y, then z, so the x range of the region is one range of the map.
        let first = BrickCoordinate::new(
            region.min().x(),
            BRICK_COORDINATE_MINIMUM,
            BRICK_COORDINATE_MINIMUM,
        );
        let last = BrickCoordinate::new(
            region.max().x(),
            BRICK_COORDINATE_MAXIMUM,
            BRICK_COORDINATE_MAXIMUM,
        );
        let mut found: Vec<EntityId> = self
            .bricks
            .range((grid, first)..=(grid, last))
            .filter(|((_, brick), _)| region.contains(*brick))
            .flat_map(|(_, ids)| ids.iter().copied())
            .collect();
        found.sort();
        found
    }

    /// Every entity in the frame of `grid`, whatever brick it is in, in order of identifier. What is told of a ship to those who see it.
    pub fn in_grid(&self, grid: GridId) -> Vec<EntityId> {
        let first = BrickCoordinate::new(
            BRICK_COORDINATE_MINIMUM,
            BRICK_COORDINATE_MINIMUM,
            BRICK_COORDINATE_MINIMUM,
        );
        let last = BrickCoordinate::new(
            BRICK_COORDINATE_MAXIMUM,
            BRICK_COORDINATE_MAXIMUM,
            BRICK_COORDINATE_MAXIMUM,
        );
        let mut found: Vec<EntityId> = self
            .bricks
            .range((grid, first)..=(grid, last))
            .flat_map(|(_, ids)| ids.iter().copied())
            .collect();
        found.sort();
        found
    }

    fn index(&mut self, id: EntityId, pose: EntityPose) {
        self.bricks
            .entry((pose.grid, pose.position.brick()))
            .or_default()
            .insert(id);
    }

    fn unindex(&mut self, id: EntityId, pose: EntityPose) {
        let key = (pose.grid, pose.position.brick());
        if let Some(ids) = self.bricks.get_mut(&key) {
            ids.remove(&id);
            if ids.is_empty() {
                self.bricks.remove(&key);
            }
        }
    }
}
