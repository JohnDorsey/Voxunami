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
            "voxunami_cave:dev_ball_joint",
            "voxunami_cave:dev_hinge_joint",
            "voxunami_cave:dev_axle_joint",
        ]
    );
}

/// Dirt's bumps (2026-11-12), the placeholder specular pictures of the lamp and the leaves (2026-11-13) and the owner's companions painted with VirtualAtlas (2026-11-18): the package a client
/// is sent, as the client reads it, has exactly eight companion pictures, each of the size of Cave's pictures (16 pixels across): four normal ones (`dirt_n`, `grass_n`, `stone_n`, `leaves_n`)
/// and four specular ones (`lamp_s`, `leaves_s`, `dirt_s`, `stone_s`).
#[test]
fn the_package_cave_sends_carries_the_normal_and_specular_pictures_that_are_painted_and_no_other_companion()
 {
    let sent = CaveContent::new().package_data().encode();
    let received = game_shell::PackageData::decode(&sent).expect("the package reads back");
    let companions = |kind: fn(&data_packs::Tile) -> Option<&[u8]>| {
        received
            .tile_classes
            .iter()
            .flatten()
            .filter(|tile| kind(tile).is_some())
            .map(|tile| (tile.size(), kind(tile).map(<[u8]>::len)))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        companions(data_packs::Tile::normal),
        [(16, Some(16 * 16 * 4)); 4]
    );
    assert_eq!(
        companions(data_packs::Tile::specular),
        [(16, Some(16 * 16 * 4)); 4]
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

/// (the length, the hash) of `PackageData::encode` for Cave as it was on 2026-11-09, with the dev ball joint. It moved from (14669, 15_918_905_900_417_198_379) of 2026-11-08, by 369 bytes
/// that are the declaration of `engine:joint_face` (198: the 17 characters of its name and 176 of its doc line, with their lengths, its kind and its default; worked out before the run and then
/// seen) and the new block with its entry (171, measured by encoding the package without the block).
/// Companion pictures (2026-11-12): each picture in the package has a byte of flags before its pixels, so the package is 11 bytes longer, 15,038 to 15,049 (11 is the number of pictures Cave's blocks use, which I
/// believe and did not count separately; the stream below grew by the same 11, and the saved world's hash did not move).
/// Bumps (2026-11-12): dirt has a normal picture, `dirt_n.png`, 16 by 16 of four bytes, so the package is 1,024 bytes longer, 15,049 to 16,073 (worked out before the run, and seen; the
/// flag byte of dirt's picture was there already).
/// Ambient occlusion (2026-11-13): the blue of `dirt_n.png` is 128 to 255 (its pits) and no longer 255 everywhere, so the length is the same (predicted, and seen) and the hash moved.
/// The lamp's picture (2026-11-13): `lamp.png`, 16 by 16, is now the lamp's texture, so the package is 1,048 bytes longer, 16,073 to 17,121 (1,024 of them the pixels; the other 24 are the picture's name and its entry, worked out by neither of us and not itemised), the lamp's faces are pictures (6 faces by 4 shades: 24 more materials, 319 to 343 colours) and every block after it has faces numbered 24 higher.
/// The placeholder specular pictures (2026-11-13): `lamp_s.png` and `leaves_s.png`, 16 by 16 of four bytes each, so the package is 2,048 bytes longer, 17,121 to 19,169 (predicted before
/// the run, and seen: the flag byte of each picture was there already); the table is the same (a companion makes no material), and the hash moved.
/// The dev hinge joint (2026-11-25, `decisions/joints-and-cars.md`): the engine declares `engine:hinge_face` and Cave has one block more, which is a placeable item. The package is 19,169 bytes before and 19,619 now (+450:
/// the declaration and the block with its entry, measured; the stream of `golden_session.rs` is 480 longer, the other 30 being the item). Every old block's face numbers moved, as they did for the ball joint: the new block's colour
/// takes a place after the blocks' (the fire's state colours, the seat's brown, the engine's red and the ball's blue are one more), and its green end and orange line faces two places after the faces' own colours (every picture's
/// face is three more: 31 to 34 for the stone's); so the old blocks' lines have other digests, and the palette is 346 colours where it was 343.
/// The dev axle joint (2026-11-25): Cave has one block more, and the property `engine:hinge_face` became `engine:joint_line_face` with a new doc line, as it serves axles too. The package is 19,619 bytes before and 19,845 now
/// (+226: 24 for the property, as 2048's and Eels' show, and 202 for the block with its entry). Face numbers moved again by the new block's colour and its purple end.
/// (graphics-world-painting's changes, merged into this demo branch:)
/// The leaves' specular picture painted again (owner, 2026-11-18, with VirtualAtlas: 31 pixels have a reflectance of 30 now, the green byte): the same length, 19,169, and the hash moved to
/// 4_612_593_762_799_626_470. Checked: with the owner's other pictures and the old `leaves_s.png` the old hash came back, so that file alone moved it (the new `_n` and `_s` files of the
/// owner's are not in the package: the plugin lists the pictures it carries).
/// The owner's five companions are in the package (2026-11-18: `grass_n`, `stone_n`, `leaves_n`, `dirt_s`, `stone_s`): 5 pictures of 16 by 16 of four bytes, so exactly 5,120 bytes longer (predicted
/// before the run, and seen), 19,169 to 24,289; the table is the same, and the hash moved to 8_706_316_493_947_038_325.
/// The thanksgiving demo, graphics-world-painting merged: main's 19,845 bytes plus the 5,120 of the five companion pictures, predicted before the run and seen.
const PINNED_PACKAGE: (usize, u64) = (24965, 14_847_884_569_308_049_585);

/// What moved on 2026-11-09 (the dev ball joint, `docs/decisions/joints-and-cars.md`, step 2), and what did not. Every line has `joint none` or `joint PositiveY` after the docking face (the
/// word `joint` is new, so every line is longer), and the new block is line 13. **Behaviour digests and face numbers moved for the old blocks, and the control says why:** with the new block
/// taken out of Cave's package, every old block's text, with the word `joint none` taken out, has the digest it had before (13 of 13) and the palette line is the old one to the digit (317
/// colours); so what changed is the word, and the numbers of materials: the new block has a colour of its own, which takes a place after the 13 blocks' (every number after them is 1 more,
/// the colours of the states of the fire among them: 13 to 14), and its blue end is a colour of a face (every number after the faces' own colours is 1 more again, the pictures' among them:
/// 29 to 31 for the stone's, 221 to 223 for the port's). The seat's brown (27 to 28) and the engine's red (28 to 29) are one more only, as they come before the blue.
const PINNED_TABLE: &[&str] = &[
    " 0 base_blocks:air: colour #000000, 1 states, state 0 faces [0 0 0 0 0 0], front none, axis none, docking none, joint none, placement Fixed, behaviour 67ba0384e7f1d7a0",
    " 1 base_blocks:stone: colour #606060, 7 states, state 0 faces [36 37 38 39 40 41], front none, axis none, docking none, joint none, placement Fixed, behaviour 9ec485ab932549ea",
    " 2 base_blocks:dirt: colour #81703b, 7 states, state 0 faces [60 61 62 63 64 65], front none, axis none, docking none, joint none, placement Fixed, behaviour 463a70b309e67463",
    " 3 base_blocks:grass: colour #1e8929, 7 states, state 0 faces [84 85 86 87 88 89], front none, axis none, docking none, joint none, placement Fixed, behaviour b6bbdf29d0cf441c",
    " 4 base_blocks:lamp: colour #ffeba6, 1 states, state 0 faces [108 109 110 111 112 113], front none, axis none, docking none, joint none, placement Fixed, behaviour 9436b63ab9d4ffd4",
    " 5 base_blocks:wood: colour #887433, 7 states, state 0 faces [132 133 134 135 136 137], front none, axis PositiveY, docking none, joint none, placement AlongPlacedFace(PositiveY), behaviour d874ba8927725d78",
    " 6 base_blocks:fire: colour #f2801a, 1 states, state 0 faces [16 16 16 16 16 16], front none, axis none, docking none, joint none, placement Fixed, behaviour 07de7d108c53ce59",
    " 7 base_blocks:great_river_water: colour #283de8, 1 states, state 0 faces [180 181 182 183 184 185], front none, axis none, docking none, joint none, placement Fixed, behaviour 542c975821b6070f",
    " 8 base_blocks:leaves: colour #1e862c, 1 states, state 0 faces [204 205 206 207 208 209], front none, axis none, docking none, joint none, placement Fixed, behaviour f7eb10c38ab0c906",
    " 9 base_blocks:glass: colour #e1f3ff, 1 states, state 0 faces [228 229 230 231 232 233], front none, axis none, docking none, joint none, placement Fixed, behaviour ad1bb9bab5251b11",
    "10 voxunami_cave:dev_docking_port: colour #797658, 1 states, state 0 faces [252 253 254 255 256 257], front PositiveX, axis PositiveY, docking PositiveX, joint none, placement AlongPlacedFaceFacingPlayer { axis: PositiveY, front: PositiveX }, behaviour 154b09e6ef89c3a0",
    "11 voxunami_cave:dev_command_seat: colour #2b3638, 1 states, state 0 faces [11 30 30 11 11 11], front PositiveX, axis none, docking none, joint none, placement FrontToPlayer(PositiveX), behaviour 90ce1a983a31bff7",
    "12 voxunami_cave:dev_rocket_engine: colour #2b3638, 1 states, state 0 faces [12 12 12 31 12 12], front none, axis PositiveY, docking none, joint none, placement AlongPlacedFace(PositiveY), behaviour 4fb28115a145d55d",
    "13 voxunami_cave:dev_ball_joint: colour #2b3638, 1 states, state 0 faces [13 13 13 32 13 13], front none, axis PositiveY, docking none, joint PositiveY, placement AlongPlacedFace(PositiveY), behaviour 4eaa9df8828cd9da",
    "14 voxunami_cave:dev_hinge_joint: colour #363026, 1 states, state 0 faces [33 33 14 34 14 14], front PositiveX, axis PositiveY, docking none, joint PositiveY, line PositiveX, placement AlongPlacedFaceFacingPlayer { axis: PositiveY, front: PositiveX }, behaviour 2dc4f4451c59c21a",
    "15 voxunami_cave:dev_axle_joint: colour #262e3d, 1 states, state 0 faces [15 15 15 35 15 15], front none, axis PositiveY, docking none, joint PositiveY, line PositiveY, placement AlongPlacedFace(PositiveY), behaviour 88eb58a6ce77c272",
    "palette: 348 colours, digest 39a084ed4c411ba2, pictures 312 on it, digest f4042ec769eb5e85",
];
