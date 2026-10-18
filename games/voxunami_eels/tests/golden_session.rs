//! A whole scripted session of Voxunami Eels, locked bit for bit: the stream of frames the first
//! player receives (a replay file) and the saved world, hashed and pinned. Two players swim, steer and
//! die over several rounds.
//!
//! The script uses fixed tick counts. A change to how the game's rules are put together must leave the
//! numbers as they are; if one must change on purpose, say why in the commit, and take it on Windows
//! and on Linux, where it must agree to the bit.

use game_shell::GameContent;
use game_shell::files::record_connection;
use game_shell::session::{Mode, Session};
use game_shell::view::camera_at;
use transport::Connection;
use voxunami_eels::content::ContentEels;
use voxunami_eels::eel::Steer;
use voxunami_eels::game::{Eels, steer};
use voxunami_eels::round::{COUNTDOWN_TICKS, OVER_TICKS, STEP_TICKS};

const VIEW_RADIUS: i64 = 3;

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
    let start = camera_at([32.5, 40.0, 32.5], 0.0, 0.0, VIEW_RADIUS);
    let recorded = record_connection(connector.connect().unwrap(), &path).unwrap();
    let mut first =
        Session::<_, Eels>::connect(recorded, Mode::Playing, VIEW_RADIUS, start, || {
            server.step();
        })
        .unwrap();
    let second_connection: Box<dyn Connection> = Box::new(connector.connect().unwrap());
    let mut second =
        Session::<_, Eels>::connect(second_connection, Mode::Playing, VIEW_RADIUS, start, || {
            server.step();
        })
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
    let saved = server.save(&registries).unwrap();
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
const PINNED: (u64, usize, u64) = (
    16_102_118_019_284_220_051,
    35_366,
    7_844_908_670_161_894_155,
);
