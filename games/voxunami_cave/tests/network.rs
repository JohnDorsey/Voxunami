//! Playing over real TCP connections: a dedicated server, a hosted game, remote players.

mod common;

use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use common::{Player, PlayerExt, VIEW_RADIUS, above_ground, ground_height};
use game_shell::GameContent;
use game_shell::Identity;
use game_shell::files::{open_recording, open_world, record_connection};
use game_shell::host::{HostSettings, ServerThread};
use game_shell::options::ServeOptions;
use game_shell::serve::serve_until;
use game_shell::session::{Mode, Session};
use game_shell::view::camera_at;
use protocol::{ClientMessage, ServerMessage};
use transport::Connection;
use transport::tcp::{TcpConnection, TcpServerListener};
use voxel_space::{BrickBox, BrickCoordinate, LevelIndex};
use voxunami_cave::content::CaveContent;
use voxunami_cave::game::{Cave, break_block};

const SEED: u64 = 2024;

fn scratch_directory(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("voxunami_cave_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

/// A dedicated server on a free local port, running until dropped.
struct DedicatedServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<Result<(), String>>>,
}

impl DedicatedServer {
    fn start(world_file: Option<PathBuf>, autosave_seconds: f32) -> Self {
        Self::start_with_packs(world_file, autosave_seconds, None)
    }

    fn start_with_packs(
        world_file: Option<PathBuf>,
        autosave_seconds: f32,
        packs: Option<PathBuf>,
    ) -> Self {
        let listener = TcpServerListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_address().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let options = ServeOptions {
            listen: address.to_string(),
            seed: SEED,
            world_file,
            packs,
            autosave_seconds,
            view_radius: VIEW_RADIUS,
        };
        let thread = std::thread::spawn(move || serve_until::<Cave>(&options, listener, &stopping));
        Self {
            address,
            stop,
            thread: Some(thread),
        }
    }

    fn connect(&self) -> Box<dyn Connection> {
        Box::new(TcpConnection::connect(self.address, Duration::from_secs(5)).unwrap())
    }

    fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap().unwrap();
    }
}

impl Drop for DedicatedServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// How many players have joined servers in this file's tests: each is an identity of its own.
static JOINED: AtomicU8 = AtomicU8::new(0);

fn join(connection: Box<dyn Connection>, position: [f64; 3]) -> Player {
    let start = camera_at(position, 0.0, 0.0, VIEW_RADIUS);
    let identity = Identity::from_secret([JOINED.fetch_add(1, Ordering::Relaxed); 32]);
    Session::<_, Cave>::connect(
        connection,
        &identity,
        Mode::Playing,
        VIEW_RADIUS,
        start,
        || {
            std::thread::sleep(Duration::from_millis(1));
        },
    )
    .unwrap()
}

/// Lets the player take in what the server sends until their view is quiet.
fn settle(player: &mut Player) {
    player.send_pose();
    player
        .settle(|| std::thread::sleep(Duration::from_millis(5)))
        .unwrap();
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn players_on_a_dedicated_server_see_each_others_edits_and_positions() {
    let server = DedicatedServer::start(None, 60.0);
    let mut breaker = join(server.connect(), [16.5, 110.0, 16.5]);
    let mut watcher = join(server.connect(), [20.5, 110.0, 20.5]);
    settle(&mut breaker);
    settle(&mut watcher);
    breaker.camera = above_ground(&breaker, 16, 16, 3.0);
    breaker.teleport_to_camera();
    settle(&mut breaker);
    settle(&mut watcher);

    let hit = breaker.target().expect("the ground is within reach");
    assert_eq!(hit.voxel.y(), ground_height(&breaker, 16, 16));
    break_block(&mut breaker);
    wait_until("the other player to see the block go", || {
        let _ = breaker.advance();
        let _ = watcher.advance();
        let _ = watcher.update_view();
        watcher.mesher().is_opaque_at(hit.voxel) == Some(false)
    });

    // Each sees where the other is.
    wait_until("both players' positions", || {
        let _ = watcher.advance();
        watcher.players().len() == 2
    });
    drop(breaker);
    wait_until("the departure to be noticed", || {
        let _ = watcher.advance();
        watcher.players().len() == 1
    });
    server.stop();
}

#[test]
fn a_dedicated_server_saves_its_world_while_running_and_when_stopped() {
    let directory = scratch_directory("serve");
    let path = directory.join("world.voxunami");
    let server = DedicatedServer::start(Some(path.clone()), 0.3);
    let mut player = join(server.connect(), [16.5, 110.0, 16.5]);
    settle(&mut player);
    player.camera = above_ground(&player, 16, 16, 3.0);
    player.teleport_to_camera();
    settle(&mut player);
    let hit = player.target().unwrap();
    break_block(&mut player);

    // The autosave writes a world holding the edit without anyone stopping the server.
    let mut content = CaveContent::new();
    wait_until("an autosave with the edit", || {
        let _ = player.advance();
        path.exists()
            && open_world(&mut content, Some(&path), 0).is_ok_and(|world| {
                world.main().overrides.brick_count() == 1
                    && world
                        .main()
                        .overrides
                        .get(content.block_channel(), hit.voxel)
                        == content.blocks.air
            })
    });
    drop(player);
    server.stop();

    let world = open_world(&mut content, Some(&path), 0).unwrap();
    assert_eq!(world.world_seed, SEED);
    assert_eq!(
        world
            .main()
            .overrides
            .get(content.block_channel(), hit.voxel),
        content.blocks.air
    );
    assert!(
        !path.with_extension("voxunami.saving").exists(),
        "no temporary file is left behind"
    );
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_hosted_game_has_the_local_player_and_network_players_in_one_world() {
    let content = CaveContent::new();
    let world = content.new_world(SEED);
    let listener = TcpServerListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_address().unwrap();
    let mut settings = HostSettings::local(VIEW_RADIUS);
    settings.network = Some(Box::new(listener));
    let host = ServerThread::<Cave>::start(content, world, settings).unwrap();

    let mut local = join(Box::new(host.connect().unwrap()), [16.5, 110.0, 16.5]);
    let mut remote = join(
        Box::new(TcpConnection::connect(address, Duration::from_secs(5)).unwrap()),
        [20.5, 110.0, 20.5],
    );
    settle(&mut local);
    settle(&mut remote);
    local.camera = above_ground(&local, 16, 16, 3.0);
    local.teleport_to_camera();
    settle(&mut local);
    let hit = local.target().unwrap();
    break_block(&mut local);
    wait_until("the remote player to see the local player's edit", || {
        let _ = local.advance();
        let _ = remote.advance();
        let _ = remote.update_view();
        remote.mesher().is_opaque_at(hit.voxel) == Some(false)
    });
    wait_until("each to see the other", || {
        let _ = local.advance();
        let _ = remote.advance();
        local.players().len() == 2 && remote.players().len() == 2
    });
    drop(local);
    drop(remote);
    let (world, _) = host.stop();
    assert_eq!(world.main().overrides.brick_count(), 1);
}

#[test]
fn a_recording_made_over_the_network_replays_the_same_view() {
    let directory = scratch_directory("recorded");
    let path = directory.join("remote.voxreplay");
    let server = DedicatedServer::start(None, 60.0);
    let mut player = join(
        record_connection(
            TcpConnection::connect(server.address, Duration::from_secs(5)).unwrap(),
            &path,
        )
        .unwrap(),
        [16.5, 110.0, 16.5],
    );
    settle(&mut player);
    player.camera = above_ground(&player, 16, 16, 3.0);
    player.teleport_to_camera();
    settle(&mut player);
    let hit = player.target().unwrap();
    break_block(&mut player);
    wait_until("the edit to arrive", || {
        let _ = player.advance();
        let _ = player.update_view();
        player.mesher().is_opaque_at(hit.voxel) == Some(false)
    });
    settle(&mut player);
    player.advance().unwrap();
    let live_summary = player.client().world().summary();
    let live_camera = player.camera;
    drop(player);
    server.stop();

    let connection: Box<dyn Connection> = Box::new(open_recording(&path).unwrap());
    let mut replay = Session::<_, Cave>::connect(
        connection,
        &Identity::generate(),
        Mode::Replaying,
        VIEW_RADIUS,
        camera_at([0.0; 3], 0.0, 0.0, VIEW_RADIUS),
        || {},
    )
    .unwrap();
    while replay.advance_one_tick().is_ok() {}
    replay.settle(|| {}).unwrap();
    assert_eq!(replay.client().world().summary(), live_summary);
    assert_eq!(replay.camera.position, live_camera.position);
    assert_eq!(replay.mesher().is_opaque_at(hit.voxel), Some(false));
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_connection_that_speaks_nonsense_is_dropped_without_disturbing_anyone_else() {
    let server = DedicatedServer::start(None, 60.0);
    let mut player = join(server.connect(), [16.5, 110.0, 16.5]);
    settle(&mut player);

    // Not even a frame: random bytes with an absurd length.
    let mut vandal = TcpStream::connect(server.address).unwrap();
    vandal.write_all(&[0xFF; 64]).unwrap();
    // A well-framed message that is not the protocol.
    let mut confused = TcpStream::connect(server.address).unwrap();
    let junk = [1u8, 2, 3];
    confused
        .write_all(&(junk.len() as u32).to_le_bytes())
        .unwrap();
    confused.write_all(&junk).unwrap();
    // Silence: connects and says nothing for a good while.
    let _silent = TcpStream::connect(server.address).unwrap();

    std::thread::sleep(Duration::from_millis(300));
    settle(&mut player);
    assert!(
        player.client().world().tick().get() > 5,
        "the server carried on"
    );
    let mut late = join(server.connect(), [30.5, 110.0, 30.5]);
    settle(&mut late);
    assert!(late.mesher().loaded_brick_count() > 0);
    server.stop();
}

#[test]
fn joining_a_server_that_is_not_there_is_an_error_not_a_hang() {
    let listener = TcpServerListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_address().unwrap();
    drop(listener);
    let started = Instant::now();
    assert!(TcpConnection::connect(address, Duration::from_secs(3)).is_err());
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_player_who_asks_to_watch_more_than_the_server_allows_is_refused_with_a_reason() {
    let server = DedicatedServer::start(None, 60.0);
    let mut connection = server.connect();
    let identity = Identity::from_secret([200; 32]);
    let game = voxunami_cave::game_id();
    connection.send(identity.hello(&game).encode()).unwrap();
    let challenge = loop {
        if let Some(frame) = connection.receive().unwrap() {
            break frame;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let proof = identity
        .answer(&game, &challenge)
        .expect("the server answers a hello with a challenge");
    connection.send(proof.encode()).unwrap();
    connection
        .send(
            ClientMessage::SetInterest {
                level: LevelIndex::MAIN,
                region: BrickBox::around(BrickCoordinate::ORIGIN, 40),
            }
            .encode(),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let reason = loop {
        match connection.receive() {
            Ok(Some(frame)) => {
                if let ServerMessage::Disconnected { reason } =
                    ServerMessage::decode(&frame).unwrap()
                {
                    break reason;
                }
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(_) => panic!("the connection closed without saying why"),
        }
        assert!(Instant::now() < deadline, "no answer");
    };
    assert!(reason.contains("limit"), "{reason}");
    server.stop();
}

#[test]
fn a_player_over_the_network_goes_to_another_level_with_a_command_and_changes_only_that_level() {
    let content = CaveContent::new();
    let mut world = content.new_world(SEED);
    let cellar = world
        .add_level(
            "voxunami_cave:cellar".parse().unwrap(),
            content.new_world(SEED).into_main(),
        )
        .unwrap();
    let listener = TcpServerListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_address().unwrap();
    let mut settings = HostSettings::local(VIEW_RADIUS);
    settings.network = Some(Box::new(listener));
    let host = ServerThread::<Cave>::start(content, world, settings).unwrap();

    let mut remote = join(
        Box::new(TcpConnection::connect(address, Duration::from_secs(5)).unwrap()),
        [16.5, 110.0, 16.5],
    );
    settle(&mut remote);
    remote.send_command("/level go cellar");
    wait_until("the move to the cellar", || {
        let _ = remote.advance();
        let _ = remote.update_view();
        remote.level() == cellar
            && remote.client().world().watched().map(|(level, _)| level) == Some(cellar)
    });
    assert_eq!(
        remote.take_command_replies(),
        ["you are now in voxunami_cave:cellar"]
    );
    settle(&mut remote);
    remote.camera = above_ground(&remote, 16, 16, 3.0);
    remote.teleport_to_camera();
    settle(&mut remote);
    let hit = remote.target().expect("the ground is within reach");
    break_block(&mut remote);
    wait_until("the edit to come back", || {
        let _ = remote.advance();
        let _ = remote.update_view();
        remote.mesher().is_opaque_at(hit.voxel) == Some(false)
    });
    drop(remote);
    let (world, _) = host.stop();
    assert_eq!(world.main().overrides.brick_count(), 0);
    assert_eq!(world.level(cellar).unwrap().overrides.brick_count(), 1);
}

#[test]
fn a_dedicated_server_started_with_packs_gives_every_player_the_packs_blocks() {
    let packs = scratch_directory("packs");
    let pack = packs.join("my_pack");
    std::fs::create_dir_all(pack.join("blocks")).unwrap();
    std::fs::write(
        pack.join("pack.json"),
        r#"{ "id": "my_pack", "depends_on": ["base_blocks"] }"#,
    )
    .unwrap();
    std::fs::write(
        pack.join("blocks").join("marble.json"),
        r#"{ "color": [0.9, 0.9, 0.95] }"#,
    )
    .unwrap();

    let server = DedicatedServer::start_with_packs(None, 60.0, Some(packs.clone()));
    let mut player = join(server.connect(), [16.5, 110.0, 16.5]);
    settle(&mut player);
    // This program's own copy of Cave has never heard of marble; the server's package is what draws it.
    let marble = player.content().registries()[0]
        .1
        .raw_id_of(&"my_pack:marble".parse().unwrap())
        .expect("the player knows the block by name");
    assert_eq!(player.block_table().color(marble), [0.9, 0.9, 0.95]);
    server.stop();
    std::fs::remove_dir_all(&packs).unwrap();
}

#[test]
fn a_dedicated_server_does_not_start_when_its_packs_have_problems() {
    let packs = scratch_directory("bad_packs");
    std::fs::create_dir_all(packs.join("my_pack")).unwrap();
    std::fs::write(
        packs.join("my_pack").join("pack.json"),
        r#"{ "id": "other" }"#,
    )
    .unwrap();
    // Already told to stop: if the server did start, it would stop at once and this would fail, not hang.
    let listener = TcpServerListener::bind("127.0.0.1:0").unwrap();
    let options = ServeOptions {
        listen: "127.0.0.1:0".to_owned(),
        seed: SEED,
        world_file: None,
        packs: Some(packs.clone()),
        autosave_seconds: 60.0,
        view_radius: VIEW_RADIUS,
    };
    let error = serve_until::<Cave>(&options, listener, &AtomicBool::new(true)).unwrap_err();
    assert!(
        error.contains(
            "my_pack/pack.json: /id: the folder is called \"my_pack\" but pack.json says \"other\""
        ) && error.contains("nothing was loaded"),
        "{error}"
    );
    std::fs::remove_dir_all(&packs).unwrap();
}
