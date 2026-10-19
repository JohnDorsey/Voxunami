//! The multiscale light (the spike), through the server and sessions: switching to it, what it lights,
//! what players see, switching back, and saving the choice.

mod common;

use common::{Player, PlayerExt, Rig, above_ground};
use game_shell::GameContent;
use game_shell::files::{open_world, save_world};
use voxel_space::VoxelCoordinate;
use voxunami_cave::content::CaveContent;
use voxunami_cave::game::{light_scheme, place_block, request_block};
use voxunami_cave::multiscale_light::{LightScheme, anchor_of, level_of_word, word_homes};

/// The scales of the default scheme, 16 levels with cells up to 8.
const SCALES: usize = 4;

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

/// The level of the cell of 2 that holds a lamp placed at `spot`, on the ground: 15, as the cell has
/// air in it as well as the lamp and the ground (a cell with nothing open in it would hold none).
fn lamp_cell_level(_spot: VoxelCoordinate) -> u32 {
    15
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
        .get(
            rig.content.scale_channel(scale),
            anchor_of(1 << scale, voxel),
        )
        .pipe_level()
}

fn seen_level(rig: &Rig, player: &Player, scale: usize, voxel: VoxelCoordinate) -> u32 {
    player
        .client()
        .world()
        .world()
        .main()
        .overrides
        .get(
            rig.content.scale_channel(scale),
            anchor_of(1 << scale, voxel),
        )
        .pipe_level()
}

/// What a cell holds, as a level.
trait Word {
    fn pipe_level(self) -> u32;
}

impl Word for u32 {
    fn pipe_level(self) -> u32 {
        u32::from(level_of_word(self))
    }
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

const USAGE: &str = "usage: /light, /light classic, /light multiscale [32|16|8|4], /light multiscale LEVELS WIDTH... (widths 1 to 32, growing)";

#[test]
fn the_light_command_says_which_light_there_is_and_changes_it() {
    let mut rig = Rig::new();
    let mut player = player_above_ground(&mut rig);
    let mut say = |command: &str| reply_to(&mut rig, &mut player, command);
    assert_eq!(say("/light"), ["the light is classic"]);
    assert_eq!(say("/light classic"), ["the light is already classic"]);
    assert_eq!(
        say("/light multiscale"),
        ["the light is now multiscale (16 levels, cells 1, 2, 4, 8)"]
    );
    assert_eq!(
        say("/light"),
        ["the light is multiscale (16 levels, cells 1, 2, 4, 8)"]
    );
    assert_eq!(
        say("/light multiscale 16"),
        ["the light is already multiscale (16 levels, cells 1, 2, 4, 8)"]
    );
    // The other schemes, by their number of levels.
    for (levels, name) in [
        (32, "32 levels, cells 1, 2, 4"),
        (8, "8 levels, cells 1, 2, 4, 8, 16"),
        (4, "4 levels, cells 1, 2, 4, 8, 16, 32"),
    ] {
        assert_eq!(
            say(&format!("/light multiscale {levels}")),
            [format!("the light is now multiscale ({name})")]
        );
        assert_eq!(say("/light"), [format!("the light is multiscale ({name})")]);
    }
    assert_eq!(say("/light classic"), ["the light is now classic"]);
    for bad in [
        "/light sideways",
        "/light multiscale classic",
        "/light multiscale 5",
        "/light multiscale 16 8 4",
        "/light classic 8",
    ] {
        assert_eq!(say(bad), [USAGE], "{bad}");
    }
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
    // Scale 1: 15 in the cell that holds the lamp (unless that cell is half ground), 14 in the cell
    // above it, one less for each cell further.
    assert_eq!(server_level(&rig, 1, spot), lamp_cell_level(spot));
    assert_eq!(server_level(&rig, 1, cell_above_lamp(spot)), 14);
    assert_eq!(server_level(&rig, 1, above(cell_above_lamp(spot), 6)), 11);
    // Scales 2 and 3: 15 in the cell with the lamp, or none if more than half of the cell is ground,
    // and next to it, over it, one less. The cell over a cell of 4 or of 8 is 14, or 13 if the lamp's
    // own cell is blocked and the cell over it is half ground too (a hill).
    for (scale, width) in [(2, 4), (3, 8)] {
        assert!(
            [0, 15].contains(&server_level(&rig, scale, spot)),
            "scale {scale}"
        );
        let over = server_level(&rig, scale, above(spot, width));
        assert!((13..=15).contains(&over), "scale {scale}: {over}");
    }
    // And the classic light is not lit at all.
    assert_eq!(server_classic_light(&rig, above(spot, 1)), 0);
}

#[test]
fn what_is_drawn_is_the_scales_blended_so_it_is_not_in_steps_of_seventeen() {
    let mut rig = Rig::new();
    let mut builder = player_above_ground(&mut rig);
    reply_to(&mut rig, &mut builder, "/light multiscale");
    place_block(&mut builder, "base_blocks:lamp");
    ticks(&mut rig, 40);
    let lights = quad_lights(&mut rig, &mut builder);
    // The scales are blended, so faces are not whole numbers of levels of 17; faces by the lamp are as
    // bright as can be, and faces far from it are dimmer.
    assert!(lights.iter().any(|&light| light % 17 != 0));
    assert!(lights.iter().any(|&light| light >= 250));
    assert!(lights.iter().any(|&light| light < 200));
}

#[test]
fn cells_of_any_width_hold_their_light_in_every_brick_they_are_in_and_the_player_draws_from_it() {
    let mut rig = Rig::new();
    let mut builder = player_above_ground(&mut rig);
    assert_eq!(
        reply_to(&mut rig, &mut builder, "/light multiscale 8 1 5 17"),
        ["the light is now multiscale (8 levels, cells 1, 5, 17)"]
    );
    let scheme = LightScheme::new(8, &[1, 5, 17]).unwrap();
    assert_eq!(
        light_scheme(&builder),
        Some(scheme),
        "the player is told how the light is cut"
    );
    // A lamp in the cell of 17 that goes from x = 17 to 33, across the edge of the first brick.
    let spot = builder.target().unwrap().previous;
    let lamp = VoxelCoordinate::new(20, spot.y(), spot.z());
    request_block(&mut builder, lamp, "base_blocks:lamp");
    ticks(&mut rig, 60);

    let word_at = |channel, home| {
        rig.server
            .world()
            .main()
            .overrides
            .get(rig.content.scale_channel(channel), home)
    };
    let cells = [(1, 5), (2, 17)].map(|(scale, width)| {
        let anchor = anchor_of(width, lamp);
        (scale, word_homes(width, anchor), word_at(scale, anchor))
    });
    for (scale, homes, word) in &cells {
        assert_eq!(level_of_word(*word), 7, "scale {scale}: the lamp's cell");
        for &home in homes {
            assert_eq!(word_at(*scale, home), *word, "scale {scale} home {home:?}");
        }
    }
    assert!(
        cells[1].1.len() >= 2,
        "the cell is in two bricks: {:?}",
        cells[1].1
    );

    // The player's view is drawn from them: blended, not in steps of 36 (255 over 7 levels).
    let lights = quad_lights(&mut rig, &mut builder);
    assert!(lights.iter().any(|&light| light >= 250), "by the lamp");
    assert!(lights.iter().any(|&light| light < 200), "far from it");
    assert!(lights.iter().any(|&light| light % 17 != 0));
    // And the player has the word in each brick, so that each can be drawn.
    for (scale, homes, word) in &cells {
        for &home in homes {
            let seen = builder
                .client()
                .world()
                .world()
                .main()
                .overrides
                .get(rig.content.scale_channel(*scale), home);
            assert_eq!(seen, *word, "the player has scale {scale} at {home:?}");
        }
    }
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
    // Coarse light that loses its source dies out a level a tick and has a lot of cells to do it in.
    ticks(&mut rig, 1500);
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
        ["the light is multiscale (16 levels, cells 1, 2, 4, 8)"]
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
fn a_world_saved_before_the_scales_existed_loads_and_its_lamps_light_up_in_multiscale() {
    // The old layout had the block, state and light channels and none for the scales, and no saved
    // choice of light.
    let content = CaveContent::new();
    let old_layout = std::sync::Arc::new(
        voxel_grid::GridLayout::new(vec![
            content.blocks.block_channel(),
            content.blocks.state_channel(),
            content.blocks.light_channel(),
        ])
        .unwrap(),
    );
    let generators = content
        .blocks
        .terrain_generators(5, content.block_channel());
    let default = generators
        .runtime_id(&base_blocks::rolling_hills_generator_identifier())
        .unwrap();
    let mut old_world = world_storage::WorldData::new(
        5,
        voxunami_cave::content::overworld_id(),
        world_storage::LevelData::new(
            terrain::WorldTerrain::new(
                generators,
                default,
                voxunami_cave::content::ASSIGNMENT_CUBE_LEVEL,
            )
            .unwrap(),
            voxel_grid::Grid::new(old_layout),
        ),
    );
    // A lamp placed by a player of the old version, high in the air where nothing else is.
    let lamp = VoxelCoordinate::new(40, 150, 40);
    old_world
        .main_mut()
        .overrides
        .set(content.block_channel(), lamp, content.blocks.lamp);
    let bytes = old_world.save(&content.registries()).unwrap();

    let mut later = CaveContent::new();
    let parsed = world_storage::ParsedWorld::parse(&bytes).unwrap();
    let seed = parsed.world_seed();
    let (loaded, report) = parsed.load(later.load_context(seed)).unwrap();
    assert!(report.missing_content.is_empty());
    let mut rig = Rig::with_world(later, loaded);
    let mut player = rig.join([40.5, 152.0, 40.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.take_command_replies();
    assert_eq!(
        reply_to(&mut rig, &mut player, "/light"),
        ["the light is classic"]
    );
    assert_eq!(
        reply_to(&mut rig, &mut player, "/light multiscale"),
        ["the light is now multiscale (16 levels, cells 1, 2, 4, 8)"]
    );
    ticks(&mut rig, 40);
    rig.settle(&mut [&mut player]);
    // The lamp that was already in the world gives light at the scales, and the player sees it.
    assert_eq!(server_level(&rig, 0, above(lamp, 1)), 14);
    assert_eq!(server_level(&rig, 0, above(lamp, 3)), 12);
    assert_eq!(seen_level(&rig, &player, 0, above(lamp, 3)), 12);
    assert_eq!(server_level(&rig, 1, lamp), 15);
    // And one the player places now.
    let new_lamp = above(lamp, 6);
    request_block(&mut player, new_lamp, "base_blocks:lamp");
    ticks(&mut rig, 40);
    rig.settle(&mut [&mut player]);
    assert_eq!(server_level(&rig, 0, above(new_lamp, 1)), 14);
    assert_eq!(seen_level(&rig, &player, 0, above(new_lamp, 1)), 14);
}

#[test]
fn every_scheme_lights_its_scales_with_its_own_greatest_level_and_leaves_the_others_dark() {
    for scheme in LightScheme::ALL {
        let mut rig = Rig::new();
        let mut builder = player_above_ground(&mut rig);
        reply_to(
            &mut rig,
            &mut builder,
            &format!("/light multiscale {}", scheme.levels()),
        );
        let spot = builder.target().unwrap().previous;
        place_block(&mut builder, "base_blocks:lamp");
        ticks(&mut rig, 300);
        rig.settle(&mut [&mut builder]);
        let greatest = u32::from(scheme.maximum_level());
        // The voxel over the lamp holds one less than the greatest, as the cell next to a lamp does.
        assert_eq!(
            server_level_in(&rig, 0, above(spot, 1)),
            greatest - 1,
            "{scheme:?}"
        );
        assert_eq!(
            seen_level_in(&rig, &builder, 0, above(spot, 1)),
            greatest - 1,
            "{scheme:?}, seen"
        );
        // The cell with the lamp at each of the scheme's scales holds the greatest level (or none, if
        // it is a cell of nothing but ground, which a cell with the lamp in it is not), and no scale
        // beyond the scheme's has anything.
        for scale in 0..scheme.scales() {
            let level = server_level_in(&rig, scale, spot);
            assert!(
                [0, greatest].contains(&level) && level != 0,
                "{scheme:?} scale {scale}: {level}"
            );
        }
        for scale in scheme.scales()..6 {
            let anything = rig
                .server
                .world()
                .main()
                .overrides
                .bricks()
                .any(|(_, brick)| {
                    voxel_space::LocalVoxelIndex::all()
                        .any(|local| brick.get(rig.content.scale_channel(scale), local) != 0)
                });
            assert!(!anything, "{scheme:?} scale {scale} holds light");
        }
    }
}

/// What a cell holds at a scale, in a world of any scheme, as a level.
fn server_level_in(rig: &Rig, scale: usize, voxel: VoxelCoordinate) -> u32 {
    server_level(rig, scale, voxel)
}

fn seen_level_in(rig: &Rig, player: &Player, scale: usize, voxel: VoxelCoordinate) -> u32 {
    seen_level(rig, player, scale, voxel)
}

#[test]
fn a_world_saved_with_a_scheme_keeps_it_when_loaded() {
    let directory =
        std::env::temp_dir().join(format!("voxunami_cave_scheme_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("world.voxunami");
    let mut rig = Rig::new();
    let mut builder = player_above_ground(&mut rig);
    reply_to(&mut rig, &mut builder, "/light multiscale 8");
    let spot = builder.target().unwrap().previous;
    place_block(&mut builder, "base_blocks:lamp");
    ticks(&mut rig, 100);
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
        ["the light is multiscale (8 levels, cells 1, 2, 4, 8, 16)"]
    );
    assert_eq!(
        server_level(&rig, 0, above(spot, 1)),
        6,
        "the levels were saved"
    );
    std::fs::remove_dir_all(&directory).unwrap();
}
