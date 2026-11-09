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
            "base_blocks:leaves",
            "base_blocks:glass",
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

/// (the length, the hash) of `PackageData::encode` for Cave as it was on 2026-11-08, with leaves and glass (their definitions, their two pictures and the two bytes each definition
/// gained, whether light passes and the transparency mode, moved it; before them, the pictures redrawn since the pin of 2026-11-07 had already moved the hash and not the length).
const PINNED_PACKAGE: (usize, u64) = (14669, 15_918_905_900_417_198_379);

const PINNED_TABLE: &[&str] = &[
    " 0 base_blocks:air: colour #000000, 1 states, state 0 faces [0 0 0 0 0 0], front none, axis none, docking none, placement Fixed, behaviour 8375d9cf84a8c372",
    " 1 base_blocks:stone: colour #606060, 7 states, state 0 faces [29 30 31 32 33 34], front none, axis none, docking none, placement Fixed, behaviour 05508a69b39f112c",
    " 2 base_blocks:dirt: colour #81703b, 7 states, state 0 faces [53 54 55 56 57 58], front none, axis none, docking none, placement Fixed, behaviour 2fd372cc0986003f",
    " 3 base_blocks:grass: colour #1e8929, 7 states, state 0 faces [77 78 79 80 81 82], front none, axis none, docking none, placement Fixed, behaviour 05ecf83cfceca83a",
    " 4 base_blocks:lamp: colour #ffeba6, 1 states, state 0 faces [4 4 4 4 4 4], front none, axis none, docking none, placement Fixed, behaviour ac89a6dda29bc088",
    " 5 base_blocks:wood: colour #887433, 7 states, state 0 faces [101 102 103 104 105 106], front none, axis PositiveY, docking none, placement AlongPlacedFace(PositiveY), behaviour aa2fd550c6309468",
    " 6 base_blocks:fire: colour #f2801a, 1 states, state 0 faces [13 13 13 13 13 13], front none, axis none, docking none, placement Fixed, behaviour e21fa3117d1dea5a",
    " 7 base_blocks:great_river_water: colour #283de8, 1 states, state 0 faces [149 150 151 152 153 154], front none, axis none, docking none, placement Fixed, behaviour bec3c599d5daca51",
    " 8 base_blocks:leaves: colour #1e862c, 1 states, state 0 faces [173 174 175 176 177 178], front none, axis none, docking none, placement Fixed, behaviour 30b62a8e52d4eaae",
    " 9 base_blocks:glass: colour #e1f3ff, 1 states, state 0 faces [197 198 199 200 201 202], front none, axis none, docking none, placement Fixed, behaviour db12fbea210f6993",
    "10 voxunami_cave:dev_docking_port: colour #797658, 1 states, state 0 faces [221 222 223 224 225 226], front PositiveX, axis PositiveY, docking PositiveX, placement AlongPlacedFaceFacingPlayer { axis: PositiveY, front: PositiveX }, behaviour d9cef649c554b1f2",
    "11 voxunami_cave:dev_command_seat: colour #2b3638, 1 states, state 0 faces [11 27 27 11 11 11], front PositiveX, axis none, docking none, placement FrontToPlayer(PositiveX), behaviour cd35ad9f3dff6639",
    "12 voxunami_cave:dev_rocket_engine: colour #2b3638, 1 states, state 0 faces [12 12 12 28 12 12], front none, axis PositiveY, docking none, placement AlongPlacedFace(PositiveY), behaviour 1e4a9054949c8ca7",
    "palette: 317 colours, digest 8f1ebc6c04c72571, pictures 288 on it, digest 6c0d66a4b42fe8e5",
];
