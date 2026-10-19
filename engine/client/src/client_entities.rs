use std::collections::BTreeMap;

use binary_codec::DecodeError;
use entities::ComponentBag;
use namespaced_id::NamespacedId;
use protocol::{EntityChange, EntityId, EntityState, Message};
use voxel_space::EntityPose;

use crate::error::ClientError;

/// An entity as a client knows it: what the server last told of it. Only the components the server
/// replicates are here, as the bytes their codecs made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientEntity {
    pub id: EntityId,
    pub kind: NamespacedId,
    pub pose: EntityPose,
    pub components: ComponentBag,
}

impl ClientEntity {
    /// A component, `Ok(None)` if the server does not replicate one of this type for the entity's kind.
    pub fn component<C: Message>(&self) -> Result<Option<C>, DecodeError> {
        self.components.get::<C>()
    }
}

/// The entities a client has been told of, by id, built only from the stream.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientEntities {
    entities: BTreeMap<EntityId, ClientEntity>,
}

impl ClientEntities {
    pub fn get(&self, id: EntityId) -> Option<&ClientEntity> {
        self.entities.get(&id)
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Every entity, in order of id.
    pub fn iter(&self) -> impl Iterator<Item = &ClientEntity> {
        self.entities.values()
    }

    pub(crate) fn spawn(&mut self, entities: Vec<EntityState>) -> Result<(), ClientError> {
        for state in entities {
            if self.entities.contains_key(&state.id) {
                return Err(ClientError::EntityAlreadyKnown(state.id));
            }
            let mut components = ComponentBag::new();
            for (identifier, bytes) in state.components {
                components.set_raw(identifier, bytes);
            }
            self.entities.insert(
                state.id,
                ClientEntity {
                    id: state.id,
                    kind: state.kind,
                    pose: state.pose,
                    components,
                },
            );
        }
        Ok(())
    }

    pub(crate) fn change(&mut self, changes: Vec<EntityChange>) -> Result<(), ClientError> {
        for change in changes {
            let entity = self
                .entities
                .get_mut(&change.id)
                .ok_or(ClientError::UnknownEntity(change.id))?;
            if let Some(pose) = change.pose {
                entity.pose = pose;
            }
            for (identifier, bytes) in change.components {
                entity.components.set_raw(identifier, bytes);
            }
        }
        Ok(())
    }

    pub(crate) fn despawn(&mut self, ids: Vec<EntityId>) -> Result<(), ClientError> {
        for id in ids {
            self.entities
                .remove(&id)
                .ok_or(ClientError::UnknownEntity(id))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use protocol::EntityState;
    use voxel_space::{BrickCoordinate, GridId, Orientation, WorldPosition};

    use super::*;

    fn pose(x: f32) -> EntityPose {
        EntityPose::new(
            GridId::MAIN,
            WorldPosition::new(BrickCoordinate::ORIGIN, [x, 1.0, 1.0]),
            Orientation::IDENTITY,
        )
    }

    fn id(text: &str) -> NamespacedId {
        text.parse().unwrap()
    }

    fn state(raw: u64, x: f32) -> EntityState {
        EntityState {
            id: EntityId::new(raw),
            kind: id("test:cow"),
            pose: pose(x),
            components: vec![(id("test:health"), vec![10, 0, 0, 0])],
        }
    }

    #[test]
    fn what_the_server_says_is_applied_in_order_and_listed_by_id() {
        let mut entities = ClientEntities::default();
        entities.spawn(vec![state(7, 1.0), state(3, 2.0)]).unwrap();
        let order: Vec<u64> = entities.iter().map(|entity| entity.id.get()).collect();
        assert_eq!(order, [3, 7]);
        entities
            .change(vec![
                EntityChange {
                    id: EntityId::new(3),
                    pose: Some(pose(9.0)),
                    components: Vec::new(),
                },
                EntityChange {
                    id: EntityId::new(7),
                    pose: None,
                    components: vec![(id("test:health"), vec![4, 0, 0, 0])],
                },
            ])
            .unwrap();
        let three = entities.get(EntityId::new(3)).unwrap();
        assert_eq!(three.pose, pose(9.0));
        assert_eq!(
            three.components.raw(&id("test:health")),
            Some(&[10, 0, 0, 0][..])
        );
        let seven = entities.get(EntityId::new(7)).unwrap();
        assert_eq!(seven.pose, pose(1.0), "no pose was sent, so it stays");
        assert_eq!(
            seven.components.raw(&id("test:health")),
            Some(&[4, 0, 0, 0][..])
        );
        entities.despawn(vec![EntityId::new(3)]).unwrap();
        assert_eq!(entities.len(), 1);
    }

    #[test]
    fn a_stream_that_contradicts_itself_is_an_error() {
        let mut entities = ClientEntities::default();
        entities.spawn(vec![state(1, 0.0)]).unwrap();
        assert!(matches!(
            entities.spawn(vec![state(1, 0.0)]),
            Err(ClientError::EntityAlreadyKnown(known)) if known == EntityId::new(1)
        ));
        let nobody = EntityId::new(2);
        assert!(matches!(
            entities.change(vec![EntityChange {
                id: nobody,
                pose: None,
                components: Vec::new()
            }]),
            Err(ClientError::UnknownEntity(unknown)) if unknown == nobody
        ));
        assert!(matches!(
            entities.despawn(vec![nobody]),
            Err(ClientError::UnknownEntity(unknown)) if unknown == nobody
        ));
    }

    #[test]
    fn a_component_is_read_with_its_type() {
        let mut entities = ClientEntities::default();
        entities.spawn(vec![state(1, 0.0)]).unwrap();
        let entity = entities.get(EntityId::new(1)).unwrap();
        assert!(entity.components.raw(&id("test:health")).is_some());
        assert_eq!(entity.kind, id("test:cow"));
    }
}
