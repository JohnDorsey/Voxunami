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
use voxunami_cave::game::request_block;
use voxunami_cave::game::request_block_with_state;

/// The height of the highest solid voxel in the column (16, 16) of the world of seed 2024. The
/// terrain generator is frozen, so it does not change; `the_script_assumes_the_ground_it_is_built_on`
/// checks it.
const GROUND: i64 = 102;

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
fn session(name: &str) -> (u64, usize, u64) {
    let directory = std::env::temp_dir().join(format!(
        "voxunami_cave_golden_{}_{name}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path: PathBuf = directory.join("session.voxreplay");

    let mut rig = Rig::new();
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
    let saved = rig.server.save(&registries).unwrap();
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
    let first = session("first");
    let second = session("second");
    assert_eq!(
        first, second,
        "the session differs between two runs in one process"
    );
    assert_eq!(
        first, PINNED,
        "stream hash, stream bytes, saved world hash: {first:?}"
    );
}

/// Stream hash, stream length, saved-world hash.
const PINNED: (u64, usize, u64) = (
    13_671_793_627_762_328_938,
    7_179_705,
    12_857_685_528_707_532_367,
);
