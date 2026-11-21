//! The meshes the renderer holds of one kind, apart from those it draws.
//!
//! A mesh is **drawn** (in the draw list, which a frame walks, culls and casts shadows from) or **hidden** (held on the graphics card and nothing else). What a frame costs is
//! what is drawn and not what is held (`docs/specs/viewpoints-spec.md`: holding 20,000 meshes that a frame walked cost 2 ms of it, for nothing seen), and a mesh goes from one list
//! to the other by a move, not by being made again. A key is in one list at most.
//!
//! Every call that can change the draw list says whether it did, which is what the shadow maps that are kept are kept against: a mesh that is only held changes nothing they hold.

use std::collections::HashMap;
use std::hash::Hash;

pub(crate) struct MeshStore<K, V> {
    drawn: HashMap<K, V>,
    hidden: HashMap<K, V>,
}

impl<K, V> Default for MeshStore<K, V> {
    fn default() -> Self {
        Self {
            drawn: HashMap::new(),
            hidden: HashMap::new(),
        }
    }
}

impl<K: Hash + Eq + Copy, V> MeshStore<K, V> {
    /// Puts a mesh in the store: a mesh that is there is replaced where it is (drawn or hidden, whatever `new_is_drawn` says), and one that is not is drawn if `new_is_drawn`
    /// and hidden if not. Whether the draw list changed.
    pub(crate) fn put(&mut self, key: K, value: V, new_is_drawn: bool) -> bool {
        if let Some(held) = self.hidden.get_mut(&key) {
            *held = value;
            return false;
        }
        if new_is_drawn || self.drawn.contains_key(&key) {
            self.drawn.insert(key, value);
            true
        } else {
            self.hidden.insert(key, value);
            false
        }
    }

    /// Forgets a mesh wherever it is. Whether the draw list changed.
    pub(crate) fn remove(&mut self, key: &K) -> bool {
        self.hidden.remove(key);
        self.drawn.remove(key).is_some()
    }

    /// Takes a mesh out of the draw list and keeps it. Whether it was drawn.
    pub(crate) fn hide(&mut self, key: &K) -> bool {
        let Some(value) = self.drawn.remove(key) else {
            return false;
        };
        self.hidden.insert(*key, value);
        true
    }

    /// Puts a hidden mesh in the draw list. Whether it was hidden.
    pub(crate) fn show(&mut self, key: &K) -> bool {
        let Some(value) = self.hidden.remove(key) else {
            return false;
        };
        self.drawn.insert(*key, value);
        true
    }

    /// Keeps only the meshes whose key `keep` says yes to, drawn or hidden. Whether the draw list changed.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&K) -> bool) -> bool {
        let drawn_before = self.drawn.len();
        self.drawn.retain(|key, _| keep(key));
        self.hidden.retain(|key, _| keep(key));
        self.drawn.len() != drawn_before
    }

    /// The mesh if it is drawn.
    pub(crate) fn drawn(&self, key: &K) -> Option<&V> {
        self.drawn.get(key)
    }

    /// The keys of the draw list: what a frame walks.
    pub(crate) fn drawn_keys(&self) -> impl Iterator<Item = &K> {
        self.drawn.keys()
    }

    pub(crate) fn drawn_values(&self) -> impl Iterator<Item = &V> {
        self.drawn.values()
    }

    pub(crate) fn drawn_len(&self) -> usize {
        self.drawn.len()
    }

    pub(crate) fn hidden_len(&self) -> usize {
        self.hidden.len()
    }

    pub(crate) fn is_hidden(&self, key: &K) -> bool {
        self.hidden.contains_key(key)
    }

    #[cfg(test)]
    fn hidden_entries(&self) -> impl Iterator<Item = (&K, &V)> {
        self.hidden.iter()
    }
}

#[cfg(test)]
mod tests;
