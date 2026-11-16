use block_models::{BlockDefinition, PropertyKey};
use namespaced_id::NamespacedId;

use super::{ReplaceError, SharedPackage};
use crate::PackageData;

fn id(text: &str) -> NamespacedId {
    text.parse().unwrap()
}

/// A package of air and a cube whose red is `red`.
fn package_with_red(red: f32) -> PackageData {
    let mut package = PackageData::for_tests();
    package.blocks.push((
        id("test:cube"),
        Some(BlockDefinition::cube([red, 0.0, 0.0])),
    ));
    package
}

#[test]
fn a_new_package_is_held_with_the_table_made_of_it_at_version_zero() {
    let shared = SharedPackage::new(package_with_red(0.25)).unwrap();
    assert_eq!(shared.version(), 0);
    assert_eq!(shared.package(), Arc::new(package_with_red(0.25)));
    assert_eq!(shared.table().current().color(1), [0.25, 0.0, 0.0]);
}

use std::sync::Arc;

#[test]
fn replacing_the_package_replaces_the_table_and_a_clone_sees_both() {
    let shared = SharedPackage::new(package_with_red(0.25)).unwrap();
    let clone = shared.clone();
    assert!(shared.is_the_same_as(&clone));
    let before = shared.package();

    // The package gains a block, as a pack that adds one does.
    let mut changed = package_with_red(0.5);
    changed.blocks.push((
        id("test:added"),
        Some(BlockDefinition::cube([0.0, 1.0, 0.0])),
    ));
    assert_eq!(clone.replace(changed.clone()).unwrap(), 1);

    assert_eq!(shared.version(), 1);
    assert_eq!(shared.package(), Arc::new(changed));
    assert_eq!(shared.package().blocks[2].0, id("test:added"));
    assert_eq!(shared.table().current().color(1), [0.5, 0.0, 0.0]);
    assert_eq!(shared.table().current().color(2), [0.0, 1.0, 0.0]);
    assert_eq!(before.blocks.len(), 2, "what was taken before is as it was");
}

#[test]
fn a_package_whose_blocks_cannot_make_a_table_is_refused_and_nothing_changes() {
    let shared = SharedPackage::new(package_with_red(0.25)).unwrap();
    let mut broken = package_with_red(0.5);
    // A property that the package does not declare.
    broken.blocks[1].1 =
        Some(BlockDefinition::cube([0.5; 3]).with_property_value(id("test:undeclared"), 1));
    assert!(matches!(
        shared.replace(broken),
        Err(ReplaceError::Table(_))
    ));
    assert_eq!(shared.version(), 0);
    assert_eq!(shared.package(), Arc::new(package_with_red(0.25)));
    assert_eq!(shared.table().current().color(1), [0.25, 0.0, 0.0]);
}

#[test]
fn a_package_that_declares_other_properties_is_refused_and_nothing_changes() {
    let shared = SharedPackage::new(package_with_red(0.25)).unwrap();
    let mut other = package_with_red(0.5);
    other
        .properties
        .declare(PropertyKey::new("test:extra", 0u8, "another property").declaration())
        .unwrap();
    assert!(matches!(
        shared.replace(other),
        Err(ReplaceError::Properties(_))
    ));
    assert_eq!(shared.version(), 0);
    assert_eq!(shared.package(), Arc::new(package_with_red(0.25)));
}
