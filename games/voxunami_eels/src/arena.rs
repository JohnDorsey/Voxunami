//! The arena: a hollow cube of empty space with a wall all round it.
//!
//! The inside is the cells 0 to [`ARENA_EDGE`] - 1 along each axis. The wall is the layer of voxels
//! just outside, at -1 and at [`ARENA_EDGE`]. Panels of the wall are dark, and a grid of lines
//! bright, so that an eel can tell how fast it is going and how near it is to a wall.
//!
//! The wall is terrain: [`ArenaWalls`] makes it, so it takes no room in saves and never changes.
//! The generator `voxunami_eels:arena_v1` is part of every world's file format: its output must
//! never change. A different arena is a new generator with a new name.

use terrain::TerrainGenerator;
use voxel_grid::{Brick, ChannelIndex, ChannelStorage, GridLayout};
use voxel_space::{BRICK_VOXEL_COUNT, BrickCoordinate, LocalVoxelIndex};

use crate::content::blocks;

/// How many cells the arena is wide, tall and deep.
pub const ARENA_EDGE: i64 = 64;

/// How far apart the lines of the wall's grid are.
pub const LINE_SPACING: i64 = 8;

/// A voxel, as its three coordinates.
pub type Cell = [i64; 3];

/// Whether a cell is inside the arena (not in its wall, and not outside it).
pub fn is_inside(cell: Cell) -> bool {
    cell.iter()
        .all(|coordinate| (0..ARENA_EDGE).contains(coordinate))
}

/// What the wall is made of at a voxel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wall {
    Panel,
    Line,
}

/// The wall at a voxel, if it is part of the wall.
pub fn wall_at(cell: Cell) -> Option<Wall> {
    if cell
        .iter()
        .any(|coordinate| !(-1..=ARENA_EDGE).contains(coordinate))
    {
        return None;
    }
    let on_face = |coordinate: i64| coordinate == -1 || coordinate == ARENA_EDGE;
    let faces = cell
        .iter()
        .filter(|&&coordinate| on_face(coordinate))
        .count();
    if faces == 0 {
        return None;
    }
    // A line along every edge of the arena, and where the grid of a face has one.
    let on_a_grid_line = cell
        .iter()
        .any(|&coordinate| !on_face(coordinate) && coordinate.rem_euclid(LINE_SPACING) == 0);
    Some(if faces > 1 || on_a_grid_line {
        Wall::Line
    } else {
        Wall::Panel
    })
}

/// Whether a brick holds any of the wall.
fn brick_touches_wall(brick: BrickCoordinate) -> bool {
    let origin = [brick.x() * 32, brick.y() * 32, brick.z() * 32];
    let overlaps = |start: i64| start <= ARENA_EDGE && start + 31 >= -1;
    let holds_a_face = |start: i64| (start..start + 32).any(|c| c == -1 || c == ARENA_EDGE);
    origin.iter().all(|&start| overlaps(start)) && origin.iter().any(|&start| holds_a_face(start))
}

/// The terrain of the arena: its wall, and nothing else anywhere.
#[derive(Debug)]
pub struct ArenaWalls {
    block_channel: ChannelIndex,
}

impl ArenaWalls {
    pub fn new(block_channel: ChannelIndex) -> Self {
        Self { block_channel }
    }
}

impl TerrainGenerator for ArenaWalls {
    fn generate_brick(&self, layout: &GridLayout, coordinate: BrickCoordinate) -> Brick {
        let mut brick = Brick::new(layout);
        if !brick_touches_wall(coordinate) {
            return brick;
        }
        let mut values = vec![blocks::AIR; BRICK_VOXEL_COUNT];
        for local in LocalVoxelIndex::all() {
            let voxel = coordinate.voxel_at(local);
            values[local.index()] = match wall_at([voxel.x(), voxel.y(), voxel.z()]) {
                None => blocks::AIR,
                Some(Wall::Panel) => blocks::WALL,
                Some(Wall::Line) => blocks::WALL_LINE,
            };
        }
        *brick.channel_mut(self.block_channel) = ChannelStorage::from_values(&values);
        brick
    }
}

#[cfg(test)]
mod tests {
    use voxel_grid::test_support::StandardTestLayout;

    use super::*;

    fn generate(brick: BrickCoordinate) -> Brick {
        let fixture = StandardTestLayout::default();
        ArenaWalls::new(fixture.material).generate_brick(&fixture.layout, brick)
    }

    #[test]
    fn the_wall_is_the_layer_just_outside_the_arena_and_nothing_else() {
        let last = ARENA_EDGE;
        for outside in [
            [-1, 5, 5],
            [last, 5, 5],
            [5, -1, 5],
            [5, last, 5],
            [5, 5, -1],
            [5, 5, last],
            [-1, -1, -1],
            [last, last, last],
        ] {
            assert!(wall_at(outside).is_some(), "{outside:?}");
            assert!(!is_inside(outside), "{outside:?}");
        }
        for not_wall in [
            [0, 0, 0],
            [5, 5, 5],
            [last - 1, last - 1, last - 1],
            [-2, 5, 5],
            [last + 1, 5, 5],
        ] {
            assert_eq!(wall_at(not_wall), None, "{not_wall:?}");
        }
        assert!(is_inside([0, 0, 0]) && is_inside([last - 1, last - 1, last - 1]));
        assert!(!is_inside([last, 0, 0]) && !is_inside([0, -1, 0]));
    }

    #[test]
    fn lines_run_along_the_edges_and_across_the_faces_at_a_spacing() {
        assert_eq!(wall_at([-1, -1, 10]), Some(Wall::Line), "an edge");
        assert_eq!(
            wall_at([ARENA_EDGE, 3, ARENA_EDGE]),
            Some(Wall::Line),
            "an edge"
        );
        assert_eq!(wall_at([-1, 8, 3]), Some(Wall::Line), "a grid line");
        assert_eq!(wall_at([-1, 3, 16]), Some(Wall::Line), "a grid line");
        assert_eq!(wall_at([-1, 3, 5]), Some(Wall::Panel));
        assert_eq!(wall_at([20, ARENA_EDGE, 5]), Some(Wall::Panel));
        assert_eq!(wall_at([24, ARENA_EDGE, 5]), Some(Wall::Line));
    }

    #[test]
    fn bricks_away_from_the_wall_are_empty_and_the_rest_hold_exactly_the_wall() {
        let channel = StandardTestLayout::default().material;
        // Inside the arena, and far from it.
        for far in [
            BrickCoordinate::new(0, 0, 0),
            BrickCoordinate::new(1, 1, 1),
            BrickCoordinate::new(5, 0, 0),
            BrickCoordinate::new(0, -4, 0),
        ] {
            let brick = generate(far);
            assert!(
                LocalVoxelIndex::all().all(|local| brick.get(channel, local) == blocks::AIR),
                "{far:?}"
            );
        }
        // Around the arena: every voxel is the wall if it is, and air if not.
        for x in -1..=2 {
            for y in -1..=2 {
                for z in -1..=2 {
                    let coordinate = BrickCoordinate::new(x, y, z);
                    let brick = generate(coordinate);
                    for local in LocalVoxelIndex::all() {
                        let voxel = coordinate.voxel_at(local);
                        let expected = match wall_at([voxel.x(), voxel.y(), voxel.z()]) {
                            None => blocks::AIR,
                            Some(Wall::Panel) => blocks::WALL,
                            Some(Wall::Line) => blocks::WALL_LINE,
                        };
                        assert_eq!(brick.get(channel, local), expected, "{voxel}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_arena_generator_is_pinned() {
        // A released generator version must never change its output. These bricks are its
        // fingerprint: a corner of the wall, a flat face, and the inside. If this fails, add a new
        // generator version instead of changing this one.
        let corner = generate(BrickCoordinate::new(-1, -1, -1));
        let face = generate(BrickCoordinate::new(0, -1, 1));
        let inside = generate(BrickCoordinate::new(0, 0, 0));
        assert_eq!(
            (
                corner.content_digest(),
                face.content_digest(),
                inside.content_digest()
            ),
            (
                PINNED_CORNER_DIGEST,
                PINNED_FACE_DIGEST,
                PINNED_INSIDE_DIGEST
            )
        );
    }

    const PINNED_CORNER_DIGEST: u64 = 10_078_360_533_234_443_498;
    const PINNED_FACE_DIGEST: u64 = 8_203_966_454_387_843_218;
    const PINNED_INSIDE_DIGEST: u64 = 12_280_675_352_798_744_816;
}
