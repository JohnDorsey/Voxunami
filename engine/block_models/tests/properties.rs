//! Properties of blocks: declared by key, set by definitions (for every state or for one), and read from
//! the table by handle.

use block_models::{
    BlockDefinition, BlockTable, PropertyDeclaration, PropertyError, PropertyKey, PropertyKind,
    PropertySchema, Shape, engine,
};
use voxel_space::FaceDirection;

const WARMTH: PropertyKey<u8> = PropertyKey::new("test:warmth", 7, "how warm a block is");
const OPEN: PropertyKey<bool> = PropertyKey::new("test:open", false, "whether a door is open");
const DEPTH: PropertyKey<i32> = PropertyKey::new("test:depth", -5, "a signed number");
const BIG: PropertyKey<u32> = PropertyKey::new("test:big", 0, "a large number");
/// The same name as `WARMTH`, read as a wider integer.
const WARMTH_AS_U16: PropertyKey<u16> = PropertyKey::new("test:warmth", 7, "the same, wider");

fn schema() -> PropertySchema {
    let mut schema = PropertySchema::new();
    for declaration in [
        WARMTH.declaration(),
        OPEN.declaration(),
        DEPTH.declaration(),
        BIG.declaration(),
    ] {
        schema.declare(declaration).unwrap();
    }
    schema
}

/// A block of three states (whole, bottom half, top half).
fn three_states() -> BlockDefinition {
    BlockDefinition::cube([0.5; 3]).with_shapes(vec![
        Shape::full(),
        Shape::half(FaceDirection::NegativeY),
        Shape::half(FaceDirection::PositiveY),
    ])
}

fn table(definitions: &[&BlockDefinition]) -> BlockTable {
    let definitions: Vec<Option<&BlockDefinition>> = definitions.iter().map(|&d| Some(d)).collect();
    BlockTable::try_from_definitions(&schema(), &definitions).unwrap()
}

#[test]
fn a_property_is_read_as_the_definition_set_it_and_the_default_where_it_did_not() {
    let warm = BlockDefinition::cube([1.0; 3])
        .with_property(&WARMTH, 200)
        .with_property(&OPEN, true);
    let plain = BlockDefinition::cube([0.5; 3]);
    let table = table(&[&warm, &plain]);
    let warmth = table.handle(&WARMTH).unwrap();
    let open = table.handle(&OPEN).unwrap();
    assert_eq!(table.get_of(warmth, 0, 0), 200);
    assert!(table.get_of(open, 0, 0));
    assert_eq!(table.get_of(warmth, 1, 0), 7, "the key's default");
    assert!(!table.get_of(open, 1, 0));
    // The same through the model.
    assert_eq!(table.get(warmth, table.model_of(0, 0)), 200);
    // A block the table has no definition for, and one beyond its ids, have the default.
    assert_eq!(table.get_of(warmth, 99, 0), 7);
    let with_a_gap = BlockTable::try_from_definitions(&schema(), &[Some(&warm), None]).unwrap();
    let warmth = with_a_gap.handle(&WARMTH).unwrap();
    assert_eq!(
        with_a_gap.get_of(warmth, 1, 0),
        7,
        "a block with no definition"
    );
}

#[test]
fn a_property_can_differ_by_state_and_a_state_the_block_lacks_reads_as_its_first() {
    // Every state is 100; state 1 is 20; and a block that sets only one state has the default elsewhere.
    let lamp = three_states()
        .with_property(&WARMTH, 100)
        .with_state_property(1, &WARMTH, 20);
    let door = three_states().with_state_property(2, &OPEN, true);
    let table = table(&[&lamp, &door]);
    let warmth = table.handle(&WARMTH).unwrap();
    let open = table.handle(&OPEN).unwrap();
    assert_eq!(
        [0, 1, 2].map(|state| table.get_of(warmth, 0, state)),
        [100, 20, 100]
    );
    assert_eq!(
        [0, 1, 2].map(|state| table.get_of(open, 1, state)),
        [false, false, true]
    );
    // The lamp's other block and the door's warmth are untouched.
    assert_eq!(
        [0, 1, 2].map(|state| table.get_of(warmth, 1, state)),
        [7, 7, 7]
    );
    // A state the block does not have is its state 0 (as its model is).
    assert_eq!(table.get_of(warmth, 0, 9), 100);
    assert!(!table.get_of(open, 1, 9));
}

#[test]
fn every_integer_type_and_bool_survive_the_trip_through_a_column_at_their_extremes() {
    let extreme = BlockDefinition::cube([0.0; 3])
        .with_property(&WARMTH, u8::MAX)
        .with_property(&DEPTH, i32::MIN)
        .with_property(&BIG, u32::MAX);
    let other = BlockDefinition::cube([0.0; 3])
        .with_property(&WARMTH, 0)
        .with_property(&DEPTH, i32::MAX)
        .with_property(&BIG, 0);
    let table = table(&[&extreme, &other]);
    let (warmth, depth, big) = (
        table.handle(&WARMTH).unwrap(),
        table.handle(&DEPTH).unwrap(),
        table.handle(&BIG).unwrap(),
    );
    assert_eq!(
        (
            table.get_of(warmth, 0, 0),
            table.get_of(depth, 0, 0),
            table.get_of(big, 0, 0)
        ),
        (u8::MAX, i32::MIN, u32::MAX)
    );
    assert_eq!(
        (
            table.get_of(warmth, 1, 0),
            table.get_of(depth, 1, 0),
            table.get_of(big, 1, 0)
        ),
        (0, i32::MAX, 0)
    );
}

#[test]
fn the_engines_properties_are_always_there_with_their_defaults() {
    let table = BlockTable::from_definitions(&[Some(&BlockDefinition::cube([0.5; 3]))]);
    let density = table.handle(&engine::DENSITY).unwrap();
    let placeable = table.handle(&engine::PLACEABLE_ON_MOVING_GRIDS).unwrap();
    assert_eq!(table.get_of(density, 0, 0), 1000);
    assert!(
        table.get_of(placeable, 0, 0),
        "placeable on a moving grid unless a block says no"
    );
    // A block can say no, and be heavier, for one state.
    let heavy = three_states()
        .with_property(&engine::PLACEABLE_ON_MOVING_GRIDS, false)
        .with_state_property(2, &engine::DENSITY, 7800);
    let table = BlockTable::from_definitions(&[Some(&heavy)]);
    let density = table.handle(&engine::DENSITY).unwrap();
    let placeable = table.handle(&engine::PLACEABLE_ON_MOVING_GRIDS).unwrap();
    assert_eq!(
        [0, 1, 2].map(|s| table.get_of(density, 0, s)),
        [1000, 1000, 7800]
    );
    assert!(!table.get_of(placeable, 0, 1));
}

#[test]
fn what_cannot_be_declared_set_or_read_is_an_error_that_names_the_key() {
    let define = |definition: &BlockDefinition| {
        BlockTable::try_from_definitions(&schema(), &[Some(definition)]).map(|_| ())
    };
    // A key nobody declared.
    const STRANGER: PropertyKey<u8> = PropertyKey::new("test:stranger", 0, "undeclared");
    assert_eq!(
        define(&BlockDefinition::cube([0.0; 3]).with_property(&STRANGER, 1)),
        Err(PropertyError::Undeclared("test:stranger".parse().unwrap()))
    );
    // A value outside a narrower declared range (declared 0 to 10, set as a u8 of 50).
    let mut narrow = PropertySchema::new();
    narrow
        .declare(PropertyDeclaration {
            id: "test:warmth".parse().unwrap(),
            kind: PropertyKind::Integer {
                minimum: 0,
                maximum: 10,
            },
            default: 0,
            doc: "narrow".to_owned(),
        })
        .unwrap();
    let hot = BlockDefinition::cube([0.0; 3]).with_property(&WARMTH, 50);
    assert_eq!(
        BlockTable::try_from_definitions(&narrow, &[Some(&hot)]).map(|_| ()),
        Err(PropertyError::OutOfRange {
            key: "test:warmth".parse().unwrap(),
            value: 50,
            minimum: 0,
            maximum: 10
        })
    );
    // A state the block does not have.
    assert_eq!(
        define(&three_states().with_state_property(3, &WARMTH, 1)),
        Err(PropertyError::NoSuchState {
            key: "test:warmth".parse().unwrap(),
            block: 0,
            state: 3,
            states: 3
        })
    );
    // Declared twice, including an engine key.
    let mut twice = schema();
    assert_eq!(
        twice.declare(WARMTH.declaration()),
        Err(PropertyError::Redeclared("test:warmth".parse().unwrap()))
    );
    assert_eq!(
        twice.declare(engine::DENSITY.declaration()),
        Err(PropertyError::Redeclared("engine:density".parse().unwrap()))
    );
    // Asked for as the wrong type, or not at all.
    let table = table(&[&BlockDefinition::cube([0.0; 3])]);
    assert!(matches!(
        table.handle(&WARMTH_AS_U16),
        Err(PropertyError::WrongKind { .. })
    ));
    assert_eq!(
        table.handle(&STRANGER).err(),
        Some(PropertyError::Undeclared("test:stranger".parse().unwrap()))
    );
    // `from_definitions` panics with the same message rather than returning it.
    let message = std::panic::catch_unwind(|| {
        BlockTable::from_definitions(&[Some(
            &BlockDefinition::cube([0.0; 3]).with_property(&STRANGER, 1),
        )])
    })
    .err()
    .and_then(|payload| payload.downcast_ref::<String>().cloned())
    .unwrap_or_default();
    assert!(message.contains("test:stranger"), "{message}");
}

#[test]
fn definitions_that_set_the_same_things_are_equal_whatever_order_they_set_them_in() {
    let one = three_states()
        .with_property(&WARMTH, 3)
        .with_property(&OPEN, true)
        .with_state_property(2, &DEPTH, 9)
        .with_state_property(1, &DEPTH, 8);
    let other = three_states()
        .with_state_property(1, &DEPTH, 8)
        .with_state_property(2, &DEPTH, 9)
        .with_property(&OPEN, true)
        .with_property(&WARMTH, 3);
    assert_eq!(one, other);
    assert_eq!(
        format!("{one:?}"),
        format!("{other:?}"),
        "one canonical form"
    );
    // Setting a value again replaces it.
    assert_ne!(one, one.clone().with_property(&WARMTH, 4));
    assert_eq!(
        one.clone().with_property(&WARMTH, 4),
        other.with_property(&WARMTH, 4)
    );
}
