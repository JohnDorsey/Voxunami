//! Data packs in Cave: a server started with a pack has the pack's blocks, a client that has never heard of the pack
//! draws them as the server says, a recording of the session carries them, and a world saved with them loads without
//! the pack.

mod common;

use base_blocks::EMISSION;
use common::{Player, Rig, SEED, VIEW_RADIUS, above_the_ground, raw_in};
use entities::Inventory;
use game_shell::client_plugin::MouseButton;
use game_shell::files::{open_recording, record_connection};
use game_shell::session::{Mode, Session};
use game_shell::view::camera_at;
use game_shell::{GameContent, Identity, KeyCode, RawFile, RawPack, RawTexture};
use namespaced_id::NamespacedId;
use pack_archive::{Archive, FileIndex};
use player_presence::player_kind;
use transport::Connection;
use voxunami_cave::content::CaveContent;
use voxunami_cave::game::{Cave, request_block};
use world_storage::ParsedWorld;

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
        textures: Vec::new(),
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

/// The bytes of a PNG picture of one colour, 16 pixels on a side.
fn picture_of(rgba: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, 16, 16);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let data: Vec<u8> = (0..16 * 16).flat_map(|_| rgba).collect();
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&data).unwrap();
    writer.finish().unwrap();
    bytes
}

#[test]
fn a_pack_may_repaint_a_picture_of_the_game_and_the_colours_that_follow_it_follow_and_a_block_of_its_own_may_use_the_picture()
 {
    let mut content = CaveContent::new();
    let colour_of = |content: &CaveContent, block: &str| {
        let package = content.package_data();
        let (_, definition) = package
            .blocks
            .iter()
            .find(|(identifier, _)| *identifier == id(block))
            .unwrap_or_else(|| panic!("no block {block}"));
        definition.as_ref().unwrap().color
    };
    let before = colour_of(&content, "base_blocks:stone");
    let pack = RawPack {
        folder: "repaint".to_owned(),
        manifest: r#"{ "id": "repaint", "depends_on": ["base_blocks"] }"#.to_owned(),
        blocks: vec![file("slab", r#"{ "texture": "base_blocks:stone" }"#)],
        patches: Vec::new(),
        bodies: Vec::new(),
        textures: vec![RawTexture {
            name: "base_blocks/stone".to_owned(),
            bytes: picture_of([255, 255, 255, 255]),
        }],
    };
    let warnings = content.apply_packs(&[pack]).unwrap();
    // The game's stone follows its picture, which the pack replaced: it is white now, as is the pack's own block, which uses the same picture.
    assert_ne!(before, [1.0; 3]);
    for block in ["base_blocks:stone", "repaint:slab"] {
        let colour = colour_of(&content, block);
        for component in colour {
            assert!((component - 1.0).abs() < 1e-5, "{block}: {colour:?}");
        }
    }
    // A block whose colour was its own is left alone, and the replacement is said, with the loss of the stone's companions (it has a specular and a normal picture since 2026-11-18).
    assert_eq!(colour_of(&content, "base_blocks:lamp"), [1.0, 0.92, 0.65]);
    assert_eq!(
        warnings,
        [
            "repaint/textures/base_blocks/stone.png: replaces base_blocks:stone from plugins/base_blocks/textures/stone.png (its specular and normal pictures go; the pack brings its own)"
        ]
    );
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

#[test]
fn a_server_with_a_pack_has_its_blocks_after_the_games_own_and_its_patches_applied() {
    let content = content_with_the_pack();
    assert_eq!(
        raw_in(&content, "base_blocks:fire"),
        6,
        "the games blocks keep their numbers"
    );
    // In the order of the file names: glow_stone before marble, after the games own blocks (the base blocks, and then the dev docking port, command seat, rocket engine and ball joint).
    assert_eq!(raw_in(&content, "my_pack:glow_stone"), 14);
    assert_eq!(raw_in(&content, "my_pack:marble"), 15);
    let table = content.block_table();
    let emission = table.handle(&EMISSION).unwrap();
    assert_eq!(
        table.get_of(emission, content.blocks.lamp, 0),
        90,
        "patched"
    );
    assert_eq!(
        table.get_of(emission, 14, 0),
        10,
        "a block of the pack that glows"
    );
    assert_eq!(table.get_of(emission, 15, 0), 0);
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
        textures: Vec::new(),
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

    // The first key is stone, as it was; the seventh is leaves, the eighth glass, the ninth the dev docking port, then come the command seat, the rocket engine and the ball joint
    // and, last, the pack's block, which has no number key: the right square bracket reaches it from the ninth (by the seat, the engine and the joint), and goes on round to the
    // first block.
    assert!(
        player.hud_text().contains("placing stone (whole)"),
        "{}",
        player.hud_text()
    );
    player.key_pressed(KeyCode::Digit9);
    for _ in 0..4 {
        player.key_pressed(KeyCode::BracketRight);
    }
    assert!(
        player.hud_text().contains("placing ruby (whole)"),
        "{}",
        player.hud_text()
    );
    player.key_pressed(KeyCode::BracketRight);
    assert!(player.hud_text().contains("placing stone (whole)"));
    player.key_pressed(KeyCode::BracketLeft);

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
        13,
        "the twelve of the game (the dev docking port, command seat, rocket engine and ball joint are the last four) and the pack's ruby"
    );
    assert_eq!(
        swatches[12],
        ui::Color([204, 26, 51]),
        "ruby is [0.8, 0.1, 0.2]"
    );
}

// ---- Packs a client keeps as files (`specs/pack-archive-spec.md`, step 4), in the real game ----

/// A folder of packs on disk: `my_pack`, which clients keep (with a note beside its files), and `quiet`, which says they do not.
fn a_folder_of_packs(name: &str) -> std::path::PathBuf {
    let directory =
        std::env::temp_dir().join(format!("voxunami_cave_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    for (path, text) in [
        (
            "my_pack/pack.json",
            r#"{ "id": "my_pack", "depends_on": ["base_blocks"] }"#,
        ),
        (
            "my_pack/blocks/marble.json",
            r#"{ "color": [0.9, 0.9, 0.95] }"#,
        ),
        ("my_pack/readme.md", "carried to clients too"),
        (
            "quiet/pack.json",
            r#"{ "id": "quiet", "replicate": false }"#,
        ),
        ("quiet/blocks/ember.json", r#"{ "color": [0.9, 0.3, 0.1] }"#),
    ] {
        let file = directory.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    directory
}

/// A player who joins `rig` over `connection`, keeping the packs it is sent in `store`.
fn join_keeping_packs(
    rig: &mut Rig,
    connection: Box<dyn Connection>,
    store: &client::MemoryPackStore,
) -> common::Player {
    let server = &mut rig.server;
    Session::<_, Cave>::connect_with_packs(
        connection,
        &Identity::from_secret([9; 32]),
        game_shell::session::Mode::Playing,
        VIEW_RADIUS,
        camera_at([0.0, 40.0, 0.0], 0.0, 0.0),
        || {
            server.step();
        },
        Box::new(store.clone()),
    )
    .unwrap()
}

#[test]
fn a_server_made_from_a_folder_of_packs_sends_the_ones_that_clients_keep_and_a_recording_and_a_cache_both_hold_them()
 {
    let directory = a_folder_of_packs("replicated");
    let content = game_shell::content_with_packs::<Cave>(Some(&directory)).unwrap();
    // only the pack that does not say no is replicated, in the identity every tool prints
    let listed: Vec<String> = content
        .replicated_packs()
        .iter()
        .map(|pack| pack.listing().id.clone())
        .collect();
    assert_eq!(listed, ["my_pack"]);
    let (_, identity) = data_packs::pack_identities(&directory)
        .unwrap()
        .into_iter()
        .find(|(name, _)| name == "my_pack")
        .unwrap();
    let root = identity.unwrap();
    assert_eq!(content.replicated_packs()[0].root(), root);
    let archive_bytes = content.replicated_packs()[0].archive_bytes().to_vec();

    let world = content.new_overworld_world(SEED);
    let mut rig = Rig::with_world(content, world);
    let recordings =
        std::env::temp_dir().join(format!("voxunami_cave_recordings_{}", std::process::id()));
    std::fs::create_dir_all(&recordings).unwrap();

    // A first player, with nothing in its store, is sent the pack and keeps it.
    let store = client::MemoryPackStore::new();
    let first_path = recordings.join("first.voxreplay");
    let recorded = record_connection(rig.connector.connect().unwrap(), &first_path).unwrap();
    let mut first = join_keeping_packs(&mut rig, recorded, &store);
    for _ in 0..4 {
        rig.settle(&mut [&mut first]);
    }
    assert!(first.client().packs().is_complete());
    let held = first.client().packs().get(&root).expect("the pack is held");
    assert_eq!(held.archive.to_bytes(), archive_bytes);
    assert!(
        held.archive.get("readme.md").is_some(),
        "the note is carried too"
    );
    assert_eq!(
        store.get("my_pack", &root),
        Some(archive_bytes.clone()),
        "kept for the next time"
    );
    drop(first);

    // A second player whose store has it is not sent it, and its recording has it all the same.
    let second_path = recordings.join("second.voxreplay");
    let recorded = record_connection(rig.connector.connect().unwrap(), &second_path).unwrap();
    let mut second = join_keeping_packs(&mut rig, recorded, &store);
    for _ in 0..2 {
        rig.settle(&mut [&mut second]);
    }
    assert!(second.client().packs().is_complete());
    drop(second);

    // Both recordings replay with the pack, and nothing installed.
    for path in [&first_path, &second_path] {
        let connection: Box<dyn Connection> = Box::new(open_recording(path).unwrap());
        let mut replay = Session::<_, Cave>::connect(
            connection,
            &Identity::generate(),
            game_shell::session::Mode::Replaying,
            VIEW_RADIUS,
            camera_at([0.0; 3], 0.0, 0.0),
            || {},
        )
        .unwrap();
        while replay.advance_one_tick().is_ok() {}
        replay.settle(|| {}).unwrap();
        let held = replay
            .client()
            .packs()
            .get(&root)
            .unwrap_or_else(|| panic!("{path:?} replays with the pack"));
        assert_eq!(held.archive.to_bytes(), archive_bytes, "{path:?}");
    }
    std::fs::remove_dir_all(&recordings).unwrap();
    std::fs::remove_dir_all(&directory).unwrap();
}

// ---- The client loads the packs it was sent and checks that they make the package the server sent (step 5) ----

use game_shell::PackCheck;

/// A server for the content a folder of packs makes, and a player over a recorded connection that keeps what it is sent in `store`.
fn server_and_player(
    content: CaveContent,
    store: &client::MemoryPackStore,
    recording: &std::path::Path,
) -> (Rig, common::Player) {
    let world = content.new_overworld_world(SEED);
    let mut rig = Rig::with_world(content, world);
    let recorded = record_connection(rig.connector.connect().unwrap(), recording).unwrap();
    let player = join_keeping_packs(&mut rig, recorded, store);
    (rig, player)
}

fn settle_a_few(rig: &mut Rig, player: &mut common::Player) {
    for _ in 0..4 {
        rig.settle(&mut [&mut *player]);
    }
}

/// The recording replayed as a client with nothing installed, to its end.
fn replayed(path: &std::path::Path) -> common::Player {
    let connection: Box<dyn Connection> = Box::new(open_recording(path).unwrap());
    let mut replay = Session::<_, Cave>::connect(
        connection,
        &Identity::generate(),
        game_shell::session::Mode::Replaying,
        VIEW_RADIUS,
        camera_at([0.0; 3], 0.0, 0.0),
        || {},
    )
    .unwrap();
    while replay.advance_one_tick().is_ok() {}
    replay.settle(|| {}).unwrap();
    replay
}

#[test]
fn a_client_that_holds_every_pack_loads_them_over_its_own_game_and_makes_the_package_the_server_sent()
 {
    let directory = a_folder_of_packs_that_all_replicate("check_agrees");
    let content = game_shell::content_with_packs::<Cave>(Some(&directory)).unwrap();
    assert!(content.packs_whole());
    let recordings =
        std::env::temp_dir().join(format!("voxunami_cave_check_{}", std::process::id()));
    std::fs::create_dir_all(&recordings).unwrap();
    let path = recordings.join("agrees.voxreplay");
    let (mut rig, mut player) = server_and_player(content, &client::MemoryPackStore::new(), &path);
    assert_eq!(
        player.pack_check(),
        &PackCheck::Waiting,
        "before it has any tick"
    );
    settle_a_few(&mut rig, &mut player);
    assert_eq!(player.pack_check(), &PackCheck::Agrees);
    // the packs made blocks that the client's own game does not have, so this was a real load, and not the game's own compared with itself
    assert!(
        player
            .package()
            .blocks
            .iter()
            .any(|(block, definition)| *block == id("my_pack:marble") && definition.is_some())
    );
    drop(player);
    // a replay of the recording loads the packs it holds, and agrees as the live client did
    assert_eq!(replayed(&path).pack_check(), &PackCheck::Agrees);
    std::fs::remove_dir_all(&recordings).unwrap();
    std::fs::remove_dir_all(&directory).unwrap();
}

/// A folder with the one pack `my_pack`, which clients keep.
fn a_folder_of_packs_that_all_replicate(name: &str) -> std::path::PathBuf {
    let directory = a_folder_of_packs(name);
    std::fs::remove_dir_all(directory.join("quiet")).unwrap();
    directory
}

#[test]
fn a_server_that_runs_only_the_games_own_content_is_checked_too_and_one_with_a_pack_it_does_not_replicate_cannot_be()
 {
    let recordings =
        std::env::temp_dir().join(format!("voxunami_cave_check_none_{}", std::process::id()));
    std::fs::create_dir_all(&recordings).unwrap();
    // no packs: the client's own copy of the game against the package the server sent
    let (mut rig, mut player) = server_and_player(
        CaveContent::new(),
        &client::MemoryPackStore::new(),
        &recordings.join("none.voxreplay"),
    );
    settle_a_few(&mut rig, &mut player);
    assert_eq!(player.pack_check(), &PackCheck::Agrees);
    drop(player);
    // `quiet` says it is not replicated: there is more behind the package than the client was sent, and it says so
    let directory = a_folder_of_packs("check_quiet");
    let content = game_shell::content_with_packs::<Cave>(Some(&directory)).unwrap();
    assert!(!content.packs_whole());
    let path = recordings.join("quiet.voxreplay");
    let (mut rig, mut player) = server_and_player(content, &client::MemoryPackStore::new(), &path);
    settle_a_few(&mut rig, &mut player);
    assert!(
        player.client().packs().is_complete(),
        "it still keeps the pack that is replicated"
    );
    assert_eq!(player.pack_check(), &PackCheck::NotApplicable);
    drop(player);
    assert_eq!(replayed(&path).pack_check(), &PackCheck::NotApplicable);
    std::fs::remove_dir_all(&recordings).unwrap();
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn packs_that_do_not_make_the_package_the_server_sent_are_told_block_by_block_and_the_client_goes_on_with_the_servers()
 {
    // The server's package was made from one marble and its packs say another: a loader, a schema or an order that differs between two programs would look like this.
    let made_from = a_folder_of_packs_that_all_replicate("check_made_from");
    let says = a_folder_of_packs_that_all_replicate("check_says");
    std::fs::write(
        says.join("my_pack/blocks/marble.json"),
        r#"{ "color": [0.1, 0.2, 0.3] }"#,
    )
    .unwrap();
    let mut content = game_shell::content_with_packs::<Cave>(Some(&made_from)).unwrap();
    let other = game_shell::content_with_packs::<Cave>(Some(&says)).unwrap();
    content.set_replicated_packs(other.replicated_packs().to_vec());
    let recordings =
        std::env::temp_dir().join(format!("voxunami_cave_check_bad_{}", std::process::id()));
    std::fs::create_dir_all(&recordings).unwrap();
    let path = recordings.join("bad.voxreplay");
    let (mut rig, mut player) = server_and_player(content, &client::MemoryPackStore::new(), &path);
    settle_a_few(&mut rig, &mut player);
    let PackCheck::Disagrees(lines) = player.pack_check() else {
        panic!(
            "the packs and the package differ: {:?}",
            player.pack_check()
        );
    };
    assert!(
        lines.iter().any(|line| line.contains("my_pack:marble")),
        "{lines:?}"
    );
    // told once to the program, which says it where the player will see it, and the client draws what the server sent
    let report = player.take_pack_check_report().expect("a report");
    assert!(report.contains("my_pack:marble"), "{report}");
    assert_eq!(player.take_pack_check_report(), None, "once");
    let marble = raw_in(&rig.content, "my_pack:marble");
    assert_eq!(player.block_table().color(marble), MARBLE);
    drop(player);
    assert!(matches!(
        replayed(&path).pack_check(),
        PackCheck::Disagrees(_)
    ));
    std::fs::remove_dir_all(&recordings).unwrap();
    std::fs::remove_dir_all(&made_from).unwrap();
    std::fs::remove_dir_all(&says).unwrap();
}

// ---- Files a server keeps to itself: the `server` folder of a pack (step 6), in the real game ----

#[test]
fn a_client_is_never_sent_the_server_folder_of_a_pack_yet_holds_the_pack_the_server_has_the_identity_of_and_agrees_with_it()
 {
    let directory = a_folder_of_packs_that_all_replicate("withheld");
    let secret = directory.join("my_pack/server/rules.py");
    std::fs::create_dir_all(secret.parent().unwrap()).unwrap();
    std::fs::write(&secret, "SECRET_RULES = 1").unwrap();
    let content = game_shell::content_with_packs::<Cave>(Some(&directory)).unwrap();
    let served = &content.replicated_packs()[0];
    let root = served.root();
    // the identity is the one `check-pack` prints, and it has the code in it
    let (_, identity) = data_packs::pack_identities(&directory)
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(identity.unwrap(), root);
    let whole = Archive::from_files(
        [
            (
                "pack.json",
                &br#"{ "id": "my_pack", "depends_on": ["base_blocks"] }"#[..],
            ),
            ("blocks/marble.json", br#"{ "color": [0.9, 0.9, 0.95] }"#),
            ("readme.md", b"carried to clients too"),
            ("server/rules.py", b"SECRET_RULES = 1"),
        ]
        .map(|(path, bytes)| (path.to_owned(), bytes.to_vec())),
    )
    .unwrap();
    assert_eq!(
        served.root(),
        FileIndex::of(&whole, |path| path != "server/rules.py").root_hash()
    );
    assert_ne!(
        served.root(),
        FileIndex::of(&whole, |_| true).root_hash(),
        "the code is in the identity"
    );
    let sent_bytes = served.archive_bytes().to_vec();
    assert!(
        !sent_bytes
            .windows(16)
            .any(|window| window == b"SECRET_RULES = 1"),
        "the bytes that go over the wire do not hold the code"
    );

    let recordings = std::env::temp_dir().join(format!(
        "voxunami_cave_withheld_recordings_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&recordings).unwrap();
    let path = recordings.join("withheld.voxreplay");
    let store = client::MemoryPackStore::new();
    let (mut rig, mut player) = server_and_player(content, &store, &path);
    settle_a_few(&mut rig, &mut player);
    let held = player
        .client()
        .packs()
        .get(&root)
        .expect("the pack is held");
    assert_eq!(held.archive.to_bytes(), sent_bytes);
    assert!(held.archive.get("server/rules.py").is_none());
    assert!(held.archive.get("readme.md").is_some());
    assert_eq!(
        held.index
            .entries()
            .iter()
            .find(|entry| !entry.sent)
            .map(|entry| entry.path.as_str()),
        Some("server/rules.py"),
        "the client can see that a file is not sent, and not what it says"
    );
    // the pack without its code makes what the server sent
    assert_eq!(player.pack_check(), &PackCheck::Agrees);
    // kept for the next join under the same name, checked against the index when it is read again
    assert_eq!(store.get("my_pack", &root), Some(sent_bytes.clone()));
    drop(player);
    // a replay holds the pack as it was sent, and agrees
    let replay = replayed(&path);
    assert_eq!(
        replay
            .client()
            .packs()
            .get(&root)
            .unwrap()
            .archive
            .to_bytes(),
        sent_bytes
    );
    assert_eq!(replay.pack_check(), &PackCheck::Agrees);
    std::fs::remove_dir_all(&recordings).unwrap();
    std::fs::remove_dir_all(&directory).unwrap();
}
