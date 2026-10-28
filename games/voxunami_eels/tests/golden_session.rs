//! A whole scripted session of Voxunami Eels, locked bit for bit: the stream of frames the first
//! player receives (a replay file) and the saved world, hashed and pinned. Two players swim, steer and
//! die over several rounds.
//!
//! The script uses fixed tick counts. A change to how the game's rules are put together must leave the
//! numbers as they are; if one must change on purpose, say why in the commit, and take it on Windows
//! and on Linux, where it must agree to the bit.

use game_shell::GameContent;
use game_shell::Identity;
use game_shell::files::record_connection;
use game_shell::session::{Mode, Session};
use game_shell::view::camera_at;
use transport::Connection;
use voxunami_eels::content::ContentEels;
use voxunami_eels::eel::Steer;
use voxunami_eels::game::{Eels, steer};
use voxunami_eels::round::{COUNTDOWN_TICKS, OVER_TICKS, STEP_TICKS};

const VIEW_RADIUS: i128 = 3;

type Player = Session<Box<dyn Connection>, Eels>;

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

fn ticks(server: &mut server::Server, players: &mut [&mut Player], count: u32) {
    for _ in 0..count {
        server.step();
        for player in players.iter_mut() {
            player.advance().unwrap();
            // The players watch what is around where they are, as the window does every frame (and so are told
            // of the players there).
            let _ = player.update_view();
        }
    }
}

fn session(name: &str, arrangement: usize) -> (u64, usize, u64) {
    let directory = std::env::temp_dir().join(format!(
        "voxunami_eels_golden_{}_{name}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("session.voxreplay");

    let content = ContentEels::new();
    let world = content.new_world(1);
    let (mut server, connector) = game_shell::host::build_server_with_plugins::<Eels>(
        &content,
        world,
        VIEW_RADIUS,
        arranged(
            <Eels as game_shell::GameDefinition>::plugins(&content),
            arrangement,
        ),
    )
    .unwrap();
    let start = camera_at([32.5, 40.0, 32.5], 0.0, 0.0);
    let recorded = record_connection(connector.connect().unwrap(), &path).unwrap();
    let mut first = Session::<_, Eels>::connect(
        recorded,
        &Identity::from_secret([1; 32]),
        Mode::Playing,
        VIEW_RADIUS,
        start,
        || {
            server.step();
        },
    )
    .unwrap();
    let second_connection: Box<dyn Connection> = Box::new(connector.connect().unwrap());
    let mut second = Session::<_, Eels>::connect(
        second_connection,
        &Identity::from_secret([2; 32]),
        Mode::Playing,
        VIEW_RADIUS,
        start,
        || {
            server.step();
        },
    )
    .unwrap();

    // The countdown, then both swim; each steers about and rolls; one runs into a wall; a new round.
    ticks(
        &mut server,
        &mut [&mut first, &mut second],
        COUNTDOWN_TICKS + 3,
    );
    for (index, turn) in [
        Steer::Left,
        Steer::Roll,
        Steer::Up,
        Steer::Right,
        Steer::Down,
        Steer::Left,
    ]
    .into_iter()
    .enumerate()
    {
        ticks(&mut server, &mut [&mut first, &mut second], 4 * STEP_TICKS);
        steer(&mut first, turn);
        if index % 2 == 0 {
            steer(&mut second, Steer::Right);
        }
    }
    // Swim on until the walls and bodies have decided the round, and into the next.
    ticks(&mut server, &mut [&mut first, &mut second], 90 * STEP_TICKS);
    ticks(
        &mut server,
        &mut [&mut first, &mut second],
        OVER_TICKS + COUNTDOWN_TICKS + 10,
    );
    steer(&mut second, Steer::Up);
    ticks(&mut server, &mut [&mut first, &mut second], 30 * STEP_TICKS);
    drop(second);
    ticks(&mut server, &mut [&mut first], 20);
    drop(first); // closes the recording

    let stream = std::fs::read(&path).unwrap();
    let registries = content.registries();
    let saved = server.save(&registries).unwrap().world;
    let _ = std::fs::remove_dir_all(&directory);
    (hash(&stream), stream.len(), hash(&saved))
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

/// Stream hash, stream length, saved-world hash.
/// Phase 27c: the stream hash changed once, for the protocol version in the welcome (3, was 2); the length and the
/// saved world did not, and setting the version back to 2 gave the old hash.
/// Phase 27e: the stream hash changed once, and the length by 38 bytes (the challenge frame: kind, 32 bytes, framing) for the
/// protocol version (4, was 3) and the challenge, and because a join takes a tick longer, so the game has run a tick more
/// when the player arrives. Checked, not assumed: the old code with an idle server tick before each join, and this stream
/// with the challenge taken out and the welcome's version set back to 3, give the same hash (and the saved world is the same).
/// Phase 27f: players are entities. The stream no longer has the `Players` message (every player's pose, told to everyone every
/// tick whatever they watched); it has the entity messages for the players in the region the client watches (so the scripts
/// now ask for their view each tick, as a window does, which Cave's already did), and the saved world holds its players.
/// Checked by what the stream holds, not by a control run: two players spawned, one despawned when the script has them leave, 265 changes of pose or look.
/// Phase 27g: the player kind has an inventory component, nine empty slots told to clients: 28 bytes (a 17 byte
/// identifier and 11 bytes of value) in each spawn of a player, and a different saved world (the players in it hold one). Two spawns here: 2 x 28 = 56 bytes: 70,676 to 70,732.
/// Phase 28: the welcome carries the package data (how every block looks, the view channels), and the protocol version is 5. The
/// package is 648 bytes and its length takes 2 more: 70,732 to 71,382, the saved world is the same. The hash changes with the bytes.
/// Docking, part 1 (2026-10-22): the engine declares three more block properties (`engine:marked_face`, `engine:mark_color`, `engine:docking_face`), and the welcome carries them in
/// the package's schema: 432 bytes, measured by encoding the three declarations alone (not assumed), 71,382 to 71,814. The saved world is the same, and no message changed, so the protocol
/// version stays 6.
/// Orientation, one channel for every block (2026-10-22): the engine declares three more block properties (`engine:mark_both_ends`, `engine:front_face`, `engine:axis_face`) and the
/// package data says whether the game has an orientation channel (the eels has none: one byte). The welcome grows by 467 bytes, 71,814 to 72,281: 466 for the declarations,
/// measured by encoding them alone, and 1 for that byte. The saved world is the same.
/// Orientations are `f64` (Phase 38a, 2026-10-23; protocol 7, the sections that hold poses at version 2): a pose is 16 bytes longer. Measured by recording the stream of the script with the code before and after and comparing the frames one by one: the same number of frames, every frame the same except those that carry a pose, and the welcome (version 7, same length):
/// 1,274 frames, 1,007 identical; 531 poses in 266 of the other 267 frames (one spawn and 265 changes; the 267th is the welcome), 531 x 16 = 8,496, and 3 bytes for frames whose length now takes a byte more in the recording:
/// 72,281 to 80,780. The saved world changed (it holds the players' poses).
/// Planets as ships (Phase 39c, 2026-10-24; protocol 8): the welcome says version 8 (same length) and a ship that is told of has a byte more for its terrain. The stream hash moved once, for the version: the code of the commit before with only the version raised to 8 gives this hash, and the length is the same, so none of the frames of this script tells of a ship. The saved world is the same.
/// Coordinates of up to 128 bits (2026-10-27; protocol 9, the plain sections at version 2, the sections that hold poses and the generators at version 3): a coordinate is a length byte and
/// the bytes of its zigzag value. Measured as the 2048 script was: the stream recorded with the code of the commit before and with this, and every message decoded and compared: 1,274 in both,
/// 1,273 the same value for value, the welcome different (version 9, and the world description, whose sections are the new versions). The bytes 80,780 to 82,541, 1,761 more, are the same
/// values written longer. The saved world is different (the same sections, at the new versions).
/// The world's clock in the welcome (2026-10-28; protocol 10): the welcome carries a `u64` more, the time of the world at its tick, and says version 10. Predicted before the run and then seen
/// in all four pinned streams: 8 bytes more (82,541 to 82,549) and the saved world the same to the hash (nothing is saved that was not). The stream hash moved for those 8 bytes and the version.
const PINNED: (u64, usize, u64) = (
    9_878_055_937_370_023_847,
    82_549,
    12_252_857_276_061_378_912,
);
