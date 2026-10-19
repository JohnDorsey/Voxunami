//! Property-test strategies and fixtures for entities. Enabled by the `test-support` feature.

use namespaced_id::NamespacedId;
use proptest::prelude::*;

use crate::{Inventory, ItemSpec, ItemStack, ItemTable};

fn id(text: &str) -> NamespacedId {
    text.parse().expect("the fixtures' identifiers are valid")
}

/// Three items: two that stack to 64 and one that does not stack.
pub fn item_table() -> ItemTable {
    let mut table = ItemTable::new();
    for spec in [
        ItemSpec::new("test:stone", 64),
        ItemSpec::new("test:wood", 64),
        ItemSpec::new("test:pickaxe", 1),
    ] {
        table
            .register(spec)
            .expect("the fixtures' items are distinct");
    }
    table
}

/// Any stack of the fixtures' items, held or being put in: the count may be more than fits in one slot,
/// and a quarter of the stacks carry a component.
pub fn item_stack() -> impl Strategy<Value = ItemStack> {
    (
        prop_oneof![Just("test:stone"), Just("test:wood"), Just("test:pickaxe")],
        1u32..130,
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(item, count, marked, other_mark)| {
            let mut stack = ItemStack::new(id(item), count);
            if marked && other_mark {
                stack.components.set_raw(id("test:mark"), vec![1]);
            }
            stack
        })
}

/// An inventory of up to ten slots, none holding more than its item stacks to.
pub fn inventory() -> impl Strategy<Value = Inventory> {
    proptest::collection::vec(proptest::option::of(item_stack()), 0..10).prop_map(|slots| {
        let table = item_table();
        Inventory::from_slots(
            slots
                .into_iter()
                .map(|slot| {
                    slot.map(|mut stack| {
                        stack.count = stack
                            .count
                            .min(table.max_stack(&stack.item).expect("a fixture item"));
                        stack
                    })
                })
                .collect(),
        )
    })
}
