use std::collections::{BTreeMap, HashMap};

use proptest::prelude::*;

use super::MeshStore;

type Store = MeshStore<u8, u32>;

/// The drawn meshes and the hidden ones, each sorted by key.
fn lists(store: &Store) -> (BTreeMap<u8, u32>, BTreeMap<u8, u32>) {
    (
        store
            .drawn_keys()
            .map(|key| (*key, *store.drawn(key).unwrap()))
            .collect(),
        store
            .hidden_entries()
            .map(|(&key, &value)| (key, value))
            .collect(),
    )
}

#[test]
fn a_new_mesh_is_drawn_unless_it_is_to_be_kept_hidden() {
    let mut store = Store::default();
    assert!(
        store.put(1, 10, true),
        "a mesh that is drawn changes the draw list"
    );
    assert!(!store.put(2, 20, false), "one that is kept hidden does not");
    assert_eq!((store.drawn_len(), store.hidden_len()), (1, 1));
    assert!(store.drawn(&1).is_some() && store.drawn(&2).is_none());
    assert!(store.is_hidden(&2) && !store.is_hidden(&1));
}

#[test]
fn putting_a_mesh_replaces_it_where_it_is_whatever_a_new_one_would_have_done() {
    let mut store = Store::default();
    store.put(1, 10, true);
    store.put(2, 20, false);
    // A drawn mesh stays drawn (and the picture changes), a hidden one stays hidden (and nothing that is drawn changes).
    assert!(store.put(1, 11, false));
    assert!(!store.put(2, 21, true));
    let (drawn, hidden) = lists(&store);
    assert_eq!(drawn, BTreeMap::from([(1, 11)]));
    assert_eq!(hidden, BTreeMap::from([(2, 21)]));
}

#[test]
fn hiding_and_showing_move_a_mesh_between_the_lists_and_say_whether_the_draw_list_changed() {
    let mut store = Store::default();
    store.put(1, 10, true);
    assert!(store.hide(&1));
    assert_eq!(lists(&store), (BTreeMap::new(), BTreeMap::from([(1, 10)])));
    assert!(store.show(&1));
    assert_eq!(lists(&store), (BTreeMap::from([(1, 10)]), BTreeMap::new()));
}

#[test]
fn hiding_what_is_not_drawn_and_showing_what_is_not_hidden_change_nothing() {
    let mut store = Store::default();
    store.put(1, 10, true);
    store.put(2, 20, false);
    let before = lists(&store);
    assert!(!store.hide(&2), "already hidden");
    assert!(!store.hide(&3), "not held");
    assert!(!store.show(&1), "already drawn");
    assert!(!store.show(&3), "not held");
    assert_eq!(lists(&store), before);
}

#[test]
fn removing_forgets_a_mesh_wherever_it_is_and_says_whether_the_draw_list_changed() {
    let mut store = Store::default();
    store.put(1, 10, true);
    store.put(2, 20, false);
    assert!(store.remove(&1));
    assert!(!store.remove(&2), "a hidden mesh was not in the draw list");
    assert!(!store.remove(&3), "nothing was there");
    assert_eq!(lists(&store), (BTreeMap::new(), BTreeMap::new()));
}

#[test]
fn retaining_forgets_in_both_lists_and_says_whether_the_draw_list_changed() {
    let mut store = Store::default();
    for key in 0..4 {
        store.put(key, u32::from(key), key % 2 == 0);
    }
    // Keeps the keys below 2: drawn 0 and hidden 1 stay, drawn 2 and hidden 3 go.
    assert!(store.retain(|key| *key < 2));
    assert_eq!(
        lists(&store),
        (BTreeMap::from([(0, 0)]), BTreeMap::from([(1, 1)]))
    );
    // Only a hidden mesh goes: the draw list is as it was.
    assert!(!store.retain(|key| *key == 0));
    assert_eq!(lists(&store), (BTreeMap::from([(0, 0)]), BTreeMap::new()));
}

/// What a store is asked to do, with meshes told apart by a number that is never repeated (so that putting a mesh always changes it).
#[derive(Clone, Debug)]
enum Step {
    Put(u8, bool),
    Remove(u8),
    Hide(u8),
    Show(u8),
    RetainBelow(u8),
}

fn steps() -> impl Strategy<Value = Vec<Step>> {
    let key = 0u8..6;
    prop::collection::vec(
        prop_oneof![
            (key.clone(), any::<bool>()).prop_map(|(key, drawn)| Step::Put(key, drawn)),
            key.clone().prop_map(Step::Remove),
            key.clone().prop_map(Step::Hide),
            key.clone().prop_map(Step::Show),
            (0u8..8).prop_map(Step::RetainBelow),
        ],
        0..80,
    )
}

proptest! {
    /// Against a model that is a map from each key to its mesh and whether it is drawn: no mesh is in both lists, the lists are what the model says, and each
    /// call says the draw list changed exactly when the meshes drawn are not the ones that were.
    #[test]
    fn the_store_is_a_map_with_a_flag_and_says_when_the_draw_list_changed(steps in steps()) {
        let mut store = Store::default();
        let mut model: HashMap<u8, (u32, bool)> = HashMap::new();
        for (number, step) in steps.into_iter().enumerate() {
            let number = number as u32;
            let drawn_before: BTreeMap<u8, u32> = model
                .iter()
                .filter(|(_, (_, drawn))| *drawn)
                .map(|(&key, &(value, _))| (key, value))
                .collect();
            let said_changed = match step {
                Step::Put(key, new_is_drawn) => {
                    // (Where a mesh is, it stays, whatever a new one would have been.)
                    let drawn = model.get(&key).map_or(new_is_drawn, |&(_, was)| was);
                    model.insert(key, (number, drawn));
                    store.put(key, number, new_is_drawn)
                }
                Step::Remove(key) => {
                    model.remove(&key);
                    store.remove(&key)
                }
                Step::Hide(key) => {
                    if let Some((_, drawn)) = model.get_mut(&key) {
                        *drawn = false;
                    }
                    store.hide(&key)
                }
                Step::Show(key) => {
                    if let Some((_, drawn)) = model.get_mut(&key) {
                        *drawn = true;
                    }
                    store.show(&key)
                }
                Step::RetainBelow(limit) => {
                    model.retain(|&key, _| key < limit);
                    store.retain(|key| *key < limit)
                }
            };
            let drawn_now: BTreeMap<u8, u32> = model
                .iter()
                .filter(|(_, (_, drawn))| *drawn)
                .map(|(&key, &(value, _))| (key, value))
                .collect();
            let hidden_now: BTreeMap<u8, u32> = model
                .iter()
                .filter(|(_, (_, drawn))| !*drawn)
                .map(|(&key, &(value, _))| (key, value))
                .collect();
            prop_assert_eq!(lists(&store), (drawn_now.clone(), hidden_now));
            prop_assert_eq!(said_changed, drawn_before != drawn_now);
        }
    }
}
