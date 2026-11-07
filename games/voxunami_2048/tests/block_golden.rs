//! What Voxunami 2048's blocks are, pinned before the blocks move out of Rust into data (`block-textures-spec.md`, step 0): their numbers, the bytes of the
//! package a client is sent, and what the block table answers for each block.
//!
//! The bytes and the table may move only in a step the spec names, and that step lists the pins it moved. If a pin here fails, the description
//! says which block and which answer changed.

use block_models::snapshot::{describe_blocks, fnv_hash};
use game_shell::GameContent;
use voxunami_2048::content::Content2048;

fn names_of_the_blocks(content: &Content2048) -> Vec<String> {
    content
        .package_data()
        .blocks
        .iter()
        .map(|(identifier, _)| identifier.to_string())
        .collect()
}

#[test]
fn game_2048_numbers_its_blocks_as_it_always_did() {
    assert_eq!(names_of_the_blocks(&Content2048::new()), PINNED_NAMES);
}

#[test]
fn the_package_2048_sends_has_its_pinned_bytes() {
    let bytes = Content2048::new().package_data().encode();
    assert_eq!(
        (bytes.len(), fnv_hash(&bytes)),
        PINNED_PACKAGE,
        "the bytes of Voxunami 2048's package moved"
    );
}

#[test]
fn what_the_block_table_says_of_each_2048_block_is_pinned() {
    let content = Content2048::new();
    let table = content
        .package_data()
        .block_table()
        .expect("Voxunami 2048's package makes a table");
    let described = describe_blocks(&table, &names_of_the_blocks(&content));
    assert_eq!(
        described.lines().collect::<Vec<_>>(),
        PINNED_TABLE,
        "what Voxunami 2048's block table says moved"
    );
}

/// The identifiers of the blocks in the order of their numbers, as they were on 2026-11-07.
const PINNED_NAMES: &[&str] = &[
    "voxunami_2048:air",
    "voxunami_2048:stone",
    "voxunami_2048:ice",
    "voxunami_2048:spring_ice",
];

/// (the length, the hash) of `PackageData::encode` as it was on 2026-11-07.
const PINNED_PACKAGE: (usize, u64) = (3510, 18_180_317_838_360_501_055);

const PINNED_TABLE: &[&str] = &[
    " 0 voxunami_2048:air: colour #000000, 1 states, state 0 faces [0 0 0 0 0 0], front none, axis none, docking none, placement Fixed, behaviour 10e1fcf0fdf150ec",
    " 1 voxunami_2048:stone: colour #b3b3b3, 1 states, state 0 faces [4 4 4 4 4 4], front none, axis none, docking none, placement Fixed, behaviour 29a658216fc19cf2",
    " 2 voxunami_2048:ice: colour #c6d2d9, 1 states, state 0 faces [65 65 65 65 65 65], front none, axis none, docking none, placement Fixed, behaviour adc4c009d7ca0cd3",
    " 3 voxunami_2048:spring_ice: colour #d9cfac, 1 states, state 0 faces [126 126 126 126 126 126], front none, axis none, docking none, placement Fixed, behaviour 5a7932f3cd24b13b",
    "palette: 187 colours, digest 42a4a72f5966cadc",
];
