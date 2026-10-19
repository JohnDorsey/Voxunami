use std::collections::BTreeMap;

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use protocol::Message;

/// Typed values by component identifier, kept as the bytes the component's own codec makes of them.
///
/// Keeping the bytes makes three things true without further work: a component of a plugin that is not
/// loaded passes through a save, a send and a load unread; two bags are equal exactly when they would
/// be saved the same; and whether a component changed is whether its bytes did. The typed accessors
/// decode on the way out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComponentBag {
    values: BTreeMap<NamespacedId, Vec<u8>>,
}

impl ComponentBag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Stores a value under the identifier its type gives.
    pub fn set<C: Message>(&mut self, value: &C) {
        self.values.insert(C::kind(), value.encode());
    }

    /// The value, `Ok(None)` if the bag has none of this type, or an error if the bytes it has do not
    /// decode as this type.
    pub fn get<C: Message>(&self) -> Result<Option<C>, DecodeError> {
        self.values
            .get(&C::kind())
            .map(|bytes| C::decode(bytes))
            .transpose()
    }

    /// The bytes of a component, whatever it is.
    pub fn raw(&self, identifier: &NamespacedId) -> Option<&[u8]> {
        self.values.get(identifier).map(Vec::as_slice)
    }

    /// Stores bytes under an identifier without looking at them.
    pub fn set_raw(&mut self, identifier: NamespacedId, bytes: Vec<u8>) {
        self.values.insert(identifier, bytes);
    }

    pub fn remove(&mut self, identifier: &NamespacedId) -> Option<Vec<u8>> {
        self.values.remove(identifier)
    }

    /// Every component, in order of identifier.
    pub fn iter(&self) -> impl Iterator<Item = (&NamespacedId, &[u8])> {
        self.values
            .iter()
            .map(|(identifier, bytes)| (identifier, bytes.as_slice()))
    }

    /// How a bag is written: a count, then each identifier and its bytes, in order of identifier.
    pub fn write(&self, writer: &mut ByteWriter) {
        writer.write_varint(self.values.len() as u64);
        for (identifier, bytes) in &self.values {
            writer.write_identifier(identifier);
            writer.write_bytes(bytes);
        }
    }

    /// Reads what [`ComponentBag::write`] wrote. The identifiers must be in order, once each, so that
    /// there is one way to write any bag.
    pub fn read(reader: &mut ByteReader<'_>) -> Result<Self, DecodeError> {
        // An identifier is at least 4 bytes (a length, `a:b`) and the bytes of a component have a length.
        let count = reader.read_count(5)?;
        let mut values = BTreeMap::new();
        for _ in 0..count {
            let identifier = reader.read_identifier()?;
            let bytes = reader.read_bytes()?.to_vec();
            if let Some((last, _)) = values.last_key_value()
                && *last >= identifier
            {
                return Err(DecodeError::Invalid(
                    "components are out of order or repeated",
                ));
            }
            values.insert(identifier, bytes);
        }
        Ok(Self { values })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A component that is one number.
    #[derive(Debug, PartialEq)]
    struct Health(u32);

    impl Message for Health {
        fn kind() -> NamespacedId {
            "test:health".parse().unwrap()
        }
        fn encode(&self) -> Vec<u8> {
            self.0.to_le_bytes().to_vec()
        }
        fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
            let mut reader = ByteReader::new(payload);
            let value = reader.read_u32()?;
            reader.finish()?;
            Ok(Self(value))
        }
        fn samples() -> Vec<Self> {
            vec![Self(0), Self(u32::MAX)]
        }
    }

    fn id(text: &str) -> NamespacedId {
        text.parse().unwrap()
    }

    #[test]
    fn a_value_goes_in_by_its_type_and_comes_out_the_same() {
        let mut bag = ComponentBag::new();
        assert_eq!(bag.get::<Health>(), Ok(None));
        bag.set(&Health(7));
        assert_eq!(bag.get::<Health>(), Ok(Some(Health(7))));
        bag.set(&Health(9));
        assert_eq!((bag.len(), bag.get::<Health>()), (1, Ok(Some(Health(9)))));
        assert_eq!(bag.raw(&Health::kind()), Some(&[9, 0, 0, 0][..]));
    }

    #[test]
    fn bytes_that_are_not_the_type_are_an_error_not_a_default() {
        let mut bag = ComponentBag::new();
        bag.set_raw(Health::kind(), vec![1, 2]);
        assert!(bag.get::<Health>().is_err());
        bag.set_raw(Health::kind(), vec![1, 0, 0, 0, 0]);
        assert!(bag.get::<Health>().is_err(), "trailing bytes");
    }

    #[test]
    fn a_component_nobody_here_knows_is_kept_as_it_is() {
        let mut bag = ComponentBag::new();
        bag.set_raw(id("other_mod:mood"), vec![1, 2, 3]);
        bag.set(&Health(1));
        let mut writer = ByteWriter::new();
        bag.write(&mut writer);
        let bytes = writer.into_bytes();
        let read = ComponentBag::read(&mut ByteReader::new(&bytes)).unwrap();
        assert_eq!(read, bag);
        assert_eq!(read.raw(&id("other_mod:mood")), Some(&[1, 2, 3][..]));
    }

    #[test]
    fn components_are_written_in_order_of_identifier_whatever_order_they_were_set() {
        let mut first = ComponentBag::new();
        first.set_raw(id("b:x"), vec![1]);
        first.set_raw(id("a:y"), vec![2]);
        let mut second = ComponentBag::new();
        second.set_raw(id("a:y"), vec![2]);
        second.set_raw(id("b:x"), vec![1]);
        let write = |bag: &ComponentBag| {
            let mut writer = ByteWriter::new();
            bag.write(&mut writer);
            writer.into_bytes()
        };
        assert_eq!(write(&first), write(&second));
        assert_eq!(
            first
                .iter()
                .map(|(id, _)| id.to_string())
                .collect::<Vec<_>>(),
            ["a:y", "b:x"]
        );
    }

    #[test]
    fn a_bag_written_out_of_order_or_with_a_repeat_is_refused() {
        let write = |entries: &[(&str, u8)]| {
            let mut writer = ByteWriter::new();
            writer.write_varint(entries.len() as u64);
            for (identifier, byte) in entries {
                writer.write_identifier(&id(identifier));
                writer.write_bytes(&[*byte]);
            }
            writer.into_bytes()
        };
        assert!(
            ComponentBag::read(&mut ByteReader::new(&write(&[("a:x", 1), ("b:x", 2)]))).is_ok()
        );
        for bad in [
            write(&[("b:x", 1), ("a:x", 2)]),
            write(&[("a:x", 1), ("a:x", 2)]),
        ] {
            assert!(ComponentBag::read(&mut ByteReader::new(&bad)).is_err());
        }
    }

    #[test]
    fn a_bag_cut_short_is_refused_and_never_panics() {
        let mut bag = ComponentBag::new();
        bag.set(&Health(5));
        bag.set_raw(id("other:thing"), vec![9; 40]);
        let mut writer = ByteWriter::new();
        bag.write(&mut writer);
        let bytes = writer.into_bytes();
        for length in 0..bytes.len() {
            assert!(
                ComponentBag::read(&mut ByteReader::new(&bytes[..length])).is_err(),
                "cut at {length}"
            );
        }
    }
}
