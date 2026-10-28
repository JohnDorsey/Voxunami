//! Data packs in Cave: a server started with a pack has the pack's blocks, a client that has never heard of the pack
//! draws them as the server says, a recording of the session carries them, and a world saved with them loads without
//! the pack.

mod common;

use base_blocks::EMISSION;
use common::{Player, Rig, SEED, VIEW_RADIUS, above_ground, ground_height};
use entities::Inventory;
use game_shell::client_plugin::MouseButton;
use game_shell::files::{open_recording, record_connection};
use game_shell::session::{Mode, Session};
use game_shell::view::camera_at;
use game_shell::{GameContent, Identity, KeyCode, RawFile, RawPack};
use namespaced_id::NamespacedId;
use player_presence::player_kind;
use transport::Connection;
use voxel_space::VoxelCoordinate;
use voxunami_cave::content::CaveContent;
use voxunami_cave::game::{Cave, request_block};
use world_storage::ParsedWorld;

use common::PlayerExt;

fn id(text: &str) -> NamespacedId {
    text.parse().unwrap()
}

fn file(name: &str, text: &str) -> RawFile {
    RawFile {
        name: name.to_owned(),
        text: text.to_owned(),
    }
}

const MARBLE: [f32; 3] = [0.9, 0.9, 0.95];

/// A pack that adds a plain block and a glowing one, and makes lamps brighter.
fn the_pack() -> Vec<RawPack> {
    vec![RawPack {
        folder: "my_pack".to_owned(),
        manifest: r#"{ "id": "my_pack", "depends_on": ["base_blocks"] }"#.to_owned(),
        blocks: vec![
            file("marble", r#"{ "color": [0.9, 0.9, 0.95] }"#),
            file(
                "glow_stone",
                r#"{ "color": [0.2, 0.9, 0.4], "properties": { "base_blocks:emission": 10 } }"#,
            ),
        ],
        patches: vec![file(
            "brighter_lamps",
            r#"{ "target": "base_blocks:lamp", "operations": [
                { "op": "test", "property": "base_blocks:emission", "value": 64 },
                { "op": "set", "property": "base_blocks:emission", "value": 90 } ] }"#,
        )],
        bodies: Vec::new(),
    }]
}

fn content_with_the_pack() -> CaveContent {
    let mut content = CaveContent::new();
    let warnings = content.apply_packs(&the_pack()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    content
}

fn rig_with_the_pack() -> Rig {
    let content = content_with_the_pack();
    let world = content.new_overworld_world(SEED);
    Rig::with_world(content, world)
}

/// The number the server has for a block.
fn raw_in(content: &CaveContent, block: &str) -> u32 {
    content.registries()[0]
        .1
        .raw_id_of(&id(block))
        .unwrap_or_else(|| panic!("no block {block}"))
}

/// A player hovering above the ground at the column (16, 16), and the voxel above that ground.
fn above_the_ground(
    rig: &mut Rig,
    connection: Option<Box<dyn Connection>>,
) -> (Player, VoxelCoordinate) {
    let mut player = match connection {
        Some(connection) => rig.join_with(connection, [16.5, 110.0, 16.5], 0.0, 0.0),
        None => rig.join([16.5, 110.0, 16.5], 0.0, 0.0),
    };
    rig.settle(&mut [&mut player]);
    player.camera = above_ground(&player, 16, 16, 2.0);
    player.teleport_to_camera();
    rig.settle(&mut [&mut player]);
    let ground = ground_height(&player, 16, 16);
    (player, VoxelCoordinate::new(16, ground + 1, 16))
}

#[test]
fn a_server_with_a_pack_has_its_blocks_after_the_games_own_and_its_patches_applied() {
    let content = content_with_the_pack();
    assert_eq!(
        raw_in(&content, "base_blocks:fire"),
        6,
        "the games blocks keep their numbers"
    );
    // In the order of the file names: glow_stone before marble, after the games own blocks (the base blocks, and then the dev docking port).
    assert_eq!(raw_in(&content, "my_pack:glow_stone"), 9);
    assert_eq!(raw_in(&content, "my_pack:marble"), 10);
    let table = content.block_table();
    let emission = table.handle(&EMISSION).unwrap();
    assert_eq!(
        table.get_of(emission, content.blocks.lamp, 0),
        90,
        "patched"
    );
    assert_eq!(
        table.get_of(emission, 9, 0),
        10,
        "a block of the pack that glows"
    );
    assert_eq!(table.get_of(emission, 10, 0), 0);
}

#[test]
fn a_pack_that_cannot_be_loaded_is_an_error_that_names_the_file_and_the_game_is_untouched() {
    let mut content = CaveContent::new();
    let before = content.package_data();
    let mut broken = the_pack();
    broken[0].patches[0].text = broken[0].patches[0].text.replace("64", "65");
    let errors = content.apply_packs(&broken).unwrap_err();
    assert_eq!(
        errors.to_string(),
        "my_pack/patches/brighter_lamps.json: /operations/0: the test failed: base_blocks:emission is 64, not 65"
    );
    assert_eq!(content.package_data(), before);
}

#[test]
fn a_client_that_has_never_heard_of_the_pack_draws_its_blocks_as_the_server_says_and_sees_them_placed()
 {
    let mut rig = rig_with_the_pack();
    let (mut player, at) = above_the_ground(&mut rig, None);
    let mut onlooker = rig.join([20.5, 110.0, 20.5], 0.0, 0.0);
    rig.settle(&mut [&mut player, &mut onlooker]);

    // The client's own game is Cave without the pack, and still the pack's colours are what it has.
    for client in [&player, &onlooker] {
        let numbering = client.content().registries()[0].1;
        let marble = numbering
            .raw_id_of(&id("my_pack:marble"))
            .expect("known to the client as a name");
        assert_eq!(marble, raw_in(&rig.content, "my_pack:marble"));
        assert_eq!(client.block_table().color(marble), MARBLE);
        let glow = numbering.raw_id_of(&id("my_pack:glow_stone")).unwrap();
        assert_eq!(client.block_table().color(glow), [0.2, 0.9, 0.4]);
    }
    assert!(
        !player
            .package()
            .blocks
            .iter()
            .any(|(_, definition)| definition.is_none())
    );

    request_block(&mut player, at, "my_pack:marble");
    rig.settle(&mut [&mut player, &mut onlooker]);
    let marble = raw_in(&rig.content, "my_pack:marble");
    for client in [&player, &onlooker] {
        assert_eq!(client.mesher().material_at(at), Some(marble));
    }
    let (brick, local) = at.split();
    assert_eq!(
        match rig.server.world().main().effective_brick(brick) {
            world_storage::BrickSource::Edited(edited) =>
                edited.get(rig.content.block_channel(), local),
            other => panic!("{other:?}"),
        },
        marble,
        "the server holds the block too"
    );
}

#[test]
fn a_recording_of_a_session_with_a_pack_shows_the_packs_blocks_with_no_pack_installed() {
    let directory =
        std::env::temp_dir().join(format!("voxunami_cave_packs_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("session.voxreplay");
    let mut rig = rig_with_the_pack();
    let recorded: Box<dyn Connection> =
        record_connection(rig.connector.connect().unwrap(), &path).unwrap();
    let (mut player, at) = above_the_ground(&mut rig, Some(recorded));
    request_block(&mut player, at, "my_pack:glow_stone");
    rig.settle(&mut [&mut player]);
    let live_package = player.package().clone();
    drop(player);

    let connection: Box<dyn Connection> = Box::new(open_recording(&path).unwrap());
    let mut replay = Session::<_, Cave>::connect(
        connection,
        &Identity::generate(),
        Mode::Replaying,
        VIEW_RADIUS,
        camera_at([0.0; 3], 0.0, 0.0),
        || {},
    )
    .unwrap();
    while replay.advance_one_tick().is_ok() {}
    replay.settle(|| {}).unwrap();
    assert_eq!(replay.package(), &live_package);
    let glow = raw_in(&rig.content, "my_pack:glow_stone");
    assert_eq!(replay.block_table().color(glow), [0.2, 0.9, 0.4]);
    assert_eq!(replay.mesher().material_at(at), Some(glow));
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_world_saved_with_a_packs_block_loads_without_the_pack_and_saves_back_the_same() {
    let mut rig = rig_with_the_pack();
    let (mut player, at) = above_the_ground(&mut rig, None);
    request_block(&mut player, at, "my_pack:marble");
    rig.settle(&mut [&mut player]);
    let saved = rig.server.world().save(&rig.content.registries()).unwrap();

    // Another run, without the pack.
    let mut without = CaveContent::new();
    let parsed = ParsedWorld::parse(&saved).unwrap();
    let seed = parsed.world_seed();
    let (loaded, report) = parsed.load(without.load_context(seed)).unwrap();
    assert_eq!(
        report.missing_content.len(),
        2,
        "marble and glow stone are not known now: {:?}",
        report.missing_content
    );
    assert_eq!(loaded.save(&without.registries()).unwrap(), saved);

    // And with the pack again, the block is back as what it was.
    let mut again = content_with_the_pack();
    let (reloaded, report) = ParsedWorld::parse(&saved)
        .unwrap()
        .load(again.load_context(seed))
        .unwrap();
    assert!(
        report.missing_content.is_empty(),
        "{:?}",
        report.missing_content
    );
    assert_eq!(reloaded.save(&again.registries()).unwrap(), saved);
}

// ---- a pack's block that a player can place ----

/// A pack whose block says it can be placed, which gives it an item and a place among the number keys.
fn the_placeable_pack() -> Vec<RawPack> {
    vec![RawPack {
        folder: "my_pack".to_owned(),
        manifest: r#"{ "id": "my_pack", "depends_on": ["base_blocks"] }"#.to_owned(),
        blocks: vec![
            file(
                "ruby",
                r#"{ "color": [0.8, 0.1, 0.2], "properties": { "voxunami_cave:placeable": true } }"#,
            ),
            file("marble", r#"{ "color": [0.9, 0.9, 0.95] }"#),
        ],
        patches: Vec::new(),
        bodies: Vec::new(),
    }]
}

fn rig_with_the_placeable_pack() -> Rig {
    let mut content = CaveContent::new();
    content.apply_packs(&the_placeable_pack()).unwrap();
    let world = content.new_overworld_world(SEED);
    Rig::with_world(content, world)
}

fn ticks(rig: &mut Rig, player: &mut Player, count: usize) {
    for _ in 0..count {
        rig.step();
        player.advance().unwrap();
        let _ = player.update_view();
    }
}

fn held_of(rig: &Rig, item: &str) -> u64 {
    rig.server
        .world()
        .entities()
        .iter()
        .find(|entity| entity.kind == player_kind())
        .and_then(|entity| entity.components.get::<Inventory>().ok().flatten())
        .map_or(0, |inventory| inventory.count_of(&id(item)))
}

#[test]
fn cave_says_which_of_its_own_blocks_a_player_can_place_and_in_what_order() {
    use voxunami_cave::items::{PLACEABLE_BY_DEFAULT, placeable_blocks};
    let placeable = placeable_blocks(&CaveContent::new().package_data());
    let expected: Vec<NamespacedId> = PLACEABLE_BY_DEFAULT.iter().map(|name| id(name)).collect();
    assert_eq!(placeable, expected);
    // With the pack, its block that says so follows them, and the one that does not say so is not there.
    let mut with = CaveContent::new();
    with.apply_packs(&the_placeable_pack()).unwrap();
    let mut longer = expected;
    longer.push(id("my_pack:ruby"));
    assert_eq!(placeable_blocks(&with.package_data()), longer);
}

#[test]
fn a_block_of_a_pack_that_can_be_placed_has_a_number_key_an_item_and_is_used_up_like_any_other() {
    let mut rig = rig_with_the_placeable_pack();
    let (mut player, at) = above_the_ground(&mut rig, None);
    ticks(&mut rig, &mut player, 2);
    let ruby = raw_in(&rig.content, "my_pack:ruby");

    // The first key is stone, as it was; the seventh is the dev docking port and the eighth the pack's block; a key past the end of the list is
    // the first block again, never nothing or a crash.
    assert!(
        player.hud_text().contains("placing stone (whole)"),
        "{}",
        player.hud_text()
    );
    player.key_pressed(KeyCode::Digit8);
    assert!(
        player.hud_text().contains("placing ruby (whole)"),
        "{}",
        player.hud_text()
    );
    player.key_pressed(KeyCode::Digit9);
    assert!(player.hud_text().contains("placing stone (whole)"));
    player.key_pressed(KeyCode::Digit8);

    // Creative: the mouse button places it.
    player.mouse_pressed(MouseButton::Secondary);
    rig.settle(&mut [&mut player]);
    assert_eq!(player.mesher().material_at(at), Some(ruby));

    // Survival: breaking it leaves its item, which comes into the inventory...
    player.send_command("/gamemode survival");
    ticks(&mut rig, &mut player, 2);
    player.mouse_pressed(MouseButton::Primary);
    ticks(&mut rig, &mut player, 16);
    assert_eq!(
        held_of(&rig, "my_pack:ruby"),
        1,
        "the block left an item and it was picked up"
    );
    assert!(
        player
            .hud_text()
            .contains("placing ruby (whole), 1 in hand"),
        "{}",
        player.hud_text()
    );

    // ...and placing it uses it up.
    player.mouse_pressed(MouseButton::Secondary);
    ticks(&mut rig, &mut player, 4);
    assert_eq!(held_of(&rig, "my_pack:ruby"), 0);
    assert_eq!(player.mesher().material_at(at), Some(ruby));
}

#[test]
fn a_block_of_a_pack_that_does_not_say_it_can_be_placed_leaves_no_item_when_it_is_broken() {
    use voxunami_cave::items::dropped_item_kind;
    let mut rig = rig_with_the_placeable_pack();
    let (mut player, at) = above_the_ground(&mut rig, None);
    ticks(&mut rig, &mut player, 2);
    // Put one there by request (creative), then break it in survival.
    request_block(&mut player, at, "my_pack:marble");
    rig.settle(&mut [&mut player]);
    let marble = raw_in(&rig.content, "my_pack:marble");
    assert_eq!(player.mesher().material_at(at), Some(marble));
    player.send_command("/gamemode survival");
    ticks(&mut rig, &mut player, 2);
    player.mouse_pressed(MouseButton::Primary);
    ticks(&mut rig, &mut player, 8);
    assert_ne!(
        player.mesher().material_at(at),
        Some(marble),
        "it was broken"
    );
    let dropped = rig
        .server
        .world()
        .entities()
        .iter()
        .filter(|entity| entity.kind == dropped_item_kind())
        .count();
    assert_eq!(dropped, 0, "nothing falls out of a block that has no item");
}

#[test]
fn the_hotbar_of_a_player_who_has_never_heard_of_a_pack_has_a_swatch_for_its_placeable_block_in_its_colour()
 {
    use game_shell::screen::{ScreenText, Source};
    let mut rig = rig_with_the_placeable_pack();
    let (mut player, _) = above_the_ground(&mut rig, None);
    ticks(&mut rig, &mut player, 2);
    let screen = player.screen(&ScreenText::default());
    let index = screen
        .sources
        .iter()
        .position(|source| matches!(source, Source::Overlay(0)))
        .expect("the hotbar is on the screen");
    let swatches: Vec<ui::Color> = screen.uis[index]
        .root
        .children
        .iter()
        .filter_map(|node| match node {
            ui::Node::Swatch { color, .. } => Some(*color),
            _ => None,
        })
        .collect();
    assert_eq!(
        swatches.len(),
        8,
        "the seven of the game (the dev docking port is the seventh) and the pack's ruby"
    );
    assert_eq!(
        swatches[7],
        ui::Color([204, 26, 51]),
        "ruby is [0.8, 0.1, 0.2]"
    );
}
