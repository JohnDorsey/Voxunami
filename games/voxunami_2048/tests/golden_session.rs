//! A whole scripted session of Voxunami 2048, locked bit for bit: the stream of frames one player
//! receives (a replay file) and the saved world, hashed and pinned.
//!
//! The script uses fixed tick counts, never a settle that steps the server until the client's worker
//! threads are quiet (that number varies). A change to how the game's rules are put together must leave
//! the numbers as they are; if one must change on purpose, say why in the commit, and take it on
//! Windows and on Linux, where it must agree to the bit.

use game_shell::GameContent;
use game_shell::files::record_connection;
use game_shell::session::{Mode, Session};
use game_shell::view::camera_at;
use protocol::Message;
use transport::Connection;
use voxel_space::{FaceDirection, VoxelCoordinate};
use voxunami_2048::content::Content2048;
use voxunami_2048::game::Game2048;
use voxunami_2048::material::Material;
use voxunami_2048::messages::PushRequest;
use voxunami_2048::numbered_block::NumberedBlock;
use voxunami_2048::tool::Tool;

const SEED: u64 = 2048;
const VIEW_RADIUS: i64 = 3;
/// Above the tallest hill the landscape makes.
const SKY: i64 = 100;

fn at(x: i64, y: i64, z: i64) -> VoxelCoordinate {
    VoxelCoordinate::new(x, y, z)
}

fn block(material: Material, exponent: u8) -> NumberedBlock {
    NumberedBlock::new(material, 1 << exponent)
}

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

type Player = Session<Box<dyn Connection>, Game2048>;

fn push(player: &mut Player, voxel: VoxelCoordinate, direction: FaceDirection, tool: Tool) {
    player.send_game_message(
        PushRequest::kind(),
        PushRequest {
            voxel,
            direction,
            tool,
        }
        .encode(),
    );
}

fn ticks(server: &mut server::Server, player: &mut Player, count: usize) {
    for _ in 0..count {
        server.step();
        player.advance().unwrap();
    }
}

fn session(name: &str, arrangement: usize) -> (u64, usize, u64) {
    let directory = std::env::temp_dir().join(format!(
        "voxunami_2048_golden_{}_{name}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("session.voxreplay");

    let content = Content2048::new();
    let mut world = content.new_world(SEED);
    let blocks = [
        // Two stones that merge, then a third equal one.
        (at(10, SKY, 10), block(Material::Stone, 2)),
        (at(10, SKY, 13), block(Material::Stone, 2)),
        (at(10, SKY, 16), block(Material::Stone, 3)),
        // Ice that coasts to spring ice, which bounces unequal blocks back.
        (at(20, SKY, 10), block(Material::Ice, 1)),
        (at(20, SKY, 15), block(Material::SpringIce, 4)),
        // A row of stones pushed along together.
        (at(30, SKY, 10), block(Material::Stone, 1)),
        (at(31, SKY, 10), block(Material::Stone, 2)),
        (at(32, SKY, 10), block(Material::Stone, 1)),
        // Blocks for the other tools.
        (at(40, SKY, 10), block(Material::Stone, 3)),
        (at(40, SKY, 12), block(Material::Stone, 1)),
        (at(46, SKY, 10), block(Material::Stone, 2)),
        (at(46, SKY, 12), block(Material::Stone, 2)),
        // Ice with a long clear run, which coasts as far as ice can.
        (at(38, SKY, 14), block(Material::Ice, 2)),
    ];
    for (voxel, numbered) in blocks {
        world
            .main_mut()
            .overrides
            .set(content.block_channel(), voxel, numbered.block_id());
        world
            .main_mut()
            .overrides
            .set(content.number_channel(), voxel, numbered.state());
    }
    let (mut server, connector) = game_shell::host::build_server_with_plugins::<Game2048>(
        &content,
        world,
        VIEW_RADIUS,
        arranged(
            <Game2048 as game_shell::GameDefinition>::plugins(&content),
            arrangement,
        ),
    )
    .unwrap();
    let occupied = |server: &server::Server| {
        blocks
            .iter()
            .filter(|(voxel, _)| {
                server
                    .world()
                    .main()
                    .overrides
                    .get(content.block_channel(), *voxel)
                    != 0
            })
            .count()
    };
    assert_eq!(occupied(&server), blocks.len());
    let connection = connector.connect().unwrap();
    let recorded = record_connection(connection, &path).unwrap();
    let start = camera_at([30.5, SKY as f64 + 2.0, 12.5], 0.0, 0.0, VIEW_RADIUS);
    let mut player =
        Session::<_, Game2048>::connect(recorded, Mode::Playing, VIEW_RADIUS, start, || {
            server.step();
        })
        .unwrap();
    player.send_pose();
    ticks(&mut server, &mut player, 5);

    push(
        &mut player,
        at(10, SKY, 10),
        FaceDirection::PositiveZ,
        Tool::Double,
    );
    ticks(&mut server, &mut player, 20);
    push(
        &mut player,
        at(10, SKY, 13),
        FaceDirection::PositiveZ,
        Tool::Double,
    );
    ticks(&mut server, &mut player, 20);
    push(
        &mut player,
        at(20, SKY, 10),
        FaceDirection::PositiveZ,
        Tool::Double,
    );
    ticks(&mut server, &mut player, 40);
    push(
        &mut player,
        at(30, SKY, 10),
        FaceDirection::PositiveX,
        Tool::Double,
    );
    ticks(&mut server, &mut player, 20);
    push(
        &mut player,
        at(40, SKY, 10),
        FaceDirection::PositiveZ,
        Tool::Add,
    );
    ticks(&mut server, &mut player, 20);
    push(
        &mut player,
        at(46, SKY, 10),
        FaceDirection::PositiveZ,
        Tool::Subtract,
    );
    ticks(&mut server, &mut player, 20);
    push(
        &mut player,
        at(38, SKY, 14),
        FaceDirection::PositiveX,
        Tool::Double,
    );
    ticks(&mut server, &mut player, 80);
    // Something that does nothing: a push at air.
    push(
        &mut player,
        at(5, SKY, 5),
        FaceDirection::PositiveX,
        Tool::Double,
    );
    ticks(&mut server, &mut player, 10);
    // (So that the number locks something: the pushes moved, merged and cancelled blocks.)
    assert_ne!(occupied(&server), blocks.len());
    drop(player); // closes the recording

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
const PINNED: (u64, usize, u64) = (14_697_639_412_026_886_717, 3_998, 1_379_750_969_377_606_339);
