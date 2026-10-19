use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use protocol::Message;

use crate::{EntityError, ItemStack, ItemTable};

/// The most slots an inventory can have; a larger count in a message or a file is damage.
pub const MAXIMUM_SLOTS: usize = 4096;

/// A fixed number of slots, each empty or holding a stack. A component of the things that carry items:
/// a player, a chest, a creature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inventory {
    slots: Vec<Option<ItemStack>>,
}

impl Inventory {
    /// An inventory of `slot_count` empty slots.
    pub fn new(slot_count: usize) -> Self {
        assert!(
            slot_count <= MAXIMUM_SLOTS,
            "an inventory has at most {MAXIMUM_SLOTS} slots"
        );
        Self {
            slots: vec![None; slot_count],
        }
    }

    /// An inventory with exactly these slots. Panics if there are more than [`MAXIMUM_SLOTS`].
    pub fn from_slots(slots: Vec<Option<ItemStack>>) -> Self {
        assert!(
            slots.len() <= MAXIMUM_SLOTS,
            "an inventory has at most {MAXIMUM_SLOTS} slots"
        );
        Self { slots }
    }

    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    pub fn slot(&self, slot: usize) -> Option<&ItemStack> {
        self.slots.get(slot).and_then(Option::as_ref)
    }

    /// Every slot in order.
    pub fn slots(&self) -> &[Option<ItemStack>] {
        &self.slots
    }

    /// How many of an item there are in all, whatever their components.
    pub fn count_of(&self, item: &NamespacedId) -> u64 {
        self.slots
            .iter()
            .flatten()
            .filter(|stack| stack.item == *item)
            .map(|stack| u64::from(stack.count))
            .sum()
    }

    /// Puts a stack in: first onto the stacks it can join, in slot order, then into empty slots, in slot
    /// order, never past the item's stack size. Returns what did not fit, if anything.
    pub fn insert(
        &mut self,
        mut stack: ItemStack,
        items: &ItemTable,
    ) -> Result<Option<ItemStack>, EntityError> {
        let maximum = items.max_stack(&stack.item)?;
        for held in self.slots.iter_mut().flatten() {
            if stack.count == 0 {
                break;
            }
            if held.stacks_with(&stack) {
                let moved = maximum.saturating_sub(held.count).min(stack.count);
                held.count += moved;
                stack.count -= moved;
            }
        }
        for slot in &mut self.slots {
            if stack.count == 0 {
                break;
            }
            if slot.is_none() {
                let moved = maximum.min(stack.count);
                let mut placed = stack.clone();
                placed.count = moved;
                *slot = Some(placed);
                stack.count -= moved;
            }
        }
        Ok((stack.count > 0).then_some(stack))
    }

    /// Takes up to `count` items out of a slot. Returns what was taken (nothing if the slot is empty or
    /// `count` is 0); a slot emptied is empty.
    pub fn take(&mut self, slot: usize, count: u32) -> Result<Option<ItemStack>, EntityError> {
        let slots = self.slots.len();
        let held = self
            .slots
            .get_mut(slot)
            .ok_or(EntityError::NoSuchSlot { slot, slots })?;
        let Some(stack) = held else {
            return Ok(None);
        };
        let taken = count.min(stack.count);
        if taken == 0 {
            return Ok(None);
        }
        let mut result = stack.clone();
        result.count = taken;
        stack.count -= taken;
        if stack.count == 0 {
            *held = None;
        }
        Ok(Some(result))
    }

    /// Exchanges the contents of two slots.
    pub fn swap(&mut self, first: usize, second: usize) -> Result<(), EntityError> {
        let slots = self.slots.len();
        for slot in [first, second] {
            if slot >= slots {
                return Err(EntityError::NoSuchSlot { slot, slots });
            }
        }
        self.slots.swap(first, second);
        Ok(())
    }
}

impl Message for Inventory {
    fn kind() -> NamespacedId {
        "engine:inventory"
            .parse()
            .expect("constant identifier is valid")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.write_varint(self.slots.len() as u64);
        for slot in &self.slots {
            match slot {
                None => writer.write_u8(0),
                Some(stack) => {
                    writer.write_u8(1);
                    stack.write(&mut writer);
                }
            }
        }
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let count = reader.read_count(1)?;
        if count > MAXIMUM_SLOTS {
            return Err(DecodeError::Invalid("an inventory has too many slots"));
        }
        let mut slots = Vec::with_capacity(count);
        for _ in 0..count {
            slots.push(match reader.read_u8()? {
                0 => None,
                1 => Some(ItemStack::read(&mut reader)?),
                _ => return Err(DecodeError::Invalid("a slot is neither empty nor full")),
            });
        }
        reader.finish()?;
        Ok(Self { slots })
    }

    fn samples() -> Vec<Self> {
        let mut full = Self::new(3);
        full.slots[0] = Some(ItemStack::new(
            "base_blocks:stone".parse().expect("valid"),
            64,
        ));
        full.slots[2] = ItemStack::samples().pop();
        vec![Self::new(0), Self::new(9), full]
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::test_support::{inventory, item_stack, item_table};

    fn id(text: &str) -> NamespacedId {
        text.parse().unwrap()
    }

    fn table() -> ItemTable {
        item_table()
    }

    fn total(inventory: &Inventory) -> u64 {
        inventory
            .slots()
            .iter()
            .flatten()
            .map(|s| u64::from(s.count))
            .sum()
    }

    #[test]
    fn items_join_the_stacks_they_can_before_taking_an_empty_slot() {
        let mut bag = Inventory::new(3);
        assert_eq!(
            bag.insert(ItemStack::new(id("test:stone"), 60), &table()),
            Ok(None)
        );
        assert_eq!(
            bag.insert(ItemStack::new(id("test:wood"), 5), &table()),
            Ok(None)
        );
        // 10 more stone: 4 fill the first stack, 6 start a new one.
        assert_eq!(
            bag.insert(ItemStack::new(id("test:stone"), 10), &table()),
            Ok(None)
        );
        let counts: Vec<Option<(String, u32)>> = bag
            .slots()
            .iter()
            .map(|slot| slot.as_ref().map(|s| (s.item.to_string(), s.count)))
            .collect();
        assert_eq!(
            counts,
            [
                Some(("test:stone".to_owned(), 64)),
                Some(("test:wood".to_owned(), 5)),
                Some(("test:stone".to_owned(), 6)),
            ]
        );
    }

    #[test]
    fn what_does_not_fit_is_handed_back_and_nothing_is_lost() {
        let mut bag = Inventory::new(1);
        let back = bag
            .insert(ItemStack::new(id("test:stone"), 100), &table())
            .unwrap()
            .expect("36 do not fit");
        assert_eq!((back.item, back.count), (id("test:stone"), 36));
        assert_eq!(bag.count_of(&id("test:stone")), 64);
        // A full inventory takes nothing.
        let back = bag
            .insert(ItemStack::new(id("test:wood"), 1), &table())
            .unwrap();
        assert_eq!(back.map(|s| s.count), Some(1));
    }

    #[test]
    fn stacks_with_other_components_do_not_merge() {
        let mut bag = Inventory::new(2);
        let plain = ItemStack::new(id("test:stone"), 1);
        let mut marked = ItemStack::new(id("test:stone"), 1);
        marked.components.set_raw(id("test:mark"), vec![1]);
        bag.insert(plain.clone(), &table()).unwrap();
        bag.insert(marked.clone(), &table()).unwrap();
        bag.insert(plain, &table()).unwrap();
        assert_eq!(bag.slot(0).map(|s| s.count), Some(2));
        assert_eq!(bag.slot(1), Some(&marked));
    }

    #[test]
    fn an_item_the_table_lacks_is_refused_and_changes_nothing() {
        let mut bag = Inventory::new(2);
        assert_eq!(
            bag.insert(ItemStack::new(id("test:mystery"), 1), &table()),
            Err(EntityError::UnknownItem(id("test:mystery")))
        );
        assert_eq!(bag, Inventory::new(2));
    }

    #[test]
    fn taking_returns_what_was_taken_and_empties_a_slot_that_runs_out() {
        let mut bag = Inventory::new(2);
        bag.insert(ItemStack::new(id("test:stone"), 10), &table())
            .unwrap();
        assert_eq!(bag.take(0, 4).unwrap().map(|s| s.count), Some(4));
        assert_eq!(bag.slot(0).map(|s| s.count), Some(6));
        assert_eq!(bag.take(0, 100).unwrap().map(|s| s.count), Some(6));
        assert_eq!(bag.slot(0), None);
        assert_eq!(bag.take(0, 1), Ok(None), "an empty slot gives nothing");
        assert_eq!(bag.take(1, 0), Ok(None));
        assert_eq!(
            bag.take(2, 1),
            Err(EntityError::NoSuchSlot { slot: 2, slots: 2 })
        );
    }

    #[test]
    fn swapping_exchanges_two_slots_and_refuses_one_that_is_not_there() {
        let mut bag = Inventory::new(3);
        bag.insert(ItemStack::new(id("test:stone"), 1), &table())
            .unwrap();
        bag.swap(0, 2).unwrap();
        assert_eq!((bag.slot(0), bag.slot(2).map(|s| s.count)), (None, Some(1)));
        assert_eq!(
            bag.swap(0, 3),
            Err(EntityError::NoSuchSlot { slot: 3, slots: 3 })
        );
    }

    #[test]
    fn an_inventory_with_too_many_slots_or_a_damaged_slot_is_refused() {
        let mut writer = ByteWriter::new();
        writer.write_varint(MAXIMUM_SLOTS as u64 + 1);
        writer.write_raw(&vec![0; MAXIMUM_SLOTS + 1]);
        assert!(Inventory::decode(&writer.into_bytes()).is_err());
        assert!(
            Inventory::decode(&[1, 2]).is_err(),
            "a slot that is neither"
        );
    }

    #[test]
    fn the_codec_of_an_inventory_passes_the_generated_check() {
        assert_eq!(protocol::check_message::<Inventory>(), Ok(()));
    }

    #[test]
    fn the_table_of_the_tests_has_the_items_they_use() {
        let table = table();
        for (item, maximum) in [("test:stone", 64), ("test:wood", 64), ("test:pickaxe", 1)] {
            assert_eq!(table.max_stack(&id(item)), Ok(maximum), "{item}");
        }
    }

    proptest! {
        #[test]
        fn inserting_conserves_every_item_and_never_exceeds_a_stack(
            mut bag in inventory(),
            stacks in proptest::collection::vec(item_stack(), 0..12),
        ) {
            let table = table();
            let mut expected = total(&bag);
            for stack in stacks {
                expected += u64::from(stack.count);
                let leftover = bag.insert(stack, &table).unwrap();
                expected -= leftover.map_or(0, |s| u64::from(s.count));
                prop_assert_eq!(total(&bag), expected);
                for held in bag.slots().iter().flatten() {
                    prop_assert!(held.count >= 1);
                    prop_assert!(held.count <= table.max_stack(&held.item).unwrap());
                }
            }
        }

        #[test]
        fn what_is_taken_plus_what_is_left_is_what_there_was(
            mut bag in inventory(),
            slot in 0usize..10,
            count in 0u32..100,
        ) {
            let before = total(&bag);
            let slot = slot % bag.slot_count().max(1);
            if bag.slot_count() > 0 {
                let taken = bag.take(slot, count).unwrap().map_or(0, |s| u64::from(s.count));
                prop_assert_eq!(taken + total(&bag), before);
            }
        }

        #[test]
        fn an_inventory_survives_encoding_and_never_panics_on_damage(
            bag in inventory(),
            noise in proptest::collection::vec(any::<u8>(), 0..64),
        ) {
            prop_assert_eq!(Inventory::decode(&bag.encode()), Ok(bag.clone()));
            let _ = Inventory::decode(&noise);
        }
    }
}
