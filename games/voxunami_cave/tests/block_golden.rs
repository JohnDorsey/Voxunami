//! What Cave's blocks are, pinned before the blocks move out of Rust into data (`block-textures-spec.md`, step 0): their numbers, the bytes of the
//! package a client is sent, and what the block table answers for each block.
//!
//! The bytes and the table may move only in a step the spec names, and that step lists the pins it moved. The numbering and the table's
//! behaviour must survive every step; if a pin here fails, the description says which block and which answer changed.

use block_models::snapshot::{describe_blocks, fnv_hash};
use game_shell::GameContent;
use voxunami_cave::content::CaveContent;

fn names_of_the_blocks(content: &CaveContent) -> Vec<String> {
    content
        .package_data()
        .blocks
        .iter()
        .map(|(identifier, _)| identifier.to_string())
        .collect()
}

#[test]
fn cave_numbers_its_blocks_as_it_always_did() {
    assert_eq!(
        names_of_the_blocks(&CaveContent::new()),
        [
            "base_blocks:air",
            "base_blocks:stone",
            "base_blocks:dirt",
            "base_blocks:grass",
            "base_blocks:lamp",
            "base_blocks:wood",
            "base_blocks:fire",
            "base_blocks:great_river_water",
            "voxunami_cave:dev_docking_port",
        ]
    );
}

#[test]
fn the_package_cave_sends_has_its_pinned_bytes() {
    let bytes = CaveContent::new().package_data().encode();
    assert_eq!(
        (bytes.len(), fnv_hash(&bytes)),
        PINNED_PACKAGE,
        "the bytes of Cave's package moved"
    );
}

#[test]
fn what_the_block_table_says_of_each_cave_block_is_pinned() {
    let content = CaveContent::new();
    let table = content
        .package_data()
        .block_table()
        .expect("Cave's package makes a table");
    let described = describe_blocks(&table, &names_of_the_blocks(&content));
    assert_eq!(
        described.lines().collect::<Vec<_>>(),
        PINNED_TABLE,
        "what Cave's block table says moved"
    );
}

/// (the length, the hash) of `PackageData::encode` for Cave as it was on 2026-11-07.
const PINNED_PACKAGE: (usize, u64) = (3088, 9_365_039_542_407_574_718);

const PINNED_TABLE: &[&str] = &[
    " 0 base_blocks:air: colour #000000, 1 states, state 0 faces [0 0 0 0 0 0], front none, axis none, docking none, placement Fixed, behaviour 10e1fcf0fdf150ec",
    " 1 base_blocks:stone: colour #808087, 7 states, state 0 faces [1 1 1 1 1 1], front none, axis none, docking none, placement Fixed, behaviour fa566482241665e0",
    " 2 base_blocks:dirt: colour #734d2e, 7 states, state 0 faces [2 2 2 2 2 2], front none, axis none, docking none, placement Fixed, behaviour 76ef6fcd6ab3c195",
    " 3 base_blocks:grass: colour #4d9e33, 7 states, state 0 faces [3 3 3 3 3 3], front none, axis none, docking none, placement Fixed, behaviour e6280ca6456bb6da",
    " 4 base_blocks:lamp: colour #ffeba6, 1 states, state 0 faces [4 4 4 4 4 4], front none, axis none, docking none, placement Fixed, behaviour 29a658216fc19cf2",
    " 5 base_blocks:wood: colour #8c6133, 7 states, state 0 faces [5 5 21 21 5 5], front none, axis PositiveY, docking none, placement AlongPlacedFace(PositiveY), behaviour e14f37ae8fdd16e2",
    " 6 base_blocks:fire: colour #f2801a, 1 states, state 0 faces [9 9 9 9 9 9], front none, axis none, docking none, placement Fixed, behaviour 9f1593674525a51f",
    " 7 base_blocks:great_river_water: colour #1447d9, 1 states, state 0 faces [7 7 7 7 7 7], front none, axis none, docking none, placement Fixed, behaviour 57ac60228431ee45",
    " 8 voxunami_cave:dev_docking_port: colour #2b3638, 1 states, state 0 faces [8 22 8 8 8 8], front PositiveX, axis none, docking PositiveX, placement FrontToPlayer(PositiveX), behaviour 0d98df2d76b13b2d",
    "palette: 23 colours, digest 30fe57426e454c88",
];
