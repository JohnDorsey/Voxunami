//! Runtime numbering of named content.
//!
//! Content such as block types is named by [`NamespacedId`] in saves and on the wire, but running
//! code wants small dense numbers. A [`Registry`] hands those out as [`RuntimeId`]s in registration
//! order. They are meaningful only within one run of one process: never persist or transmit them
//! without translating through the identifier.
//!
//! When a save mentions content whose plugin is not loaded, the loader registers a *missing*
//! entry for it. The entry keeps the identifier, so data that refers to it is written back
//! unchanged on the next save, but it has no content, so nothing can simulate or render it.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use namespaced_id::NamespacedId;
use rustc_hash::FxHashMap;

/// A small number identifying one entry of a `Registry<T>`. The type parameter keeps ids of
/// different registries (blocks, entities, ...) from being mixed up.
pub struct RuntimeId<T> {
    raw: u32,
    marker: PhantomData<fn() -> T>,
}

impl<T> RuntimeId<T> {
    /// For values stored in voxel channels and wire formats. An id made up this way may not
    /// belong to any entry; lookups report that as absence.
    pub const fn from_raw(raw: u32) -> Self {
        Self {
            raw,
            marker: PhantomData,
        }
    }

    pub const fn raw(self) -> u32 {
        self.raw
    }
}

// Implemented by hand so they do not require `T` to implement the same traits.
impl<T> Clone for RuntimeId<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for RuntimeId<T> {}

impl<T> PartialEq for RuntimeId<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<T> Eq for RuntimeId<T> {}

impl<T> PartialOrd for RuntimeId<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for RuntimeId<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.raw.cmp(&other.raw)
    }
}

impl<T> Hash for RuntimeId<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}

impl<T> fmt::Debug for RuntimeId<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "RuntimeId({})", self.raw)
    }
}

/// One registered name, with its content unless the content's plugin is missing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryEntry<T> {
    identifier: NamespacedId,
    content: Option<T>,
}

impl<T> RegistryEntry<T> {
    pub fn identifier(&self) -> &NamespacedId {
        &self.identifier
    }

    /// `None` for a missing entry.
    pub fn content(&self) -> Option<&T> {
        self.content.as_ref()
    }

    /// True for a placeholder registered because its plugin is not loaded.
    pub fn is_missing(&self) -> bool {
        self.content.is_none()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{identifier} is already registered")]
pub struct AlreadyRegistered {
    pub identifier: NamespacedId,
}

/// Entries numbered 0, 1, 2, ... in registration order.
#[derive(Clone, Debug)]
pub struct Registry<T> {
    entries: Vec<RegistryEntry<T>>,
    runtime_id_by_identifier: FxHashMap<NamespacedId, RuntimeId<T>>,
}

impl<T> Default for Registry<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Registry<T> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            runtime_id_by_identifier: FxHashMap::default(),
        }
    }

    /// Registers new content under the next free runtime id.
    pub fn register(
        &mut self,
        identifier: NamespacedId,
        content: T,
    ) -> Result<RuntimeId<T>, AlreadyRegistered> {
        if self.runtime_id_by_identifier.contains_key(&identifier) {
            return Err(AlreadyRegistered { identifier });
        }
        Ok(self.push_entry(identifier, Some(content)))
    }

    /// Makes sure `identifier` has an entry, adding a content-less placeholder if it has none.
    /// Returns the existing id (known or already missing) without changing it, so this is safe to
    /// call for every identifier a save mentions.
    pub fn register_missing(&mut self, identifier: NamespacedId) -> RuntimeId<T> {
        match self.runtime_id_by_identifier.get(&identifier) {
            Some(&existing) => existing,
            None => self.push_entry(identifier, None),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn runtime_id(&self, identifier: &NamespacedId) -> Option<RuntimeId<T>> {
        self.runtime_id_by_identifier.get(identifier).copied()
    }

    /// `None` if `runtime_id` does not belong to this registry.
    pub fn entry(&self, runtime_id: RuntimeId<T>) -> Option<&RegistryEntry<T>> {
        self.entries.get(runtime_id.raw as usize)
    }

    /// Entries in runtime id order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (RuntimeId<T>, &RegistryEntry<T>)> {
        self.entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (RuntimeId::from_raw(index as u32), entry))
    }

    fn push_entry(&mut self, identifier: NamespacedId, content: Option<T>) -> RuntimeId<T> {
        let raw = u32::try_from(self.entries.len()).expect("fewer than 2^32 registry entries");
        let runtime_id = RuntimeId::from_raw(raw);
        self.runtime_id_by_identifier
            .insert(identifier.clone(), runtime_id);
        self.entries.push(RegistryEntry {
            identifier,
            content,
        });
        runtime_id
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    type TestRegistry = Registry<&'static str>;

    fn id(text: &str) -> NamespacedId {
        text.parse().expect("test identifiers are valid")
    }

    fn registry_with(identifiers: &[&str]) -> TestRegistry {
        let mut registry = TestRegistry::new();
        for text in identifiers {
            registry
                .register(id(text), "content")
                .expect("test identifiers are distinct");
        }
        registry
    }

    #[test]
    fn runtime_ids_are_dense_and_follow_registration_order() {
        let mut registry = TestRegistry::new();
        let stone = registry.register(id("base:stone"), "grey").unwrap();
        let dirt = registry.register(id("base:dirt"), "brown").unwrap();
        let grass = registry.register(id("base:grass"), "green").unwrap();
        assert_eq!([stone.raw(), dirt.raw(), grass.raw()], [0, 1, 2]);
        assert_eq!(registry.len(), 3);
    }

    #[test]
    fn lookups_work_in_both_directions() {
        let mut registry = TestRegistry::new();
        registry.register(id("base:stone"), "grey").unwrap();
        let dirt = registry.register(id("base:dirt"), "brown").unwrap();

        assert_eq!(registry.runtime_id(&id("base:dirt")), Some(dirt));
        assert_eq!(registry.runtime_id(&id("base:sand")), None);
        let entry = registry.entry(dirt).unwrap();
        assert_eq!(entry.identifier(), &id("base:dirt"));
        assert_eq!(entry.content(), Some(&"brown"));
        assert!(!entry.is_missing());
    }

    #[test]
    fn registering_a_duplicate_is_rejected_and_changes_nothing() {
        let mut registry = TestRegistry::new();
        let stone = registry.register(id("base:stone"), "grey").unwrap();

        let error = registry.register(id("base:stone"), "other").unwrap_err();

        assert_eq!(error.identifier, id("base:stone"));
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.entry(stone).unwrap().content(), Some(&"grey"));
    }

    #[test]
    fn missing_entries_are_appended_without_disturbing_existing_runtime_ids() {
        let mut registry = registry_with(&["base:stone", "base:dirt"]);

        let ghost = registry.register_missing(id("gone_plugin:ghost"));

        assert_eq!(ghost.raw(), 2);
        assert_eq!(registry.runtime_id(&id("base:stone")).unwrap().raw(), 0);
        assert_eq!(registry.runtime_id(&id("base:dirt")).unwrap().raw(), 1);
    }

    #[test]
    fn missing_entries_keep_their_identifier_but_have_no_content() {
        let mut registry = TestRegistry::new();
        let ghost = registry.register_missing(id("gone_plugin:ghost"));

        let entry = registry.entry(ghost).unwrap();
        assert!(entry.is_missing());
        assert_eq!(entry.content(), None);
        assert_eq!(entry.identifier(), &id("gone_plugin:ghost"));
    }

    #[test]
    fn registering_missing_content_twice_returns_the_same_id() {
        let mut registry = TestRegistry::new();
        let first = registry.register_missing(id("gone_plugin:ghost"));
        let second = registry.register_missing(id("gone_plugin:ghost"));
        assert_eq!(first, second);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn registering_missing_for_known_content_returns_it_unchanged() {
        let mut registry = TestRegistry::new();
        let stone = registry.register(id("base:stone"), "grey").unwrap();

        assert_eq!(registry.register_missing(id("base:stone")), stone);

        assert_eq!(registry.len(), 1);
        assert_eq!(registry.entry(stone).unwrap().content(), Some(&"grey"));
    }

    #[test]
    fn real_content_cannot_replace_an_existing_placeholder() {
        let mut registry = TestRegistry::new();
        registry.register_missing(id("base:stone"));
        assert!(registry.register(id("base:stone"), "grey").is_err());
    }

    #[test]
    fn runtime_ids_that_belong_to_no_entry_are_absent_rather_than_panicking() {
        let registry = registry_with(&["base:stone"]);
        assert!(registry.entry(RuntimeId::from_raw(1)).is_none());
        assert!(registry.entry(RuntimeId::from_raw(u32::MAX)).is_none());
    }

    #[test]
    fn iteration_follows_runtime_id_order() {
        let mut registry = registry_with(&["base:stone", "base:dirt"]);
        registry.register_missing(id("gone_plugin:ghost"));

        let listed: Vec<_> = registry
            .iter()
            .map(|(runtime_id, entry)| (runtime_id.raw(), entry.identifier().to_string()))
            .collect();

        assert_eq!(
            listed,
            [
                (0, "base:stone".to_owned()),
                (1, "base:dirt".to_owned()),
                (2, "gone_plugin:ghost".to_owned()),
            ]
        );
    }

    #[test]
    fn runtime_id_debug_shows_the_number() {
        assert_eq!(
            format!("{:?}", RuntimeId::<()>::from_raw(7)),
            "RuntimeId(7)"
        );
    }

    proptest! {
        /// Whatever mix of real and placeholder registrations happens, ids stay dense, and
        /// identifiers and runtime ids stay a one-to-one mapping.
        #[test]
        fn identifiers_and_runtime_ids_stay_a_bijection(
            operations in proptest::collection::vec(
                (0usize..12, any::<bool>()),
                0..60,
            )
        ) {
            let identifiers: Vec<NamespacedId> =
                (0..12).map(|index| id(&format!("test:entry_{index}"))).collect();
            let mut registry = TestRegistry::new();
            for (index, as_missing) in operations {
                let identifier = identifiers[index].clone();
                if as_missing {
                    registry.register_missing(identifier);
                } else {
                    let _ = registry.register(identifier, "content");
                }
            }

            for (position, (runtime_id, entry)) in registry.iter().enumerate() {
                prop_assert_eq!(runtime_id.raw() as usize, position);
                prop_assert_eq!(registry.runtime_id(entry.identifier()), Some(runtime_id));
            }
            prop_assert_eq!(registry.iter().count(), registry.len());
        }
    }
}
