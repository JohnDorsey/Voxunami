//! Reading the packs of a running Cave again (`hot-reload-plan.md`, step 5, phase 4): the players that were there are told, a block the packs gained is one that can be
//! put down and that everyone sees, a player who joins after has it in the welcome, and a recording of it all shows the same.

mod common;

use common::{Scene, above_the_ground, raw_in};
use game_shell::files::{open_recording, record_connection};
use game_shell::session::{Mode, Session};
use game_shell::view::camera_at;
use game_shell::{GameContent, Identity, RawFile, RawPack, ReloadError, Reloaded};
use transport::Connection;
use voxunami_cave::game::{Cave, request_block};

const MARBLE: [f32; 3] = [0.9, 0.9, 0.95];

fn file(name: &str, text: &str) -> RawFile {
    RawFile {
        name: name.to_owned(),
        text: text.to_owned(),
    }
}

/// A pack with a block of the colour `marble`, and the lamps of the game made as bright as `lamp` says.
fn the_pack(marble: &str, lamp: u32) -> Vec<RawPack> {
    vec![RawPack {
        textures: Vec::new(),
        folder: "my_pack".to_owned(),
        manifest: r#"{ "id": "my_pack", "depends_on": ["base_blocks"] }"#.to_owned(),
        blocks: vec![file("marble", &format!(r#"{{ "color": {marble} }}"#))],
        patches: vec![file(
            "lamps",
            &format!(
                r#"{{ "target": "base_blocks:lamp", "operations": [
                    {{ "op": "set", "property": "base_blocks:emission", "value": {lamp} }} ] }}"#
            ),
        )],
        bodies: Vec::new(),
    }]
}

fn reload(scene: &mut Scene, packs: &[RawPack]) -> Result<Reloaded, ReloadError> {
    let Scene { rig, package, .. } = scene;
    game_shell::reload_packs(&mut rig.content, packs, package, &mut rig.server)
}

#[test]
fn a_reload_tells_the_players_that_were_there_what_the_packs_gained_and_a_player_who_joins_after_is_welcomed_with_it()
 {
    let mut scene = Scene::new();
    let (mut player, _) = above_the_ground(&mut scene.rig, None);
    let mut onlooker = scene.rig.join([20.5, 110.0, 20.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player, &mut onlooker]);
    let before = player.package().blocks.len();
    assert!(
        player.content().registries()[0]
            .1
            .raw_id_of(&"my_pack:marble".parse().unwrap())
            .is_none()
    );

    let reloaded = reload(&mut scene, &the_pack("[0.9, 0.9, 0.95]", 90)).unwrap();
    assert_eq!((reloaded.blocks_added, reloaded.version), (1, 1));
    scene.rig.settle(&mut [&mut player, &mut onlooker]);

    let marble = raw_in(&scene.rig.content, "my_pack:marble");
    assert_eq!(marble as usize, before, "it takes the next number");
    let mut late = scene.rig.join([24.5, 110.0, 24.5], 0.0, 0.0);
    scene
        .rig
        .settle(&mut [&mut player, &mut onlooker, &mut late]);
    let served = scene.rig.content.package_data();
    for client in [&player, &onlooker, &late] {
        assert_eq!(client.package(), &served, "the package the server has now");
        assert_eq!(client.block_table().color(marble), MARBLE);
        assert_eq!(
            client.content().registries()[0]
                .1
                .raw_id_of(&"my_pack:marble".parse().unwrap()),
            Some(marble),
            "the client's own registry grew"
        );
    }
}

#[test]
fn a_block_the_packs_gained_is_put_down_and_seen_by_the_players_that_were_there_and_the_one_that_came_after()
 {
    let mut scene = Scene::new();
    let (mut player, at) = above_the_ground(&mut scene.rig, None);
    let mut onlooker = scene.rig.join([20.5, 110.0, 20.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player, &mut onlooker]);
    reload(&mut scene, &the_pack("[0.9, 0.9, 0.95]", 90)).unwrap();
    scene.rig.settle(&mut [&mut player, &mut onlooker]);

    request_block(&mut player, at, "my_pack:marble");
    scene.rig.settle(&mut [&mut player, &mut onlooker]);
    let mut late = scene.rig.join([24.5, 110.0, 24.5], 0.0, 0.0);
    scene
        .rig
        .settle(&mut [&mut player, &mut onlooker, &mut late]);
    let marble = raw_in(&scene.rig.content, "my_pack:marble");
    for client in [&player, &onlooker, &late] {
        assert_eq!(client.mesher().material_at(at), Some(marble));
    }
}

#[test]
fn a_changed_block_is_drawn_as_it_is_now_and_what_the_rules_read_of_it_is_what_it_is_now() {
    let mut scene = Scene::new();
    let (mut player, _) = above_the_ground(&mut scene.rig, None);
    let lamp = scene.rig.content.blocks.lamp;
    let emission = {
        let table = scene.package.table().current();
        let handle = table.handle(&base_blocks::EMISSION).unwrap();
        move |scene: &Scene| scene.package.table().current().get_of(handle, lamp, 0)
    };
    assert_eq!(emission(&scene), 64);

    reload(&mut scene, &the_pack("[0.9, 0.9, 0.95]", 90)).unwrap();
    scene.rig.settle(&mut [&mut player]);
    assert_eq!(emission(&scene), 90, "the rules read the new table");
    assert_eq!(
        player.block_table().get_of(
            player.block_table().handle(&base_blocks::EMISSION).unwrap(),
            lamp,
            0
        ),
        90,
        "and so does the client"
    );

    // The packs as they were: the lamp is the game's again.
    reload(&mut scene, &[]).unwrap();
    scene.rig.settle(&mut [&mut player]);
    assert_eq!(emission(&scene), 64);
    let marble = raw_in(&scene.rig.content, "my_pack:marble");
    assert!(
        player.package().blocks[marble as usize].1.is_none(),
        "a block the packs lost keeps its number, as missing content"
    );
}

#[test]
fn packs_that_are_no_good_change_nothing_and_a_player_is_told_nothing() {
    let mut scene = Scene::new();
    let (mut player, _) = above_the_ground(&mut scene.rig, None);
    let before = scene.rig.content.package_data();
    let mut broken = the_pack("[0.9, 0.9, 0.95]", 90);
    broken[0].blocks[0].text = r#"{ "colour": [0, 0, 0] }"#.to_owned();
    let error = reload(&mut scene, &broken).unwrap_err();
    assert!(
        matches!(&error, ReloadError::Packs(errors) if errors.to_string().contains("my_pack/blocks/marble.json")),
        "{error}"
    );
    assert_eq!(scene.rig.content.package_data(), before);
    assert_eq!(scene.package.version(), 0);
    scene.rig.settle(&mut [&mut player]);
    assert_eq!(player.package(), &before);
}

/// Writes the pack of `the_pack` into `directory`, as the files a person edits.
fn write_pack(directory: &std::path::Path, marble: &str) {
    for pack in the_pack(marble, 90) {
        let folder = directory.join(&pack.folder);
        std::fs::create_dir_all(folder.join("blocks")).unwrap();
        std::fs::create_dir_all(folder.join("patches")).unwrap();
        std::fs::write(folder.join("pack.json"), &pack.manifest).unwrap();
        for block in &pack.blocks {
            std::fs::write(
                folder.join("blocks").join(format!("{}.json", block.name)),
                &block.text,
            )
            .unwrap();
        }
        for patch in &pack.patches {
            std::fs::write(
                folder.join("patches").join(format!("{}.json", patch.name)),
                &patch.text,
            )
            .unwrap();
        }
    }
}

#[test]
fn the_command_reload_makes_a_running_server_read_its_packs_folder_again_and_a_player_has_what_is_there_and_what_was_edited()
 {
    use std::time::{Duration, Instant};

    use game_shell::host::{HostSettings, ServerThread};
    use voxunami_cave::content::CaveContent;

    let directory = std::env::temp_dir().join(format!(
        "voxunami_cave_reload_folder_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&directory);
    write_pack(&directory, "[0.9, 0.9, 0.95]");

    // The server starts with no pack at all: the folder is made after, as a person who edits it while the game runs does.
    let content = CaveContent::new();
    let world = content.new_overworld_world(common::SEED);
    let mut settings = HostSettings::local(common::VIEW_RADIUS);
    settings.packs = Some(directory.clone());
    let host = ServerThread::<Cave>::start(content, world, settings).unwrap();
    let connection = host.connect().unwrap();
    let mut player = Session::<_, Cave>::connect(
        Box::new(connection) as Box<dyn Connection>,
        &Identity::from_secret([61; 32]),
        Mode::Playing,
        common::VIEW_RADIUS,
        camera_at([16.5, 110.0, 16.5], 0.0, 0.0),
        || std::thread::sleep(Duration::from_millis(1)),
    )
    .unwrap();

    // Waits for the player's picture of marble (its colour, or none if the block is not known) to be `wanted`, a command at a time.
    let marble_colour = |player: &common::Player| {
        let table = player.block_table();
        player.content().registries()[0]
            .1
            .raw_id_of(&"my_pack:marble".parse().unwrap())
            .map(|number| table.color(number))
    };
    let wait_for = |player: &mut common::Player, wanted: Option<[f32; 3]>| {
        let deadline = Instant::now() + Duration::from_secs(30);
        while marble_colour(player) != wanted {
            player.advance().unwrap();
            assert!(
                Instant::now() < deadline,
                "gave up waiting for {wanted:?}, the player has {:?}",
                marble_colour(player)
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    assert_eq!(marble_colour(&player), None);
    player.send_command("/reload");
    wait_for(&mut player, Some(MARBLE));

    // Edit the file, ask again.
    write_pack(&directory, "[0.1, 0.2, 0.3]");
    player.send_command("/reload");
    wait_for(&mut player, Some([0.1, 0.2, 0.3]));

    player.say_goodbye();
    drop(player);
    let (_world, content) = host.stop().unwrap();
    assert!(
        content
            .package_data()
            .blocks
            .iter()
            .any(
                |(identifier, definition)| identifier.as_str() == "my_pack:marble"
                    && definition.is_some()
            ),
        "the content the server hands back has what was read"
    );
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_recording_of_a_session_across_a_reload_shows_the_package_and_the_block_as_it_was_live() {
    let directory =
        std::env::temp_dir().join(format!("voxunami_cave_reload_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("session.voxreplay");
    let mut scene = Scene::new();
    let recorded: Box<dyn Connection> =
        record_connection(scene.rig.connector.connect().unwrap(), &path).unwrap();
    let (mut player, at) = above_the_ground(&mut scene.rig, Some(recorded));
    reload(&mut scene, &the_pack("[0.9, 0.9, 0.95]", 90)).unwrap();
    scene.rig.settle(&mut [&mut player]);
    request_block(&mut player, at, "my_pack:marble");
    scene.rig.settle(&mut [&mut player]);
    // A second reload that changes the colour, so that the recording has two replacements and the second changes what the first made.
    reload(&mut scene, &the_pack("[0.1, 0.2, 0.3]", 90)).unwrap();
    scene.rig.settle(&mut [&mut player]);
    let live_package = player.package().clone();
    drop(player);

    let connection: Box<dyn Connection> = Box::new(open_recording(&path).unwrap());
    let mut replay = Session::<_, Cave>::connect(
        connection,
        &Identity::generate(),
        Mode::Replaying,
        common::VIEW_RADIUS,
        camera_at([0.0; 3], 0.0, 0.0),
        || {},
    )
    .unwrap();
    while replay.advance_one_tick().is_ok() {}
    replay.settle(|| {}).unwrap();
    let marble = raw_in(&scene.rig.content, "my_pack:marble");
    assert_eq!(replay.package(), &live_package);
    assert_eq!(replay.block_table().color(marble), [0.1, 0.2, 0.3]);
    assert_eq!(replay.mesher().material_at(at), Some(marble));
    std::fs::remove_dir_all(&directory).unwrap();
}
