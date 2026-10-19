use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use protocol::Message;

use crate::Entity;

/// The component that takes an entity out of the picture clients are given: an entity of a kind that has
/// it and whose value is `true` stays in the world and in the save, and is told to nobody. A player whose
/// connection has ended is hidden until their key connects again.
///
/// It is the engine's, not a plugin's: replication reads it. A kind that wants entities to be hideable
/// declares it (not replicated: what is hidden is not sent, and the clients that could see the entity are
/// told it despawned).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hidden(pub bool);

impl Message for Hidden {
    fn kind() -> NamespacedId {
        "engine:hidden"
            .parse()
            .expect("constant identifier is valid")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.write_u8(u8::from(self.0));
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let hidden = match reader.read_u8()? {
            0 => false,
            1 => true,
            _ => return Err(DecodeError::Invalid("a hidden flag is 0 or 1")),
        };
        reader.finish()?;
        Ok(Self(hidden))
    }

    fn samples() -> Vec<Self> {
        vec![Self(false), Self(true)]
    }
}

impl Entity {
    /// Whether the entity is hidden from clients: it has the [`Hidden`] component and it is set. An entity
    /// whose bytes for it cannot be read counts as not hidden, so that a damaged flag shows a thing rather
    /// than losing it.
    pub fn is_hidden(&self) -> bool {
        matches!(self.components.get::<Hidden>(), Ok(Some(Hidden(true))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ComponentBag, EntityKindSpec, EntityKinds, EntityStore};
    use voxel_space::{BrickCoordinate, EntityPose, GridId, Orientation, WorldPosition};

    #[test]
    fn the_flag_is_a_message_that_round_trips() {
        assert_eq!(protocol::check_message::<Hidden>(), Ok(()));
    }

    #[test]
    fn an_entity_is_hidden_only_when_it_has_the_flag_and_it_is_set() {
        let mut kinds = EntityKinds::new();
        kinds
            .register(EntityKindSpec::new("test:ghost").component(Hidden(false), false))
            .unwrap();
        kinds.register(EntityKindSpec::new("test:cow")).unwrap();
        let pose = EntityPose::new(
            GridId::MAIN,
            WorldPosition::new(BrickCoordinate::ORIGIN, [1.0, 1.0, 1.0]),
            Orientation::IDENTITY,
        );
        let mut store = EntityStore::new();
        let ghost = store
            .spawn(
                &kinds,
                &"test:ghost".parse().unwrap(),
                pose,
                ComponentBag::new(),
            )
            .unwrap();
        let cow = store
            .spawn(
                &kinds,
                &"test:cow".parse().unwrap(),
                pose,
                ComponentBag::new(),
            )
            .unwrap();
        assert!(!store.get(ghost).unwrap().is_hidden(), "starts as declared");
        assert!(
            !store.get(cow).unwrap().is_hidden(),
            "a kind without the flag shows"
        );
        store.set_component(&kinds, ghost, &Hidden(true)).unwrap();
        assert!(store.get(ghost).unwrap().is_hidden());
        store.set_component(&kinds, ghost, &Hidden(false)).unwrap();
        assert!(!store.get(ghost).unwrap().is_hidden());
        // Damaged bytes show the entity.
        let mut damaged = store.get(ghost).unwrap().clone();
        damaged.components.set_raw(Hidden::kind(), vec![9]);
        assert!(!damaged.is_hidden());
    }
}
