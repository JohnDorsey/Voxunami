//! What the 2048 server does: slides the board when a player pushes, adds a tile, keeps score, and
//! tells everyone how the game stands.
//!
//! The board itself is not remembered here: it is read from the world's voxels whenever it is
//! needed, so the voxels (which are saved, streamed and recorded) are always the truth. What the
//! voxels do not show, the score and move count, is saved with the world through
//! [`GameLogic::save_sections`].

use game_shell::PoseBook;
use namespaced_id::NamespacedId;
use protocol::{ClientId, Tick};
use server::{Audience, GameLogic, ServerWorld};
use voxel_grid::ChannelIndex;
use voxel_space::FaceDirection;
use world_storage::{Section, WorldData};

use crate::board::Board;
use crate::content::{Content2048, game_seed, read_board, starting_board};
use crate::layout::{cells, tile_voxels};
use crate::messages::{
    SavedState, Status, decode_push, encode_status, new_game_kind, push_kind, read_saved_state,
    saved_state_section, status_kind,
};

#[derive(Debug)]
pub struct Logic2048 {
    seed: u64,
    channel: ChannelIndex,
    state: SavedState,
    poses: PoseBook,
    status_changed: bool,
}

impl Logic2048 {
    /// The rules for the game in `world`, continuing from the state saved with it, if any.
    pub fn new(content: &Content2048, world: &WorldData) -> Self {
        Self {
            seed: world.world_seed,
            channel: content.block_channel(),
            state: read_saved_state(world.opaque_sections()).unwrap_or_default(),
            poses: PoseBook::new(),
            status_changed: false,
        }
    }

    fn board(&self, world: &ServerWorld) -> Board {
        read_board(&world.data().overrides, self.channel)
    }

    fn status(&self, board: &Board) -> Status {
        Status {
            score: self.state.score,
            moves: self.state.moves,
            highest: board.highest(),
            game_over: !board.can_move(),
            game_number: self.state.game_number,
        }
    }

    /// Changes the voxels of every cell that differs between `from` and `to`.
    fn show(&self, world: &mut ServerWorld, from: &Board, to: &Board) {
        for cell in cells().filter(|&cell| from.get(cell) != to.get(cell)) {
            let block = match to.get(cell) {
                0 => Content2048::air(),
                exponent => Content2048::tile_block(exponent),
            };
            for voxel in tile_voxels(cell) {
                world
                    .set_voxel(self.channel, voxel, block)
                    .expect("the void has no unavailable terrain");
            }
        }
    }

    fn push(&mut self, toward: FaceDirection, world: &mut ServerWorld) {
        // Once the game is over no slide moves anything, so pushes do nothing until a new game.
        let before = self.board(world);
        let slide = before.slide(toward);
        if !slide.moved {
            return;
        }
        let mut after = slide.board;
        after.spawn(
            game_seed(self.seed, self.state.game_number),
            2 + self.state.moves,
        );
        self.state.score += slide.score;
        self.state.moves += 1;
        self.show(world, &before, &after);
        self.status_changed = true;
    }

    fn new_game(&mut self, world: &mut ServerWorld) {
        let before = self.board(world);
        self.state = SavedState {
            score: 0,
            moves: 0,
            game_number: self.state.game_number + 1,
        };
        let after = starting_board(self.seed, self.state.game_number);
        self.show(world, &before, &after);
        self.status_changed = true;
    }
}

impl GameLogic for Logic2048 {
    fn client_joined(&mut self, client: ClientId, world: &mut ServerWorld) {
        let status = self.status(&self.board(world));
        world.send(
            Audience::Client(client),
            status_kind(),
            encode_status(&status),
        );
    }

    fn client_left(&mut self, client: ClientId, _world: &mut ServerWorld) {
        self.poses.client_left(client);
    }

    fn message_received(
        &mut self,
        client: ClientId,
        kind: &NamespacedId,
        payload: &[u8],
        world: &mut ServerWorld,
    ) {
        if self.poses.handle_message(client, kind, payload) {
            return;
        }
        if *kind == push_kind() {
            if let Ok(toward) = decode_push(payload) {
                self.push(toward, world);
            }
        } else if *kind == new_game_kind() {
            self.new_game(world);
        }
        // Anything else is another game's, or malformed: ignored.
    }

    fn tick(&mut self, _tick: Tick, world: &mut ServerWorld) {
        self.poses.tell_everyone(world);
        if self.status_changed {
            let status = self.status(&self.board(world));
            world.send(Audience::Everyone, status_kind(), encode_status(&status));
            self.status_changed = false;
        }
    }

    fn save_sections(&self) -> Vec<Section> {
        vec![saved_state_section(&self.state)]
    }
}
