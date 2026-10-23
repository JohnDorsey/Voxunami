//! A whole scripted session of Voxunami Cave, locked bit for bit.
//!
//! The server's side of one player's session is recorded as the stream of frames that player
//! received (a replay file), and the world is saved at the end. Both are hashed and pinned. The
//! session drives every system the game has: block placing and breaking, the classic light and
//! every multiscale scheme, fire, walking, flying, commands and levels. A refactor of how the game's
//! rules are put together (the plugin registrar, Phase 23) must leave both numbers as they are: the
//! same messages in the same order, and the same saved bytes.
//!
//! The script uses fixed tick counts and fixed coordinates, never `Rig::settle` (which steps the
//! server until the client's worker threads are quiet, a number that varies from run to run) and
//! never what the client's mesher has drawn.
//!
//! If a number must change on purpose, say why in the commit, and take it on Windows and on Linux:
//! the two must agree to the bit.

mod common;

use std::path::PathBuf;

use common::{PlayerExt, Rig, above_ground};
use game_shell::GameContent;
use game_shell::KeyCode;
use game_shell::files::record_connection;
use voxel_space::VoxelCoordinate;
use voxunami_cave::content::CaveContent;
use voxunami_cave::game::request_block;
use voxunami_cave::game::request_block_with_state;
use voxunami_cave::plugins::cave_plugins;

/// The height of the highest solid voxel in the column (16, 16) of the world of seed 2024. The
/// terrain generator is frozen, so it does not change; `the_script_assumes_the_ground_it_is_built_on`
/// checks it.
const GROUND: i64 = 102;

/// The same plugins in another order: 0 as listed, 1 reversed, 2 rotated. The order must change nothing.
fn arranged(
    mut plugins: Vec<Box<dyn plugin_host::Plugin>>,
    arrangement: usize,
) -> Vec<Box<dyn plugin_host::Plugin>> {
    match arrangement {
        0 => {}
        1 => plugins.reverse(),
        _ => plugins.rotate_left(1),
    }
    plugins
}

/// FNV-1a over bytes: simple, and the same everywhere.
fn hash(bytes: &[u8]) -> u64 {
    let mut state: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        state ^= u64::from(byte);
        state = state.wrapping_mul(0x0000_0100_0000_01b3);
    }
    state
}

fn ticks(rig: &mut Rig, player: &mut common::Player, count: usize) {
    for _ in 0..count {
        rig.step();
        player.advance().unwrap();
        // The player watches what is around where they are, as the window does every frame.
        let _ = player.update_view();
    }
}

fn at(x: i64, y: i64, z: i64) -> VoxelCoordinate {
    VoxelCoordinate::new(x, y, z)
}

/// Runs the session and returns the hash of the recorded stream, the number of bytes in it, and the
/// hash of the saved world.
fn session(name: &str, arrangement: usize) -> (u64, usize, u64) {
    let directory = std::env::temp_dir().join(format!(
        "voxunami_cave_golden_{}_{name}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path: PathBuf = directory.join("session.voxreplay");

    let content = CaveContent::new();
    let world = content.new_world(common::SEED);
    let plugins = arranged(cave_plugins(&content), arrangement);
    let mut rig = Rig::with_plugins(content, world, plugins);
    let connection = rig.connector.connect().unwrap();
    let recorded = record_connection(connection, &path).unwrap();
    let mut player = rig.join_with(recorded, [16.5, GROUND as f64 + 4.0, 16.5], 0.0, 0.0);
    ticks(&mut rig, &mut player, 5);
    player.send_command(&format!("/tp 16.5 {} 16.5", GROUND as f64 + 4.0));
    ticks(&mut rig, &mut player, 5);

    // A lamp on the ground, and the classic light spreading from it.
    request_block(&mut player, at(16, GROUND + 1, 16), "base_blocks:lamp");
    ticks(&mut rig, &mut player, 70);
    // (So that the number locks something: the lamp is there and its light has spread.)
    let overrides = &rig.server.world().main().overrides;
    assert_eq!(
        overrides.get(rig.content.block_channel(), at(16, GROUND + 1, 16)),
        rig.content.blocks.lamp
    );
    assert_eq!(
        overrides.get(rig.content.light_channel(), at(16, GROUND + 2, 16)),
        u32::from(base_blocks::LAMP_EMISSION) - 1
    );

    // Fires far from the lamp (its light would hide theirs near it), which spread by chance and go out.
    let far = 44;
    player.send_command(&format!("/tp {}.5 {} 16.5", 16 + far, GROUND + 4));
    ticks(&mut rig, &mut player, 3);
    for (dx, dz) in [(0, 0), (2, 0), (-2, 0), (0, 2), (0, -2)] {
        request_block(
            &mut player,
            at(16 + far + dx, GROUND + 1, 16 + dz),
            "base_blocks:fire",
        );
    }
    ticks(&mut rig, &mut player, 3);
    // (So that the number locks the fire's light: a flame gives light where the lamp's does not reach.)
    let overrides = &rig.server.world().main().overrides;
    assert_eq!(
        overrides.get(rig.content.block_channel(), at(16 + far, GROUND + 1, 16)),
        rig.content.blocks.fire
    );
    assert!(
        overrides.get(rig.content.light_channel(), at(16 + far, GROUND + 1, 16))
            >= u32::from(base_blocks::FIRE_EMISSION) - 2
    );
    ticks(&mut rig, &mut player, 250);
    player.send_command(&format!("/tp 16.5 {} 16.5", GROUND + 4));
    ticks(&mut rig, &mut player, 3);

    // Shaped blocks, and breaking one.
    request_block(&mut player, at(18, GROUND + 3, 16), "base_blocks:stone");
    request_block_with_state(&mut player, at(19, GROUND + 3, 16), "base_blocks:wood", 1);
    request_block_with_state(&mut player, at(20, GROUND + 3, 16), "base_blocks:stone", 3);
    ticks(&mut rig, &mut player, 5);
    request_block(&mut player, at(18, GROUND + 3, 16), "base_blocks:air");
    ticks(&mut rig, &mut player, 5);

    // The other lights: every scheme the game has, and one with odd widths, then back to classic.
    for command in [
        "/light multiscale",
        "/light multiscale 32",
        "/light multiscale 8 1 3 7 17",
        "/light classic",
    ] {
        player.send_command(command);
        ticks(&mut rig, &mut player, 50);
    }

    // Commands the game answers, and one it does not.
    for command in ["/light", "/level", "/level list", "/nonsense"] {
        player.send_command(command);
        ticks(&mut rig, &mut player, 2);
    }

    // Items: two stones built in creative, then in survival broken from out of reach of picking up; the one
    // above falls onto the other's place and they join, and a player who walks up to them has both.
    request_block(&mut player, at(22, GROUND + 1, 16), "base_blocks:stone");
    request_block(&mut player, at(22, GROUND + 2, 16), "base_blocks:stone");
    ticks(&mut rig, &mut player, 5);
    player.send_command("/gamemode survival");
    ticks(&mut rig, &mut player, 3);
    request_block(&mut player, at(22, GROUND + 2, 16), "base_blocks:air");
    ticks(&mut rig, &mut player, 3);
    request_block(&mut player, at(22, GROUND + 1, 16), "base_blocks:air");
    ticks(&mut rig, &mut player, 20);
    // (So that the number locks something: both are one stack on the ground, and nothing is held yet.)
    let kind = voxunami_cave::items::dropped_item_kind();
    let lying: Vec<_> = rig
        .server
        .world()
        .entities()
        .iter()
        .filter(|entity| entity.kind == kind)
        .map(|entity| {
            entity
                .components
                .get::<entities::ItemStack>()
                .unwrap()
                .unwrap()
        })
        .collect();
    assert_eq!(
        lying,
        [entities::ItemStack::new(
            "base_blocks:stone".parse().unwrap(),
            2
        )]
    );
    player.send_command(&format!("/tp 22.5 {} 16.5", GROUND + 3));
    ticks(&mut rig, &mut player, 8);
    let held = rig
        .server
        .world()
        .entities()
        .iter()
        .find(|entity| entity.kind == player_presence::player_kind())
        .and_then(|entity| entity.components.get::<entities::Inventory>().unwrap())
        .expect("the player has an inventory");
    assert_eq!(held.count_of(&"base_blocks:stone".parse().unwrap()), 2);
    player.send_command(&format!("/tp 16.5 {} 16.5", GROUND + 4));
    ticks(&mut rig, &mut player, 3);

    // Walking: falling, walking about and jumping, then flying again and teleporting.
    player.send_command("/fly");
    ticks(&mut rig, &mut player, 100);
    for (keys, yaw, count) in [
        (&[KeyCode::KeyW][..], 0.4, 60),
        (&[KeyCode::KeyW, KeyCode::Space][..], 0.4, 30),
        (&[KeyCode::KeyW, KeyCode::ControlLeft][..], -1.0, 60),
    ] {
        player.camera.yaw = yaw;
        player.hold(keys);
        ticks(&mut rig, &mut player, count);
    }
    player.hold(&[]);
    player.send_command("/fly");
    player.send_command("/tp 20 140 20");
    ticks(&mut rig, &mut player, 20);
    drop(player); // closes the recording

    let stream = std::fs::read(&path).unwrap();
    let registries = rig.content.registries();
    let saved = rig.server.save(&registries).unwrap().world;
    let _ = std::fs::remove_dir_all(&directory);
    (hash(&stream), stream.len(), hash(&saved))
}

#[test]
fn the_script_assumes_the_ground_it_is_built_on() {
    let mut rig = Rig::new();
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    let camera = above_ground(&player, 16, 16, 3.0);
    // The camera is 3 above the voxel over the ground.
    assert_eq!(common::voxels_of(&camera)[1].floor() as i64 - 4, GROUND);
}

#[test]
fn a_whole_session_gives_the_same_stream_and_the_same_saved_world_every_time_and_on_every_machine()
{
    let first = session("first", 0);
    let second = session("second", 0);
    assert_eq!(
        first, second,
        "the session differs between two runs in one process"
    );
    assert_eq!(
        first, PINNED,
        "stream hash, stream bytes, saved world hash: {first:?}"
    );
}

#[test]
fn the_order_the_plugins_are_listed_in_changes_nothing() {
    for arrangement in [1, 2] {
        assert_eq!(
            session("arranged", arrangement),
            PINNED,
            "arrangement {arrangement}"
        );
    }
}

/// Stream hash, stream length, saved-world hash. The stream changed once in Phase 25c, and only in
/// the reply to the unknown command `/nonsense`, which now lists `/help` and the commands' arguments
/// (49 bytes longer; checked by restoring the old text, which gave the old hash); the saved world did not.
/// Phase 27c: the stream hash changed once, for the protocol version in the welcome (3, was 2); the length and the
/// saved world did not, and setting the version back to 2 gave the old hash.
/// Phase 27e: the stream hash changed once, and the length by 38 bytes (the challenge frame: kind, 32 bytes, framing) for the
/// protocol version (4, was 3) and the challenge, and because a join takes a tick longer, so the game has run a tick more
/// when the player arrives. Checked, not assumed: the old code with an idle server tick before each join, and this stream
/// with the challenge taken out and the welcome's version set back to 3, give the same hash and the same saved world (which did change, because the world is a tick older when the player is in it).
/// Phase 27f: players are entities. The stream no longer has the `Players` message (every player's pose, told to everyone every
/// tick whatever they watched); it has the entity messages for the players in the region the client watches (so the scripts
/// now ask for their view each tick, as a window does, which Cave's already did), and the saved world holds its players.
/// Checked by what the stream holds, not by a control run: one player spawned once and 120 changes as it moves.
/// Phase 27g: the player kind has an inventory component, nine empty slots told to clients: 28 bytes (a 17 byte
/// identifier and 11 bytes of value) in each spawn of a player, and a different saved world (the players in it hold one). Besides that, the world
/// description names the new entity kind (27 bytes) and the player is told the game mode when it joins (a 30 byte frame):
/// +85 bytes, 7,177,754 to 7,177,839 (checked before the script below was added). The script then grew an items
/// segment: two stones built, broken in survival, falling, joining into a stack of 2, and collected (asserted), and
/// the world is left in survival, so it saves its game mode: 7,177,839 to 7,181,819.
/// Phase 28: the welcome carries the package data (how every block looks, the view channels), and the protocol version is 5. The
/// package is 1,345 bytes and its length takes 2 more: 7,181,819 to 7,183,166, the saved world is the same. The hash changes with the bytes.
/// Phase 29: Cave declares the property `voxunami_cave:placeable` and sets it on the six blocks a player can place, so its package grows
/// from 1,345 to 1,647 bytes (+302): 7,183,166 to 7,183,468; the saved world is the same.
/// Phase 32h (a walker slides along terrain walls): the walking stage had the player stand still against a hillside wall for about a
/// hundred steps (the controller lost the slide, see `walking.rs`); now it slides on, so the stream holds the moves it did not have:
/// the stream hash changed and the length by 1,478 bytes (7,183,468 to 7,184,946); the saved world did not (the session ends with a
/// teleport, which is probably why). Checked by tracing the old code: in this session the first move of a step came out as under a
/// tenth of what was asked at 104 steps, all at one place, to the bit, which is the walker stuck on the hillside wall.
/// Phase 32h: the command `/ship` is in the reply to `/nonsense`, which lists the commands: 29 bytes, 7,183,468 to 7,183,497; the saved
/// world is the same. Checked by a control run: with the plugin of the command left out of the list the old values came back.
/// Both together (the slide, then the command): 7,183,468 + 1,478 + 29 = 7,184,975 bytes. Taken on Windows and Linux; the two agree.
/// Phase 32g, after: `/tp` tells the player where they are (`PlayerPlaced`, so that the client watches around the new place even when it is
/// far from the camera). The script teleports six times and each message is 45 bytes: +270, 7,183,468 to 7,183,738; the saved world is the same.
/// All three together (the slide, the command, the teleports): 7,183,468 + 1,478 + 29 + 270 = 7,185,245 bytes.
/// Docking, part 1 (2026-10-22): the engine declares three more block properties (`engine:marked_face`, `engine:mark_color`, `engine:docking_face`), and the welcome carries them in
/// the package's schema: 432 bytes, measured by encoding the three declarations alone (not assumed), 7,185,245 to 7,185,677. The saved world is the same, and no message changed, so the protocol
/// version stays 6.
/// Docking, part 2 (2026-10-22): Cave has a block more, the creative-only dev docking port. The welcome grows by 395 bytes, 7,185,677 to 7,186,072: the package by 364 (the block's
/// definition 207, its entry 32 and the declaration of the property `voxunami_cave:creative_only` 125, measured by encoding each alone) and, read and not measured separately, 31
/// for its item (the 30 characters of its identifier and a length). The saved world changes too, as a world file lists the blocks of its game.
/// Every block in every orientation (2026-10-22): the solid blocks and the lamp have a state for each way they stand (stone, dirt, grass and wood have 19: whole, six
/// slabs, twelve stairs), so their definitions carry more shapes. The welcome grows by 528 bytes, 7,186,072 to 7,186,600, all of it the package, which went from 2,443 to
/// 2,971 bytes (measured by encoding it). The saved world is the same: the numbers of the states it holds are the same.
/// Gravity of 5 metres a second a second and a jump of 3.6 (2026-10-22; was 32 and 9, a jump of the same height). The welcome and the package are the same bytes, and the saved
/// world is the same (every player ends where they stood); the stream is 210 bytes longer, 7,186,600 to 7,186,810, and its hash is not the same: the players fall and jump more
/// slowly, so the stream holds more ticks of their motion. (Read from what changed, and not measured separately.)
/// Orientation, one channel for every block (2026-10-22): the game has a channel more, `base_blocks:block_orientation`, and the engine declares three more block properties
/// (`engine:mark_both_ends`, `engine:front_face`, `engine:axis_face`). The stream is 619 bytes longer, 7,186,810 to 7,187,429, with the same 6,934 frames, and the saved world
/// is not the same (its bricks have a channel more). Measured, by recording the stream of this script with the code before and after and comparing them frame by frame: the welcome
/// is 74 bytes longer: the package by 39 (measured by encoding it, 2,971 to 3,010 bytes; +466 for the three declarations, measured by encoding them alone, and so -427 for the
/// shorter list of shapes of the solid blocks, which now have seven states and not nineteen, read and not measured separately) and 35 for the channel in the description of the world; 109 frames that carry a whole brick are 5 bytes longer each (the new channel of
/// the brick: 545), and 5,747 frames of changes to a brick have the same length and another digest of the brick (the digest covers the channels, and there is one more). Nothing else
/// differs: 74 + 545 = 619.
/// Orientations are `f64` (Phase 38a, 2026-10-23; protocol 7, the sections that hold poses at version 2): a pose is 16 bytes longer. Measured by recording the stream of the script with the code before and after and comparing the frames one by one: the same number of frames, every frame the same except those that carry a pose, and the welcome (version 7, same length):
/// 6,934 frames, 6,765 identical; 169 poses in 168 of the other 169 frames (3 spawns and 165 changes, one frame carrying two; the 169th is the welcome), 169 x 16 = 2,704: 7,187,429 to 7,190,133. The saved world changed (it holds the players' poses).
const PINNED: (u64, usize, u64) = (
    15_876_146_309_874_477_616,
    7_190_133,
    5_952_411_052_594_281_229,
);
