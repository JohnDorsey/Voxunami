use std::collections::BTreeMap;

use namespaced_id::NamespacedId;
use protocol::Message;

use crate::EntityError;

/// A component an entity kind has: what it is called, whether clients are told of it, and the bytes
/// an entity starts with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentDeclaration {
    pub id: NamespacedId,
    /// Whether the server sends the component to the clients that see the entity. A component that is
    /// only the server's business (what an AI remembers) is not.
    pub replicated: bool,
    pub default: Vec<u8>,
}

/// A kind of entity: its identifier and the components every entity of the kind has.
///
/// Kinds are content, declared by plugins; the identifier is what a saved file and a recording hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityKindSpec {
    id: NamespacedId,
    components: Vec<ComponentDeclaration>,
}

impl EntityKindSpec {
    /// `id` is `plugin:name`. A kind is written by a plugin, so an invalid identifier is a mistake in
    /// the plugin and panics.
    pub fn new(id: &str) -> Self {
        Self {
            id: id
                .parse()
                .unwrap_or_else(|error| panic!("the entity kind {id:?} is not valid: {error}")),
            components: Vec::new(),
        }
    }

    /// Adds a component with the value an entity of the kind starts with. Panics if the kind has the
    /// component already: a kind declares each once.
    pub fn component<C: Message>(mut self, default: C, replicated: bool) -> Self {
        assert!(
            self.declaration(&C::kind()).is_none(),
            "the entity kind {} declares {} twice",
            self.id,
            C::kind()
        );
        self.components.push(ComponentDeclaration {
            id: C::kind(),
            replicated,
            default: default.encode(),
        });
        self
    }

    pub fn id(&self) -> &NamespacedId {
        &self.id
    }

    pub fn components(&self) -> &[ComponentDeclaration] {
        &self.components
    }

    pub fn declaration(&self, component: &NamespacedId) -> Option<&ComponentDeclaration> {
        self.components
            .iter()
            .find(|declaration| declaration.id == *component)
    }
}

/// The entity kinds a game has, by identifier.
#[derive(Clone, Debug, Default)]
pub struct EntityKinds {
    kinds: BTreeMap<NamespacedId, EntityKindSpec>,
}

impl EntityKinds {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, spec: EntityKindSpec) -> Result<(), EntityError> {
        if self.kinds.contains_key(spec.id()) {
            return Err(EntityError::DuplicateKind(spec.id().clone()));
        }
        self.kinds.insert(spec.id().clone(), spec);
        Ok(())
    }

    pub fn get(&self, kind: &NamespacedId) -> Option<&EntityKindSpec> {
        self.kinds.get(kind)
    }

    /// Every kind, in order of identifier.
    pub fn iter(&self) -> impl Iterator<Item = &EntityKindSpec> {
        self.kinds.values()
    }
}

#[cfg(test)]
mod tests {
    use binary_codec::{ByteReader, DecodeError};

    use super::*;

    macro_rules! flag {
        ($name:ident, $id:expr) => {
            #[derive(Debug, PartialEq)]
            struct $name(bool);
            impl Message for $name {
                fn kind() -> NamespacedId {
                    $id.parse().unwrap()
                }
                fn encode(&self) -> Vec<u8> {
                    vec![u8::from(self.0)]
                }
                fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
                    let mut reader = ByteReader::new(payload);
                    let value = reader.read_u8()? == 1;
                    reader.finish()?;
                    Ok(Self(value))
                }
                fn samples() -> Vec<Self> {
                    vec![Self(false), Self(true)]
                }
            }
        };
    }

    flag!(Burning, "test:burning");
    flag!(Frozen, "test:frozen");

    #[test]
    fn a_kind_lists_its_components_with_their_defaults_and_whether_clients_hear_of_them() {
        let spec = EntityKindSpec::new("test:cow")
            .component(Burning(true), true)
            .component(Frozen(false), false);
        assert_eq!(spec.id().to_string(), "test:cow");
        let declared: Vec<(String, bool, Vec<u8>)> = spec
            .components()
            .iter()
            .map(|c| (c.id.to_string(), c.replicated, c.default.clone()))
            .collect();
        assert_eq!(
            declared,
            [
                ("test:burning".to_owned(), true, vec![1]),
                ("test:frozen".to_owned(), false, vec![0])
            ]
        );
        assert!(spec.declaration(&Burning::kind()).is_some());
        assert!(spec.declaration(&"test:other".parse().unwrap()).is_none());
    }

    #[test]
    #[should_panic(expected = "declares test:burning twice")]
    fn a_kind_declares_a_component_once() {
        let _ = EntityKindSpec::new("test:cow")
            .component(Burning(true), true)
            .component(Burning(false), true);
    }

    #[test]
    fn two_kinds_with_one_identifier_are_refused() {
        let mut kinds = EntityKinds::new();
        kinds.register(EntityKindSpec::new("test:cow")).unwrap();
        kinds.register(EntityKindSpec::new("test:pig")).unwrap();
        assert_eq!(
            kinds.register(EntityKindSpec::new("test:cow")),
            Err(EntityError::DuplicateKind("test:cow".parse().unwrap()))
        );
        let order: Vec<String> = kinds.iter().map(|spec| spec.id().to_string()).collect();
        assert_eq!(order, ["test:cow", "test:pig"]);
    }
}
