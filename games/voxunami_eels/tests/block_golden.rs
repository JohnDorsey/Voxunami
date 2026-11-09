//! What Voxunami Eels's blocks are, pinned before the blocks move out of Rust into data (`block-textures-spec.md`, step 0): their numbers, the bytes of the
//! package a client is sent, and what the block table answers for each block.
//!
//! The bytes and the table may move only in a step the spec names, and that step lists the pins it moved. If a pin here fails, the description
//! says which block and which answer changed.

use block_models::snapshot::{describe_blocks, fnv_hash};
use game_shell::GameContent;
use voxunami_eels::content::ContentEels;

fn names_of_the_blocks(content: &ContentEels) -> Vec<String> {
    content
        .package_data()
        .blocks
        .iter()
        .map(|(identifier, _)| identifier.to_string())
        .collect()
}

#[test]
fn eels_numbers_its_blocks_as_it_always_did() {
    assert_eq!(names_of_the_blocks(&ContentEels::new()), PINNED_NAMES);
}

#[test]
fn the_package_eels_sends_has_its_pinned_bytes() {
    let bytes = ContentEels::new().package_data().encode();
    assert_eq!(
        (bytes.len(), fnv_hash(&bytes)),
        PINNED_PACKAGE,
        "the bytes of Voxunami Eels's package moved"
    );
}

#[test]
fn what_the_block_table_says_of_each_eels_block_is_pinned() {
    let content = ContentEels::new();
    let table = content
        .package_data()
        .block_table()
        .expect("Voxunami Eels's package makes a table");
    let described = describe_blocks(&table, &names_of_the_blocks(&content));
    assert_eq!(
        described.lines().collect::<Vec<_>>(),
        PINNED_TABLE,
        "what Voxunami Eels's block table says moved"
    );
}

/// The identifiers of the blocks in the order of their numbers, as they were on 2026-11-07.
const PINNED_NAMES: &[&str] = &[
    "voxunami_eels:air",
    "voxunami_eels:wall",
    "voxunami_eels:wall_line",
    "voxunami_eels:trail",
    "voxunami_eels:head",
];

/// (the length, the hash) of `PackageData::encode` as it was on 2026-11-08: 10 bytes longer than on 2026-11-07, the two bytes each of its 5 definitions gained (whether light passes
/// through the block, and its transparency mode).
/// Moved on 2026-11-09 from (1215, 11_721_676_500_271_260_102) by the declaration of `engine:joint_face`, which every game's package carries: 198 bytes (worked out from Cave's, where it was
/// measured, before the run, and then seen: 1,215 to 1,413).
const PINNED_PACKAGE: (usize, u64) = (1413, 12_124_095_517_510_165_630);

/// Moved on 2026-11-09 by the new word `joint none` in every line (the dev ball joint, `decisions/joints-and-cars.md`, step 2): the behaviour digests are of text that has the word, and
/// the control is that the old text, with the word taken out, has the old digest for every block (5 of 5) and the palette line is as it was.
const PINNED_TABLE: &[&str] = &[
    " 0 voxunami_eels:air: colour #000000, 1 states, state 0 faces [0 0 0 0 0 0], front none, axis none, docking none, joint none, placement Fixed, behaviour 67ba0384e7f1d7a0",
    " 1 voxunami_eels:wall: colour #1a1f38, 1 states, state 0 faces [1 1 1 1 1 1], front none, axis none, docking none, joint none, placement Fixed, behaviour 4e0e4ea92b9a6ebb",
    " 2 voxunami_eels:wall_line: colour #40bff2, 1 states, state 0 faces [2 2 2 2 2 2], front none, axis none, docking none, joint none, placement Fixed, behaviour 0fd4c5bb6ffb02b8",
    " 3 voxunami_eels:trail: colour #e63333, 1 states, state 0 faces [5 5 5 5 5 5], front none, axis none, docking none, joint none, placement Fixed, behaviour 69f9ccec52495d37",
    " 4 voxunami_eels:head: colour #f4a3a3, 1 states, state 0 faces [13 13 13 13 13 13], front none, axis none, docking none, joint none, placement Fixed, behaviour d0a2b9ec627060f8",
    "palette: 21 colours, digest dfb345c9ae2453ee, pictures 0 on it, digest 6975f1271857fe35",
];
