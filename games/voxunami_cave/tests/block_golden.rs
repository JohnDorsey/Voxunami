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
            "voxunami_cave:dev_command_seat",
            "voxunami_cave:dev_rocket_engine",
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
const PINNED_PACKAGE: (usize, u64) = (12399, 9_118_255_085_295_258_911);

const PINNED_TABLE: &[&str] = &[
    " 0 base_blocks:air: colour #000000, 1 states, state 0 faces [0 0 0 0 0 0], front none, axis none, docking none, placement Fixed, behaviour a28133c087488ace",
    " 1 base_blocks:stone: colour #606060, 7 states, state 0 faces [27 28 29 30 31 32], front none, axis none, docking none, placement Fixed, behaviour 110e970b5b3041d4",
    " 2 base_blocks:dirt: colour #81703b, 7 states, state 0 faces [51 52 53 54 55 56], front none, axis none, docking none, placement Fixed, behaviour 4959538694d32281",
    " 3 base_blocks:grass: colour #278d2b, 7 states, state 0 faces [75 76 77 78 79 80], front none, axis none, docking none, placement Fixed, behaviour a4fcf437994b7026",
    " 4 base_blocks:lamp: colour #ffeba6, 1 states, state 0 faces [4 4 4 4 4 4], front none, axis none, docking none, placement Fixed, behaviour 95be8fd8fb01184c",
    " 5 base_blocks:wood: colour #877333, 7 states, state 0 faces [99 100 101 102 103 104], front none, axis PositiveY, docking none, placement AlongPlacedFace(PositiveY), behaviour c9d5b3f57bc54abc",
    " 6 base_blocks:fire: colour #f2801a, 1 states, state 0 faces [11 11 11 11 11 11], front none, axis none, docking none, placement Fixed, behaviour af1641be1a6e00a0",
    " 7 base_blocks:great_river_water: colour #283de8, 1 states, state 0 faces [147 148 149 150 151 152], front none, axis none, docking none, placement Fixed, behaviour ab298b85b1cde3f9",
    " 8 voxunami_cave:dev_docking_port: colour #797658, 1 states, state 0 faces [171 172 173 174 175 176], front PositiveX, axis PositiveY, docking PositiveX, placement AlongPlacedFaceFacingPlayer { axis: PositiveY, front: PositiveX }, behaviour 4f49df55440b0773",
    " 9 voxunami_cave:dev_command_seat: colour #2b3638, 1 states, state 0 faces [9 25 25 9 9 9], front PositiveX, axis none, docking none, placement FrontToPlayer(PositiveX), behaviour 913dc874d23dadb6",
    "10 voxunami_cave:dev_rocket_engine: colour #2b3638, 1 states, state 0 faces [10 10 10 26 10 10], front none, axis PositiveY, docking none, placement AlongPlacedFace(PositiveY), behaviour b60f290b8e9b2be9",
    "palette: 267 colours, digest 38e000c742a75198, pictures 240 on it, digest b7e51d04e5882b85",
];
