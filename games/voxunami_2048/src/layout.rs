//! Where the board is in the world, and what it is built of.
//!
//! The board is a 20-voxel cube whose 64 cells each hold a 4-voxel tile, one voxel apart. A thin
//! frame marks its edges, and six push plates stand a little way off the middle of its faces. All of
//! it fits in the brick at the origin.

use voxel_space::{FaceDirection, VoxelCoordinate};

use crate::board::{CELL_COUNT, SIZE, cell_coordinates};

/// The voxel of the board's lowest corner.
pub const BOARD_MINIMUM: i64 = 4;
/// Voxels from one cell to the next.
pub const CELL_PITCH: i64 = 5;
/// Voxels along a tile.
pub const TILE_EDGE: i64 = 4;
/// Voxels along the whole board, including the last gap.
pub const BOARD_EDGE: i64 = CELL_PITCH * SIZE as i64;

const FRAME_MINIMUM: i64 = BOARD_MINIMUM - 1;
const FRAME_MAXIMUM: i64 = BOARD_MINIMUM + BOARD_EDGE;

/// Plates are this many voxels thick and sit this far from the frame.
const PLATE_THICKNESS: i64 = 2;
const PLATE_GAP: i64 = 1;
/// The plate covers this range of the two axes it faces along.
const PLATE_RANGE: (i64, i64) = (10, 17);

/// The lowest voxel of a cell's tile.
pub fn cell_origin(cell: usize) -> VoxelCoordinate {
    let [x, y, z] = cell_coordinates(cell);
    let at = |along: usize| BOARD_MINIMUM + CELL_PITCH * along as i64;
    VoxelCoordinate::new(at(x), at(y), at(z))
}

/// The 64 voxels of a cell's tile.
pub fn tile_voxels(cell: usize) -> impl Iterator<Item = VoxelCoordinate> {
    let origin = cell_origin(cell);
    (0..TILE_EDGE).flat_map(move |dz| {
        (0..TILE_EDGE).flat_map(move |dy| {
            (0..TILE_EDGE).map(move |dx| {
                VoxelCoordinate::new(origin.x() + dx, origin.y() + dy, origin.z() + dz)
            })
        })
    })
}

/// Every cell, in index order.
pub fn cells() -> impl Iterator<Item = usize> {
    0..CELL_COUNT
}

/// The voxels of the frame: the twelve edges of the cube around the board.
pub fn frame_voxels() -> Vec<VoxelCoordinate> {
    let mut voxels = Vec::new();
    for axis in 0..3 {
        for first in [FRAME_MINIMUM, FRAME_MAXIMUM] {
            for second in [FRAME_MINIMUM, FRAME_MAXIMUM] {
                for along in FRAME_MINIMUM..=FRAME_MAXIMUM {
                    let mut at = [0i64; 3];
                    at[axis] = along;
                    at[(axis + 1) % 3] = first;
                    at[(axis + 2) % 3] = second;
                    voxels.push(VoxelCoordinate::new(at[0], at[1], at[2]));
                }
            }
        }
    }
    voxels.sort_by_key(|voxel| (voxel.x(), voxel.y(), voxel.z()));
    voxels.dedup();
    voxels
}

/// The ranges, per axis, of the plate that pushes tiles toward `toward`. It stands on the opposite
/// side of the board: pushing from the left moves everything right.
fn plate_ranges(toward: FaceDirection) -> [(i64, i64); 3] {
    let axis = toward.axis();
    let mut ranges = [PLATE_RANGE; 3];
    ranges[axis] = if toward.is_positive() {
        let high = FRAME_MINIMUM - PLATE_GAP - 1;
        (high - PLATE_THICKNESS + 1, high)
    } else {
        let low = FRAME_MAXIMUM + PLATE_GAP + 1;
        (low, low + PLATE_THICKNESS - 1)
    };
    ranges
}

/// The voxels of the plate that pushes tiles toward `toward`.
pub fn plate_voxels(toward: FaceDirection) -> Vec<VoxelCoordinate> {
    let [x, y, z] = plate_ranges(toward);
    let mut voxels = Vec::new();
    for dz in z.0..=z.1 {
        for dy in y.0..=y.1 {
            for dx in x.0..=x.1 {
                voxels.push(VoxelCoordinate::new(dx, dy, dz));
            }
        }
    }
    voxels
}

/// The direction tiles are pushed if `voxel` is part of a push plate.
pub fn push_direction_for_plate(voxel: VoxelCoordinate) -> Option<FaceDirection> {
    let at = [voxel.x(), voxel.y(), voxel.z()];
    FaceDirection::ALL.into_iter().find(|&toward| {
        plate_ranges(toward)
            .iter()
            .zip(at)
            .all(|(&(low, high), value)| (low..=high).contains(&value))
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn voxel_set(voxels: impl IntoIterator<Item = VoxelCoordinate>) -> HashSet<VoxelCoordinate> {
        voxels.into_iter().collect()
    }

    #[test]
    fn everything_fits_in_the_brick_at_the_origin_without_overlapping() {
        let mut everything = Vec::new();
        for cell in cells() {
            everything.extend(tile_voxels(cell));
        }
        let tiles = everything.len();
        assert_eq!(tiles, CELL_COUNT * 64);
        let frame = frame_voxels();
        everything.extend(frame.iter().copied());
        let mut plates = 0;
        for toward in FaceDirection::ALL {
            let plate = plate_voxels(toward);
            assert_eq!(plate.len(), 2 * 8 * 8);
            plates += plate.len();
            everything.extend(plate);
        }
        assert_eq!(
            voxel_set(everything.iter().copied()).len(),
            everything.len()
        );
        assert!(everything.iter().all(|voxel| {
            [voxel.x(), voxel.y(), voxel.z()]
                .iter()
                .all(|&component| (0..32).contains(&component))
        }));
        assert_eq!(
            frame.len(),
            12 * 22 - 8 * 2,
            "twelve edges of 22 voxels, corners shared"
        );
        assert_eq!(plates, 6 * 128);
    }

    #[test]
    fn tiles_are_four_voxels_wide_with_a_voxel_between_neighbours() {
        let first = cell_origin(0);
        assert_eq!((first.x(), first.y(), first.z()), (4, 4, 4));
        let next = cell_origin(1);
        assert_eq!(next.x() - first.x(), CELL_PITCH);
        let last: Vec<_> = tile_voxels(CELL_COUNT - 1).collect();
        assert_eq!(
            last.iter().map(|v| v.x()).max(),
            Some(BOARD_MINIMUM + 3 * 5 + 3)
        );
        // Inside the frame with room to spare.
        assert!(last.iter().all(|v| v.x() < FRAME_MAXIMUM));
    }

    #[test]
    fn each_plate_pushes_toward_the_far_side_and_is_found_from_any_of_its_voxels() {
        for toward in FaceDirection::ALL {
            let plate = plate_voxels(toward);
            for voxel in &plate {
                assert_eq!(push_direction_for_plate(*voxel), Some(toward));
            }
            // The plate is on the side opposite to where it pushes.
            let axis = toward.axis();
            let along = |voxel: &VoxelCoordinate| [voxel.x(), voxel.y(), voxel.z()][axis];
            let centre = BOARD_MINIMUM + BOARD_EDGE / 2;
            let on_the_low_side = plate.iter().all(|voxel| along(voxel) < centre);
            assert_eq!(on_the_low_side, toward.is_positive(), "{toward:?}");
        }
    }

    #[test]
    fn tiles_and_the_frame_are_not_plates() {
        for cell in cells() {
            for voxel in tile_voxels(cell) {
                assert_eq!(push_direction_for_plate(voxel), None);
            }
        }
        for voxel in frame_voxels() {
            assert_eq!(push_direction_for_plate(voxel), None);
        }
        assert_eq!(
            push_direction_for_plate(VoxelCoordinate::new(100, 100, 100)),
            None
        );
    }
}
