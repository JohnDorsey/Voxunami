//! The rules of 3D 2048, on their own: a cube of cells, tiles that slide and merge, and tiles that
//! appear. Nothing here knows about voxels, servers or screens.

use terrain::mix;
use voxel_space::FaceDirection;

/// Cells along each edge of the board.
pub const SIZE: usize = 4;
pub const CELL_COUNT: usize = SIZE * SIZE * SIZE;

/// The largest tile exponent: tiles of 2^24 do not merge further, so a value always fits.
pub const MAXIMUM_EXPONENT: u8 = 24;

/// The tile that wins the game, as an exponent: 2^11 = 2048.
pub const WINNING_EXPONENT: u8 = 11;

/// The index of the cell at `(x, y, z)`, each in `0..SIZE`.
pub fn cell_index(x: usize, y: usize, z: usize) -> usize {
    debug_assert!(x < SIZE && y < SIZE && z < SIZE);
    x + SIZE * (y + SIZE * z)
}

/// The coordinates of a cell index.
pub fn cell_coordinates(index: usize) -> [usize; 3] {
    [index % SIZE, index / SIZE % SIZE, index / (SIZE * SIZE)]
}

/// A board: each cell holds nothing (0) or a tile, stored as its exponent (1 is a 2, 2 is a 4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Board {
    cells: [u8; CELL_COUNT],
}

/// What sliding the board did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slide {
    pub board: Board,
    /// The value of every tile made by merging, added up: the points this move scores.
    pub score: u64,
    /// Whether any tile moved or merged. A slide that changes nothing is not a move.
    pub moved: bool,
}

impl Default for Board {
    fn default() -> Self {
        Self::empty()
    }
}

impl Board {
    pub fn empty() -> Self {
        Self {
            cells: [0; CELL_COUNT],
        }
    }

    pub fn from_cells(cells: [u8; CELL_COUNT]) -> Self {
        Self { cells }
    }

    pub fn cells(&self) -> &[u8; CELL_COUNT] {
        &self.cells
    }

    pub fn get(&self, index: usize) -> u8 {
        self.cells[index]
    }

    pub fn set(&mut self, index: usize, exponent: u8) {
        self.cells[index] = exponent;
    }

    pub fn tile_count(&self) -> usize {
        self.cells.iter().filter(|&&cell| cell != 0).count()
    }

    /// The exponent of the largest tile, or 0 on an empty board.
    pub fn highest(&self) -> u8 {
        self.cells.iter().copied().max().unwrap_or(0)
    }

    /// The sum of the values of all tiles.
    pub fn total_value(&self) -> u64 {
        self.cells
            .iter()
            .filter(|&&cell| cell != 0)
            .map(|&cell| 1u64 << cell)
            .sum()
    }

    /// Indices of the cells with no tile, in order.
    pub fn empty_cells(&self) -> Vec<usize> {
        (0..CELL_COUNT)
            .filter(|&index| self.cells[index] == 0)
            .collect()
    }

    /// Pushes every tile as far as it goes toward `toward`, merging equal tiles that meet. A tile
    /// merges at most once per slide, and the tiles nearest the destination merge first, so a row of
    /// four equal tiles becomes two tiles, not one.
    pub fn slide(&self, toward: FaceDirection) -> Slide {
        let axis = toward.axis();
        let mut board = Self::empty();
        let mut score = 0u64;
        // Every line of cells along the axis, as the two coordinates that stay fixed.
        for first in 0..SIZE {
            for second in 0..SIZE {
                let place = |along: usize| {
                    let mut at = [0usize; 3];
                    at[axis] = along;
                    at[(axis + 1) % 3] = first;
                    at[(axis + 2) % 3] = second;
                    cell_index(at[0], at[1], at[2])
                };
                // Positions along the axis in the order tiles travel: destination end first.
                let travel: Vec<usize> = if toward.is_positive() {
                    (0..SIZE).rev().collect()
                } else {
                    (0..SIZE).collect()
                };
                let line: Vec<u8> = travel
                    .iter()
                    .map(|&along| self.cells[place(along)])
                    .collect();
                let (merged, gained) = slide_line(&line);
                score += gained;
                for (&along, exponent) in travel.iter().zip(merged) {
                    board.cells[place(along)] = exponent;
                }
            }
        }
        let moved = board != *self;
        Slide {
            board,
            score,
            moved,
        }
    }

    /// Whether any slide changes the board.
    pub fn can_move(&self) -> bool {
        FaceDirection::ALL
            .iter()
            .any(|&direction| self.slide(direction).moved)
    }

    /// Puts a new tile in an empty cell chosen from `seed` and `step`: a 2 nine times in ten, else
    /// a 4. The same seed and step on the same board always choose the same cell and tile. Returns
    /// the cell, or `None` if the board is full.
    pub fn spawn(&mut self, seed: u64, step: u64) -> Option<usize> {
        let empty = self.empty_cells();
        if empty.is_empty() {
            return None;
        }
        let hash = mix(seed ^ mix(step));
        let cell = empty[(hash % empty.len() as u64) as usize];
        let exponent = if (hash >> 32).is_multiple_of(10) {
            2
        } else {
            1
        };
        self.cells[cell] = exponent;
        Some(cell)
    }
}

/// Slides one line toward its front (index 0), returning the new line and the points scored.
fn slide_line(line: &[u8]) -> (Vec<u8>, u64) {
    let tiles: Vec<u8> = line.iter().copied().filter(|&tile| tile != 0).collect();
    let mut result = Vec::with_capacity(line.len());
    let mut score = 0u64;
    let mut next = 0;
    while next < tiles.len() {
        let tile = tiles[next];
        let merges = tiles.get(next + 1) == Some(&tile) && tile < MAXIMUM_EXPONENT;
        if merges {
            result.push(tile + 1);
            score += 1u64 << (tile + 1);
            next += 2;
        } else {
            result.push(tile);
            next += 1;
        }
    }
    result.resize(line.len(), 0);
    (result, score)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn line(cells: [u8; 4]) -> Vec<u8> {
        slide_line(&cells).0
    }

    #[test]
    fn a_line_slides_to_its_front_and_merges_equal_neighbours_once() {
        assert_eq!(line([0, 0, 0, 0]), [0, 0, 0, 0]);
        assert_eq!(line([0, 1, 0, 2]), [1, 2, 0, 0]);
        assert_eq!(line([1, 1, 0, 0]), [2, 0, 0, 0]);
        assert_eq!(line([1, 0, 0, 1]), [2, 0, 0, 0]);
        assert_eq!(
            line([1, 1, 1, 1]),
            [2, 2, 0, 0],
            "two merges, not one chain"
        );
        assert_eq!(
            line([1, 1, 1, 0]),
            [2, 1, 0, 0],
            "the tile nearest the front merges"
        );
        assert_eq!(
            line([2, 1, 1, 0]),
            [2, 2, 0, 0],
            "a merged tile does not merge again"
        );
        assert_eq!(
            line([1, 2, 1, 2]),
            [1, 2, 1, 2],
            "unequal tiles do not merge"
        );
    }

    #[test]
    fn merging_scores_the_value_of_the_new_tile() {
        assert_eq!(slide_line(&[1, 1, 0, 0]).1, 4);
        assert_eq!(slide_line(&[3, 3, 2, 2]).1, 16 + 8);
        assert_eq!(slide_line(&[1, 2, 3, 4]).1, 0);
    }

    #[test]
    fn the_largest_tile_does_not_merge_so_values_stay_in_range() {
        assert_eq!(
            line([MAXIMUM_EXPONENT, MAXIMUM_EXPONENT, 0, 0]),
            [MAXIMUM_EXPONENT, MAXIMUM_EXPONENT, 0, 0]
        );
        assert_eq!(
            line([MAXIMUM_EXPONENT - 1, MAXIMUM_EXPONENT - 1, 0, 0])[0],
            MAXIMUM_EXPONENT
        );
    }

    #[test]
    fn cell_indices_and_coordinates_agree() {
        for index in 0..CELL_COUNT {
            let [x, y, z] = cell_coordinates(index);
            assert_eq!(cell_index(x, y, z), index);
        }
        assert_eq!(cell_index(1, 2, 3), 1 + 4 * (2 + 4 * 3));
    }

    fn board_with(tiles: &[([usize; 3], u8)]) -> Board {
        let mut board = Board::empty();
        for &([x, y, z], exponent) in tiles {
            board.set(cell_index(x, y, z), exponent);
        }
        board
    }

    #[test]
    fn a_slide_moves_every_line_in_the_chosen_direction() {
        let board = board_with(&[([0, 0, 0], 1), ([1, 0, 0], 1), ([0, 3, 2], 2)]);
        let right = board.slide(FaceDirection::PositiveX);
        assert_eq!(right.board, board_with(&[([3, 0, 0], 2), ([3, 3, 2], 2)]));
        assert_eq!(right.score, 4);
        assert!(right.moved);

        let up = board.slide(FaceDirection::PositiveY);
        assert_eq!(
            up.board,
            board_with(&[([0, 3, 0], 1), ([1, 3, 0], 1), ([0, 3, 2], 2)])
        );
        assert_eq!(up.score, 0);

        let backwards = board.slide(FaceDirection::NegativeZ);
        assert_eq!(
            backwards.board,
            board_with(&[([0, 0, 0], 1), ([1, 0, 0], 1), ([0, 3, 0], 2)])
        );
    }

    #[test]
    fn a_slide_that_changes_nothing_is_not_a_move() {
        let packed = board_with(&[([0, 0, 0], 1), ([0, 1, 0], 2)]);
        let slide = packed.slide(FaceDirection::NegativeX);
        assert!(!slide.moved);
        assert_eq!(slide.board, packed);
        assert!(!Board::empty().slide(FaceDirection::PositiveZ).moved);
    }

    #[test]
    fn a_full_board_with_no_equal_neighbours_cannot_move() {
        // Exponents chosen so that no two tiles that share a face are equal: a three-colouring of
        // the cube lattice by (x + y + z) mod 3.
        let mut cells = [0u8; CELL_COUNT];
        for (index, cell) in cells.iter_mut().enumerate() {
            let [x, y, z] = cell_coordinates(index);
            *cell = ((x + y + z) % 3) as u8 + 1;
        }
        let stuck = Board::from_cells(cells);
        assert!(stuck.empty_cells().is_empty());
        assert!(!stuck.can_move());
        // Make one pair equal and it can.
        let mut free = stuck.clone();
        free.set(cell_index(0, 0, 0), free.get(cell_index(1, 0, 0)));
        assert!(free.can_move());
        // A board with an empty cell can move, but only toward the side the gap is not already on:
        // with the corner empty, tiles can slide into it from one side and not from the other.
        let mut corner = stuck;
        corner.set(cell_index(0, 0, 0), 0);
        assert!(corner.can_move());
        assert!(corner.slide(FaceDirection::NegativeX).moved);
        assert!(!corner.slide(FaceDirection::PositiveX).moved);
        assert!(!Board::empty().can_move());
    }

    #[test]
    fn spawning_is_deterministic_picks_an_empty_cell_and_fills_a_board_up() {
        let mut first = Board::empty();
        let mut second = Board::empty();
        assert_eq!(first.spawn(7, 0), second.spawn(7, 0));
        assert_eq!(first, second);
        assert_eq!(first.tile_count(), 1);
        assert!(first.highest() == 1 || first.highest() == 2);

        let mut board = Board::empty();
        for step in 0..CELL_COUNT as u64 {
            assert!(board.spawn(99, step).is_some());
        }
        assert_eq!(board.tile_count(), CELL_COUNT);
        assert_eq!(board.spawn(99, 1000), None, "a full board has no room");
    }

    #[test]
    fn spawned_tiles_are_mostly_twos_and_sometimes_fours_and_land_everywhere() {
        let mut fours = 0;
        let mut cells = std::collections::BTreeSet::new();
        for step in 0..2000 {
            let mut board = Board::empty();
            let cell = board.spawn(1, step).unwrap();
            cells.insert(cell);
            fours += usize::from(board.get(cell) == 2);
        }
        assert!((100..300).contains(&fours), "{fours} fours in 2000");
        assert_eq!(cells.len(), CELL_COUNT);
    }

    fn arbitrary_board() -> impl Strategy<Value = Board> {
        proptest::collection::vec(0u8..6, CELL_COUNT)
            .prop_map(|cells| Board::from_cells(cells.try_into().unwrap()))
    }

    fn direction() -> impl Strategy<Value = FaceDirection> {
        (0u8..6).prop_map(|index| FaceDirection::from_index(index).unwrap())
    }

    /// The board seen in a mirror across the axis of `direction`.
    fn mirrored(board: &Board, axis: usize) -> Board {
        let mut result = Board::empty();
        for index in 0..CELL_COUNT {
            let mut at = cell_coordinates(index);
            at[axis] = SIZE - 1 - at[axis];
            result.set(cell_index(at[0], at[1], at[2]), board.get(index));
        }
        result
    }

    proptest! {
        #[test]
        fn sliding_keeps_all_the_value_except_what_merging_reports(
            board in arbitrary_board(),
            toward in direction(),
        ) {
            let slide = board.slide(toward);
            // Merging two tiles of value v makes one of 2v: the total is unchanged.
            prop_assert_eq!(slide.board.total_value(), board.total_value());
            prop_assert!(slide.board.tile_count() <= board.tile_count());
            prop_assert_eq!(slide.moved, slide.board != board);
            prop_assert!(slide.score <= board.total_value());
            // Fewer tiles exactly when something merged, and then something scored.
            prop_assert_eq!(slide.board.tile_count() < board.tile_count(), slide.score > 0);
        }

        #[test]
        fn sliding_one_way_is_the_mirror_image_of_sliding_the_other(
            board in arbitrary_board(),
            toward in direction(),
        ) {
            let axis = toward.axis();
            let ours = board.slide(toward);
            let theirs = mirrored(&board, axis).slide(toward.opposite());
            prop_assert_eq!(mirrored(&theirs.board, axis), ours.board);
            prop_assert_eq!(theirs.score, ours.score);
        }

        #[test]
        fn after_sliding_toward_a_side_nothing_can_move_further_toward_it_without_merging(
            board in arbitrary_board(),
            toward in direction(),
        ) {
            // A second slide in the same direction changes only by merging tiles that the first slide
            // brought together: it never moves a tile into empty space.
            let once = board.slide(toward).board;
            let twice = once.slide(toward);
            prop_assert_eq!(twice.board.total_value(), once.total_value());
            if twice.moved {
                prop_assert!(twice.score > 0 || twice.board.tile_count() < once.tile_count());
            }
        }
    }
}
