//! Properties of blocks: values, by namespaced key, that plugins declare and block definitions set.
//!
//! A key is declared once ([`PropertyDeclaration`], usually made from a typed [`PropertyKey`]) with a
//! kind, a default and a doc line. A [`crate::BlockDefinition`] sets the keys it cares about, for every
//! state of the block and, if it wants, differently for some states. [`crate::BlockTable`] turns them into
//! one column per key, so that a rule that has resolved a [`PropertyHandle`] reads a block and state's value
//! as an array index.
//!
//! Kinds are bool and integers with a range: nothing a rule reads is a float, which keeps the simulation
//! bit-identical, and every definition has one canonical form.

use std::collections::BTreeMap;
use std::marker::PhantomData;

use namespaced_id::NamespacedId;

mod sealed {
    pub trait Sealed {}
}

/// What values a property can have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyKind {
    Bool,
    /// Whole numbers from `minimum` to `maximum`.
    Integer {
        minimum: i64,
        maximum: i64,
    },
}

impl PropertyKind {
    fn contains(self, value: i64) -> bool {
        match self {
            Self::Bool => value == 0 || value == 1,
            Self::Integer { minimum, maximum } => (minimum..=maximum).contains(&value),
        }
    }
}

/// A Rust type a property can be read as.
pub trait PropertyType: Copy + sealed::Sealed + 'static {
    const KIND: PropertyKind;
    fn to_value(self) -> i64;
    fn from_value(value: i64) -> Self;
}

impl sealed::Sealed for bool {}
impl PropertyType for bool {
    const KIND: PropertyKind = PropertyKind::Bool;
    fn to_value(self) -> i64 {
        i64::from(self)
    }
    fn from_value(value: i64) -> Self {
        value != 0
    }
}

macro_rules! integer_property_type {
    ($($type:ty),*) => {$(
        impl sealed::Sealed for $type {}
        impl PropertyType for $type {
            const KIND: PropertyKind = PropertyKind::Integer {
                minimum: <$type>::MIN as i64,
                maximum: <$type>::MAX as i64,
            };
            fn to_value(self) -> i64 {
                i64::from(self)
            }
            fn from_value(value: i64) -> Self {
                value as $type
            }
        }
    )*};
}

integer_property_type!(u8, u16, u32, i32);

/// A property's name, type and default, as a constant a plugin can export and rules can read by.
#[derive(Clone, Copy, Debug)]
pub struct PropertyKey<T: PropertyType> {
    id: &'static str,
    default: T,
    doc: &'static str,
}

impl<T: PropertyType> PropertyKey<T> {
    /// A key called `id` (such as `base_blocks:emission`) whose value is `default` where a block does
    /// not say.
    pub const fn new(id: &'static str, default: T, doc: &'static str) -> Self {
        Self { id, default, doc }
    }

    pub fn id(&self) -> &'static str {
        self.id
    }

    pub fn default_value(&self) -> T {
        self.default
    }

    /// The declaration that makes this key exist in a schema. Panics if the key's name is not a valid
    /// identifier (a mistake in the plugin's constant).
    pub fn declaration(&self) -> PropertyDeclaration {
        PropertyDeclaration {
            id: self
                .id
                .parse()
                .unwrap_or_else(|error| panic!("{:?} is not a property name: {error}", self.id)),
            kind: T::KIND,
            default: self.default.to_value(),
            doc: self.doc.to_owned(),
        }
    }
}

/// A declared property: what a key means and what values it takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyDeclaration {
    pub id: NamespacedId,
    pub kind: PropertyKind,
    pub default: i64,
    pub doc: String,
}

/// The engine's own properties: the ones the engine itself reads, so that every game has them.
pub mod engine {
    use voxel_space::FaceDirection;

    use super::PropertyKey;

    /// What a property that names a face holds: 0 for no face, else 1 plus the face's index (west 1, east 2, down 3, up 4, north 5, south 6),
    /// with east along +x, up along +y and north along -z.
    pub fn face_code(face: Option<FaceDirection>) -> u8 {
        face.map_or(0, |face| face.index() + 1)
    }

    /// The face a code names: `None` for 0, and for a number that is no face.
    pub fn face_from_code(code: u8) -> Option<FaceDirection> {
        code.checked_sub(1).and_then(FaceDirection::from_index)
    }

    /// The word for a face.
    pub fn face_name(face: FaceDirection) -> &'static str {
        ["west", "east", "down", "up", "north", "south"][usize::from(face.index())]
    }

    /// Which face of a block, in each state, is drawn in [`MARK_COLOR`] instead of the block's own colour (a face code, 0 for none), so a block
    /// can show which way it faces.
    pub const MARKED_FACE: PropertyKey<u8> = PropertyKey::new(
        "engine:marked_face",
        0,
        "the face of the block drawn in its mark colour: 0 for none, else 1 plus the face's index (west 1, east 2, down 3, up 4, north 5, south 6)",
    );

    /// The colour of the marked face, as 0xRRGGBB.
    pub const MARK_COLOR: PropertyKey<u32> = PropertyKey::new(
        "engine:mark_color",
        0,
        "the colour of the marked face, as 0xRRGGBB",
    );

    /// Whether the face opposite the marked face is drawn in the mark colour too: a log, with its two ends.
    pub const MARK_BOTH_ENDS: PropertyKey<bool> = PropertyKey::new(
        "engine:mark_both_ends",
        false,
        "whether the face opposite the marked face is drawn in the mark colour too (the two ends of a log)",
    );

    /// The front of a block that has one, in each state, as it is made (a face code, 0 for none): a block with a front is turned, when a player places
    /// it, to face the player, as a piston does.
    pub const FRONT_FACE: PropertyKey<u8> = PropertyKey::new(
        "engine:front_face",
        0,
        "the front of the block as it is made, 0 for a block with none, else 1 plus the face's index (west 1, east 2, down 3, up 4, north 5, south 6)",
    );

    /// One end of the line a block is symmetrical about, in each state, as it is made (a face code, 0 for none): a block with such a line is turned, when a
    /// player places it, so that the line runs along the face it is placed against, as a log is.
    pub const AXIS_FACE: PropertyKey<u8> = PropertyKey::new(
        "engine:axis_face",
        0,
        "one end of the line the block is symmetrical about as it is made, 0 for none, else 1 plus the face's index (west 1, east 2, down 3, up 4, north 5, south 6)",
    );

    /// Which face of a block, in each state, is its docking face: the face a ship docks by (a face code, 0 for a block that is not a docking
    /// port). A face that looks one way in a state looks the way the grid is turned to when the block's grid is, so a docking port is turned
    /// with its ship (see [`crate::BlockTable::turned_state`]).
    pub const DOCKING_FACE: PropertyKey<u8> = PropertyKey::new(
        "engine:docking_face",
        0,
        "the face of the block that docks with another ship's: 0 for a block that is not a docking port, else 1 plus the face's index (west 1, east 2, down 3, up 4, north 5, south 6)",
    );

    /// A block's mass for a body made of it, in kilograms a voxel. (Nothing reads it yet: bodies made of
    /// blocks are Phase 32.)
    pub const DENSITY: PropertyKey<u32> = PropertyKey::new(
        "engine:density",
        1000,
        "mass of the block in kilograms a voxel, for bodies made of blocks",
    );

    /// Whether the block can be placed on a ship or any other moving grid. The one thing a mod author
    /// sets for airships.
    pub const PLACEABLE_ON_MOVING_GRIDS: PropertyKey<bool> = PropertyKey::new(
        "engine:placeable_on_moving_grids",
        true,
        "whether the block can be placed on a grid that moves",
    );
}

/// Why properties cannot be used as asked.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PropertyError {
    #[error("the property {0} is declared twice")]
    Redeclared(NamespacedId),
    #[error("the property {0} is not declared")]
    Undeclared(NamespacedId),
    #[error("the property {key} is {value}, outside what it takes ({minimum} to {maximum})")]
    OutOfRange {
        key: NamespacedId,
        value: i64,
        minimum: i64,
        maximum: i64,
    },
    #[error("the property {key} is declared as {declared:?} and was asked for as {asked:?}")]
    WrongKind {
        key: NamespacedId,
        declared: PropertyKind,
        asked: PropertyKind,
    },
    #[error(
        "the property {key} is set for state {state} of block {block}, which has {states} states"
    )]
    NoSuchState {
        key: NamespacedId,
        block: u32,
        state: u32,
        states: u32,
    },
}

/// The properties that exist: the engine's, and what a game's plugins declare.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertySchema {
    declared: BTreeMap<NamespacedId, PropertyDeclaration>,
}

impl Default for PropertySchema {
    fn default() -> Self {
        Self::new()
    }
}

impl PropertySchema {
    /// A schema with the engine's properties and no others.
    pub fn new() -> Self {
        let mut schema = Self::empty();
        for declaration in [
            engine::DENSITY.declaration(),
            engine::PLACEABLE_ON_MOVING_GRIDS.declaration(),
            engine::MARKED_FACE.declaration(),
            engine::MARK_COLOR.declaration(),
            engine::DOCKING_FACE.declaration(),
            engine::MARK_BOTH_ENDS.declaration(),
            engine::FRONT_FACE.declaration(),
            engine::AXIS_FACE.declaration(),
        ] {
            schema
                .declare(declaration)
                .expect("the engine's properties are distinct");
        }
        schema
    }

    /// A schema with no properties at all, not even the engine's: what a decoded one starts as.
    pub(crate) fn empty() -> Self {
        Self {
            declared: BTreeMap::new(),
        }
    }

    /// Adds a property. An error if the name is taken.
    pub fn declare(&mut self, declaration: PropertyDeclaration) -> Result<(), PropertyError> {
        if self.declared.contains_key(&declaration.id) {
            return Err(PropertyError::Redeclared(declaration.id));
        }
        self.declared.insert(declaration.id.clone(), declaration);
        Ok(())
    }

    pub fn get(&self, id: &NamespacedId) -> Option<&PropertyDeclaration> {
        self.declared.get(id)
    }

    /// The declarations, in the order of their names.
    pub fn iter(&self) -> impl Iterator<Item = &PropertyDeclaration> {
        self.declared.values()
    }
}

/// A block's values for one key: for every state, and for the states that differ.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SetValues {
    pub every_state: Option<i64>,
    pub by_state: BTreeMap<u32, i64>,
}

/// What a block definition sets, by key. A `BTreeMap` all through, so equal settings are equal bytes
/// however they were made.
pub(crate) type SetProperties = BTreeMap<NamespacedId, SetValues>;

pub(crate) fn key_id(id: &'static str) -> NamespacedId {
    id.parse()
        .unwrap_or_else(|error| panic!("{id:?} is not a property name: {error}"))
}

/// One column of the table: a key's value for every model (every block and state).
#[derive(Clone, Debug)]
pub(crate) struct Column {
    pub id: NamespacedId,
    pub kind: PropertyKind,
    pub values: Vec<i64>,
}

/// Where a key's column is in a table, and what type it is read as. Made once by
/// [`crate::BlockTable::handle`]; reading through it is an array index.
#[derive(Debug)]
pub struct PropertyHandle<T: PropertyType> {
    pub(crate) column: u32,
    pub(crate) marker: PhantomData<fn() -> T>,
}

impl<T: PropertyType> Clone for PropertyHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: PropertyType> Copy for PropertyHandle<T> {}

/// Checks that a value fits a declaration's kind.
pub(crate) fn check_value(
    declaration: &PropertyDeclaration,
    value: i64,
) -> Result<(), PropertyError> {
    if declaration.kind.contains(value) {
        return Ok(());
    }
    let (minimum, maximum) = match declaration.kind {
        PropertyKind::Bool => (0, 1),
        PropertyKind::Integer { minimum, maximum } => (minimum, maximum),
    };
    Err(PropertyError::OutOfRange {
        key: declaration.id.clone(),
        value,
        minimum,
        maximum,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bool_takes_only_0_and_1_and_an_integer_only_its_range_for_values_that_do_not_come_from_rust_types()
     {
        // Values that are read from data (Phase 29) are numbers, and are checked against the declaration.
        let flag = PropertyDeclaration {
            id: "test:flag".parse().unwrap(),
            kind: PropertyKind::Bool,
            default: 0,
            doc: String::new(),
        };
        assert_eq!(check_value(&flag, 0), Ok(()));
        assert_eq!(check_value(&flag, 1), Ok(()));
        for bad in [2, -1, i64::MAX] {
            assert_eq!(
                check_value(&flag, bad),
                Err(PropertyError::OutOfRange {
                    key: flag.id.clone(),
                    value: bad,
                    minimum: 0,
                    maximum: 1
                })
            );
        }
        let small = PropertyDeclaration {
            id: "test:small".parse().unwrap(),
            kind: PropertyKind::Integer {
                minimum: -3,
                maximum: 3,
            },
            default: 0,
            doc: String::new(),
        };
        assert_eq!(check_value(&small, -3), Ok(()));
        assert_eq!(check_value(&small, 3), Ok(()));
        assert!(check_value(&small, 4).is_err() && check_value(&small, -4).is_err());
    }
}
