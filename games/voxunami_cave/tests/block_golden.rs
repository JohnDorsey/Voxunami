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
const PINNED_PACKAGE: (usize, u64) = (12066, 11_065_068_392_312_003_131);

const PINNED_TABLE: &[&str] = &[
    " 0 base_blocks:air: colour #000000, 1 states, state 0 faces [0 0 0 0 0 0], front none, axis none, docking none, placement Fixed, behaviour a28133c087488ace",
    " 1 base_blocks:stone: colour #606060, 7 states, state 0 faces [23 24 25 26 27 28], front none, axis none, docking none, placement Fixed, behaviour c002feaa7935e442",
    " 2 base_blocks:dirt: colour #81703b, 7 states, state 0 faces [47 48 49 50 51 52], front none, axis none, docking none, placement Fixed, behaviour a10d7e4c65acf7b9",
    " 3 base_blocks:grass: colour #278d2b, 7 states, state 0 faces [71 72 73 74 75 76], front none, axis none, docking none, placement Fixed, behaviour f1ee09b4838804f6",
    " 4 base_blocks:lamp: colour #ffeba6, 1 states, state 0 faces [4 4 4 4 4 4], front none, axis none, docking none, placement Fixed, behaviour 95be8fd8fb01184c",
    " 5 base_blocks:wood: colour #877333, 7 states, state 0 faces [95 96 97 98 99 100], front none, axis PositiveY, docking none, placement AlongPlacedFace(PositiveY), behaviour ba79a2d2ddbebb84",
    " 6 base_blocks:fire: colour #f2801a, 1 states, state 0 faces [9 9 9 9 9 9], front none, axis none, docking none, placement Fixed, behaviour fa5f4357000a02e1",
    " 7 base_blocks:great_river_water: colour #283de8, 1 states, state 0 faces [143 144 145 146 147 148], front none, axis none, docking none, placement Fixed, behaviour 5002183497c3b133",
    " 8 voxunami_cave:dev_docking_port: colour #797658, 1 states, state 0 faces [167 168 169 170 171 172], front PositiveX, axis PositiveY, docking PositiveX, placement AlongPlacedFaceFacingPlayer { axis: PositiveY, front: PositiveX }, behaviour d020a84db4c235e2",
    "palette: 263 colours, digest 2bddb6bcb2eb43eb, pictures 240 on it, digest ae07d2660e244545",
];
