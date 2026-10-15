//! Voxunami 2048, played through its server and sessions, without a window.

use game_shell::GameContent;
use game_shell::files::{open_recording, record_connection};
use game_shell::host::build_server;
use game_shell::session::{Mode, Session};
use game_shell::view::camera_at;
use proptest::prelude::*;
use renderer::Camera;
use server::Server;
use transport::Connection;
use transport::loopback::LoopbackConnector;
use voxel_space::FaceDirection;
use voxunami_2048::board::{Board, CELL_COUNT, cell_coordinates, cell_index};
use voxunami_2048::content::{Content2048, game_seed, read_board, starting_board, write_cell};
use voxunami_2048::game::{Game2048, push_with_tool, start_new_game};
use voxunami_2048::messages::{
    Status, encode_push, new_game_kind, push_kind, read_saved_state, saved_state_section_id,
};
use world_storage::WorldData;

const SEED: u64 = 2048;
const VIEW_RADIUS: i64 = 3;

type Player = Session<Box<dyn Connection>, Game2048>;

struct Rig {
    content: Content2048,
    server: Server,
    connector: LoopbackConnector,
}

impl Rig {
    fn new() -> Self {
        let content = Content2048::new();
        let world = content.new_world(SEED);
        Self::with_world(content, world)
    }

    fn with_world(content: Content2048, world: WorldData) -> Self {
        let (server, connector) = build_server::<Game2048>(&content, world, VIEW_RADIUS).unwrap();
        Self {
            content,
            server,
            connector,
        }
    }

    /// A player in front of the plate that pushes tiles toward `toward`, looking at it.
    fn join_facing(&mut self, toward: Option<FaceDirection>) -> Player {
        let connection = self.connector.connect().unwrap();
        self.join_with(Box::new(connection), toward)
    }

    fn join_with(
        &mut self,
        connection: Box<dyn Connection>,
        toward: Option<FaceDirection>,
    ) -> Player {
        // The plate pushing toward +x is on the low-x side: stand beyond it and look along +x.
        let (position, yaw_degrees) = match toward {
            Some(FaceDirection::PositiveX) => ([-12.0, 13.5, 13.5], -90.0),
            Some(FaceDirection::NegativeX) => ([44.0, 13.5, 13.5], 90.0),
            Some(FaceDirection::PositiveZ) => ([13.5, 13.5, -12.0], 180.0),
            Some(FaceDirection::NegativeZ) => ([13.5, 13.5, 44.0], 0.0),
            Some(FaceDirection::PositiveY) => ([13.5, -12.0, 13.5], 0.0),
            Some(FaceDirection::NegativeY) => ([13.5, 44.0, 13.5], 0.0),
            None => ([13.5, 13.5, 60.0], 0.0),
        };
        let pitch_degrees = match toward {
            Some(FaceDirection::PositiveY) => 89.0,
            Some(FaceDirection::NegativeY) => -89.0,
            _ => 0.0,
        };
        let start = camera_at(position, yaw_degrees, pitch_degrees, VIEW_RADIUS);
        let server = &mut self.server;
        Session::<_, Game2048>::connect(connection, Mode::Playing, VIEW_RADIUS, start, || {
            server.step();
        })
        .unwrap()
    }

    fn settle(&mut self, players: &mut [&mut Player]) {
        for player in players.iter_mut() {
            player.send_pose();
        }
        for _ in 0..2 {
            for player in players.iter_mut() {
                let server = &mut self.server;
                player
                    .settle(|| {
                        server.step();
                    })
                    .unwrap();
            }
        }
    }

    fn board(&self) -> Board {
        read_board(&self.server.world().overrides, self.content.block_channel())
    }
}

fn board_seen_by(player: &Player, content: &Content2048) -> Board {
    read_board(
        &player.client().world().world().overrides,
        content.block_channel(),
    )
}

/// What the rules say the board and score are after pushes, with spawning as the server does it.
#[derive(Clone, Debug)]
struct Model {
    board: Board,
    score: u64,
    moves: u64,
    game_number: u64,
}

impl Model {
    fn new() -> Self {
        Self {
            board: starting_board(SEED, 0),
            score: 0,
            moves: 0,
            game_number: 0,
        }
    }

    fn push(&mut self, toward: FaceDirection) {
        if !self.board.can_move() {
            return;
        }
        let slide = self.board.slide(toward);
        if !slide.moved {
            return;
        }
        let mut board = slide.board;
        board.spawn(game_seed(SEED, self.game_number), 2 + self.moves);
        self.board = board;
        self.score += slide.score;
        self.moves += 1;
    }

    fn new_game(&mut self) {
        self.game_number += 1;
        self.board = starting_board(SEED, self.game_number);
        self.score = 0;
        self.moves = 0;
    }

    fn status(&self) -> Status {
        Status {
            score: self.score,
            moves: self.moves,
            highest: self.board.highest(),
            game_over: !self.board.can_move(),
            game_number: self.game_number,
        }
    }
}

fn send_push(player: &mut Player, toward: FaceDirection) {
    player.send_game_message(push_kind(), encode_push(toward));
}

fn run_ticks(rig: &mut Rig, players: &mut [&mut Player]) {
    for _ in 0..2 {
        rig.server.step();
    }
    for player in players.iter_mut() {
        player.advance().unwrap();
        let _ = player.update_view();
    }
}

#[test]
fn a_new_game_shows_two_tiles_and_a_score_of_nothing_to_everyone_who_joins() {
    let mut rig = Rig::new();
    let mut player = rig.join_facing(None);
    rig.settle(&mut [&mut player]);

    let board = board_seen_by(&player, &rig.content);
    assert_eq!(board, starting_board(SEED, 0));
    assert_eq!(board.tile_count(), 2);
    assert_eq!(player.state.status, Model::new().status());
    // And the server's own world agrees.
    assert_eq!(rig.board(), board);
}

#[test]
fn clicking_a_plate_pushes_the_tiles_toward_the_opposite_side() {
    for toward in FaceDirection::ALL {
        let mut rig = Rig::new();
        let mut player = rig.join_facing(Some(toward));
        rig.settle(&mut [&mut player]);
        push_with_tool(&mut player);
        run_ticks(&mut rig, &mut [&mut player]);
        rig.settle(&mut [&mut player]);

        let mut expected = Model::new();
        expected.push(toward);
        if expected.moves == 0 {
            // The first board happened to be packed already against that side: nothing moves.
            assert_eq!(rig.board(), expected.board, "{toward:?}");
            continue;
        }
        assert_eq!(rig.board(), expected.board, "{toward:?}");
        assert_eq!(
            board_seen_by(&player, &rig.content),
            expected.board,
            "{toward:?}"
        );
        assert_eq!(player.state.status, expected.status(), "{toward:?}");
    }
}

#[test]
fn clicking_where_there_is_no_plate_does_nothing() {
    let mut rig = Rig::new();
    // Looking at the board from the front: the middle of the view is a tile or the frame, not a plate.
    let mut player = rig.join_facing(None);
    rig.settle(&mut [&mut player]);
    player.camera = Camera {
        pitch: 1.0,
        ..player.camera
    }; // looking up into the empty sky
    push_with_tool(&mut player);
    run_ticks(&mut rig, &mut [&mut player]);
    assert_eq!(rig.board(), starting_board(SEED, 0));
    assert_eq!(player.state.status.moves, 0);
}

fn direction() -> impl Strategy<Value = FaceDirection> {
    (0u8..6).prop_map(|index| FaceDirection::from_index(index).unwrap())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10))]

    /// Whatever sequence of pushes (and new games) players send, the world, every player's picture
    /// and the score match the rules played out on a plain board.
    #[test]
    fn the_server_plays_exactly_the_rules(
        script in proptest::collection::vec(
            prop_oneof![9 => direction().prop_map(Some), 1 => Just(None)],
            1..40,
        ),
    ) {
        let mut rig = Rig::new();
        let mut pusher = rig.join_facing(None);
        let mut watcher = rig.join_facing(None);
        rig.settle(&mut [&mut pusher, &mut watcher]);
        let mut model = Model::new();
        for step in script {
            match step {
                Some(toward) => {
                    send_push(&mut pusher, toward);
                    model.push(toward);
                }
                None => {
                    start_new_game(&mut pusher);
                    model.new_game();
                }
            }
            run_ticks(&mut rig, &mut [&mut pusher, &mut watcher]);
            prop_assert_eq!(rig.board(), model.board.clone());
            prop_assert_eq!(pusher.state.status, model.status());
        }
        rig.settle(&mut [&mut pusher, &mut watcher]);
        prop_assert_eq!(board_seen_by(&pusher, &rig.content), model.board.clone());
        prop_assert_eq!(board_seen_by(&watcher, &rig.content), model.board.clone());
        prop_assert_eq!(watcher.state.status, model.status());
    }
}

/// A board with every cell full and no two neighbours equal.
fn stuck_board() -> Board {
    let mut cells = [0u8; CELL_COUNT];
    for (index, cell) in cells.iter_mut().enumerate() {
        let [x, y, z] = cell_coordinates(index);
        *cell = ((x + y + z) % 3) as u8 + 1;
    }
    Board::from_cells(cells)
}

#[test]
fn when_no_push_can_change_the_board_the_game_is_over_until_a_new_one_starts() {
    let content = Content2048::new();
    let mut world = content.new_world(SEED);
    let stuck = stuck_board();
    for cell in 0..CELL_COUNT {
        write_cell(
            &mut world.overrides,
            content.block_channel(),
            cell,
            stuck.get(cell),
        );
    }
    let mut rig = Rig::with_world(content, world);
    let mut player = rig.join_facing(None);
    rig.settle(&mut [&mut player]);
    assert!(player.state.status.game_over);

    for toward in FaceDirection::ALL {
        send_push(&mut player, toward);
    }
    run_ticks(&mut rig, &mut [&mut player]);
    assert_eq!(rig.board(), stuck, "nothing moved");
    assert_eq!(player.state.status.moves, 0);

    start_new_game(&mut player);
    run_ticks(&mut rig, &mut [&mut player]);
    assert_eq!(rig.board(), starting_board(SEED, 1));
    assert!(!player.state.status.game_over);
    assert_eq!(player.state.status.game_number, 1);
}

#[test]
fn everyone_on_the_server_plays_the_same_game_and_a_late_joiner_hears_the_score() {
    let mut rig = Rig::new();
    let mut first = rig.join_facing(None);
    let mut second = rig.join_facing(None);
    rig.settle(&mut [&mut first, &mut second]);
    let mut model = Model::new();
    for toward in [
        FaceDirection::PositiveX,
        FaceDirection::NegativeZ,
        FaceDirection::PositiveY,
    ] {
        send_push(&mut first, toward);
        model.push(toward);
        run_ticks(&mut rig, &mut [&mut first, &mut second]);
    }
    send_push(&mut second, FaceDirection::NegativeX);
    model.push(FaceDirection::NegativeX);
    run_ticks(&mut rig, &mut [&mut first, &mut second]);
    assert_eq!(first.state.status, model.status());
    assert_eq!(second.state.status, model.status());

    let mut late = rig.join_facing(None);
    rig.settle(&mut [&mut late]);
    assert_eq!(late.state.status, model.status());
    assert_eq!(board_seen_by(&late, &rig.content), model.board);
}

#[test]
fn the_score_and_the_sequence_of_tiles_survive_saving_and_loading() {
    let mut rig = Rig::new();
    let mut player = rig.join_facing(None);
    rig.settle(&mut [&mut player]);
    let mut model = Model::new();
    let script = [
        FaceDirection::PositiveX,
        FaceDirection::PositiveZ,
        FaceDirection::NegativeY,
        FaceDirection::NegativeX,
        FaceDirection::PositiveZ,
    ];
    for toward in script {
        send_push(&mut player, toward);
        model.push(toward);
        run_ticks(&mut rig, &mut [&mut player]);
    }
    player.say_goodbye();
    rig.server.step();
    let Rig {
        content, server, ..
    } = rig;
    let world = server.into_world();
    let saved = read_saved_state(world.opaque_sections()).expect("the game saved its state");
    assert_eq!((saved.score, saved.moves), (model.score, model.moves));
    let bytes = world.save(&content.registries()).unwrap();

    // A new program run, from the file.
    let mut content = Content2048::new();
    let parsed = world_storage::ParsedWorld::parse(&bytes).unwrap();
    let seed = parsed.world_seed();
    let (loaded, report) = parsed.load(content.load_context(seed)).unwrap();
    // The loader keeps every plugin section as it is; the game reads its own from the world.
    assert_eq!(report.opaque_sections, [saved_state_section_id()]);
    assert_eq!(read_saved_state(loaded.opaque_sections()), Some(saved));
    let mut rig = Rig::with_world(content, loaded);
    let mut player = rig.join_facing(None);
    rig.settle(&mut [&mut player]);
    assert_eq!(player.state.status, model.status());
    // The game carries on exactly as if it had never stopped.
    for toward in [
        FaceDirection::NegativeZ,
        FaceDirection::PositiveX,
        FaceDirection::NegativeY,
    ] {
        send_push(&mut player, toward);
        model.push(toward);
        run_ticks(&mut rig, &mut [&mut player]);
        assert_eq!(rig.board(), model.board);
    }
    assert_eq!(player.state.status, model.status());
}

#[test]
fn a_recording_of_a_game_replays_the_board_and_the_score() {
    let directory =
        std::env::temp_dir().join(format!("voxunami_2048_replay_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("game.voxreplay");

    let mut rig = Rig::new();
    let connection = rig.connector.connect().unwrap();
    let recorded = record_connection(connection, &path).unwrap();
    let mut player = rig.join_with(recorded, None);
    rig.settle(&mut [&mut player]);
    let mut model = Model::new();
    for toward in [
        FaceDirection::PositiveX,
        FaceDirection::PositiveY,
        FaceDirection::NegativeZ,
        FaceDirection::NegativeX,
    ] {
        send_push(&mut player, toward);
        model.push(toward);
        run_ticks(&mut rig, &mut [&mut player]);
    }
    rig.settle(&mut [&mut player]);
    let live_summary = player.client().world().summary();
    drop(player);

    let connection: Box<dyn Connection> = Box::new(open_recording(&path).unwrap());
    let mut replay = Session::<_, Game2048>::connect(
        connection,
        Mode::Replaying,
        VIEW_RADIUS,
        camera_at([0.0; 3], 0.0, 0.0, VIEW_RADIUS),
        || {},
    )
    .unwrap();
    while replay.advance_one_tick().is_ok() {}
    assert_eq!(replay.client().world().summary(), live_summary);
    assert_eq!(board_seen_by(&replay, &rig.content), model.board);
    assert_eq!(replay.state.status, model.status());
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn messages_that_are_not_pushes_or_new_games_are_ignored() {
    let mut rig = Rig::new();
    let mut player = rig.join_facing(None);
    rig.settle(&mut [&mut player]);
    player.send_game_message(push_kind(), vec![]);
    player.send_game_message(push_kind(), vec![9]);
    player.send_game_message(push_kind(), vec![1, 1]);
    player.send_game_message("other_game:hello".parse().unwrap(), vec![1, 2, 3]);
    run_ticks(&mut rig, &mut [&mut player]);
    assert_eq!(rig.board(), starting_board(SEED, 0));
    assert_eq!(player.state.status.moves, 0);
    // A new-game message with a body is still a new game: the body means nothing.
    player.send_game_message(new_game_kind(), vec![5]);
    run_ticks(&mut rig, &mut [&mut player]);
    assert_eq!(player.state.status.game_number, 1);
    let _ = cell_index(0, 0, 0);
}
