//! The multiscale light (the spike), through the server and sessions: switching to it, what it lights,
//! what players see, switching back, and saving the choice.

mod common;

use common::{Player, PlayerExt, Rig, above_ground};
use game_shell::GameContent;
use game_shell::files::{open_world, save_world};
use voxel_space::VoxelCoordinate;
use voxunami_cave::content::CaveContent;
use voxunami_cave::game::{place_block, request_block};
use voxunami_cave::multiscale_light::{SCALES, anchor_of};

fn player_above_ground(rig: &mut Rig) -> Player {
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.camera = above_ground(&player, 16, 16, 3.0);
    player.teleport_to_camera();
    rig.settle(&mut [&mut player]);
    player.take_command_replies();
    player
}

fn ticks(rig: &mut Rig, count: usize) {
    for _ in 0..count {
        rig.step();
    }
}

fn above(voxel: VoxelCoordinate, height: i64) -> VoxelCoordinate {
    VoxelCoordinate::new(voxel.x(), voxel.y() + height, voxel.z())
}

/// The level of the cell of 2 that holds a lamp placed at `spot`, on the ground. A lamp is a source of
/// 64 / 32 = 2 there, but a cell of 2 that holds the ground as well as the lamp (when the lamp is at an
/// odd height, the cell is the ground below it and its own layer) is more than half solid, and is
/// blocked: it holds no light, though it still gives its 2 to the cells next to it.
fn lamp_cell_level(spot: VoxelCoordinate) -> u32 {
    if spot.y().rem_euclid(2) == 1 { 0 } else { 2 }
}

/// A voxel in the cell of 2 above the cell that holds `spot`.
fn cell_above_lamp(spot: VoxelCoordinate) -> VoxelCoordinate {
    above(spot, 2 - spot.y().rem_euclid(2))
}

fn reply_to(rig: &mut Rig, player: &mut Player, command: &str) -> Vec<String> {
    player.send_command(command);
    ticks(rig, 2);
    rig.settle(&mut [player]);
    player.take_command_replies()
}

/// The level the server holds for the cell of a scale that has a voxel.
fn server_level(rig: &Rig, scale: usize, voxel: VoxelCoordinate) -> u32 {
    rig.server
        .world()
        .main()
        .overrides
        .get(rig.content.scale_channel(scale), anchor_of(scale, voxel))
}

fn seen_level(rig: &Rig, player: &Player, scale: usize, voxel: VoxelCoordinate) -> u32 {
    player
        .client()
        .world()
        .world()
        .main()
        .overrides
        .get(rig.content.scale_channel(scale), anchor_of(scale, voxel))
}

fn server_classic_light(rig: &Rig, voxel: VoxelCoordinate) -> u32 {
    rig.server
        .world()
        .main()
        .overrides
        .get(rig.content.light_channel(), voxel)
}

/// The brightness of every quad of the newest mesh of each brick the player has drawn.
fn quad_lights(rig: &mut Rig, player: &mut Player) -> Vec<u8> {
    let server = &mut rig.server;
    let updates = player
        .settle(|| {
            server.step();
        })
        .unwrap();
    let mut newest = std::collections::HashMap::new();
    for update in updates {
        if let world_view::MeshUpdate::Upload { coordinate, mesh } = update {
            newest.insert(coordinate, mesh);
        }
    }
    newest
        .values()
        .flat_map(|mesh| mesh.quads.iter().map(|quad| quad.unpack().light))
        .collect()
}

#[test]
fn the_light_command_says_which_light_there_is_and_changes_it() {
    let mut rig = Rig::new();
    let mut player = player_above_ground(&mut rig);
    assert_eq!(
        reply_to(&mut rig, &mut player, "/light"),
        ["the light is classic"]
    );
    assert_eq!(
        reply_to(&mut rig, &mut player, "/light classic"),
        ["the light is already classic"]
    );
    assert_eq!(
        reply_to(&mut rig, &mut player, "/light multiscale"),
        ["the light is now multiscale"]
    );
    assert_eq!(
        reply_to(&mut rig, &mut player, "/light"),
        ["the light is multiscale"]
    );
    assert_eq!(
        reply_to(&mut rig, &mut player, "/light sideways"),
        ["usage: /light, /light classic, /light multiscale"]
    );
    assert_eq!(
        reply_to(&mut rig, &mut player, "/light multiscale classic"),
        ["usage: /light, /light classic, /light multiscale"]
    );
}

#[test]
fn a_lamp_lights_four_scales_of_cells_and_every_player_sees_the_levels() {
    let mut rig = Rig::new();
    let mut builder = player_above_ground(&mut rig);
    let mut onlooker = rig.join([20.5, 110.0, 20.5], 0.0, 0.0);
    rig.settle(&mut [&mut builder, &mut onlooker]);
    reply_to(&mut rig, &mut builder, "/light multiscale");
    let spot = builder.target().unwrap().previous;

    place_block(&mut builder, "base_blocks:lamp");
    ticks(&mut rig, 40);
    rig.settle(&mut [&mut builder, &mut onlooker]);

    // Scale 0: a lamp of 64 is a source of 15 beside it, and a level less each voxel up.
    for (height, level) in [(1, 14), (2, 13), (8, 7), (14, 1), (15, 0)] {
        assert_eq!(
            server_level(&rig, 0, above(spot, height)),
            level,
            "scale 0, {height} up"
        );
        for player in [&builder, &onlooker] {
            assert_eq!(
                seen_level(&rig, player, 0, above(spot, height)),
                level,
                "seen, {height}"
            );
        }
    }
    // Scale 1: 2 in the cell that holds the lamp (unless that cell is half ground), 1 in the cell
    // above it, none beyond.
    assert_eq!(server_level(&rig, 1, spot), lamp_cell_level(spot));
    assert_eq!(server_level(&rig, 1, cell_above_lamp(spot)), 1);
    assert_eq!(server_level(&rig, 1, above(cell_above_lamp(spot), 2)), 0);
    // Scales 2 and 3: a faint 1 in the one cell, which a cell of 4 or of 8 is likely to share with
    // the ground (more than half of it stone or dirt would make it a blocked cell holding none).
    for scale in 2..SCALES {
        assert!(server_level(&rig, scale, spot) <= 1, "scale {scale}");
        assert_eq!(
            server_level(&rig, scale, above(spot, 16)),
            0,
            "scale {scale}"
        );
    }
    // And the classic light is not lit at all.
    assert_eq!(server_classic_light(&rig, above(spot, 1)), 0);
}

#[test]
fn what_is_drawn_is_the_brightest_of_the_scales_in_steps_of_seventeen() {
    let mut rig = Rig::new();
    let mut builder = player_above_ground(&mut rig);
    reply_to(&mut rig, &mut builder, "/light multiscale");
    place_block(&mut builder, "base_blocks:lamp");
    ticks(&mut rig, 40);
    let lights = quad_lights(&mut rig, &mut builder);
    // Every face is a whole number of levels of 17 (nothing is lit by the classic light), the faces
    // by the lamp are as bright as 14 levels, and far from it none is lit.
    assert!(lights.iter().all(|&light| light % 17 == 0), "{:?}", {
        let mut odd: Vec<_> = lights.iter().filter(|&&l| l % 17 != 0).collect();
        odd.dedup();
        odd
    });
    assert!(lights.iter().any(|&light| light >= 14 * 17));
    assert!(lights.contains(&0));
    assert!(
        lights
            .iter()
            .any(|&light| (3 * 17..10 * 17).contains(&light))
    );
}

#[test]
fn switching_back_clears_the_scales_and_lights_the_classic_light_from_the_same_lamp() {
    let mut rig = Rig::new();
    let mut builder = player_above_ground(&mut rig);
    let spot = builder.target().unwrap().previous;
    place_block(&mut builder, "base_blocks:lamp");
    ticks(&mut rig, 70);
    let emission = u32::from(base_blocks::LAMP_EMISSION);
    assert_eq!(server_classic_light(&rig, above(spot, 1)), emission - 1);

    // To multiscale: the classic light is cleared, and the scales are lit from the lamp that was there.
    reply_to(&mut rig, &mut builder, "/light multiscale");
    ticks(&mut rig, 40);
    assert_eq!(server_classic_light(&rig, above(spot, 1)), 0);
    assert_eq!(server_classic_light(&rig, above(spot, 30)), 0);
    assert_eq!(server_level(&rig, 0, above(spot, 1)), 14);
    assert_eq!(server_level(&rig, 1, spot), lamp_cell_level(spot));

    // And back: every scale is cleared and the classic light spreads from the lamp again.
    reply_to(&mut rig, &mut builder, "/light classic");
    ticks(&mut rig, 70);
    for scale in 0..SCALES {
        assert_eq!(
            server_level(&rig, scale, above(spot, 1)),
            0,
            "scale {scale}"
        );
        assert_eq!(server_level(&rig, scale, spot), 0, "scale {scale}");
    }
    assert_eq!(server_classic_light(&rig, above(spot, 1)), emission - 1);
    assert_eq!(server_classic_light(&rig, above(spot, 30)), emission - 30);
    rig.settle(&mut [&mut builder]);
    assert_eq!(
        builder
            .client()
            .world()
            .world()
            .main()
            .overrides
            .get(rig.content.light_channel(), above(spot, 3)),
        emission - 3,
        "and the player sees it"
    );
}

#[test]
fn taking_the_lamp_away_puts_the_multiscale_light_out() {
    let mut rig = Rig::new();
    let mut builder = player_above_ground(&mut rig);
    reply_to(&mut rig, &mut builder, "/light multiscale");
    let spot = builder.target().unwrap().previous;
    place_block(&mut builder, "base_blocks:lamp");
    ticks(&mut rig, 40);
    assert_eq!(server_level(&rig, 0, above(spot, 1)), 14);
    request_block(&mut builder, spot, "base_blocks:air");
    ticks(&mut rig, 60);
    for scale in 0..SCALES {
        assert_eq!(server_level(&rig, scale, spot), 0, "scale {scale}");
        assert_eq!(
            server_level(&rig, scale, above(spot, 1)),
            0,
            "scale {scale}"
        );
    }
}

#[test]
fn a_world_saved_with_the_multiscale_light_keeps_it_and_its_levels() {
    let directory =
        std::env::temp_dir().join(format!("voxunami_cave_multiscale_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("world.voxunami");

    let mut rig = Rig::new();
    let mut builder = player_above_ground(&mut rig);
    reply_to(&mut rig, &mut builder, "/light multiscale");
    let spot = builder.target().unwrap().previous;
    place_block(&mut builder, "base_blocks:lamp");
    ticks(&mut rig, 40);
    // Saved while the light is still spreading: carries on after loading.
    place_block(&mut builder, "base_blocks:lamp");
    ticks(&mut rig, 2);
    builder.say_goodbye();
    rig.step();
    let Rig {
        content, server, ..
    } = rig;
    let mut world = server.into_world();
    save_world(&content, &mut world, &path).unwrap();

    let mut content = CaveContent::new();
    let loaded = open_world(&mut content, Some(&path), 1).unwrap();
    let mut rig = Rig::with_world(content, loaded);
    let mut later = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut later]);
    later.take_command_replies();
    assert_eq!(
        reply_to(&mut rig, &mut later, "/light"),
        ["the light is multiscale"]
    );
    assert_eq!(
        server_level(&rig, 0, above(spot, 1)),
        14,
        "the levels were saved"
    );
    assert_eq!(server_level(&rig, 1, spot), lamp_cell_level(spot));
    assert_eq!(server_classic_light(&rig, above(spot, 1)), 0);
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_world_made_before_the_scales_existed_loads_with_the_classic_light_and_empty_scales() {
    // The old layout had no channels for the scales and no saved choice.
    let content = CaveContent::new();
    let world = content.new_world(3);
    let bytes = world.save(&content.registries()).unwrap();
    let mut later = CaveContent::new();
    let parsed = world_storage::ParsedWorld::parse(&bytes).unwrap();
    let seed = parsed.world_seed();
    let (loaded, _) = parsed.load(later.load_context(seed)).unwrap();
    let mut rig = Rig::with_world(later, loaded);
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.take_command_replies();
    assert_eq!(
        reply_to(&mut rig, &mut player, "/light"),
        ["the light is classic"]
    );
}
