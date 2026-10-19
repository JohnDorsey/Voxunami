use std::collections::BTreeMap;

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use protocol::Message;

use crate::{ComponentBag, EntityError};

/// A kind of item: its identifier and how many fit in one stack.
///
/// Items are content, declared by plugins (what an item does is a property of its own, set by the
/// plugin that cares); the identifier is what a saved file holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemSpec {
    id: NamespacedId,
    max_stack: u32,
}

impl ItemSpec {
    /// `id` is `plugin:name`. A plugin writes its items, so an invalid identifier or a maximum stack
    /// of 0 is a mistake in the plugin and panics.
    pub fn new(id: &str, max_stack: u32) -> Self {
        assert!(max_stack >= 1, "the item {id} must stack to at least 1");
        Self {
            id: id
                .parse()
                .unwrap_or_else(|error| panic!("the item {id:?} is not valid: {error}")),
            max_stack,
        }
    }

    pub fn id(&self) -> &NamespacedId {
        &self.id
    }

    pub fn max_stack(&self) -> u32 {
        self.max_stack
    }
}

/// The items a game has, by identifier.
#[derive(Clone, Debug, Default)]
pub struct ItemTable {
    items: BTreeMap<NamespacedId, ItemSpec>,
}

impl ItemTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, spec: ItemSpec) -> Result<(), EntityError> {
        if self.items.contains_key(spec.id()) {
            return Err(EntityError::DuplicateItem(spec.id().clone()));
        }
        self.items.insert(spec.id().clone(), spec);
        Ok(())
    }

    pub fn get(&self, item: &NamespacedId) -> Option<&ItemSpec> {
        self.items.get(item)
    }

    /// How many of the item fit in a stack.
    pub fn max_stack(&self, item: &NamespacedId) -> Result<u32, EntityError> {
        self.get(item)
            .map(ItemSpec::max_stack)
            .ok_or_else(|| EntityError::UnknownItem(item.clone()))
    }

    /// Every item, in order of identifier.
    pub fn iter(&self) -> impl Iterator<Item = &ItemSpec> {
        self.items.values()
    }
}

/// Some of one kind of item together: how many, and any components the stack carries (a tool's wear).
/// Stacks are equal, and so may be merged, only when their components are equal as well.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemStack {
    pub item: NamespacedId,
    pub count: u32,
    pub components: ComponentBag,
}

impl ItemStack {
    /// A stack of `count` items with no components. Panics on a count of 0: a stack holds something.
    pub fn new(item: NamespacedId, count: u32) -> Self {
        assert!(count >= 1, "an item stack must hold at least one item");
        Self {
            item,
            count,
            components: ComponentBag::new(),
        }
    }

    /// Whether `other` is the same sort of stack: the same item with the same components.
    pub fn stacks_with(&self, other: &Self) -> bool {
        self.item == other.item && self.components == other.components
    }

    pub(crate) fn write(&self, writer: &mut ByteWriter) {
        writer.write_identifier(&self.item);
        writer.write_varint(u64::from(self.count));
        self.components.write(writer);
    }

    pub(crate) fn read(reader: &mut ByteReader<'_>) -> Result<Self, DecodeError> {
        let item = reader.read_identifier()?;
        let count = u32::try_from(reader.read_varint()?)
            .map_err(|_| DecodeError::Invalid("a stack holds too many items"))?;
        if count == 0 {
            return Err(DecodeError::Invalid("a stack holds no items"));
        }
        Ok(Self {
            item,
            count,
            components: ComponentBag::read(reader)?,
        })
    }
}

impl Message for ItemStack {
    fn kind() -> NamespacedId {
        "engine:item_stack"
            .parse()
            .expect("constant identifier is valid")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        self.write(&mut writer);
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let stack = Self::read(&mut reader)?;
        reader.finish()?;
        Ok(stack)
    }

    fn samples() -> Vec<Self> {
        let mut worn = Self::new("test_mod:pickaxe".parse().expect("valid"), 1);
        worn.components
            .set_raw("test_mod:wear".parse().expect("valid"), vec![3, 0, 0, 0]);
        vec![
            Self::new("base_blocks:stone".parse().expect("valid"), 1),
            Self::new("base_blocks:stone".parse().expect("valid"), u32::MAX),
            worn,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(text: &str) -> NamespacedId {
        text.parse().unwrap()
    }

    #[test]
    fn the_table_answers_how_many_fit_in_a_stack_and_refuses_what_it_lacks() {
        let mut table = ItemTable::new();
        table.register(ItemSpec::new("test:stone", 64)).unwrap();
        table.register(ItemSpec::new("test:pickaxe", 1)).unwrap();
        assert_eq!(table.max_stack(&id("test:stone")), Ok(64));
        assert_eq!(
            table.max_stack(&id("test:wood")),
            Err(EntityError::UnknownItem(id("test:wood")))
        );
        assert_eq!(
            table.register(ItemSpec::new("test:stone", 16)),
            Err(EntityError::DuplicateItem(id("test:stone")))
        );
        let order: Vec<String> = table.iter().map(|spec| spec.id().to_string()).collect();
        assert_eq!(order, ["test:pickaxe", "test:stone"]);
    }

    #[test]
    #[should_panic(expected = "at least 1")]
    fn an_item_must_stack_to_at_least_one() {
        let _ = ItemSpec::new("test:stone", 0);
    }

    #[test]
    #[should_panic(expected = "at least one item")]
    fn a_stack_holds_something() {
        let _ = ItemStack::new(id("test:stone"), 0);
    }

    #[test]
    fn stacks_merge_only_with_the_same_item_and_the_same_components() {
        let plain = ItemStack::new(id("test:stone"), 3);
        let mut worn = ItemStack::new(id("test:stone"), 1);
        assert!(plain.stacks_with(&worn));
        worn.components.set_raw(id("test:wear"), vec![1]);
        assert!(!plain.stacks_with(&worn));
        assert!(!plain.stacks_with(&ItemStack::new(id("test:wood"), 3)));
    }

    #[test]
    fn a_stack_of_no_items_or_too_many_is_refused_on_reading() {
        let mut empty = ItemStack::new(id("test:stone"), 1).encode();
        // The count is the byte after the identifier (a length byte and 10 characters).
        empty[11] = 0;
        assert!(ItemStack::decode(&empty).is_err());
        let mut writer = ByteWriter::new();
        writer.write_identifier(&id("test:stone"));
        writer.write_varint(u64::from(u32::MAX) + 1);
        ComponentBag::new().write(&mut writer);
        assert!(ItemStack::decode(&writer.into_bytes()).is_err());
    }

    #[test]
    fn the_codec_of_a_stack_passes_the_generated_check() {
        assert_eq!(protocol::check_message::<ItemStack>(), Ok(()));
    }
}
