use std::sync::Arc;
use std::time::{Duration, Instant};

use super::SharedBlockTable;
use crate::{BlockDefinition, BlockTable};

/// A table of air and one cube whose red is `red`, so that a table says which one it is.
fn table_with_red(red: f32) -> BlockTable {
    let air = BlockDefinition::air();
    let cube = BlockDefinition::cube([red, 0.0, 0.0]);
    BlockTable::from_definitions(&[Some(&air), Some(&cube)])
}

fn red_of(table: &BlockTable) -> f32 {
    table.color(1)[0]
}

#[test]
fn a_new_handle_holds_the_table_it_was_given_at_version_zero() {
    let shared = SharedBlockTable::new(table_with_red(0.25));
    assert_eq!(red_of(&shared.current()), 0.25);
    assert_eq!(shared.version(), 0);
    let (table, version) = shared.snapshot();
    assert_eq!((red_of(&table), version), (0.25, 0));
}

#[test]
fn a_table_that_is_already_shared_is_taken_as_it_is_and_not_copied() {
    let table = Arc::new(table_with_red(0.5));
    let shared = SharedBlockTable::new(Arc::clone(&table));
    assert!(Arc::ptr_eq(&table, &shared.current()));
}

#[test]
fn a_clone_is_the_same_table_and_a_replacement_through_either_is_seen_by_both() {
    let shared = SharedBlockTable::new(table_with_red(0.25));
    let clone = shared.clone();
    assert!(shared.is_the_same_as(&clone));
    assert_eq!(shared.handle_count(), 2);

    assert_eq!(clone.replace(table_with_red(0.5)), 1);
    assert_eq!((red_of(&shared.current()), shared.version()), (0.5, 1));
    assert_eq!((red_of(&clone.current()), clone.version()), (0.5, 1));

    assert_eq!(shared.replace(table_with_red(0.75)), 2);
    assert_eq!((red_of(&clone.current()), clone.version()), (0.75, 2));
}

#[test]
fn two_handles_made_separately_are_not_the_same_even_when_their_tables_are_equal() {
    let one = SharedBlockTable::new(table_with_red(0.25));
    let other = SharedBlockTable::new(table_with_red(0.25));
    assert!(!one.is_the_same_as(&other));
    one.replace(table_with_red(0.5));
    assert_eq!(
        red_of(&other.current()),
        0.25,
        "a replacement does not reach a handle that is not a clone"
    );
}

#[test]
fn a_snapshot_taken_before_a_replacement_keeps_the_table_it_took() {
    let shared = SharedBlockTable::new(table_with_red(0.25));
    let (before, version) = shared.snapshot();
    shared.replace(table_with_red(0.5));
    assert_eq!((red_of(&before), version), (0.25, 0));
    assert_eq!(red_of(&shared.current()), 0.5);
}

#[test]
fn the_version_counts_every_replacement_even_by_an_equal_table() {
    let shared = SharedBlockTable::new(table_with_red(0.25));
    for expected in 1..=5 {
        assert_eq!(shared.replace(table_with_red(0.25)), expected);
    }
    assert_eq!(shared.version(), 5);
}

#[test]
fn a_dropped_handle_is_no_longer_counted() {
    let shared = SharedBlockTable::new(table_with_red(0.25));
    let clones: Vec<_> = (0..3).map(|_| shared.clone()).collect();
    assert_eq!(shared.handle_count(), 4);
    drop(clones);
    assert_eq!(shared.handle_count(), 1);
}

#[test]
fn the_holders_of_the_table_are_the_snapshots_kept_of_it_and_a_replaced_table_is_not_counted() {
    let shared = SharedBlockTable::new(table_with_red(0.25));
    assert_eq!(shared.table_holders(), 1, "the handle's own hold");
    let kept: Vec<_> = (0..3).map(|_| shared.current()).collect();
    assert_eq!(shared.table_holders(), 4);
    let (also, _) = shared.snapshot();
    assert_eq!(shared.table_holders(), 5);
    drop(also);
    // A replacement: the snapshots kept hold the table that was, and the new one is held by the handle alone.
    shared.replace(table_with_red(0.5));
    assert_eq!(shared.table_holders(), 1);
    assert_eq!(red_of(&kept[0]), 0.25);
}

#[test]
fn readers_while_another_thread_replaces_always_see_a_table_with_the_version_it_came_with() {
    // The table of version `n` has red `n`, so a snapshot that mixed the table of one version with the number of another is seen.
    const REPLACEMENTS: u64 = 300;
    let shared = SharedBlockTable::new(table_with_red(0.0));
    let deadline = Instant::now() + Duration::from_secs(60);
    let readers: Vec<_> = (0..3)
        .map(|_| {
            let shared = shared.clone();
            std::thread::spawn(move || {
                let mut seen = 0;
                while seen < REPLACEMENTS {
                    assert!(Instant::now() < deadline, "the writer did not finish");
                    let (table, version) = shared.snapshot();
                    assert_eq!(
                        red_of(&table),
                        version as f32,
                        "a table and a version of two moments"
                    );
                    assert!(version >= seen, "the version went back");
                    seen = version;
                }
            })
        })
        .collect();
    for version in 1..=REPLACEMENTS {
        assert_eq!(shared.replace(table_with_red(version as f32)), version);
    }
    for reader in readers {
        reader.join().expect("a reader saw a mixed snapshot");
    }
}
